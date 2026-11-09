//! Startup timeline, printed on stderr when `SIGNET_TRACE_STARTUP=1`.
//!
//! Every mark prints the milliseconds since the process started (the time the OS created it, so dyld and static
//! initializers are counted), the milliseconds since the previous mark and a name:
//! `[startup]   412.3 ms (+12.0) cef.initialize`. When the variable is not set, every function returns after one
//! relaxed atomic load: the tracing costs nothing in normal runs.
//!
//! The browser process exports its start time in `SIGNET_TRACE_T0` (microseconds since the Unix epoch), so the CEF
//! subprocesses (they inherit the environment) print their marks on the same time base, tagged with their type.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// 0 unknown, 1 off, 2 on.
static STATE: AtomicU8 = AtomicU8::new(0);
static CLOCK: Mutex<Option<Clock>> = Mutex::new(None);

struct Clock {
    /// Process start, microseconds since the Unix epoch.
    t0_us: f64,
    /// Last printed mark, same unit.
    last_us: f64,
    /// Names already printed by [`once`].
    seen: Vec<&'static str>,
    /// Prefix: `startup` in the browser process, `startup:<type>` in subprocesses.
    tag: String,
}

/// True when `SIGNET_TRACE_STARTUP` is set to a non empty value other than `0`.
#[inline]
pub fn enabled() -> bool {
    match STATE.load(Ordering::Relaxed) {
        1 => false,
        2 => true,
        _ => init(),
    }
}

#[cold]
fn init() -> bool {
    let on = std::env::var_os("SIGNET_TRACE_STARTUP").is_some_and(|v| !v.is_empty() && v != "0");
    STATE.store(if on { 2 } else { 1 }, Ordering::Relaxed);
    on
}

fn now_us() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64() * 1e6).unwrap_or(0.0)
}

fn with_clock<R>(f: impl FnOnce(&mut Clock) -> R) -> Option<R> {
    let mut g = CLOCK.lock().ok()?;
    let c = g.get_or_insert_with(|| {
        let inherited = std::env::var("SIGNET_TRACE_T0").ok().and_then(|v| v.parse::<f64>().ok());
        let subprocess = std::env::args().find_map(|a| a.strip_prefix("--type=").map(str::to_string));
        let (t0_us, tag) = match (inherited, subprocess) {
            (Some(t0), Some(kind)) => (t0, format!("startup:{kind}")),
            _ => {
                let t0 = process_start_us().unwrap_or_else(now_us);
                // SAFETY: called from the first mark of the browser process, on the main thread, before CEF starts any
                // thread that reads the environment (the first mark is in `main`).
                unsafe { std::env::set_var("SIGNET_TRACE_T0", format!("{t0:.0}")) };
                (t0, "startup".to_string())
            }
        };
        Clock { t0_us, last_us: t0_us, seen: Vec::new(), tag }
    });
    Some(f(c))
}

fn print(c: &mut Clock, name: &str) {
    use std::io::Write as _;
    let now = now_us();
    // One write per line: the subprocesses share stderr with the browser process.
    let line = format!("[{}] {:>8.1} ms (+{:.1}) {name}\n", c.tag, (now - c.t0_us) / 1000.0, (now - c.last_us) / 1000.0);
    let _ = std::io::stderr().lock().write_all(line.as_bytes());
    c.last_us = now;
}

/// Print a mark.
#[inline]
pub fn mark(name: &str) {
    if enabled() {
        with_clock(|c| print(c, name));
    }
}

/// Print a mark whose text is only built when tracing is on.
#[inline]
pub fn mark_with(name: impl FnOnce() -> String) {
    if enabled() {
        let text = name();
        with_clock(|c| print(c, &text));
    }
}

/// Print a mark the first time `name` is seen (first browser created, first load...).
#[inline]
pub fn once(name: &'static str) {
    if enabled() {
        with_clock(|c| {
            if !c.seen.contains(&name) {
                c.seen.push(name);
                print(c, name);
            }
        });
    }
}

/// Same as [`once`] with details appended to the first line only.
#[inline]
pub fn once_with(name: &'static str, details: impl FnOnce() -> String) {
    if enabled() {
        with_clock(|c| {
            if !c.seen.contains(&name) {
                c.seen.push(name);
                let text = format!("{name} {}", details());
                print(c, &text);
            }
        });
    }
}

/// Times a block: prints `<name> took N ms` when dropped. `None` when tracing is off.
pub struct Span {
    name: &'static str,
    start: Instant,
}

/// Start a [`Span`]. Keep the result alive for the duration to measure: `let _s = startup::span("store.open");`.
#[inline]
pub fn span(name: &'static str) -> Option<Span> {
    enabled().then(|| Span { name, start: Instant::now() })
}

impl Drop for Span {
    fn drop(&mut self) {
        let ms = self.start.elapsed().as_secs_f64() * 1000.0;
        let name = self.name;
        with_clock(|c| print(c, &format!("{name} done ({ms:.1} ms)")));
    }
}

/// Process creation time in microseconds since the Unix epoch, as recorded by the OS.
#[cfg(target_os = "macos")]
fn process_start_us() -> Option<f64> {
    // SAFETY: plain syscall filling a struct of the size we pass.
    unsafe {
        let mut info: libc::proc_bsdinfo = std::mem::zeroed();
        let size = std::mem::size_of::<libc::proc_bsdinfo>() as i32;
        let n = libc::proc_pidinfo(std::process::id() as i32, libc::PROC_PIDTBSDINFO, 0, (&raw mut info).cast(), size);
        (n == size).then(|| info.pbi_start_tvsec as f64 * 1e6 + info.pbi_start_tvusec as f64)
    }
}

#[cfg(target_os = "linux")]
fn process_start_us() -> Option<f64> {
    // Field 22 of /proc/self/stat: start time in clock ticks after boot; /proc/uptime: seconds since boot.
    let stat = std::fs::read_to_string("/proc/self/stat").ok()?;
    let after = stat.rsplit_once(')')?.1;
    let ticks: f64 = after.split_whitespace().nth(19)?.parse().ok()?;
    let uptime: f64 = std::fs::read_to_string("/proc/uptime").ok()?.split_whitespace().next()?.parse().ok()?;
    // SAFETY: sysconf has no preconditions.
    let hz = unsafe { libc::sysconf(libc::_SC_CLK_TCK) } as f64;
    if hz <= 0.0 {
        return None;
    }
    let age_s = uptime - ticks / hz;
    Some(now_us() - age_s * 1e6)
}

#[cfg(target_os = "windows")]
fn process_start_us() -> Option<f64> {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
    let zero = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
    let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
    // SAFETY: the pseudo handle of the current process is always valid; the out pointers are live locals.
    let ok = unsafe { GetProcessTimes(GetCurrentProcess(), &mut created, &mut exited, &mut kernel, &mut user) };
    if ok == 0 {
        return None;
    }
    // FILETIME: 100 ns intervals since 1601-01-01.
    let t = ((created.dwHighDateTime as u64) << 32) | created.dwLowDateTime as u64;
    Some((t as f64 - 116_444_736_000_000_000.0) / 10.0)
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn process_start_us() -> Option<f64> {
    None
}

//! Timing log of the floating surfaces on stderr when `SIGNET_TRACE_FLOAT=1` (otherwise one cached check).
use std::sync::OnceLock;
use std::time::Instant;

pub fn on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("SIGNET_TRACE_FLOAT").is_some_and(|v| !v.is_empty() && v != "0"))
}

/// Prints `[float <ms since first mark>] <msg>`; `msg` is only built when tracing.
pub fn mark(msg: impl FnOnce() -> String) {
    if on() {
        static EPOCH: OnceLock<Instant> = OnceLock::new();
        let t0 = *EPOCH.get_or_init(Instant::now);
        eprintln!("[float {:>9.1}] {}", t0.elapsed().as_secs_f64() * 1000.0, msg());
    }
}

/// Logs its lifetime when dropped.
pub struct Span(&'static str, Instant);

impl Drop for Span {
    fn drop(&mut self) {
        let ms = self.1.elapsed().as_secs_f64() * 1000.0;
        mark(|| format!("{} {ms:.2} ms", self.0));
    }
}

pub fn span(name: &'static str) -> Option<Span> {
    on().then(|| Span(name, Instant::now()))
}

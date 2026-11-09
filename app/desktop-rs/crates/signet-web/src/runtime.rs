//! Process-wide state of the web layer (main thread only) and the CEF lifecycle.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use cef::{sys, *};

use crate::app::SignetApp;
use crate::types::*;
use crate::types::KeyEvent;

/// User agent used when the app does not provide one (Chrome-like, no "Electron" token).
pub fn default_user_agent() -> String {
    let (os, extra) = if cfg!(target_os = "macos") {
        ("Macintosh; Intel Mac OS X 10_15_7", "")
    } else if cfg!(target_os = "windows") {
        ("Windows NT 10.0; Win64; x64", "")
    } else {
        ("X11; Linux x86_64", "")
    };
    format!(
        "Mozilla/5.0 ({os}) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/156.0.0.0 Safari/537.36{extra}"
    )
}

/// Startup configuration.
#[derive(Clone, Debug)]
pub struct Config {
    /// App data folder; CEF stores its profile under `<data_dir>/cef`.
    pub data_dir: PathBuf,
    /// Locale such as `en-US`.
    pub locale: String,
    /// `Accept-Language` list, e.g. `en-US,en`.
    pub accept_language: String,
    /// User agent, `None` for [`default_user_agent`].
    pub user_agent: Option<String>,
    /// Helper executable. `None`: derived from the bundle layout (macOS), or the current exe.
    pub helper_path: Option<PathBuf>,
    /// Disable the Chromium sandbox. Default `true` (see README: the macOS sandbox needs the
    /// `cef_sandbox` static library linked into every helper, which cef-rs does not provide yet).
    pub no_sandbox: bool,
    /// Remote debugging port (0 = off). The built-in developer tools need it: the frontend is served on this port
    /// (loopback only) and connects to the page's target. Use [`pick_loopback_port`] for a random one.
    pub remote_debugging_port: i32,
    /// Chromium log severity: 0 default, 1 verbose, 2 info, 3 warning, 4 error, 5 fatal, 99 disable.
    pub log_severity: i32,
}

impl Config {
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
            locale: "en-US".into(),
            accept_language: "en-US,en".into(),
            user_agent: None,
            helper_path: None,
            no_sandbox: true,
            remote_debugging_port: 0,
            log_severity: 3,
        }
    }
}

pub(crate) struct ViewEntry {
    pub browser: Option<Browser>,
    /// Raw parent handle (NSView*, HWND, X11 window id) as an integer.
    pub parent: usize,
    pub profile: String,
    pub visible: bool,
    pub bounds: Bounds,
    pub closing: bool,
    /// macOS: the native view keeps a fixed size and top-left position when the window resizes (parked views).
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub pinned: bool,
    #[allow(dead_code)]
    pub devtools_for: Option<ViewId>,
}

pub(crate) struct State {
    pub config: Config,
    pub next_view: u64,
    pub views: HashMap<ViewId, ViewEntry>,
    pub contexts: HashMap<String, RequestContext>,
    pub scripts: HashMap<String, Vec<PageScript>>,
    pub events: VecDeque<WebEvent>,
    pub key_filter: Option<Box<dyn Fn(&KeyEvent) -> bool>>,
    pub key_observer: Option<Box<dyn Fn(&KeyEvent, bool) -> bool>>,
    pub swallowed_keys: Vec<i32>,
    pub shutting_down: bool,
    pub scheme_handler: Option<Arc<SchemeHandlerFn>>,
    pub downloads: HashMap<u32, DownloadItemCallback>,
    pub certificate_handler: Option<Box<CertificateHandlerFn>>,
    pub audio: HashMap<ViewId, crate::media::AudioState>,
    pub cdp: HashMap<ViewId, crate::cdp::CdpView>,
    pub cdp_ready: Vec<(crate::cdp::Done, Result<String, String>)>,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

pub(crate) static CONTEXT_READY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static MAIN_THREAD: OnceLock<std::thread::ThreadId> = OnceLock::new();
type WakeFn = dyn Fn(i64) + Send + Sync;
static WAKE: Mutex<Option<Arc<WakeFn>>> = Mutex::new(None);

static DEVTOOLS_PORT: std::sync::atomic::AtomicU16 = std::sync::atomic::AtomicU16::new(0);

/// Port of the remote debugging server (loopback), `None` when it is off.
pub fn devtools_port() -> Option<u16> {
    match DEVTOOLS_PORT.load(std::sync::atomic::Ordering::Relaxed) {
        0 => None,
        p => Some(p),
    }
}

/// A free random port on 127.0.0.1 (bound then released: a small race window remains).
pub fn pick_loopback_port() -> Option<u16> {
    let l = std::net::TcpListener::bind(("127.0.0.1", 0)).ok()?;
    let p = l.local_addr().ok()?.port();
    (p >= 1024).then_some(p)
}

pub(crate) fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    STATE.with(|s| s.try_borrow_mut().ok().and_then(|mut b| b.as_mut().map(f)))
}

pub(crate) fn push_event(ev: WebEvent) {
    with_state(|s| s.events.push_back(ev));
}

/// True once [`init`] succeeded and until [`shutdown`] completes.
pub fn is_initialized() -> bool {
    with_state(|_| ()).is_some()
}

pub fn assert_main_thread() {
    debug_assert_eq!(MAIN_THREAD.get(), Some(&std::thread::current().id()), "signet-web must be used on the main thread");
}

/// Register the function CEF calls (from any thread) when it wants `do_message_loop_work`
/// to run after `delay_ms` (0 or negative: as soon as possible).
pub fn set_wake_handler(f: impl Fn(i64) + Send + Sync + 'static) {
    if let Ok(mut w) = WAKE.lock() {
        *w = Some(Arc::new(f));
    }
}

pub(crate) fn wake(delay_ms: i64) {
    let f = WAKE.lock().ok().and_then(|w| w.clone());
    if let Some(f) = f {
        f(delay_ms);
    }
}

/// Thread started by `preload_framework`, joined by `init`.
#[cfg(target_os = "macos")]
static PRELOAD: std::sync::Mutex<Option<std::thread::JoinHandle<()>>> = std::sync::Mutex::new(None);

/// macOS: starts loading the CEF framework (`dlopen`, no CEF call) on a background thread so that `init` finds it
/// already mapped. Call it as early as possible in `main`. No-op on Linux and Windows.
pub fn preload_framework() {
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::ffi::OsStrExt;
        let Some(dir) = exe_path().parent().map(|p| p.join("../Frameworks/Chromium Embedded Framework.framework/Chromium Embedded Framework")) else { return };
        let Ok(path) = std::ffi::CString::new(dir.as_os_str().as_bytes()) else { return };
        let spawned = std::thread::Builder::new().name("cef-preload".into()).spawn(move || {
            // Same flags as `cef_load_library`; the handle is never closed. A failure is ignored: `init` loads it again.
            // SAFETY: `path` is a valid C string; dlopen is thread safe and CEF runs nothing at load time but static init.
            unsafe { libc::dlopen(path.as_ptr(), libc::RTLD_LAZY | libc::RTLD_LOCAL | libc::RTLD_FIRST) };
        });
        if let (Ok(h), Ok(mut g)) = (spawned, PRELOAD.lock()) {
            *g = Some(h);
        }
    }
}

/// Run one iteration of the CEF message loop. Call on the main thread.
pub fn pump() {
    if is_initialized() {
        do_message_loop_work();
    }
}

/// Take all queued events (call after [`pump`]).
pub fn drain_events() -> Vec<WebEvent> {
    crate::cdp::run_ready();
    let mut out: Vec<WebEvent> = with_state(|s| s.events.drain(..).collect()).unwrap_or_default();
    out.extend(crate::hooks::take_io_events());
    for ev in &mut out {
        crate::extension::remap_event(ev);
    }
    out
}

/// Decide what happens on certificate errors. Without a handler the load is cancelled and
/// [`WebEvent::CertificateError`] is emitted. The handler runs on the main thread inside the CEF callback.
pub fn set_certificate_handler(f: impl Fn(&CertificateErrorInfo) -> CertificateDecision + 'static) {
    with_state(|s| s.certificate_handler = Some(Box::new(f)));
}

/// Install a synchronous filter run for every key event before the page sees it. Return `true` to
/// swallow the event (app shortcuts). It runs inside CEF callbacks: keep it fast and do not call
/// back into signet-web from it. The event is also queued as [`WebEvent::PreKey`].
pub fn set_key_filter(f: impl Fn(&KeyEvent) -> bool + 'static) {
    with_state(|s| s.key_filter = Some(Box::new(f)));
}

/// Install an observer run before the key filter for every key down (repeats included) and key up (`true` as second
/// argument) the page receives. Returning `true` for a key down swallows it (and its release). Same rules as the key
/// filter: it runs inside CEF callbacks, keep it fast and never call back into signet-web.
pub fn set_key_observer(f: impl Fn(&KeyEvent, bool) -> bool + 'static) {
    with_state(|s| s.key_observer = Some(Box::new(f)));
}

/// Set the handler of the `signet` / `signetui` custom schemes. May be called before or after [`init`].
pub fn set_scheme_handler(f: impl Fn(&str) -> Option<SchemeResponse> + Send + Sync + 'static) {
    let f: Arc<SchemeHandlerFn> = Arc::new(f);
    with_state(|s| s.scheme_handler = Some(f.clone()));
    crate::scheme::set_global_handler(f);
}

/// Register page scripts for a profile. Applies to views created afterwards.
pub fn set_page_scripts(profile: &ProfileKind, scripts: Vec<PageScript>) {
    let key = profile.key();
    with_state(|s| {
        s.scripts.insert(key, scripts);
    });
}

fn exe_path() -> PathBuf {
    std::env::current_exe().unwrap_or_default()
}

/// Helper executable path derived from the layout of the running binary.
fn default_helper_path() -> PathBuf {
    let exe = exe_path();
    #[cfg(target_os = "macos")]
    {
        // Signet.app/Contents/MacOS/signet -> Signet.app/Contents/Frameworks/<name> Helper.app/Contents/MacOS/<name> Helper
        let name = exe.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let display = bundle_display_name().unwrap_or(name);
        let helper = format!("{display} Helper");
        if let Some(contents) = exe.parent().and_then(|p| p.parent()) {
            return contents
                .join("Frameworks")
                .join(format!("{helper}.app"))
                .join("Contents/MacOS")
                .join(helper);
        }
    }
    // Linux and Windows: the helper sits next to the app (see the bundle scripts). Without it, the app re-runs itself.
    #[cfg(not(target_os = "macos"))]
    {
        let name = if cfg!(windows) { "signet-helper.exe" } else { "signet-helper" };
        if let Some(helper) = exe.parent().map(|d| d.join(name)).filter(|p| p.exists()) {
            return helper;
        }
    }
    exe
}

#[cfg(target_os = "macos")]
fn bundle_display_name() -> Option<String> {
    // The main app bundle folder name ("Signet.app") gives the helper prefix.
    let exe = exe_path();
    exe.parent()?.parent()?.parent()?.file_stem().map(|s| s.to_string_lossy().to_string())
}

/// Load the CEF library (macOS), run the subprocess entry if this process is a CEF subprocess
/// (then this function never returns) and initialize CEF in the browser process.
///
/// Must be called on the main thread, after the GPUI application exists on macOS (the NSApplication
/// instance must be created so its class can be patched), and before any view is created.
pub fn init(config: Config) -> Result<(), Error> {
    if is_initialized() {
        return Err(Error::AlreadyInitialized);
    }
    let _ = MAIN_THREAD.set(std::thread::current().id());
    std::fs::create_dir_all(config.data_dir.join("cef")).map_err(|e| Error::Init(e.to_string()))?;
    // CEF compares cache paths with the root cache path literally: resolve symlinks (/var -> /private/var).
    let mut config = config;
    if let Ok(real) = std::fs::canonicalize(&config.data_dir) {
        config.data_dir = real;
    }

    #[cfg(target_os = "macos")]
    {
        crate::mac::install_cef_app_protocols();
        crate::mac::trace_views_start();
        let loader = cef::library_loader::LibraryLoader::new(&exe_path(), false);
        let load_span = crate::startup::span("cef.library_load");
        // Wait for `preload_framework` so that the two loads never overlap.
        if let Some(h) = PRELOAD.lock().ok().and_then(|mut g| g.take()) {
            let _ = h.join();
        }
        let loaded = loader.load();
        drop(load_span);
        if !loaded {
            return Err(Error::LibraryLoad("Contents/Frameworks/Chromium Embedded Framework.framework".into()));
        }
        // Keep the framework loaded until the process exits.
        std::mem::forget(loader);
    }
    let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);

    let args = cef::args::Args::new();
    let mut app = SignetApp::build(false);
    let exec_span = crate::startup::span("cef.execute_process");
    let ret = execute_process(Some(args.as_main_args()), Some(&mut app), std::ptr::null_mut());
    drop(exec_span);
    if ret >= 0 {
        // This process was a CEF subprocess (non-macOS builds that re-run the main binary).
        std::process::exit(ret);
    }
    // Browser process: the bundle server must exist before CEF starts renderers (env inheritance).
    crate::bundle::start_server();

    if (1024..=65535).contains(&config.remote_debugging_port) {
        DEVTOOLS_PORT.store(config.remote_debugging_port as u16, std::sync::atomic::Ordering::Relaxed);
    }
    let helper = config.helper_path.clone().unwrap_or_else(default_helper_path);
    let root_cache = config.data_dir.join("cef");
    let ua = config.user_agent.clone().unwrap_or_else(default_user_agent);
    let settings = Settings {
        size: std::mem::size_of::<Settings>(),
        no_sandbox: config.no_sandbox as i32,
        browser_subprocess_path: CefString::from(helper.to_string_lossy().as_ref()),
        external_message_pump: 1,
        root_cache_path: CefString::from(root_cache.to_string_lossy().as_ref()),
        cache_path: CefString::from(root_cache.join("default").to_string_lossy().as_ref()),
        persist_session_cookies: 1,
        user_agent: CefString::from(ua.as_str()),
        locale: CefString::from(config.locale.as_str()),
        accept_language_list: CefString::from(config.accept_language.as_str()),
        remote_debugging_port: config.remote_debugging_port,
        log_severity: LogSeverity::from(match config.log_severity {
            1 => sys::cef_log_severity_t::LOGSEVERITY_VERBOSE,
            2 => sys::cef_log_severity_t::LOGSEVERITY_INFO,
            4 => sys::cef_log_severity_t::LOGSEVERITY_ERROR,
            5 => sys::cef_log_severity_t::LOGSEVERITY_FATAL,
            99 => sys::cef_log_severity_t::LOGSEVERITY_DISABLE,
            _ => sys::cef_log_severity_t::LOGSEVERITY_WARNING,
        }),
        ..Default::default()
    };

    STATE.with(|s| {
        *s.borrow_mut() = Some(State {
            config: config.clone(),
            next_view: 1,
            views: HashMap::new(),
            contexts: HashMap::new(),
            scripts: HashMap::new(),
            events: VecDeque::new(),
            key_filter: None,
            key_observer: None,
            swallowed_keys: Vec::new(),
            shutting_down: false,
            scheme_handler: crate::scheme::global_handler(),
            downloads: HashMap::new(),
            certificate_handler: None,
            audio: HashMap::new(),
            cdp: HashMap::new(),
            cdp_ready: Vec::new(),
        });
    });

    let mut app = SignetApp::build(false);
    let init_span = crate::startup::span("cef.initialize");
    let ok = initialize(Some(args.as_main_args()), Some(&settings), Some(&mut app), std::ptr::null_mut());
    drop(init_span);
    if ok != 1 {
        STATE.with(|s| *s.borrow_mut() = None);
        return Err(Error::Init("cef::initialize returned 0".into()));
    }
    // Keep the App alive: CEF holds its own reference, but dropping ours is fine; leak nothing.
    crate::scheme::register_factories_global();
    #[cfg(target_os = "macos")]
    crate::mac::install_page_view_fill();
    // Browsers can only be created once the browser context is initialized (OnContextInitialized).
    let deadline = Instant::now() + Duration::from_secs(15);
    let ctx_span = crate::startup::span("cef.wait_context_initialized");
    let mut spins = 0u32;
    while !CONTEXT_READY.load(std::sync::atomic::Ordering::SeqCst) && Instant::now() < deadline {
        do_message_loop_work();
        std::thread::sleep(Duration::from_millis(5));
        spins += 1;
    }
    drop(ctx_span);
    crate::startup::mark_with(|| format!("cef.context_ready after {spins} pump+sleep(5 ms) rounds"));
    if !CONTEXT_READY.load(std::sync::atomic::Ordering::SeqCst) {
        return Err(Error::Init("browser context was not initialized in time".into()));
    }
    Ok(())
}

/// Close every browser, pump until they are gone (bounded), then shut CEF down.
/// Safe to call twice. Blocks the main thread for at most ~3 seconds.
pub fn shutdown() {
    if !is_initialized() {
        return;
    }
    let t0 = Instant::now();
    let dbg = std::env::var_os("SIGNET_DEBUG").is_some();
    let browsers: Vec<Browser> = with_state(|s| {
        s.shutting_down = true;
        s.views.values_mut().filter_map(|v| { v.closing = true; v.browser.clone() }).collect()
    })
    .unwrap_or_default();
    for b in browsers {
        if let Some(h) = b.host() {
            h.close_browser(1);
        }
    }
    if dbg {
        println!("[quit] close_browser issued: {} ms", t0.elapsed().as_millis());
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        do_message_loop_work();
        let remaining = with_state(|s| s.views.len()).unwrap_or(0);
        if dbg && (remaining == 0 || t0.elapsed().as_millis() < 30) {
            let left = with_state(|s| s.views.iter().map(|(k, v)| format!("{k:?}:browser={}:closing={}", v.browser.is_some(), v.closing)).collect::<Vec<_>>()).unwrap_or_default();
            println!("[quit] pump: {remaining} views left at {} ms {left:?}", t0.elapsed().as_millis());
        }
        if remaining == 0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    // Let pending tasks (cache flush, subprocess exit) run before tearing down.
    for _ in 0..20 {
        do_message_loop_work();
        std::thread::sleep(Duration::from_millis(5));
    }
    crate::profile::clear_pending();
    with_state(|s| {
        s.contexts.clear();
        s.views.clear();
    });
    if dbg {
        let left = with_state(|s| s.views.iter().map(|(k, v)| format!("{k:?}:browser={}:closing={}", v.browser.is_some(), v.closing)).collect::<Vec<_>>()).unwrap_or_default();
        println!("[quit] remaining views {left:?}");
        println!("[quit] cef browsers closed: {} ms", t0.elapsed().as_millis());
    }
    cef::shutdown();
    STATE.with(|s| *s.borrow_mut() = None);
    let total = t0.elapsed().as_millis();
    if dbg {
        println!("[quit] cef shutdown total: {total} ms");
    }
    // Self-test check (the run ends right after this): the web layer must go down quickly with its tabs open.
    if std::env::var_os("SIGNET_SELFTEST").is_some_and(|v| !v.is_empty() && v != "0") {
        // Closing dozens of browsers takes about 3 s on the Linux CI runners (X11 windows, software rendering).
        let limit: u128 = if cfg!(target_os = "macos") { 1500 } else { 8000 };
        println!("[selftest] {} quit.cef_shutdown - {total} ms (limit {limit})", if (total as u128) < limit { "PASS" } else { "FAIL" });
    }
}

pub(crate) fn data_dir() -> Option<PathBuf> {
    with_state(|s| s.config.data_dir.clone())
}

pub(crate) fn profile_cache_path(data_dir: &Path, key: &str) -> Option<PathBuf> {
    key.strip_prefix("p:").map(|id| {
        let safe: String = id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
        data_dir.join("cef").join(format!("profile-{safe}"))
    })
}

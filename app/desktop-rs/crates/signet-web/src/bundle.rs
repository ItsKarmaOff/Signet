//! Delivery of the page-script bundle to the renderer, once per document, before page scripts.
//!
//! CEF has no synchronous browser/renderer call, and the right bundle depends on the document
//! (URL, main frame or not, profile, settings), so the renderer asks the browser process over a
//! loopback socket: the browser starts a tiny server at `init`, publishes `host:port:token` in the
//! `SIGNET_WEB_BUNDLE` environment variable (inherited by every CEF subprocess), and the
//! renderer, in `OnContextCreated`, sends one JSON line and blocks for the one-line answer (about
//! 100 microseconds on loopback). The answer comes from the closure given to `set_frame_bundle`.
//! The token keeps other local processes out; page code cannot reach the variable.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

use crate::types::*;

const ENV: &str = "SIGNET_WEB_BUNDLE";
/// Command line switch carrying the same `port:token` to every CEF subprocess (works even when the
/// platform launcher does not forward the environment).
pub(crate) const SWITCH: &str = "signet-bundle";

/// `port:token` of the server, from the switch or the environment (renderer side).
fn address() -> Option<String> {
    use cef::ImplCommandLine as _;
    if let Some(cl) = cef::command_line_get_global() {
        let v = cef::CefString::from(&cl.switch_value(Some(&cef::CefString::from(SWITCH)))).to_string();
        if !v.is_empty() {
            return Some(v);
        }
    }
    std::env::var(ENV).ok()
}

/// Value to give to the `signet-bundle` switch of child processes (browser side).
pub(crate) fn child_switch_value() -> Option<String> {
    std::env::var(ENV).ok()
}

// Browser-side registry: what the renderer needs to know about each app browser. The renderer
// normally gets it through `extra_info`, but after a cross-process navigation (about:blank to a web
// page, for example) CEF does not call `OnBrowserCreated` in the new process: it asks here instead.
static STASH: std::sync::Mutex<Option<std::collections::HashMap<u64, String>>> = std::sync::Mutex::new(None);
static BROWSERS: std::sync::Mutex<Option<std::collections::HashMap<i32, String>>> = std::sync::Mutex::new(None);

/// Remember the `extra_info` JSON of a view being created.
pub(crate) fn stash_extra(view: ViewId, json: String) {
    if let Ok(mut g) = STASH.lock() {
        g.get_or_insert_with(Default::default).insert(view.0, json);
    }
}

/// Bind the stashed JSON of `view` to its CEF browser id.
pub(crate) fn register_browser(view: ViewId, browser_id: i32) {
    let json = STASH.lock().ok().and_then(|mut g| g.as_mut().and_then(|m| m.remove(&view.0)));
    if let (Some(json), Ok(mut g)) = (json, BROWSERS.lock()) {
        g.get_or_insert_with(Default::default).insert(browser_id, json);
    }
}

pub(crate) fn forget_browser(browser_id: i32) {
    if let Ok(mut g) = BROWSERS.lock() {
        if let Some(m) = g.as_mut() {
            m.remove(&browser_id);
        }
    }
}

fn random_token() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut out = String::new();
    for _ in 0..4 {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
        h.write_u32(std::process::id());
        out.push_str(&format!("{:016x}", h.finish()));
    }
    out
}

/// Start the server (browser process). Idempotent. Must run before CEF spawns renderers.
pub(crate) fn start_server() {
    static STARTED: std::sync::Once = std::sync::Once::new();
    STARTED.call_once(|| {
        let Ok(listener) = TcpListener::bind(("127.0.0.1", 0)) else {
            eprintln!("[signet-web] cannot start the bundle server; page scripts will not run");
            return;
        };
        let Ok(addr) = listener.local_addr() else { return };
        let token = random_token();
        // SAFETY: called once during `init`, before other threads read the environment.
        unsafe { std::env::set_var(ENV, format!("{}:{}", addr.port(), token)) };
        std::thread::Builder::new()
            .name("signet-bundle-accept".into())
            .spawn(move || {
                for stream in listener.incoming().flatten() {
                    let token = token.clone();
                    let _ = std::thread::Builder::new().name("signet-bundle".into()).spawn(move || serve(stream, &token));
                }
            })
            .ok();
    });
}

fn serve(stream: TcpStream, token: &str) {
    let _ = stream.set_nodelay(true);
    let Ok(read_half) = stream.try_clone() else { return };
    let mut reader = BufReader::new(read_half);
    let mut writer = stream;
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        let answer = answer(line.trim_end(), token);
        let mut out = serde_json::to_string(&answer).unwrap_or_else(|_| "null".into());
        out.push('\n');
        if writer.write_all(out.as_bytes()).is_err() {
            return;
        }
    }
}

fn answer(line: &str, token: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    if v["t"].as_str()? != token {
        return None;
    }
    if let Some(id) = v["lookup"].as_i64() {
        return BROWSERS.lock().ok()?.as_ref()?.get(&(id as i32)).cloned();
    }
    let profile = v["profile"].as_str().unwrap_or("p:default");
    let profile = match profile.split_once(':') {
        Some(("m", id)) => ProfileKind::Private(id.into()),
        Some((_, id)) => ProfileKind::Persistent(id.into()),
        None => ProfileKind::Persistent(profile.into()),
    };
    let info = FrameInfo {
        view: ViewId(v["view"].as_u64()?),
        is_private: matches!(profile, ProfileKind::Private(_)),
        profile,
        url: crate::extension::from_nav(v["url"].as_str().unwrap_or_default()),
        is_main_frame: v["main"].as_bool().unwrap_or(false),
        frame_id: v["frame"].as_str().unwrap_or_default().into(),
    };
    let f = crate::hooks::FRAME_BUNDLE.read().ok()?.clone()?;
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&info))).ok().flatten()
}

// ---- renderer side ---------------------------------------------------------------------------

struct Conn {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
}

thread_local! {
    static CONN: std::cell::RefCell<Option<Conn>> = const { std::cell::RefCell::new(None) };
}

fn connect() -> Option<(Conn, String)> {
    let env = address()?;
    let (port, token) = env.split_once(':')?;
    let stream = TcpStream::connect(("127.0.0.1", port.parse::<u16>().ok()?)).ok()?;
    let _ = stream.set_nodelay(true);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    let writer = stream.try_clone().ok()?;
    Some((Conn { reader: BufReader::new(stream), writer }, token.to_string()))
}

/// Ask the browser process for the bundle of a document. `None`: nothing to inject (or no server).
pub(crate) fn fetch(view: u64, profile: &str, url: &str, is_main: bool, frame_id: &str) -> Option<String> {
    let token = address()?.split_once(':')?.1.to_string();
    let req = serde_json::json!({"t": token, "view": view, "profile": profile, "url": url, "main": is_main, "frame": frame_id}).to_string();
    request_line(&req)
}

fn request_line(req: &str) -> Option<String> {
    let debug = std::env::var_os("SIGNET_WEB_DEBUG").is_some();
    for _attempt in 0..2 {
        let result = CONN.with(|c| {
            let mut c = c.borrow_mut();
            if c.is_none() {
                *c = connect().map(|(conn, _)| conn);
                if c.is_none() && debug {
                    eprintln!("[signet-web] cannot connect to the bundle server");
                }
            }
            let conn = c.as_mut()?;
            let mut line = req.to_string();
            line.push('\n');
            let mut reply = String::new();
            let ok = conn.writer.write_all(line.as_bytes()).is_ok() && matches!(conn.reader.read_line(&mut reply), Ok(n) if n > 0);
            if !ok {
                if debug {
                    eprintln!("[signet-web] bundle request failed (connection lost)");
                }
                *c = None;
                return None;
            }
            Some(serde_json::from_str::<Option<String>>(reply.trim_end()).ok().flatten())
        });
        if let Some(r) = result {
            return r;
        }
    }
    None
}

/// Ask the browser process for the `extra_info` JSON of a browser (renderer side).
pub(crate) fn lookup(browser_id: i32) -> Option<String> {
    let token = address()?.split_once(':')?.1.to_string();
    let req = serde_json::json!({"t": token, "lookup": browser_id}).to_string();
    request_line(&req)
}

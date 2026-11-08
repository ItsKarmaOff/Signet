//! Registries for the synchronous web hooks, which signet-web allows only once each. Modules register at `startup`;
//! `install()` sets one signet-web closure per hook that runs the registered ones in priority order (lower first).
//!
//! - Request hooks: the first `Block` or `Redirect` wins; `AddHeaders` from every hook are merged; otherwise `Allow`.
//! - Response hooks: the first `Redirect` wins.
//! - Frame bundle parts: every part is concatenated (in priority order) into the program evaluated once per document.
//!
//! Closures run on CEF threads (IO thread for requests): they must be fast and must not touch `Core`.

use std::sync::{Arc, RwLock};

use signet_web::{FrameInfo, RequestDecision, RequestInfo, ResponseDecision, ResponseInfo};

type RequestHook = Arc<dyn Fn(&RequestInfo) -> Option<RequestDecision> + Send + Sync>;
type ResponseHook = Arc<dyn Fn(&ResponseInfo) -> Option<ResponseDecision> + Send + Sync>;
type BundlePart = Arc<dyn Fn(&FrameInfo) -> Option<String> + Send + Sync>;

static REQUEST: RwLock<Vec<(i32, RequestHook)>> = RwLock::new(Vec::new());
static RESPONSE: RwLock<Vec<(i32, ResponseHook)>> = RwLock::new(Vec::new());
static BUNDLE: RwLock<Vec<(i32, BundlePart)>> = RwLock::new(Vec::new());

fn add<T>(list: &RwLock<Vec<(i32, T)>>, priority: i32, item: T) {
    if let Ok(mut v) = list.write() {
        v.push((priority, item));
        v.sort_by_key(|(p, _)| *p);
    }
}

/// Registers a request hook. Return `None` to let the next hooks decide.
pub fn add_request_hook(priority: i32, f: impl Fn(&RequestInfo) -> Option<RequestDecision> + Send + Sync + 'static) {
    add(&REQUEST, priority, Arc::new(f) as RequestHook);
}

/// Registers a response hook. Return `None` to let the next hooks decide.
pub fn add_response_hook(priority: i32, f: impl Fn(&ResponseInfo) -> Option<ResponseDecision> + Send + Sync + 'static) {
    add(&RESPONSE, priority, Arc::new(f) as ResponseHook);
}

/// Registers a part of the per-document program (for example the signet-scripts bundle, extension content scripts).
pub fn add_bundle_part(priority: i32, f: impl Fn(&FrameInfo) -> Option<String> + Send + Sync + 'static) {
    add(&BUNDLE, priority, Arc::new(f) as BundlePart);
}

/// Installs the chained closures into signet-web. Call once, after every module's `startup`.
pub fn install() {
    signet_web::set_request_filter(|r| {
        let hooks = REQUEST.read().map(|v| v.iter().map(|(_, h)| h.clone()).collect::<Vec<_>>()).unwrap_or_default();
        let mut headers: Vec<(String, String)> = Vec::new();
        for h in hooks {
            match h(r) {
                Some(RequestDecision::Block) => return RequestDecision::Block,
                Some(RequestDecision::Redirect(url)) => return RequestDecision::Redirect(url),
                Some(RequestDecision::AddHeaders(mut more)) => headers.append(&mut more),
                Some(RequestDecision::Allow) | None => {}
            }
        }
        if headers.is_empty() { RequestDecision::Allow } else { RequestDecision::AddHeaders(headers) }
    });
    signet_web::set_response_hook(|r| {
        let hooks = RESPONSE.read().map(|v| v.iter().map(|(_, h)| h.clone()).collect::<Vec<_>>()).unwrap_or_default();
        for h in hooks {
            if let Some(ResponseDecision::Redirect(url)) = h(r) {
                return ResponseDecision::Redirect(url);
            }
        }
        ResponseDecision::Continue
    });
    signet_web::set_frame_bundle(|f| {
        let parts = BUNDLE.read().map(|v| v.iter().map(|(_, h)| h.clone()).collect::<Vec<_>>()).unwrap_or_default();
        let mut out = String::new();
        let debug = std::env::var_os("SIGNET_WEB_DEBUG").is_some();
        for (i, p) in parts.iter().enumerate() {
            if let Some(js) = p(f) {
                if debug {
                    eprintln!("[bundle] part {i} gives {} bytes for {}", js.len(), f.url);
                }
                out.push_str(&js);
                out.push('\n');
            }
        }
        if out.is_empty() { None } else { Some(out) }
    });
}

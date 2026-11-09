//! `chrome-extension://` scheme and hidden background views, for Chrome extensions (see the signet-extensions README).
//!
//! The scheme is registered standard + secure + CORS + fetch enabled, so every extension id is its own origin.
//! Responses come from an app closure (`set_extension_handler`) that serves the extension folders; it receives the
//! requested URL and the referrer so it can apply `web_accessible_resources` to requests made by web pages.

use std::sync::{Arc, Mutex};

use cef::*;
use raw_window_handle::RawWindowHandle;

use crate::types::{Bounds, Error, ProfileKind, ViewOptions};
use crate::view::WebView;

/// The scheme of extension pages.
pub const EXTENSION_SCHEME: &str = "chrome-extension";

/// Scheme under which the extension pages (background, popup, options, tabs) are really navigated.
///
/// Chromium's `ExtensionNavigationThrottle` cancels every navigation to `chrome-extension://<id>/` whose extension is
/// not in its own registry (`ERR_BLOCKED_BY_CLIENT`), and ours never is. Sub-resource requests are not throttled, so
/// only navigations use this alias. The rest of the app never sees it: URLs going in are rewritten by [`to_nav`] and
/// URLs coming out (events, frame lists, request infos) by [`from_nav`].
pub const NAV_SCHEME: &str = "signet-extension";

const EXT_PREFIX: &str = "chrome-extension://";
const NAV_PREFIX: &str = "signet-extension://";

fn swap(url: &str, from: &str, to: &str) -> String {
    match url.get(..from.len()) {
        Some(head) if head.eq_ignore_ascii_case(from) => format!("{to}{}", &url[from.len()..]),
        _ => url.to_string(),
    }
}

/// `chrome-extension://id/x` to the scheme really navigated.
pub fn to_nav(url: &str) -> String {
    swap(url, EXT_PREFIX, NAV_PREFIX)
}

/// Inverse of [`to_nav`].
pub fn from_nav(url: &str) -> String {
    swap(url, NAV_PREFIX, EXT_PREFIX)
}

fn fix(u: &mut String) {
    if u.len() >= NAV_PREFIX.len() && u.get(..NAV_PREFIX.len()).is_some_and(|h| h.eq_ignore_ascii_case(NAV_PREFIX)) {
        *u = from_nav(u);
    }
}

/// Rewrites the URLs of an event leaving the crate.
pub(crate) fn remap_event(ev: &mut crate::types::WebEvent) {
    use crate::types::WebEvent as E;
    match ev {
        E::NewWindowRequested { url, .. }
        | E::AddressChanged { url, .. }
        | E::LoadStart { url, .. }
        | E::LoadEnd { url, .. }
        | E::LoadError { url, .. }
        | E::StatusText { url, .. }
        | E::RequestBlocked { url, .. }
        | E::MainFrameResponse { url, .. } => fix(url),
        E::PageMessage { frame_url, top_url, .. } => {
            fix(frame_url);
            fix(top_url);
        }
        E::ContextMenu { info, .. } => {
            fix(&mut info.page_url);
            fix(&mut info.frame_url);
            for u in [&mut info.link_url, &mut info.image_url, &mut info.media_url].into_iter().flatten() {
                fix(u);
            }
        }
        _ => {}
    }
}

/// A request for a file of an extension.
#[derive(Clone, Debug)]
pub struct ExtensionRequest {
    pub url: String,
    /// Referrer of the request: empty for address bar navigations and views the app opened itself.
    pub referrer: String,
    pub method: String,
}

/// A file served for a `chrome-extension://` URL.
#[derive(Clone, Debug)]
pub struct ExtensionResponse {
    pub mime: String,
    pub body: Vec<u8>,
    pub status: u16,
    /// Extra response headers (`Access-Control-Allow-Origin`...).
    pub headers: Vec<(String, String)>,
}

pub type ExtensionHandlerFn = dyn Fn(&ExtensionRequest) -> Option<ExtensionResponse> + Send + Sync;

static HANDLER: Mutex<Option<Arc<ExtensionHandlerFn>>> = Mutex::new(None);

/// Sets the handler of `chrome-extension://` URLs. It runs on a CEF IO thread: `Send + Sync`, no call into signet-web.
/// `None` from the closure gives a 404.
pub fn set_extension_handler(f: impl Fn(&ExtensionRequest) -> Option<ExtensionResponse> + Send + Sync + 'static) {
    if let Ok(mut h) = HANDLER.lock() {
        *h = Some(Arc::new(f) as Arc<ExtensionHandlerFn>);
    }
}

fn handler() -> Option<Arc<ExtensionHandlerFn>> {
    HANDLER.lock().ok().and_then(|h| h.clone())
}

fn not_found() -> ExtensionResponse {
    ExtensionResponse { mime: "text/plain".into(), body: b"Not found".to_vec(), status: 404, headers: vec![] }
}

wrap_resource_handler! {
    struct ExtensionResource {
        response: Arc<Mutex<Option<ExtensionResponse>>>,
        offset: Arc<Mutex<usize>>,
    }

    impl ResourceHandler {
        fn open(
            &self,
            request: Option<&mut Request>,
            handle_request: Option<&mut ::std::os::raw::c_int>,
            _callback: Option<&mut Callback>,
        ) -> ::std::os::raw::c_int {
            if let Some(h) = handle_request {
                *h = 1;
            }
            let req = request.map(|r| ExtensionRequest {
                url: from_nav(&CefString::from(&r.url()).to_string()),
                referrer: from_nav(&CefString::from(&r.referrer_url()).to_string()),
                method: CefString::from(&r.method()).to_string(),
            });
            if std::env::var_os("SIGNET_WEB_DEBUG").is_some() {
                eprintln!("[signet-web] extension request {:?}", req.as_ref().map(|r| r.url.as_str()));
            }
            let resp = req.and_then(|r| handler().and_then(|h| std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| h(&r))).ok().flatten()));
            if let Ok(mut r) = self.response.lock() {
                *r = Some(resp.unwrap_or_else(not_found));
            }
            1
        }

        fn response_headers(
            &self,
            response: Option<&mut Response>,
            response_length: Option<&mut i64>,
            _redirect_url: Option<&mut CefString>,
        ) {
            let guard = self.response.lock().ok();
            let Some(guard) = guard else { return };
            let Some(r) = guard.as_ref() else { return };
            if let Some(resp) = response {
                resp.set_status(r.status as i32);
                // The bare type only: a `; charset=...` suffix makes Chromium show the page as plain text.
                let (mime, charset) = match r.mime.split_once(';') {
                    Some((m, rest)) => (m.trim(), rest.split_once('=').map(|(_, c)| c.trim().trim_matches('"').to_string())),
                    None => (r.mime.trim(), None),
                };
                resp.set_mime_type(Some(&CefString::from(mime)));
                if let Some(c) = charset.filter(|c| !c.is_empty()) {
                    resp.set_charset(Some(&CefString::from(c.as_str())));
                }
                resp.set_header_by_name(Some(&CefString::from("Cache-Control")), Some(&CefString::from("no-store")), 1);
                for (k, v) in &r.headers {
                    resp.set_header_by_name(Some(&CefString::from(k.as_str())), Some(&CefString::from(v.as_str())), 1);
                }
            }
            if let Some(len) = response_length {
                *len = r.body.len() as i64;
            }
        }

        fn read(
            &self,
            data_out: *mut u8,
            bytes_to_read: ::std::os::raw::c_int,
            bytes_read: Option<&mut ::std::os::raw::c_int>,
            _callback: Option<&mut ResourceReadCallback>,
        ) -> ::std::os::raw::c_int {
            let (Ok(guard), Ok(mut off)) = (self.response.lock(), self.offset.lock()) else { return 0 };
            let Some(r) = guard.as_ref() else { return 0 };
            let remaining = r.body.len().saturating_sub(*off);
            let n = remaining.min(bytes_to_read.max(0) as usize);
            if let Some(out) = bytes_read {
                *out = n as i32;
            }
            if n == 0 || data_out.is_null() {
                return 0;
            }
            // SAFETY: CEF provides a buffer of at least `bytes_to_read` bytes.
            unsafe { std::ptr::copy_nonoverlapping(r.body.as_ptr().add(*off), data_out, n) };
            *off += n;
            1
        }

        fn cancel(&self) {}
    }
}

wrap_scheme_handler_factory! {
    pub struct ExtensionSchemeFactory;

    impl SchemeHandlerFactory {
        fn create(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _scheme_name: Option<&CefString>,
            _request: Option<&mut Request>,
        ) -> Option<ResourceHandler> {
            Some(ExtensionResource::new(Arc::new(Mutex::new(None)), Arc::new(Mutex::new(0))))
        }
    }
}

pub(crate) fn register_global() {
    let mut f = ExtensionSchemeFactory::new();
    register_scheme_handler_factory(Some(&CefString::from(EXTENSION_SCHEME)), None, Some(&mut f));
    let mut f = ExtensionSchemeFactory::new();
    register_scheme_handler_factory(Some(&CefString::from(NAV_SCHEME)), None, Some(&mut f));
}

pub(crate) fn register_in(ctx: &RequestContext) {
    let mut f = ExtensionSchemeFactory::new();
    ctx.register_scheme_handler_factory(Some(&CefString::from(EXTENSION_SCHEME)), None, Some(&mut f));
    let mut f = ExtensionSchemeFactory::new();
    ctx.register_scheme_handler_factory(Some(&CefString::from(NAV_SCHEME)), None, Some(&mut f));
}

impl WebView {
    /// A view nobody sees, for extension backgrounds: a 2x2 view parked outside the window's visible area. It is not
    /// hidden (hidden views get throttled), and it never takes the keyboard focus.
    pub fn create_background(parent: RawWindowHandle, profile: ProfileKind, url: &str) -> Result<WebView, Error> {
        crate::runtime::assert_main_thread();
        let bounds = Bounds::new(-40.0, -40.0, 2.0, 2.0);
        let parent = crate::platform::parent_to_usize(parent).map_err(Error::Parent)?;
        let view = Self::create_native(parent, ViewOptions { profile, url: url.to_string(), bounds, visible: true, transparent_background: false }, None)?;
        // CEF's host view resizes with the window (width and height sizable): a larger window would grow this parked
        // view into the visible area (a white rectangle at the top-left). Signet places it itself.
        view.pin_top_left();
        view.set_bounds(bounds);
        Ok(view)
    }
}

// ---------------------------------------------------------------------------------------------
// Preferences set by extensions (`chrome.proxy`, `chrome.privacy`)
// ---------------------------------------------------------------------------------------------

static PREFS: Mutex<Vec<(String, serde_json::Value)>> = Mutex::new(Vec::new());

fn to_cef_value(v: &serde_json::Value) -> Option<Value> {
    use serde_json::Value as J;
    let out = value_create()?;
    match v {
        J::Bool(b) => {
            out.set_bool(*b as i32);
        }
        J::Number(n) => {
            if let Some(i) = n.as_i64() {
                out.set_int(i as i32);
            } else {
                out.set_double(n.as_f64().unwrap_or(0.0));
            }
        }
        J::String(s) => {
            out.set_string(Some(&CefString::from(s.as_str())));
        }
        J::Array(items) => {
            let mut list = list_value_create()?;
            list.set_size(items.len());
            for (i, item) in items.iter().enumerate() {
                let mut child = to_cef_value(item)?;
                list.set_value(i, Some(&mut child));
            }
            out.set_list(Some(&mut list));
        }
        J::Object(map) => {
            let mut dict = dictionary_value_create()?;
            for (k, item) in map {
                let mut child = to_cef_value(item)?;
                dict.set_value(Some(&CefString::from(k.as_str())), Some(&mut child));
            }
            out.set_dictionary(Some(&mut dict));
        }
        J::Null => {
            out.set_null();
        }
    }
    Some(out)
}

pub(crate) fn apply_preferences(ctx: &RequestContext) {
    let prefs = PREFS.lock().map(|p| p.clone()).unwrap_or_default();
    for (name, value) in prefs {
        set_on(ctx, &name, &value);
    }
}

fn set_on(ctx: &RequestContext, name: &str, value: &serde_json::Value) -> bool {
    let Some(mut v) = to_cef_value(value) else { return false };
    let mut err = CefString::default();
    ctx.set_preference(Some(&CefString::from(name)), Some(&mut v), Some(&mut err)) == 1
}

/// Sets a Chromium preference (for example `proxy`, `webrtc.ip_handling_policy`) on every profile, now and for the
/// profiles created later. Main thread. Returns how many existing profiles accepted it.
pub fn set_preference_json(name: &str, value: &serde_json::Value) -> usize {
    crate::runtime::assert_main_thread();
    if let Ok(mut p) = PREFS.lock() {
        p.retain(|(n, _)| n != name);
        p.push((name.to_string(), value.clone()));
    }
    let contexts: Vec<RequestContext> = crate::runtime::with_state(|st| st.contexts.values().cloned().collect()).unwrap_or_default();
    contexts.iter().filter(|c| set_on(c, name, value)).count()
}

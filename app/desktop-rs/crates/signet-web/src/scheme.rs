//! Custom schemes (`signet`, `signetui`): registration and the resource handler that serves
//! responses produced by the app-provided closure.

use std::sync::{Arc, Mutex};

use cef::*;

use crate::types::{SchemeHandlerFn, SchemeResponse};

/// Schemes registered as standard + secure + CORS + fetch enabled, in every process.
pub const CUSTOM_SCHEMES: &[&str] = &["signet", "signetui"];

// standard (1) | secure (8) | cors enabled (16) | fetch enabled (64)
const SCHEME_OPTIONS: i32 = 1 | 8 | 16 | 64;

static HANDLER: Mutex<Option<Arc<SchemeHandlerFn>>> = Mutex::new(None);

pub(crate) fn set_global_handler(f: Arc<SchemeHandlerFn>) {
    if let Ok(mut h) = HANDLER.lock() {
        *h = Some(f);
    }
}

pub(crate) fn global_handler() -> Option<Arc<SchemeHandlerFn>> {
    HANDLER.lock().ok().and_then(|h| h.clone())
}

pub(crate) fn register_custom_schemes(registrar: &mut SchemeRegistrar) {
    for s in CUSTOM_SCHEMES {
        registrar.add_custom_scheme(Some(&CefString::from(*s)), SCHEME_OPTIONS);
    }
    registrar.add_custom_scheme(Some(&CefString::from(crate::extension::EXTENSION_SCHEME)), SCHEME_OPTIONS);
    registrar.add_custom_scheme(Some(&CefString::from(crate::extension::NAV_SCHEME)), SCHEME_OPTIONS);
}

wrap_resource_handler! {
    struct SignetResource {
        response: Arc<Mutex<Option<SchemeResponse>>>,
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
            let url = request.map(|r| CefString::from(&r.url()).to_string()).unwrap_or_default();
            let resp = global_handler().and_then(|h| h(&url));
            let found = resp.is_some();
            if let Ok(mut r) = self.response.lock() {
                *r = Some(resp.unwrap_or(SchemeResponse { mime: "text/plain".into(), body: b"Not found".to_vec(), status: 404 }));
            }
            let _ = found;
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
                // CEF wants the bare type; a `; charset=...` suffix makes Chromium show the page as plain text.
                let (mime, charset) = match r.mime.split_once(';') {
                    Some((m, rest)) => (m.trim(), rest.split_once('=').map(|(_, c)| c.trim().trim_matches('"').to_string())),
                    None => (r.mime.trim(), None),
                };
                resp.set_mime_type(Some(&CefString::from(mime)));
                if let Some(c) = charset.filter(|c| !c.is_empty()) {
                    resp.set_charset(Some(&CefString::from(c.as_str())));
                }
                resp.set_header_by_name(
                    Some(&CefString::from("Cache-Control")),
                    Some(&CefString::from("no-store")),
                    1,
                );
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
    pub struct SignetSchemeFactory;

    impl SchemeHandlerFactory {
        fn create(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _scheme_name: Option<&CefString>,
            _request: Option<&mut Request>,
        ) -> Option<ResourceHandler> {
            Some(SignetResource::new(Arc::new(Mutex::new(None)), Arc::new(Mutex::new(0))))
        }
    }
}

/// Register the factories on the global request context.
pub(crate) fn register_factories_global() {
    for s in CUSTOM_SCHEMES {
        let mut f = SignetSchemeFactory::new();
        register_scheme_handler_factory(Some(&CefString::from(*s)), None, Some(&mut f));
    }
    crate::extension::register_global();
}

/// Register the factories on a profile request context.
pub(crate) fn register_factories(ctx: &RequestContext) {
    for s in CUSTOM_SCHEMES {
        let mut f = SignetSchemeFactory::new();
        ctx.register_scheme_handler_factory(Some(&CefString::from(*s)), None, Some(&mut f));
    }
    crate::extension::register_in(ctx);
}

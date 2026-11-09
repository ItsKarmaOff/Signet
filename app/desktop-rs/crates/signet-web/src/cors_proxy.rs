//! CORS exemption for extension pages that hold host permissions.
//!
//! Chrome lets an extension's pages (background, popup, options) fetch the hosts of its `host_permissions` without CORS.
//! CEF's own origin whitelist is not honored for the extension schemes, so those requests are answered here: a request
//! made by `signet-extension://<id>` (or `chrome-extension://<id>`) to an `http(s)` URL the extension may access is
//! carried out by a `CefURLRequest` on the page's request context (cookies are shared), and its response is handed back
//! with `Access-Control-Allow-Origin` set. Preflights are answered directly. Everything else, including pages without
//! host permissions, keeps the normal CORS rules.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use cef::*;

fn trace(msg: impl FnOnce() -> String) {
    if std::env::var_os("SIGNET_WEB_DEBUG").is_some() {
        eprintln!("[signet-web] cors {}", msg());
    }
}

/// `(scheme, domain, with_subdomains)` targets per extension id; an empty domain with subdomains means every host.
type Targets = Vec<(String, String, bool)>;

static EXEMPT: RwLock<Option<HashMap<String, Targets>>> = RwLock::new(None);

/// Sets (or, with `allow` false, removes) the hosts an extension's pages may fetch across origins.
pub fn set_cors_exemption(extension_id: &str, targets: &[(String, String, bool)], allow: bool) {
    if let Ok(mut g) = EXEMPT.write() {
        let map = g.get_or_insert_with(HashMap::new);
        if allow && !targets.is_empty() {
            map.insert(extension_id.to_string(), targets.to_vec());
        } else {
            map.remove(extension_id);
        }
    }
}

/// The extension id of a request initiator (`signet-extension://id` or `chrome-extension://id`, with or without path).
pub(crate) fn extension_of_initiator(initiator: &str) -> Option<&str> {
    let rest = initiator.strip_prefix("signet-extension://").or_else(|| initiator.strip_prefix("chrome-extension://"))?;
    let id = rest.split(['/', '?', '#']).next().unwrap_or("");
    (!id.is_empty()).then_some(id)
}

/// Does one of `targets` cover `url`? Only `http` and `https` URLs can be covered.
pub(crate) fn covers(targets: &[(String, String, bool)], url: &str) -> bool {
    let Ok(u) = url::Url::parse(url) else { return false };
    let scheme = u.scheme();
    if scheme != "http" && scheme != "https" {
        return false;
    }
    let Some(host) = u.host_str() else { return false };
    let host = host.to_ascii_lowercase();
    targets.iter().any(|(s, domain, sub)| {
        s == scheme
            && match (domain.as_str(), *sub) {
                ("", true) => true,
                (d, true) => host == d || host.ends_with(&format!(".{d}")),
                (d, false) => host == d,
            }
    })
}

/// Is a request from `initiator` to `url` exempt from CORS?
pub(crate) fn is_exempt(initiator: &str, url: &str) -> bool {
    let Some(id) = extension_of_initiator(initiator) else { return false };
    EXEMPT.read().ok().and_then(|g| g.as_ref().and_then(|m| m.get(id).map(|t| covers(t, url)))).unwrap_or(false)
}

/// The answer being built: filled by the URL request client, read by the resource handler.
#[derive(Default)]
struct Answer {
    status: i32,
    mime: String,
    charset: Option<String>,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    offset: usize,
    failed: bool,
    done: bool,
    pending_open: Option<Callback>,
}

type Shared = Arc<Mutex<Answer>>;

/// Headers that describe the transfer of the original response, not the one we hand back.
const DROPPED: &[&str] = &["content-encoding", "content-length", "transfer-encoding", "connection", "keep-alive", "access-control-allow-origin", "access-control-allow-credentials", "access-control-expose-headers"];

fn with_cors(mut headers: Vec<(String, String)>, origin: &str) -> Vec<(String, String)> {
    headers.retain(|(k, _)| !DROPPED.contains(&k.to_ascii_lowercase().as_str()));
    headers.push(("Access-Control-Allow-Origin".into(), origin.into()));
    headers.push(("Access-Control-Allow-Credentials".into(), "true".into()));
    headers.push(("Access-Control-Expose-Headers".into(), "*".into()));
    headers
}

wrap_urlrequest_client! {
    struct ProxyClient { answer: Shared, origin: String }

    impl UrlrequestClient {
        fn on_download_data(&self, _request: Option<&mut Urlrequest>, data: *const u8, data_length: usize) {
            if data.is_null() || data_length == 0 {
                return;
            }
            // SAFETY: CEF passes `data_length` readable bytes.
            let chunk = unsafe { std::slice::from_raw_parts(data, data_length) };
            if let Ok(mut a) = self.answer.lock() {
                a.body.extend_from_slice(chunk);
            }
        }

        fn on_request_complete(&self, request: Option<&mut Urlrequest>) {
            let Some(request) = request else { return };
            trace(|| "complete".into());
            let ok = matches!(request.request_status().as_ref(), sys::cef_urlrequest_status_t::UR_SUCCESS);
            trace(|| format!("inner request status ok={ok} error={}", request.request_error().as_ref().clone() as i32));
            let mut head = (0, String::new(), None, Vec::new());
            if ok && let Some(resp) = request.response() {
                let mut map = CefStringMultimap::new();
                resp.header_map(Some(&mut map));
                let headers: Vec<(String, String)> = map.into_iter().flat_map(|(k, vs)| vs.into_iter().map(move |v| (k.clone(), v))).collect();
                let charset = CefString::from(&resp.charset()).to_string();
                trace(|| format!("inner response {} mime={:?} headers={}", resp.status(), CefString::from(&resp.mime_type()).to_string(), headers.iter().map(|(k, v)| format!("{k}: {v}")).collect::<Vec<_>>().join(" | ").chars().take(400).collect::<String>()));
                head = (resp.status(), CefString::from(&resp.mime_type()).to_string(), (!charset.is_empty()).then_some(charset), headers);
            }
            let cb = {
                let Ok(mut a) = self.answer.lock() else { return };
                a.failed = !ok;
                a.status = head.0;
                a.mime = head.1;
                a.charset = head.2;
                a.headers = with_cors(head.3, &self.origin);
                a.done = true;
                a.pending_open.take()
            };
            if let Some(cb) = cb {
                cb.cont();
            }
        }
    }
}

type StartJob = Arc<Mutex<Option<(Request, Option<RequestContext>, UrlrequestClient)>>>;

wrap_task! {
    struct StartRequest { job: StartJob, answer: Shared, keep: Arc<Mutex<Option<Urlrequest>>> }

    impl Task {
        fn execute(&self) {
            let Some((req, mut ctx, mut client)) = self.job.lock().ok().and_then(|mut j| j.take()) else { return };
            let mut req = req;
            let created = urlrequest_create(Some(&mut req), Some(&mut client), ctx.as_mut());
            trace(|| format!("urlrequest created: {}", created.is_some()));
            match created {
                Some(r) => {
                    if let Ok(mut k) = self.keep.lock() {
                        *k = Some(r);
                    }
                }
                None => {
                    let cb = self.answer.lock().ok().and_then(|mut a| {
                        a.failed = true;
                        a.done = true;
                        a.pending_open.take()
                    });
                    if let Some(cb) = cb {
                        cb.cont();
                    }
                }
            }
        }
    }
}

wrap_resource_handler! {
    struct ProxyResource {
        answer: Shared,
        origin: String,
        // The request context of the page and what to ask for; None for a preflight answered locally.
        job: Arc<Mutex<Option<(Request, Option<RequestContext>)>>>,
        keep: Arc<Mutex<Option<Urlrequest>>>,
    }

    impl ResourceHandler {
        fn open(
            &self,
            _request: Option<&mut Request>,
            handle_request: Option<&mut ::std::os::raw::c_int>,
            callback: Option<&mut Callback>,
        ) -> ::std::os::raw::c_int {
            trace(|| "open".into());
            let job = self.job.lock().ok().and_then(|mut j| j.take());
            trace(|| format!("job taken: {}", job.is_some()));
            let Some((mut req, mut ctx)) = job else {
                // A preflight: allow what is asked for.
                if let Some(h) = handle_request {
                    *h = 1;
                }
                return 1;
            };
            if let Ok(mut a) = self.answer.lock() {
                a.pending_open = callback.cloned();
            }
            trace(|| "callback stored".into());
            let client = ProxyClient::new(self.answer.clone(), self.origin.clone());
            if let Some(h) = handle_request {
                // Answered later: `callback.cont()` runs when the request completes.
                *h = 0;
            }
            // CefURLRequest must be created on the UI thread (creating it on the IO thread aborts the process).
            let start = Arc::new(Mutex::new(Some((req, ctx.take(), client))));
            let (answer, keep) = (self.answer.clone(), self.keep.clone());
            let mut task = StartRequest::new(start, answer, keep);
            if post_task(ThreadId::UI, Some(&mut task)) == 0 {
                if let Ok(mut a) = self.answer.lock() {
                    a.failed = true;
                    a.done = true;
                    a.pending_open = None;
                }
                return 0;
            }
            1
        }

        fn cancel(&self) {
            trace(|| "cancel".into());
        }

        fn response_headers(&self, response: Option<&mut Response>, response_length: Option<&mut i64>, _redirect_url: Option<&mut CefString>) {
            let Ok(a) = self.answer.lock() else { return };
            let Some(resp) = response else { return };
            if a.failed {
                resp.set_error(Errorcode::from(sys::cef_errorcode_t::ERR_FAILED));
                return;
            }
            trace(|| format!("response_headers status={} mime={:?} body={} headers={}", a.status, a.mime, a.body.len(), a.headers.len()));
            resp.set_status(a.status);
            resp.set_status_text(Some(&CefString::from(if a.status == 204 { "No Content" } else { "OK" })));
            if !a.mime.is_empty() {
                resp.set_mime_type(Some(&CefString::from(a.mime.as_str())));
            }
            if let Some(c) = &a.charset {
                resp.set_charset(Some(&CefString::from(c.as_str())));
            }
            for (k, v) in &a.headers {
                if k.eq_ignore_ascii_case("content-type") {
                    continue;
                }
                resp.set_header_by_name(Some(&CefString::from(k.as_str())), Some(&CefString::from(v.as_str())), 0);
            }
            if let Some(len) = response_length {
                *len = a.body.len() as i64;
            }
        }

        fn read(&self, data_out: *mut u8, bytes_to_read: ::std::os::raw::c_int, bytes_read: Option<&mut ::std::os::raw::c_int>, _callback: Option<&mut ResourceReadCallback>) -> ::std::os::raw::c_int {
            let Ok(mut a) = self.answer.lock() else { return 0 };
            trace(|| format!("read want={bytes_to_read} have={}", a.body.len().saturating_sub(a.offset)));
            let remaining = a.body.len().saturating_sub(a.offset);
            let n = remaining.min(bytes_to_read.max(0) as usize);
            if let Some(out) = bytes_read {
                *out = n as i32;
            }
            if n == 0 || data_out.is_null() {
                return 0;
            }
            // SAFETY: CEF provides a buffer of at least `bytes_to_read` bytes.
            unsafe { std::ptr::copy_nonoverlapping(a.body.as_ptr().add(a.offset), data_out, n) };
            a.offset += n;
            1
        }
    }
}

/// The handler for a request of an extension page to a host it may access, or None to leave the request alone.
pub(crate) fn handler_for(initiator: &str, request: &mut Request, ctx: Option<RequestContext>) -> Option<ResourceHandler> {
    let url = CefString::from(&request.url()).to_string();
    if !is_exempt(initiator, &url) {
        return None;
    }
    trace(|| format!("handle {initiator} -> {url}"));
    let origin = initiator.trim_end_matches('/').to_string();
    let answer: Shared = Arc::new(Mutex::new(Answer::default()));
    let method = CefString::from(&request.method()).to_string();
    let job = if method.eq_ignore_ascii_case("OPTIONS") && header(request, "Access-Control-Request-Method").is_some() {
        // A preflight: 204 with what was asked for.
        let asked_headers = header(request, "Access-Control-Request-Headers").unwrap_or_default();
        let mut h = vec![("Access-Control-Allow-Methods".to_string(), "GET, POST, PUT, PATCH, DELETE, HEAD, OPTIONS".to_string()), ("Access-Control-Max-Age".to_string(), "600".to_string())];
        if !asked_headers.is_empty() {
            h.push(("Access-Control-Allow-Headers".to_string(), asked_headers));
        }
        if let Ok(mut a) = answer.lock() {
            a.status = 204;
            a.mime = "text/plain".into();
            a.headers = with_cors(h, &origin);
            a.done = true;
        }
        None
    } else {
        let copy = request_create()?;
        copy.set_url(Some(&CefString::from(url.as_str())));
        copy.set_method(Some(&CefString::from(method.as_str())));
        let mut all = CefStringMultimap::new();
        request.header_map(Some(&mut all));
        let mut map = CefStringMultimap::new();
        let mut shown = Vec::new();
        for (k, vs) in all.into_iter() {
            if forwarded(&k) {
                for v in vs {
                    map.append(&k, &v);
                    shown.push(format!("{k}: {v}"));
                }
            }
        }
        trace(|| format!("{method} request headers: {}", shown.join(" | ").chars().take(300).collect::<String>()));
        copy.set_header_map(Some(&mut map));
        if let Some(mut body) = request.post_data() {
            copy.set_post_data(Some(&mut body));
        }
        // Cookies of the page's profile go with the request and the response's cookies are kept.
        copy.set_flags(1 << 3 /* UR_FLAG_ALLOW_STORED_CREDENTIALS */);
        Some((copy, ctx))
    };
    Some(ProxyResource::new(answer, origin, Arc::new(Mutex::new(job)), Arc::new(Mutex::new(None))))
}

/// Request headers the inner request may carry: the browser sets Origin, Referer, Host, Cookie and the Sec-* ones itself.
fn forwarded(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    !(n.starts_with("sec-") || ["origin", "referer", "host", "content-length", "cookie", "connection", "accept-encoding", "access-control-request-method", "access-control-request-headers"].contains(&n.as_str()))
}

fn header(request: &Request, name: &str) -> Option<String> {
    let v = CefString::from(&request.header_by_name(Some(&CefString::from(name)))).to_string();
    (!v.is_empty()).then_some(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(l: &[(&str, &str, bool)]) -> Vec<(String, String, bool)> {
        l.iter().map(|(a, b, c)| (a.to_string(), b.to_string(), *c)).collect()
    }

    #[test]
    fn initiators_name_the_extension() {
        assert_eq!(extension_of_initiator("signet-extension://abc"), Some("abc"));
        assert_eq!(extension_of_initiator("chrome-extension://abc/"), Some("abc"));
        assert_eq!(extension_of_initiator("https://abc"), None);
        assert_eq!(extension_of_initiator("signet-extension://"), None);
        assert_eq!(extension_of_initiator(""), None);
    }

    #[test]
    fn targets_cover_only_the_hosts_they_name() {
        let all = t(&[("http", "", true), ("https", "", true)]);
        assert!(covers(&all, "https://example.com/x?y=1"));
        assert!(covers(&all, "http://127.0.0.1:8080/"));
        assert!(!covers(&all, "file:///etc/passwd"));
        assert!(!covers(&all, "wss://example.com/"));
        assert!(!covers(&all, "not a url"));
        let nord = t(&[("https", "nordvpn.com", true)]);
        assert!(covers(&nord, "https://api.nordvpn.com/v1"));
        assert!(covers(&nord, "https://nordvpn.com/"));
        assert!(!covers(&nord, "https://evilnordvpn.com/"));
        assert!(!covers(&nord, "http://api.nordvpn.com/"));
        let exact = t(&[("https", "api.example.com", false)]);
        assert!(covers(&exact, "https://API.example.com/a"));
        assert!(!covers(&exact, "https://x.api.example.com/a"));
        assert!(!covers(&[], "https://example.com/"));
    }

    #[test]
    fn only_extensions_with_entries_are_exempt() {
        set_cors_exemption("idone", &t(&[("https", "example.com", false)]), true);
        assert!(is_exempt("signet-extension://idone", "https://example.com/a"));
        assert!(is_exempt("chrome-extension://idone/", "https://example.com/a"));
        assert!(!is_exempt("signet-extension://idone", "https://other.com/a"));
        assert!(!is_exempt("signet-extension://idtwo", "https://example.com/a"));
        assert!(!is_exempt("https://example.com", "https://example.com/a"));
        set_cors_exemption("idone", &[], false);
        assert!(!is_exempt("signet-extension://idone", "https://example.com/a"));
    }

    #[test]
    fn browser_owned_request_headers_are_not_forwarded() {
        for h in ["Origin", "referer", "Sec-Fetch-Mode", "Host", "Content-Length", "Cookie"] {
            assert!(!forwarded(h), "{h}");
        }
        for h in ["Content-Type", "X-Custom", "Authorization", "Accept"] {
            assert!(forwarded(h), "{h}");
        }
    }

    #[test]
    fn cors_headers_replace_the_originals() {
        let h = with_cors(vec![("Content-Length".into(), "5".into()), ("X-A".into(), "1".into()), ("access-control-allow-origin".into(), "*".into())], "signet-extension://i");
        assert!(h.iter().any(|(k, v)| k == "Access-Control-Allow-Origin" && v == "signet-extension://i"));
        assert_eq!(h.iter().filter(|(k, _)| k.eq_ignore_ascii_case("access-control-allow-origin")).count(), 1);
        assert!(h.iter().any(|(k, _)| k == "X-A"));
        assert!(!h.iter().any(|(k, _)| k.eq_ignore_ascii_case("content-length")));
    }
}

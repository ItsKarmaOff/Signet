//! Chrome DevTools Protocol access: `Browser.ExecuteDevToolsMethod` style calls with callbacks,
//! subscribed events, and the helpers built on them (screenshots, emulation, MHTML...).

use std::collections::{HashMap, HashSet};

use cef::*;

use crate::runtime::{push_event, with_state};
use crate::types::*;

pub(crate) type Done = Box<dyn FnOnce(Result<String, String>)>;

#[derive(Default)]
pub(crate) struct CdpView {
    next_id: i32,
    pending: HashMap<i32, Done>,
    subscribed: HashSet<String>,
    registration: Option<Registration>,
}

wrap_dev_tools_message_observer! {
    struct SignetDevTools { view: ViewId }

    impl DevToolsMessageObserver {
        fn on_dev_tools_message(&self, _browser: Option<&mut Browser>, message: Option<&[u8]>) -> ::std::os::raw::c_int {
            let Some(bytes) = message else { return 0 };
            if bytes.starts_with(b"{\"id\":") {
                let Ok(v) = serde_json::from_slice::<serde_json::Value>(bytes) else { return 0 };
                let Some(id) = v["id"].as_i64() else { return 0 };
                let done = with_state(|st| st.cdp.get_mut(&self.view).and_then(|c| c.pending.remove(&(id as i32)))).flatten();
                if let Some(done) = done {
                    let result = if let Some(e) = v.get("error") {
                        Err(e["message"].as_str().unwrap_or("devtools error").to_string())
                    } else {
                        Ok(v["result"].to_string())
                    };
                    // Not run here: CDP observers must not send commands re-entrantly. See `run_ready`.
                    with_state(|st| st.cdp_ready.push((done, result)));
                }
            } else if let Some(method) = event_method(bytes) {
                let wanted = with_state(|st| st.cdp.get(&self.view).map(|c| c.subscribed.contains(&method))).flatten().unwrap_or(false);
                if wanted {
                    let params = serde_json::from_slice::<serde_json::Value>(bytes)
                        .map(|v| v["params"].to_string())
                        .unwrap_or_default();
                    push_event(WebEvent::DevToolsEvent { view: self.view, method, params });
                }
            }
            0
        }
    }
}

fn event_method(bytes: &[u8]) -> Option<String> {
    const KEY: &[u8] = b"\"method\":\"";
    let start = bytes.windows(KEY.len()).take(64).position(|w| w == KEY)? + KEY.len();
    let end = bytes[start..].iter().position(|b| *b == b'"')?;
    String::from_utf8(bytes[start..start + end].to_vec()).ok()
}

/// Register the observer of a view on first use (and keep it for the view's life, which keeps `Emulation.*` overrides
/// alive). Not done at creation: an attached DevTools agent makes Chromium reset the page view to its default size
/// about 250 ms after every navigation, which blanks the page for a frame (the "flash" on each load and reload).
pub(crate) fn ensure_attached(view: ViewId, host: &BrowserHost) {
    let attached = with_state(|st| st.cdp.get(&view).is_some_and(|c| c.registration.is_some())).unwrap_or(true);
    if attached {
        return;
    }
    crate::view::tlog(|| format!("CDP ensure_attached view={} (first use)", view.0));
    let mut obs = SignetDevTools::new(view);
    let reg = host.add_dev_tools_message_observer(Some(&mut obs));
    with_state(|st| {
        let c = st.cdp.entry(view).or_default();
        c.registration = reg;
    });
}

/// Run the callbacks of CDP results received during the last pump. Called by `drain_events`, outside
/// any CEF callback, so a callback may send new commands.
pub(crate) fn run_ready() {
    loop {
        let ready = with_state(|st| std::mem::take(&mut st.cdp_ready)).unwrap_or_default();
        if ready.is_empty() {
            break;
        }
        for (done, result) in ready {
            done(result);
        }
    }
}

/// Drop the state of a closed view; pending calls fail.
pub(crate) fn detach(view: ViewId) {
    let pending: Vec<Done> = with_state(|st| st.cdp.remove(&view).map(|c| c.pending.into_values().collect())).flatten().unwrap_or_default();
    with_state(|st| st.cdp_ready.extend(pending.into_iter().map(|p| (p, Err("view closed".to_string())))));
}

pub(crate) fn subscribe(view: ViewId, methods: &[&str], on: bool) {
    if on && let Some(h) = crate::view::host_of(view) {
        ensure_attached(view, &h);
    }
    with_state(|st| {
        let c = st.cdp.entry(view).or_default();
        for m in methods {
            if on { c.subscribed.insert((*m).to_string()); } else { c.subscribed.remove(*m); }
        }
    });
}

/// Send a CDP command. `done` runs on the main thread when the result (JSON of `result`) or error arrives.
pub(crate) fn send(host: &BrowserHost, view: ViewId, method: &str, params_json: &str, done: Done) {
    let params: serde_json::Value = if params_json.trim().is_empty() {
        serde_json::json!({})
    } else {
        match serde_json::from_str(params_json) {
            Ok(v) => v,
            Err(e) => return done(Err(format!("invalid params JSON: {e}"))),
        }
    };
    let id = with_state(|st| {
        let c = st.cdp.entry(view).or_default();
        c.next_id += 1;
        c.next_id
    });
    let Some(id) = id else { return done(Err("not initialized".into())) };
    crate::view::tlog(|| format!("CDP send view={} {}", view.0, method));
    ensure_attached(view, host);
    let msg = serde_json::json!({"id": id, "method": method, "params": params}).to_string();
    with_state(|st| st.cdp.entry(view).or_default().pending.insert(id, done));
    if host.send_dev_tools_message(Some(msg.as_bytes())) != 1 {
        let d = with_state(|st| st.cdp.get_mut(&view).and_then(|c| c.pending.remove(&id))).flatten();
        if let Some(d) = d {
            d(Err("cannot send the DevTools message".into()));
        }
    }
}

/// Standard base64 decoder (the `data` of screenshots and snapshots).
pub(crate) fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' | b'\n' | b'\r' | b' ' => continue,
            _ => return None,
        };
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64() {
        assert_eq!(base64_decode("aGVsbG8gd29ybGQ=").unwrap(), b"hello world");
        assert_eq!(base64_decode("aGk=").unwrap(), b"hi");
        assert!(base64_decode("a$b").is_none());
    }

    #[test]
    fn method_of_event() {
        assert_eq!(event_method(br#"{"method":"Media.x","params":{}}"#).as_deref(), Some("Media.x"));
        assert_eq!(event_method(br#"{"id":1,"result":{}}"#), None);
    }
}

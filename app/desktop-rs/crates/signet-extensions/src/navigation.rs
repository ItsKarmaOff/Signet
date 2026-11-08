//! `webNavigation` events the browser layer does not give extensions: errors and fragment changes. Port of
//! `apis/navigation.ts`. The app reports navigations of tabs; the model emits events to running extension contexts only
//! (waking a stopped worker for every page load would start every extension's worker each time).

use crate::common::Emitter;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::sync::Arc;
use url::Url;

pub const ERROR_EVENT: &str = "webNavigation.onErrorOccurred";
pub const FRAGMENT_EVENT: &str = "webNavigation.onReferenceFragmentUpdated";

/// Chrome's text for a network error: `net::ERR_NAME_NOT_RESOLVED` (CEF gives the `ERR_` name).
pub fn error_string(code: i64, description: &str) -> String {
    if description.starts_with("net::") {
        return description.to_string();
    }
    if description.starts_with("ERR_") {
        format!("net::{description}")
    } else {
        format!("net::ERR_FAILED ({code})")
    }
}

/// A navigation inside a document that only changed the `#fragment`.
pub fn is_fragment_change(previous: Option<&str>, next: &str) -> bool {
    let Some(prev) = previous.filter(|p| !p.is_empty()) else { return false };
    let (Ok(mut a), Ok(mut b)) = (Url::parse(prev), Url::parse(next)) else { return false };
    let (ha, hb) = (a.fragment().map(String::from), b.fragment().map(String::from));
    a.set_fragment(None);
    b.set_fragment(None);
    a.as_str() == b.as_str() && ha != hb
}

/// How `webNavigation` names a frame (the main frame is 0).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameInfo {
    pub frame_id: i64,
    pub parent_frame_id: i64,
    pub sub_frame: bool,
}

impl FrameInfo {
    pub fn main() -> Self {
        FrameInfo { frame_id: 0, parent_frame_id: -1, sub_frame: false }
    }
}

#[derive(Default)]
struct TabState {
    /// Last URL per frame key.
    last: HashMap<String, String>,
    main_url: String,
}

pub struct NavigationExtras {
    emitter: Arc<dyn Emitter>,
    tabs: HashMap<i64, TabState>,
}

impl NavigationExtras {
    pub fn new(emitter: Arc<dyn Emitter>) -> Self {
        NavigationExtras { emitter, tabs: HashMap::new() }
    }

    /// Declares a tab (idempotent) with its current URL.
    pub fn watch(&mut self, tab: i64, url: &str) {
        self.tabs.entry(tab).or_insert_with(|| TabState { last: HashMap::new(), main_url: url.to_string() });
    }

    pub fn forget(&mut self, tab: i64) {
        self.tabs.remove(&tab);
    }

    fn remember(state: &mut TabState, frame_key: &str, main: bool, url: &str) {
        state.last.insert(frame_key.to_string(), url.to_string());
        if main {
            state.main_url = url.to_string();
            state.last.insert("main".into(), url.to_string());
        }
    }

    /// A full navigation of a frame committed (resets the reference URL).
    pub fn frame_navigated(&mut self, tab: i64, frame_key: &str, main: bool, url: &str) {
        if let Some(s) = self.tabs.get_mut(&tab) {
            Self::remember(s, frame_key, main, url);
        }
    }

    /// A navigation inside the document (history API or fragment). `frame` is None when the frame is gone.
    pub fn navigated_in_page(&mut self, tab: i64, frame_key: &str, main: bool, url: &str, frame: Option<&FrameInfo>, now_ms: u64) {
        let Some(s) = self.tabs.get_mut(&tab) else { return };
        let previous = if main { s.last.get("main").cloned().or_else(|| Some(s.main_url.clone())) } else { s.last.get(frame_key).cloned() };
        Self::remember(s, frame_key, main, url);
        if is_fragment_change(previous.as_deref(), url) {
            self.send(tab, FRAGMENT_EVENT, frame, url, json!({ "transitionType": "link", "transitionQualifiers": [] }), now_ms);
        }
    }

    /// A load failed. Cancelled navigations, downloads and stops are reported with `net::ERR_ABORTED` too, as in Chrome.
    pub fn load_failed(&mut self, tab: i64, code: i64, description: &str, url: &str, frame: Option<&FrameInfo>, now_ms: u64) {
        self.send(tab, ERROR_EVENT, frame, url, json!({ "error": error_string(code, description) }), now_ms);
    }

    fn send(&self, tab: i64, event: &str, frame: Option<&FrameInfo>, url: &str, extra: Value, now_ms: u64) {
        let Some(f) = frame else { return };
        let mut details = Map::new();
        details.insert("frameId".into(), json!(f.frame_id));
        details.insert("parentFrameId".into(), json!(f.parent_frame_id));
        details.insert("frameType".into(), json!(if f.sub_frame { "sub_frame" } else { "outermost_frame" }));
        details.insert("documentLifecycle".into(), json!("active"));
        details.insert("tabId".into(), json!(tab));
        details.insert("timeStamp".into(), json!(now_ms));
        details.insert("url".into(), json!(url));
        if let Value::Object(e) = extra {
            details.extend(e);
        }
        let details = Value::Object(details);
        for id in self.emitter.extensions_with("webNavigation") {
            self.emitter.emit(&id, event, vec![details.clone()], false, true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::Outbox;

    #[test]
    fn fragment_change_only_when_the_fragment_is_the_sole_difference() {
        assert!(is_fragment_change(Some("https://a/p"), "https://a/p#x"));
        assert!(is_fragment_change(Some("https://a/p#x"), "https://a/p#y"));
        assert!(is_fragment_change(Some("https://a/p#x"), "https://a/p"));
        assert!(!is_fragment_change(Some("https://a/p#x"), "https://a/p#x"));
        assert!(!is_fragment_change(Some("https://a/p"), "https://a/q#x"));
        assert!(!is_fragment_change(Some("https://a/p?x=1"), "https://a/p?x=2#y"));
        assert!(!is_fragment_change(None, "https://a/p#x"));
        assert!(!is_fragment_change(Some("nope"), "https://a/p#x"));
    }

    #[test]
    fn writes_chrome_net_error_names() {
        assert_eq!(error_string(-105, "ERR_NAME_NOT_RESOLVED"), "net::ERR_NAME_NOT_RESOLVED");
        assert_eq!(error_string(-105, "net::ERR_X"), "net::ERR_X");
        assert_eq!(error_string(-999, "weird"), "net::ERR_FAILED (-999)");
    }

    fn setup() -> (NavigationExtras, Arc<Outbox>) {
        let out = Arc::new(Outbox::new());
        out.set_holders("webNavigation", vec!["ext1".into(), "ext2".into()]);
        let mut nav = NavigationExtras::new(out.clone());
        nav.watch(4, "https://a/p");
        (nav, out)
    }
    fn sub() -> FrameInfo {
        FrameInfo { frame_id: 5, parent_frame_id: 0, sub_frame: true }
    }

    #[test]
    fn fragment_updates_go_to_extensions_with_webnavigation_without_waking() {
        let (mut nav, out) = setup();
        nav.navigated_in_page(4, "main", true, "https://a/p#sec", Some(&FrameInfo::main()), 123);
        let ev = out.drain();
        assert_eq!(ev.len(), 2);
        assert_eq!((ev[0].extension_id.as_str(), ev[0].name.as_str(), ev[0].wake), ("ext1", FRAGMENT_EVENT, false));
        let d = &ev[0].args[0];
        assert_eq!((d["tabId"].clone(), d["frameId"].clone(), d["parentFrameId"].clone(), d["url"].clone(), d["timeStamp"].clone(), d["transitionType"].clone()), (json!(4), json!(0), json!(-1), json!("https://a/p#sec"), json!(123), json!("link")));
    }

    #[test]
    fn history_api_changes_and_repeated_fragments_are_not_reported() {
        let (mut nav, out) = setup();
        let f = FrameInfo::main();
        nav.navigated_in_page(4, "main", true, "https://a/other", Some(&f), 1);
        nav.navigated_in_page(4, "main", true, "https://a/other#a", Some(&f), 1);
        nav.navigated_in_page(4, "main", true, "https://a/other#a", Some(&f), 1);
        assert_eq!(out.drain().len(), 2);
    }

    #[test]
    fn a_full_navigation_resets_the_reference_url() {
        let (mut nav, out) = setup();
        nav.frame_navigated(4, "main", true, "https://b/z");
        nav.navigated_in_page(4, "main", true, "https://b/z#k", Some(&FrameInfo::main()), 1);
        assert_eq!(out.drain()[0].args[0]["url"], "https://b/z#k");
    }

    #[test]
    fn errors_are_reported_including_aborted_and_skip_frames_that_are_gone() {
        let (mut nav, out) = setup();
        nav.load_failed(4, -105, "ERR_NAME_NOT_RESOLVED", "https://nope.test/", None, 1);
        assert!(out.drain().is_empty());
        nav.load_failed(4, -3, "ERR_ABORTED", "https://a/x", Some(&FrameInfo::main()), 1);
        let ev = out.drain();
        assert_eq!(ev.len(), 2);
        assert_eq!((ev[0].name.as_str(), ev[0].args[0]["error"].clone(), ev[0].args[0]["frameId"].clone(), ev[0].wake), (ERROR_EVENT, json!("net::ERR_ABORTED"), json!(0), false));
        nav.load_failed(4, -105, "ERR_NAME_NOT_RESOLVED", "https://nope.test/", Some(&sub()), 1);
        let ev = out.drain();
        let d = &ev[1].args[0];
        assert_eq!((d["error"].clone(), d["frameId"].clone(), d["parentFrameId"].clone(), d["frameType"].clone(), d["url"].clone()), (json!("net::ERR_NAME_NOT_RESOLVED"), json!(5), json!(0), json!("sub_frame"), json!("https://nope.test/")));
    }

    #[test]
    fn watching_a_tab_twice_keeps_one_state() {
        let (mut nav, out) = setup();
        nav.watch(4, "https://elsewhere/");
        nav.navigated_in_page(4, "main", true, "https://a/p#x", Some(&FrameInfo::main()), 1);
        assert_eq!(out.drain().len(), 2);
    }
}

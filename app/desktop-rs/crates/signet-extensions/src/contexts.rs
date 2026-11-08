//! `runtime.getContexts`, the placeholder registry and `chrome.offscreen`. Port of `apis/contexts.ts`, `apis/stubs.ts` and
//! `apis/offscreen.ts`.

use crate::common::{ApiError, ApiResult};
use crate::shared::OFFSCREEN_REASONS;
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap};
use url::Url;

/// `chrome.runtime.ExtensionContext` as JSON.
pub type ExtensionContext = Value;

/// Context of the extension's background (service worker or background page).
pub fn background_context(extension_id: &str, worker_url: &str) -> ExtensionContext {
    json!({
        "contextType": "BACKGROUND",
        "contextId": format!("{extension_id}-background"),
        "tabId": -1,
        "windowId": -1,
        "frameId": -1,
        "documentUrl": worker_url,
        "documentOrigin": format!("chrome-extension://{extension_id}"),
        "incognito": false
    })
}

/// Context of an open extension page (popup, options tab, offscreen document).
pub fn page_context(extension_id: &str, page_id: i64, url: &str, kind: &str) -> ExtensionContext {
    json!({
        "contextType": kind,
        "contextId": format!("{extension_id}-page-{page_id}"),
        "tabId": -1,
        "windowId": -1,
        "documentId": page_id.to_string(),
        "frameId": 0,
        "documentUrl": url,
        "documentOrigin": format!("chrome-extension://{extension_id}"),
        "incognito": false
    })
}

fn matches(wanted: Option<&Value>, value: Option<&Value>) -> bool {
    match wanted.and_then(|w| w.as_array()) {
        None => true,
        Some(list) => value.is_some_and(|v| list.contains(v)),
    }
}

/// Applies a ContextFilter like Chrome: every given property must match; a missing filter keeps all.
pub fn filter_contexts(contexts: Vec<ExtensionContext>, filter: &Value) -> Vec<ExtensionContext> {
    if !filter.is_object() {
        return contexts;
    }
    contexts
        .into_iter()
        .filter(|c| {
            matches(filter.get("contextTypes"), c.get("contextType"))
                && matches(filter.get("contextIds"), c.get("contextId"))
                && matches(filter.get("tabIds"), c.get("tabId"))
                && matches(filter.get("windowIds"), c.get("windowId"))
                && matches(filter.get("documentIds"), c.get("documentId"))
                && matches(filter.get("frameIds"), c.get("frameId"))
                && matches(filter.get("documentUrls"), c.get("documentUrl"))
                && matches(filter.get("documentOrigins"), c.get("documentOrigin"))
                && filter.get("incognito").and_then(|i| i.as_bool()).is_none_or(|i| Some(i) == c.get("incognito").and_then(|v| v.as_bool()))
        })
        .collect()
}

/// What the safety net replaced by placeholders, per extension version (reported by the page script of each context).
/// Kept in memory for a later "Some features of this extension are not available in Signet" line in Settings.
#[derive(Default)]
pub struct StubRegistry {
    seen: HashMap<String, BTreeSet<String>>,
}

impl StubRegistry {
    /// Merges a report. Returns true the first time this extension version reports (the caller logs only then).
    pub fn record(&mut self, extension_id: &str, version: &str, names: &[String]) -> bool {
        let key = format!("{extension_id}@{version}");
        let first = !self.seen.contains_key(&key);
        self.seen.entry(key).or_default().extend(names.iter().cloned());
        first
    }

    /// Sorted names for an extension (every version recorded so far, or one version).
    pub fn of(&self, extension_id: &str, version: Option<&str>) -> Vec<String> {
        if let Some(v) = version {
            return self.seen.get(&format!("{extension_id}@{v}")).map(|s| s.iter().cloned().collect()).unwrap_or_default();
        }
        let prefix = format!("{extension_id}@");
        self.seen.iter().filter(|(k, _)| k.starts_with(&prefix)).flat_map(|(_, s)| s.iter().cloned()).collect::<BTreeSet<_>>().into_iter().collect()
    }
}

struct Doc {
    url: String,
    #[allow(dead_code)]
    reasons: Vec<String>,
    /// Page id once the web layer created it.
    page_id: Option<i64>,
}

/// `chrome.offscreen`: at most one hidden document per extension, loaded from the extension's own origin. It stays until
/// the extension closes it (or is unloaded). The web layer creates a hidden browser that is never attached to a window.
///
/// Flow: [`Offscreen::create`] validates and reserves the slot, returning the URL to load; the web layer opens the page
/// and calls [`Offscreen::loaded`] (or [`Offscreen::load_failed`]), which completes the extension's call.
#[derive(Default)]
pub struct Offscreen {
    docs: HashMap<String, Doc>,
}

impl Offscreen {
    pub fn create(&mut self, extension_id: &str, params: &Value) -> ApiResult<String> {
        let url = params.get("url").and_then(|u| u.as_str()).filter(|u| !u.is_empty());
        let Some(url) = url else { return Err(ApiError::new("Invalid value for argument 1. Property 'url': Expected 'string'.")) };
        let reasons = params.get("reasons").and_then(|r| r.as_array()).filter(|r| !r.is_empty() && r.iter().all(|x| x.as_str().is_some_and(|x| OFFSCREEN_REASONS.contains(&x))));
        let Some(reasons) = reasons else {
            return Err(ApiError::new("Invalid value for argument 1. Property 'reasons': Expected a non-empty array of offscreen reasons."));
        };
        if params.get("justification").and_then(|j| j.as_str()).is_none_or(str::is_empty) {
            return Err(ApiError::new("Invalid value for argument 1. Property 'justification': Expected 'string'."));
        }
        let base = Url::parse(&format!("chrome-extension://{extension_id}/")).map_err(|_| ApiError::new("Invalid URL."))?;
        let resolved = base.join(url).map_err(|_| ApiError::new("Invalid URL."))?;
        if resolved.scheme() != "chrome-extension" || resolved.host_str() != Some(extension_id) {
            return Err(ApiError::new("Offscreen documents must be pages of the extension."));
        }
        if self.docs.contains_key(extension_id) {
            return Err(ApiError::new("Only a single offscreen document may be created."));
        }
        let href = resolved.as_str().to_string();
        // Reserve the slot before loading: a second call while this one loads must be refused.
        self.docs.insert(extension_id.to_string(), Doc { url: href.clone(), reasons: reasons.iter().filter_map(|r| r.as_str().map(String::from)).collect(), page_id: None });
        Ok(href)
    }

    /// The web layer opened the page (hidden browser id) and it loaded.
    pub fn loaded(&mut self, extension_id: &str, page_id: i64) {
        if let Some(d) = self.docs.get_mut(extension_id) {
            d.page_id = Some(page_id);
        }
    }

    /// The load failed: the slot is freed. Returns the error to give the extension; the web layer closes the page.
    pub fn load_failed(&mut self, extension_id: &str, reason: &str) -> ApiError {
        self.docs.remove(extension_id);
        ApiError::new(format!("Failed to load the offscreen document: {reason}"))
    }

    /// `closeDocument`: returns the page id to close (None while it is still loading).
    pub fn close(&mut self, extension_id: &str) -> ApiResult<Option<i64>> {
        let doc = self.docs.remove(extension_id).ok_or_else(|| ApiError::new("No current offscreen document."))?;
        Ok(doc.page_id)
    }

    pub fn has(&self, extension_id: &str) -> bool {
        self.docs.contains_key(extension_id)
    }

    /// Extension unloaded: its document goes (no error if there is none). Returns the page id to close.
    pub fn close_for(&mut self, extension_id: &str) -> Option<i64> {
        self.docs.remove(extension_id).and_then(|d| d.page_id)
    }

    /// The page went away by itself.
    pub fn page_closed(&mut self, page_id: i64) {
        self.docs.retain(|_, d| d.page_id != Some(page_id));
    }

    /// True for the page of an offscreen document (so it is not reported as a tab).
    pub fn owns(&self, page_id: i64) -> bool {
        self.docs.values().any(|d| d.page_id == Some(page_id))
    }

    pub fn contexts(&self, extension_id: &str) -> Vec<ExtensionContext> {
        match self.docs.get(extension_id) {
            Some(Doc { url, page_id: Some(id), .. }) => vec![page_context(extension_id, *id, url, "OFFSCREEN_DOCUMENT")],
            _ => vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    #[test]
    fn filter_keeps_everything_without_a_filter() {
        let worker = background_context(A, &format!("chrome-extension://{A}/sw.js"));
        let popup = page_context(A, 5, &format!("chrome-extension://{A}/popup.html"), "POPUP");
        assert_eq!(filter_contexts(vec![worker.clone(), popup.clone()], &Value::Null).len(), 2);
        assert_eq!(filter_contexts(vec![worker.clone(), popup.clone()], &json!({})).len(), 2);
        assert_eq!(filter_contexts(vec![worker.clone(), popup.clone()], &json!({ "contextTypes": ["BACKGROUND"] })), vec![worker.clone()]);
        let url = popup["documentUrl"].clone();
        assert_eq!(filter_contexts(vec![worker.clone(), popup.clone()], &json!({ "contextTypes": ["POPUP", "TAB"], "documentUrls": [url] })), vec![popup.clone()]);
        assert!(filter_contexts(vec![worker.clone(), popup.clone()], &json!({ "contextTypes": ["OFFSCREEN_DOCUMENT"] })).is_empty());
        assert!(filter_contexts(vec![worker.clone(), popup.clone()], &json!({ "incognito": true })).is_empty());
        assert!(filter_contexts(vec![worker, popup], &json!({ "tabIds": [3] })).is_empty());
    }

    #[test]
    fn stub_registry_reports_first_once_per_version_and_merges() {
        let mut r = StubRegistry::default();
        assert!(r.record("a", "1.0", &["tabs.group".into()]));
        assert!(!r.record("a", "1.0", &["userScripts".into(), "tabs.group".into()]));
        assert!(r.record("a", "2.0", &[]));
        assert_eq!(r.of("a", Some("1.0")), ["tabs.group", "userScripts"]);
        assert_eq!(r.of("a", None), ["tabs.group", "userScripts"]);
        assert!(r.of("b", None).is_empty());
    }

    fn params(url: &str) -> Value {
        json!({ "url": url, "reasons": ["LOCAL_STORAGE", "DOM_PARSER"], "justification": "dom" })
    }

    #[test]
    fn creates_one_document_resolved_against_the_extension() {
        let mut o = Offscreen::default();
        let url = o.create(A, &params("offscreen.html")).unwrap();
        assert_eq!(url, format!("chrome-extension://{A}/offscreen.html"));
        o.loaded(A, 40);
        assert!(o.has(A) && o.owns(40));
        let ctx = o.contexts(A);
        assert_eq!(ctx[0]["contextType"], "OFFSCREEN_DOCUMENT");
        assert_eq!(ctx[0]["documentUrl"], url);
        assert_eq!(ctx[0]["documentOrigin"], format!("chrome-extension://{A}"));
    }

    #[test]
    fn refuses_a_second_document_a_foreign_origin_and_bad_parameters() {
        let mut o = Offscreen::default();
        o.create(A, &params("offscreen.html")).unwrap();
        assert!(o.create(A, &params("offscreen.html")).unwrap_err().0.contains("single offscreen document"));
        let mut o = Offscreen::default();
        assert!(o.create(A, &params("https://evil.example/x.html")).unwrap_err().0.contains("pages of the extension"));
        assert!(o.create(A, &params(&format!("chrome-extension://{}/x.html", "b".repeat(32)))).unwrap_err().0.contains("pages of the extension"));
        assert!(o.create(A, &json!({ "url": "a.html", "reasons": [], "justification": "x" })).unwrap_err().0.contains("reasons"));
        assert!(o.create(A, &json!({ "url": "a.html", "reasons": ["NOPE"], "justification": "x" })).unwrap_err().0.contains("reasons"));
        assert!(o.create(A, &json!({ "url": "a.html", "reasons": ["TESTING"] })).unwrap_err().0.contains("justification"));
    }

    #[test]
    fn close_close_for_and_the_page_closing_free_the_slot() {
        let mut o = Offscreen::default();
        o.create(A, &params("offscreen.html")).unwrap();
        o.loaded(A, 40);
        assert_eq!(o.close(A).unwrap(), Some(40));
        assert!(!o.has(A));
        assert!(o.close(A).unwrap_err().0.contains("No current offscreen document"));
        assert_eq!(o.close_for(A), None);
        o.create(A, &params("offscreen.html")).unwrap();
        o.loaded(A, 41);
        o.page_closed(41);
        assert!(!o.has(A));
    }

    #[test]
    fn a_failed_load_frees_the_slot() {
        let mut o = Offscreen::default();
        o.create(A, &params("offscreen.html")).unwrap();
        let err = o.load_failed(A, "ERR_FILE_NOT_FOUND");
        assert!(err.0.contains("ERR_FILE_NOT_FOUND"));
        assert!(!o.has(A));
    }
}

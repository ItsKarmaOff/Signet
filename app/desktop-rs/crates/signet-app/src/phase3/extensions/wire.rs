//! Hooks that run on CEF helper threads: the scheme handler, the per-document program (page scripts, content scripts)
//! and the request filter. They only touch `Shared`; the host is locked for short moments.

use std::collections::HashSet;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use signet_extensions::crx::is_store_url;
use signet_extensions::dnr_engine::{RequestDetails, RequestOutcome};
use signet_extensions::host::Frame;
use signet_web::{FrameInfo, RequestDecision, RequestInfo};

use super::bridge::{content_wrapper, extension_id_of, frame_number, injection_js, BG_MARKER, POPUP_SIZE_SCRIPT, STORE_CSS_SCRIPT};
use super::ctx::Kind;
use super::files;
use super::shared::Shared;
use crate::phase3::hooks;

const HOST_WAIT: Duration = Duration::from_millis(400);

pub fn install(shared: &Arc<Shared>) {
    let s = shared.clone();
    signet_web::set_extension_handler(move |req| {
        let loaded = s.loaded.read().ok()?;
        files::serve(&loaded, req)
    });
    let s = shared.clone();
    hooks::add_bundle_part(200, move |f| bundle(&s, f));
    let s = shared.clone();
    hooks::add_request_hook(100, move |r| request(&s, r));
}

fn is_page_url(url: &str) -> bool {
    ["http://", "https://", "file://", "ftp://"].iter().any(|p| url.starts_with(p))
}

/// The JavaScript to run first in a frame, or `None`.
pub fn bundle(s: &Shared, f: &FrameInfo) -> Option<String> {
    if let Some(ext) = extension_id_of(&f.url) {
        if !s.loaded.read().ok()?.contains_key(&ext) {
            return None;
        }
        let kind = if f.is_main_frame && f.url.ends_with(BG_MARKER) { Kind::Worker } else { Kind::Page };
        let ctx = s.ctxs().open(&ext, kind, f.view, &f.frame_id, &f.url);
        let mut out = s.host_within(HOST_WAIT)?.page_script(&ext, kind.name(), &ctx.id, None, frame_number(&f.frame_id, f.is_main_frame))?;
        if f.is_main_frame && s.popup_view.load(Ordering::Relaxed) == f.view.0 {
            out.push_str("\n;\n");
            out.push_str(POPUP_SIZE_SCRIPT);
        }
        return Some(out);
    }
    if is_store_url(&f.url) && f.is_main_frame {
        return Some(format!("{}\n;\n{}", signet_extensions::assets::webstore_script(), STORE_CSS_SCRIPT));
    }
    if f.is_private || !is_page_url(&f.url) {
        return None;
    }
    content_scripts(s, f)
}

fn content_scripts(s: &Shared, f: &FrameInfo) -> Option<String> {
    let host = s.host_within(HOST_WAIT)?;
    let injections = host.injections(&Frame { url: &f.url, is_main_frame: f.is_main_frame, fallback_url: None }, None);
    if injections.is_empty() {
        return None;
    }
    let mut out = String::new();
    let mut done: HashSet<String> = HashSet::new();
    let frame = frame_number(&f.frame_id, f.is_main_frame);
    for first in &injections {
        if !done.insert(first.extension_id.clone()) {
            continue;
        }
        let ctx = s.ctxs().open(&first.extension_id, Kind::Content, f.view, &f.frame_id, &f.url);
        let Some(script) = host.page_script(&first.extension_id, "content", &ctx.id, None, frame) else { continue };
        out.push_str(&content_wrapper(&first.extension_id, &ctx.id, &script));
        out.push('\n');
        for inj in injections.iter().filter(|i| i.extension_id == first.extension_id) {
            out.push_str(&injection_js(inj));
            out.push('\n');
        }
    }
    (!out.is_empty()).then_some(out)
}

fn request(s: &Shared, r: &RequestInfo) -> Option<RequestDecision> {
    if !s.dnr_on.load(Ordering::Relaxed) || !["http://", "https://", "ws://", "wss://", "ftp://"].iter().any(|p| r.url.starts_with(p)) {
        return None;
    }
    let tab_id = s.view_tab.read().ok().and_then(|m| m.get(&r.view).copied());
    let details = RequestDetails {
        url: r.url.clone(),
        method: r.method.clone(),
        resource_type: Some(r.resource_type.dnr_name().to_string()),
        tab_id,
        frame_origin: Some(r.initiator.clone()).filter(|i| !i.is_empty()),
        ..Default::default()
    };
    let mut host = s.host_within(Duration::from_millis(20))?;
    match host.before_request(&details) {
        RequestOutcome::Pass => None,
        RequestOutcome::Cancel => Some(RequestDecision::Block),
        RequestOutcome::Redirect(url) => Some(RequestDecision::Redirect(url)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use signet_extensions::host::{Effect, ExtensionHost, HostConfig};
    use signet_web::{ProfileKind, ViewId};

    fn frame(url: &str, main: bool, private: bool) -> FrameInfo {
        FrameInfo { view: ViewId(5), profile: ProfileKind::persistent("default"), url: url.into(), is_main_frame: main, is_private: private, frame_id: "7".into() }
    }

    /// Checks the syntax with Node when it is installed.
    fn assert_parses(js: &str) {
        let Ok(dir) = tempfile::tempdir() else { return };
        let file = dir.path().join("bundle.js");
        std::fs::write(&file, js).unwrap();
        match std::process::Command::new("node").arg("--check").arg(&file).output() {
            Ok(o) => assert!(o.status.success(), "syntax error: {}", String::from_utf8_lossy(&o.stderr)),
            Err(_) => {}
        }
    }

    /// A shared state with one unpacked extension: a popup, a service worker and a content script for example.com.
    fn with_extension() -> (tempfile::TempDir, tempfile::TempDir, Shared, String) {
        let data = tempfile::tempdir().unwrap();
        let ext = tempfile::tempdir().unwrap();
        let manifest = serde_json::json!({
            "manifest_version": 3, "name": "Probe", "version": "1.0",
            "background": { "service_worker": "sw.js" },
            "action": { "default_popup": "popup.html" },
            "content_scripts": [{ "matches": ["https://example.com/*"], "js": ["content.js"], "run_at": "document_end" }]
        });
        std::fs::write(ext.path().join("manifest.json"), manifest.to_string()).unwrap();
        std::fs::write(ext.path().join("content.js"), "window.__ran = 42;").unwrap();
        let mut host = ExtensionHost::new(HostConfig::new(data.path()));
        let fx = host.add_unpacked(ext.path()).unwrap();
        let shared = Shared::new(host);
        let mut id = String::new();
        for e in fx {
            if let Effect::LoadExtension { id: i, dir, manifest, background } = e {
                id = i.clone();
                shared.loaded.write().unwrap().insert(i, files::Loaded { dir, manifest: signet_extensions::manifest::Manifest::from_value(manifest).unwrap(), background });
            }
        }
        assert!(!id.is_empty(), "the extension was not loaded");
        (data, ext, shared, id)
    }

    #[test]
    fn extension_frames_get_their_page_script_and_content_scripts_get_a_private_bridge() {
        let (_d, _e, shared, id) = with_extension();
        let popup = bundle(&shared, &frame(&format!("chrome-extension://{id}/popup.html"), true, false)).unwrap();
        assert!(popup.contains("__signetExtensionConfig") && popup.contains("\"kind\":\"page\""));
        let worker = bundle(&shared, &frame(&format!("chrome-extension://{id}/_generated_background_page.html{BG_MARKER}"), true, false)).unwrap();
        assert!(worker.contains("\"kind\":\"worker\""));
        let page = bundle(&shared, &frame("https://example.com/a", true, false)).unwrap();
        assert!(page.contains("__signetCrxHub") && page.contains("window.__ran = 42;") && page.contains("DOMContentLoaded"));
        for js in [&popup, &worker, &page, &(page.clone() + super::POPUP_SIZE_SCRIPT), &STORE_CSS_SCRIPT.to_string()] {
            assert_parses(js);
        }
        assert!(bundle(&shared, &frame("https://other.org/", true, false)).is_none());
        assert!(bundle(&shared, &frame("https://example.com/a", true, true)).is_none());
        // Three contexts were registered: popup, worker and the content script.
        assert_eq!(shared.ctxs().take_opened().len(), 3);
    }

    #[test]
    fn unknown_extensions_get_nothing() {
        let (_d, _e, shared, _id) = with_extension();
        assert!(bundle(&shared, &frame("chrome-extension://zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz/x.html", true, false)).is_none());
    }

    #[test]
    fn store_pages_get_the_store_bridge_in_the_main_frame_only() {
        let (_d, _e, shared, _id) = with_extension();
        let url = "https://chromewebstore.google.com/detail/x/abc";
        assert!(bundle(&shared, &frame(url, true, false)).unwrap().contains("webstorePrivate"));
        assert!(bundle(&shared, &frame(url, false, false)).is_none());
    }

    #[test]
    fn only_web_documents_get_content_scripts() {
        assert!(is_page_url("https://example.com/") && is_page_url("file:///tmp/a.html"));
        assert!(!is_page_url("signet://app/x") && !is_page_url("chrome-extension://abc/x") && !is_page_url("about:blank"));
    }
}

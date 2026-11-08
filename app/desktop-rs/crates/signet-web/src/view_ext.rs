//! More `WebView` methods: frame targeting, CSS injection, CDP helpers (screenshots, emulation,
//! MHTML), navigation history, user agent, zoom presets, media control, graceful close.

use std::sync::{Arc, Mutex};

use cef::*;

use crate::runtime::with_state;
use crate::types::*;
use crate::view::{browser_of, cs, host_of, WebView};

/// Description of a frame of a view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameDescription {
    pub id: String,
    pub url: String,
    pub name: String,
    pub is_main: bool,
}

fn all_frames(b: &Browser) -> Vec<Frame> {
    let mut ids = CefStringList::new();
    b.frame_identifiers(Some(&mut ids));
    ids.into_iter().filter_map(|id| b.frame_by_identifier(Some(&cs(&id)))).collect()
}

fn frames_of(b: &Browser, t: &FrameTarget) -> Vec<Frame> {
    match t {
        FrameTarget::Main => b.main_frame().into_iter().collect(),
        FrameTarget::Focused => b.focused_frame().or_else(|| b.main_frame()).into_iter().collect(),
        FrameTarget::Id(id) => b.frame_by_identifier(Some(&cs(id))).into_iter().collect(),
        FrameTarget::Url(u) => all_frames(b).into_iter().filter(|f| crate::extension::from_nav(&CefString::from(&f.url()).to_string()) == *u).take(1).collect(),
        FrameTarget::All => all_frames(b),
    }
}

/// Chromium zoom step: each level multiplies the page size by this factor.
pub const ZOOM_FACTOR: f64 = 1.2;
/// Zoom presets of the browser (percent), in increasing order.
pub const ZOOM_PRESETS: &[u32] = &[25, 33, 50, 67, 75, 80, 90, 100, 110, 125, 150, 175, 200, 250, 300, 400, 500];

/// Chromium zoom level to percent (0 is 100).
pub fn zoom_level_to_percent(level: f64) -> u32 {
    (100.0 * ZOOM_FACTOR.powf(level)).round() as u32
}

/// Percent to Chromium zoom level.
pub fn percent_to_zoom_level(percent: u32) -> f64 {
    (percent.max(1) as f64 / 100.0).ln() / ZOOM_FACTOR.ln()
}

/// Next preset above (or below) a percentage.
pub fn next_zoom_percent(current: u32, up: bool) -> u32 {
    if up {
        ZOOM_PRESETS.iter().copied().find(|p| *p > current).unwrap_or(*ZOOM_PRESETS.last().unwrap_or(&500))
    } else {
        ZOOM_PRESETS.iter().rev().copied().find(|p| *p < current).unwrap_or(*ZOOM_PRESETS.first().unwrap_or(&25))
    }
}

fn js_string(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

impl WebView {
    // ---- frames -----------------------------------------------------------------------------

    /// Frames of the view (main first).
    pub fn frames(&self) -> Vec<FrameDescription> {
        let Some(b) = browser_of(self.id()) else { return vec![] };
        let mut v: Vec<FrameDescription> = all_frames(&b)
            .into_iter()
            .map(|f| FrameDescription {
                id: CefString::from(&f.identifier()).to_string(),
                url: crate::extension::from_nav(&CefString::from(&f.url()).to_string()),
                name: CefString::from(&f.name()).to_string(),
                is_main: f.is_main() == 1,
            })
            .collect();
        v.sort_by_key(|f| !f.is_main);
        v
    }

    /// Run JavaScript in the targeted frame(s) (fire and forget), in the page's main world. Use it
    /// with `signet_scripts::bridge::dispatch_js` / `Reply::to_js` to answer a `PageMessage`
    /// (`FrameTarget::Id(frame_id)`). Returns how many frames ran it.
    pub fn execute_js_in(&self, target: &FrameTarget, code: &str) -> usize {
        let Some(b) = browser_of(self.id()) else { return 0 };
        let frames = frames_of(&b, target);
        for f in &frames {
            f.execute_java_script(Some(&cs(code)), Some(&cs("signet://app/eval")), 0);
        }
        frames.len()
    }

    /// Insert (or replace, same `key`) a stylesheet in the targeted frame(s). Uses a constructed
    /// stylesheet, which page CSP does not block, with a `<style>` fallback. For cosmetic filters.
    /// For CSS that must apply before first paint put the same code in the `set_frame_bundle` result.
    pub fn insert_css(&self, target: &FrameTarget, key: &str, css: &str) -> usize {
        self.execute_js_in(target, &insert_css_js(key, css))
    }

    /// Remove a stylesheet added with [`insert_css`](Self::insert_css).
    pub fn remove_css(&self, target: &FrameTarget, key: &str) -> usize {
        let k = js_string(key);
        self.execute_js_in(
            target,
            &format!(
                "(function(k){{try{{document.adoptedStyleSheets=document.adoptedStyleSheets.filter(function(s){{return s.__sk!==k}});}}catch(e){{}}\
                 var n=document.querySelector('style[data-sg-css=\"'+k+'\"]');if(n)n.remove();}})({k})"
            ),
        )
    }

    // ---- CDP --------------------------------------------------------------------------------

    /// Run a DevTools Protocol command on this view. `params_json` is a JSON object (or empty).
    /// `done` runs on the main thread with the JSON of `result` or the error message.
    pub fn execute_devtools_method(&self, method: &str, params_json: &str, done: impl FnOnce(Result<String, String>) + 'static) {
        match host_of(self.id()) {
            Some(h) => crate::cdp::send(&h, self.id(), method, params_json, Box::new(done)),
            None => done(Err("view not ready".into())),
        }
    }

    /// Receive these CDP events as [`WebEvent::DevToolsEvent`] (the domain must be enabled first,
    /// e.g. `Media.enable`).
    pub fn devtools_subscribe(&self, methods: &[&str]) {
        crate::cdp::subscribe(self.id(), methods, true);
    }

    pub fn devtools_unsubscribe(&self, methods: &[&str]) {
        crate::cdp::subscribe(self.id(), methods, false);
    }

    /// Screenshot of the page (`Page.captureScreenshot`). The view must be visible: a hidden view
    /// produces no frames. `done` gets the encoded image bytes.
    pub fn capture_screenshot(&self, opts: &ScreenshotOptions, done: impl FnOnce(Result<Vec<u8>, String>) + 'static) {
        let mut p = serde_json::Map::new();
        match opts.format {
            ImageFormat::Png => {
                p.insert("format".into(), "png".into());
            }
            ImageFormat::Jpeg(q) => {
                p.insert("format".into(), "jpeg".into());
                p.insert("quality".into(), q.into());
            }
            ImageFormat::Webp(q) => {
                p.insert("format".into(), "webp".into());
                p.insert("quality".into(), q.into());
            }
        }
        if let Some(c) = opts.clip {
            p.insert("clip".into(), serde_json::json!({"x": c.x, "y": c.y, "width": c.width, "height": c.height, "scale": opts.scale}));
        }
        if opts.capture_beyond_viewport {
            p.insert("captureBeyondViewport".into(), true.into());
        }
        self.execute_devtools_method("Page.captureScreenshot", &serde_json::Value::Object(p).to_string(), move |r| {
            done(r.and_then(|json| {
                let v: serde_json::Value = serde_json::from_str(&json).map_err(|e| e.to_string())?;
                let data = v["data"].as_str().ok_or("no image data")?;
                crate::cdp::base64_decode(data).ok_or_else(|| "invalid image data".to_string())
            }))
        });
    }

    /// Save the page as a single MHTML file (`Page.captureSnapshot`).
    pub fn save_page_mhtml(&self, path: &str, done: impl FnOnce(Result<(), String>) + 'static) {
        let path = path.to_string();
        self.execute_devtools_method("Page.captureSnapshot", r#"{"format":"mhtml"}"#, move |r| {
            done(r.and_then(|json| {
                let v: serde_json::Value = serde_json::from_str(&json).map_err(|e| e.to_string())?;
                let data = v["data"].as_str().ok_or("no snapshot data")?;
                std::fs::write(&path, data).map_err(|e| e.to_string())
            }))
        });
    }

    /// Save the current DOM as an HTML file (`document.documentElement.outerHTML` with the doctype).
    pub fn save_page_html(&self, path: &str, done: impl FnOnce(Result<(), String>) + 'static) {
        let path = path.to_string();
        let expr = "(document.doctype?new XMLSerializer().serializeToString(document.doctype)+'\\n':'')+document.documentElement.outerHTML";
        let params = serde_json::json!({"expression": expr, "returnByValue": true}).to_string();
        self.execute_devtools_method("Runtime.evaluate", &params, move |r| {
            done(r.and_then(|json| {
                let v: serde_json::Value = serde_json::from_str(&json).map_err(|e| e.to_string())?;
                let html = v["result"]["value"].as_str().ok_or("no HTML")?;
                std::fs::write(&path, html).map_err(|e| e.to_string())
            }))
        });
    }

    /// Responsive mode: emulate a device (size, scale, touch, user agent). `None` clears it.
    pub fn set_device_emulation(&self, emu: Option<&DeviceEmulation>) {
        let noop = |_r: Result<String, String>| {};
        match emu {
            Some(e) => {
                let m = serde_json::json!({
                    "width": e.width, "height": e.height, "deviceScaleFactor": e.device_scale_factor,
                    "mobile": e.mobile, "screenWidth": e.width, "screenHeight": e.height,
                });
                self.execute_devtools_method("Emulation.setDeviceMetricsOverride", &m.to_string(), noop);
                self.execute_devtools_method(
                    "Emulation.setTouchEmulationEnabled",
                    &serde_json::json!({"enabled": e.touch, "maxTouchPoints": 5}).to_string(),
                    noop,
                );
                self.execute_devtools_method(
                    "Emulation.setEmitTouchEventsForMouse",
                    &serde_json::json!({"enabled": e.touch, "configuration": if e.mobile { "mobile" } else { "desktop" }}).to_string(),
                    noop,
                );
                self.set_user_agent_override(e.user_agent.as_deref());
            }
            None => {
                self.execute_devtools_method("Emulation.clearDeviceMetricsOverride", "{}", noop);
                self.execute_devtools_method("Emulation.setTouchEmulationEnabled", r#"{"enabled":false}"#, noop);
                self.execute_devtools_method("Emulation.setEmitTouchEventsForMouse", r#"{"enabled":false}"#, noop);
                self.set_user_agent_override(None);
            }
        }
    }

    /// Override the user agent of this view (requests and `navigator.userAgent`); `None` resets it.
    /// Takes effect for the next navigation and for new scripts.
    pub fn set_user_agent_override(&self, user_agent: Option<&str>) {
        let p = serde_json::json!({"userAgent": user_agent.unwrap_or("")}).to_string();
        self.execute_devtools_method("Emulation.setUserAgentOverride", &p, |_| {});
    }

    // ---- navigation history -----------------------------------------------------------------

    /// Back/forward list, oldest first (for the long-press back/forward menus).
    pub fn navigation_entries(&self) -> Vec<HistoryEntry> {
        let Some(host) = host_of(self.id()) else { return vec![] };
        let out: Arc<Mutex<Vec<HistoryEntry>>> = Arc::new(Mutex::new(Vec::new()));
        let mut visitor = EntryCollector::new(out.clone());
        host.navigation_entries(Some(&mut visitor), 0);
        out.lock().map(|v| v.clone()).unwrap_or_default()
    }

    /// Go to an entry of [`navigation_entries`](Self::navigation_entries) by its `index`.
    pub fn go_to_history_index(&self, index: usize) {
        let me = *self;
        self.execute_devtools_method("Page.getNavigationHistory", "{}", move |r| {
            let Ok(json) = r else { return };
            let Ok(v) = serde_json::from_str::<serde_json::Value>(&json) else { return };
            let Some(id) = v["entries"].get(index).and_then(|e| e["id"].as_i64()) else { return };
            me.execute_devtools_method("Page.navigateToHistoryEntry", &serde_json::json!({"entryId": id}).to_string(), |_| {});
        });
    }

    /// Add a word to the custom dictionary (context menu "Add to dictionary").
    pub fn add_word_to_dictionary(&self, word: &str) {
        if let Some(h) = host_of(self.id()) {
            h.add_word_to_dictionary(Some(&cs(word)));
        }
    }

    /// Clear web storage of one origin through CDP (`Storage.clearDataForOrigin`, all storage types
    /// including cookies, IndexedDB, caches, service workers). For "forget this site" and history clearing.
    pub fn clear_site_data(&self, origin: &str, done: impl FnOnce(Result<(), String>) + 'static) {
        let p = serde_json::json!({"origin": origin, "storageTypes": "all"}).to_string();
        self.execute_devtools_method("Storage.clearDataForOrigin", &p, move |r| done(r.map(|_| ())));
    }

    /// Take the keyboard focus away from the page and give it to the GPUI window that hosts it.
    pub fn blur(&self) {
        let parent = with_state(|st| st.views.get(&self.id()).map(|e| e.parent)).flatten();
        if let Some(h) = host_of(self.id()) {
            h.set_focus(0);
        }
        #[cfg(target_os = "macos")]
        if let Some(p) = parent {
            crate::mac::focus_parent_view(p as *mut std::ffi::c_void);
        }
        let _ = parent;
    }

    // ---- close, media, zoom -----------------------------------------------------------------

    /// Close like the user would: runs `beforeunload` first. With `set_js_dialog_mode(Events)` the
    /// app receives the dialog as [`WebEvent::JsDialogRequest`] (kind `BeforeUnload`); accepting it
    /// closes the view ([`WebEvent::Closed`] follows), dismissing keeps it open. [`close`](Self::close)
    /// forces the close without asking.
    pub fn request_close(&self) {
        let h = with_state(|st| {
            st.views.get_mut(&self.id()).and_then(|e| {
                e.closing = true;
                e.browser.clone()
            })
        })
        .flatten()
        .and_then(|b| b.host());
        if let Some(h) = h {
            h.try_close_browser();
        }
    }

    /// Control the page's media (play, pause, next track...) through its media session handlers,
    /// falling back to the media elements for play/pause/seek.
    pub fn media_control(&self, cmd: MediaCommand) {
        let code = format!("window.__signetMediaCtl&&window.__signetMediaCtl({})", js_string(cmd.name()));
        let target = crate::media::session_frame(self.id()).map(FrameTarget::Id).unwrap_or(FrameTarget::All);
        self.execute_js_in(&target, &code);
    }

    /// Zoom as a percentage (100 = normal). Chromium keeps the zoom per site in the profile: every
    /// tab of the same host and profile follows.
    pub fn zoom_percent(&self) -> u32 {
        zoom_level_to_percent(self.zoom_level())
    }

    pub fn set_zoom_percent(&self, percent: u32) {
        self.set_zoom_level(percent_to_zoom_level(percent));
    }

    /// One preset step (see [`ZOOM_PRESETS`]); returns the new percentage.
    pub fn zoom_step(&self, up: bool) -> u32 {
        let next = next_zoom_percent(self.zoom_percent(), up);
        self.set_zoom_percent(next);
        next
    }

    pub fn zoom_reset(&self) {
        self.set_zoom_level(0.0);
    }
}

wrap_navigation_entry_visitor! {
    struct EntryCollector { out: Arc<Mutex<Vec<HistoryEntry>>> }

    impl NavigationEntryVisitor {
        fn visit(
            &self,
            entry: Option<&mut NavigationEntry>,
            current: ::std::os::raw::c_int,
            index: ::std::os::raw::c_int,
            _total: ::std::os::raw::c_int,
        ) -> ::std::os::raw::c_int {
            if let (Some(e), Ok(mut v)) = (entry, self.out.lock()) {
                let s = |u: &CefStringUserfree| CefString::from(u).to_string();
                v.push(HistoryEntry {
                    index: index.max(0) as usize,
                    url: s(&e.url()),
                    display_url: s(&e.display_url()),
                    title: s(&e.title()),
                    current: current != 0,
                    http_status: e.http_status_code(),
                });
            }
            1
        }
    }
}

pub(crate) fn insert_css_js(key: &str, css: &str) -> String {
    let (k, c) = (js_string(key), js_string(css));
    format!(
        "(function(k,css){{function go(){{try{{var s=new CSSStyleSheet();s.replaceSync(css);s.__sk=k;\
         document.adoptedStyleSheets=document.adoptedStyleSheets.filter(function(x){{return x.__sk!==k}}).concat([s]);}}\
         catch(e){{var n=document.querySelector('style[data-sg-css=\"'+k+'\"]')||document.createElement('style');\
         n.setAttribute('data-sg-css',k);n.textContent=css;(document.head||document.documentElement).appendChild(n);}}}}\
         if(document.documentElement)go();else document.addEventListener('DOMContentLoaded',go,{{once:true}});}})({k},{c})"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_math() {
        assert_eq!(zoom_level_to_percent(0.0), 100);
        assert_eq!(zoom_level_to_percent(1.0), 120);
        assert_eq!(percent_to_zoom_level(100), 0.0);
        assert_eq!(zoom_level_to_percent(percent_to_zoom_level(150)), 150);
        assert_eq!(next_zoom_percent(100, true), 110);
        assert_eq!(next_zoom_percent(100, false), 90);
        assert_eq!(next_zoom_percent(500, true), 500);
        assert_eq!(next_zoom_percent(25, false), 25);
    }

    #[test]
    fn css_js_escapes() {
        let js = insert_css_js("k\"1", "a{content:\"</style>\"}");
        assert!(js.contains("\\\"</style>\\\""));
    }
}

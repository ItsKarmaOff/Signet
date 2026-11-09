//! `WebView`: a CEF browser embedded as a native child of a window.

use std::path::PathBuf;

use cef::{sys, *};
use raw_window_handle::RawWindowHandle;

use crate::client::make_client;
use crate::runtime::{assert_main_thread, with_state, ViewEntry};
use crate::types::*;

/// Edit commands applied to the focused frame (used by app-drawn context menus).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditCommand {
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    PasteAndMatchStyle,
    Delete,
    SelectAll,
}

/// Handle to a browser view. Cheap to copy; all methods must run on the main thread.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WebView {
    id: ViewId,
}

/// Timestamped trace line (SIGNET_TRACE_VIEWS=1), shared with the native view tree trace.
pub(crate) fn tlog(msg: impl FnOnce() -> String) {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if *ON.get_or_init(|| std::env::var_os("SIGNET_TRACE_VIEWS").is_some()) {
        let ep = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
        eprintln!("[call  {}] {}", ep, msg());
    }
}

pub(crate) fn attach_browser(id: ViewId, browser: Browser) {
    let geom = with_state(|st| {
        st.views.get_mut(&id).map(|e| {
            e.browser = Some(browser.clone());
            (e.parent, e.bounds, e.visible)
        })
    })
    .flatten();
    if is_closing(id) {
        return;
    }
    if let (Some((parent, b, vis)), Some(host)) = (geom, browser.host()) {
        crate::platform::adopt(id.0, &host);
        crate::platform::apply(&host, parent, b, vis);
        // The view was created at (0,0) and moved just now: tell Chromium the final size and scale before the first paint.
        tlog(|| format!("attach_browser view={} {:?} vis={}", id.0, b, vis));
        host.notify_screen_info_changed();
        host.was_resized();
        crate::bundle::register_browser(id, browser.identifier());
    }
}

/// The view was asked to close (`WebView::close`): its native view is gone (or going) and must not be placed again.
pub(crate) fn is_closing(id: ViewId) -> bool {
    with_state(|st| st.views.get(&id).is_some_and(|e| e.closing)).unwrap_or(false)
}

pub(crate) fn browser_of(id: ViewId) -> Option<Browser> {
    with_state(|st| st.views.get(&id).and_then(|e| e.browser.clone())).flatten()
}

pub(crate) fn host_of(id: ViewId) -> Option<BrowserHost> {
    browser_of(id).and_then(|b| b.host())
}

pub(crate) fn cs(s: &str) -> CefString {
    CefString::from(s)
}

impl WebView {
    /// Create a view as a child of `parent` (from `raw-window-handle`, e.g. a GPUI window).
    pub fn create(parent: RawWindowHandle, opts: ViewOptions) -> Result<WebView, Error> {
        assert_main_thread();
        let parent = crate::platform::parent_to_usize(parent).map_err(Error::Parent)?;
        Self::create_native(parent, opts, None)
    }

    pub(crate) fn create_native(parent: usize, opts: ViewOptions, devtools_for: Option<ViewId>) -> Result<WebView, Error> {
        if !crate::runtime::is_initialized() {
            return Err(Error::NotInitialized);
        }
        let key = opts.profile.key();
        let mut ctx = crate::profile::context_for(&opts.profile).ok_or_else(|| Error::Create("request context".into()))?;
        let scripts = with_state(|st| st.scripts.get(&key).cloned()).flatten().unwrap_or_default();
        let id = with_state(|st| {
            let id = ViewId(st.next_view);
            st.next_view += 1;
            st.views.insert(
                id,
                ViewEntry {
                    browser: None,
                    parent,
                    profile: key.clone(),
                    visible: opts.visible,
                    bounds: opts.bounds,
                    closing: false,
                    devtools_for,
                },
            );
            id
        })
        .ok_or(Error::NotInitialized)?;

        let extra_json = serde_json::json!({
            "view": id.0,
            "profile": key,
            "scripts": scripts.iter().map(|s| serde_json::json!({"n": s.name, "s": s.source, "m": s.main_frame_only})).collect::<Vec<_>>(),
        });
        crate::bundle::stash_extra(id, extra_json.to_string());
        let mut extra = dictionary_value_create().ok_or_else(|| Error::Create("dictionary".into()))?;
        extra.set_string(Some(&cs(crate::app::EXTRA_KEY)), Some(&cs(&extra_json.to_string())));

        let (iw, ih) = crate::platform::initial_size(parent, opts.bounds);
        let rect = Rect { x: 0, y: 0, width: iw, height: ih };
        let mut wi = WindowInfo { runtime_style: RuntimeStyle::from(sys::cef_runtime_style_t::CEF_RUNTIME_STYLE_ALLOY), ..Default::default() }
            .set_as_child(crate::platform::native_parent(crate::platform::host_parent(id.0, parent, opts.bounds)), &rect);
        wi.size = std::mem::size_of::<WindowInfo>();
        let bs = BrowserSettings { size: std::mem::size_of::<BrowserSettings>(), ..Default::default() };
        let mut client = make_client(id);
        let url = cs(&crate::extension::to_nav(&opts.url));
        let create_span = crate::startup::enabled().then(|| (std::time::Instant::now(), opts.url.clone()));
        let browser = browser_host_create_browser_sync(Some(&wi), Some(&mut client), Some(&url), Some(&bs), Some(&mut extra), Some(&mut ctx));
        if let Some((t, u)) = create_span {
            crate::startup::mark_with(|| format!("web.create_browser_sync view={} took {:.1} ms url={u}", id.0, t.elapsed().as_secs_f64() * 1000.0));
        }
        match browser {
            Some(b) => {
                attach_browser(id, b);
                Ok(WebView { id })
            }
            None => {
                with_state(|st| st.views.remove(&id));
                Err(Error::Create("browser_host_create_browser_sync returned null".into()))
            }
        }
    }

    /// Reattach a handle from a [`ViewId`] (e.g. one returned by a docked DevTools).
    pub fn from_id(id: ViewId) -> Option<WebView> {
        with_state(|st| st.views.contains_key(&id)).unwrap_or(false).then_some(WebView { id })
    }

    pub fn id(&self) -> ViewId {
        self.id
    }

    pub fn is_alive(&self) -> bool {
        browser_of(self.id).is_some()
    }

    /// Position in window points, top-left origin. Handles HiDPI and flipped coordinates.
    pub fn set_bounds(&self, bounds: Bounds) {
        let vis = with_state(|st| {
            st.views.get_mut(&self.id).map(|e| {
                e.bounds = bounds;
                (e.parent, e.visible)
            })
        })
        .flatten();
        if is_closing(self.id) {
            return;
        }
        if let (Some((parent, vis)), Some(host)) = (vis, host_of(self.id)) {
            tlog(|| format!("set_bounds view={} {:?} vis={}", self.id.0, bounds, vis));
            crate::platform::apply(&host, parent, bounds, vis);
            host.was_resized();
        }
    }

    /// Rounds the corners of the page (macOS; the other platforms keep square views).
    pub fn set_corner_radius(&self, radius: f32) {
        #[cfg(target_os = "macos")]
        if let Some(host) = host_of(self.id) {
            tlog(|| format!("set_corner_radius view={} {}", self.id.0, radius));
            let handle = host.window_handle();
            if !handle.is_null() {
                crate::mac::set_view_corner_radius(handle, radius);
            }
        }
        #[cfg(not(target_os = "macos"))]
        let _ = radius;
    }

    pub fn bounds(&self) -> Bounds {
        with_state(|st| st.views.get(&self.id).map(|e| e.bounds)).flatten().unwrap_or_default()
    }

    pub fn set_visible(&self, visible: bool) {
        let g = with_state(|st| {
            st.views.get_mut(&self.id).map(|e| {
                e.visible = visible;
                (e.parent, e.bounds)
            })
        })
        .flatten();
        if is_closing(self.id) {
            return;
        }
        if let (Some((parent, b)), Some(host)) = (g, host_of(self.id)) {
            tlog(|| format!("set_visible view={} {}", self.id.0, visible));
            crate::platform::apply(&host, parent, b, visible);
            host.was_hidden(!visible as i32);
        }
    }

    pub fn is_visible(&self) -> bool {
        with_state(|st| st.views.get(&self.id).map(|e| e.visible)).flatten().unwrap_or(false)
    }

    /// Debug description of the native view (frame, hidden, parent size). macOS only.
    pub fn debug_native_frame(&self) -> String {
        #[cfg(target_os = "macos")]
        {
            host_of(self.id).map(|h| crate::mac::describe_view(h.window_handle())).unwrap_or_else(|| "no host".into())
        }
        #[cfg(not(target_os = "macos"))]
        {
            host_of(self.id).map(|h| crate::platform::describe(crate::platform::native_id(&h))).unwrap_or_else(|| "no host".into())
        }
    }

    /// Address of the native view (`NSView*` on macOS, 0 elsewhere or without a host). Test helper.
    pub fn debug_native_handle(&self) -> usize {
        #[cfg(target_os = "macos")]
        {
            host_of(self.id).map(|h| h.window_handle() as usize).unwrap_or(0)
        }
        #[cfg(not(target_os = "macos"))]
        {
            host_of(self.id).map(|h| crate::platform::native_id(&h)).unwrap_or(0)
        }
    }

    /// Give keyboard focus to the page.
    pub fn focus(&self) {
        if let Some(h) = host_of(self.id) {
            crate::platform::focus(&h);
        }
    }

    pub fn load_url(&self, url: &str) {
        if let Some(f) = browser_of(self.id).and_then(|b| b.main_frame()) {
            f.load_url(Some(&cs(&crate::extension::to_nav(url))));
        }
    }

    pub fn back(&self) {
        if let Some(b) = browser_of(self.id) { b.go_back() }
    }
    pub fn forward(&self) {
        if let Some(b) = browser_of(self.id) { b.go_forward() }
    }
    pub fn reload(&self) {
        if let Some(b) = browser_of(self.id) { b.reload() }
    }
    pub fn reload_ignore_cache(&self) {
        if let Some(b) = browser_of(self.id) { b.reload_ignore_cache() }
    }
    pub fn stop(&self) {
        if let Some(b) = browser_of(self.id) { b.stop_load() }
    }
    pub fn can_go_back(&self) -> bool {
        browser_of(self.id).map(|b| b.can_go_back() != 0).unwrap_or(false)
    }
    pub fn can_go_forward(&self) -> bool {
        browser_of(self.id).map(|b| b.can_go_forward() != 0).unwrap_or(false)
    }
    pub fn is_loading(&self) -> bool {
        browser_of(self.id).map(|b| b.is_loading() != 0).unwrap_or(false)
    }

    /// Current URL of the main frame.
    pub fn url(&self) -> String {
        browser_of(self.id)
            .and_then(|b| b.main_frame())
            .map(|f| crate::extension::from_nav(&CefString::from(&f.url()).to_string()))
            .unwrap_or_default()
    }

    /// Run JavaScript in the main frame (fire and forget).
    pub fn execute_js(&self, code: &str) {
        if let Some(f) = browser_of(self.id).and_then(|b| b.main_frame()) {
            f.execute_java_script(Some(&cs(code)), Some(&cs("signet://app/eval")), 0);
        }
    }

    /// Chromium zoom level (0 = 100%; each step is 1.2x, negative zooms out).
    pub fn set_zoom_level(&self, level: f64) {
        if let Some(h) = host_of(self.id) { h.set_zoom_level(level) }
    }
    pub fn zoom_level(&self) -> f64 {
        host_of(self.id).map(|h| h.zoom_level()).unwrap_or(0.0)
    }

    pub fn find(&self, text: &str, forward: bool, match_case: bool, find_next: bool) {
        if let Some(h) = host_of(self.id) {
            h.find(Some(&cs(text)), forward as i32, match_case as i32, find_next as i32);
        }
    }
    pub fn stop_find(&self, clear_selection: bool) {
        if let Some(h) = host_of(self.id) { h.stop_finding(clear_selection as i32) }
    }

    pub fn print(&self) {
        if let Some(h) = host_of(self.id) { h.print() }
    }

    /// Print the page to a PDF file.
    pub fn print_to_pdf(&self, path: &str) {
        if let Some(h) = host_of(self.id) {
            h.print_to_pdf(Some(&cs(path)), Some(&PdfPrintSettings::default()), None);
        }
    }

    /// Navigate to `view-source:` of the current page in this view. Apps usually open
    /// `view-source:<url>` in a new tab instead.
    pub fn view_source(&self) {
        if let Some(f) = browser_of(self.id).and_then(|b| b.main_frame()) {
            f.view_source();
        }
    }

    pub fn set_audio_muted(&self, muted: bool) {
        if let Some(h) = host_of(self.id) { h.set_audio_muted(muted as i32) }
    }
    pub fn is_audio_muted(&self) -> bool {
        host_of(self.id).map(|h| h.is_audio_muted() != 0).unwrap_or(false)
    }

    pub fn edit(&self, cmd: EditCommand) {
        let Some(f) = browser_of(self.id).and_then(|b| b.focused_frame().or_else(|| b.main_frame())) else { return };
        match cmd {
            EditCommand::Undo => f.undo(),
            EditCommand::Redo => f.redo(),
            EditCommand::Cut => f.cut(),
            EditCommand::Copy => f.copy(),
            EditCommand::Paste => f.paste(),
            EditCommand::PasteAndMatchStyle => f.paste_and_match_style(),
            EditCommand::Delete => f.del(),
            EditCommand::SelectAll => f.select_all(),
        }
    }

    /// Download `url` through the download handler (as if the user chose "save link as").
    pub fn start_download(&self, url: &str) {
        if let Some(h) = host_of(self.id) { h.start_download(Some(&cs(url))) }
    }

    pub fn replace_misspelling(&self, word: &str) {
        if let Some(h) = host_of(self.id) { h.replace_misspelling(Some(&cs(word))) }
    }

    /// True when the remote debugging server runs (see [`Config::remote_debugging_port`]): the built-in tools are
    /// ordinary views loading the DevTools frontend from it (`ShowDevTools` aborts the process with the Alloy style).
    pub fn devtools_available() -> bool {
        crate::runtime::devtools_port().is_some()
    }

    /// URL of the DevTools frontend inspecting this view. `done` gets it once the page's own target id is known
    /// (CDP `Target.getTargetInfo` on the view's session).
    pub fn devtools_frontend_url(&self, done: impl FnOnce(Result<String, String>) + 'static) {
        let Some(port) = crate::runtime::devtools_port() else { return done(Err("remote debugging is off".into())) };
        self.execute_devtools_method("Target.getTargetInfo", "{}", move |r| {
            done(r.and_then(|json| {
                let v: serde_json::Value = serde_json::from_str(&json).map_err(|e| e.to_string())?;
                let id = v["targetInfo"]["targetId"].as_str().filter(|i| i.chars().all(|c| c.is_ascii_alphanumeric())).ok_or("no target id")?;
                Ok(frontend_url(port, id))
            }))
        });
    }

    /// Moves the native view into another window (the mini player, Peek in the overlay window). The bounds are
    /// kept and re-applied in the new parent's coordinates.
    pub fn reparent(&self, parent: RawWindowHandle) -> Result<(), Error> {
        assert_main_thread();
        let parent = crate::platform::parent_to_usize(parent).map_err(Error::Parent)?;
        let g = with_state(|st| {
            st.views.get_mut(&self.id).map(|e| {
                e.parent = parent;
                (e.bounds, e.visible)
            })
        })
        .flatten()
        .ok_or(Error::UnknownView(self.id))?;
        if is_closing(self.id) {
            return Ok(());
        }
        if let Some(host) = host_of(self.id) {
            crate::platform::reparent(&host, parent, g.0);
            crate::platform::apply(&host, parent, g.0, g.1);
            host.was_resized();
        }
        Ok(())
    }

    /// Close the browser. [`WebEvent::Closed`] follows once CEF has torn it down.
    pub fn close(&self) {
        let b = with_state(|st| {
            st.views.get_mut(&self.id).and_then(|e| {
                e.closing = true;
                e.browser.clone()
            })
        })
        .flatten();
        if let Some(h) = b.and_then(|b| b.host()) {
            h.close_browser(1);
        }
    }
}

/// Cancel an in-progress download by id.
pub fn cancel_download(id: u32) {
    if let Some(cb) = with_state(|st| st.downloads.remove(&id)).flatten() {
        cb.cancel();
    }
}
pub fn pause_download(id: u32) {
    if let Some(cb) = with_state(|st| st.downloads.get(&id).cloned()).flatten() { cb.pause() }
}
pub fn resume_download(id: u32) {
    if let Some(cb) = with_state(|st| st.downloads.get(&id).cloned()).flatten() { cb.resume() }
}

#[allow(dead_code)]
fn _unused(_: PathBuf) {}

/// Frontend address for a target id.
pub fn frontend_url(port: u16, target_id: &str) -> String {
    format!("http://127.0.0.1:{port}/devtools/devtools_app.html?ws=127.0.0.1:{port}/devtools/page/{target_id}")
}

/// True for the only address the tools views may show: the bundled frontend on our loopback debugging port.
pub fn is_frontend_url(url: &str) -> bool {
    match crate::runtime::devtools_port() {
        Some(port) => url.strip_prefix(&format!("http://127.0.0.1:{port}/devtools/")).is_some(),
        None => false,
    }
}

#[cfg(test)]
mod devtools_tests {
    use super::*;

    #[test]
    fn frontend_address_is_on_the_loopback_port() {
        assert_eq!(
            frontend_url(9222, "ABC123"),
            "http://127.0.0.1:9222/devtools/devtools_app.html?ws=127.0.0.1:9222/devtools/page/ABC123"
        );
    }

    #[test]
    fn nothing_is_a_frontend_while_remote_debugging_is_off() {
        assert!(!is_frontend_url("http://127.0.0.1:9222/devtools/devtools_app.html"));
    }
}

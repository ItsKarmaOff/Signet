//! `SIGNET_SELFTEST=1`: drives the whole app once and logs `[selftest] PASS|FAIL <name> - <detail>` lines.
//!
//! Inputs (environment):
//! - `SIGNET_TEST_BASE`: base URL of a local HTTP server serving `index.html`, `two.html`, `data.json`, `ads.html`,
//!   `video.html` (see `scripts/selftest-site.sh`).
//! - `SIGNET_TEST_EXT`: folder of a tiny unpacked MV3 extension (background, popup, content script).
//!
//! It runs on the GPUI foreground executor and uses the same `Core` methods as the screens. Web events are read from
//! `Core::trace`, page state through DevTools `Runtime.evaluate`.

use std::cell::{Ref, RefCell};
use std::rc::Rc;
use std::time::Duration;

use gpui::{App, AsyncApp, BackgroundExecutor};
use serde_json::Value;
use signet_core::api::{CaptureAction, FindOptions};
use signet_core::model::OpenOptions;
use signet_core::types::{CaptureTarget, DevToolsDock, PaletteMode};
use signet_web::WebView;

use crate::browser::{Browser, Core};
use crate::windows::{Cmd, Win};

struct T {
    b: Rc<Browser>,
    ex: BackgroundExecutor,
    base: String,
    ext_dir: String,
    only: Vec<String>,
    pass: std::cell::Cell<u32>,
    fail: std::cell::Cell<u32>,
    rows: RefCell<Vec<(String, bool, String)>>,
}

pub fn spawn(browser: Rc<Browser>, cx: &mut App) {
    *browser.core.borrow().trace.borrow_mut() = Some(Vec::new());
    cx.spawn(async move |cx: &mut AsyncApp| {
        let ex = cx.background_executor().clone();
        let base = std::env::var("SIGNET_TEST_BASE").unwrap_or_default();
        let ext_dir = std::env::var("SIGNET_TEST_EXT").unwrap_or_default();
        let only = std::env::var("SIGNET_TEST_ONLY").unwrap_or_default().split(',').filter(|s| !s.is_empty()).map(str::to_string).collect();
        let mut t = T { b: browser, ex, base, ext_dir, only, pass: Default::default(), fail: Default::default(), rows: Default::default() };
        t.run().await;
    })
    .detach();
}

impl T {
    /// Group enabled (`SIGNET_TEST_ONLY=a,b` limits the run to these groups; empty: all).
    fn on(&self, group: &str) -> bool {
        self.only.is_empty() || self.only.iter().any(|g| g == group)
    }

    fn core(&self) -> Ref<'_, Core> {
        self.b.core.borrow()
    }

    async fn sleep(&self, ms: u64) {
        self.ex.timer(Duration::from_millis(ms)).await;
    }

    /// Polls `f` every 100 ms until it is true or `ms` passed.
    async fn wait(&self, ms: u64, f: impl Fn(&Core) -> bool) -> bool {
        let mut left = ms as i64;
        loop {
            if f(&self.b.core.borrow()) {
                return true;
            }
            if left <= 0 {
                return false;
            }
            self.sleep(100).await;
            left -= 100;
        }
    }

    fn rec(&self, name: &str, ok: bool, detail: impl AsRef<str>) {
        if ok {
            self.pass.set(self.pass.get() + 1);
        } else {
            self.fail.set(self.fail.get() + 1);
        }
        let line = format!("[selftest] {} {} - {}", if ok { "PASS" } else { "FAIL" }, name, detail.as_ref());
        println!("{line}");
        self.rows.borrow_mut().push((name.to_string(), ok, detail.as_ref().to_string()));
    }

    fn mark(&self) -> usize {
        self.core().trace.borrow().as_ref().map(Vec::len).unwrap_or(0)
    }

    /// Trace lines after `from` that contain every needle.
    fn seen(&self, from: usize, needles: &[&str]) -> Vec<String> {
        self.core().trace.borrow().as_ref().map(|t| t.iter().skip(from).filter(|l| needles.iter().all(|n| l.contains(n))).cloned().collect()).unwrap_or_default()
    }

    async fn wait_trace(&self, ms: u64, from: usize, needles: &[&str]) -> Option<String> {
        let mut left = ms as i64;
        loop {
            if let Some(l) = self.seen(from, needles).into_iter().next() {
                return Some(l);
            }
            if left <= 0 {
                return None;
            }
            self.sleep(100).await;
            left -= 100;
        }
    }

    /// Evaluates `expr` in the main frame of `view`; None on error or timeout.
    async fn eval(&self, view: &WebView, expr: &str) -> Option<Value> {
        let slot: Rc<RefCell<Option<Option<Value>>>> = Rc::new(RefCell::new(None));
        let s2 = slot.clone();
        let params = serde_json::json!({ "expression": expr, "returnByValue": true, "awaitPromise": true }).to_string();
        view.execute_devtools_method("Runtime.evaluate", &params, move |r| {
            let v = r.ok().and_then(|j| serde_json::from_str::<Value>(&j).ok()).map(|v| v["result"]["value"].clone());
            *s2.borrow_mut() = Some(v);
        });
        for _ in 0..60 {
            if let Some(v) = slot.borrow_mut().take() {
                return v;
            }
            self.sleep(100).await;
        }
        None
    }

    /// Polls the page of `view` until Signet's error page shows (the engine commits its error page slowly under load,
    /// the injection follows its `LoadEnd`), at most 15 s. Returns the last probe answer.
    async fn error_probe(&self, view: Option<WebView>, probe: &str) -> Option<Value> {
        let v = view?;
        let mut last = None;
        for _ in 0..30 {
            last = self.eval(&v, probe).await;
            if last.as_ref().and_then(Value::as_str).is_some_and(|s| s.starts_with("error-page:")) {
                break;
            }
            self.sleep(500).await;
        }
        last
    }

    fn tab_view(&self, tab: &str) -> Option<WebView> {
        self.core().views.view_of(tab)
    }

    fn tab_info(&self, tab: &str) -> String {
        self.core().state().tabs.get(tab).map(|t| format!("url={} title={:?} loading={} loaded={}", t.url, t.title, t.loading, t.loaded)).unwrap_or_else(|| "no such tab".into())
    }

    /// Opens `url` in the active space and waits for the load to end. Returns the tab id.
    async fn open_and_load(&mut self, url: &str) -> String {
        let id = self.b.core.borrow_mut().open_tab(url, OpenOptions { activate: true, ..Default::default() });
        let idc = id.clone();
        self.wait(12_000, move |c| c.state().tabs.get(&idc).is_some_and(|t| t.loaded && !t.loading && !t.url.is_empty())).await;
        self.sleep(400).await;
        id
    }

    /// Native frame of the view of `tab`, parsed from `debug_native_frame`: (x, y, w, h, hidden).
    fn frame_of(&self, tab: &str) -> Option<(f64, f64, f64, f64, bool)> {
        let d = self.tab_view(tab)?.debug_native_frame();
        let inner = d.split("frame=(").nth(1)?.split(')').next()?;
        let n: Vec<f64> = inner.split(',').filter_map(|s| s.trim().parse().ok()).collect();
        if n.len() != 4 {
            return None;
        }
        Some((n[0], n[1], n[2], n[3], d.contains("hidden=true")))
    }

    /// Native CEF views attached to application windows: (window, class, address, hidden).
    fn native_views(&self) -> Vec<(usize, String, usize, bool)> {
        signet_web::debug_native_cef_views()
    }

    fn attached(&self, ptr: usize) -> bool {
        ptr != 0 && signet_web::debug_native_all_views().iter().any(|v| v.contains(&format!("@{ptr:x}/")))
    }

    fn handle_of(&self, tab: &str) -> usize {
        self.tab_view(tab).map(|v| v.debug_native_handle()).unwrap_or(0)
    }

    /// Number of native CEF views in the window `anchor` (address of the NSWindow).
    fn count_in_window_of(&self, anchor: usize) -> usize {
        self.native_views().iter().filter(|x| x.0 == anchor).count()
    }

    /// Polls until none of `ptrs` is in a window hierarchy any more (6 s). Returns the ones still attached.
    async fn gone(&self, ptrs: &[usize]) -> Vec<usize> {
        let mut left = Vec::new();
        for _ in 0..60 {
            left = ptrs.iter().copied().filter(|p| self.attached(*p)).collect();
            if left.is_empty() {
                break;
            }
            self.sleep(100).await;
        }
        left
    }

    /// Zombie check: closing views must take their native views out of the window (not leave them below or above
    /// the current tab). Counts the CEF views of the main window and follows the exact view addresses.
    async fn zombie(&mut self, base: &str) {
        // Later groups (find, sidebar...) expect the test page in the active tab: restore it at the end.
        let start = self.core().active_tab_id();
        let all = signet_web::debug_native_all_views();
        println!("[selftest] info zombie hierarchy: {all:?}");
        let a = self.open_and_load(&format!("{base}/index.html")).await;
        let anchor = self.native_views().iter().find(|x| x.2 == self.handle_of(&a)).map(|x| x.0).unwrap_or(0);
        let n_a = self.count_in_window_of(anchor);
        // Several tabs, then close some.
        let b = self.open_and_load(&format!("{base}/two.html")).await;
        let c = self.open_and_load(&format!("{base}/index.html")).await;
        let d = self.open_and_load(&format!("{base}/two.html")).await;
        let (hb, hc, hd) = (self.handle_of(&b), self.handle_of(&c), self.handle_of(&d));
        let n_open = self.count_in_window_of(anchor);
        self.b.core.borrow_mut().close_tab(&b);
        self.b.core.borrow_mut().close_tab(&c);
        let left = self.gone(&[hb, hc]).await;
        let n_close = self.count_in_window_of(anchor);
        self.rec("zombie.tabs", n_open == n_a + 3 && n_close == n_open - 2 && left.is_empty() && !self.attached(hb), format!("main window cef views: 1 tab {n_a}, +3 tabs {n_open}, after closing 2 {n_close}, still attached {left:?}"));

        // Sleep: a hidden tab discarded by the idle check, and unload from the menu.
        let e = self.open_and_load(&format!("{base}/index.html")).await;
        let he = self.handle_of(&d);
        let before = self.count_in_window_of(anchor);
        {
            let mut core = self.b.core.borrow_mut();
            core.store.data.state.settings.tab_sleep_minutes = Some(1);
            core.hidden_since.insert(d.clone(), 1);
            core.discard_idle();
        }
        let left = self.gone(&[he]).await;
        let after = self.count_in_window_of(anchor);
        self.rec("zombie.sleep", self.core().views.view_of(&d).is_none() && left.is_empty() && after == before - 1, format!("idle discard: cef views {before} -> {after}, still attached {left:?}"));
        let he2 = self.handle_of(&a);
        let before = self.count_in_window_of(anchor);
        self.b.core.borrow_mut().open_tab("about:blank", OpenOptions { activate: true, ..Default::default() });
        self.sleep(600).await;
        let _ = e;
        self.b.core.borrow_mut().unload_tab(&a);
        let left = self.gone(&[he2]).await;
        let after = self.count_in_window_of(anchor);
        self.rec("zombie.unload", left.is_empty() && after == before, format!("unload menu action (new tab in, unloaded tab out): cef views {before} -> {after}, still attached {left:?}"));

        // Split pane.
        let base_n = self.count_in_window_of(anchor);
        let second = self.b.core.borrow_mut().open_in_split(&format!("{base}/two.html"));
        self.wait(10_000, |c| c.panes.len() == 2).await;
        self.sleep(800).await;
        let hs = self.handle_of(&second);
        let n_split = self.count_in_window_of(anchor);
        self.b.core.borrow_mut().close_tab(&second);
        let left = self.gone(&[hs]).await;
        let n_after = self.count_in_window_of(anchor);
        self.rec("zombie.split", hs != 0 && n_split == base_n + 1 && n_after == base_n && left.is_empty(), format!("split: cef views {base_n} -> {n_split} -> {n_after}, still attached {left:?}"));

        // Peek, closed by the overlay (drop_peek).
        let n0 = self.native_views().len();
        let from = self.core().active_tab_id();
        self.b.core.borrow_mut().open_peek(&format!("{base}/two.html"), from.as_deref());
        self.wait(6000, |c| c.peek_tab().is_some_and(|t| c.views.view_of(t).is_some())).await;
        self.sleep(1500).await;
        let ptab = self.core().peek_tab().map(str::to_string).unwrap_or_default();
        let hp = self.handle_of(&ptab);
        let n1 = self.native_views().len();
        self.b.core.borrow_mut().close_overlay(false);
        let left = self.gone(&[hp]).await;
        let n2 = self.native_views().len();
        self.rec("zombie.peek", hp != 0 && n1 == n0 + 1 && n2 == n0 && left.is_empty(), format!("peek: all cef views {n0} -> {n1} -> {n2}, still attached {left:?}"));

        // Private window and Little Signet window.
        let n0 = self.native_views().len();
        self.b.core.borrow_mut().open_private_url(&format!("{base}/index.html"));
        self.wait(10_000, |c| !c.selftest_aux().0.is_empty()).await;
        self.sleep(2000).await;
        self.b.core.borrow_mut().open_little_window(&format!("{base}/two.html"));
        self.wait(10_000, |c| c.selftest_aux().1 == 2).await;
        self.sleep(2000).await;
        let aux = self.core().selftest_aux();
        let hs: Vec<usize> = aux.0.iter().filter_map(|(v, _, _)| WebView::from_id(*v)).map(|v| v.debug_native_handle()).collect();
        let n1 = self.native_views().len();
        let ids: Vec<u64> = aux.0.iter().map(|(_, w, _)| *w).collect();
        for id in ids {
            self.b.core.borrow_mut().on_aux_closed(id);
            self.b.core.borrow().send(Cmd::CloseAux(id));
        }
        let left = self.gone(&hs).await;
        let n2 = self.native_views().len();
        self.rec("zombie.aux", hs.len() == 2 && n1 == n0 + 2 && left.is_empty(), format!("private + little windows: all cef views {n0} -> {n1} -> {n2} (a closed window takes its views with it), views {hs:?} still attached {left:?}"));

        // The user's scenario: site in tab A, close A, tab B on a refused port.
        let probe = "(document.querySelector('main.signet-error h1') ? 'error-page:' + document.querySelector('main.signet-error h1').textContent : 'other:' + location.href + ':' + document.body.innerText.slice(0, 60))";
        let ta = self.open_and_load(&format!("{base}/video.html")).await;
        self.sleep(1500).await;
        let hta = self.handle_of(&ta);
        self.b.core.borrow_mut().close_tab(&ta);
        let left = self.gone(&[hta]).await;
        let m = self.mark();
        let tb = self.b.core.borrow_mut().open_tab("http://127.0.0.1:8769/", OpenOptions { activate: true, ..Default::default() });
        let got = self.wait_trace(10_000, m, &["LoadError"]).await;
        let r = self.error_probe(self.tab_view(&tb), probe).await;
        self.sleep(500).await;
        let htb = self.handle_of(&tb);
        let win_b = self.native_views().iter().find(|x| x.2 == htb).map(|x| x.0).unwrap_or(0);
        let visible: Vec<_> = self.native_views().into_iter().filter(|v| v.0 == win_b && !v.3).collect();
        let only_b = visible.len() == 1 && visible[0].2 == htb;
        self.rec(
            "zombie.scenario",
            left.is_empty() && !self.attached(hta) && got.is_some() && only_b && r.as_ref().and_then(Value::as_str).is_some_and(|s| s.starts_with("error-page:")),
            format!("A closed (attached={}), B visible alone={only_b} (visible views {}), load error={} probe={r:?}", self.attached(hta), visible.len(), got.is_some()),
        );
        // Leave the state as found: close the tabs of this group and reselect the starting tab.
        for t in [&tb, &a, &d] {
            if self.core().state().tabs.contains_key(t.as_str()) {
                self.b.core.borrow_mut().close_tab(t);
            }
        }
        if let Some(s) = start.filter(|s| self.core().state().tabs.contains_key(s.as_str())) {
            self.b.core.borrow_mut().activate_tab(&s);
        }
        self.sleep(1000).await;
    }

    async fn run(&mut self) {
        let have_site = !self.base.is_empty();
        self.sleep(2500).await;

        // ---------------------------------------------------------------- startup and import
        {
            let c = self.core();
            let tabs = c.state().tabs.len();
            let spaces = c.state().spaces.len();
            let win = c.win.as_ref().map(|w| w.size);
            let electron = signet_core::paths::electron_dir().map(|d| d.join("state.json").exists()).unwrap_or(false) && signet_core::paths::import_allowed();
            let (ok_win, detail) = (win.is_some(), format!("window {win:?} tabs={tabs} spaces={spaces} views={}", c.views.ids().len()));
            let imported = !electron || spaces > 0;
            drop(c);
            self.rec("startup.window", ok_win, detail);
            self.rec("startup.import", imported, format!("electron state present={electron}"));
        }

        // The context menu needs no page: it runs before the site checks.
        if self.on("menu") {
            self.context_menu().await;
        }

        if !have_site {
            self.rec("site", false, "SIGNET_TEST_BASE is not set: page checks skipped");
            return self.finish().await;
        }
        let base = self.base.clone();

        // ---------------------------------------------------------------- tab, load, rect, page scripts
        let m = self.mark();
        let tab = self.open_and_load(&format!("{base}/index.html")).await;
        // The very first page load can be slow on a cold machine (CI): give the title and the end of loading some time.
        let mut info = self.tab_info(&tab);
        for _ in 0..30 {
            if info.contains("Signet Test Page") && info.contains("loading=false") {
                break;
            }
            self.sleep(500).await;
            info = self.tab_info(&tab);
        }
        self.rec("tab.load", info.contains("Signet Test Page") && info.contains("loading=false"), info);
        let win = self.core().win.as_ref().map(|w| w.size).unwrap_or((0.0, 0.0));
        let frame = self.frame_of(&tab);
        let frame_ok = frame.is_some_and(|(x, y, w, h, hidden)| !hidden && w > 300.0 && h > 300.0 && x >= 0.0 && y >= 0.0 && x + w <= win.0 + 1.0 && y + h <= win.1 + 1.0);
        let pane = self.core().panes.iter().find(|p| p.tab_id == tab).map(|p| p.rect);
        self.rec("tab.rect", frame_ok, format!("view frame {frame:?} pane {pane:?} window {win:?}"));
        // The page viewport must match the pane (points) and the scale of the screen, not a default size or 1x.
        if let (Some(v), Some(p)) = (self.tab_view(&tab), pane) {
            let r = self.eval(&v, "[window.innerWidth, window.innerHeight, window.devicePixelRatio].join(',')").await;
            let n: Vec<f64> = r.as_ref().and_then(Value::as_str).unwrap_or("").split(',').filter_map(|x| x.parse().ok()).collect();
            let ok = n.len() == 3 && (n[0] - p.width).abs() <= 2.0 && (n[1] - p.height).abs() <= 2.0 && n[2] >= 1.0;
            self.rec("viewport.size", ok, format!("page {n:?} (w,h,dpr) pane {}x{}", p.width, p.height));
        }
        if self.on("spaceform") {
            self.space_form().await;
        }

        if let Some(v) = self.tab_view(&tab) {
            let r = self.eval(&v, "typeof window.__signetPost + ':' + document.title").await;
            self.rec("pagescript.bridge", r.as_ref().and_then(Value::as_str).is_some_and(|s| s.starts_with("function")), format!("{r:?}"));
            let m2 = self.mark();
            let _ = self.eval(&v, "window.__signetPost('swipe:can', JSON.stringify({ rid: 'st1', data: null })); 1").await;
            let swipe = self.wait_trace(4000, m2, &["PageMessage", "swipe:can"]).await;
            self.rec("pagescript.message", swipe.is_some(), swipe.map(|l| l.chars().take(200).collect::<String>()).unwrap_or_else(|| "no swipe:can PageMessage after __signetPost".into()));
        }
        let first = self.seen(m, &["PageMessage"]).into_iter().next().unwrap_or_default();
        println!("[selftest] info first page message while loading: {}", first.chars().take(200).collect::<String>());

        if self.on("close") {
            let extra = self.open_and_load(&format!("{base}/two.html")).await;
            let views_before = self.core().views.ids().len();
            self.b.core.borrow_mut().close_tab(&extra);
            self.sleep(2000).await;
            let win = self.core().win.is_some();
            let views = self.core().views.ids().len();
            self.rec("tab.close", win && self.core().views.view_of(&extra).is_none(), format!("main window alive={win} views {views_before} -> {views}"));
            if !win {
                return self.finish().await;
            }
        }

        if self.on("zombie") {
            self.zombie(&base).await;
        }

        if self.on("palette") {
        // ---------------------------------------------------------------- command bar
            let m = self.mark();
            self.b.core.borrow_mut().open_palette(PaletteMode::NewTab, None);
            let up = self.wait(3000, |c| c.ov.payload.is_some() && c.desired.overlay.is_some()).await;
            let opened = self.wait_trace(3000, m, &["surface-opened overlay"]).await;
            self.sleep(800).await;
            let rows = self.b.core.borrow_mut().palette_query("exam", PaletteMode::NewTab).len();
            self.rec("palette.overlay", up && opened.is_some(), format!("payload+rect={up} window opened={} rows for 'exam'={rows} rect={:?}", opened.is_some(), self.core().desired.overlay));
            self.b.core.borrow_mut().close_overlay(false);
            let closed = self.wait(3000, |c| c.desired.overlay.is_none()).await;
            self.rec("palette.close", closed, format!("overlay rect cleared={closed}"));

        }

        if self.on("find") {
            // ---------------------------------------------------------------- find
            self.b.core.borrow_mut().show_find();
            let shown = self.wait(2000, |c| c.desired.find.is_some()).await;
            self.b.core.borrow_mut().find("Signet", FindOptions { forward: true, find_next: false });
            let matches = self.wait(4000, |c| c.find.state.matches > 0).await;
            let n = self.core().find.state.matches;
            self.b.core.borrow_mut().close_find(true);
            self.rec("find", shown && matches, format!("bar shown={shown} matches={n}"));

        }

        if self.on("sidebar") {
            // ---------------------------------------------------------------- sidebar toggle and slide
            let was = self.core().state().sidebar_visible;
            let m = self.mark();
            self.b.core.borrow_mut().toggle_sidebar();
            self.sleep(1200).await;
            let after = self.core().state().sidebar_visible;
            let slides = self.seen(m, &["slide "]);
            let last = slides.last().cloned().unwrap_or_default();
            self.rec("sidebar.toggle", after != was && slides.len() >= 2, format!("visible {was} -> {after}, slide steps={} last={last:?}", slides.len()));
            let m = self.mark();
            self.b.core.borrow_mut().toggle_sidebar();
            self.sleep(1200).await;
            let back = self.core().state().sidebar_visible;
            let slides = self.seen(m, &["slide "]);
            self.rec("sidebar.restore", back == was && slides.len() >= 2, format!("visible={back} slide steps={}", slides.len()));
            let frame = self.frame_of(&tab);
            self.rec("sidebar.rect_after", frame.is_some_and(|f| f.2 > 300.0 && !f.4), format!("view frame {frame:?}"));

        }

        if self.on("split") {
            // ---------------------------------------------------------------- split view
            let before = self.core().panes.len();
            let second = self.b.core.borrow_mut().open_in_split(&format!("{base}/two.html"));
            let idc = second.clone();
            self.wait(10_000, move |c| c.state().tabs.get(&idc).is_some_and(|t| t.loaded && !t.loading)).await;
            self.sleep(600).await;
            let panes: Vec<_> = self.core().panes.iter().map(|p| (p.tab_id.clone(), p.rect)).collect();
            let frames: Vec<_> = panes.iter().map(|(t, _)| self.frame_of(t)).collect();
            let ok = panes.len() == 2 && frames.iter().all(|f| f.is_some_and(|f| f.2 > 200.0 && f.3 > 200.0 && !f.4));
            let overlap = match (frames.first().copied().flatten(), frames.get(1).copied().flatten()) {
                (Some(a), Some(b)) => a.0 < b.0 + b.2 - 1.0 && b.0 < a.0 + a.2 - 1.0 && a.1 < b.1 + b.3 - 1.0 && b.1 < a.1 + a.3 - 1.0,
                _ => true,
            };
            self.rec("split.panes", ok && !overlap, format!("panes before={before} now={} frames={frames:?} overlap={overlap}", panes.len()));
            let sid = self.core().is_split_item(&second);
            if let Some(sid) = sid {
                self.b.core.borrow_mut().unsplit(&sid);
            }
            self.sleep(500).await;
            self.rec("split.unsplit", self.core().panes.len() == 1, format!("panes={}", self.core().panes.len()));

        }

        if self.on("aux") {
            // ---------------------------------------------------------------- private window and Little Signet
            let m = self.mark();
            self.b.core.borrow_mut().open_private_url(&format!("{base}/index.html"));
            let got = self.wait(10_000, |c| !c.selftest_aux().0.is_empty()).await;
            self.sleep(2500).await;
            let (views, wins) = self.core().selftest_aux();
            let load = self.seen(m, &["LoadEnd", "index.html"]);
            self.rec("private.window", got && wins == 1 && !load.is_empty(), format!("aux windows={wins} views={} LoadEnd lines={}", views.len(), load.len()));
            let pview = self.core().selftest_aux().0.first().map(|(v, _, _)| *v);
            let bridge = match pview.and_then(WebView::from_id) {
                Some(v) => self.eval(&v, "typeof window.__signetPost").await,
                None => None,
            };
            self.rec("private.pagescript", bridge.as_ref().and_then(Value::as_str) == Some("function"), format!("typeof __signetPost in the private view: {bridge:?}"));
            let m = self.mark();
            self.b.core.borrow_mut().open_little_window(&format!("{base}/index.html"));
            let got = self.wait(10_000, |c| c.selftest_aux().1 == 2).await;
            self.sleep(2500).await;
            let (views, wins) = self.core().selftest_aux();
            let load = self.seen(m, &["LoadEnd", "index.html"]);
            self.rec("little.window", got && wins == 2 && !load.is_empty(), format!("aux windows={wins} views={} LoadEnd lines={}", views.len(), load.len()));
            let ids: Vec<u64> = self.core().selftest_aux().0.iter().map(|(_, w, _)| *w).collect();
            for id in ids {
                // As the window manager does when the user closes the window.
                self.b.core.borrow_mut().on_aux_closed(id);
                self.b.core.borrow().send(Cmd::CloseAux(id));
            }
            self.sleep(1000).await;
            self.rec("aux.close", self.core().selftest_aux().1 == 0, format!("aux windows left={}", self.core().selftest_aux().1));

        }

        if self.on("note") {
            // ---------------------------------------------------------------- note
            let m = self.mark();
            self.b.core.borrow_mut().create_note("note");
            let note_tab = {
                let ok = self.wait(5000, |c| c.active_tab_id().and_then(|id| c.state().tabs.get(&id).map(|t| t.url.starts_with("signet://note/"))).unwrap_or(false)).await;
                if ok { self.core().active_tab_id() } else { None }
            };
            let load = self.wait_trace(8000, m, &["PageMessage", "notes:load"]).await;
            self.sleep(800).await;
            let ninfo = note_tab.as_deref().map(|t| self.tab_info(t)).unwrap_or_default();
            let nerr = self.seen(m, &["LoadError"]);
            self.rec("note.load", note_tab.is_some() && load.is_some() && nerr.is_empty(), format!("{ninfo}; notes:load seen={}; load errors={nerr:?}", load.is_some()));
            if let Some(v) = note_tab.as_deref().and_then(|t| self.tab_view(t)) {
                let r = self.eval(&v, "document.title + ' | ' + document.querySelectorAll('*').length + ' elements | ' + document.body.innerText.slice(0, 80).replace(/\\s+/g, ' ')").await;
                self.rec("note.render", r.as_ref().and_then(Value::as_str).is_some_and(|s| !s.contains("404") && !s.to_lowercase().contains("not found")), format!("{r:?}"));
            }

        }

        if self.on("json") {
            // ---------------------------------------------------------------- JSON viewer
            let m = self.mark();
            let _jtab = self.open_and_load(&format!("{base}/data.json")).await;
            let jv = self.wait_trace(6000, m, &["json-viewer"]).await;
            self.rec("jsonviewer", jv.as_ref().is_some_and(|l| l.contains("ok")), format!("{jv:?}"));

        }

        if self.on("errpage") {
            // ---------------------------------------------------------------- error page (refused connection)
            let dead = "http://127.0.0.1:8769/";
            let probe = "(document.querySelector('main.signet-error h1') ? 'error-page:' + document.querySelector('main.signet-error h1').textContent : 'other:' + location.href + ':' + document.body.innerText.slice(0, 60))";
            // New tab opened on a dead port, the previously active tab keeps its own page.
            let before = self.open_and_load(&format!("{base}/two.html")).await;
            let m = self.mark();
            let id = self.b.core.borrow_mut().open_tab(dead, OpenOptions { activate: true, ..Default::default() });
            let got = self.wait_trace(10_000, m, &["LoadError"]).await;
            self.sleep(2500).await;
            let r = self.error_probe(self.tab_view(&id), probe).await;
            let fr = self.frame_of(&id);
            let old = self.frame_of(&before);
            self.rec(
                "errpage.newtab",
                got.is_some() && r.as_ref().and_then(Value::as_str).is_some_and(|s| s.starts_with("error-page:")) && fr.is_some_and(|f| !f.4) && old.is_some_and(|f| f.4),
                format!("load error={} probe={r:?} new frame={fr:?} previous tab frame={old:?}", got.is_some()),
            );
            // Typed in the address bar of a fresh tab (https upgrade then http fallback), as the user did.
            let tid = self.open_and_load(&format!("{base}/index.html")).await;
            let tid2 = self.b.core.borrow_mut().open_tab("about:blank", OpenOptions { activate: true, ..Default::default() });
            self.sleep(800).await;
            let m = self.mark();
            self.b.core.borrow_mut().navigate(&tid2, "localhost:8769");
            let got = self.wait_trace(10_000, m, &["LoadError"]).await;
            self.sleep(3000).await;
            let r = self.error_probe(self.tab_view(&tid2), probe).await;
            let prev = self.frame_of(&tid);
            self.rec("errpage.typed", got.is_some() && r.as_ref().and_then(Value::as_str).is_some_and(|s| s.starts_with("error-page:")) && prev.is_some_and(|f| f.4), format!("load error={} probe={r:?} previous tab frame={prev:?}", got.is_some()));
            // Palette flow (new tab, typed) from a heavy page.
            let yt = self.open_and_load(&format!("{base}/video.html")).await;
            self.sleep(3000).await;
            let m = self.mark();
            self.b.core.borrow_mut().open_url("localhost:8769", signet_core::types::overlay::PaletteMode::NewTab, true);
            let got = self.wait_trace(10_000, m, &["LoadError"]).await;
            self.sleep(3000).await;
            let aid = self.core().active_tab_id().unwrap_or_default();
            let r = self.error_probe(self.tab_view(&aid), probe).await;
            let prev = self.frame_of(&yt);
            self.rec("errpage.palette", got.is_some() && r.as_ref().and_then(Value::as_str).is_some_and(|s| s.starts_with("error-page:")) && prev.is_some_and(|f| f.4), format!("load error={} probe={r:?} previous tab frame={prev:?}", got.is_some()));
            // Palette flow (edit the URL of the current tab) from a heavy page.
            let yt2 = self.open_and_load(&format!("{base}/video.html")).await;
            self.sleep(3000).await;
            let m = self.mark();
            self.b.core.borrow_mut().open_url("localhost:8769", signet_core::types::overlay::PaletteMode::EditUrl, true);
            let got = self.wait_trace(10_000, m, &["LoadError"]).await;
            self.sleep(3000).await;
            let r = self.error_probe(self.tab_view(&yt2), probe).await;
            self.rec("errpage.edit", got.is_some() && r.as_ref().and_then(Value::as_str).is_some_and(|s| s.starts_with("error-page:")), format!("load error={} probe={r:?}", got.is_some()));
            // New tab on a dead port while the previous page shows live video.
            let vid = self.open_and_load(&format!("{base}/video.html")).await;
            self.sleep(2500).await;
            let m = self.mark();
            let nid = self.b.core.borrow_mut().open_tab(dead, OpenOptions { activate: true, ..Default::default() });
            let got = self.wait_trace(10_000, m, &["LoadError"]).await;
            self.sleep(3000).await;
            let r = self.error_probe(self.tab_view(&nid), probe).await;
            let prev = self.frame_of(&vid);
            self.rec("errpage.video", got.is_some() && r.as_ref().and_then(Value::as_str).is_some_and(|s| s.starts_with("error-page:")) && prev.is_some_and(|f| f.4), format!("load error={} probe={r:?} previous tab frame={prev:?}", got.is_some()));
            // Real flow: the command bar is open over the page, the address is accepted, the bar closes.
            let yt3 = self.open_and_load(&format!("{base}/video.html")).await;
            self.sleep(2500).await;
            self.b.core.borrow_mut().open_palette(PaletteMode::NewTab, None);
            self.wait(3000, |c| c.ov.payload.is_some() && c.desired.overlay.is_some()).await;
            self.sleep(800).await;
            let m = self.mark();
            self.b.core.borrow_mut().open_url("localhost:8769", PaletteMode::NewTab, true);
            self.b.core.borrow_mut().close_overlay(false);
            let got = self.wait_trace(10_000, m, &["LoadError"]).await;
            self.sleep(3000).await;
            let aid = self.core().active_tab_id().unwrap_or_default();
            let r = self.error_probe(self.tab_view(&aid), probe).await;
            let prev = self.frame_of(&yt3);
            let nf = self.frame_of(&aid);
            self.rec("errpage.bar", got.is_some() && r.as_ref().and_then(Value::as_str).is_some_and(|s| s.starts_with("error-page:")) && prev.is_some_and(|f| f.4), format!("load error={} probe={r:?} previous={prev:?} new={nf:?}", got.is_some()));
            // Existing tab: from a working site to the dead port.
            let tab = self.open_and_load(&format!("{base}/two.html")).await;
            let m = self.mark();
            self.b.core.borrow_mut().navigate(&tab, dead);
            let got = self.wait_trace(10_000, m, &["LoadError"]).await;
            self.sleep(2500).await;
            let r = self.error_probe(self.tab_view(&tab), probe).await;
            self.rec("errpage.sametab", got.is_some() && r.as_ref().and_then(Value::as_str).is_some_and(|s| s.starts_with("error-page:")), format!("load error={} probe={r:?}", got.is_some()));
        }

        if self.on("adblock") {
            // ---------------------------------------------------------------- ad blocker
            let m = self.mark();
            let atab = self.open_and_load(&format!("{base}/ads.html")).await;
            let blocked = self.wait_trace(20_000, m, &["RequestBlocked"]).await;
            let count = self.core().adblock_blocked(&atab);
            self.rec("adblock", blocked.is_some() || count > 0, format!("blocked event={:?} counter={count} (needs the filter lists, downloaded on first use)", blocked.map(|b| b.chars().take(160).collect::<String>())));

        }

        if self.on("https") {
            // ---------------------------------------------------------------- HTTPS-only upgrade
            let m = self.mark();
            let https_on = self.core().state().settings.https_only;
            let id = self.b.core.borrow_mut().open_tab("http://example.com/", OpenOptions { activate: true, ..Default::default() });
            let up = self.wait_trace(15_000, m, &["LoadStart", "https://example.com"]).await;
            let _ = id;
            self.rec("httpsonly", up.is_some(), format!("setting={https_on:?}; first https load seen={}", up.is_some()));

        }

        if self.on("peek") {
            // ---------------------------------------------------------------- Peek
            let m = self.mark();
            let from = self.core().active_tab_id();
            self.b.core.borrow_mut().open_peek(&format!("{base}/two.html"), from.as_deref());
            let peek = self.wait(4000, |c| c.peek_tab().is_some()).await;
            let ptab = self.core().peek_tab().map(str::to_string);
            let pv = self.wait(6000, |c| c.peek_tab().is_some_and(|t| c.views.view_of(t).is_some())).await;
            self.sleep(2000).await;
            let ld = self.seen(m, &["LoadEnd", "two.html"]);
            let before_tabs = self.core().state().tabs.len();
            self.rec("peek.open", peek && pv && !ld.is_empty(), format!("peek tab={ptab:?} view={pv} LoadEnd={}", ld.len()));
            self.b.core.borrow_mut().expand_peek();
            self.sleep(1500).await;
            let after = self.core().peek_tab().is_none();
            self.rec("peek.expand", after, format!("peek gone={after} tabs {before_tabs} -> {}", self.core().state().tabs.len()));

        }

        // ---------------------------------------------------------------- windows
        if self.on("windows") {
            self.windows().await;
        }

        if self.on("resp") {
            // ---------------------------------------------------------------- responsive, capture
            let dtab = self.open_and_load(&format!("{base}/index.html")).await;
            self.b.core.borrow_mut().toggle_responsive(&dtab);
            self.sleep(2500).await;
            let resp = self.core().dev_overrides(&dtab).responsive.clone();
            let w = match self.tab_view(&dtab) {
                Some(v) => self.eval(&v, "screen.width + 'x' + screen.height + ' dpr ' + devicePixelRatio + ' touch ' + navigator.maxTouchPoints").await,
                None => None,
            };
            self.rec("responsive", resp.is_some() && w.as_ref().and_then(Value::as_str).is_some_and(|s| s.starts_with("393x852")), format!("spec={resp:?} page screen={w:?} (a page without a viewport tag lays out at 980 wide)"));
            self.b.core.borrow_mut().toggle_responsive(&dtab);
            self.sleep(1500).await;
            self.rec("responsive.off", self.core().dev_overrides(&dtab).responsive.is_none(), "left");

            let m = self.mark();
            let tabs_before = self.core().state().tabs.len();
            let res = self.b.core.borrow_mut().take_capture(&dtab, CaptureTarget::Visible, CaptureAction::Easel);
            let done = self.wait_trace(10_000, m, &["capture-done"]).await;
            self.sleep(1500).await;
            self.rec("capture", res.ok && done.as_ref().is_some_and(|l| l.contains("ok=true")), format!("started={} done={done:?} tabs {tabs_before} -> {}", res.ok, self.core().state().tabs.len()));

        }

        if self.on("pip") {
            // ---------------------------------------------------------------- mini player
            let vtab = self.open_and_load(&format!("{base}/video.html")).await;
            self.sleep(2000).await;
            self.b.core.borrow_mut().open_pip(Some(&vtab));
            let owned = self.wait(6000, |c| c.pip_owns(&vtab)).await;
            self.sleep(1500).await;
            let (_, auxw) = self.core().selftest_aux();
            self.rec("pip.open", owned, format!("pip owns tab={owned} aux={auxw}"));
            // The controls' resize gesture goes through the host: the saved rect and the tab view must follow.
            let before = self.core().state().pip_bounds;
            let host = crate::phase3::pip_host(&self.core());
            {
                use signet_ui::screens::windows::pip::{PipHandle, PipResizePhase};
                host.resize(PipResizePhase::Start { handle: PipHandle::NW });
                host.resize(PipResizePhase::Move { handle: PipHandle::NW, dx: -200.0, dy: -150.0 });
                host.resize(PipResizePhase::End);
            }
            self.sleep(1200).await;
            let after = self.core().state().pip_bounds;
            let view = self.core().views.view_of(&vtab).map(|v| v.bounds());
            let same = match (after, view) {
                (Some(a), Some(v)) => (f64::from(v.width) - a.width).abs() < 1.5 && (f64::from(v.height) - a.height).abs() < 1.5,
                _ => false,
            };
            self.rec("pip.resize", before != after && after.is_some() && same, format!("bounds {before:?} -> {after:?}, view {view:?}"));
            // GPUI must have learned the new native size: a stale viewport lays the controls out for the old card.
            let vps = crate::windows::pip::viewports();
            let tracked = match (after, vps) {
                (Some(a), Some((c, p))) => [c, p].iter().all(|v| (f64::from(v.0) - a.width).abs() < 1.5 && (f64::from(v.1) - a.height).abs() < 1.5),
                _ => false,
            };
            self.rec("pip.viewport", tracked, format!("saved {after:?}, viewports (controls, panel) {vps:?}"));
            // A command is read back and rendered right away, not at the next 250 ms tick.
            let last = |t: &Self| t.seen(0, &["pip-state"]).last().cloned().unwrap_or_default();
            let (before, m) = (last(self), self.mark());
            let t0 = std::time::Instant::now();
            self.b.core.borrow_mut().pip_command(signet_core::types::PipCommand::Toggle, None);
            let mut got = None;
            while t0.elapsed().as_millis() < 400 {
                self.sleep(10).await;
                if let Some(l) = self.seen(m, &["pip-state"]).into_iter().next() {
                    got = Some((l, t0.elapsed().as_millis()));
                    break;
                }
            }
            let ok = got.as_ref().is_some_and(|(l, ms)| *ms <= 150 && *l != before);
            self.rec("pip.command_latency", ok, format!("before={before:?} after={got:?}"));
            if let Some(ms) = std::env::var("SIGNET_PIP_HOLD").ok().and_then(|v| v.parse::<u64>().ok()) {
                eprintln!("[pip-diag] holding {ms} ms");
                self.sleep(ms).await;
            }
            self.b.core.borrow_mut().pip_command(signet_core::types::PipCommand::Close, None);
            let freed = self.wait(5000, |c| !c.pip_owns(&vtab)).await;
            self.rec("pip.close", freed, format!("released={freed}"));

        }

        // ---------------------------------------------------------------- extensions
        if self.on("ext") {
            self.extensions().await;
        }
        if self.on("devtools") {
            self.devtools().await;
        }

        if self.on("quithold") {
            self.quit_hold().await;
        }
        self.finish().await;
    }

    /// Cmd+Q as the keyboard sends it: a tap or an early release must not quit, a full hold must.
    #[cfg(target_os = "macos")]
    async fn quit_hold(&mut self) {
        let Some(parent) = self.core().views.parent else {
            self.rec("quit.tap", false, "no main window");
            return;
        };
        if let Some(a) = self.core().active_tab_id().and_then(|t| self.core().views.view_of(&t)) {
            a.focus();
        }
        self.sleep(500).await;
        // Tap: down then up after 120 ms.
        Self::cmd_q(true, false, parent);
        self.sleep(120).await;
        Self::cmd_q(false, false, parent);
        self.sleep(700).await;
        let shown = self.core().quit.shown;
        let quit = self.core().quit.confirmed;
        self.rec("quit.tap", shown && !quit, format!("bubble shown={shown} quit={quit}"));
        // Held 600 ms with repeats, then released before the end of the gauge (900 ms).
        self.sleep(2500).await;
        Self::cmd_q(true, false, parent);
        for _ in 0..4 {
            self.sleep(130).await;
            Self::cmd_q(true, true, parent);
        }
        Self::cmd_q(false, false, parent);
        self.sleep(1500).await;
        let quit = self.core().quit.confirmed;
        self.rec("quit.early_release", !quit, format!("quit={quit} after an early release"));
        // Held to the end: the key repeats keep coming.
        self.sleep(2500).await;
        Self::cmd_q(true, false, parent);
        let t0 = std::time::Instant::now();
        let mut quit = false;
        while t0.elapsed().as_millis() < 3000 && !quit {
            self.sleep(30).await;
            quit = self.core().quit.confirmed;
            if !quit && t0.elapsed().as_millis() > 380 {
                Self::cmd_q(true, true, parent);
            }
        }
        // `quit.hold` is printed by the app when the hold is confirmed (the process exits right after).
        if !quit {
            self.rec("quit.hold", false, "no quit after 3 s of hold");
        }
    }

    #[cfg(target_os = "macos")]
    fn cmd_q(down: bool, repeat: bool, parent: raw_window_handle::RawWindowHandle) {
        let _ = signet_web::mac::post_key_event_ex(parent, 12, "q", 1 << 20, down, repeat);
    }

    /// Ctrl+Q as the key sources of Windows and Linux feed it (`quitkeys::feed`, drained by the main loop): a tap or an
    /// early release must not quit, a full hold must.
    #[cfg(not(target_os = "macos"))]
    async fn quit_hold(&mut self) {
        use crate::quitkeys::{Key, RawKey, feed};
        let ctrl_q = |down: bool, repeat: bool| {
            feed(RawKey { key: Key::Q, up: !down, repeat, ctrl: true, shift: false, alt: false, meta: false });
        };
        if self.core().win.is_none() {
            self.rec("quit.tap", false, "no main window");
            return;
        }
        self.sleep(500).await;
        // Tap: down then up after 120 ms.
        ctrl_q(true, false);
        self.sleep(120).await;
        ctrl_q(false, false);
        self.sleep(700).await;
        let shown = self.core().quit.shown;
        let quit = self.core().quit.confirmed;
        self.rec("quit.tap", shown && !quit, format!("bubble shown={shown} quit={quit}"));
        // Held 600 ms with repeats, then released before the end of the gauge.
        self.sleep(2500).await;
        ctrl_q(true, false);
        for _ in 0..4 {
            self.sleep(130).await;
            ctrl_q(true, true);
        }
        ctrl_q(false, false);
        self.sleep(1500).await;
        let quit = self.core().quit.confirmed;
        self.rec("quit.early_release", !quit, format!("quit={quit} after an early release"));
        // Held to the end: the key repeats keep coming, with no repeat flag (the tracker detects them).
        self.sleep(2500).await;
        ctrl_q(true, false);
        let t0 = std::time::Instant::now();
        let mut quit = false;
        while t0.elapsed().as_millis() < 3500 && !quit {
            self.sleep(30).await;
            quit = self.core().quit.confirmed;
            if !quit && t0.elapsed().as_millis() > 380 {
                ctrl_q(true, false);
            }
        }
        // `quit.hold` is printed by the app when the hold is confirmed (the process exits right after).
        if !quit {
            self.rec("quit.hold", false, "no quit after 3.5 s of hold");
        }
    }

    /// Docked tools: frontend address, load, connection (the frontend shows the Elements panel of the page).
    async fn devtools_connected(&self, dtab: &str) -> String {
        let Some(front) = self.core().phase3.devtools.view_of(dtab).and_then(WebView::from_id) else { return "no tools view".into() };
        // The frontend titles itself after the inspected page (host:port) once its WebSocket to the target answered.
        let host = self.base.trim_start_matches("http://").to_string();
        for _ in 0..40 {
            let r = self.eval(&front, "document.title").await;
            if let Some(Value::String(t)) = &r
                && t.contains(&host)
            {
                return format!("title={t:?} connected=true");
            }
            self.sleep(250).await;
        }
        let r = self.eval(&front, "location.href + ' | ' + document.title + ' | ' + document.readyState + ' | ' + document.body.innerText.slice(0, 200).replace(/\\s+/g, ' ')").await;
        format!("frontend never showed its panels: {r:?}")
    }

    /// `SIGNET_TEST_SHOTS=<folder>`: saves a screenshot of the tools view (diagnostics).
    async fn devtools_shot(&self, dtab: &str, name: &str) {
        let Some(dir) = std::env::var_os("SIGNET_TEST_SHOTS") else { return };
        let Some(front) = self.core().phase3.devtools.view_of(dtab).and_then(WebView::from_id) else { return };
        let slot: Rc<RefCell<Option<Vec<u8>>>> = Rc::new(RefCell::new(None));
        let s2 = slot.clone();
        front.capture_screenshot(&signet_web::ScreenshotOptions::default(), move |r| {
            if let Ok(b) = r {
                *s2.borrow_mut() = Some(b);
            }
        });
        for _ in 0..30 {
            if let Some(b) = slot.borrow_mut().take() {
                let _ = std::fs::write(std::path::Path::new(&dir).join(format!("{name}.png")), b);
                return;
            }
            self.sleep(100).await;
        }
    }

    /// Text of the elements of the frontend (shadow roots included) matching `selector`.
    async fn devtools_texts(&self, dtab: &str, selector: &str) -> String {
        let Some(front) = self.core().phase3.devtools.view_of(dtab).and_then(WebView::from_id) else { return "no tools view".into() };
        let sel = serde_json::to_string(selector).unwrap_or_default();
        let js = format!("(function(){{var out=[];(function f(r){{r.querySelectorAll({sel}).forEach(function(e){{out.push((e.textContent||'').trim().slice(0,60))}});r.querySelectorAll('*').forEach(function(e){{if(e.shadowRoot)f(e.shadowRoot)}})}})(document);return out.join(' | ')}})()");
        match self.eval(&front, &js).await {
            Some(Value::String(t)) => t,
            other => format!("{other:?}"),
        }
    }

    async fn devtools(&mut self) {
        let base = self.base.clone();
        let dtab = self.open_and_load(&format!("{base}/index.html")).await;
        let port = signet_web::devtools_port();
        self.rec("devtools.port", port.is_some(), format!("remote debugging port {port:?} (loopback)"));
        let m = self.mark();
        self.b.core.borrow_mut().toggle_dev_tools(Some(&dtab), false);
        let loaded = self.wait_trace(10_000, m, &["devtools-loaded", "/devtools/devtools_app.html?ws=127.0.0.1:"]).await;
        self.sleep(1500).await;
        let open = self.core().devtools_open(&dtab);
        let pane = self.core().panes.iter().find(|p| p.tab_id == dtab).cloned();
        self.rec("devtools.docked", open && pane.as_ref().is_some_and(|p| p.devtools.is_some()), format!("open={open} pane.devtools={:?} page rect={:?}", pane.as_ref().and_then(|p| p.devtools.as_ref().map(|d| d.rect)), pane.as_ref().map(|p| p.rect)));
        self.rec("devtools.loaded", loaded.is_some(), format!("{loaded:?} url={:?}", self.core().phase3.devtools.url_of(&dtab)));
        let conn = self.devtools_connected(&dtab).await;
        self.devtools_shot(&dtab, "docked").await;
        self.rec("devtools.connected", conn.ends_with("connected=true"), conn);
        // Console panel, then inspect mode on the page.
        self.b.core.borrow_mut().toggle_dev_tools(Some(&dtab), true);
        self.sleep(4000).await;
        self.devtools_shot(&dtab, "console").await;
        let tabs = self.devtools_texts(&dtab, "[role=tab][aria-selected=true]").await;
        self.rec("devtools.console", tabs.contains("Console"), format!("selected panel tabs: {tabs}"));
        let front = self.core().phase3.devtools.view_of(&dtab).and_then(WebView::from_id);
        let page = self.tab_view(&dtab);
        let (cx_, cy_) = (30, 45);
        self.b.core.borrow_mut().inspect_at(&dtab, cx_, cy_);
        self.sleep(4000).await;
        self.devtools_shot(&dtab, "inspect").await;
        let still = front.is_some_and(|f| f.is_alive()) && page.is_some_and(|p| p.is_alive());
        let mut selected = self.devtools_texts(&dtab, "li.selected").await;
        for _ in 0..8 {
            if selected.contains("<h1") {
                break;
            }
            self.sleep(500).await;
            selected = self.devtools_texts(&dtab, "li.selected").await;
        }
        self.rec("devtools.inspect", still && selected.contains("<h1"), format!("selected rows after inspectAt({cx_},{cy_}): {selected}"));
        // Only the frontend may load in the tools view.
        if let Some(f) = front {
            let m = self.mark();
            f.load_url("https://example.com/");
            let refused = self.wait(6000, |_| f.url().starts_with("about:blank")).await;
            let _ = m;
            self.rec("devtools.origin_guard", refused, format!("tools view url after a foreign navigation: {}", f.url()));
            f.load_url(&self.core().phase3.devtools.url_of(&dtab).unwrap_or_default());
            self.sleep(1500).await;
        }
        self.b.core.borrow_mut().set_dev_tools_dock(&dtab, DevToolsDock::Bottom);
        self.sleep(2500).await;
        let pane = self.core().panes.iter().find(|p| p.tab_id == dtab).cloned();
        let conn = self.devtools_connected(&dtab).await;
        self.rec("devtools.bottom", self.core().devtools_open(&dtab) && conn.ends_with("connected=true"), format!("page rect={:?} frame={:?} {conn}", pane.as_ref().map(|p| p.rect), self.frame_of(&dtab)));
        // Window mode: the `#devtools` window hosts the view under its header.
        let m = self.mark();
        self.b.core.borrow_mut().set_dev_tools_dock(&dtab, DevToolsDock::Window);
        let win = self.wait_trace(4000, m, &["win-opened Devtools"]).await;
        let m2 = self.mark();
        let loaded = self.wait_trace(10_000, m2, &["devtools-loaded"]).await;
        self.sleep(1000).await;
        let conn = self.devtools_connected(&dtab).await;
        self.devtools_shot(&dtab, "window").await;
        let vb = self.core().phase3.devtools.view_of(&dtab).and_then(WebView::from_id).map(|v| v.bounds());
        self.rec("devtools.window", win.is_some() && loaded.is_some() && conn.ends_with("connected=true"), format!("window={win:?} view bounds={vb:?} {conn}"));
        self.b.core.borrow_mut().close_dev_tools(&dtab);
        self.sleep(1500).await;
        let closed = !self.core().devtools_open(&dtab) && self.core().phase3.devtools.view_of(&dtab).is_none();
        self.rec("devtools.close", closed, "closed");
        // Open and close once more (reopen after window mode).
        self.b.core.borrow_mut().set_dev_tools_dock(&dtab, DevToolsDock::Right);
        self.sleep(3000).await;
        let again = self.core().devtools_open(&dtab);
        self.b.core.borrow_mut().close_dev_tools(&dtab);
        self.sleep(1000).await;
        self.rec("devtools.reopen", again && !self.core().devtools_open(&dtab), format!("reopened={again}"));
    }

    /// The space form (a text field in the overlay window) must receive real key events: the overlay window is the key
    /// window, a name typed through key events lands in the field and Enter creates the space.
    #[cfg(target_os = "macos")]
    async fn space_form(&mut self) {
        // Normal layout, then compact mode (a floating toolbar opens next to the popover).
        self.space_form_round("spaceform").await;
        self.b.core.borrow_mut().set_compact(true);
        self.sleep(1000).await;
        self.space_form_round("spaceform.compact").await;
        self.b.core.borrow_mut().set_compact(false);
        self.sleep(800).await;
        self.space_form_edit_round().await;
        self.sleep(800).await;
        self.sidebar_inputs().await;
    }

    /// Brings this app to the front (other apps running on the machine may take the activation, and then there is no
    /// key window to send key events to).
    #[cfg(target_os = "macos")]
    fn activate_app(&self) {
        // SAFETY: plain AppKit calls on the main thread (the self-test runs on the GPUI main thread).
        unsafe {
            let app: *mut objc2::runtime::AnyObject = objc2::msg_send![objc2::class!(NSApplication), sharedApplication];
            let _: () = objc2::msg_send![app, activateIgnoringOtherApps: true];
        }
    }

    /// One key press (down and up) sent to the key window, after making sure the app is active.
    #[cfg(target_os = "macos")]
    fn press(&self, code: u16, ch: &str) {
        use signet_web::mac;
        self.activate_app();
        mac::post_key_to_key_window(code, ch, 0, true);
        mac::post_key_to_key_window(code, ch, 0, false);
    }

    /// Types `text` (lowercase letters only) as key events sent to the key window.
    #[cfg(target_os = "macos")]
    async fn type_text(&self, text: &str) {
        for ch in text.chars() {
            let code: u16 = match ch {
                'a' => 0,
                's' => 1,
                'x' => 7,
                'z' => 6,
                'b' => 11,
                _ => continue,
            };
            self.press(code, &ch.to_string());
            self.sleep(120).await;
        }
    }

    /// Focuses the field of the local test page, as a click in the page would (the CEF view is first responder).
    #[cfg(target_os = "macos")]
    async fn focus_page_field(&self) -> Option<WebView> {
        let view = self.core().active_tab_id().and_then(|t| self.core().views.view_of(&t))?;
        view.focus();
        self.sleep(500).await;
        self.eval(&view, "(() => { const i = document.getElementById('t'); i.value = ''; i.focus(); return document.activeElement === i; })()").await;
        Some(view)
    }

    /// Whether keys typed now reach the page: the field of the test page receives `ab`.
    #[cfg(target_os = "macos")]
    async fn page_types(&self, view: &WebView) -> (bool, String) {
        self.eval(view, "document.getElementById('t').value = ''").await;
        self.type_text("ab").await;
        self.sleep(300).await;
        let v = self.eval(view, "document.getElementById('t').value").await;
        (v.as_ref().and_then(Value::as_str) == Some("ab"), format!("{v:?}"))
    }

    /// GPUI inputs inside the main window (vertical layout: sidebar), opened while the page owns the keyboard:
    /// the new space form and the inline tab rename must receive real key events, and the page must type again after.
    #[cfg(target_os = "macos")]
    async fn sidebar_inputs(&mut self) {
        use signet_web::mac;
        self.b.core.borrow_mut().set_tab_layout(signet_core::types::TabLayout::Vertical);
        self.sleep(1200).await;
        let parent = self.core().views.parent;
        let responder = || parent.map(mac::describe_first_responder).unwrap_or_default();
        let Some(view) = self.focus_page_field().await else {
            self.rec("spaceform.sidebar", false, "no active page view");
            return;
        };
        let (ok, d) = self.page_types(&view).await;
        self.rec("spaceform.sidebar_page_before", ok, format!("page field={d}; first responder: {}", responder()));
        // New space form in the sidebar.
        let before: Vec<String> = self.core().state().spaces.iter().map(|s| s.id.clone()).collect();
        self.focus_page_field().await;
        self.b.core.borrow_mut().edit_space(None);
        self.sleep(1500).await;
        let overlay = self.core().ov.payload.is_some();
        println!("[selftest] info sidebar form: overlay={overlay} first responder: {}", responder());
        self.type_text("zs").await;
        self.sleep(300).await;
        self.press(36, "\r");
        let made = self.wait(4000, |c| c.state().spaces.iter().any(|s| s.name == "zs")).await;
        self.rec("spaceform.sidebar_typing", made && !overlay, format!("space created from the sidebar form={made} (overlay open={overlay}); first responder: {}", responder()));
        let id = self.core().state().spaces.iter().find(|s| s.name == "zs" && !before.contains(&s.id)).map(|s| s.id.clone());
        if let Some(id) = id {
            self.b.core.borrow_mut().delete_space(&id);
            self.sleep(800).await;
        }
        let (ok, d) = self.page_types(&view).await;
        self.rec("spaceform.sidebar_page_after", ok, format!("page typing after the form: {d}; first responder: {}", responder()));
        // Form open, then a click in the page: the page owns the keyboard again and types.
        self.focus_page_field().await;
        self.b.core.borrow_mut().edit_space(None);
        self.sleep(1200).await;
        // The click lands on the field of the test page (so the page keeps a focused field to type in).
        let pane = self.core().panes.first().map(|p| p.rect);
        let r = self.eval(&view, "(() => { const r = document.getElementById('t').getBoundingClientRect(); return [r.left + r.width / 2, r.top + r.height / 2]; })()").await;
        if let (Some(raw), Some(pane), Some(Value::Array(c))) = (parent, pane, r) {
            self.activate_app();
            let (cx, cy) = (c.first().and_then(Value::as_f64).unwrap_or(0.0), c.get(1).and_then(Value::as_f64).unwrap_or(0.0));
            let _ = mac::post_click(raw, pane.x + cx, pane.y + cy);
        }
        self.sleep(800).await;
        let (ok, d) = self.page_types(&view).await;
        self.rec("spaceform.sidebar_click_page", ok, format!("page typing after a click in the page: {d}; first responder: {}", responder()));
        self.press(53, "\u{1b}");
        self.sleep(600).await;
        // Cancelled with Escape: the page gets the keyboard back.
        self.focus_page_field().await;
        self.b.core.borrow_mut().edit_space(None);
        self.sleep(1200).await;
        self.press(53, "\u{1b}");
        self.sleep(1000).await;
        let (ok, d) = self.page_types(&view).await;
        self.rec("spaceform.sidebar_escape_page", ok, format!("page typing after Escape: {d}; first responder: {}", responder()));
        // Inline tab rename.
        let Some(tab) = self.core().active_tab_id() else { return };
        self.focus_page_field().await;
        self.b.core.borrow_mut().emit(signet_core::api::UiEvent::RenameTab { tab_id: tab.clone() });
        self.sleep(1200).await;
        println!("[selftest] info tab rename: first responder: {}", responder());
        self.type_text("zx").await;
        self.sleep(300).await;
        self.press(36, "\r");
        let t2 = tab.clone();
        let renamed = self.wait(4000, move |c| c.state().tabs.get(&t2).and_then(|t| t.custom_title.clone()).is_some_and(|n| n.contains("zx"))).await;
        let title = self.core().state().tabs.get(&tab).and_then(|t| t.custom_title.clone());
        self.rec("spaceform.sidebar_rename", renamed, format!("custom title {title:?}; first responder: {}", responder()));
        self.b.core.borrow_mut().rename_tab(&tab, None);
        self.sleep(500).await;
        let (ok, d) = self.page_types(&view).await;
        self.rec("spaceform.sidebar_rename_page", ok, format!("page typing after the rename: {d}; first responder: {}", responder()));
    }

    #[cfg(target_os = "macos")]
    async fn space_form_round(&mut self, name: &str) {
        use signet_web::mac;
        let before: Vec<String> = self.core().state().spaces.iter().map(|s| s.id.clone()).collect();
        // As the user does: focus the page, open the spaces menu, then "new space" inside the same overlay window.
        if let Some(a) = self.core().active_tab_id().and_then(|t| self.core().views.view_of(&t)) {
            a.focus();
        }
        self.sleep(600).await;
        self.b.core.borrow_mut().open_popover_kind(signet_core::types::PopoverRequest::Spaces);
        self.sleep(1000).await;
        println!("[selftest] info spaces menu: key window {}", mac::describe_key_window());
        let anchor = signet_core::types::Rect::new(90.0, 6.0, 130.0, 30.0);
        self.b.core.borrow_mut().open_popover(signet_core::types::PopoverRequest::SpaceForm { space_id: None }, anchor);
        self.sleep(1500).await;
        let raw = self.core().lx.overlay_raw;
        let key = raw.is_some_and(mac::is_key_window);
        self.rec(&format!("{name}.key_window"), key, format!("overlay key={key} now: {}", mac::describe_key_window()));
        self.activate_app();
        // "Zq" then Enter, as the keyboard sends them to the key window.
        for (code, ch) in [(6u16, "Z"), (12u16, "q")] {
            let sent = mac::post_key_to_key_window(code, ch, 1 << 17, true);
            mac::post_key_to_key_window(code, ch, 1 << 17, false);
            if !sent {
                break;
            }
            self.sleep(150).await;
        }
        self.sleep(400).await;
        self.press(36, "\r");
        let made = self.wait(4000, |c| c.state().spaces.iter().any(|s| s.name == "Zq")).await;
        let id = self.core().state().spaces.iter().find(|s| s.name == "Zq" && !before.contains(&s.id)).map(|s| s.id.clone());
        self.rec(&format!("{name}.typing"), made, format!("space created with the typed name={made}; key window: {}", mac::describe_key_window()));
        if let Some(id) = id {
            self.b.core.borrow_mut().delete_space(&id);
            self.sleep(500).await;
        }
        self.b.core.borrow_mut().close_overlay(true);
        self.sleep(500).await;
        let main_key = self.core().views.parent.is_some_and(mac::is_key_window);
        self.rec(&format!("{name}.key_back"), main_key, format!("main window key after close={main_key}: {}", mac::describe_key_window()));
    }

    /// Edit of an existing space through real key events (Enter saves, Escape closes), then the name is restored.
    #[cfg(target_os = "macos")]
    async fn space_form_edit_round(&mut self) {
        use signet_web::mac;
        let (id, original, icon, hue, theme) = {
            let c = self.core();
            let sp = c.active_space();
            (sp.id.clone(), sp.name.clone(), sp.icon.clone(), sp.hue, sp.theme.clone())
        };
        let key = |code: u16, ch: &str, flags: usize| {
            let sent = mac::post_key_to_key_window(code, ch, flags, true);
            mac::post_key_to_key_window(code, ch, flags, false);
            sent
        };
        if let Some(a) = self.core().active_tab_id().and_then(|t| self.core().views.view_of(&t)) {
            a.focus();
        }
        self.sleep(600).await;
        // Escape closes the form.
        let anchor = signet_core::types::Rect::new(78.0, 6.0, 140.0, 28.0);
        self.b.core.borrow_mut().open_popover(signet_core::types::PopoverRequest::SpaceForm { space_id: Some(id.clone()) }, anchor);
        self.sleep(1200).await;
        let up = self.core().ov.payload.is_some();
        key(53, "\u{1b}", 0);
        let closed = self.wait(3000, |c| c.ov.payload.is_none()).await;
        self.rec("spaceform.escape", up && closed, format!("open={up} closed by Escape={closed}"));
        // Enter saves the edit: a letter typed into the name field, then Enter.
        self.b.core.borrow_mut().open_popover(signet_core::types::PopoverRequest::SpaceForm { space_id: Some(id.clone()) }, anchor);
        self.sleep(1200).await;
        self.activate_app();
        key(6, "Z", 1 << 17);
        self.sleep(400).await;
        key(36, "\r", 0);
        let id2 = id.clone();
        let orig2 = original.clone();
        let saved = self.wait(4000, move |c| c.state().spaces.iter().any(|s| s.id == id2 && s.name != orig2)).await;
        let now = self.core().state().spaces.iter().find(|s| s.id == id).map(|s| s.name.clone()).unwrap_or_default();
        self.rec("spaceform.edit_enter", saved, format!("name {original:?} -> {now:?}"));
        let draft = signet_core::types::SpaceDraft { name: original.clone(), icon, hue, theme };
        self.b.core.borrow_mut().update_space(&id, &draft);
        self.b.core.borrow_mut().close_overlay(true);
        self.sleep(600).await;
        let back = self.core().state().spaces.iter().find(|s| s.id == id).map(|s| s.name == original).unwrap_or(false);
        self.rec("spaceform.edit_restored", back, format!("name restored={back}"));
    }

    #[cfg(not(target_os = "macos"))]
    async fn space_form_edit_round(&mut self) {}

    #[cfg(not(target_os = "macos"))]
    async fn space_form(&mut self) {}

    #[cfg(not(target_os = "macos"))]
    async fn sidebar_inputs(&mut self) {}

    async fn windows(&mut self) {
        let mut cases: Vec<(&str, Win, Option<String>)> = vec![
            ("win.settings", Win::Settings, Some("privacy".into())),
            ("win.library", Win::Library, Some("history".into())),
            ("win.passkey", Win::Passkey, None),
            ("win.boost", Win::Boost, Some("127.0.0.1".into())),
            ("win.welcome", Win::Welcome, None),
        ];
        for (name, win, section) in cases.drain(..) {
            let m = self.mark();
            self.b.core.borrow().send(Cmd::OpenWin(win, section.clone()));
            let opened = self.wait_trace(4000, m, &[&format!("win-opened {win:?}")]).await;
            self.sleep(700).await;
            let sec = self.seen(m, &["settings-section"]);
            let mut ok = opened.is_some();
            let mut detail = format!("opened={}", opened.is_some());
            if win == Win::Settings {
                let right = sec.last().is_some_and(|l| l.contains("privacy"));
                ok &= right;
                detail.push_str(&format!(" section lines={sec:?}"));
                // Asking again focuses the window and moves to the other section.
                let m2 = self.mark();
                self.b.core.borrow().send(Cmd::OpenWin(Win::Settings, Some("shortcuts".into())));
                self.sleep(800).await;
                let sec2 = self.seen(m2, &["settings-section"]);
                let right2 = sec2.last().is_some_and(|l| l.contains("shortcuts"));
                ok &= right2;
                detail.push_str(&format!(" again={sec2:?}"));
            }
            let m3 = self.mark();
            self.b.core.borrow().send(Cmd::CloseWin(win));
            let closed = self.wait_trace(3000, m3, &[&format!("win-closed {win:?}")]).await.is_some();
            self.rec(name, ok && closed, format!("{detail} closed={closed}"));
            self.sleep(300).await;
        }
        // Install as app: the prompt is an overlay (the install itself writes outside the data folder: not run).
        let m = self.mark();
        let tab = self.core().active_tab_id();
        self.b.core.borrow_mut().prompt_install_web_app(tab.as_deref());
        self.sleep(1500).await;
        let payload = self.core().ov.payload.is_some();
        let url = tab.as_deref().map(|t| self.tab_info(t)).unwrap_or_default();
        self.rec("win.webapp_prompt", payload || !self.seen(m, &["surface-opened overlay"]).is_empty(), format!("overlay payload={payload} for {url}"));
        self.b.core.borrow_mut().close_overlay(true);
        self.sleep(500).await;
    }

    async fn extensions(&mut self) {
        if self.ext_dir.is_empty() {
            self.rec("ext.setup", false, "SIGNET_TEST_EXT is not set");
            return;
        }
        let m = self.mark();
        let dir = self.ext_dir.clone();
        self.b.core.borrow_mut().selftest_load_unpacked(&dir);
        let ok = self.wait(8000, |c| !c.installed_extension_list().is_empty()).await;
        let list = self.core().installed_extension_list();
        self.rec("ext.load", ok, format!("installed={list:?}"));
        let Some((id, _)) = list.first().cloned() else { return };
        // The background page: a chrome-extension:// document loaded in a hidden view.
        let bg_loaded = self.wait_trace(10_000, m, &["LoadEnd", "chrome-extension://"]).await;
        self.rec("ext.scheme", bg_loaded.is_some(), format!("{:?}", bg_loaded.map(|l| l.chars().take(200).collect::<String>())));
        let bg_ready = self.wait_trace(6000, m, &["BG_READY"]).await;
        let bgv = self.core().selftest_background(&id);
        if let Some(v) = bgv {
            let r = self.eval(&v, "(async () => { let f; try { const r = await fetch('/bg.js'); f = r.status + ' ' + (await r.text()).length } catch (e) { f = 'ERR ' + e } return JSON.stringify({ href: location.href, ready: document.readyState, scripts: [...document.scripts].map((s) => s.src || 'inline').slice(0, 5), res: performance.getEntriesByType('resource').map((r) => r.name), fetch: f, onMessage: typeof (chrome.runtime && chrome.runtime.onMessage) }) })()").await;
            println!("[selftest] info background page: {r:?}");
        }
        let bg = self.core().selftest_background(&id);
        let state = match &bg {
            Some(v) => self.eval(v, "(() => { const d = Object.getOwnPropertyDescriptor(window, '__signetDispatch'); return JSON.stringify({ dispatch: d && { w: d.writable, c: d.configurable, get: !!d.get }, post: typeof __signetPost, crx: typeof __signetCrx, cfg: typeof __signetExtensionConfig, chrome: typeof chrome, rt: typeof (window.chrome && chrome.runtime), send: typeof (window.chrome && chrome.runtime && chrome.runtime.sendMessage) }) })()").await,
            None => None,
        };
        self.rec("ext.background", bg.is_some() && state.as_ref().and_then(Value::as_str).is_some_and(|s| s.contains("\"send\":\"function\"")), format!("view={} BG_READY console={} chrome in bg={state:?}", bg.is_some(), bg_ready.is_some()));
        // Content script -> background -> reply.
        let rt = self.open_and_load(&format!("{}/index.html", self.base)).await;
        let cs = self.wait_trace(8000, m, &["CS_GOT", "pong"]).await;
        let bgm = self.seen(m, &["BG_GOT"]);
        self.rec("ext.sendMessage", cs.is_some(), format!("content script reply={:?} background received={} tab={}", cs.map(|l| l.chars().take(200).collect::<String>()), !bgm.is_empty(), self.tab_info(&rt)));
        // Popup.
        self.b.core.borrow_mut().selftest_click_extension(&id);
        let up = self.wait(5000, |c| c.selftest_popup().is_some()).await;
        let pm = self.wait_trace(6000, m, &["POPUP_GOT", "pong"]).await;
        self.rec("ext.popup", up && pm.is_some(), format!("popup open={up} POPUP_GOT={:?}", pm.map(|l| l.chars().take(200).collect::<String>())));
        self.b.core.borrow_mut().selftest_click_extension(&id);
        self.sleep(800).await;
        self.rec("ext.popup_close", self.core().selftest_popup().is_none(), "closed by a second click");
    }

    /// Context menu: opens one at a fixed point, lets the menu dismiss itself (nothing is chosen) and checks the
    /// callback did not run. macOS runs it only with `SIGNET_GPUI_MENU=1` (its native menu cannot be closed from here).
    async fn context_menu(&mut self) {
        use crate::windows::{Entry, popup_menu};
        if cfg!(target_os = "macos") && !popup_menu::wanted() {
            return;
        }
        popup_menu::set_autoclose(1200);
        let m = self.mark();
        let entries = vec![
            Entry::Item { label: "First & one".into(), id: "first".into(), enabled: true, checked: true },
            Entry::Sep,
            Entry::Item { label: "Off".into(), id: "off".into(), enabled: false, checked: false },
            Entry::Sub { label: "More".into(), items: vec![Entry::Item { label: "Deep".into(), id: "deep".into(), enabled: true, checked: false }] },
        ];
        self.b.core.borrow_mut().popup(entries, (60.0, 60.0), |c, id| c.note(format!("menu-callback {id}")));
        let shown = self.wait_trace(6000, m, &["popup-shown"]).await;
        let closed = self.wait_trace(8000, m, &["popup-closed"]).await;
        popup_menu::set_autoclose(0);
        let ran = !self.seen(m, &["menu-callback"]).is_empty();
        self.rec("menu.popup", shown.is_some() && closed.as_deref().is_some_and(|l| l.contains("picked=none")) && !ran, format!("shown={shown:?} closed={closed:?} callback ran={ran}"));
    }

    async fn finish(&mut self) {
        let errs: Vec<String> = self.seen(0, &["RenderProcessGone"]);
        self.rec("renderers", errs.is_empty(), format!("render process gone events: {}", errs.len()));
        println!("[selftest] ---- summary: {} passed, {} failed", self.pass.get(), self.fail.get());
        for (n, ok, d) in self.rows.borrow().iter() {
            println!("[selftest-table] {:<22} {:<5} {}", n, if *ok { "PASS" } else { "FAIL" }, d.chars().take(180).collect::<String>());
        }
        self.sleep(500).await;
        self.b.core.borrow().send(Cmd::Quit);
    }
}

//! Port of `browser/layout.ts`: pane rectangles, card insets, split view, dividers, tabs on top, compact mode,
//! floating sidebar and toolbar, page fullscreen. Publishes `panes`/`dividers` and places the native views.

use std::collections::HashMap;

use raw_window_handle::RawWindowHandle;
use signet_core::model::{displayed_split, displayed_tab_ids, locate};
use signet_core::slide::{slide_at, slide_rect, SlideMotion, SlidePlan, Span};
use signet_core::split::{compute_split, SplitShape};
use signet_core::types::{OverlayPayload, Pane, Rect};
use signet_web::{Bounds, ProfileKind, WebView};

use super::Core;
use crate::windows::{Cmd, Later};

/// Window-level state of the layout that does not belong to another module: the sidebar slide (`layout.ts`
/// `slideMotion`), the overlay window handle (the Peek view lives in it) and the gesture marks Peek needs.
#[derive(Default)]
pub struct LayoutExtra {
    /// Sidebar slide in progress: the spring (shared/slide) and where the pages are on it (1 shown, 0 hidden).
    pub slide: Option<SlideMotion>,
    pub progress: Option<f64>,
    /// Views that follow the slide, with their laid-out rectangle.
    slid: Vec<(WebView, Rect)>,
    /// Native window of the overlay surface while it is open.
    pub overlay_raw: Option<RawWindowHandle>,
    /// Last form submission per tab (ms): the navigation that follows is not a link, so no Peek.
    pub submits: HashMap<String, i64>,
    /// Last click per tab (ms, page point).
    pub clicks: HashMap<String, (i64, Option<(f64, f64)>)>,
    /// Last Peek payload sent (title and address changes are sent again).
    pub peek_sent: Option<(String, String)>,
}

/// Shadow margin around the compact overlays (password prompt, zoom bubble).
const PW_MARGIN: f64 = 24.0;
/// Width of the prompt card (`WIDTH` in `overlay/password.rs`): wide enough for the three French buttons.
const PW_BODY_W: f64 = 360.0;
const PW_W: f64 = PW_BODY_W + PW_MARGIN * 2.0;
const PW_H: f64 = 152.0 + PW_MARGIN * 2.0;
const ZOOM_BODY_W: f64 = 248.0;
const ZOOM_W: f64 = ZOOM_BODY_W + PW_MARGIN * 2.0;
const ZOOM_H: f64 = 52.0 + PW_MARGIN * 2.0;

/// Overlay rectangle of the zoom bubble: top right of the card, under the panel bar unless there is none.
pub fn zoom_bounds(card: &Rect, w: f64, compact: bool) -> Rect {
    let x = (card.x + card.width - ZOOM_BODY_W - 12.0 - PW_MARGIN).min(w - ZOOM_W).max(0.0);
    Rect::new(x, (card.y + if compact { 8.0 } else { PANE_BAR + 4.0 } - PW_MARGIN).max(0.0), ZOOM_W, ZOOM_H)
}

/// Overlay rectangle of the unanchored password prompt: top right of the card, shadow margin included.
pub fn password_bounds(card: &Rect, w: f64) -> Rect {
    let x = (card.x + card.width - PW_BODY_W - 12.0 - PW_MARGIN).min(w - PW_W).max(0.0);
    Rect::new(x, (card.y + 12.0 - PW_MARGIN).max(0.0), PW_W, PW_H)
}

/// Peek card: centered, 300 to 1100 px wide, under a 64 px margin (`peek.ts`).
pub fn peek_card(width: f64, height: f64) -> Rect {
    let w = (width - 160.0).min(1100.0).max(300.0);
    Rect::new(((width - w) / 2.0).round(), 64.0, w, (height - 96.0).max(200.0))
}

pub const PAD: f64 = 6.0;
/// Height of the panel bar at the top of each card.
pub const PANE_BAR: f64 = 40.0;
/// Height of the tab bar (tabs on top) and gap below it.
pub const TAB_BAR: f64 = 40.0;
pub const TAB_BAR_GAP: f64 = 4.0;
pub const BOOKMARK_BAR: f64 = signet_features::bookmarks::BOOKMARK_BAR as f64;
pub const COMPACT_MARGIN: f64 = 6.0;
const FIND_W: f64 = 360.0;
const FIND_H: f64 = 48.0;
const FIND_MARGIN: f64 = 12.0;
const TOAST_W: f64 = 384.0;
const TOAST_H: f64 = 72.0;
pub const APP_BASE: &str = "signet://app";
/// Base of the note address mapping: empty, because notes and Easels are served at `signet://note/<id>` themselves
/// (there is no UI page with a `#note/<id>` fragment as in the Electron app).
pub const DOC_BASE: &str = "";

fn web(r: Rect) -> Bounds {
    Bounds::new(r.x as f32, r.y as f32, r.width.max(1.0) as f32, r.height.max(1.0) as f32)
}

impl Core {
    /// Profile of a tab's view: the isolated space's own, otherwise the shared one (web/profiles.ts).
    pub fn profile_for_tab(&self, tab_id: &str) -> ProfileKind {
        let st = self.state();
        match locate(st, tab_id).and_then(|l| l.space).and_then(|i| st.spaces.get(i)) {
            Some(s) if s.is_isolated() => ProfileKind::persistent(format!("space-{}", s.id)),
            _ => ProfileKind::persistent("default"),
        }
    }

    /// Creates the tab's view if it has none.
    pub fn ensure_view(&mut self, tab_id: &str, bounds: Rect, visible: bool) -> Option<WebView> {
        if let Some(v) = self.views.view_of(tab_id) {
            return Some(v);
        }
        let tab = self.state().tabs.get(tab_id)?.clone();
        let profile = self.profile_for_tab(tab_id);
        let url = if tab.url.is_empty() { "about:blank".to_string() } else { signet_features::internal::resolve_internal(DOC_BASE, &tab.url) };
        let v = self.views.create(tab_id, profile, &url, web(bounds), visible)?;
        if let Some(t) = self.store.data.state.tabs.get_mut(tab_id) {
            t.loaded = true;
        }
        self.log(format!("view created for {tab_id} -> {url}"));
        Some(v)
    }

    /// Recomputes the layout of the main window and moves the native views.
    pub fn layout_update(&mut self) {
        let Some(win) = &self.win else {
            self.panes.clear();
            self.dividers.clear();
            self.desired = Default::default();
            return;
        };
        let (w, h) = win.size;
        let card_r = signet_core::floating::web_card_radius(win.corner) as f32;
        let space = self.active_space().clone();
        let compact = self.state().settings.compact_mode;
        let side_w = self.live_sidebar_width.unwrap_or(self.state().sidebar_width);
        let sidebar_on = self.sidebar_on();
        let right = self.sidebar_right();
        let horizontal = self.horizontal();
        // Slide in progress: the geometry of the hidden sidebar, the pages move with its edge (`put_slid`).
        let side = if sidebar_on && self.lx.progress.is_none() { side_w } else { PAD };
        self.lx.slid.clear();
        let x0 = if right { PAD } else { side };
        let top = if horizontal { TAB_BAR + TAB_BAR_GAP } else { PAD };
        let area = if compact {
            Rect::new(COMPACT_MARGIN, COMPACT_MARGIN, w - COMPACT_MARGIN * 2.0, h - COMPACT_MARGIN * 2.0)
        } else {
            Rect::new(x0, top, w - PAD - side, h - top - PAD)
        };
        self.content_area = area;
        let ids = displayed_tab_ids(self.state(), &space);
        let split = displayed_split(self.state(), &space);
        let shape = split.as_ref().map(SplitShape::from).unwrap_or_else(|| SplitShape { tab_ids: ids.clone(), ..Default::default() });
        let geometry = compute_split(area, &shape);
        self.dividers = match &split {
            Some(s) => geometry.dividers.iter().cloned().map(|d| d.into_divider(&s.id)).collect(),
            None => Vec::new(),
        };
        let active = space.active_tab_id.clone();
        let bar = if compact { 0.0 } else { PANE_BAR + if self.bookmarks_visible() { BOOKMARK_BAR } else { 0.0 } };
        let full_tab = self.fullscreen.clone().filter(|t| ids.contains(t));
        let mut panes = Vec::new();
        for (i, tab_id) in ids.iter().enumerate() {
            let card = geometry.rects.get(i).copied().unwrap_or(area);
            // While the page is in the mini player, the main window does not display it.
            if self.pip_owns(tab_id) {
                panes.push(Pane { tab_id: tab_id.clone(), rect: Rect::new(card.x, card.y + bar, card.width, (card.height - bar).max(0.0)), card, focused: active.as_deref() == Some(tab_id.as_str()), devtools: None, responsive: None });
                continue;
            }
            let ex = self.pane_extras(tab_id, card, bar);
            let rect = ex.rect;
            let shown = full_tab.as_ref().map(|f| f == tab_id).unwrap_or(true);
            let is_full = full_tab.as_deref() == Some(tab_id.as_str());
            let placed = if is_full { Rect::new(0.0, 0.0, w, h) } else { rect };
            if let Some(v) = self.ensure_view(tab_id, placed, shown) {
                v.set_corner_radius(if is_full { 0.0 } else { card_r });
                if is_full {
                    v.set_bounds(web(placed));
                } else {
                    self.put_slid(v, placed);
                }
                v.set_visible(shown);
            }
            panes.push(Pane { tab_id: tab_id.clone(), rect, card: ex.card, focused: active.as_deref() == Some(tab_id.as_str()), devtools: ex.devtools, responsive: ex.responsive });
        }
        // Hide the views of tabs no longer displayed, and track since when.
        let now = signet_core::ids::now_ms();
        for id in self.views.ids() {
            if ids.contains(&id) || self.ov.peek_tab.as_deref() == Some(id.as_str()) || self.pip_owns(&id) {
                self.hidden_since.remove(&id);
            } else {
                if let Some(v) = self.views.view_of(&id) {
                    v.set_visible(false);
                }
                self.hidden_since.entry(id).or_insert(now);
            }
        }
        self.panes = panes;
        self.shown_tabs = ids.clone();
        self.devtools_after_layout(&ids, full_tab.is_some());
        self.place_peek();
        // Focus: the active page gets the keyboard unless an overlay or the find bar wants it.
        if let Some(a) = &active
            && self.ov.payload.is_none()
            && self.find.tab_id.is_none()
            && self.focus_pending
        {
            self.focus_pending = false;
            if let Some(v) = self.views.view_of(a) {
                v.focus();
            }
        }
        if self.debug {
            for p in &self.panes {
                self.log(format!("pane {} rect=({:.0},{:.0},{:.0},{:.0})", p.tab_id, p.rect.x, p.rect.y, p.rect.width, p.rect.height));
            }
        }
        self.compute_desired(w, h, area, full_tab.is_some());
    }

    /// Rectangles (window coordinates) of the floating surfaces.
    fn compute_desired(&mut self, w: f64, h: f64, area: Rect, fullscreen: bool) {
        let mut d = super::super::windows::Desired::default();
        let side_w = self.live_sidebar_width.unwrap_or(self.state().sidebar_width);
        let right = self.sidebar_right();
        let tb = signet_core::floating::toolbar_rect(w, area, self.horizontal());
        if !fullscreen {
            let st = self.state();
            if self.hover.sidebar && !self.sidebar_on() && !self.horizontal() && self.ov.payload.is_none() {
                d.sidebar = Some(signet_core::floating::sidebar_rect(w, h, side_w, right));
            }
            if st.settings.compact_mode && (self.hover.toolbar || matches!(self.ov.payload, Some(signet_core::types::OverlayPayload::Popover { .. }))) {
                d.toolbar = Some(tb);
            }
            if self.find.tab_id.is_some()
                && let Some(p) = self.panes.iter().find(|p| p.focused)
            {
                d.find = Some(Rect::new((p.rect.x).max(p.rect.x + p.rect.width - FIND_W - FIND_MARGIN), p.rect.y + FIND_MARGIN, FIND_W, FIND_H));
            }
            if self.notices.current.is_some() && !self.sidebar_on() {
                let tw = TOAST_W.min(area.width.max(0.0));
                d.toast = Some(Rect::new((area.x + (area.width - tw) / 2.0).round(), (area.y + area.height - TOAST_H).max(0.0), tw, TOAST_H));
            }
            if let Some(p) = &self.preview {
                d.preview = Some(*p);
            }
        }
        if self.ov.payload.is_some() || self.ov.leaving {
            // The password prompt without anchor and the zoom bubble only cover a corner of their card.
            d.overlay = Some(match &self.ov.payload {
                Some(OverlayPayload::Password { anchor: None, card, .. }) => password_bounds(card, w),
                Some(OverlayPayload::Zoom { card, compact, .. }) => zoom_bounds(card, w, *compact),
                _ => Rect::new(0.0, 0.0, w, h),
            });
        }
        if self.quit.shown {
            d.quit = Some(Rect::new((w - 320.0) / 2.0, (h - 120.0) / 2.0, 320.0, 120.0));
        }
        // Warm-up: once the sidebar stays hidden for a while its window is created and parked, so a reveal only slides it
        // in (a fullscreen video keeps the parked window).
        if !self.can_float_sidebar() {
            self.hover.sidebar_warm = false;
            self.hover.sidebar_warming = false;
        } else if !self.hover.sidebar_warm && !self.hover.sidebar_warming {
            self.hover.sidebar_warming = true;
            self.send(Cmd::Later(signet_core::floating::SIDEBAR_WARM_MS, Later::SidebarWarm));
        }
        if self.hover.sidebar_warm && self.can_float_sidebar() {
            d.sidebar_warm = Some(signet_core::floating::sidebar_rect(w, h, side_w, right));
        }
        // Same for the compact toolbar window (macOS parks it; the other platforms ignore the warm rectangle).
        if !self.can_float_toolbar() {
            self.hover.toolbar_warm = false;
            self.hover.toolbar_warming = false;
        } else if !self.hover.toolbar_warm && !self.hover.toolbar_warming {
            self.hover.toolbar_warming = true;
            self.send(Cmd::Later(signet_core::floating::TOOLBAR_WARM_MS, Later::ToolbarWarm));
        }
        if self.hover.toolbar_warm && self.can_float_toolbar() {
            d.toolbar_warm = Some(tb);
        }
        self.desired = d;
        self.update_traffic_lights();
    }

    /// macOS: the traffic lights follow the sidebar (hidden with it), like `LayoutManager.update`.
    #[cfg(target_os = "macos")]
    fn update_traffic_lights(&mut self) {
        let (compact, right, horizontal, sidebar_on) = (self.state().settings.compact_mode, self.sidebar_right(), self.horizontal(), self.sidebar_on());
        thread_local! {
            static LAST: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
        }
        let floating = self.hover.toolbar || self.hover.sidebar;
        let visible = if horizontal || right { !compact || floating } else { sidebar_on || (compact && floating) };
        if LAST.with(|l| l.replace(Some(visible))) == Some(visible) || self.lx.progress.is_some() && visible {
            return;
        }
        if let Some(raw_window_handle::RawWindowHandle::AppKit(a)) = self.views.parent {
            crate::mac::set_traffic_lights_visible(a.ns_view.as_ptr(), visible);
        }
    }

    #[cfg(not(target_os = "macos"))]
    fn update_traffic_lights(&mut self) {}

    /// Requests the keyboard for the active page at the next layout.
    pub fn request_focus(&mut self) {
        self.focus_pending = true;
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Sidebar slide (`layout.ts` slidePlan/stepSlide, `shell.ts` startSlide)
// ---------------------------------------------------------------------------------------------------------------

fn reduced_motion() -> bool {
    #[cfg(target_os = "macos")]
    {
        crate::mac::prefers_reduced_motion()
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

/// Interval between two steps of the slide (ms).
pub const SLIDE_STEP_MS: u64 = 8;
/// A slide from rest starts this much later (ms): the UI must first draw the new state.
const SLIDE_START_DELAY_MS: f64 = 60.0;
/// Closing on a Mac also hides the traffic lights, which relays the window out first.
const SLIDE_CLOSE_DELAY_MS: f64 = 140.0;

impl Core {
    /// The slide as sent to the main UI; `None` at rest.
    pub fn slide_plan(&self) -> Option<SlidePlan> {
        let motion = self.lx.slide?;
        self.lx.progress?;
        let a = self.content_area;
        let bar = self.live_sidebar_width.unwrap_or(self.state().sidebar_width);
        Some(SlidePlan { from: Span { x: a.x, width: a.width }, width: bar, gap: PAD, right: self.sidebar_right(), motion })
    }

    /// A page during the slide: at its size with the sidebar shown, moved with the edge next to the bar.
    fn slid_rect(rect: Rect, plan: &SlidePlan, p: f64) -> Rect {
        let shown = slide_at(plan, 1.0);
        let now = slide_at(plan, p);
        let r = slide_rect(rect, &shown, false);
        let shift = if plan.right { now.to.x + now.to.width - (shown.to.x + shown.to.width) } else { now.to.x - shown.to.x };
        Rect { x: r.x + shift, ..r }
    }

    /// Places a page view, following the slide if one is playing.
    fn put_slid(&mut self, v: WebView, rect: Rect) {
        let at = match (self.slide_plan(), self.lx.progress) {
            (Some(plan), Some(p)) => {
                self.lx.slid.push((v, rect));
                Self::slid_rect(rect, &plan, p)
            }
            _ => rect,
        };
        v.set_bounds(web(at));
    }

    /// Starts sliding the sidebar toward shown or hidden, from where it is now with its speed.
    pub fn start_slide(&mut self, show: bool) {
        if reduced_motion() {
            return self.stop_slide();
        }
        let now = signet_core::ids::now_ms() as f64;
        let moving = self.lx.slide;
        let cur = match moving {
            Some(m) => signet_core::slide::slide_state(&m, now),
            None => signet_core::slide::SlideState { progress: if show { 0.0 } else { 1.0 }, velocity: 0.0, done: false },
        };
        let start = if moving.is_some() { now } else { now + if !show && crate::app::platform::IS_MAC { SLIDE_CLOSE_DELAY_MS } else { SLIDE_START_DELAY_MS } };
        let running = moving.is_some();
        self.lx.slide = Some(SlideMotion { start, from: cur.progress, velocity: cur.velocity, target: if show { 1.0 } else { 0.0 } });
        self.lx.progress = Some(cur.progress);
        if !running {
            self.send(crate::windows::Cmd::Later(SLIDE_STEP_MS, crate::windows::Later::SlideStep));
        }
    }

    /// Ends the slide at once (the layout follows on the caller's next change).
    pub fn stop_slide(&mut self) {
        self.lx.slide = None;
        self.lx.progress = None;
        self.lx.slid.clear();
    }

    /// One step of the slide: moves the pages, tells the main UI where the bar is.
    pub fn slide_step(&mut self) {
        let Some(plan) = self.slide_plan() else { return };
        let now = signet_core::ids::now_ms() as f64;
        let st = signet_core::slide::slide_state(&plan.motion, now);
        self.lx.progress = Some(st.progress);
        let slid = self.lx.slid.clone();
        for (v, rect) in slid {
            v.set_bounds(web(Self::slid_rect(rect, &plan, st.progress)));
        }
        let lead = if plan.motion.target == 0.0 { signet_core::slide::SLIDE_UI_LEAD_MS } else { 0.0 };
        self.emit(signet_core::api::UiEvent::SidebarSlide(signet_core::slide::slide_state(&plan.motion, now + lead).progress));
        if st.done {
            self.emit(signet_core::api::UiEvent::SidebarSlide(plan.motion.target));
            self.stop_slide();
            self.changed_view();
        } else {
            self.send(crate::windows::Cmd::Later(SLIDE_STEP_MS, crate::windows::Later::SlideStep));
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Peek (`peek.ts`): a detached tab in a card over the page, which can become a tab
// ---------------------------------------------------------------------------------------------------------------

impl Core {
    /// Opens `url` in Peek (a link that should not replace the page). `from_tab`: the page that asked.
    pub fn open_peek(&mut self, url: &str, from_tab: Option<&str>) {
        let Some(win) = &self.win else { return };
        let (w, h) = win.size;
        if self.ov.peek_tab.is_some() {
            self.drop_peek();
        }
        let tab = signet_core::factory::new_tab(url, "");
        let id = tab.id.clone();
        self.store.data.state.tabs.insert(id.clone(), tab);
        self.store.data.detached.insert(id.clone());
        self.hover.sidebar = false;
        self.hover.toolbar = false;
        let _ = from_tab;
        self.ov.peek_tab = Some(id.clone());
        let payload = OverlayPayload::Peek { tab_id: id, url: url.to_string(), title: url.to_string(), card: peek_card(w, h), origin: None };
        self.lx.peek_sent = Some((url.to_string(), url.to_string()));
        self.show_overlay_raw(payload);
        self.request_focus();
    }

    /// The navigation of `tab` toward `url` should open in Peek instead (a link of a pinned or favorite tab that
    /// leaves its site, `peekExternalLinks`): only web pages, after a click, never after a form submission.
    /// Returns true when Peek took it (the caller cancels the navigation).
    pub fn peek_navigation(&mut self, tab: &str, url: &str) -> bool {
        let Some(t) = self.state().tabs.get(tab) else { return false };
        let now = signet_core::ids::now_ms();
        let url_ok = url.starts_with("http://") || url.starts_with("https://");
        let pinned = t.pinned_url.is_some();
        let other_site = signet_core::site::site_of(url) != signet_core::site::site_of(&t.url);
        let clicked = self.lx.clicks.get(tab).is_some_and(|(at, _)| now - at < 1500);
        let submitted = self.lx.submits.get(tab).is_some_and(|at| now - at < 2000);
        if !(self.state().settings.peek_external_links && pinned && other_site && url_ok && clicked && !submitted) {
            return false;
        }
        self.open_peek(url, Some(tab));
        true
    }

    /// A link that asks for a new window: Peek, unless the tab has no tabs around it (Peek, Little Signet), where
    /// the caller navigates in place. Returns true when Peek took it.
    pub fn peek_new_window(&mut self, opener: &str, url: &str) -> bool {
        if !(url.starts_with("http://") || url.starts_with("https://")) || self.store.data.detached.contains(opener) || self.win.is_none() {
            return false;
        }
        self.open_peek(url, Some(opener));
        true
    }

    /// A form was submitted in `tab` (`Accepted::PageSubmit`): the navigation that follows is not a link.
    pub fn note_form_submit(&mut self, tab: &str) {
        self.lx.submits.insert(tab.to_string(), signet_core::ids::now_ms());
    }

    /// The user clicked in `tab` (page point): a navigation soon after comes from a link.
    pub fn note_click(&mut self, tab: &str, point: Option<(f64, f64)>) {
        self.lx.clicks.insert(tab.to_string(), (signet_core::ids::now_ms(), point));
    }

    /// Peek's address or title changed: its card is sent again.
    pub fn peek_refresh(&mut self) {
        let Some(id) = self.ov.peek_tab.clone() else { return };
        let Some(t) = self.state().tabs.get(&id) else { return };
        let (url, title) = (t.url.clone(), if t.title.is_empty() { t.url.clone() } else { t.title.clone() });
        if self.lx.peek_sent.as_ref() == Some(&(url.clone(), title.clone())) {
            return;
        }
        let Some(win) = &self.win else { return };
        let card = peek_card(win.size.0, win.size.1);
        self.lx.peek_sent = Some((url.clone(), title.clone()));
        self.ov.payload = Some(OverlayPayload::Peek { tab_id: id, url, title, card, origin: None });
        self.emit_overlay();
    }

    /// The Peek tab, if one is open.
    pub fn peek_tab(&self) -> Option<&str> {
        self.ov.peek_tab.as_deref()
    }

    /// Closes the Peek: its view and its detached tab go.
    pub(super) fn drop_peek(&mut self) {
        let Some(id) = self.ov.peek_tab.take() else { return };
        self.views.destroy(&id);
        self.store.data.state.tabs.shift_remove(&id);
        self.store.data.detached.remove(&id);
        self.upgraded.remove(&id);
        self.typed.remove(&id);
        self.lx.peek_sent = None;
    }

    /// The Peek becomes a tab of the active space (the view moves back to the main window).
    pub fn expand_peek(&mut self) {
        let Some(id) = self.ov.peek_tab.take() else { return };
        let view = self.views.take(&id);
        self.ov.payload = None;
        self.ov.leaving = false;
        self.lx.peek_sent = None;
        self.emit(signet_core::api::UiEvent::OverlayHide { instant: true });
        self.model(|m| m.adopt(&id));
        if let Some(v) = view {
            self.views.put(&id, v);
            if let Some(raw) = self.views.parent {
                let _ = v.reparent(raw);
            }
            if let Some(t) = self.store.data.state.tabs.get_mut(&id) {
                t.loaded = true;
            }
        }
        self.request_focus();
        self.changed();
    }

    /// Places (creating it in the overlay window if needed) the Peek view on the card.
    fn place_peek(&mut self) {
        let Some(id) = self.ov.peek_tab.clone() else { return };
        let Some(win) = &self.win else { return };
        let card = peek_card(win.size.0, win.size.1);
        if let Some(v) = self.views.view_of(&id) {
            v.set_bounds(web(card));
            v.set_visible(true);
            return;
        }
        // The view is a child of the overlay window, drawn above its scrim: wait for that window.
        let Some(raw) = self.lx.overlay_raw else { return };
        let saved = self.views.parent.replace(raw);
        let v = self.ensure_view(&id, card, true);
        self.views.parent = saved;
        if let Some(v) = v {
            v.focus();
        }
    }

    /// The overlay window exists (or is gone): the Peek view needs it.
    pub fn overlay_window_changed(&mut self, raw: Option<RawWindowHandle>) {
        self.lx.overlay_raw = raw;
        if raw.is_some() && self.ov.peek_tab.is_some() {
            self.place_peek();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insets_match_the_floating_inset() {
        assert_eq!(PAD, signet_core::floating::FLOAT_INSET);
        assert_eq!(COMPACT_MARGIN, signet_core::floating::FLOAT_INSET);
    }

    #[test]
    fn password_prompt_hugs_the_card_corner() {
        let card = Rect::new(6.0, 6.0, 1000.0, 700.0);
        let r = password_bounds(&card, 1200.0);
        assert_eq!((r.x, r.y, r.width, r.height), (1000.0 + 6.0 - 360.0 - 12.0 - 24.0, 0.0, 408.0, 200.0));
        // Never past the window's right edge.
        let narrow = password_bounds(&Rect::new(0.0, 0.0, 2000.0, 700.0), 800.0);
        assert_eq!(narrow.x, 800.0 - 408.0);
    }

    #[test]
    fn zoom_bubble_sits_under_the_panel_bar() {
        let card = Rect::new(6.0, 6.0, 1000.0, 700.0);
        let r = zoom_bounds(&card, 1200.0, false);
        assert_eq!((r.width, r.height), (296.0, 100.0));
        assert_eq!(r.y, 6.0 + PANE_BAR + 4.0 - 24.0);
        assert_eq!(zoom_bounds(&card, 1200.0, true).y, 0.0);
    }

    #[test]
    fn peek_card_is_centered_within_limits() {
        let c = peek_card(1400.0, 900.0);
        assert_eq!((c.x, c.y, c.width, c.height), (150.0, 64.0, 1100.0, 804.0));
        let small = peek_card(400.0, 300.0);
        assert_eq!((small.width, small.height), (300.0, 204.0));
    }

    #[test]
    fn slid_pages_follow_the_edge_of_the_bar() {
        let plan = SlidePlan { from: Span { x: 6.0, width: 1000.0 }, width: 240.0, gap: 6.0, right: false, motion: SlideMotion::default() };
        let page = Rect::new(6.0, 6.0, 1000.0, 600.0);
        // Fully shown: the page is where it is laid out for the shown sidebar (narrower).
        let shown = Core::slid_rect(page, &plan, 1.0);
        let hidden = Core::slid_rect(page, &plan, 0.0);
        assert!(shown.x > hidden.x);
        assert_eq!(hidden.x, 6.0);
        assert_eq!(shown.width, hidden.width);
    }
}

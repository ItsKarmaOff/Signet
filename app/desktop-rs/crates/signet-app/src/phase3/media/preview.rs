//! Port of `browser/preview.ts`: hover previews of tabs in the sidebar or the top tab row.
//!
//! Thumbnails of the displayed tabs are captured while they are visible (a hidden CEF view produces no frames) and
//! kept in memory. `show_tab_preview` pushes `UiEvent::TabPreview` and places the `#preview` surface through
//! `Core::preview` (the layout turns it into `Desired::preview`).

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use signet_core::api::UiEvent;
use signet_core::ids::now_ms;
use signet_core::types::{PreviewFrom, Rect, TabPreviewPayload};
use signet_web::{ImageFormat, ScreenshotOptions};

use super::capture::{Metrics, METRICS_JS};
use super::{base64, Done};
use crate::browser::Core;

/// Thumbnails kept in memory (the oldest go first).
const MAX_THUMBS: usize = 60;
const THUMB_W: f64 = 560.0;
/// Delay before capturing a tab that was just displayed (time for it to paint).
const SHOWN_DELAY_MS: i64 = 900;
/// A page already captured less than 60 s ago is not recaptured.
const RECAPTURE_MS: i64 = 60_000;
/// After a load finishes, the time for the page to repaint.
const LOADED_DELAY: Duration = Duration::from_millis(600);
/// Card: width, thumbnail height, text height; transparent margin for the shadow.
const CARD_W: f64 = 280.0;
const THUMB_H: f64 = 158.0;
const TEXT_H: f64 = 64.0;
const MARGIN: f64 = 14.0;
const GAP: f64 = 6.0;
/// Animated exit: the surface stays in place for the duration of the fade.
const LEAVE: Duration = Duration::from_millis(120);
/// The preview surface is created after the event: the card is sent again once it exists.
const RESEND: Duration = Duration::from_millis(80);

pub(super) struct Thumb {
    tab: String,
    page: String,
    url: Result<String, String>,
}

struct Stored {
    url: String,
    at: i64,
    page: String,
}

#[derive(Default)]
pub(super) struct Previews {
    thumbs: HashMap<String, Stored>,
    shown_since: HashMap<String, Instant>,
    due: HashMap<String, Instant>,
    inflight: Option<String>,
    visible: bool,
    tab: Option<String>,
    from: Option<PreviewFrom>,
    resend_at: Option<Instant>,
    leave_at: Option<Instant>,
}

impl Previews {
    pub(super) fn forget(&mut self, tab: &str) {
        self.thumbs.remove(tab);
        self.shown_since.remove(tab);
        self.due.remove(tab);
    }
}

/// Position of the card (window coordinates) next to `r`, the row's rectangle. `below`: under the element (top tab
/// row); otherwise next to the sidebar, on the page side. No margin (hence no shadow) on the bar side.
pub(super) fn placement(r: &Rect, below: bool, sidebar_right: bool, has_thumb: bool, win: (f64, f64)) -> Rect {
    let ml = if !below && !sidebar_right { 0.0 } else { MARGIN };
    let mr = if !below && sidebar_right { 0.0 } else { MARGIN };
    let mt = if below { 0.0 } else { MARGIN };
    let width = CARD_W + ml + mr;
    let height = (if has_thumb { THUMB_H } else { 0.0 }) + TEXT_H + mt + MARGIN;
    let (x, y) = if below {
        (r.x - ml, r.y + r.height + GAP - mt)
    } else if sidebar_right {
        (r.x - width - GAP + mr, r.y - mt - 4.0)
    } else {
        (r.x + r.width + GAP - ml, r.y - mt - 4.0)
    };
    let x = x.min(win.0 - width).max(0.0).round();
    let y = y.min(win.1 - height).max(0.0).round();
    Rect::new(x, y, width, height)
}

/// Whether a displayed tab should be captured now.
pub(super) fn wants_capture(stored: Option<(&str, i64)>, page: &str, now: i64, shown_ms: i64, due: bool) -> bool {
    if due {
        return true;
    }
    match stored {
        Some((p, at)) if p == page => now - at >= RECAPTURE_MS,
        _ => shown_ms >= SHOWN_DELAY_MS,
    }
}

/// Only web pages, files and internal pages are worth a thumbnail.
pub(super) fn capturable(url: &str) -> bool {
    let l = url.to_ascii_lowercase();
    ["http:", "https:", "file:", "signet:"].iter().any(|p| l.starts_with(p))
}

/// Request for a thumbnail: the visible area scaled to `THUMB_W` pixels wide.
pub(super) fn thumb_options(m: &Metrics) -> Option<ScreenshotOptions> {
    if m.vw < 1.0 || m.vh < 1.0 {
        return None;
    }
    Some(ScreenshotOptions {
        clip: Some(signet_web::Bounds::new(m.x as f32, m.y as f32, m.vw as f32, m.vh as f32)),
        format: ImageFormat::Jpeg(72),
        scale: (THUMB_W / m.vw).min(1.0 / 0.1) as f32,
        capture_beyond_viewport: false,
    })
}

fn enabled(core: &Core) -> bool {
    core.state().settings.tab_preview != Some(false)
}

pub(super) fn on_loaded(core: &mut Core, tab: &str) {
    if enabled(core) && core.shown_tabs.iter().any(|t| t == tab) {
        core.phase3.media.preview.due.insert(tab.to_string(), Instant::now() + LOADED_DELAY);
    }
}

pub(super) fn store(core: &mut Core, t: Thumb) {
    let p = &mut core.phase3.media.preview;
    if p.inflight.as_deref() == Some(t.tab.as_str()) {
        p.inflight = None;
    }
    let Ok(url) = t.url else { return };
    p.thumbs.insert(t.tab.clone(), Stored { url, at: now_ms(), page: t.page });
    let tabs = &core.store.data.state.tabs;
    p.thumbs.retain(|id, _| tabs.contains_key(id));
    while p.thumbs.len() > MAX_THUMBS {
        let Some(oldest) = p.thumbs.iter().min_by_key(|(_, s)| s.at).map(|(k, _)| k.clone()) else { break };
        p.thumbs.remove(&oldest);
    }
    // The card on screen gets its image.
    if p.visible && p.tab.as_deref() == Some(t.tab.as_str()) {
        let (tab, from) = (t.tab, p.from.unwrap_or(PreviewFrom::Right));
        push(core, &tab, false, from);
    }
}

fn capture_thumb(core: &mut Core, tab: &str) {
    let Some(view) = core.views.view_of(tab) else { return };
    let page = core.state().tabs.get(tab).map(|t| t.url.clone()).unwrap_or_default();
    let outbox = core.phase3.media.outbox.clone();
    core.phase3.media.preview.inflight = Some(tab.to_string());
    core.phase3.media.preview.due.remove(tab);
    let id = tab.to_string();
    let params = serde_json::json!({ "expression": METRICS_JS, "returnByValue": true }).to_string();
    view.execute_devtools_method("Runtime.evaluate", &params, move |r| {
        let opts = r
            .and_then(|json| {
                let v: serde_json::Value = serde_json::from_str(&json).map_err(|e| e.to_string())?;
                let text = v["result"]["value"].as_str().ok_or("no page measures")?;
                serde_json::from_str::<Metrics>(text).map_err(|e| e.to_string())
            })
            .and_then(|m| thumb_options(&m).ok_or_else(|| "empty page".to_string()));
        match opts {
            Err(e) => outbox.push(Done::Thumb(Thumb { tab: id, page, url: Err(e) })),
            Ok(o) => view.capture_screenshot(&o, move |r| {
                let url = r.map(|jpg| format!("data:image/jpeg;base64,{}", base64(&jpg)));
                outbox.push(Done::Thumb(Thumb { tab: id, page, url }));
            }),
        }
    });
}

pub(super) fn tick(core: &mut Core) {
    let now = Instant::now();
    if !enabled(core) {
        if core.phase3.media.preview.visible || core.preview.is_some() {
            release(core);
        }
        return;
    }
    // Preview surface: late resend of the card, end of the exit animation.
    let p = &mut core.phase3.media.preview;
    let resend = p.resend_at.is_some_and(|t| now >= t) && p.visible;
    if resend {
        p.resend_at = None;
        if let (Some(tab), Some(from)) = (p.tab.clone(), p.from) {
            push(core, &tab, false, from);
        }
    }
    let p = &mut core.phase3.media.preview;
    if !p.visible && p.leave_at.is_some_and(|t| now >= t) {
        p.leave_at = None;
        if core.preview.take().is_some() {
            core.changed_view();
        }
    }
    // Thumbnails of the displayed tabs.
    let shown: HashSet<String> = core.shown_tabs.iter().cloned().collect();
    let p = &mut core.phase3.media.preview;
    p.shown_since.retain(|id, _| shown.contains(id));
    for id in &shown {
        p.shown_since.entry(id.clone()).or_insert(now);
    }
    if p.inflight.is_some() || !core.win.as_ref().is_some_and(|w| w.active) {
        return;
    }
    let ms = now_ms();
    for id in core.shown_tabs.clone() {
        let p = &core.phase3.media.preview;
        let Some(tab) = core.state().tabs.get(&id) else { continue };
        let Some(view) = core.views.view_of(&id) else { continue };
        if core.store.data.detached.contains(&id) || view.is_loading() || !capturable(&tab.url) {
            continue;
        }
        let since = p.shown_since.get(&id).map_or(0, |t| now.duration_since(*t).as_millis() as i64);
        let due = p.due.get(&id).is_some_and(|t| now >= *t);
        let stored = p.thumbs.get(&id).map(|s| (s.page.as_str(), s.at));
        if wants_capture(stored, &tab.url, ms, since, due) {
            capture_thumb(core, &id);
            break;
        }
    }
}

fn push(core: &Core, tab_id: &str, fresh: bool, from: PreviewFrom) {
    let Some(tab) = core.state().tabs.get(tab_id) else { return };
    let title = tab.custom_title.clone().filter(|t| !t.is_empty()).or_else(|| Some(tab.title.clone()).filter(|t| !t.is_empty())).unwrap_or_else(|| tab.url.clone());
    core.emit(UiEvent::TabPreview(TabPreviewPayload {
        tab_id: tab_id.to_string(),
        title,
        url: tab.url.clone(),
        favicon: tab.favicon.clone(),
        thumb: core.phase3.media.preview.thumbs.get(tab_id).map(|s| s.url.clone()),
        audible: tab.audible && !tab.muted,
        asleep: !tab.loaded,
        fresh,
        from,
    }));
}

/// Setting turned off: the surface and the thumbnails go.
fn release(core: &mut Core) {
    let p = &mut core.phase3.media.preview;
    p.visible = false;
    p.tab = None;
    p.thumbs.clear();
    p.due.clear();
    p.inflight = None;
    p.resend_at = None;
    p.leave_at = None;
    core.emit(UiEvent::TabPreviewHide);
    if core.preview.take().is_some() {
        core.changed_view();
    }
}

impl Core {
    /// Shows the tab's preview next to `rect` (window coordinates of the requesting surface). `below`: under the
    /// element (top tab row); otherwise next to the sidebar, on the page side.
    pub fn show_tab_preview(&mut self, tab_id: &str, rect: Rect, below: bool) {
        let tab_known = self.state().tabs.contains_key(tab_id);
        // A tab already on screen teaches nothing; detached tabs and an off setting show nothing.
        if !enabled(self) || !tab_known || self.store.data.detached.contains(tab_id) || self.shown_tabs.iter().any(|t| t == tab_id) {
            return self.hide_tab_preview();
        }
        let Some(win) = self.win.as_ref().map(|w| w.size) else { return };
        // The floating sidebar is its own window: its coordinates start at its own origin.
        let origin = if !self.sidebar_on() { self.desired.sidebar.map_or((0.0, 0.0), |s| (s.x, s.y)) } else { (0.0, 0.0) };
        let r = Rect::new(rect.x + origin.0, rect.y + origin.1, rect.width, rect.height);
        let right = self.sidebar_right();
        let has_thumb = self.phase3.media.preview.thumbs.contains_key(tab_id);
        let was_placed = self.preview.is_some();
        self.preview = Some(placement(&r, below, right, has_thumb, win));
        let from = if below {
            PreviewFrom::Below
        } else if right {
            PreviewFrom::Left
        } else {
            PreviewFrom::Right
        };
        let p = &mut self.phase3.media.preview;
        let fresh = !p.visible;
        p.visible = true;
        p.leave_at = None;
        p.tab = Some(tab_id.to_string());
        p.from = Some(from);
        p.resend_at = (!was_placed || fresh).then(|| Instant::now() + RESEND);
        self.changed_view();
        push(self, tab_id, fresh, from);
    }

    pub fn hide_tab_preview(&mut self) {
        let p = &mut self.phase3.media.preview;
        if !p.visible {
            return;
        }
        p.visible = false;
        p.resend_at = None;
        p.leave_at = Some(Instant::now() + LEAVE);
        self.emit(UiEvent::TabPreviewHide);
    }

    /// The preview surface was created: it missed the event sent before it existed.
    pub fn preview_ready(&mut self) {
        let p = &self.phase3.media.preview;
        if let (true, Some(tab), Some(from)) = (p.visible, p.tab.clone(), p.from) {
            push(self, &tab, true, from);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn card_goes_on_the_page_side_of_the_sidebar() {
        let row = Rect::new(8.0, 200.0, 240.0, 36.0);
        let left_bar = placement(&row, false, false, true, (1400.0, 900.0));
        assert_eq!(left_bar.x, 248.0 + 6.0);
        assert_eq!(left_bar.width, 280.0 + 14.0);
        assert_eq!(left_bar.y, 200.0 - 14.0 - 4.0);
        assert_eq!(left_bar.height, 158.0 + 64.0 + 14.0 + 14.0);
        let right_bar = placement(&Rect::new(1150.0, 200.0, 240.0, 36.0), false, true, false, (1400.0, 900.0));
        assert_eq!(right_bar.width, 280.0 + 14.0);
        assert_eq!(right_bar.x, 1150.0 - 294.0 - 6.0 + 0.0);
    }

    #[test]
    fn card_below_the_top_row() {
        let r = placement(&Rect::new(100.0, 10.0, 160.0, 30.0), true, false, false, (1400.0, 900.0));
        assert_eq!(r.x, 86.0);
        assert_eq!(r.y, 10.0 + 30.0 + 6.0);
        assert_eq!(r.height, 64.0 + 14.0);
    }

    #[test]
    fn card_stays_inside_the_window() {
        let r = placement(&Rect::new(1300.0, 880.0, 100.0, 30.0), false, false, true, (1400.0, 900.0));
        assert!(r.x + r.width <= 1400.0 && r.y + r.height <= 900.0 && r.x >= 0.0 && r.y >= 0.0);
    }

    #[test]
    fn capture_schedule() {
        // Never captured: after the paint delay.
        assert!(!wants_capture(None, "u", 1_000_000, 899, false));
        assert!(wants_capture(None, "u", 1_000_000, 900, false));
        // Same page captured recently: wait a minute; other page: capture again.
        assert!(!wants_capture(Some(("u", 1_000_000)), "u", 1_030_000, 5_000, false));
        assert!(wants_capture(Some(("u", 1_000_000)), "u", 1_060_000, 5_000, false));
        assert!(wants_capture(Some(("old", 1_000_000)), "u", 1_001_000, 1_000, false));
        // A finished load forces it.
        assert!(wants_capture(Some(("u", 1_000_000)), "u", 1_001_000, 5_000, true));
    }

    #[test]
    fn only_real_pages_are_captured() {
        assert!(capturable("https://a.example"));
        assert!(capturable("signet://note/x"));
        assert!(!capturable("about:blank"));
        assert!(!capturable("data:text/html,x"));
    }

    #[test]
    fn thumbnail_is_scaled_to_560_px() {
        let m = Metrics { x: 0.0, y: 40.0, vw: 1120.0, vh: 700.0, cw: 1120.0, ch: 3000.0, dpr: 2.0 };
        let o = thumb_options(&m).unwrap();
        assert_eq!(o.scale, 0.5);
        assert_eq!(o.clip.unwrap().y, 40.0);
        assert!(matches!(o.format, ImageFormat::Jpeg(72)));
        assert!(thumb_options(&Metrics { vw: 0.0, ..m }).is_none());
    }
}

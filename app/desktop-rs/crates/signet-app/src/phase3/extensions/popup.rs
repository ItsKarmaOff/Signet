//! The popup of a toolbar button: a small CEF view placed under the button, inside the main window (native views are
//! drawn above the GPUI content, which is what a popup needs).

use std::sync::atomic::Ordering;

use serde_json::Value;
use signet_core::api::UiEvent;
use signet_core::ids::now_ms;
use signet_core::types::Rect;
use signet_extensions::reload::RelayOutcome;
use signet_web::{Bounds, ProfileKind, ViewOptions, WebView};

use super::bridge::tab_number;
use crate::browser::Core;

pub const START_SIZE: (f64, f64) = (360.0, 420.0);
pub const MIN_SIZE: (f64, f64) = (25.0, 25.0);
pub const MAX_SIZE: (f64, f64) = (800.0, 600.0);
const GAP: f64 = 4.0;
const MARGIN: f64 = 8.0;

pub struct Popup {
    pub ext: String,
    pub view: WebView,
    pub anchor: Rect,
    pub size: (f64, f64),
}

/// Where the popup goes: under the button, right edges aligned, kept inside the window.
pub fn place(anchor: Rect, size: (f64, f64), window: (f64, f64)) -> Bounds {
    let (w, h) = (size.0.clamp(MIN_SIZE.0, MAX_SIZE.0), size.1.clamp(MIN_SIZE.1, MAX_SIZE.1));
    let w = w.min((window.0 - 2.0 * MARGIN).max(MIN_SIZE.0));
    let x = (anchor.x + anchor.width - w).clamp(MARGIN, (window.0 - w - MARGIN).max(MARGIN));
    let y = anchor.y + anchor.height + GAP;
    let h = h.min((window.1 - y - MARGIN).max(MIN_SIZE.1));
    Bounds::new(x as f32, y as f32, w as f32, h as f32)
}

/// A size reported by the popup's page: the popup only grows with its content, up to the maximum.
pub fn grow(current: (f64, f64), content: (f64, f64)) -> (f64, f64) {
    (content.0.clamp(current.0.min(MAX_SIZE.0), MAX_SIZE.0).max(MIN_SIZE.0), content.1.clamp(current.1.min(MAX_SIZE.1), MAX_SIZE.1).max(MIN_SIZE.1))
}

impl Core {
    fn ext_window_size(&self) -> (f64, f64) {
        self.win.as_ref().map(|w| w.size).unwrap_or((1200.0, 800.0))
    }

    /// Anchor used when the toolbar did not tell where the button is: its last click, else the top right corner.
    fn ext_fallback_anchor(&self, ext: &str) -> Rect {
        self.phase3.extensions.rt.as_ref().and_then(|r| r.anchors.get(ext).copied()).unwrap_or_else(|| {
            let (w, _) = self.ext_window_size();
            Rect::new(w - 90.0, 40.0, 28.0, 28.0)
        })
    }

    /// Self-test: click the button of `ext` as the toolbar would.
    pub fn selftest_click_extension(&mut self, ext: &str) {
        let anchor = self.ext_fallback_anchor(ext);
        self.ext_activate(ext, anchor);
    }

    /// Self-test: is the popup of `ext` open, and its view.
    pub fn selftest_popup(&self) -> Option<(String, signet_web::ViewId, bool)> {
        self.phase3.extensions.rt.as_ref().and_then(|r| r.popup.as_ref()).map(|p| (p.ext.clone(), p.view.id(), p.view.is_alive()))
    }

    /// Self-test: has `ext` a background view.
    pub fn selftest_background(&self, ext: &str) -> Option<signet_web::WebView> {
        self.phase3.extensions.rt.as_ref().and_then(|r| r.bg.get(ext).cloned())
    }

    /// The toolbar button of `ext` was clicked (or a shortcut reached it) at `anchor`.
    pub(super) fn ext_activate(&mut self, ext: &str, anchor: Rect) {
        let Some(shared) = self.ext_shared() else { return };
        if let Some(rt) = self.phase3.extensions.rt.as_mut() {
            rt.anchors.insert(ext.to_string(), anchor);
            if rt.popup.as_ref().is_some_and(|p| p.ext == ext) {
                self.ext_close_popup();
                return;
            }
        }
        let active = self.active_tab_id();
        let tab = active.as_deref().map(tab_number);
        let url = shared.host().popup_url(ext, tab);
        match url {
            Some(u) => self.ext_open_popup(ext, &u, anchor),
            None => {
                let snap = super::tabhost::snapshot(self, Default::default());
                let tab_json = snap.active.and_then(|n| signet_extensions::tabs::TabHost::tab(&snap, n)).map(|t| t.to_chrome_tab(true));
                let fx = shared.host().action_clicked(ext, tab_json);
                self.ext_apply(fx);
            }
        }
    }

    fn ext_open_popup(&mut self, ext: &str, url: &str, anchor: Rect) {
        self.ext_close_popup();
        let Some(parent) = self.views.parent else { return };
        let Some(shared) = self.ext_shared() else { return };
        let bounds = place(anchor, START_SIZE, self.ext_window_size());
        let opts = ViewOptions { profile: ProfileKind::persistent("default"), url: url.to_string(), bounds, visible: true, transparent_background: false };
        match WebView::create(parent, opts) {
            Ok(view) => {
                shared.popup_view.store(view.id().0, Ordering::Relaxed);
                view.focus();
                if let Some(rt) = self.phase3.extensions.rt.as_mut() {
                    rt.popup = Some(Popup { ext: ext.to_string(), view, anchor, size: START_SIZE });
                }
            }
            Err(e) => self.log(format!("extensions: cannot open the popup of {ext}: {e}")),
        }
    }

    pub(super) fn ext_close_popup(&mut self) {
        let Some(rt) = self.phase3.extensions.rt.as_mut() else { return };
        if let Some(p) = rt.popup.take() {
            rt.shared.popup_view.store(0, Ordering::Relaxed);
            p.view.close();
        }
    }

    /// `signet-crx-size` from the popup page.
    pub(super) fn ext_popup_size(&mut self, view: signet_web::ViewId, payload: &str) {
        let win = self.ext_window_size();
        let Some(p) = self.phase3.extensions.rt.as_mut().and_then(|r| r.popup.as_mut()).filter(|p| p.view.id() == view) else { return };
        let Ok(v) = serde_json::from_str::<Value>(payload) else { return };
        let content = (v["w"].as_f64().unwrap_or(0.0), v["h"].as_f64().unwrap_or(0.0));
        let size = grow(p.size, content);
        if size != p.size {
            p.size = size;
            p.view.set_bounds(place(p.anchor, size, win));
        }
    }

    /// Closes the popup when its tab went away or its view died.
    pub(super) fn ext_popup_tick(&mut self) {
        let alive = self.phase3.extensions.rt.as_ref().and_then(|r| r.popup.as_ref().map(|p| p.view.is_alive()));
        if alive == Some(false) {
            self.ext_close_popup();
        }
        let active = self.active_tab_id();
        let changed = self.phase3.extensions.rt.as_ref().is_some_and(|r| r.popup.is_some() && r.last_active != active && r.last_active.is_some());
        if changed {
            self.ext_close_popup();
        }
    }

    /// A shortcut wants the button's popup: the toolbar knows where the button is, so it is asked first.
    pub(super) fn ext_run_action(&mut self, ext: &str, popup_url: Option<String>) {
        let Some(url) = popup_url else { return };
        let bus = self.bus.clone();
        let outcome = match self.phase3.extensions.rt.as_mut() {
            Some(rt) => rt.relay.request(ext, now_ms() as u64, |token, id| {
                bus.emit(UiEvent::RunExtensionAction { token: token.to_string(), extension_id: id.to_string() });
                true
            }),
            None => return,
        };
        if matches!(outcome, RelayOutcome::Fallback) {
            let anchor = self.ext_fallback_anchor(ext);
            self.ext_open_popup(ext, &url, anchor);
        }
    }

    /// Shortcuts the toolbar did not take: open the popup under the last known place of the button.
    pub(super) fn ext_expire_relay(&mut self) {
        let due = match self.phase3.extensions.rt.as_mut() {
            Some(rt) => rt.relay.expire(now_ms() as u64),
            None => return,
        };
        let Some(shared) = self.ext_shared() else { return };
        for ext in due {
            let url = shared.host().popup_url(&ext, self.active_tab_id().as_deref().map(tab_number));
            if let Some(u) = url {
                let anchor = self.ext_fallback_anchor(&ext);
                self.ext_open_popup(&ext, &u, anchor);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_popup_sits_under_the_button_and_stays_inside_the_window() {
        let b = place(Rect::new(1000.0, 10.0, 28.0, 28.0), (360.0, 420.0), (1100.0, 800.0));
        assert_eq!((b.x, b.y, b.width, b.height), (668.0, 42.0, 360.0, 420.0));
        let left = place(Rect::new(10.0, 10.0, 28.0, 28.0), (360.0, 420.0), (1100.0, 800.0));
        assert_eq!(left.x, 8.0);
        let tall = place(Rect::new(10.0, 10.0, 28.0, 28.0), (360.0, 600.0), (1100.0, 300.0));
        assert!(f64::from(tall.y + tall.height) <= 300.0);
    }

    #[test]
    fn the_popup_grows_with_its_content_but_never_shrinks() {
        assert_eq!(grow((360.0, 420.0), (500.0, 700.0)), (500.0, 600.0));
        assert_eq!(grow((360.0, 420.0), (100.0, 100.0)), (360.0, 420.0));
        assert_eq!(grow((360.0, 420.0), (2000.0, 10.0)), (800.0, 420.0));
    }
}

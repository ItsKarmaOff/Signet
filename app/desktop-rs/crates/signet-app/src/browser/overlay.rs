//! Port of `browser/overlay.ts` (command bar, popovers, zoom bubble) and the Peek hooks of `peek.ts`.

use signet_core::api::UiEvent;
use signet_core::types::{OverlayPayload, PaletteMode, PopoverRequest, Rect};

use super::Core;
use crate::windows::{Cmd, Surface};

impl Core {
    pub fn emit_overlay(&self) {
        if let Some(p) = &self.ov.payload {
            self.emit(UiEvent::OverlayShow(Box::new(p.clone())));
        }
    }

    fn show_overlay(&mut self, payload: OverlayPayload) {
        if self.ov.peek_tab.is_some() {
            self.drop_peek();
        }
        self.show_overlay_raw(payload);
    }

    /// Shows a payload without touching the Peek (used by Peek itself).
    pub(super) fn show_overlay_raw(&mut self, payload: OverlayPayload) {
        self.ov.payload = Some(payload);
        self.ov.leaving = false;
        self.changed_view();
        self.emit_overlay();
        // An overlay window that is already open takes the keyboard for the new payload.
        self.send(Cmd::MakeKey(Surface::Overlay));
    }

    pub fn open_palette(&mut self, mode: PaletteMode, anchor: Option<Rect>) {
        if self.win.is_none() {
            return;
        }
        let initial = match mode {
            PaletteMode::EditUrl => self.active_tab_id().and_then(|id| self.state().tabs.get(&id)).map(|t| t.url.clone()).unwrap_or_default(),
            // Capture harness only: text typed into the bar (SIGNET_PALETTE_QUERY).
            _ => crate::dev::capture_spec().and_then(|_| std::env::var("SIGNET_PALETTE_QUERY").ok()).unwrap_or_default(),
        };
        let payload = OverlayPayload::Palette { palette_mode: mode, initial, hue: self.hue(), keywords: self.search.keyword_list(), anchor };
        self.show_overlay(payload);
    }

    /// The overlay view subscribed: send it the pending payload.
    pub fn overlay_ready(&mut self) {
        self.ov.ready = true;
        self.emit_overlay();
    }

    pub fn close_overlay(&mut self, instant: bool) {
        if self.ov.payload.take().is_none() && !self.ov.leaving {
            return;
        }
        self.ov.leaving = false;
        self.drop_peek();
        self.dragging = false;
        self.emit(UiEvent::OverlayHide { instant });
        self.request_focus();
        self.changed_view();
    }

    pub fn open_popover(&mut self, popover: PopoverRequest, anchor: Rect) {
        let hue = self.hue();
        self.show_overlay(OverlayPayload::Popover { popover, anchor, hue });
    }

    /// Popover opened without a known anchor (a menu command): under the top left of the web card.
    pub fn open_popover_kind(&mut self, popover: PopoverRequest) {
        // Electron's DEFAULT_ANCHOR: under the space switcher of the horizontal layout.
        self.open_popover(popover, Rect::new(78.0, 6.0, 140.0, 28.0));
    }

    pub fn open_zoom_bubble(&mut self, tab_id: &str) {
        let Some(pane) = self.panes.iter().find(|p| p.tab_id == tab_id).cloned() else { return };
        let percent = self.state().tabs.get(tab_id).and_then(|t| t.zoom).unwrap_or(100);
        let default_percent = self.state().settings.default_zoom.unwrap_or(100);
        let compact = self.state().settings.compact_mode;
        self.show_overlay(OverlayPayload::Zoom { tab_id: tab_id.to_string(), percent, default_percent, card: pane.card, compact });
    }

    /// Site panel (no scrim) anchored under the toolbar's padlock.
    pub fn open_site_panel(&mut self, anchor: Rect) {
        if self.win.is_none() {
            return;
        }
        let Some(mut info) = self.site_info() else { return };
        info.blocked = self.adblock_blocked(&info.tab_id);
        self.hover.sidebar = false;
        self.hover.toolbar = false;
        self.show_overlay(OverlayPayload::Site { info, anchor });
        self.request_focus();
    }

}

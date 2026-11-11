//! Port of `tabs/DropZones.tsx`: drop zones drawn over the page while a tab is dragged from the sidebar. Edges split
//! the focused pane, the center replaces the displayed tab.
//!
//! The overlay window (`OverlayPayload::Dropzones { area, split_card }`) creates a [`DropZones`] with the payload. The
//! drag payload is [`TabDrag`]; the view calls `drop_on_content` on release.

use gpui::{Context, DragMoveEvent, InteractiveElement, IntoElement, ParentElement, Render, Styled, Window, div, px};
use signet_core::types::{DragSource, DropZone, Rect};

use super::row::TabDrag;
use crate::app_state::api;
use crate::theme::ThemeExt;

const EDGE: f32 = 0.25;
/// Hysteresis (px): the current zone is kept until the pointer has crossed its boundary by this margin.
const HYST: f32 = 12.0;

/// Target zone: the nearest edge if the pointer is in its 25% band, otherwise the center. `prev`: displayed zone; it
/// is left only once the boundary is crossed by `HYST` px (no flicker).
pub fn zone_at(x: f32, y: f32, w: f32, h: f32, edges: bool, prev: Option<DropZone>) -> DropZone {
    if !edges || w <= 0.0 || h <= 0.0 {
        return DropZone::Center;
    }
    // Distance (px) to each edge, and band thickness.
    let near = [(DropZone::Left, x, w * EDGE), (DropZone::Right, w - x, w * EDGE), (DropZone::Top, y, h * EDGE), (DropZone::Bottom, h - y, h * EDGE)];
    let in_band = |z: DropZone, margin: f32| near.iter().any(|(n, d, band)| *n == z && *d < band + margin);
    // Current zone kept until we have clearly left its band (or clearly entered another's).
    if let Some(p) = prev
        && p != DropZone::Center
        && in_band(p, HYST)
    {
        return p;
    }
    let (zone, d, band) = near.iter().copied().reduce(|a, b| if b.1 < a.1 { b } else { a }).unwrap_or((DropZone::Center, 0.0, 0.0));
    if prev == Some(DropZone::Center) {
        return if d < band - HYST { zone } else { DropZone::Center };
    }
    if d < band { zone } else { DropZone::Center }
}

/// Where the tab will land: the half of the focused pane on the targeted side; the whole content at the center.
pub fn preview_rect(zone: DropZone, whole: Rect, pane: Rect) -> Rect {
    if zone == DropZone::Center {
        return whole;
    }
    let hw = (pane.width / 2.0).round();
    let hh = (pane.height / 2.0).round();
    match zone {
        DropZone::Left => Rect { width: hw, ..pane },
        DropZone::Right => Rect { x: pane.x + pane.width - hw, width: hw, ..pane },
        DropZone::Top => Rect { height: hh, ..pane },
        _ => Rect { y: pane.y + pane.height - hh, height: hh, ..pane },
    }
}

pub struct DropZones {
    area: Rect,
    split_card: Option<Rect>,
    zone: Option<DropZone>,
    favorite: bool,
}

impl DropZones {
    /// `area`: the web card (window coordinates) the overlay covers; `split_card`: the focused pane's card when edges
    /// are allowed (fewer than 4 panes, a tab that is not a favorite).
    pub fn new(area: Rect, split_card: Option<Rect>) -> Self {
        DropZones { area, split_card, zone: None, favorite: false }
    }

    pub fn set_payload(&mut self, area: Rect, split_card: Option<Rect>, cx: &mut Context<Self>) {
        self.area = area;
        self.split_card = split_card;
        self.zone = None;
        cx.notify();
    }

    fn edges(&self) -> bool {
        !self.favorite && self.split_card.is_some()
    }
}

impl Render for DropZones {
    fn render(&mut self, _w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.signet().clone();
        let area = self.area;
        let whole = Rect::new(0.0, 0.0, area.width, area.height);
        let pane = match (self.edges(), self.split_card) {
            (true, Some(c)) => Rect::new(c.x - area.x, c.y - area.y, c.width, c.height),
            _ => whole,
        };
        let shown = self.zone.map(|z| preview_rect(z, whole, pane));
        let card_r = crate::screens::main_window::card_radius(&crate::app_state::snapshot(cx));
        div()
            .id("dropzones")
            .absolute()
            .size_full()
            .on_drag_move::<TabDrag>(cx.listener(|this, e: &DragMoveEvent<TabDrag>, _w, cx| {
                let b = e.bounds;
                if !b.contains(&e.event.position) {
                    if this.zone.take().is_some() {
                        cx.notify();
                    }
                    return;
                }
                this.favorite = e.drag(cx).favorite;
                let (x, y) = (f32::from(e.event.position.x - b.origin.x), f32::from(e.event.position.y - b.origin.y));
                let z = zone_at(x, y, f32::from(b.size.width), f32::from(b.size.height), this.edges(), this.zone);
                if this.zone != Some(z) {
                    this.zone = Some(z);
                    cx.notify();
                }
            }))
            .on_drop::<TabDrag>(cx.listener(|this, d: &TabDrag, _w, cx| {
                let zone = this.zone.take().unwrap_or(DropZone::Center);
                match &d.src {
                    DragSource::Tab { tab_id } => api(cx).drop_on_content(tab_id, zone),
                    _ => api(cx).end_tab_drag(),
                }
                cx.notify();
            }))
            .children(shown.map(|r| {
                div()
                    .absolute()
                    .left(px(r.x as f32))
                    .top(px(r.y as f32))
                    .w(px(r.width as f32))
                    .h(px(r.height as f32))
                    .rounded(card_r)
                    .bg(t.accent_a(0.18))
                    .border_2()
                    .border_color(t.accent)
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn center_without_edges_or_empty_box() {
        assert_eq!(zone_at(1.0, 1.0, 400.0, 300.0, false, None), DropZone::Center);
        assert_eq!(zone_at(1.0, 1.0, 0.0, 0.0, true, None), DropZone::Center);
    }

    #[test]
    fn nearest_edge_inside_its_quarter_band() {
        assert_eq!(zone_at(10.0, 150.0, 400.0, 300.0, true, None), DropZone::Left);
        assert_eq!(zone_at(390.0, 150.0, 400.0, 300.0, true, None), DropZone::Right);
        assert_eq!(zone_at(200.0, 10.0, 400.0, 300.0, true, None), DropZone::Top);
        assert_eq!(zone_at(200.0, 290.0, 400.0, 300.0, true, None), DropZone::Bottom);
        assert_eq!(zone_at(200.0, 150.0, 400.0, 300.0, true, None), DropZone::Center);
    }

    #[test]
    fn hysteresis_keeps_the_current_zone_a_little_longer() {
        // Band is 100 px wide on the left; 105 is outside it but inside the 12 px margin.
        assert_eq!(zone_at(105.0, 150.0, 400.0, 300.0, true, None), DropZone::Center);
        assert_eq!(zone_at(105.0, 150.0, 400.0, 300.0, true, Some(DropZone::Left)), DropZone::Left);
        assert_eq!(zone_at(115.0, 150.0, 400.0, 300.0, true, Some(DropZone::Left)), DropZone::Center);
        // From the center, an edge must be entered by the margin.
        assert_eq!(zone_at(95.0, 150.0, 400.0, 300.0, true, Some(DropZone::Center)), DropZone::Center);
        assert_eq!(zone_at(80.0, 150.0, 400.0, 300.0, true, Some(DropZone::Center)), DropZone::Left);
    }

    #[test]
    fn preview_rect_is_half_the_pane_on_the_targeted_side() {
        let whole = Rect::new(0.0, 0.0, 800.0, 600.0);
        let pane = Rect::new(100.0, 50.0, 400.0, 300.0);
        assert_eq!(preview_rect(DropZone::Center, whole, pane), whole);
        assert_eq!(preview_rect(DropZone::Left, whole, pane), Rect::new(100.0, 50.0, 200.0, 300.0));
        assert_eq!(preview_rect(DropZone::Right, whole, pane), Rect::new(300.0, 50.0, 200.0, 300.0));
        assert_eq!(preview_rect(DropZone::Top, whole, pane), Rect::new(100.0, 50.0, 400.0, 150.0));
        assert_eq!(preview_rect(DropZone::Bottom, whole, pane), Rect::new(100.0, 200.0, 400.0, 150.0));
    }
}

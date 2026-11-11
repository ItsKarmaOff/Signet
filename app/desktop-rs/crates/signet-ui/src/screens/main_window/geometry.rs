//! Pure layout helpers of the main window (tests included).

use signet_core::slide::{SidebarSlide, slide_rect};
use signet_core::types::{Divider, Pane, PaneDevtools, PaneResponsive, Rect, Snapshot};

/// Height of the tab bar (`horizontal` layout), `TAB_BAR` of `browser/layout.ts`.
pub const TAB_BAR: f32 = 40.0;
/// Gap around the cards, `--gap`.
pub const GAP: f32 = 6.0;
/// Extra height of the bar background below the bar, so the web view's rounded top corners rest on it.
pub const BAR_OVERFLOW: f32 = 12.0;

/// Radius of the web cards, concentric with the window corners (`Snapshot.window_corner`).
pub fn card_radius(s: &Snapshot) -> gpui::Pixels {
    gpui::px(signet_core::floating::web_card_radius(s.window_corner) as f32)
}

/// Position of the macOS traffic lights (`Layout.trafficLights`): left offset and top offset in px.
pub fn traffic_lights(horizontal: bool, sidebar_right: bool, compact: bool) -> (f32, f32) {
    use crate::screens::toolbar::PANE_BAR;
    if horizontal {
        return (16.0, ((TAB_BAR - 14.0) / 2.0).round());
    }
    if sidebar_right && !compact {
        return (16.0, (GAP + (PANE_BAR - 14.0) / 2.0).round());
    }
    (16.0, 16.0)
}

/// The page chrome (cards, dividers) stretched into the space the bar leaves on this step of the slide.
pub fn stretched(s: &Snapshot, slide: Option<&SidebarSlide>) -> (Vec<Pane>, Vec<Divider>) {
    let Some(slide) = slide else { return (s.panes.clone(), s.dividers.clone()) };
    let r = |x: Rect| slide_rect(x, slide, false);
    let panes = s
        .panes
        .iter()
        .map(|p| Pane {
            rect: r(p.rect),
            card: r(p.card),
            devtools: p.devtools.as_ref().map(|d| PaneDevtools { card: r(d.card), rect: r(d.rect), divider: r(d.divider), ..d.clone() }),
            responsive: p.responsive.as_ref().map(|d| PaneResponsive { bar: r(d.bar), area: r(d.area), ..d.clone() }),
            ..p.clone()
        })
        .collect();
    let dividers = s.dividers.iter().map(|d| Divider { rect: r(d.rect), ..d.clone() }).collect();
    (panes, dividers)
}

/// Index of the top-left pane (by card position).
pub fn top_left(panes: &[Pane]) -> Option<usize> {
    (0..panes.len()).min_by(|&a, &b| {
        let (x, y) = (&panes[a].card, &panes[b].card);
        x.y.total_cmp(&y.y).then(x.x.total_cmp(&y.x))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use signet_core::slide::{SlidePlan, Span, slide_at};

    fn pane(id: &str, x: f64, y: f64) -> Pane {
        Pane { tab_id: id.into(), rect: Rect::new(x, y, 100.0, 100.0), card: Rect::new(x, y, 100.0, 100.0), focused: false, devtools: None, responsive: None }
    }

    #[test]
    fn lights() {
        assert_eq!(traffic_lights(true, false, false), (16.0, 13.0));
        assert_eq!(traffic_lights(false, true, false), (16.0, 19.0));
        assert_eq!(traffic_lights(false, true, true), (16.0, 16.0));
        assert_eq!(traffic_lights(false, false, false), (16.0, 16.0));
    }

    #[test]
    fn top_left_pane() {
        let panes = vec![pane("a", 300.0, 6.0), pane("b", 6.0, 6.0), pane("c", 6.0, 200.0)];
        assert_eq!(top_left(&panes), Some(1));
        assert_eq!(top_left(&[]), None);
    }

    #[test]
    fn no_slide_keeps_geometry() {
        let mut s = Snapshot::default();
        s.panes = vec![pane("a", 200.0, 6.0)];
        let (panes, _) = stretched(&s, None);
        assert_eq!(panes[0].card, s.panes[0].card);
    }

    #[test]
    fn slide_stretches_cards() {
        let mut s = Snapshot::default();
        s.panes = vec![pane("a", 6.0, 6.0)];
        let plan = SlidePlan { from: Span { x: 6.0, width: 1000.0 }, width: 200.0, gap: 6.0, right: false, motion: Default::default() };
        let slide = slide_at(&plan, 0.0);
        let (panes, _) = stretched(&s, Some(&slide));
        assert_eq!(panes[0].card.x, 6.0);
        let slide = slide_at(&plan, 1.0);
        let (panes, _) = stretched(&s, Some(&slide));
        assert!(panes[0].card.x > 6.0);
    }
}

//! Port of `windows/SitePanel.tsx` + `styles/site-panel.css` + the `.site-*` rules of `sidebar-panels.css`: the site panel
//! (click on the padlock), a card under the padlock with the site's permissions, blocked requests, Boost and "clear data".
//!
//! It is the content of the overlay's `site` mode (`OverlayPayload::Site { info, anchor }`): the overlay root builds
//! `SitePanel::new(window, cx, info, anchor)` and gives it the whole overlay window. The view paints a full-size,
//! transparent layer (a press outside the card calls `close_overlay(false)`) and the card at `left = clamp(anchor.x - 6)`,
//! `top = anchor.y + anchor.height + 8`, 320 px wide, with an arrow pointing at the padlock.

use super::common::favicon;
use crate::app_state::{api, tr, tr_args};
use crate::icon::IconName;
use crate::motion::{reduced_motion, surface_enter};
use crate::theme::{SignetTheme, ThemeExt, radius, space, weight};
use crate::widgets::{Segmented, Toggle};
use gpui::{
    AnyElement, AppContext, Context, ElementId, Entity, InteractiveElement, IntoElement, MouseButton, ParentElement, PathBuilder, Render, StatefulInteractiveElement, Styled, Window, canvas, div,
    point, prelude::FluentBuilder, px,
};
use gpui_component::Icon;
use signet_core::types::{PermissionState, Rect, SiteBoostFlags, SiteInfo, SitePermissionRow};
use std::time::Duration;

pub const WIDTH: f32 = 320.0;
const STATES: [PermissionState; 3] = [PermissionState::Ask, PermissionState::Allow, PermissionState::Deny];

/// Icon of a permission id.
pub fn permission_icon(id: &str) -> IconName {
    match id {
        "media" => IconName::Camera,
        "geolocation" => IconName::MapPin,
        "notifications" => IconName::Bell,
        "clipboard-read" => IconName::ClipboardPaste,
        _ => IconName::Info,
    }
}

/// Translated label of a permission: the catalog's for the four known ones, the one the app sent otherwise.
pub fn permission_label(row: &SitePermissionRow) -> String {
    match row.id.as_str() {
        "media" | "geolocation" | "notifications" | "clipboard-read" => tr(&format!("site.perm.{}", row.id)),
        _ => row.label.clone(),
    }
}

/// Position of the card and of its arrow for a padlock `anchor` in a window `window_width` wide:
/// `(left, top, arrow offset from the card's left edge)`.
pub fn placement(anchor: &Rect, window_width: f64) -> (f64, f64, f64) {
    let cx = anchor.x + anchor.width / 2.0;
    let left = (anchor.x - 6.0).min(window_width - f64::from(WIDTH) - 8.0).max(8.0);
    let top = anchor.y + anchor.height + 8.0;
    let arrow = (cx - left).clamp(14.0, f64::from(WIDTH) - 14.0);
    (left, top, arrow)
}

/// Host shown in the header: the origin without its scheme.
pub fn host_of_origin(origin: &str) -> String {
    match origin.split_once("://") {
        Some((scheme, rest)) if !scheme.is_empty() && scheme.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') => rest.to_string(),
        _ => origin.to_string(),
    }
}

/// `.site-btn-s`: small 24 px button on `surface-2`.
pub fn site_btn(id: impl Into<ElementId>, t: &SignetTheme) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .gap(px(5.0))
        .h(px(24.0))
        .px(space::S3)
        .rounded(radius::CONTROL)
        .bg(t.surface_2)
        .text_size(t.type_scale.sub)
        .cursor_pointer()
        .hover(|s| s.opacity(0.9))
        .active(|s| s.opacity(0.8))
}

pub struct SitePanel {
    info: SiteInfo,
    anchor: Rect,
    perms: Vec<SitePermissionRow>,
    boost: SiteBoostFlags,
    confirm: bool,
    cleared: bool,
}

impl SitePanel {
    pub fn new(_window: &mut Window, cx: &mut gpui::App, info: SiteInfo, anchor: Rect) -> Entity<Self> {
        cx.new(|_: &mut Context<Self>| Self { perms: info.permissions.clone(), boost: info.boost.clone(), info, anchor, confirm: false, cleared: false })
    }

    fn set_perm(&mut self, id: &str, state: PermissionState, cx: &mut Context<Self>) {
        if let Some(p) = self.perms.iter_mut().find(|p| p.id == id) {
            p.state = state;
        }
        api(cx).set_site_permission(&self.info.origin, id, state);
        cx.notify();
    }

    fn clear_data(&mut self, cx: &mut Context<Self>) {
        api(cx).clear_site_data(&self.info.origin);
        self.confirm = false;
        self.cleared = true;
        cx.notify();
        cx.spawn(async move |_this, cx| {
            cx.background_executor().timer(Duration::from_millis(700)).await;
            cx.update(|cx| api(cx).close_overlay(false));
        })
        .detach();
    }
}

impl Render for SitePanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.signet().clone();
        let reduced = reduced_motion(cx);
        let me = cx.entity();
        let (left, top, arrow) = placement(&self.anchor, f64::from(f32::from(window.viewport_size().width)));
        let host = host_of_origin(&self.info.origin);
        let secure = self.info.secure;

        let head = div()
            .relative()
            .flex()
            .items_center()
            .gap(space::S3)
            .px(space::S2)
            .pt(space::S2)
            .pb(space::S3)
            .child(favicon(self.info.favicon.as_deref(), 20.0, false))
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(1.0))
                    .child(div().truncate().font_weight(weight::STRONG).child(host))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(space::S1)
                            .text_size(t.type_scale.sub)
                            .text_color(if secure { t.text_2 } else { t.danger })
                            .child(Icon::from(if secure { IconName::Lock } else { IconName::Info }).size(px(11.0)))
                            .child(if secure { tr("site.secure") } else { tr("site.insecure") }),
                    ),
            );

        let section = div()
            .px(space::S2)
            .pt(space::S2)
            .pb(space::S1)
            .border_t(px(0.5))
            .border_color(t.border)
            .text_size(t.type_scale.cap)
            .font_weight(weight::STRONG)
            .text_color(t.text_2)
            .child(tr("site.permissions").to_uppercase());

        let perms = div().flex().flex_col().gap(space::S3).px(space::S2).pt(space::S1).pb(space::S3).children(self.perms.iter().enumerate().map(|(ix, p)| {
            let me = me.clone();
            let id = p.id.clone();
            let selected = STATES.iter().position(|s| *s == p.state).unwrap_or(0);
            let label = permission_label(p);
            div()
                .flex()
                .flex_col()
                .gap(space::S1)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(space::S2)
                        .child(div().flex_none().w(space::S5).child(Icon::from(permission_icon(&p.id)).size(px(15.0)).text_color(t.text_2)))
                        .child(div().flex_1().min_w_0().child(label)),
                )
                .child(
                    Segmented::new(ElementId::from(("perm", ix)))
                        .item(tr("site.state.ask"))
                        .tinted_item(tr("site.state.allow"), t.accent)
                        .tinted_item(tr("site.state.deny"), t.danger)
                        .selected(selected)
                        .on_change(move |i, _, cx| {
                            let state = STATES[i];
                            me.update(cx, |this, cx| this.set_perm(&id, state, cx));
                        }),
                )
        }));

        let row = |ix: usize| div().flex().items_center().gap(space::S2).min_h(px(40.0)).px(space::S2).py(space::S1).border_t(px(0.5)).border_color(t.border).id(ElementId::from(("site-row", ix)));
        let blocked = row(0)
            .child(Icon::from(IconName::ShieldOff).size(px(15.0)).text_color(t.text_2))
            .child(div().flex_1().min_w_0().child(if self.info.blocked == 0 { tr("site.blockedNone") } else { tr_args("site.blocked", &[("count", &self.info.blocked.to_string())]) }));

        let exists = self.boost.exists;
        let enabled = self.boost.enabled;
        let (me_boost, site_boost) = (me.clone(), self.info.site.clone());
        let site_open = self.info.site.clone();
        let boost = row(1)
            .child(Icon::from(IconName::Rocket).size(px(15.0)).text_color(t.text_2))
            .child(div().flex_1().min_w_0().child(if exists { tr("site.boost") } else { tr("site.noBoost") }))
            .when(exists, |r| {
                r.child(Toggle::new("site-boost", enabled).on_change(move |on, _, cx| {
                    me_boost.update(cx, |this, cx| {
                        this.boost = SiteBoostFlags { exists: true, enabled: on };
                        cx.notify();
                    });
                    api(cx).save_boost(&site_boost, None, Some(on));
                }))
            })
            .child(site_btn("site-boost-edit", &t).on_click(move |_, _, cx| {
                api(cx).open_boost(&site_open);
                api(cx).close_overlay(false);
            }).child(if exists { tr("site.edit") } else { tr("site.create") }));

        let foot: AnyElement = if self.confirm {
            let (me_cancel, me_clear) = (me.clone(), me.clone());
            div()
                .flex()
                .items_center()
                .gap(space::S2)
                .pt(space::S2)
                .pb(px(2.0))
                .px(space::S1)
                .border_t(px(0.5))
                .border_color(t.border)
                .child(div().flex_1().pl(space::S1).text_size(t.type_scale.sub).text_color(t.text_2).child(tr("site.confirmClear")))
                .child(site_btn("site-cancel", &t).on_click(move |_, _, cx| me_cancel.update(cx, |this, cx| {
                    this.confirm = false;
                    cx.notify();
                })).child(tr("common.cancel")))
                .child(site_btn("site-clear-ok", &t).bg(t.danger).text_color(t.on_accent).on_click(move |_, _, cx| me_clear.update(cx, |this, cx| this.clear_data(cx))).child(tr("site.clear")))
                .into_any_element()
        } else {
            let me_ask = me.clone();
            let cleared = self.cleared;
            let hover = t.surface_2;
            div()
                .flex()
                .items_center()
                .pt(space::S2)
                .pb(px(2.0))
                .px(space::S1)
                .border_t(px(0.5))
                .border_color(t.border)
                .child(
                    div()
                        .id("site-clear")
                        .flex_1()
                        .flex()
                        .items_center()
                        .gap(space::S2)
                        .h(space::S7)
                        .px(space::S2)
                        .rounded(radius::CONTROL)
                        .text_color(if cleared { t.text_2 } else { t.danger })
                        .when(!cleared, |d| d.cursor_pointer().hover(move |s| s.bg(hover)).on_click(move |_, _, cx| me_ask.update(cx, |this, cx| {
                            this.confirm = true;
                            cx.notify();
                        })))
                        .child(Icon::from(IconName::Trash2).size(px(14.0)))
                        .child(if cleared { tr("site.cleared") } else { tr("site.clearData") }),
                )
                .into_any_element()
        };

        // Arrow on the card's top edge, pointing at the padlock.
        let glass = t.glass_view;
        let ring = t.border;
        let arrow_el = canvas(
            |_, _, _| (),
            move |b, _, window, _| {
                let (x, y) = (f32::from(b.origin.x), f32::from(b.origin.y));
                let mut ring_path = PathBuilder::fill();
                ring_path.move_to(point(px(x - 0.5), px(y + 5.5)));
                ring_path.line_to(point(px(x + 5.0), px(y - 0.5)));
                ring_path.line_to(point(px(x + 10.5), px(y + 5.5)));
                ring_path.close();
                if let Ok(p) = ring_path.build() {
                    window.paint_path(p, ring);
                }
                let mut fill = PathBuilder::fill();
                fill.move_to(point(px(x), px(y + 6.0)));
                fill.line_to(point(px(x + 5.0), px(y + 1.0)));
                fill.line_to(point(px(x + 10.0), px(y + 6.0)));
                fill.close();
                if let Ok(p) = fill.build() {
                    window.paint_path(p, glass);
                }
            },
        )
        .absolute()
        .top(px(-5.0))
        .left(px(arrow as f32 - 5.0))
        .w(px(10.0))
        .h(px(6.0));

        let card = div()
            .id("site-panel")
            .flex()
            .flex_col()
            .rounded(radius::POP)
            .bg(t.glass_view)
            .border(px(0.5))
            .border_color(t.border)
            .shadow(t.shadow_pop.clone())
            .text_color(t.text)
            .text_size(t.type_scale.base)
            .absolute()
            .left(px(left as f32))
            .top(px(top as f32))
            .w(px(WIDTH))
            .p(px(6.0))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(arrow_el)
            .child(head)
            .child(section)
            .child(perms)
            .child(blocked)
            .child(boost)
            .child(foot);

        div().id("site-layer").absolute().inset_0().on_mouse_down(MouseButton::Left, |_, _, cx| api(cx).close_overlay(false)).child(surface_enter(card, "site-enter", false, reduced))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn card_stays_inside_the_window_and_the_arrow_inside_the_card() {
        let anchor = Rect::new(100.0, 10.0, 24.0, 24.0);
        let (left, top, arrow) = placement(&anchor, 1000.0);
        assert_eq!((left, top), (94.0, 42.0));
        assert_eq!(arrow, 18.0);
        // anchor near the right edge: the card is pushed left, the arrow follows the padlock
        let (left, _, arrow) = placement(&Rect::new(980.0, 10.0, 24.0, 24.0), 1000.0);
        assert_eq!(left, 1000.0 - 320.0 - 8.0);
        assert_eq!(arrow, 306.0);
        // anchor at the far left: minimum margin of 8
        let (left, _, arrow) = placement(&Rect::new(0.0, 0.0, 10.0, 10.0), 1000.0);
        assert_eq!(left, 8.0);
        assert_eq!(arrow, 14.0);
    }

    #[test]
    fn host_drops_the_scheme() {
        assert_eq!(host_of_origin("https://example.com"), "example.com");
        assert_eq!(host_of_origin("http://localhost:3000"), "localhost:3000");
        assert_eq!(host_of_origin("example.com"), "example.com");
    }

    #[test]
    fn unknown_permissions_keep_the_app_label() {
        let row = SitePermissionRow { id: "midi".into(), label: "MIDI devices".into(), state: PermissionState::Ask };
        assert_eq!(permission_label(&row), "MIDI devices");
        assert_eq!(permission_icon("midi"), IconName::Info);
        assert_eq!(permission_icon("media"), IconName::Camera);
    }
}

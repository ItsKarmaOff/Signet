//! Site panel (port of `windows/SitePanel.tsx`): card under the padlock with the site's permissions, blocked items and Boost.

use super::common::{arrow_up, favicon, hairline, host_of, lucide, panel, small_btn, viewport};
use crate::app_state::{api, tr, tr_args};
use crate::theme::{ThemeExt, space, weight};
use crate::widgets::{Segmented, Toggle};
use gpui::{
    AppContext, Context, Entity, InteractiveElement, IntoElement, ParentElement, Render, StatefulInteractiveElement, Styled, Task, Window, div, prelude::FluentBuilder, px,
};
use signet_core::types::{PermissionState, Rect, SiteInfo};
use std::time::Duration;

const WIDTH: f32 = 320.0;

pub struct SitePanel {
    info: SiteInfo,
    anchor: Rect,
    confirm: bool,
    cleared: bool,
    _close: Option<Task<()>>,
}

impl SitePanel {
    pub fn new(info: SiteInfo, anchor: Rect, cx: &mut gpui::App) -> Entity<Self> {
        cx.new(|_| Self { info, anchor, confirm: false, cleared: false, _close: None })
    }
}

fn perm_icon(id: &str) -> &'static str {
    match id {
        "media" => "camera",
        "geolocation" => "map-pin",
        "notifications" => "bell",
        "clipboard-read" => "clipboard-paste",
        _ => "info",
    }
}

impl Render for SitePanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.signet().clone();
        let (w, _) = viewport(window);
        let me = cx.entity();
        let cx_mid = (self.anchor.x + self.anchor.width / 2.0) as f32;
        let left = (self.anchor.x as f32 - 6.0).clamp(8.0, (w - WIDTH - 8.0).max(8.0));
        let top = (self.anchor.y + self.anchor.height) as f32 + 8.0;
        let arrow = (cx_mid - left).clamp(14.0, WIDTH - 14.0);
        let info = self.info.clone();
        let host = host_of(&info.origin);
        let states = [PermissionState::Ask, PermissionState::Allow, PermissionState::Deny];
        let section = |text: String| div().px(space::S2).pt(space::S2).pb(space::S1).border_t_1().border_color(t.border).text_size(t.type_scale.cap).font_weight(weight::STRONG).text_color(t.text_2).child(text.to_uppercase());

        let perms = info.permissions.iter().enumerate().map(|(i, p)| {
            let label = match p.id.as_str() {
                "media" | "geolocation" | "notifications" | "clipboard-read" => tr(&format!("site.perm.{}", p.id)),
                _ => p.label.clone(),
            };
            let (me, origin, pid) = (me.clone(), info.origin.clone(), p.id.clone());
            let selected = states.iter().position(|s| *s == p.state).unwrap_or(0);
            div()
                .id(("perm", i))
                .flex()
                .flex_col()
                .gap(space::S1)
                .child(div().flex().items_center().gap(space::S2).child(lucide(perm_icon(&p.id)).size(px(15.0)).text_color(t.text_2)).child(div().flex_1().min_w_0().child(label)))
                .child(
                    Segmented::new(("perm-seg", i))
                        .item(tr("site.state.ask"))
                        .tinted_item(tr("site.state.allow"), t.accent)
                        .tinted_item(tr("site.state.deny"), t.danger)
                        .selected(selected)
                        .on_change(move |ix, _, cx| {
                            let state = states[ix.min(2)];
                            me.update(cx, |this, cx| {
                                if let Some(row) = this.info.permissions.iter_mut().find(|r| r.id == pid) {
                                    row.state = state;
                                }
                                cx.notify();
                            });
                            api(cx).set_site_permission(&origin, &pid, state);
                        }),
                )
        });

        let boost = info.boost.clone();
        let (site, site2, me_boost) = (info.site.clone(), info.site.clone(), me.clone());
        let origin = info.origin.clone();
        let (me_confirm, me_cancel, me_clear) = (me.clone(), me.clone(), me.clone());

        panel(cx)
            .id("site-panel")
            .absolute()
            .left(px(left))
            .top(px(top))
            .w(px(WIDTH))
            .line_height(gpui::relative(1.38))
            .p(px(6.0))
            .child(arrow_up(cx, arrow))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(space::S3)
                    .pt(space::S2)
                    .px(space::S2)
                    .pb(space::S3)
                    .child(favicon(info.favicon.as_deref(), 20.0))
                    .child(
                        div()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(1.0))
                            .child(div().overflow_hidden().text_ellipsis().whitespace_nowrap().font_weight(weight::STRONG).child(host))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(space::S1)
                                    .text_size(t.type_scale.sub)
                                    .text_color(if info.secure { t.text_2 } else { t.danger })
                                    .child(lucide(if info.secure { "lock" } else { "info" }).size(px(11.0)))
                                    .child(if info.secure { tr("site.secure") } else { tr("site.insecure") }),
                            ),
                    ),
            )
            .child(section(tr("site.permissions")))
            .child(div().flex().flex_col().gap(space::S3).px(space::S2).pt(space::S1).pb(space::S3).children(perms))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(space::S2)
                    .min_h(px(40.0))
                    .px(space::S2)
                    .py(space::S1)
                    .border_t_1()
                    .border_color(t.border)
                    .child(lucide("shield-off").size(px(15.0)).text_color(t.text_2))
                    .child(div().flex_1().min_w_0().child(if info.blocked == 0 { tr("site.blockedNone") } else { tr_args("site.blocked", &[("count", &info.blocked.to_string())]) })),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(space::S2)
                    .min_h(px(40.0))
                    .px(space::S2)
                    .py(space::S1)
                    .border_t_1()
                    .border_color(t.border)
                    .child(lucide("rocket").size(px(15.0)).text_color(t.text_2))
                    .child(div().flex_1().min_w_0().child(if boost.exists { tr("site.boost") } else { tr("site.noBoost") }))
                    .when(boost.exists, |d| {
                        d.child(Toggle::new("site-boost", boost.enabled).on_change(move |on, _, cx| {
                            me_boost.update(cx, |this, cx| {
                                this.info.boost.enabled = on;
                                cx.notify();
                            });
                            api(cx).save_boost(&site, None, Some(on));
                        }))
                    })
                    .child(small_btn("site-boost-edit", cx).child(if boost.exists { tr("site.edit") } else { tr("site.create") }).on_click(move |_, _, cx| {
                        api(cx).open_boost(&site2);
                        api(cx).close_overlay(false);
                    })),
            )
            .child(hairline(cx))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(space::S2)
                    .pt(space::S2)
                    .px(space::S1)
                    .pb(space::HALF)
                    .child(if self.confirm {
                        div()
                            .flex()
                            .flex_1()
                            .items_center()
                            .gap(space::S2)
                            .child(div().flex_1().pl(space::S1).text_size(t.type_scale.sub).text_color(t.text_2).child(tr("site.confirmClear")))
                            .child(small_btn("site-clear-cancel", cx).child(tr("common.cancel")).on_click(move |_, _, cx| {
                                me_cancel.update(cx, |this, cx| {
                                    this.confirm = false;
                                    cx.notify();
                                })
                            }))
                            .child(small_btn("site-clear-ok", cx).bg(t.danger).text_color(t.on_accent).child(tr("site.clear")).on_click(move |_, window, cx| {
                                api(cx).clear_site_data(&origin);
                                me_clear.update(cx, |this, cx| {
                                    this.confirm = false;
                                    this.cleared = true;
                                    let ex = cx.background_executor().clone();
                                    this._close = Some(cx.spawn_in(window, async move |_, cx| {
                                        ex.timer(Duration::from_millis(700)).await;
                                        cx.update(|_, cx| api(cx).close_overlay(false)).ok();
                                    }));
                                    cx.notify();
                                })
                            }))
                            .into_any_element()
                    } else {
                        let cleared = self.cleared;
                        let hover = t.surface_2;
                        div()
                            .id("site-clear")
                            .flex()
                            .flex_1()
                            .items_center()
                            .gap(space::S2)
                            .h(space::S7)
                            .px(space::S2)
                            .rounded(crate::theme::radius::CONTROL)
                            .text_color(if cleared { t.text_2 } else { t.danger })
                            .when(!cleared, |d| d.cursor_pointer().hover(move |s| s.bg(hover)).on_click(move |_, _, cx| me_confirm.update(cx, |this, cx| {
                                this.confirm = true;
                                cx.notify();
                            })))
                            .child(lucide("trash-2").size(px(14.0)))
                            .child(if cleared { tr("site.cleared") } else { tr("site.clearData") })
                            .into_any_element()
                    }),
            )
    }
}

//! Port of `windows/WebApp.tsx` + `styles/webapp.css`: route `#webapp`, the title bar of an installed app.
//!
//! Window: frameless. This bar is 38 px high at the top; the native web view takes the rest:
//! `x = 0, y = 38, width = W, height = H - 38` (the traffic lights sit in the bar's 76 px left padding).

use super::common::{BarButton, BarColors, domain_of, drag_area, is_https, pill};
use super::data_url::image_from_data_url;
use crate::app_state::{api, tr, tr_args, ui_state};
use crate::icon::IconName;
use crate::theme::{ThemeExt, space, weight};
use gpui::{AppContext, Context, Entity, IntoElement, ObjectFit, ParentElement, Render, Styled, StyledImage, Window, div, img, px};
use gpui_component::Icon;
use signet_core::api::UiEvent;
use signet_core::types::WebAppState;
use std::time::Duration;

pub const WEBAPP_BAR_HEIGHT: f32 = 38.0;

pub struct WebApp {
    st: Option<WebAppState>,
    copied: bool,
    copy_gen: u64,
}

impl WebApp {
    pub fn new(_window: &mut Window, cx: &mut gpui::App) -> Entity<Self> {
        cx.new(|cx: &mut Context<Self>| {
            cx.subscribe(&ui_state(cx), |this, _, ev: &UiEvent, cx| {
                if let UiEvent::WebAppState(s) = ev {
                    this.st = Some(s.clone());
                    cx.notify();
                }
            })
            .detach();
            api(cx).web_app_ready();
            Self { st: None, copied: false, copy_gen: 0 }
        })
    }

    fn copy_url(&mut self, cx: &mut Context<Self>) {
        api(cx).web_app_copy_url();
        self.copied = true;
        self.copy_gen += 1;
        let gen_ = self.copy_gen;
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(1400)).await;
            this.update(cx, |this, cx| {
                if this.copy_gen == gen_ {
                    this.copied = false;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }
}

impl Render for WebApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.signet().clone();
        let st = self.st.clone().unwrap_or_default();
        let away = self.st.is_some() && !st.in_scope;
        // The bar is `--surface` by default and takes the site color when the page announces one.
        let colors = BarColors::from_color(st.theme_color.as_deref(), &t);
        let a = api(cx);
        let (a_back, a_reload, a_home, a_open) = (a.clone(), a.clone(), a.clone(), a.clone());
        let me = cx.entity();

        let title: gpui::AnyElement = if away {
            let secure = is_https(&st.url);
            div()
                .flex()
                .items_center()
                .gap(space::S2)
                .min_w_0()
                .child(
                    pill(&t, colors.hover, if secure { colors.fg } else { t.danger })
                        .gap(px(4.0))
                        .children(secure.then(|| Icon::from(IconName::Lock).size(px(11.0))))
                        .child(domain_of(&st.url)),
                )
                .children((!st.title.is_empty()).then(|| div().min_w_0().truncate().text_color(colors.fg2).child(st.title.clone())))
                .into_any_element()
        } else {
            let icon = (!st.icon.is_empty()).then(|| image_from_data_url(&st.icon)).flatten();
            div()
                .flex()
                .items_center()
                .gap(space::S2)
                .min_w_0()
                .children(icon.map(|i| img(i).size(px(16.0)).flex_none().object_fit(ObjectFit::Contain)))
                .child(div().min_w_0().truncate().child(if !st.title.is_empty() { st.title.clone() } else { st.name.clone() }))
                .into_any_element()
        };

        let bar = div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .h(px(WEBAPP_BAR_HEIGHT))
            .flex()
            .items_center()
            .gap(space::S2)
            .pl(px(76.0))
            .pr(space::S2)
            .bg(colors.bg)
            .text_color(colors.fg)
            .text_size(t.type_scale.base);
        let nav = div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(2.0))
            .child(BarButton::new("back", IconName::ArrowLeft, tr("common.back")).size(26.0).disabled(!st.can_go_back).render(colors, move |_, _| a_back.go_back(None)))
            .child(
                BarButton::new("reload", if st.loading { IconName::X } else { IconName::RotateCw }, if st.loading { tr("toolbar.stop") } else { tr("toolbar.reload") })
                    .size(26.0)
                    .icon_size(if st.loading { 16.0 } else { 15.0 })
                    .render(colors, move |_, _| a_reload.reload(None)),
            );
        let mut actions = div().flex_none().flex().items_center().justify_end().gap(px(2.0)).min_w(px(56.0));
        if away {
            actions = actions.child(
                BarButton::new("home", IconName::House, tr_args("webapp.home", &[("name", &st.name)])).size(26.0).icon_size(15.0).render(colors, move |_, _| a_home.web_app_home()),
            );
        }
        let copy_icon = if self.copied { IconName::Check } else { IconName::Copy };
        actions = actions
            .child(BarButton::new("copy", copy_icon, tr("little.copyUrl")).size(26.0).icon_size(15.0).render(colors, move |_, cx| me.update(cx, |this, cx| this.copy_url(cx))))
            .child(BarButton::new("open", IconName::ExternalLink, tr("little.openInMain")).size(26.0).icon_size(15.0).render(colors, move |_, _| a_open.web_app_open_in_main()));

        div().relative().size_full().bg(t.surface).child(drag_area(bar.child(nav).child(div().flex_1().min_w_0().flex().items_center().justify_center().font_weight(weight::LABEL).child(title)).child(actions)))
    }
}

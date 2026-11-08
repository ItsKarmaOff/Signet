//! Port of `windows/Little.tsx` + `styles/little.css`: route `#little`, the bar of Little Signet.
//!
//! Window: frameless, 6 px margin (`PAD`). The native web view goes at
//! `x = 6, y = 46 (PAD + 40), width = W - 12, height = H - 52` (`main/windows/little.ts`); this view leaves it empty.

use super::common::{BarButton, BarColors, domain_of, drag_area, favicon, pane_card};
use crate::app_state::{api, tr, ui_state};
use crate::icon::IconName;
use crate::theme::{ThemeExt, radius, space, weight};
use gpui::{AppContext, Context, Entity, IntoElement, ParentElement, Render, Styled, Window, div, px};
use signet_core::api::UiEvent;
use signet_core::types::LittleState;
use std::time::Duration;

const PAD: f32 = 6.0;

pub struct Little {
    st: Option<LittleState>,
    copied: bool,
    copy_gen: u64,
}

impl Little {
    pub fn new(_window: &mut Window, cx: &mut gpui::App) -> Entity<Self> {
        cx.new(|cx: &mut Context<Self>| {
            cx.subscribe(&ui_state(cx), |this, _, ev: &UiEvent, cx| {
                if let UiEvent::LittleState(s) = ev {
                    this.st = Some(s.clone());
                    cx.notify();
                }
            })
            .detach();
            // Subscribed: main sends the current state back.
            api(cx).little_ready();
            Self { st: None, copied: false, copy_gen: 0 }
        })
    }

    fn copy_url(&mut self, cx: &mut Context<Self>) {
        api(cx).little_copy_url();
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

impl Render for Little {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.signet().clone();
        let st = self.st.clone().unwrap_or_default();
        let known = self.st.is_some();
        let colors = BarColors::from_color(st.theme_color.as_deref(), &t);
        let domain = if st.url.is_empty() { String::new() } else { domain_of(&st.url) };
        let title = if !st.title.is_empty() { st.title.clone() } else { domain.clone() };
        let me = cx.entity();
        let a = api(cx);
        let (a_back, a_fwd, a_open) = (a.clone(), a.clone(), a.clone());
        let me_copy = me.clone();

        let bar = div()
            .absolute()
            .top(px(PAD))
            .left(px(PAD))
            .right(px(PAD))
            .h(px(40.0))
            .flex()
            .items_center()
            .gap(space::HALF)
            .pl(px(70.0))
            .pr(space::S3)
            .text_color(colors.fg)
            .text_size(t.type_scale.base);
        let bar = drag_area(bar)
            .child(BarButton::new("back", IconName::ArrowLeft, tr("common.back")).icon_size(17.0).disabled(!known || !st.can_go_back).render(colors, move |_, _| a_back.go_back(None)))
            .child(BarButton::new("forward", IconName::ArrowRight, tr("common.forward")).icon_size(17.0).disabled(!known || !st.can_go_forward).render(colors, move |_, _| a_fwd.go_forward(None)))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(space::S2)
                    .px(space::S3)
                    .child(favicon(st.favicon.as_deref(), 16.0, st.loading))
                    .child(div().min_w_0().truncate().font_weight(weight::LABEL).child(title))
                    .when_domain(&domain, colors),
            )
            .child(
                BarButton::new("copy", if self.copied { IconName::Check } else { IconName::Copy }, tr("little.copyUrl"))
                    .render(colors, move |_, cx| me_copy.update(cx, |this, cx| this.copy_url(cx))),
            )
            .child(BarButton::new("open", IconName::ExternalLink, tr("little.openInMain")).render(colors, move |_, _| a_open.little_open_in_main()));

        div()
            .relative()
            .size_full()
            .bg(t.bg)
            .text_color(t.text)
            // top strip: drag zone above the card
            .child(drag_area(div().absolute().top_0().left_0().right_0().h(px(PAD))))
            .child(
                pane_card(&t)
                    .top(px(PAD))
                    .left(px(PAD))
                    .right(px(PAD))
                    .bottom(px(PAD))
                    // bar background: overflows 8 px below the bar so the web view's rounded corners rest on it
                    .child(div().absolute().top_0().left_0().right_0().h(px(52.0)).rounded_t(radius::CARD).bg(colors.bg)),
            )
            .child(bar)
    }
}

trait WhenDomain {
    fn when_domain(self, domain: &str, colors: BarColors) -> Self;
}

impl WhenDomain for gpui::Div {
    fn when_domain(self, domain: &str, colors: BarColors) -> Self {
        if domain.is_empty() { self } else { self.child(div().flex_none().text_color(colors.fg2).child(domain.to_string())) }
    }
}

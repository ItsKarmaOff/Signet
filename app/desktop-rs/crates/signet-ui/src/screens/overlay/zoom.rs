//! Zoom bubble (port of `overlay/ZoomBubble.tsx`): top right of the page; hides itself after 2 s without interaction.

use super::common::{lucide, panel, small_btn};
use crate::app_state::api;
use crate::theme::{ThemeExt, space, weight};
use gpui::{AppContext, Context, Entity, InteractiveElement, IntoElement, ParentElement, Render, StatefulInteractiveElement, Styled, Task, Window, div, prelude::FluentBuilder, px};
use signet_core::pagesettings::{ZOOM_PERCENTS, format_zoom_percent};
use std::time::Duration;

/// Time without interaction before the bubble goes away.
pub const ZOOM_BUBBLE_MS: u64 = 2000;
const STEP: i32 = 1;
/// Shadow margin around the bubble (the app's overlay window is the bubble plus this margin on each side).
pub const MARGIN: f32 = 24.0;
pub const WIDTH: f32 = 248.0;

pub struct ZoomBubble {
    tab_id: String,
    percent: u32,
    default_percent: u32,
    hovering: bool,
    timer: Option<Task<()>>,
}

impl ZoomBubble {
    pub fn new(tab_id: String, percent: u32, default_percent: u32, window: &mut Window, cx: &mut gpui::App) -> Entity<Self> {
        cx.new(|cx| {
            let mut this = Self { tab_id, percent, default_percent, hovering: false, timer: None };
            this.arm(window, cx);
            this
        })
    }

    /// A new payload (zoom changed) restarts the wait.
    pub fn set(&mut self, tab_id: String, percent: u32, default_percent: u32, window: &mut Window, cx: &mut Context<Self>) {
        self.tab_id = tab_id;
        self.percent = percent;
        self.default_percent = default_percent;
        self.arm(window, cx);
        cx.notify();
    }

    fn arm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ex = cx.background_executor().clone();
        self.timer = Some(cx.spawn_in(window, async move |this, cx| {
            ex.timer(Duration::from_millis(ZOOM_BUBBLE_MS)).await;
            this.update(cx, |this, cx| {
                this.timer = None;
                if !this.hovering {
                    api(cx).close_overlay(false);
                }
            })
            .ok();
        }));
    }
}

impl Render for ZoomBubble {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.signet().clone();
        let locale = signet_core::i18n::current_locale().id();
        let value = format_zoom_percent(self.percent, locale);
        let (min, max) = (ZOOM_PERCENTS[0], ZOOM_PERCENTS[ZOOM_PERCENTS.len() - 1]);
        let (a, b, c) = (self.tab_id.clone(), self.tab_id.clone(), self.tab_id.clone());
        let dim = |on: bool| move |d: gpui::Stateful<gpui::Div>| if on { d.opacity(0.45) } else { d.cursor_pointer() };
        let (out_off, in_off, reset_off) = (self.percent <= min, self.percent >= max, self.percent == self.default_percent);
        let step = |id: &'static str, icon: &str, off: bool, tab: String, delta: i32| {
            dim(off)(small_btn(id, cx).w(px(28.0)).px_0().child(lucide(icon).size(px(14.0)))).when(!off, |d| d.on_click(move |_, _, cx| api(cx).zoom_page(&tab, delta)))
        };
        panel(cx)
            .id("zoom-bubble")
            .absolute()
            .left(px(MARGIN))
            .top(px(MARGIN))
            .w(px(WIDTH))
            .h(px(52.0))
            .flex_row()
            .items_center()
            .gap(space::S2)
            .px(space::S3)
            .on_hover(cx.listener(|this, over: &bool, window, cx| {
                this.hovering = *over;
                if !*over {
                    this.arm(window, cx);
                }
            }))
            .child(step("zoom-out", "minus", out_off, a, -STEP))
            .child(div().flex_1().min_w_0().text_center().font_weight(weight::STRONG).text_color(t.text).child(value))
            .child(step("zoom-in", "plus", in_off, b, STEP))
            .child(dim(reset_off)(small_btn("zoom-reset", cx).child(crate::app_state::tr("zoom.reset"))).when(!reset_off, |d| d.on_click(move |_, _, cx| api(cx).zoom_page(&c, 0))))
    }
}

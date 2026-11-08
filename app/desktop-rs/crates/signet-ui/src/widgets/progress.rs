use crate::theme::{ThemeExt, duration};
use gpui::{Animation, AnimationExt, App, ElementId, IntoElement, ParentElement, RenderOnce, Styled, Window, div, percentage, px};
use gpui_component::{Icon, IconName as KitIcon};
use gpui::Transformation;
use std::time::Duration;

/// Thin progress bar (3 px, accent). `value` 0..1; `None` is an indeterminate sweep.
#[derive(IntoElement)]
pub struct ProgressBar {
    id: ElementId,
    value: Option<f32>,
    height: f32,
}

impl ProgressBar {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self { id: id.into(), value: None, height: 3.0 }
    }
    pub fn value(mut self, v: f32) -> Self {
        self.value = Some(v.clamp(0.0, 1.0));
        self
    }
    pub fn height(mut self, h: f32) -> Self {
        self.height = h;
        self
    }
}

impl RenderOnce for ProgressBar {
    fn render(self, _w: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.signet().clone();
        let track = div().relative().w_full().h(px(self.height)).rounded_full().bg(t.surface_2).overflow_hidden();
        match self.value {
            Some(v) => track.child(div().h_full().w(gpui::relative(v)).rounded_full().bg(t.accent)),
            None => track.child(
                div().absolute().top_0().h_full().w(gpui::relative(0.35)).rounded_full().bg(t.accent).with_animation(
                    self.id,
                    Animation::new(Duration::from_millis(1200)).repeat().with_easing(crate::motion::ease_out),
                    |bar, d| bar.left(gpui::relative(-0.35 + 1.35 * d)),
                ),
            ),
        }
    }
}

/// Rotating spinner (loader icon, accent or given color).
#[derive(IntoElement)]
pub struct Spinner {
    id: ElementId,
    size: f32,
    color: Option<gpui::Hsla>,
}

impl Spinner {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self { id: id.into(), size: 16.0, color: None }
    }
    pub fn size(mut self, s: f32) -> Self {
        self.size = s;
        self
    }
    pub fn color(mut self, c: gpui::Hsla) -> Self {
        self.color = Some(c);
        self
    }
}

impl RenderOnce for Spinner {
    fn render(self, _w: &mut Window, cx: &mut App) -> impl IntoElement {
        let color = self.color.unwrap_or(cx.signet().text_2);
        let _ = duration::NORMAL;
        Icon::new(KitIcon::LoaderCircle).size(px(self.size)).text_color(color).with_animation(
            self.id,
            Animation::new(Duration::from_millis(900)).repeat(),
            |icon, d| icon.transform(Transformation::rotate(percentage(d))),
        )
    }
}

use crate::theme::{ThemeExt, space};
use gpui::{App, AppContext, Context, Entity, IntoElement, ParentElement, RenderOnce, SharedString, Styled, Window, div};
use gpui_component::slider::{Slider, SliderState};

/// Creates a slider state (`min..=max`, `step`, initial `value`). Subscribe to `SliderEvent` for changes.
pub fn slider_state(cx: &mut App, min: f32, max: f32, step: f32, value: f32) -> Entity<SliderState> {
    cx.new(|_cx: &mut Context<SliderState>| SliderState::new().min(min).max(max).step(step).default_value(value))
}

/// Labelled slider row: label on the left, the track, and an optional value text on the right.
#[derive(IntoElement)]
pub struct SliderField {
    state: Entity<SliderState>,
    label: Option<SharedString>,
    value_text: Option<SharedString>,
}

impl SliderField {
    pub fn new(state: &Entity<SliderState>) -> Self {
        Self { state: state.clone(), label: None, value_text: None }
    }
    pub fn label(mut self, l: impl Into<SharedString>) -> Self {
        self.label = Some(l.into());
        self
    }
    pub fn value_text(mut self, v: impl Into<SharedString>) -> Self {
        self.value_text = Some(v.into());
        self
    }
}

impl RenderOnce for SliderField {
    fn render(self, _w: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.signet();
        div()
            .flex()
            .items_center()
            .gap(space::S3)
            .text_size(t.type_scale.base)
            .children(self.label.map(|l| div().flex_none().text_color(t.text_2).child(l)))
            .child(div().flex_1().min_w_0().child(Slider::new(&self.state)))
            .children(self.value_text.map(|v| div().flex_none().text_color(t.text_2).child(v)))
    }
}

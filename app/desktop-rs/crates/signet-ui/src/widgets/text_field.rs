use crate::theme::{ThemeExt, radius, space};
use gpui::{App, AppContext, Context, Entity, IntoElement, ParentElement, RenderOnce, SharedString, Styled, Window, div, prelude::FluentBuilder, px};
use gpui_component::{
    Sizable,
    input::{Input, InputState},
};

/// Creates the state of a single-line field. Subscribe to `InputEvent` on it for changes / Enter.
pub fn text_field_state(window: &mut Window, cx: &mut App, placeholder: impl Into<SharedString>, value: impl Into<SharedString>) -> Entity<InputState> {
    let placeholder = placeholder.into();
    let value = value.into();
    cx.new(|cx: &mut Context<InputState>| InputState::new(window, cx).placeholder(placeholder).default_value(value))
}

/// Single-line text field (`.st-input`: 28 px, control radius, `--ctl` fill, hairline ring). Wraps gpui-component's Input.
#[derive(IntoElement)]
pub struct TextField {
    state: Entity<InputState>,
    height: f32,
    cleanable: bool,
    prefix: Option<gpui::AnyElement>,
    disabled: bool,
}

impl TextField {
    pub fn new(state: &Entity<InputState>) -> Self {
        Self { state: state.clone(), height: 28.0, cleanable: false, prefix: None, disabled: false }
    }
    pub fn height(mut self, h: f32) -> Self {
        self.height = h;
        self
    }
    /// Shows a clear button when not empty.
    pub fn cleanable(mut self, c: bool) -> Self {
        self.cleanable = c;
        self
    }
    pub fn prefix(mut self, el: impl IntoElement) -> Self {
        self.prefix = Some(el.into_any_element());
        self
    }
    pub fn disabled(mut self, d: bool) -> Self {
        self.disabled = d;
        self
    }
}

impl RenderOnce for TextField {
    fn render(self, _w: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.signet();
        div().w_full().min_w_0().child(
            Input::new(&self.state)
                .appearance(false)
                .small()
                .h(px(self.height))
                .px(space::S3)
                .rounded(radius::CONTROL)
                .bg(t.ctl)
                .border_1()
                .border_color(t.ctl_ring)
                .text_color(t.text)
                .cleanable(self.cleanable)
                .disabled(self.disabled)
                .when_some(self.prefix, |i, p| i.prefix(p)),
        )
    }
}

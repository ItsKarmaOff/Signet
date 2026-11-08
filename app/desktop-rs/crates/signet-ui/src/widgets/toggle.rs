use crate::motion::{reduced_motion, spring_curve};
use crate::theme::{ThemeExt, duration};
use gpui::{Animation, AnimationExt, App, ElementId, InteractiveElement, IntoElement, ParentElement, RenderOnce, StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder, px};
use std::rc::Rc;

/// On / off switch, 36x20 with a 16 px knob that springs from one side to the other (`.switch`).
#[derive(IntoElement)]
pub struct Toggle {
    id: ElementId,
    checked: bool,
    disabled: bool,
    on_change: Option<Rc<dyn Fn(bool, &mut Window, &mut App)>>,
}

impl Toggle {
    pub fn new(id: impl Into<ElementId>, checked: bool) -> Self {
        Self { id: id.into(), checked, disabled: false, on_change: None }
    }
    pub fn disabled(mut self, d: bool) -> Self {
        self.disabled = d;
        self
    }
    /// Called with the new value when clicked.
    pub fn on_change(mut self, f: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }
}

impl RenderOnce for Toggle {
    fn render(self, _w: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.signet().clone();
        let checked = self.checked;
        let reduced = reduced_motion(cx);
        let on_change = self.on_change.clone();
        let disabled = self.disabled;
        let knob = div().absolute().top(px(2.0)).size(px(16.0)).rounded_full().bg(t.on_accent).shadow(t.shadow_knob.clone());
        let id = self.id.clone();
        // The animation is keyed by the value, so it restarts (toward the new side) on each change.
        let knob = knob.with_animation(
            ElementId::from((id, if checked { "on" } else { "off" })),
            Animation::new(if reduced { std::time::Duration::from_millis(1) } else { duration::SLOW }).with_easing(spring_curve),
            move |k, d| {
                let p = if checked { d } else { 1.0 - d };
                k.left(px(2.0 + 16.0 * p))
            },
        );
        div()
            .id(self.id)
            .relative()
            .flex_none()
            .w(px(36.0))
            .h(px(20.0))
            .rounded_full()
            .bg(if checked { t.accent } else { t.border })
            .when(disabled, |d| d.opacity(0.45))
            .when(!disabled, |d| d.cursor_pointer())
            .when_some(on_change.filter(|_| !disabled), |d, f| d.on_click(move |_, w, cx| f(!checked, w, cx)))
            .child(knob)
    }
}

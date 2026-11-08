use crate::icon::IconName;
use crate::theme::{ThemeExt, radius};
use gpui::{App, ClickEvent, ElementId, Hsla, InteractiveElement, IntoElement, ParentElement, RenderOnce, SharedString, StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder, px};
use gpui_component::{Icon, tooltip::Tooltip};
use std::rc::Rc;

/// 28 px (default) or 24 px square icon button (`button.icon` / `button.icon.sm`).
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum IconButtonSize {
    Sm,
    #[default]
    Md,
}

/// Square icon button with hover and pressed states and an optional tooltip.
///
/// ```ignore
/// IconButton::new("close", IconName::X).tooltip(t("common.close")).on_click(|_, _, cx| { /* ... */ })
/// ```
#[derive(IntoElement)]
pub struct IconButton {
    id: ElementId,
    icon: IconName,
    size: IconButtonSize,
    icon_size: Option<f32>,
    dims: Option<(f32, f32)>,
    tooltip: Option<SharedString>,
    active: bool,
    disabled: bool,
    danger: bool,
    color: Option<Hsla>,
    on_click: Option<Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>>,
}

impl IconButton {
    pub fn new(id: impl Into<ElementId>, icon: IconName) -> Self {
        Self { id: id.into(), icon, size: IconButtonSize::Md, icon_size: None, dims: None, tooltip: None, active: false, disabled: false, danger: false, color: None, on_click: None }
    }
    pub fn size(mut self, size: IconButtonSize) -> Self {
        self.size = size;
        self
    }
    /// Explicit width and height in px (default: a square of the size).
    pub fn dims(mut self, w: f32, h: f32) -> Self {
        self.dims = Some((w, h));
        self
    }
    pub fn small(self) -> Self {
        self.size(IconButtonSize::Sm)
    }
    /// Icon size in px (default 16, or 14 for the small button).
    pub fn icon_size(mut self, px: f32) -> Self {
        self.icon_size = Some(px);
        self
    }
    pub fn tooltip(mut self, text: impl Into<SharedString>) -> Self {
        self.tooltip = Some(text.into());
        self
    }
    /// Toggled-on look (accent icon), e.g. a muted tab.
    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
    pub fn danger(mut self, danger: bool) -> Self {
        self.danger = danger;
        self
    }
    /// Overrides the resting icon color.
    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }
    pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(f));
        self
    }
}

impl RenderOnce for IconButton {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.signet().clone();
        let (w, default_icon) = match self.size {
            IconButtonSize::Sm => (24.0, 14.0),
            IconButtonSize::Md => (28.0, 16.0),
        };
        let rest = if self.active { t.accent } else if self.danger { t.danger } else { self.color.unwrap_or(t.text_2) };
        let hover_fg = if self.active { t.accent } else if self.danger { t.danger } else { t.text };
        let tip = self.tooltip.clone();
        let on_click = self.on_click.clone();
        let disabled = self.disabled;
        div()
            .id(self.id)
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .w(px(self.dims.map_or(w, |d| d.0)))
            .h(px(self.dims.map_or(w, |d| d.1)))
            .rounded(radius::CONTROL)
            .text_color(rest)
            .when(disabled, |d| d.opacity(0.35))
            .when(!disabled, |d| {
                d.cursor_pointer()
                    .hover(move |s| s.bg(t.surface_2).text_color(hover_fg))
                    .active(move |s| s.bg(t.ctl_ring))
            })
            .when(!disabled, |d| d.on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation()))
            .when_some(tip, |d, text| d.tooltip(move |window, cx| Tooltip::new(text.clone()).build(window, cx)))
            .when_some(on_click.filter(|_| !disabled), |d, f| d.on_click(move |e, w, cx| f(e, w, cx)))
            .child(Icon::from(self.icon).size(px(self.icon_size.unwrap_or(default_icon))))
    }
}

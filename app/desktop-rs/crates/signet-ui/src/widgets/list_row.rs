use crate::theme::{ThemeExt, radius, space};
use gpui::{AnyElement, App, ClickEvent, ElementId, Interactivity, InteractiveElement, IntoElement, ParentElement, RenderOnce, SharedString, Stateful, StatefulInteractiveElement, Styled, Div, Window, div, prelude::FluentBuilder, px};
use std::rc::Rc;

/// Where a dragged item would land relative to a row (drop indicator).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DropEdge {
    Before,
    After,
    /// Dropping into the row (folders): the row is tinted.
    Inside,
}

/// Sidebar-style row: leading element, title, optional trailing actions, hover / active / drag states.
///
/// Implements `InteractiveElement` and `StatefulInteractiveElement`, so `on_drag`, `on_drop`,
/// `on_mouse_down`... can be chained directly. Trailing content is always visible; `trailing_on_hover`
/// shows it only while the row is hovered or active (like the close button of a tab).
#[derive(IntoElement)]
pub struct ListRow {
    base: Stateful<Div>,
    group: SharedString,
    leading: Option<AnyElement>,
    title: Option<SharedString>,
    trailing: Option<AnyElement>,
    trailing_hover: Option<AnyElement>,
    active: bool,
    dim: bool,
    dragging: bool,
    nested: bool,
    drop: Option<DropEdge>,
    height: Option<f32>,
    on_click: Option<Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>>,
}

impl ListRow {
    pub fn new(id: impl Into<ElementId>) -> Self {
        let id: ElementId = id.into();
        let group: SharedString = format!("row-{id}").into();
        Self { base: div().id(id), group, leading: None, title: None, trailing: None, trailing_hover: None, active: false, dim: false, dragging: false, nested: false, drop: None, height: None, on_click: None }
    }
    pub fn leading(mut self, el: impl IntoElement) -> Self {
        self.leading = Some(el.into_any_element());
        self
    }
    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = Some(title.into());
        self
    }
    pub fn trailing(mut self, el: impl IntoElement) -> Self {
        self.trailing = Some(el.into_any_element());
        self
    }
    pub fn trailing_on_hover(mut self, el: impl IntoElement) -> Self {
        self.trailing_hover = Some(el.into_any_element());
        self
    }
    /// Selected row: raised pill background and full-strength title.
    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }
    /// Unloaded tab: faded title and favicon.
    pub fn dim(mut self, dim: bool) -> Self {
        self.dim = dim;
        self
    }
    /// The row is being dragged: faded.
    pub fn dragging(mut self, dragging: bool) -> Self {
        self.dragging = dragging;
        self
    }
    /// Row inside a folder (smaller radius).
    pub fn nested(mut self, nested: bool) -> Self {
        self.nested = nested;
        self
    }
    pub fn drop_indicator(mut self, edge: Option<DropEdge>) -> Self {
        self.drop = edge;
        self
    }
    /// Overrides the row height (default: the density row height).
    pub fn height(mut self, h: f32) -> Self {
        self.height = Some(h);
        self
    }
    pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(f));
        self
    }
}

impl InteractiveElement for ListRow {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}
impl StatefulInteractiveElement for ListRow {}

impl RenderOnce for ListRow {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.signet().clone();
        let active = self.active;
        let group = self.group.clone();
        let h = self.height.map(px).unwrap_or(t.row_h);
        let title_color = if active { t.text } else { t.text_tab };
        let on_click = self.on_click.clone();
        let indicator = self.drop;
        self.base
            .group(group.clone())
            .relative()
            .flex()
            .flex_none()
            .items_center()
            .gap(space::S2)
            .h(h)
            .pl(if self.nested { space::S2 } else { space::S3 })
            .pr(space::S2)
            .rounded(if self.nested { radius::ROW_IN } else { radius::ROW })
            .text_size(t.type_scale.base)
            .when(active, |d| d.bg(t.active).shadow(t.active_shadow.clone()))
            .when(!active, |d| d.hover(move |s| s.bg(t.surface_2)).active(move |s| s.bg(t.surface_2)))
            .when(self.dragging, |d| d.opacity(0.4))
            .when(indicator == Some(DropEdge::Inside), |d| d.bg(t.sel))
            .when_some(on_click, |d, f| d.cursor_pointer().on_click(move |e, w, cx| f(e, w, cx)))
            .children(self.leading.map(|l| div().flex_none().when(self.dim, |d| d.opacity(0.7)).child(l)))
            .when_some(self.title, |d, title| {
                d.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .text_color(title_color)
                        .when(self.dim, |d| d.opacity(0.7))
                        .child(title),
                )
            })
            .when_some(self.trailing_hover, |d, el| {
                let g = group.clone();
                d.child(div().flex_none().when(!active, |d| d.invisible().group_hover(g, |s| s.visible())).child(el))
            })
            .children(self.trailing.map(|el| div().flex_none().child(el)))
            .when(indicator == Some(DropEdge::Before), |d| d.child(drop_line(&t, true)))
            .when(indicator == Some(DropEdge::After), |d| d.child(drop_line(&t, false)))
    }
}

fn drop_line(t: &crate::theme::SignetTheme, top: bool) -> impl IntoElement {
    div().absolute().left(px(8.0)).right(px(8.0)).h(px(2.0)).rounded_full().bg(t.accent).when(top, |d| d.top(px(-1.0))).when(!top, |d| d.bottom(px(-1.0)))
}

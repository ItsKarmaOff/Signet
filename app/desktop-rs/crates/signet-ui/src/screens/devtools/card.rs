//! Port of `devtools/DevToolsCard.tsx` + the `.devtools-*` rules of `toolbar.css`: the chrome of the developer tools.
//!
//! - [`DevToolsCard`]: the docked card of a pane (header above the native DevTools view) and the resize handle in the
//!   gap between the page and the tools. The main window creates one per pane that has `Pane.devtools` and calls
//!   [`DevToolsCard::set`] with it on each snapshot. Everything is positioned in window coordinates from `PaneDevtools`.
//!   The native DevTools frontend goes in `PaneDevtools.rect` (below the 36 px header); this view leaves it empty.
//! - [`DevToolsWindow`]: root view of the detached tools window (`#devtools`): only the 36 px header (traffic lights on the
//!   left). The native view goes at `x = 0, y = 36, width = W, height = H - 36`.

use super::pointer::track_pointer;
use crate::app_state::{api, tr, tr_args, ui_state};
use crate::icon::IconName;
use crate::theme::{ThemeExt, radius, space, weight};
use crate::widgets::IconButton;
use gpui::{
    AppContext, Context, Entity, InteractiveElement, IntoElement, MouseButton, ParentElement, Pixels, Point, Render, StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder, px,
};
use gpui_component::Icon;
use signet_core::api::UiEvent;
use signet_core::types::{DevToolsDock, DevToolsWindowState, PaneDevtools, PaneDock};

/// Height of the header of the tools card (`DEVTOOLS_BAR`, same value as in `main/devtools/devtools.ts`).
pub const DEVTOOLS_BAR: f32 = 36.0;

fn dock_of(d: PaneDock) -> DevToolsDock {
    match d {
        PaneDock::Right => DevToolsDock::Right,
        PaneDock::Bottom => DevToolsDock::Bottom,
    }
}

/// New size of the tools for a drag: they are after the page, so dragging toward the page enlarges them.
pub fn dragged_size(start_size: f32, start_pointer: f32, pointer: f32) -> i32 {
    (start_size - (pointer - start_pointer)).round() as i32
}

/// Tools header (embedded card or separate window): placement and closing.
fn header(tab_id: &str, dock: DevToolsDock, title: String, dockable: bool, left_pad: f32, cx: &mut gpui::App) -> gpui::Div {
    let t = cx.signet().clone();
    let place = |to: DevToolsDock, icon: IconName, label: String| {
        let tab = tab_id.to_string();
        let id = format!("dock-{to:?}");
        IconButton::new(gpui::SharedString::from(id), icon)
            .small()
            .icon_size(15.0)
            .tooltip(label)
            .active(dock == to)
            .disabled(to != DevToolsDock::Window && !dockable)
            .on_click(move |_, _, cx| api(cx).set_dev_tools_dock(&tab, to))
    };
    let tab_close = tab_id.to_string();
    div()
        .absolute()
        .top_0()
        .left_0()
        .right_0()
        .h(px(DEVTOOLS_BAR))
        .flex()
        .items_center()
        .gap(px(2.0))
        .pl(px(left_pad))
        .pr(space::S2)
        .text_color(t.text_2)
        .text_size(t.type_scale.sub)
        .font_weight(weight::STRONG)
        .child(Icon::from(IconName::Code).size(px(14.0)))
        .child(div().flex_1().min_w_0().ml(space::S2).truncate().child(title))
        .child(place(DevToolsDock::Right, IconName::PanelRight, tr("devtools.dockRight")))
        .child(place(DevToolsDock::Bottom, IconName::PanelBottom, tr("devtools.dockBottom")))
        .child(place(DevToolsDock::Window, IconName::AppWindow, tr("devtools.dockWindow")))
        .child(IconButton::new("dt-close", IconName::X).small().icon_size(15.0).tooltip(tr("devtools.close")).on_click(move |_, _, cx| api(cx).close_dev_tools(&tab_close)))
}

struct Drag {
    /// Pointer position (x for the right dock, y for the bottom one) and tools size at the start.
    pointer: f32,
    size: f32,
}

/// Docked developer tools card of a pane, with its resize handle.
pub struct DevToolsCard {
    tab_id: String,
    info: Option<PaneDevtools>,
    dim: bool,
    drag: Option<Drag>,
}

impl DevToolsCard {
    pub fn new(cx: &mut gpui::App, tab_id: impl Into<String>) -> Entity<Self> {
        let tab_id = tab_id.into();
        cx.new(|_: &mut Context<Self>| Self { tab_id, info: None, dim: false, drag: None })
    }

    /// Latest placement from the snapshot (`Pane.devtools`) and whether the pane is not the focused one.
    pub fn set(&mut self, info: Option<PaneDevtools>, dim: bool, cx: &mut Context<Self>) {
        if self.info != info || self.dim != dim {
            self.info = info;
            self.dim = dim;
            cx.notify();
        }
    }

    fn vertical(&self) -> bool {
        self.info.as_ref().is_some_and(|i| i.dock == PaneDock::Right)
    }

    fn pointer_axis(&self, p: Point<Pixels>) -> f32 {
        if self.vertical() { f32::from(p.x) } else { f32::from(p.y) }
    }

    fn send(&self, size: i32, done: bool, cx: &gpui::App) {
        api(cx).set_dev_tools_size(&self.tab_id, f64::from(size), done);
    }

    fn start(&mut self, p: Point<Pixels>) {
        let Some(info) = &self.info else { return };
        let size = if info.dock == PaneDock::Right { info.card.width } else { info.card.height } as f32;
        self.drag = Some(Drag { pointer: self.pointer_axis(p), size });
    }

    fn moved(&mut self, p: Point<Pixels>, _w: &mut Window, cx: &mut Context<Self>) {
        if let Some(d) = &self.drag {
            self.send(dragged_size(d.size, d.pointer, self.pointer_axis(p)), false, cx);
        }
    }

    fn released(&mut self, p: Point<Pixels>, _w: &mut Window, cx: &mut Context<Self>) {
        if let Some(d) = self.drag.take() {
            self.send(dragged_size(d.size, d.pointer, self.pointer_axis(p)), true, cx);
            cx.notify();
        }
    }

    /// Escape: the starting size is restored.
    fn cancel(&mut self, _w: &mut Window, cx: &mut Context<Self>) {
        if let Some(d) = self.drag.take() {
            self.send(d.size.round() as i32, true, cx);
            cx.notify();
        }
    }
}

impl Render for DevToolsCard {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.signet().clone();
        let Some(info) = self.info.clone() else { return div().into_any_element() };
        let me = cx.entity();
        let vertical = info.dock == PaneDock::Right;
        let active = self.drag.is_some();
        let c = info.card;
        let r = info.divider;
        let tab = self.tab_id.clone();
        let tab_reset = self.tab_id.clone();
        let head = header(&self.tab_id, dock_of(info.dock), tr("devtools.title"), true, 12.0, cx).when(self.dim, |h| h.opacity(0.7));

        let card = div().absolute().left(px(c.x as f32)).top(px(c.y as f32)).w(px(c.width as f32)).h(px(c.height as f32)).rounded(radius::CARD).bg(t.surface).shadow(t.shadow_card.clone()).child(head);

        let me_down = me.clone();
        let handle = div()
            .id(gpui::SharedString::from(format!("dt-handle-{tab}")))
            .group("dt-handle")
            .absolute()
            .left(px(r.x as f32))
            .top(px(r.y as f32))
            .w(px(r.width as f32))
            .h(px(r.height as f32))
            .flex()
            .items_center()
            .justify_center()
            .when(vertical, |d| d.cursor_col_resize())
            .when(!vertical, |d| d.cursor_row_resize())
            .tooltip(|w, cx| gpui_component::tooltip::Tooltip::new(tr("devtools.resize")).build(w, cx))
            .on_mouse_down(MouseButton::Left, move |ev, _, cx| {
                cx.stop_propagation();
                if ev.click_count >= 2 {
                    // double-click: the original size
                    api(cx).set_dev_tools_size(&tab_reset, 0.0, true);
                } else {
                    me_down.update(cx, |this, cx| {
                        this.start(ev.position);
                        cx.notify();
                    });
                }
            })
            .child(
                div()
                    .rounded(px(2.0))
                    .bg(t.accent)
                    .opacity(if active { 1.0 } else { 0.0 })
                    .group_hover("dt-handle", |s| s.opacity(if active { 1.0 } else { 0.45 }))
                    .when(vertical, |d| d.w(px(3.0)).h(space::S7))
                    .when(!vertical, |d| d.w(space::S7).h(px(3.0))),
            );

        div()
            .child(card)
            .child(handle)
            .when(active, |d| d.child(track_pointer(me, Self::moved, Self::released, Self::cancel)))
            .into_any_element()
    }
}

/// Route `#devtools`: header of the detached tools window, to reattach them to the page.
pub struct DevToolsWindow {
    st: Option<DevToolsWindowState>,
}

impl DevToolsWindow {
    pub fn new(_window: &mut Window, cx: &mut gpui::App) -> Entity<Self> {
        cx.new(|cx: &mut Context<Self>| {
            cx.subscribe(&ui_state(cx), |this, _, ev: &UiEvent, cx| {
                if let UiEvent::DevToolsWindowState(s) = ev {
                    this.st = Some(s.clone());
                    cx.notify();
                }
            })
            .detach();
            Self { st: api(cx).dev_tools_window_ready() }
        })
    }
}

impl Render for DevToolsWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.signet().clone();
        let Some(st) = self.st.clone() else { return div().size_full().bg(t.surface).into_any_element() };
        let title = if st.site.is_empty() { tr("devtools.title") } else { tr_args("devtools.windowTitle", &[("site", &st.site)]) };
        // The header serves as the title bar: traffic lights on the left (78 px), it moves the window.
        let bar = super::super::windows::common::drag_area(header(&st.tab_id, DevToolsDock::Window, title, st.dockable, 78.0, cx));
        div().relative().size_full().bg(t.surface).text_size(t.type_scale.base).text_color(t.text).child(bar).into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dragging_toward_the_page_enlarges_the_tools() {
        // right dock: moving the pointer left (smaller x) grows the card
        assert_eq!(dragged_size(400.0, 800.0, 750.0), 450);
        assert_eq!(dragged_size(400.0, 800.0, 850.0), 350);
        assert_eq!(dragged_size(400.0, 800.0, 800.0), 400);
    }

    #[test]
    fn pane_docks_map_to_dock_choices() {
        assert_eq!(dock_of(PaneDock::Right), DevToolsDock::Right);
        assert_eq!(dock_of(PaneDock::Bottom), DevToolsDock::Bottom);
    }
}

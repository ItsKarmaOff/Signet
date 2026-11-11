//! Port of `devtools/Responsive.tsx` + `styles/responsive.css`: responsive design mode of a pane, the area around the
//! simulated page, the device bar and the three size handles.
//!
//! The main window creates one [`ResponsiveFrame`] per pane that has `Pane.responsive` and calls
//! [`ResponsiveFrame::set`] on each snapshot. Everything is placed in window coordinates from `PaneResponsive`
//! (`area`, `bar`, `scale`) and `Pane.rect`. The simulated page (native web view) goes in `Pane.rect`; this view leaves it empty.

use super::devices::{as_custom, device_id, group, is_mobile, label, spec};
use super::pointer::track_pointer;
use crate::app_state::{api, tr, tr_args};
use crate::icon::IconName;
use crate::motion::{RUBBER_C, Spring, reduced_motion, rubberband, unrubberband};
use crate::theme::{ThemeExt, radius, space};
use crate::widgets::{IconButton, MenuEntry, dropdown_menu};
use gpui::{
    AppContext, Context, CursorStyle, Entity, Focusable, InteractiveElement, IntoElement, MouseButton, ParentElement, Pixels, Point, Render, StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder, px,
};
use gpui_component::{
    button::Button,
    input::{Input, InputEvent, InputState},
};
use signet_core::types::{DevDevice, DevResponsive, PaneResponsive, Rect};
use signet_pages::devices::{DeviceKind, MAX_DEVICE, MIN_DEVICE, clamp_size};
use std::time::{Duration, Instant};

/// Distance the handle stretches beyond the limits before its resistance saturates (`rubberband(x, 80)`).
const ELASTIC: f32 = 80.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Edge {
    Right,
    Bottom,
    Corner,
}

/// Rectangle of a handle in the margin around the page `f` (window coordinates): `(x, y, width, height)`.
pub fn handle_rect(edge: Edge, f: &Rect) -> (f64, f64, f64, f64) {
    match edge {
        Edge::Right => (f.x + f.width + 2.0, f.y, 10.0, f.height),
        Edge::Bottom => (f.x, f.y + f.height + 2.0, f.width, 10.0),
        Edge::Corner => (f.x + f.width + 2.0, f.y + f.height + 2.0, 12.0, 12.0),
    }
}

/// Simulated size after the handle moved by `(dx, dy)` window pixels, given the page scale. The page is centered, so the
/// width grows on both sides. Returns the raw (unclamped) size and the size sent to the app (clamped; the axis a handle does
/// not drive keeps its starting value).
pub fn dragged_frame(edge: Edge, start: (u32, u32), raw: (f32, f32)) -> (u32, u32) {
    let width = if edge == Edge::Bottom { start.0 } else { clamp_size(f64::from(raw.0)) };
    let height = if edge == Edge::Right { start.1 } else { clamp_size(f64::from(raw.1)) };
    (width, height)
}

struct HandleDrag {
    edge: Edge,
    /// Last pointer position.
    x: f32,
    y: f32,
    /// Unclamped simulated size, in CSS px.
    fw: f32,
    fh: f32,
    /// Page scale, frozen for the gesture.
    sc: f32,
    start: DevResponsive,
}

pub struct ResponsiveFrame {
    tab_id: String,
    rect: Rect,
    pane: Option<PaneResponsive>,
    width_input: Entity<InputState>,
    height_input: Entity<InputState>,
    drag: Option<HandleDrag>,
    /// Displayed elastic offset of the handle beyond the limits (px), and its return.
    extra: (f32, f32),
    back_gen: u64,
}

impl ResponsiveFrame {
    pub fn new(window: &mut Window, cx: &mut gpui::App, tab_id: impl Into<String>) -> Entity<Self> {
        let tab_id = tab_id.into();
        cx.new(|cx: &mut Context<Self>| {
            let width_input = cx.new(|cx| InputState::new(window, cx));
            let height_input = cx.new(|cx| InputState::new(window, cx));
            for (input, is_width) in [(&width_input, true), (&height_input, false)] {
                cx.subscribe_in(input, window, move |this, input, ev: &InputEvent, window, cx| match ev {
                    InputEvent::PressEnter { .. } | InputEvent::Blur => this.commit(is_width, input.read(cx).value().to_string(), window, cx),
                    _ => {}
                })
                .detach();
            }
            Self { tab_id, rect: Rect::default(), pane: None, width_input, height_input, drag: None, extra: (0.0, 0.0), back_gen: 0 }
        })
    }

    /// Latest placement from the snapshot (`Pane.rect`, `Pane.responsive`).
    pub fn set(&mut self, rect: Rect, responsive: Option<PaneResponsive>, cx: &mut Context<Self>) {
        if self.rect != rect || self.pane != responsive {
            self.rect = rect;
            self.pane = responsive;
            cx.notify();
        }
    }

    fn current(&self) -> Option<DevResponsive> {
        self.pane.as_ref().map(|p| p.responsive.clone())
    }

    fn send(&self, r: Option<DevResponsive>, cx: &gpui::App) {
        api(cx).set_responsive(&self.tab_id, r);
    }

    fn dragging(&self, on: bool, cx: &gpui::App) {
        api(cx).set_responsive_dragging(&self.tab_id, on);
    }

    /// Size field committed (Enter or leaving the field): sent when it is a valid, different size.
    fn commit(&mut self, is_width: bool, text: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(cur) = self.current() else { return };
        let value = if is_width { cur.width } else { cur.height };
        if let Ok(n) = text.trim().parse::<f64>()
            && n.is_finite()
            && n > 0.0
            && clamp_size(n) != value
        {
            let n = clamp_size(n);
            self.send(Some(if is_width { as_custom(&cur, Some(n), None) } else { as_custom(&cur, None, Some(n)) }), cx);
        } else {
            // Invalid or unchanged: the field shows the real value again.
            let input = if is_width { self.width_input.clone() } else { self.height_input.clone() };
            input.update(cx, |i, cx| i.set_value(value.to_string(), window, cx));
        }
    }

    fn pick(&self, device: DevDevice, cx: &gpui::App) {
        let Some(cur) = self.current() else { return };
        if device == DevDevice::Custom {
            return self.send(Some(as_custom(&cur, None, None)), cx);
        }
        let s = spec(device);
        self.send(Some(DevResponsive { device, width: s.width, height: s.height, base: None, mobile: None }), cx);
    }

    // ------------------------------------------------------------------ handle gestures

    fn start_drag(&mut self, edge: Edge, p: Point<Pixels>, cx: &mut Context<Self>) {
        let (Some(cur), Some(pane)) = (self.current(), self.pane.as_ref()) else { return };
        let sc = if pane.scale > 0.0 { pane.scale as f32 } else { 1.0 };
        // Grabbed during the elastic return: start from the handle's displayed position.
        self.back_gen += 1;
        let fw = cur.width as f32 + if edge == Edge::Bottom { 0.0 } else { unrubberband(self.extra.0, ELASTIC, RUBBER_C) * 2.0 / sc };
        let fh = cur.height as f32 + if edge == Edge::Right { 0.0 } else { unrubberband(self.extra.1, ELASTIC, RUBBER_C) / sc };
        self.drag = Some(HandleDrag { edge, x: f32::from(p.x), y: f32::from(p.y), fw, fh, sc, start: cur });
        self.dragging(true, cx);
        cx.notify();
    }

    fn moved(&mut self, p: Point<Pixels>, _w: &mut Window, cx: &mut Context<Self>) {
        let reduced = reduced_motion(cx);
        let Some(g) = self.drag.as_mut() else { return };
        let (px_, py_) = (f32::from(p.x), f32::from(p.y));
        // Scale frozen at the drag start (main process): the handle follows the pointer 1:1.
        if g.edge != Edge::Bottom {
            g.fw += (px_ - g.x) * 2.0 / g.sc;
        }
        if g.edge != Edge::Right {
            g.fh += (py_ - g.y) / g.sc;
        }
        g.x = px_;
        g.y = py_;
        let (width, height) = dragged_frame(g.edge, (g.start.width, g.start.height), (g.fw, g.fh));
        // Beyond the limits, only the handle follows, with resistance.
        if !reduced {
            self.extra = (rubberband((g.fw - width as f32) * g.sc / 2.0, ELASTIC, RUBBER_C), rubberband((g.fh - height as f32) * g.sc, ELASTIC, RUBBER_C));
        }
        let next = as_custom(&g.start, Some(width), Some(height));
        self.send(Some(next), cx);
        cx.notify();
    }

    fn released(&mut self, _p: Point<Pixels>, _w: &mut Window, cx: &mut Context<Self>) {
        if self.drag.take().is_some() {
            // The main process readjusts the page to the pane.
            self.dragging(false, cx);
            self.spring_back(cx);
        }
    }

    /// Escape: the starting size is restored.
    fn cancel(&mut self, _w: &mut Window, cx: &mut Context<Self>) {
        if let Some(g) = self.drag.take() {
            self.send(Some(g.start), cx);
            self.dragging(false, cx);
            self.spring_back(cx);
        }
    }

    /// Elastic return of the handle to its place.
    fn spring_back(&mut self, cx: &mut Context<Self>) {
        let (x, y) = self.extra;
        if x == 0.0 && y == 0.0 {
            return;
        }
        if reduced_motion(cx) {
            self.extra = (0.0, 0.0);
            return cx.notify();
        }
        self.back_gen += 1;
        let gen_ = self.back_gen;
        let (sx, sy) = (Spring::new(x, 0.0).response(0.3).damping(0.8), Spring::new(y, 0.0).response(0.3).damping(0.8));
        cx.spawn(async move |this, cx| {
            let t0 = Instant::now();
            loop {
                cx.background_executor().timer(Duration::from_millis(16)).await;
                let t = t0.elapsed().as_secs_f32();
                let (nx, ny) = (sx.sample(t).0, sy.sample(t).0);
                let done = sx.is_done(t) && sy.is_done(t);
                let alive = this
                    .update(cx, |this, cx| {
                        if this.back_gen != gen_ {
                            return false;
                        }
                        this.extra = if done { (0.0, 0.0) } else { (nx, ny) };
                        cx.notify();
                        true
                    })
                    .unwrap_or(false);
                if done || !alive {
                    break;
                }
            }
        })
        .detach();
    }

    /// Size fields show the real size unless they are being edited.
    fn sync_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(cur) = self.current() else { return };
        for (input, value) in [(&self.width_input, cur.width), (&self.height_input, cur.height)] {
            let focused = input.read(cx).focus_handle(cx).is_focused(window);
            if !focused && input.read(cx).value().as_ref() != value.to_string() {
                input.update(cx, |i, cx| i.set_value(value.to_string(), window, cx));
            }
        }
    }
}

impl Render for ResponsiveFrame {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.signet().clone();
        let Some(pane) = self.pane.clone() else { return div().into_any_element() };
        self.sync_inputs(window, cx);
        let me = cx.entity();
        let r = pane.responsive.clone();
        let mobile = is_mobile(&r);
        let a = pane.area;
        let b = pane.bar;
        let percent = (pane.scale * 100.0).round() as i32;

        // Device list: phones, tablets, computers, then the free size.
        let mut entries: Vec<MenuEntry> = Vec::new();
        for (kind, key) in [(DeviceKind::Phone, "develop.group.phone"), (DeviceKind::Tablet, "develop.group.tablet"), (DeviceKind::Computer, "develop.group.computer")] {
            entries.push(MenuEntry::label(tr(key)));
            for d in group(kind) {
                let (me, id) = (me.clone(), d.id);
                entries.push(MenuEntry::item(label(d), move |_, _, cx| me.update(cx, |this, cx| this.pick(super::devices::device_of(id), cx))).checked(device_id(r.device) == d.id));
            }
        }
        entries.push(MenuEntry::separator());
        {
            let me = me.clone();
            let custom = spec(DevDevice::Custom);
            entries.push(MenuEntry::item(label(custom), move |_, _, cx| me.update(cx, |this, cx| this.pick(DevDevice::Custom, cx))).checked(r.device == DevDevice::Custom));
        }
        let device_button = Button::new("rs-device-btn").label(label(spec(r.device))).tooltip(tr("develop.deviceLabel"));

        let size_input = |id: &'static str, input: &Entity<InputState>, tip: String| {
            div()
                .id(id)
                .flex_none()
                .w(px(62.0))
                .h(px(26.0))
                .rounded(radius::CONTROL)
                .border_1()
                .border_color(t.border)
                .bg(t.surface)
                .text_color(t.text)
                .tooltip(move |w, cx| gpui_component::tooltip::Tooltip::new(tip.clone()).build(w, cx))
                .child(Input::new(input).appearance(false))
        };
        let tip_w = format!("{} ({}-{})", tr("develop.width"), MIN_DEVICE, MAX_DEVICE);
        let tip_h = format!("{} ({}-{})", tr("develop.height"), MIN_DEVICE, MAX_DEVICE);

        let (me_rot, me_mobile, me_exit) = (me.clone(), me.clone(), me.clone());
        let cur_rot = r.clone();
        let cur_mobile = r.clone();
        let bar = div()
            .absolute()
            .left(px(b.x as f32))
            .top(px(b.y as f32))
            .w(px(b.width as f32))
            .h(px(b.height as f32))
            .flex()
            .items_center()
            .justify_center()
            .gap(space::S1)
            .px(space::S2)
            .text_size(t.type_scale.sub)
            .text_color(t.text_2)
            .child(div().mr(space::S2).max_w(px(180.0)).child(dropdown_menu("rs-device", device_button, entries)))
            .child(size_input("rs-w", &self.width_input, tip_w))
            .child(div().text_color(t.text_3).child("\u{d7}"))
            .child(size_input("rs-h", &self.height_input, tip_h))
            .child(IconButton::new("rs-rotate", IconName::RotateCcwSquare).small().icon_size(15.0).tooltip(tr("develop.rotate")).on_click(move |_, _, cx| {
                let next = DevResponsive { width: cur_rot.height, height: cur_rot.width, ..cur_rot.clone() };
                me_rot.update(cx, |this, cx| this.send(Some(next), cx));
            }))
            .when(r.device == DevDevice::Custom, |d| {
                d.child(IconButton::new("rs-mobile", if mobile { IconName::Smartphone } else { IconName::Monitor }).small().icon_size(15.0).tooltip(tr("develop.mobile")).active(mobile).on_click(move |_, _, cx| {
                    let next = DevResponsive { mobile: Some(!mobile), ..as_custom(&cur_mobile, None, None) };
                    me_mobile.update(cx, |this, cx| this.send(Some(next), cx));
                }))
            })
            .when(percent < 100, |d| {
                d.child(
                    div()
                        .id("rs-scale")
                        .ml(space::S1)
                        .text_color(t.text_3)
                        .tooltip({
                            let tip = tr_args("develop.scale", &[("percent", &percent.to_string())]);
                            move |w, cx| gpui_component::tooltip::Tooltip::new(tip.clone()).build(w, cx)
                        })
                        .child(tr_args("develop.scaleShort", &[("percent", &percent.to_string())])),
                )
            })
            .child(IconButton::new("rs-exit", IconName::X).small().icon_size(15.0).tooltip(tr("develop.exitResponsive")).on_click(move |_, _, cx| me_exit.update(cx, |this, cx| this.send(None, cx))));

        let handles = [Edge::Right, Edge::Bottom, Edge::Corner].into_iter().map(|edge| {
            let (x, y, w, h) = handle_rect(edge, &self.rect);
            let me = me.clone();
            let (ex, ey) = if self.drag.as_ref().map(|g| g.edge == edge).unwrap_or(false) || self.extra != (0.0, 0.0) { self.extra } else { (0.0, 0.0) };
            let cursor = match edge {
                Edge::Right => CursorStyle::ResizeLeftRight,
                Edge::Bottom => CursorStyle::ResizeUpDown,
                Edge::Corner => CursorStyle::ResizeUpLeftDownRight,
            };
            let (tw, th) = match edge {
                Edge::Right => (3.0, 28.0),
                Edge::Bottom => (28.0, 3.0),
                Edge::Corner => (6.0, 6.0),
            };
            let group_name: gpui::SharedString = format!("rs-handle-{edge:?}").into();
            div()
                .id(group_name.clone())
                .group(group_name.clone())
                .absolute()
                .left(px(x as f32 + if edge == Edge::Bottom { 0.0 } else { ex }))
                .top(px(y as f32 + if edge == Edge::Right { 0.0 } else { ey }))
                .w(px(w as f32))
                .h(px(h as f32))
                .flex()
                .items_center()
                .justify_center()
                .cursor(cursor)
                .tooltip(|w, cx| gpui_component::tooltip::Tooltip::new(tr("develop.resizeDevice")).build(w, cx))
                .on_mouse_down(MouseButton::Left, move |ev, _, cx| {
                    cx.stop_propagation();
                    me.update(cx, |this, cx| this.start_drag(edge, ev.position, cx));
                })
                .child(
                    div()
                        .w(px(tw))
                        .h(px(th))
                        .when(edge == Edge::Corner, |d| d.rounded_full())
                        .when(edge != Edge::Corner, |d| d.rounded(px(2.0)))
                        .bg(t.text_3)
                        .opacity(0.35)
                        .group_hover(group_name, |s| s.bg(t.accent).opacity(0.9)),
                )
        });

        div()
            .child(div().absolute().left(px(a.x as f32)).top(px(a.y as f32)).w(px(a.width as f32)).h(px(a.height as f32)).rounded_b(crate::screens::main_window::card_radius(&crate::app_state::snapshot(cx))).bg(t.surface_2))
            .child(bar)
            .children(handles)
            .when(self.drag.is_some(), |d| d.child(track_pointer(me, Self::moved, Self::released, Self::cancel)))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_sit_in_the_margin_around_the_page() {
        let f = Rect::new(100.0, 50.0, 400.0, 600.0);
        assert_eq!(handle_rect(Edge::Right, &f), (502.0, 50.0, 10.0, 600.0));
        assert_eq!(handle_rect(Edge::Bottom, &f), (100.0, 652.0, 400.0, 10.0));
        assert_eq!(handle_rect(Edge::Corner, &f), (502.0, 652.0, 12.0, 12.0));
    }

    #[test]
    fn each_handle_drives_its_own_axis_within_bounds() {
        assert_eq!(dragged_frame(Edge::Right, (375, 667), (500.4, 9999.0)), (500, 667));
        assert_eq!(dragged_frame(Edge::Bottom, (375, 667), (9999.0, 700.0)), (375, 700));
        assert_eq!(dragged_frame(Edge::Corner, (375, 667), (10.0, 5000.0)), (MIN_DEVICE, MAX_DEVICE));
    }
}

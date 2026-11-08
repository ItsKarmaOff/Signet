//! Screenshot tool (port of `overlay/Capture.tsx`), placed on the tab's web view: drag to choose an area (or take the
//! visible part, or the whole page), then Copy (Enter), Save or Easel. The rest of the window is dimmed.

use super::common::{lucide, viewport};
use crate::app_state::{api, tr};
use crate::theme::{ThemeExt, radius, space, weight};
use base64::Engine as _;
use gpui::{
    AppContext, ClipboardItem, Context, Entity, Hsla, ImageFormat, InteractiveElement, IntoElement, MouseButton, ParentElement, Pixels, Point, Render, StatefulInteractiveElement, Styled, Task, Window, div,
    prelude::FluentBuilder, px, rgba,
};
use signet_core::api::{CaptureAction, CaptureResult};
use signet_core::types::{CaptureTarget, Rect};
use std::time::Duration;

/// Minimum size of an area (px): below it, it is a plain click.
const MIN: f64 = 4.0;
const VEIL: u32 = 0x0000_0052;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Region,
    Visible,
    Full,
}

pub struct Capture {
    tab_id: String,
    rect: Rect,
    hue: f64,
    kind: Kind,
    sel: Option<Rect>,
    drag: Option<(f64, f64)>,
    busy: bool,
    done: bool,
    failed: bool,
    _task: Option<Task<()>>,
}

impl Capture {
    pub fn new(tab_id: String, rect: Rect, hue: f64, cx: &mut gpui::App) -> Entity<Self> {
        cx.new(|_| Self { tab_id, rect, hue, kind: Kind::Region, sel: None, drag: None, busy: false, done: false, failed: false, _task: None })
    }

    fn ready(&self) -> bool {
        self.kind != Kind::Region || self.sel.is_some_and(|s| s.width >= MIN && s.height >= MIN)
    }

    fn target(&self) -> CaptureTarget {
        match (self.kind, self.sel) {
            (Kind::Region, Some(rect)) => CaptureTarget::Region { rect },
            (Kind::Full, _) => CaptureTarget::Full,
            _ => CaptureTarget::Visible,
        }
    }

    /// Enter copies.
    pub fn handle_key(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if key == "enter" || key == "return" {
            self.take(CaptureAction::Copy, window, cx);
            return true;
        }
        false
    }

    fn take(&mut self, action: CaptureAction, window: &mut Window, cx: &mut Context<Self>) {
        if !self.ready() || self.busy {
            return;
        }
        self.busy = true;
        self.failed = false;
        let res: CaptureResult = api(cx).take_capture(&self.tab_id, self.target(), action);
        self.busy = false;
        if action != CaptureAction::Copy {
            // Success: the app closed the tool (save dialog, or new Easel). Failure: we hand control back.
            self.failed = !res.ok;
            cx.notify();
            return;
        }
        let mut ok = false;
        if res.ok
            && let Some(png) = res.png.as_deref()
            && let Some((_, data)) = png.split_once(',')
            && let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(data.trim())
        {
            cx.write_to_clipboard(ClipboardItem::new_image(&gpui::Image::from_bytes(ImageFormat::Png, bytes)));
            ok = true;
        }
        if !ok {
            self.failed = true;
            cx.notify();
            return;
        }
        self.done = true;
        let ex = cx.background_executor().clone();
        self._task = Some(cx.spawn_in(window, async move |_, cx| {
            ex.timer(Duration::from_millis(650)).await;
            cx.update(|_, cx| api(cx).close_overlay(false)).ok();
        }));
        cx.notify();
    }

    /// Point in the web view, clamped to its bounds.
    fn local(&self, p: Point<Pixels>) -> (f64, f64) {
        ((f64::from(f32::from(p.x)) - self.rect.x).clamp(0.0, self.rect.width), (f64::from(f32::from(p.y)) - self.rect.y).clamp(0.0, self.rect.height))
    }

    fn inside(&self, p: Point<Pixels>) -> bool {
        let (x, y) = (f64::from(f32::from(p.x)), f64::from(f32::from(p.y)));
        x >= self.rect.x && x <= self.rect.x + self.rect.width && y >= self.rect.y && y <= self.rect.y + self.rect.height
    }
}

impl Render for Capture {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.signet().clone();
        let accent = super::common::accent_for(cx, self.hue);
        let (ww, wh) = viewport(window);
        let r = self.rect;
        let veil: Hsla = rgba(VEIL).into();
        let hole = if self.kind == Kind::Region { self.sel } else { Some(Rect { x: 0.0, y: 0.0, width: r.width, height: r.height }) };
        let hole = hole.filter(|h| h.width >= MIN);
        let me = cx.entity();
        let (kind, ready, busy) = (self.kind, self.ready(), self.busy);

        // Dim layer over the whole window, cut out at the chosen spot.
        let mut layer = div().absolute().size_full();
        layer = match hole {
            None => layer.child(div().absolute().size_full().bg(veil)),
            Some(h) => {
                let (hx, hy, hw, hh) = ((r.x + h.x) as f32, (r.y + h.y) as f32, h.width as f32, h.height as f32);
                let whole = kind != Kind::Region;
                let label = if kind == Kind::Region { Some(format!("{} × {}", h.width.round() as i64, h.height.round() as i64)) } else if kind == Kind::Full { Some(tr("capture.fullPageHint")) } else { None };
                layer
                    .child(div().absolute().left_0().top_0().w(px(ww)).h(px(hy.max(0.0))).bg(veil))
                    .child(div().absolute().left_0().top(px(hy + hh)).w(px(ww)).h(px((wh - hy - hh).max(0.0))).bg(veil))
                    .child(div().absolute().left_0().top(px(hy)).w(px(hx.max(0.0))).h(px(hh)).bg(veil))
                    .child(div().absolute().left(px(hx + hw)).top(px(hy)).w(px((ww - hx - hw).max(0.0))).h(px(hh)).bg(veil))
                    .child(
                        div()
                            .absolute()
                            .left(px(hx))
                            .top(px(hy))
                            .w(px(hw))
                            .h(px(hh))
                            .when(!whole, |d| d.border_1().border_color(gpui::white().opacity(0.9)).rounded(px(2.0)))
                            .when(whole, |d| d.border_2().border_color(accent).rounded(radius::ROW))
                            .when_some(label, |d, l| {
                                d.child(
                                    div().absolute().left_0().w_full().flex().justify_center().top(if whole { px(12.0) } else { px(hh + 6.0) }).child(
                                        div().px(px(7.0)).py(px(2.0)).rounded(radius::ICON).bg(gpui::black().opacity(0.72)).text_color(gpui::white()).text_size(t.type_scale.cap).whitespace_nowrap().child(l),
                                    ),
                                )
                            }),
                    )
            }
        };

        let seg = |id: &'static str, icon: &'static str, label: String, k: Kind| {
            let me = me.clone();
            let on = kind == k;
            let hover = t.surface_2;
            div()
                .id(id)
                .flex()
                .items_center()
                .gap(px(6.0))
                .h(px(28.0))
                .px(space::S3)
                .rounded(radius::CONTROL)
                .cursor_pointer()
                .when(on, |d| d.bg(t.surface).shadow(t.active_shadow.clone()))
                .when(!on, |d| d.hover(move |s| s.bg(hover)))
                .child(lucide(icon).size(px(14.0)))
                .child(label)
                .on_click(move |_, _, cx| {
                    me.update(cx, |this, cx| {
                        this.kind = k;
                        if k != Kind::Region {
                            this.sel = None;
                        }
                        cx.notify();
                    })
                })
        };
        let act = |id: &'static str, icon: &'static str, label: String, tip: Option<String>, primary: bool, action: CaptureAction| {
            let me = me.clone();
            let hover = t.surface_2;
            div()
                .id(id)
                .flex()
                .items_center()
                .gap(px(6.0))
                .h(px(28.0))
                .px(space::S3)
                .rounded(radius::CONTROL)
                .when(primary, |d| d.bg(t.accent).text_color(t.on_accent))
                .when(!ready || busy, |d| d.opacity(0.45))
                .when(ready && !busy, |d| {
                    d.cursor_pointer().when(!primary, |d| d.hover(move |s| s.bg(hover))).on_click(move |_, window, cx| me.update(cx, |this, cx| this.take(action, window, cx)))
                })
                .when_some(tip, |d, tip| d.tooltip(move |window, cx| gpui_component::tooltip::Tooltip::new(tip.clone()).build(window, cx)))
                .child(lucide(icon).size(px(14.0)))
                .child(label)
        };

        let bar_inner = if self.done {
            div().flex().items_center().gap(px(6.0)).h(px(28.0)).px(space::S3).text_color(t.accent).child(lucide("check").size(px(15.0))).child(tr("capture.copied")).into_any_element()
        } else {
            div()
                .flex()
                .items_center()
                .gap(space::S1)
                .child(
                    div()
                        .flex()
                        .gap(space::HALF)
                        .p(space::HALF)
                        .rounded(radius::TILE)
                        .bg(t.surface_2)
                        .child(seg("cap-region", "crop", tr("capture.region"), Kind::Region))
                        .child(seg("cap-visible", "monitor", tr("capture.visible"), Kind::Visible))
                        .child(seg("cap-full", "file-image", tr("capture.full"), Kind::Full)),
                )
                .child(div().w(px(1.0)).h(px(18.0)).mx(space::S1).bg(t.border))
                .when(self.failed, |d| d.child(div().h(px(28.0)).px(space::S3).flex().items_center().text_color(t.accent).child(tr("capture.failed"))))
                .child(act("cap-copy", "copy", tr("capture.copy"), Some(tr("capture.copyTitle")), true, CaptureAction::Copy))
                .child(act("cap-save", "download", tr("capture.save"), None, false, CaptureAction::Save))
                .child(act("cap-easel", "pen-tool", tr("capture.easel"), Some(tr("capture.easelTitle")), false, CaptureAction::Easel))
                .child({
                    let hover = t.surface_2;
                    let tip = tr("capture.cancel");
                    div()
                        .id("cap-cancel")
                        .flex()
                        .items_center()
                        .justify_center()
                        .size(px(28.0))
                        .rounded(radius::CONTROL)
                        .cursor_pointer()
                        .hover(move |s| s.bg(hover))
                        .tooltip(move |window, cx| gpui_component::tooltip::Tooltip::new(tip.clone()).build(window, cx))
                        .on_click(|_, _, cx| api(cx).close_overlay(false))
                        .child(lucide("x").size(px(15.0)))
                })
                .into_any_element()
        };

        div()
            .id("capture")
            .absolute()
            .size_full()
            .text_color(t.text)
            .text_size(t.type_scale.base)
            .font_weight(weight::MEDIUM)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, ev: &gpui::MouseDownEvent, _, cx| {
                    if this.done || !this.inside(ev.position) {
                        return;
                    }
                    let p = this.local(ev.position);
                    this.drag = Some(p);
                    this.kind = Kind::Region;
                    this.sel = Some(Rect { x: p.0, y: p.1, width: 0.0, height: 0.0 });
                    cx.notify();
                }),
            )
            .on_mouse_move(cx.listener(move |this, ev: &gpui::MouseMoveEvent, _, cx| {
                let Some(d) = this.drag else { return };
                if ev.pressed_button != Some(MouseButton::Left) {
                    return;
                }
                let p = this.local(ev.position);
                this.sel = Some(Rect { x: d.0.min(p.0), y: d.1.min(p.1), width: (p.0 - d.0).abs(), height: (p.1 - d.1).abs() });
                cx.notify();
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    if this.drag.take().is_some() {
                        this.sel = this.sel.filter(|s| s.width >= MIN && s.height >= MIN);
                        cx.notify();
                    }
                }),
            )
            .child(layer)
            .when(self.kind == Kind::Region && self.sel.is_none(), |d| {
                d.child(
                    div().absolute().left(px(r.x as f32)).top(px((r.y + r.height * 0.4) as f32)).w(px(r.width as f32)).flex().justify_center().child(
                        div().px(space::S4).py(space::S2).rounded_full().bg(gpui::black().opacity(0.6)).text_color(gpui::white()).whitespace_nowrap().child(tr("capture.dragHint")),
                    ),
                )
            })
            .child(
                div()
                    .absolute()
                    .left(px(r.x as f32))
                    .bottom(px((wh - (r.y + r.height) as f32 + 16.0).max(0.0)))
                    .w(px(r.width as f32))
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .occlude()
                            .p(space::S1)
                            .rounded(radius::POP)
                            .bg(t.glass_view)
                            .border_1()
                            .border_color(t.border)
                            .shadow(t.shadow_pop.clone())
                            .whitespace_nowrap()
                            .child(bar_inner),
                    ),
            )
    }
}

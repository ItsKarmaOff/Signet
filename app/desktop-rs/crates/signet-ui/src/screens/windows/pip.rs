//! Port of `windows/Pip.tsx` + `styles/pip.css`: route `#pip`, the floating mini player controls.
//!
//! Window: frameless, transparent background, at the size of the video window. The native web view with the video fills the
//! whole window underneath; this view is laid over it (it is a separate transparent window above, like the Electron `UiView`).
//! Moving and resizing the window is done by the app: install a [`PipHost`] with [`install_pip_host`] (the `SignetApi` has no
//! method for it, `window.signetPip` in the preload). Without a host, a press outside the controls moves only this
//! window through the platform and the resize handles do nothing: the app always installs one.

use crate::app_state::{api, tr, ui_state};
use crate::icon::IconName;
use crate::theme::{ThemeExt, duration, weight};
use gpui::{
    Animation, AnimationExt, AnyElement, App, AppContext, Bounds, Context, CursorStyle, Entity, FocusHandle, Focusable, Global, Hsla, InteractiveElement, IntoElement, MouseButton, ParentElement, Pixels, Render, Rgba,
    StatefulInteractiveElement, Styled, Window, canvas, div, hsla, point, prelude::FluentBuilder, px, relative, size,
};
use gpui_component::{Icon, tooltip::Tooltip};
use signet_core::api::UiEvent;
use signet_core::types::{PipCommand, PipState};
use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// Resize handles, in the order of `HANDLES` in `Pip.tsx`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PipHandle {
    N,
    S,
    E,
    W,
    NE,
    NW,
    SE,
    SW,
}

impl PipHandle {
    pub const ALL: [PipHandle; 8] = [Self::N, Self::S, Self::E, Self::W, Self::NE, Self::NW, Self::SE, Self::SW];

    fn cursor(self) -> CursorStyle {
        match self {
            Self::N | Self::S => CursorStyle::ResizeUpDown,
            Self::E | Self::W => CursorStyle::ResizeLeftRight,
            Self::NE | Self::SW => CursorStyle::ResizeUpRightDownLeft,
            Self::NW | Self::SE => CursorStyle::ResizeUpLeftDownRight,
        }
    }

    /// Name sent to the main process (`n`, `se`...).
    pub fn name(self) -> &'static str {
        match self {
            Self::N => "n",
            Self::S => "s",
            Self::E => "e",
            Self::W => "w",
            Self::NE => "ne",
            Self::NW => "nw",
            Self::SE => "se",
            Self::SW => "sw",
        }
    }
}

/// Phases of a resize gesture. The app polls the pointer from `Start` to the button release, so `Move` (the total
/// movement since `Start`, in screen pixels) is only for scripted gestures.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PipResizePhase {
    Start { handle: PipHandle },
    Move { handle: PipHandle, dx: f32, dy: f32 },
    End,
}

/// What the app provides for the mini player window (`window.signetPip` of the preload).
pub trait PipHost {
    /// A press started (true) or ended (false) outside the controls: the app moves the window with the pointer.
    fn drag(&self, on: bool);
    fn resize(&self, phase: PipResizePhase);
}

struct HostGlobal(Rc<dyn PipHost>);
impl Global for HostGlobal {}

pub fn install_pip_host(cx: &mut App, host: Rc<dyn PipHost>) {
    cx.set_global(HostGlobal(host));
}

fn host(cx: &App) -> Option<Rc<dyn PipHost>> {
    cx.try_global::<HostGlobal>().map(|g| g.0.clone())
}

/// Fraction of a duration, 0 for a live stream or unknown length.
pub fn progress_of(st: &PipState) -> f32 {
    if st.duration.is_finite() && st.duration > 0.0 { (st.current_time / st.duration).clamp(0.0, 1.0) as f32 } else { 0.0 }
}

/// Position of the pointer on the bar as a 0..1 fraction.
pub fn fraction_at(x: f32, left: f32, width: f32) -> f32 {
    if width <= 0.0 { 0.0 } else { ((x - left) / width).clamp(0.0, 1.0) }
}

pub struct Pip {
    st: Option<PipState>,
    hover: bool,
    /// Fraction while dragging on the bar (the real position follows with a delay).
    scrub: Option<f32>,
    scrubbing: bool,
    sent_at: Option<Instant>,
    scrub_gen: u64,
    bar_bounds: Rc<Cell<Bounds<Pixels>>>,
    /// A resize gesture is running (the app polls the pointer until the release).
    grip: bool,
    dragging: bool,
    focus: FocusHandle,
}

impl Pip {
    pub fn new(window: &mut Window, cx: &mut App) -> Entity<Self> {
        let view = cx.new(|cx: &mut Context<Self>| {
            cx.subscribe(&ui_state(cx), |this, _, ev: &UiEvent, cx| {
                if let UiEvent::PipState(s) = ev {
                    this.st = Some(s.clone());
                    cx.notify();
                }
            })
            .detach();
            Self {
                st: None,
                hover: false,
                scrub: None,
                scrubbing: false,
                sent_at: None,
                scrub_gen: 0,
                bar_bounds: Rc::new(Cell::new(Bounds::default())),
                grip: false,
                dragging: false,
                focus: cx.focus_handle(),
            }
        });
        let focus = view.read(cx).focus.clone();
        window.focus(&focus, cx);
        view
    }

    fn command(&self, cmd: PipCommand, value: Option<f64>, cx: &App) {
        api(cx).pip_command(cmd, value);
    }

    fn finite(&self) -> bool {
        self.st.as_ref().is_some_and(|s| s.duration.is_finite() && s.duration > 0.0)
    }

    fn seek_to(&mut self, x: f32, last: bool, cx: &mut Context<Self>) {
        let Some(st) = self.st.as_ref().filter(|_| self.finite()) else { return };
        let b = self.bar_bounds.get();
        let f = fraction_at(x, f32::from(b.origin.x), f32::from(b.size.width));
        self.scrub = Some(f);
        let due = last || self.sent_at.is_none_or(|t| t.elapsed() > Duration::from_millis(60));
        if due {
            self.sent_at = Some(Instant::now());
            self.command(PipCommand::Seek, Some(f64::from(f) * st.duration), cx);
        }
        cx.notify();
    }

    fn end_scrub(&mut self, x: f32, cx: &mut Context<Self>) {
        if !self.scrubbing {
            return;
        }
        self.scrubbing = false;
        self.seek_to(x, true, cx);
        self.scrub_gen += 1;
        let gen_ = self.scrub_gen;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(400)).await;
            this.update(cx, |this, cx| {
                if this.scrub_gen == gen_ {
                    this.scrub = None;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    fn finish_gestures(&mut self, pos: gpui::Point<Pixels>, cx: &mut Context<Self>) {
        self.end_scrub(f32::from(pos.x), cx);
        if std::mem::take(&mut self.grip) {
            if let Some(h) = host(cx) {
                h.resize(PipResizePhase::End);
            }
        }
        if self.dragging {
            self.dragging = false;
            if let Some(h) = host(cx) {
                h.drag(false);
            }
        }
    }
}

impl Focusable for Pip {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// Pill background: translucent dark (the blur of the CSS is not available), opaque with high contrast.
fn pill_bg(hover: bool, high_contrast: bool) -> Hsla {
    match (high_contrast, hover) {
        (true, false) => Rgba { r: 44.0 / 255.0, g: 44.0 / 255.0, b: 46.0 / 255.0, a: 1.0 }.into(),
        (true, true) => Rgba { r: 72.0 / 255.0, g: 72.0 / 255.0, b: 74.0 / 255.0, a: 1.0 }.into(),
        (false, false) => Rgba { r: 28.0 / 255.0, g: 28.0 / 255.0, b: 30.0 / 255.0, a: 0.62 }.into(),
        (false, true) => Rgba { r: 60.0 / 255.0, g: 60.0 / 255.0, b: 64.0 / 255.0, a: 0.74 }.into(),
    }
}

/// Fades a layer when the pointer enters or leaves the window (`--t-hud`).
fn fade<E: Styled + IntoElement + 'static>(el: E, id: &'static str, shown: bool, reduced: bool) -> AnyElement
where
    E: gpui::InteractiveElement,
{
    let ms = if reduced { duration::FAST } else { duration::HUD };
    let key = gpui::ElementId::from((id, if shown { 1usize } else { 0 }));
    let _ = ms;
    el.with_animation(key, Animation::new(ms), move |e, d| e.opacity(if shown { d } else { 1.0 - d })).into_any_element()
}

impl Render for Pip {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.signet().clone();
        let reduced = crate::motion::reduced_motion(cx);
        let st = self.st.clone().unwrap_or_default();
        let finite = self.finite();
        let progress = self.scrub.unwrap_or_else(|| progress_of(&st));
        let hover = self.hover;
        let me = cx.entity();
        let white = hsla(0.0, 0.0, 1.0, 1.0);
        let width = f32::from(window.viewport_size().width);
        let center_gap = (width * 0.11).clamp(14.0, 40.0);
        let high = t.settings.high_contrast;
        let a = api(cx);

        // Round button: translucent white on hover, scale-down feedback is not available.
        let round = |id: &'static str, size_px: f32, tip: String, cmd: PipCommand, child: AnyElement| -> AnyElement {
            let a = a.clone();
            div()
                .id(id)
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .size(px(size_px))
                .rounded_full()
                .text_color(white)
                .cursor_pointer()
                .hover(|s| s.bg(hsla(0.0, 0.0, 1.0, 0.14)))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .tooltip(move |w, cx| Tooltip::new(tip.clone()).build(w, cx))
                .on_click(move |_, _, _| a.pip_command(cmd, None))
                .child(child)
                .into_any_element()
        };
        let skip = |id: &'static str, icon: IconName, tip: String, cmd: PipCommand| {
            round(
                id,
                44.0,
                tip,
                cmd,
                div()
                    .relative()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(Icon::from(icon).size(px(30.0)))
                    .child(div().absolute().mt(px(1.0)).text_size(px(10.0)).font_weight(weight::HERO).line_height(relative(1.0)).child("15"))
                    .into_any_element(),
            )
        };

        let pill = |id: &'static str, tip: Option<String>, w: Option<f32>| {
            let a = api(cx);
            let _ = &a;
            let bg = pill_bg(false, high);
            let bg_h = pill_bg(true, high);
            let mut d = div()
                .id(id)
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .h(px(30.0))
                .rounded(px(10.0))
                .bg(bg)
                .text_color(white)
                .cursor_pointer()
                .hover(move |s| s.bg(bg_h))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());
            if let Some(w) = w {
                d = d.w(px(w));
            }
            if let Some(tip) = tip {
                d = d.tooltip(move |w, cx| Tooltip::new(tip.clone()).build(w, cx));
            }
            d
        };

        let (a_ret, a_close) = (a.clone(), a.clone());
        let top = div()
            .absolute()
            .top(px(16.0))
            .left(px(16.0))
            .right(px(16.0))
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(pill("return", Some(tr("pip.return")), Some(30.0)).on_click(move |_, _, _| a_ret.pip_command(PipCommand::Return, None)).child(Icon::from(IconName::ArrowUpLeft).size(px(16.0))))
            .child(div().flex_1().min_w_0().truncate().text_center().text_size(t.type_scale.base).font_weight(weight::STRONG).text_color(white).child(st.site.clone()))
            .child(
                pill("close", None, None)
                    .gap(px(5.0))
                    .pl(px(9.0))
                    .pr(px(11.0))
                    .text_size(t.type_scale.base)
                    .font_weight(weight::STRONG)
                    .on_click(move |_, _, _| a_close.pip_command(PipCommand::Close, None))
                    .child(Icon::from(IconName::X).size(px(14.0)))
                    .child(tr("pip.close")),
            );

        let center = div().absolute().inset_0().flex().items_center().justify_center().gap(px(center_gap)).child(skip("back", IconName::RotateCcw, tr("pip.back"), PipCommand::Back)).child(round(
            "play",
            56.0,
            if st.playing { tr("pip.pause") } else { tr("pip.play") },
            PipCommand::Toggle,
            Icon::from(if st.playing { IconName::Pause } else { IconName::Play }).size(px(40.0)).into_any_element(),
        )).child(skip("forward", IconName::RotateCw, tr("pip.forward"), PipCommand::Forward));

        // Seek bar: pointer capture is emulated by the root's move and up handlers while `scrubbing`.
        let bounds_cell = self.bar_bounds.clone();
        let track_h = if finite { 5.0 } else { 5.0 };
        let bar = div()
            .id("bar")
            .flex_1()
            .relative()
            .h(px(20.0))
            .flex()
            .items_center()
            .cursor_pointer()
            .when(!finite, |d| d.invisible())
            .child(canvas(move |b, _, _| bounds_cell.set(b), |_, _, _, _| ()).absolute().size_full())
            .on_mouse_down(MouseButton::Left, {
                let me = me.clone();
                move |ev, _, cx| {
                    cx.stop_propagation();
                    me.update(cx, |this, cx| {
                        this.scrubbing = true;
                        this.seek_to(f32::from(ev.position.x), false, cx);
                    });
                }
            })
            .child(
                div()
                    .w_full()
                    .h(px(track_h))
                    .rounded_full()
                    .bg(hsla(0.0, 0.0, 1.0, 0.35))
                    .overflow_hidden()
                    .hover(|s| s.h(px(8.0)))
                    .child(div().h_full().w(relative(progress.clamp(0.0, 1.0))).bg(white)),
            );
        let a_mute = a.clone();
        let bottom = div().absolute().left(px(16.0)).right(px(16.0)).bottom(px(16.0)).flex().items_center().gap(px(10.0)).child(bar).child(
            div()
                .id("mute")
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .size(px(28.0))
                .rounded(px(8.0))
                .text_color(white)
                .opacity(0.8)
                .cursor_pointer()
                .hover(|s| s.opacity(1.0).bg(hsla(0.0, 0.0, 1.0, 0.16)))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .tooltip({
                    let tip = if st.muted { tr("pip.unmute") } else { tr("pip.mute") };
                    move |w, cx| Tooltip::new(tip.clone()).build(w, cx)
                })
                .on_click(move |_, _, _| a_mute.pip_command(PipCommand::Mute, None))
                .child(Icon::from(if st.muted { IconName::VolumeX } else { IconName::Volume2 }).size(px(18.0))),
        );

        // Invisible resize handles, above everything: 14 px corners, 8 px edges.
        let grips = PipHandle::ALL.into_iter().map(|h| {
            let me = me.clone();
            let base = div().id(("grip", h as usize)).absolute().cursor(h.cursor()).on_mouse_down(MouseButton::Left, move |_, _, cx| {
                cx.stop_propagation();
                me.update(cx, |this, _| this.grip = true);
                if let Some(host) = host(cx) {
                    host.resize(PipResizePhase::Start { handle: h });
                }
            });
            match h {
                PipHandle::N => base.top_0().left(px(14.0)).right(px(14.0)).h(px(8.0)),
                PipHandle::S => base.bottom_0().left(px(14.0)).right(px(14.0)).h(px(8.0)),
                PipHandle::E => base.right_0().top(px(14.0)).bottom(px(14.0)).w(px(8.0)),
                PipHandle::W => base.left_0().top(px(14.0)).bottom(px(14.0)).w(px(8.0)),
                PipHandle::NE => base.top_0().right_0().size(px(14.0)),
                PipHandle::NW => base.top_0().left_0().size(px(14.0)),
                PipHandle::SE => base.bottom_0().right_0().size(px(14.0)),
                PipHandle::SW => base.bottom_0().left_0().size(px(14.0)),
            }
        });

        let scrim_alpha = if high { 0.6 } else { 0.35 };
        let _ = (point::<Pixels>, size::<Pixels>);
        let a_key = a.clone();
        div()
            .id("pip")
            .track_focus(&self.focus)
            .relative()
            .size_full()
            .overflow_hidden()
            .rounded(px(12.0))
            .text_color(white)
            .on_hover({
                let me = me.clone();
                move |hovered, _, cx| me.update(cx, |this, cx| {
                    this.hover = *hovered;
                    cx.notify();
                })
            })
            .on_key_down(move |ev, _, cx| {
                let m = ev.keystroke.modifiers;
                if m.platform || m.control || m.alt {
                    return;
                }
                let cmd = match ev.keystroke.key.as_str() {
                    "space" => PipCommand::Toggle,
                    "left" => PipCommand::Back,
                    "right" => PipCommand::Forward,
                    "m" => PipCommand::Mute,
                    "escape" => PipCommand::Close,
                    _ => return,
                };
                cx.stop_propagation();
                a_key.pip_command(cmd, None);
            })
            // Drag the window from anywhere outside the buttons and the bar.
            .on_mouse_down(MouseButton::Left, {
                let me = me.clone();
                move |_, window, cx| match host(cx) {
                    Some(h) => {
                        me.update(cx, |this, _| this.dragging = true);
                        h.drag(true);
                    }
                    None => window.start_window_move(),
                }
            })
            .on_mouse_move({
                let me = me.clone();
                move |ev, _, cx| {
                    me.update(cx, |this, cx| {
                        if this.scrubbing {
                            this.seek_to(f32::from(ev.position.x), false, cx);
                        }
                    });
                }
            })
            .on_mouse_up(MouseButton::Left, {
                let me = me.clone();
                move |ev, _, cx| me.update(cx, |this, cx| this.finish_gestures(ev.position, cx))
            })
            .on_mouse_up_out(MouseButton::Left, {
                let me = me.clone();
                move |ev, _, cx| me.update(cx, |this, cx| this.finish_gestures(ev.position, cx))
            })
            .child(fade(div().absolute().inset_0().bg(hsla(0.0, 0.0, 0.0, scrim_alpha)), "pip-scrim", hover, reduced))
            .children(grips)
            .child(fade(top, "pip-top", hover, reduced))
            .child(fade(center, "pip-center", hover, reduced))
            .child(fade(bottom, "pip-bottom", hover, reduced))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_is_clamped_and_zero_for_live() {
        let mut s = PipState { current_time: 30.0, duration: 120.0, ..Default::default() };
        assert!((progress_of(&s) - 0.25).abs() < 1e-6);
        s.current_time = 500.0;
        assert_eq!(progress_of(&s), 1.0);
        s.duration = f64::INFINITY;
        assert_eq!(progress_of(&s), 0.0);
        s.duration = 0.0;
        assert_eq!(progress_of(&s), 0.0);
    }

    #[test]
    fn fraction_follows_pointer() {
        assert_eq!(fraction_at(150.0, 100.0, 200.0), 0.25);
        assert_eq!(fraction_at(0.0, 100.0, 200.0), 0.0);
        assert_eq!(fraction_at(900.0, 100.0, 200.0), 1.0);
        assert_eq!(fraction_at(10.0, 0.0, 0.0), 0.0);
    }

    #[test]
    fn handles_have_wire_names() {
        let names: Vec<_> = PipHandle::ALL.iter().map(|h| h.name()).collect();
        assert_eq!(names, ["n", "s", "e", "w", "ne", "nw", "se", "sw"]);
    }
}

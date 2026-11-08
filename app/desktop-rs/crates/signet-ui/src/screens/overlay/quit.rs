//! "Hold Cmd+Q to quit" bubble (port of `overlay/QuitHud.tsx`, route `#quit`).

use crate::app_state::{tr, ui_state};
use crate::motion::reduced_motion;
use crate::theme::{ThemeExt, radius, weight};
use gpui::{Animation, AnimationExt, AppContext, Context, ElementId, Entity, IntoElement, ParentElement, Render, Styled, Subscription, Task, Window, div, px};
use signet_core::api::UiEvent;
use signet_core::types::QuitHudState;
use std::time::{Duration, Instant};

/// Until a key repeat proves a held press, the gauge only creeps (ease-in) up to CREEP at the expected repeat time and
/// stays there: a tap barely moves it. The first repeat then lets it fill up over the rest of the hold.
const CREEP: f64 = 0.1;
/// Catch-up speed, times the normal speed.
const CATCH_UP: f64 = 2.0;
/// Share used when the app does not send the expected repeat delay.
const DEFAULT_WAIT_SHARE: f64 = 0.4;

/// Gauge target at time `t` (ms since the press) for a hold of `d` ms with the first repeat expected at `w`.
pub fn gauge_target(t: f64, d: f64, w: f64, repeating: bool) -> f64 {
    if !repeating {
        return CREEP * (t / w).min(1.0).powi(2);
    }
    (CREEP + ((1.0 - CREEP) * (t - w).max(0.0)) / (d - w).max(1.0)).min(1.0)
}

fn now_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64() * 1000.0).unwrap_or(0.0)
}

/// Root view of the `#quit` window (320 x 120 px, centered on the main window; the pill is 264 px wide).
pub struct QuitHud {
    st: QuitHudState,
    live: f64,
    last_start: Option<f64>,
    /// Replays the appearance on every press.
    shown: u64,
    frame: Option<Task<()>>,
    _sub: Subscription,
}

impl QuitHud {
    pub fn new(window: &mut Window, cx: &mut gpui::App) -> Entity<Self> {
        let state = ui_state(cx);
        cx.new(|cx: &mut Context<Self>| {
            let sub = cx.subscribe_in(&state, window, |this: &mut Self, _, ev: &UiEvent, window, cx| {
                if let UiEvent::QuitState(st) = ev {
                    this.on_state(st.clone(), window, cx);
                }
            });
            Self { st: QuitHudState::default(), live: 0.0, last_start: None, shown: 0, frame: None, _sub: sub }
        })
    }

    /// Public so the app can replay the appearance when it shows the window again.
    pub fn replay(&mut self, cx: &mut Context<Self>) {
        self.shown += 1;
        cx.notify();
    }

    fn on_state(&mut self, st: QuitHudState, window: &mut Window, cx: &mut Context<Self>) {
        let new_press = st.holding && st.start != self.last_start;
        self.st = st;
        if new_press {
            self.shown += 1;
        }
        // Released: the loop stops and the gauge stays where it is.
        let (Some(start), Some(duration)) = (self.st.start, self.st.duration) else {
            self.frame = None;
            cx.notify();
            return;
        };
        if !self.st.holding || duration <= 0.0 {
            self.frame = None;
            cx.notify();
            return;
        }
        // Same press resumed after a wrong release guess continues from the frozen value; a new press starts from zero.
        if self.last_start != Some(start) {
            self.live = 0.0;
        }
        self.last_start = Some(start);
        let ex = cx.background_executor().clone();
        self.frame = Some(cx.spawn_in(window, async move |this, cx| {
            let mut last = Instant::now();
            loop {
                ex.timer(Duration::from_millis(16)).await;
                let now = Instant::now();
                let dt = now.duration_since(last).as_secs_f64() * 1000.0;
                last = now;
                let go = this
                    .update(cx, |this, cx| {
                        let t = (now_ms() - start).max(0.0);
                        let w = this.st.wait.unwrap_or(duration * DEFAULT_WAIT_SHARE).max(1.0);
                        let target = gauge_target(t, duration, w, this.st.repeating.unwrap_or(false));
                        // Catches up to its target at most CATCH_UP times the normal speed: no jump when the repeat arrives late.
                        this.live = target.min(this.live + dt * CATCH_UP / duration);
                        cx.notify();
                        this.live < 1.0 && this.st.holding
                    })
                    .unwrap_or(false);
                if !go {
                    break;
                }
            }
        }));
        cx.notify();
    }
}

impl Render for QuitHud {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.signet().clone();
        let reduced = reduced_motion(cx);
        let live = self.live as f32;
        let pill = div()
            .relative()
            .w(px(264.0))
            .pt(px(14.0))
            .px(px(20.0))
            .pb(px(18.0))
            .rounded(radius::POP)
            .overflow_hidden()
            .text_center()
            .bg(t.glass_view)
            .border(px(0.5))
            .border_color(gpui::rgba(0x0000001f))
            .shadow(vec![gpui::BoxShadow { color: gpui::rgba(0x00000029).into(), offset: gpui::point(px(0.0), px(3.0)), blur_radius: px(10.0), spread_radius: px(0.0), inset: false }])
            .text_color(t.text)
            .child(div().text_size(t.type_scale.md).font_weight(weight::STRONG).child(tr("quit.title")))
            .child(div().mt(px(2.0)).text_size(t.type_scale.sub).text_color(t.text_2).child(tr("quit.again")))
            .child(div().absolute().left_0().bottom_0().h(px(4.0)).w(gpui::relative(live.clamp(0.0, 1.0))).bg(t.accent))
            .with_animation(ElementId::from(("quit-in", self.shown)), Animation::new(crate::theme::duration::HUD).with_easing(crate::motion::ease_out), move |el, d| {
                if reduced { el.opacity(d) } else { el.opacity(d).relative().top(px((1.0 - d) * 4.0)) }
            });
        div().size_full().flex().items_center().justify_center().child(pill)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creeps_until_a_repeat_is_seen() {
        assert_eq!(gauge_target(0.0, 1000.0, 400.0, false), 0.0);
        assert!((gauge_target(400.0, 1000.0, 400.0, false) - CREEP).abs() < 1e-9);
        // Never beyond the creep without a repeat.
        assert!((gauge_target(5000.0, 1000.0, 400.0, false) - CREEP).abs() < 1e-9);
    }

    #[test]
    fn fills_after_the_first_repeat() {
        assert!((gauge_target(400.0, 1000.0, 400.0, true) - CREEP).abs() < 1e-9);
        assert!((gauge_target(1000.0, 1000.0, 400.0, true) - 1.0).abs() < 1e-9);
        assert_eq!(gauge_target(2000.0, 1000.0, 400.0, true), 1.0);
        let mid = gauge_target(700.0, 1000.0, 400.0, true);
        assert!(mid > CREEP && mid < 1.0);
    }
}

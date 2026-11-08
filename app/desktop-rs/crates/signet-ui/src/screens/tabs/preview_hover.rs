//! Port of `lib/previewHover.ts`. Tab hover (sidebar, top row): after a stillness delay the app places a preview card
//! next to it. A card already displayed moves from one tab to another without delay; leaving the tabs removes it after
//! a short grace, then the "hot" state lasts a while longer: the next hover shows right away (crossing a row with a gap).
//!
//! [`PreviewHover`] is the pure machine (time is passed in, in milliseconds); [`PreviewDriver`] runs it with GPUI timers.

use gpui::{App, AppContext, Bounds, Context, Entity, Global, Pixels};
use std::time::Duration;

use crate::app_state::api;
use signet_core::types::Rect;

pub const DELAY: u64 = 600;
pub const GRACE: u64 = 140;
pub const WARM: u64 = 500;

/// What the machine asks the app to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    Show { tab_id: String, below: bool },
    Hide,
}

#[derive(Default)]
pub struct PreviewHover {
    shown: bool,
    /// End of the "hot" window that follows the card's removal by the grace.
    warm_until: u64,
    /// Pointer pressed or drag in progress: no preview until the next hover.
    blocked: bool,
    show_timer: Option<(u64, String, bool)>,
    hide_timer: Option<u64>,
}

impl PreviewHover {
    fn clear(&mut self) {
        self.show_timer = None;
        self.hide_timer = None;
    }

    /// `can_show`: the row is still there and is not being renamed (checked when the card would appear).
    fn show(&mut self, id: &str, below: bool, can_show: &dyn Fn(&str) -> bool) -> Vec<Effect> {
        if self.blocked || !can_show(id) {
            return vec![];
        }
        self.shown = true;
        vec![Effect::Show { tab_id: id.to_string(), below }]
    }

    /// Hides the preview right away (click, drag, rename, scroll).
    pub fn hide(&mut self) -> Vec<Effect> {
        self.clear();
        let was = self.shown;
        self.shown = false;
        self.warm_until = 0;
        if was { vec![Effect::Hide] } else { vec![] }
    }

    /// Leaving the tab row: same removal, but the next hover, if it comes quickly, does not wait.
    fn leave_now(&mut self, now: u64) -> Vec<Effect> {
        let was = self.shown;
        let fx = self.hide();
        if was {
            self.warm_until = now + WARM;
        }
        fx
    }

    /// Mouse enters a row (or a chip of the top row: `below`). `pressed`: a mouse button is down.
    pub fn enter(&mut self, now: u64, id: &str, pressed: bool, below: bool, can_show: &dyn Fn(&str) -> bool) -> Vec<Effect> {
        if pressed {
            return vec![];
        }
        self.blocked = false;
        self.clear();
        if self.shown || now < self.warm_until {
            return self.show(id, below, can_show);
        }
        self.show_timer = Some((now + DELAY, id.to_string(), below));
        vec![]
    }

    pub fn leave(&mut self, now: u64) {
        self.show_timer = None;
        self.hide_timer = Some(now + GRACE);
    }

    /// Pointer pressed on a row.
    pub fn pointer_down(&mut self) -> Vec<Effect> {
        self.blocked = true;
        self.hide()
    }

    /// A drag starts from a row.
    pub fn drag_start(&mut self) -> Vec<Effect> {
        self.pointer_down()
    }

    /// Next instant at which [`PreviewHover::tick`] has something to do.
    pub fn next_deadline(&self) -> Option<u64> {
        [self.show_timer.as_ref().map(|t| t.0), self.hide_timer].into_iter().flatten().min()
    }

    /// Runs the timers due at `now`.
    pub fn tick(&mut self, now: u64, can_show: &dyn Fn(&str) -> bool) -> Vec<Effect> {
        let mut fx = Vec::new();
        if let Some((at, id, below)) = self.show_timer.clone()
            && at <= now
        {
            self.show_timer = None;
            fx.extend(self.show(&id, below, can_show));
        }
        if let Some(at) = self.hide_timer
            && at <= now
        {
            self.hide_timer = None;
            fx.extend(self.leave_now(at));
        }
        fx
    }
}

/// Where a hovered row is, and whether it can show a preview right now (not while it is being renamed).
pub struct HoverTarget {
    pub tab_id: String,
    pub bounds: std::rc::Rc<std::cell::Cell<Bounds<Pixels>>>,
    pub below: bool,
    pub editing: bool,
}

/// GPUI side: owns the machine and its timers. One per app, see [`preview_driver`].
pub struct PreviewDriver {
    machine: PreviewHover,
    started: std::time::Instant,
    targets: std::collections::HashMap<String, (std::rc::Rc<std::cell::Cell<Bounds<Pixels>>>, bool, bool)>,
    generation: u64,
}

struct DriverGlobal(Entity<PreviewDriver>);
impl Global for DriverGlobal {}

pub fn preview_driver(cx: &mut App) -> Entity<PreviewDriver> {
    if let Some(g) = cx.try_global::<DriverGlobal>() {
        return g.0.clone();
    }
    let d = cx.new(|_| PreviewDriver { machine: PreviewHover::default(), started: std::time::Instant::now(), targets: Default::default(), generation: 0 });
    cx.set_global(DriverGlobal(d.clone()));
    d
}

impl PreviewDriver {
    fn now(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }

    fn apply(&mut self, fx: Vec<Effect>, cx: &mut Context<Self>) {
        for e in fx {
            match e {
                Effect::Hide => api(cx).hide_tab_preview(),
                Effect::Show { tab_id, .. } => {
                    if let Some((bounds, below, _)) = self.targets.get(&tab_id) {
                        let b = bounds.get();
                        let r = Rect::new(f32::from(b.origin.x) as f64, f32::from(b.origin.y) as f64, f32::from(b.size.width) as f64, f32::from(b.size.height) as f64);
                        api(cx).show_tab_preview(&tab_id, r, *below);
                    }
                }
            }
        }
    }

    fn can_show(&self) -> impl Fn(&str) -> bool + '_ {
        move |id| self.targets.get(id).is_some_and(|t| !t.2)
    }

    fn arm(&mut self, cx: &mut Context<Self>) {
        let Some(at) = self.machine.next_deadline() else { return };
        self.generation += 1;
        let generation = self.generation;
        let wait = at.saturating_sub(self.now());
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(wait)).await;
            this.update(cx, |d, cx| {
                if d.generation != generation {
                    return;
                }
                let now = d.now();
                let mut machine = std::mem::take(&mut d.machine);
                let fx = machine.tick(now, &d.can_show());
                d.machine = machine;
                d.apply(fx, cx);
                d.arm(cx);
            })
            .ok();
        })
        .detach();
    }

    /// Mouse enters a row.
    pub fn hover_enter(&mut self, target: HoverTarget, pressed: bool, cx: &mut Context<Self>) {
        self.targets.insert(target.tab_id.clone(), (target.bounds, target.below, target.editing));
        let now = self.now();
        let mut machine = std::mem::take(&mut self.machine);
        let fx = machine.enter(now, &target.tab_id, pressed, target.below, &self.can_show());
        self.machine = machine;
        self.apply(fx, cx);
        self.arm(cx);
    }

    pub fn hover_leave(&mut self, cx: &mut Context<Self>) {
        let now = self.now();
        self.machine.leave(now);
        self.arm(cx);
    }

    /// Click or drag start on a row, scroll, window blur: no preview until the next hover.
    pub fn pointer_down(&mut self, cx: &mut Context<Self>) {
        let fx = self.machine.pointer_down();
        self.apply(fx, cx);
    }

    /// Hides the card (scroll, rename, window blur) without blocking the next hover.
    pub fn hide(&mut self, cx: &mut Context<Self>) {
        let fx = self.machine.hide();
        self.apply(fx, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs the timers up to `to` (inclusive), like fake timers advancing the clock.
    fn advance(m: &mut PreviewHover, now: &mut u64, to: u64, can: &dyn Fn(&str) -> bool) -> Vec<Effect> {
        let mut fx = Vec::new();
        while let Some(d) = m.next_deadline() {
            if d > to {
                break;
            }
            *now = d.max(*now);
            fx.extend(m.tick(*now, can));
        }
        *now = to;
        fx
    }
    fn shows(fx: &[Effect]) -> usize {
        fx.iter().filter(|e| matches!(e, Effect::Show { .. })).count()
    }
    fn hides(fx: &[Effect]) -> usize {
        fx.iter().filter(|e| matches!(e, Effect::Hide)).count()
    }
    const OK: &dyn Fn(&str) -> bool = &|_| true;

    #[test]
    fn shows_the_card_after_600_ms_of_stillness() {
        let (mut m, mut now) = (PreviewHover::default(), 0);
        assert!(m.enter(now, "t1", false, false, OK).is_empty());
        assert_eq!(shows(&advance(&mut m, &mut now, 599, OK)), 0);
        let fx = advance(&mut m, &mut now, 600, OK);
        assert_eq!(fx, vec![Effect::Show { tab_id: "t1".into(), below: false }]);
    }

    #[test]
    fn flags_a_chip_of_the_top_row() {
        let (mut m, mut now) = (PreviewHover::default(), 0);
        m.enter(now, "t1", false, true, OK);
        let fx = advance(&mut m, &mut now, 600, OK);
        assert_eq!(fx, vec![Effect::Show { tab_id: "t1".into(), below: true }]);
    }

    #[test]
    fn does_nothing_if_a_button_is_pressed() {
        let (mut m, mut now) = (PreviewHover::default(), 0);
        m.enter(now, "t1", true, false, OK);
        assert_eq!(shows(&advance(&mut m, &mut now, 2000, OK)), 0);
    }

    #[test]
    fn shows_nothing_during_a_rename_or_if_the_row_is_gone() {
        let (mut m, mut now) = (PreviewHover::default(), 0);
        let no: &dyn Fn(&str) -> bool = &|_| false;
        m.enter(now, "t1", false, false, no);
        assert_eq!(shows(&advance(&mut m, &mut now, 700, no)), 0);
    }

    #[test]
    fn leaving_before_the_delay_cancels_the_display() {
        let (mut m, mut now) = (PreviewHover::default(), 0);
        m.enter(now, "t1", false, false, OK);
        advance(&mut m, &mut now, 300, OK);
        m.leave(now);
        assert_eq!(shows(&advance(&mut m, &mut now, 2300, OK)), 0);
    }

    #[test]
    fn a_displayed_card_moves_to_the_next_tab_without_delay() {
        let (mut m, mut now) = (PreviewHover::default(), 0);
        m.enter(now, "a", false, false, OK);
        let mut total = shows(&advance(&mut m, &mut now, 600, OK));
        m.leave(now);
        let mut hide = hides(&advance(&mut m, &mut now, 700, OK)); // less than the grace (140 ms)
        let fx = m.enter(now, "b", false, false, OK);
        total += shows(&fx);
        hide += hides(&fx);
        assert_eq!(total, 2);
        assert_eq!(fx, vec![Effect::Show { tab_id: "b".into(), below: false }]);
        assert_eq!(hide, 0);
    }

    #[test]
    fn removes_the_card_after_the_140_ms_grace() {
        let (mut m, mut now) = (PreviewHover::default(), 0);
        m.enter(now, "t1", false, false, OK);
        advance(&mut m, &mut now, 600, OK);
        m.leave(now);
        assert_eq!(hides(&advance(&mut m, &mut now, 739, OK)), 0);
        assert_eq!(hides(&advance(&mut m, &mut now, 740, OK)), 1);
    }

    #[test]
    fn after_removal_the_next_hover_within_500_ms_is_immediate_then_the_delay_returns() {
        let (mut m, mut now) = (PreviewHover::default(), 0);
        m.enter(now, "t1", false, false, OK);
        advance(&mut m, &mut now, 600, OK);
        m.leave(now);
        advance(&mut m, &mut now, 740, OK); // removal at 740, hot window until 1240
        advance(&mut m, &mut now, 1040, OK);
        let fx = m.enter(now, "t1", false, false, OK);
        assert_eq!(shows(&fx), 1);
        m.leave(now);
        advance(&mut m, &mut now, 1040 + 140 + 600, OK); // hot window expired, card removed
        let t = now;
        advance(&mut m, &mut now, t + 1000, OK);
        assert!(m.enter(now, "t1", false, false, OK).is_empty());
        let t = now;
        assert_eq!(shows(&advance(&mut m, &mut now, t + 600, OK)), 1);
    }

    #[test]
    fn a_click_hides_the_card_and_blocks_until_the_next_hover() {
        let (mut m, mut now) = (PreviewHover::default(), 0);
        m.enter(now, "t1", false, false, OK);
        advance(&mut m, &mut now, 600, OK);
        let fx = m.pointer_down();
        assert_eq!(hides(&fx), 1);
        m.leave(now);
        advance(&mut m, &mut now, 1600, OK);
        m.enter(now, "t1", false, false, OK); // new hover: unblocks
        let t = now;
        assert_eq!(shows(&advance(&mut m, &mut now, t + 600, OK)), 1);
    }

    #[test]
    fn a_drag_blocks_the_display_of_an_already_armed_timer() {
        let (mut m, mut now) = (PreviewHover::default(), 0);
        m.enter(now, "t1", false, false, OK);
        m.drag_start();
        assert_eq!(shows(&advance(&mut m, &mut now, 2000, OK)), 0);
    }

    #[test]
    fn hide_only_calls_the_app_if_a_card_is_displayed() {
        let (mut m, mut now) = (PreviewHover::default(), 0);
        assert!(m.hide().is_empty());
        m.enter(now, "t1", false, false, OK);
        advance(&mut m, &mut now, 600, OK);
        assert_eq!(hides(&m.hide()), 1);
        assert!(m.hide().is_empty());
    }
}

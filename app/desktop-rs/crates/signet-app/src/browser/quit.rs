//! Hold-to-quit gauge and press-twice (port of the state machine of `app/quit.ts`).
//!
//! The platform feeds [`Hold`] with key presses (with the auto-repeat flag) and releases. macOS does not report the
//! release of Q while Cmd stays held, so the release is also inferred from the silence of the key repeats, exactly
//! like the Electron app. A tick every 20 ms (`Later::QuitTick`) checks the silence and the hold threshold.

use signet_core::api::UiEvent;
use signet_core::ids::now_ms;
use signet_core::types::QuitHudState;

use super::Core;
use crate::windows::{Cmd, Later};

/// Hold duration, window for the second press.
pub const HOLD_MS: i64 = 900;
pub const AGAIN_MS: i64 = 2000;
/// Check of the threshold and of the release.
pub const TICK_MS: u64 = 20;
/// Minimum fill time after the expected first repeat.
const POST_REPEAT_MS: i64 = 600;
/// Repeat delay taken into account at most (key repeat turned off sets a huge value).
const MAX_WAIT_MS: i64 = 1500;
/// Silence tolerated beyond the expected first repeat before the key counts as released.
const FIRST_MARGIN_MS: i64 = 80;
const MIN_INITIAL_MS: i64 = 150;
const MAX_INITIAL_MS: i64 = 1500;
/// Typical macOS "delay until repeat", used until the system value or a measured first repeat gives the real one.
const FALLBACK_INITIAL_MS: i64 = 400;
/// Key repeats closer than this to the last key event (no native repeat flag available) are not a second press.
pub const REPEAT_GUARD_MS: i64 = 150;

/// What the machine asks the application to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Out {
    /// Show the bubble (if hidden) with this state.
    Show(QuitHudState),
    /// Update the bubble.
    Send(QuitHudState),
    /// Hide the bubble after [`AGAIN_MS`] unless something else happens.
    ScheduleHide,
    /// Cancel a pending hide.
    ClearHide,
    /// Start (or restart) the 20 ms tick.
    StartTick,
    /// Quit for real.
    Confirm,
}

#[derive(Default)]
pub struct Hold {
    pub pressing: bool,
    pub armed: bool,
    press_start: i64,
    hold_for: i64,
    got_repeat: bool,
    last_event: i64,
    initial_ms: i64,
    repeat_ms: i64,
    initial_known: bool,
    /// Release inferred from silence (not a real key up): a late first repeat can still resume the press.
    resumable: bool,
    /// The bubble may be hidden from this time on (ms).
    pub hide_at: i64,
}

impl Hold {
    pub fn new() -> Self {
        Hold { initial_ms: FALLBACK_INITIAL_MS, repeat_ms: 90, hold_for: HOLD_MS, ..Default::default() }
    }

    /// System keyboard timings (ms): delay until repeat, repeat interval.
    pub fn set_system_timings(&mut self, initial: Option<i64>, repeat: Option<i64>) {
        if let Some(i) = initial {
            self.initial_ms = i.clamp(MIN_INITIAL_MS, MAX_INITIAL_MS);
            self.initial_known = true;
        }
        if let Some(r) = repeat {
            self.repeat_ms = r;
        }
    }

    pub fn press_start(&self) -> i64 {
        self.press_start
    }

    fn hold_duration(&self) -> i64 {
        HOLD_MS.max(self.initial_ms.min(MAX_WAIT_MS) + POST_REPEAT_MS)
    }

    fn state(&self, repeating: bool) -> QuitHudState {
        QuitHudState {
            progress: 0.0,
            holding: true,
            start: Some(self.press_start as f64),
            duration: Some(self.hold_for as f64),
            wait: Some(self.initial_ms.min(MAX_WAIT_MS) as f64),
            repeating: repeating.then_some(true),
        }
    }

    /// Cmd+Q down. `repeat` is the auto-repeat flag of the key event.
    pub fn press(&mut self, now: i64, repeat: bool) -> Vec<Out> {
        let mut out = Vec::new();
        if repeat {
            if self.pressing {
                // First repeat: the view lifts the gauge's wait ceiling (sent only once).
                if !self.got_repeat {
                    if !self.initial_known {
                        self.initial_ms = (now - self.press_start).clamp(MIN_INITIAL_MS, MAX_INITIAL_MS);
                    }
                    self.hold_for = self.hold_duration();
                    out.push(Out::Send(self.state(true)));
                }
                self.got_repeat = true;
                self.last_event = now;
            } else if self.resumable && self.armed && now - self.press_start <= MAX_INITIAL_MS + 300 {
                // Release was guessed too early (repeat delay longer than expected): the key is still held, resume.
                self.resumable = false;
                self.pressing = true;
                self.got_repeat = true;
                self.last_event = now;
                if !self.initial_known {
                    self.initial_ms = (now - self.press_start).clamp(MIN_INITIAL_MS, MAX_INITIAL_MS);
                }
                self.hold_for = self.hold_duration();
                out.push(Out::ClearHide);
                out.push(Out::Send(self.state(true)));
                out.push(Out::StartTick);
            }
            return out;
        }
        // New press: second press if there is a bubble waiting.
        if self.armed {
            self.confirm(&mut out);
            return out;
        }
        self.pressing = true;
        self.armed = true;
        self.press_start = now;
        self.hold_for = self.hold_duration();
        self.last_event = now;
        self.got_repeat = false;
        self.resumable = false;
        out.push(Out::ClearHide);
        out.push(Out::Show(self.state(false)));
        out.push(Out::StartTick);
        out
    }

    /// Press without hold information (menu item, platform without key up): first time bubble, second time quit.
    pub fn press_twice(&mut self, now: i64) -> Vec<Out> {
        let mut out = Vec::new();
        if self.armed && !self.pressing {
            self.confirm(&mut out);
            return out;
        }
        if self.armed {
            // Within a hold in progress: a menu event is the same key, not a second press.
            return out;
        }
        self.armed = true;
        self.hide_at = now + AGAIN_MS;
        out.push(Out::Show(QuitHudState { progress: 0.0, holding: false, ..Default::default() }));
        out.push(Out::ScheduleHide);
        out
    }

    fn confirm(&mut self, out: &mut Vec<Out>) {
        self.armed = false;
        self.pressing = false;
        out.push(Out::ClearHide);
        out.push(Out::Confirm);
    }

    /// Key up, Cmd released or focus lost.
    pub fn release(&mut self, now: i64) -> Vec<Out> {
        self.resumable = false;
        if !self.pressing {
            return Vec::new();
        }
        self.pressing = false;
        self.hide_at = now + AGAIN_MS;
        vec![Out::Send(QuitHudState { progress: 0.0, holding: false, ..Default::default() }), Out::ScheduleHide]
    }

    /// Every [`TICK_MS`] while pressing. Returns the outputs and whether the tick must go on.
    pub fn tick(&mut self, now: i64) -> (Vec<Out>, bool) {
        if !self.pressing {
            return (Vec::new(), false);
        }
        let limit = if self.got_repeat { 150.max(self.repeat_ms * 2) } else { self.initial_ms + FIRST_MARGIN_MS };
        // No repeat at the expected time: the key was released.
        if now - self.last_event > limit {
            let early = !self.got_repeat;
            let out = self.release(now);
            self.resumable = early;
            return (out, false);
        }
        // Without repeats we cannot tell whether the press is brief: no quit.
        if self.got_repeat && now - self.press_start >= self.hold_for {
            let mut out = Vec::new();
            self.confirm(&mut out);
            return (out, false);
        }
        (Vec::new(), true)
    }

    /// The hide timer fired. True when the bubble must go.
    pub fn hide_due(&mut self, now: i64) -> bool {
        if self.pressing || now < self.hide_at {
            return false;
        }
        self.armed = false;
        true
    }
}

impl Core {
    fn quit_apply(&mut self, outs: Vec<Out>) {
        for o in outs {
            match o {
                Out::Show(s) | Out::Send(s) => {
                    self.quit.hud = s.clone();
                    let was = self.quit.shown;
                    self.quit.shown = true;
                    self.emit(UiEvent::QuitState(s));
                    if !was {
                        self.changed_view();
                    }
                }
                Out::ScheduleHide => self.send(Cmd::Later(AGAIN_MS as u64 + 20, Later::QuitHudHide)),
                Out::ClearHide => self.quit_hold.hide_at = i64::MAX,
                Out::StartTick => self.send(Cmd::Later(TICK_MS, Later::QuitTick)),
                Out::Confirm => self.quit_confirm(),
            }
        }
    }

    fn quit_confirm(&mut self) {
        // Self-test check: the process is gone before the test task could report it.
        if std::env::var_os("SIGNET_SELFTEST").is_some_and(|v| !v.is_empty() && v != "0") {
            let held = now_ms() - self.quit_hold.press_start();
            println!("[selftest] {} quit.hold - confirmed after {held} ms (gauge {HOLD_MS} ms or the repeat delay + {POST_REPEAT_MS} ms)", if held >= HOLD_MS { "PASS" } else { "FAIL" });
            // The process exits right after: a piped stdout is block buffered and the line would be lost.
            let _ = std::io::Write::flush(&mut std::io::stdout());
        }
        self.quit.shown = false;
        self.emit(UiEvent::QuitState(QuitHudState::default()));
        self.changed_view();
        self.quit_now();
    }

    /// Cmd+Q (Ctrl+Q) went down, with the auto-repeat flag of the native event.
    pub fn quit_key_down(&mut self, repeat: bool) {
        if self.quitting || self.quit.confirmed {
            return;
        }
        // No main window (macOS keeps the app alive after it closes): the gauge has no window to live in, quit now.
        if self.win.is_none() || self.state().settings.confirm_quit == Some(false) {
            return self.quit_now();
        }
        self.quit_poll_system_timings();
        let outs = self.quit_hold.press(now_ms(), repeat);
        self.quit_apply(outs);
    }

    /// Q or the modifier went up, or the application lost the focus.
    pub fn quit_key_up(&mut self) {
        let outs = self.quit_hold.release(now_ms());
        self.quit_apply(outs);
    }

    pub fn quit_tick(&mut self) {
        let (outs, go) = self.quit_hold.tick(now_ms());
        self.quit_apply(outs);
        if go {
            self.send(Cmd::Later(TICK_MS, Later::QuitTick));
        }
    }

    pub fn quit_hide_due(&mut self) {
        if !self.quit_hold.hide_due(now_ms()) || !self.quit.shown {
            return;
        }
        self.quit.shown = false;
        self.emit(UiEvent::QuitState(QuitHudState::default()));
        self.changed_view();
    }

    /// Menu item, Dock, platforms without native key up: press twice.
    pub fn request_quit(&mut self) {
        if self.quitting || self.quit.confirmed {
            return;
        }
        // No main window (macOS keeps the app alive after it closes): the gauge has no window to live in, quit now.
        if self.win.is_none() || self.state().settings.confirm_quit == Some(false) {
            return self.quit_now();
        }
        let now = now_ms();
        // Key repeat of a held Ctrl+Q reaches us as repeated commands: not a second press.
        if self.quit_hold.armed && now - self.quit_last_cmd < REPEAT_GUARD_MS {
            self.quit_last_cmd = now;
            return;
        }
        self.quit_last_cmd = now;
        let outs = self.quit_hold.press_twice(now);
        self.quit_apply(outs);
    }

    /// Reads the system key repeat settings (`defaults` on macOS, the system settings on Windows and Linux).
    pub fn quit_read_system_timings(&mut self) {
        #[cfg(not(target_os = "macos"))]
        {
            let (tx, rx) = std::sync::mpsc::channel::<(Option<i64>, Option<i64>)>();
            let _ = tx.send(crate::quitkeys::system_timings());
            self.quit_timings = Some(rx);
        }
        #[cfg(target_os = "macos")]
        {
            let (tx, rx) = std::sync::mpsc::channel::<(Option<i64>, Option<i64>)>();
            std::thread::spawn(move || {
                let read = |key: &str| {
                    std::process::Command::new("defaults")
                        .args(["read", "-g", key])
                        .output()
                        .ok()
                        .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse::<i64>().ok())
                        .filter(|n| *n > 0)
                        .map(|n| n * 15)
                };
                let _ = tx.send((read("InitialKeyRepeat"), read("KeyRepeat")));
            });
            self.quit_timings = Some(rx);
        }
    }

    /// Takes the system timings once the background read is done.
    pub fn quit_poll_system_timings(&mut self) {
        if let Some(rx) = &self.quit_timings
            && let Ok((i, r)) = rx.try_recv()
        {
            self.quit_hold.set_system_timings(i, r);
            self.quit_timings = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn confirms(o: &[Out]) -> bool {
        o.contains(&Out::Confirm)
    }

    #[test]
    fn brief_press_does_not_quit() {
        let mut h = Hold::new();
        let o = h.press(0, false);
        assert!(matches!(o[1], Out::Show(_)));
        // No repeat and the key is released early.
        let (o, go) = h.tick(10);
        assert!(o.is_empty() && go);
        let (o, go) = h.tick(600);
        assert!(!go && !o.is_empty());
        assert!(!h.pressing && h.armed);
    }

    #[test]
    fn held_press_quits_after_the_hold() {
        let mut h = Hold::new();
        h.press(0, false);
        let mut t = 400;
        let mut quit = false;
        while t < 2000 {
            let o = h.press(t, true);
            assert!(!confirms(&o));
            let (o, _) = h.tick(t + 10);
            if confirms(&o) {
                quit = true;
                break;
            }
            t += 30;
        }
        assert!(quit);
        // Not before the hold duration (1000 ms with a 400 ms repeat delay).
        assert!(t >= 900);
    }

    #[test]
    fn release_before_the_end_cancels() {
        let mut h = Hold::new();
        h.press(0, false);
        h.press(400, true);
        h.press(430, true);
        let o = h.release(600);
        assert!(!confirms(&o));
        let (o, go) = h.tick(900);
        assert!(!go && o.is_empty());
        assert!(!h.pressing);
    }

    #[test]
    fn repeats_are_never_a_second_press() {
        let mut h = Hold::new();
        h.press(0, false);
        for t in [400, 430, 460, 490] {
            assert!(!confirms(&h.press(t, true)));
        }
    }

    #[test]
    fn press_twice_quits() {
        let mut h = Hold::new();
        h.press(0, false);
        h.release(100);
        assert!(confirms(&h.press(500, false)));
    }

    #[test]
    fn bubble_goes_after_the_window() {
        let mut h = Hold::new();
        h.press(0, false);
        h.release(100);
        assert!(!h.hide_due(1000));
        assert!(h.hide_due(2200));
        assert!(!h.armed);
    }

    #[test]
    fn late_first_repeat_resumes() {
        let mut h = Hold::new();
        h.set_system_timings(None, None);
        h.press(0, false);
        let (_, go) = h.tick(600);
        assert!(!go);
        let o = h.press(700, true);
        assert!(o.contains(&Out::StartTick));
        assert!(h.pressing);
    }
}

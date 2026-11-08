//! Ctrl+Q on Windows and Linux: decoding of raw key events into the "quit key" stream the hold-to-quit gauge needs
//! (`browser/quit.rs`), the same stream the macOS key monitor produces from NSEvents.
//!
//! The platform glue (a Windows keyboard hook, CEF's pre key event, GPUI key events) calls [`feed`] from wherever a
//! key arrives; the main loop drains the result with [`take`] and drives `Core::quit_key_down` / `quit_key_up`. The
//! decision "is this exactly Ctrl+Q" and the repeat detection live in [`Tracker`] and are unit tested.
//!
//! Only the Q key with the control modifier alone counts (Electron: `input.control && !input.meta`, no shift, no
//! alt). A press of Q while the previous one is still down is a repeat, whatever flag the source reported. The
//! release of Q or of Ctrl stops the gauge.

#![allow(dead_code)]

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

/// Keys the hold-to-quit cares about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Q,
    Ctrl,
    Other,
}

/// One raw key event with the modifier state at that time.
#[derive(Clone, Copy, Debug)]
pub struct RawKey {
    pub key: Key,
    pub up: bool,
    /// The source says it is an auto-repeat (not always reliable, see [`Tracker`]).
    pub repeat: bool,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub meta: bool,
}

/// What the gauge is told.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuitKey {
    Down { repeat: bool },
    Up,
}

/// Turns raw key events into [`QuitKey`]s.
#[derive(Default)]
pub struct Tracker {
    q_held: bool,
}

impl Tracker {
    /// Returns the quit key to send (if any) and whether the event must be swallowed (it is Ctrl+Q going down).
    /// `paused`: a shortcut is being typed in Settings, Ctrl+Q is a key like any other there.
    pub fn feed(&mut self, k: RawKey, paused: bool) -> (Option<QuitKey>, bool) {
        if k.up {
            if matches!(k.key, Key::Q | Key::Ctrl) && self.q_held {
                self.q_held = false;
                return (Some(QuitKey::Up), false);
            }
            return (None, false);
        }
        if k.key != Key::Q || paused || !k.ctrl || k.meta || k.shift || k.alt {
            return (None, false);
        }
        let repeat = k.repeat || self.q_held;
        self.q_held = true;
        (Some(QuitKey::Down { repeat }), true)
    }

    /// The keyboard went elsewhere (window or application deactivated): the key state is unknown.
    pub fn reset(&mut self) -> Option<QuitKey> {
        std::mem::take(&mut self.q_held).then_some(QuitKey::Up)
    }
}

static TRACKER: Mutex<Tracker> = Mutex::new(Tracker { q_held: false });
static QUEUE: Mutex<Vec<QuitKey>> = Mutex::new(Vec::new());
static PAUSED: AtomicBool = AtomicBool::new(false);

/// Feeds a raw key from any source and thread; returns true when the key must be swallowed.
pub fn feed(k: RawKey) -> bool {
    let (out, swallow) = TRACKER.lock().map(|mut t| t.feed(k, PAUSED.load(Ordering::Relaxed))).unwrap_or((None, false));
    if let (Some(q), Ok(mut queue)) = (out, QUEUE.lock()) {
        queue.push(q);
    }
    swallow
}

/// Releases the gauge (focus lost).
pub fn release() {
    if let (Some(q), Ok(mut queue)) = (TRACKER.lock().ok().and_then(|mut t| t.reset()), QUEUE.lock()) {
        queue.push(q);
    }
}

/// The quit keys received since the last call, in order.
pub fn take() -> Vec<QuitKey> {
    QUEUE.lock().map(|mut q| std::mem::take(&mut *q)).unwrap_or_default()
}

/// Suspends the interception (shortcut being typed in Settings).
pub fn set_paused(on: bool) {
    PAUSED.store(on, Ordering::Relaxed);
}

// --- Sources ---

const VK_Q: i32 = 0x51;
const VK_CONTROL: i32 = 0x11;

fn key_of_vk(vk: i32) -> Key {
    match vk {
        VK_Q => Key::Q,
        VK_CONTROL => Key::Ctrl,
        _ => Key::Other,
    }
}

/// CEF pre key event (browser pages, all platforms): `up` is true for a key up, false for a key down.
pub fn from_cef(e: &signet_web::KeyEvent, up: bool) -> Option<RawKey> {
    if !up && !e.pressed {
        return None; // char events
    }
    Some(RawKey { key: key_of_vk(e.windows_key_code), up, repeat: e.is_repeat, ctrl: e.ctrl, shift: e.shift, alt: e.alt, meta: e.cmd })
}

/// Observer for `signet_web::set_key_observer`.
pub fn on_cef_key(e: &signet_web::KeyEvent, up: bool) -> bool {
    from_cef(e, up).is_some_and(feed)
}

/// GPUI key down (`is_held` is the auto-repeat flag).
pub fn from_gpui_down(e: &gpui::KeyDownEvent) -> RawKey {
    let m = &e.keystroke.modifiers;
    let key = if e.keystroke.key == "q" { Key::Q } else { Key::Other };
    RawKey { key, up: false, repeat: e.is_held, ctrl: m.control, shift: m.shift, alt: m.alt, meta: m.platform }
}

/// GPUI key up.
pub fn from_gpui_up(e: &gpui::KeyUpEvent) -> RawKey {
    let m = &e.keystroke.modifiers;
    let key = if e.keystroke.key == "q" { Key::Q } else { Key::Other };
    RawKey { key, up: true, repeat: false, ctrl: m.control, shift: m.shift, alt: m.alt, meta: m.platform }
}

/// GPUI modifiers changed: the control key going up is a release.
pub fn from_gpui_modifiers(e: &gpui::ModifiersChangedEvent) -> Option<RawKey> {
    (!e.modifiers.control).then_some(RawKey { key: Key::Ctrl, up: true, repeat: false, ctrl: false, shift: false, alt: false, meta: false })
}

/// Windows keyboard hook (virtual key codes).
#[cfg(target_os = "windows")]
pub fn on_win_key(k: signet_web::keys_win::WinKey) -> bool {
    feed(RawKey { key: key_of_vk(k.vk as i32), up: k.up, repeat: k.repeat, ctrl: k.ctrl, shift: k.shift, alt: k.alt, meta: k.meta })
}

/// Handlers for the key events of a GPUI window root (Linux; Windows has the hook, macOS has the key monitor).
#[cfg(not(target_os = "macos"))]
pub fn gpui_handlers(d: gpui::Div) -> gpui::Div {
    use gpui::InteractiveElement as _;
    d.capture_key_down(|e, _w, cx| {
        if feed(from_gpui_down(e)) {
            cx.stop_propagation();
        }
    })
    .on_key_up(|e, _w, _cx| {
        feed(from_gpui_up(e));
    })
    .on_modifiers_changed(|e, _w, _cx| {
        if let Some(k) = from_gpui_modifiers(e) {
            feed(k);
        }
    })
}

#[cfg(target_os = "macos")]
pub fn gpui_handlers(d: gpui::Div) -> gpui::Div {
    d
}

// --- System key repeat settings ---

/// Windows `SPI_GETKEYBOARDDELAY` (0..3) to the delay until repeat in ms (250, 500, 750, 1000).
pub fn win_delay_ms(index: u32) -> i64 {
    (i64::from(index.min(3)) + 1) * 250
}

/// Windows `SPI_GETKEYBOARDSPEED` (0..31, about 2.5 to 30 characters per second) to the repeat interval in ms.
pub fn win_interval_ms(speed: u32) -> i64 {
    let cps = 2.5 + f64::from(speed.min(31)) * 27.5 / 31.0;
    (1000.0 / cps).round() as i64
}

/// System timings as the gauge takes them: (delay until repeat, repeat interval), `None` when unknown.
pub fn system_timings() -> (Option<i64>, Option<i64>) {
    #[cfg(target_os = "windows")]
    {
        return match signet_web::keys_win::repeat_settings() {
            Some((d, s)) => (Some(win_delay_ms(d)), Some(win_interval_ms(s))),
            None => (None, None),
        };
    }
    #[cfg(target_os = "linux")]
    {
        return match signet_web::keys_linux::repeat_settings() {
            Some((d, i)) => (Some(i64::from(d)).filter(|n| *n > 0), Some(i64::from(i)).filter(|n| *n > 0)),
            None => (None, None),
        };
    }
    #[allow(unreachable_code)]
    (None, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(key: Key, up: bool, ctrl: bool) -> RawKey {
        RawKey { key, up, repeat: false, ctrl, shift: false, alt: false, meta: false }
    }

    #[test]
    fn ctrl_q_alone_is_swallowed_and_repeats_are_detected() {
        let mut t = Tracker::default();
        assert_eq!(t.feed(key(Key::Q, false, true), false), (Some(QuitKey::Down { repeat: false }), true));
        // The source gives no repeat flag: the key is still down.
        assert_eq!(t.feed(key(Key::Q, false, true), false), (Some(QuitKey::Down { repeat: true }), true));
        assert_eq!(t.feed(key(Key::Q, false, true), false), (Some(QuitKey::Down { repeat: true }), true));
        assert_eq!(t.feed(key(Key::Q, true, true), false), (Some(QuitKey::Up), false));
        // A new press after the release is a new press.
        assert_eq!(t.feed(key(Key::Q, false, true), false), (Some(QuitKey::Down { repeat: false }), true));
    }

    #[test]
    fn repeat_flag_of_the_source_is_honored() {
        let mut t = Tracker::default();
        let mut k = key(Key::Q, false, true);
        k.repeat = true;
        assert_eq!(t.feed(k, false), (Some(QuitKey::Down { repeat: true }), true));
    }

    #[test]
    fn releasing_ctrl_stops_the_gauge() {
        let mut t = Tracker::default();
        t.feed(key(Key::Q, false, true), false);
        assert_eq!(t.feed(key(Key::Ctrl, true, false), false), (Some(QuitKey::Up), false));
        // Nothing to release any more.
        assert_eq!(t.feed(key(Key::Ctrl, true, false), false), (None, false));
        assert_eq!(t.feed(key(Key::Q, true, false), false), (None, false));
    }

    #[test]
    fn other_combinations_are_ignored() {
        let mut t = Tracker::default();
        assert_eq!(t.feed(key(Key::Q, false, false), false), (None, false));
        for (shift, alt, meta) in [(true, false, false), (false, true, false), (false, false, true)] {
            let mut k = key(Key::Q, false, true);
            (k.shift, k.alt, k.meta) = (shift, alt, meta);
            assert_eq!(t.feed(k, false), (None, false));
        }
        assert_eq!(t.feed(key(Key::Other, false, true), false), (None, false));
    }

    #[test]
    fn paused_while_recording_a_shortcut() {
        let mut t = Tracker::default();
        assert_eq!(t.feed(key(Key::Q, false, true), true), (None, false));
        assert_eq!(t.feed(key(Key::Q, false, true), false).1, true);
    }

    #[test]
    fn reset_releases_a_held_key_once() {
        let mut t = Tracker::default();
        assert_eq!(t.reset(), None);
        t.feed(key(Key::Q, false, true), false);
        assert_eq!(t.reset(), Some(QuitKey::Up));
        assert_eq!(t.reset(), None);
    }

    #[test]
    fn cef_events() {
        let ev = |code: i32, pressed: bool, ctrl: bool| signet_web::KeyEvent {
            pressed,
            windows_key_code: code,
            native_key_code: 0,
            character: None,
            cmd: false,
            ctrl,
            alt: false,
            shift: false,
            is_repeat: false,
        };
        assert_eq!(from_cef(&ev(0x51, true, true), false).map(|k| (k.key, k.up)), Some((Key::Q, false)));
        assert_eq!(from_cef(&ev(0x11, false, false), true).map(|k| (k.key, k.up)), Some((Key::Ctrl, true)));
        // Char events (not pressed, not up) are not keys.
        assert!(from_cef(&ev(0x11, false, true), false).is_none());
    }

    #[test]
    fn windows_system_timings() {
        assert_eq!(win_delay_ms(0), 250);
        assert_eq!(win_delay_ms(1), 500);
        assert_eq!(win_delay_ms(9), 1000);
        assert_eq!(win_interval_ms(0), 400);
        assert_eq!(win_interval_ms(31), 33);
        assert_eq!(win_interval_ms(31 + 10), 33);
    }
}

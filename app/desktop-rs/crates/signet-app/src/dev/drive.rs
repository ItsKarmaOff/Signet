//! Capture harness: synthetic input for `SIGNET_ACTIONS` (macOS only), so interactive states (open menus, focused
//! fields, recording chords, selected rows) can be shot. NSEvents are sent straight to the window (`sendEvent:`).
//!
//! `SIGNET_ACTIONS` is a `;` separated list, coordinates in points relative to the captured window's top-left:
//! `click:x,y`, `move:x,y`, `type:text`, `key:esc|return|tab|backspace|up|down|left|right|space`, `chord:cmd+shift+k`,
//! `wait:ms`.

#[cfg(target_os = "macos")]
mod imp {
        use std::time::Duration;

    use gpui::AsyncApp;

    use objc2::msg_send;
    use objc2::runtime::AnyObject;
    use objc2_foundation::{NSPoint, NSString};

    static TARGET: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

    fn window() -> *mut AnyObject {
        let num = TARGET.load(std::sync::atomic::Ordering::Relaxed);
        unsafe {
            let app: *mut AnyObject = msg_send![objc2::class!(NSApplication), sharedApplication];
            let arr: *mut AnyObject = msg_send![app, windows];
            let n: usize = msg_send![arr, count];
            for i in 0..n {
                let w: *mut AnyObject = msg_send![arr, objectAtIndex: i];
                let wn: isize = msg_send![w, windowNumber];
                if wn == num {
                    return w;
                }
            }
        }
        std::ptr::null_mut()
    }

    fn send(ev: *mut AnyObject) {
        let w = window();
        if !w.is_null() && !ev.is_null() {
            unsafe {
                let _: () = msg_send![w, sendEvent: ev];
            }
        }
    }

    /// `kind`: 1 left down, 2 left up, 5 moved, 6 dragged (NSEventType). `y` is from the window top.
    fn mouse(kind: u64, x: f64, y: f64) {
        let w = window();
        if w.is_null() {
            return;
        }
        unsafe {
            let frame: objc2_foundation::NSRect = msg_send![w, frame];
            let num = TARGET.load(std::sync::atomic::Ordering::Relaxed);
            let nil: *mut AnyObject = std::ptr::null_mut();
            let ev: *mut AnyObject = msg_send![objc2::class!(NSEvent),
                mouseEventWithType: kind, location: NSPoint::new(x, frame.size.height - y), modifierFlags: 0u64,
                timestamp: 0.0f64, windowNumber: num, context: nil, eventNumber: 0isize,
                clickCount: 1isize, pressure: if kind == 1 { 1.0f32 } else { 0.0f32 }];
            send(ev);
        }
    }

    /// `kind`: 10 key down, 11 key up.
    fn key_event(kind: u64, code: u16, flags: u64, chars: &str) {
        unsafe {
            let num = TARGET.load(std::sync::atomic::Ordering::Relaxed);
            let nil: *mut AnyObject = std::ptr::null_mut();
            let c = NSString::from_str(chars);
            let ev: *mut AnyObject = msg_send![objc2::class!(NSEvent),
                keyEventWithType: kind, location: NSPoint::new(0.0, 0.0), modifierFlags: flags, timestamp: 0.0f64,
                windowNumber: num, context: nil, characters: &*c, charactersIgnoringModifiers: &*c,
                isARepeat: false, keyCode: code];
            send(ev);
        }
    }

    fn key(code: u16, flags: u64, chars: &str) {
        key_event(10, code, flags, chars);
        key_event(11, code, flags, chars);
    }

    fn text(s: &str) {
        for c in s.chars() {
            key(0, 0, &c.to_string());
        }
    }

    fn keycode(name: &str) -> Option<u16> {
        Some(match name {
            "esc" => 53,
            "return" => 36,
            "tab" => 48,
            "backspace" => 51,
            "space" => 49,
            "left" => 123,
            "right" => 124,
            "down" => 125,
            "up" => 126,
            "a" => 0,
            "s" => 1,
            "d" => 2,
            "f" => 3,
            "h" => 4,
            "g" => 5,
            "z" => 6,
            "x" => 7,
            "c" => 8,
            "v" => 9,
            "b" => 11,
            "q" => 12,
            "w" => 13,
            "e" => 14,
            "r" => 15,
            "y" => 16,
            "t" => 17,
            "o" => 31,
            "u" => 32,
            "i" => 34,
            "p" => 35,
            "l" => 37,
            "j" => 38,
            "k" => 40,
            "n" => 45,
            "m" => 46,
            _ => return None,
        })
    }

    fn key_chars(name: &str) -> &str {
        match name {
            "return" => "\r",
            "tab" => "\t",
            "esc" => "\u{1b}",
            "space" => " ",
            "backspace" => "\u{7f}",
            n if n.len() == 1 => n,
            _ => "",
        }
    }

    fn point(arg: &str, origin: (f64, f64)) -> Option<(f64, f64)> {
        let (x, y) = arg.split_once(',')?;
        Some((origin.0 + x.trim().parse::<f64>().ok()?, origin.1 + y.trim().parse::<f64>().ok()?))
    }

    /// Runs the script against a window whose top-left is `origin` (screen points).
    pub async fn run(cx: &AsyncApp, script: &str, origin: (f64, f64), window: isize) {
        TARGET.store(window, std::sync::atomic::Ordering::Relaxed);
        let pause = |ms: u64| cx.background_executor().timer(Duration::from_millis(ms));
        for step in script.split(';').map(str::trim).filter(|s| !s.is_empty()) {
            let (cmd, arg) = step.split_once(':').unwrap_or((step, ""));
            match cmd {
                "click" => {
                    if let Some((x, y)) = point(arg, origin) {
                        mouse(5, x, y);
                        pause(120).await;
                        mouse(1, x, y);
                        pause(60).await;
                        mouse(2, x, y);
                    }
                }
                "move" => {
                    if let Some((x, y)) = point(arg, origin) {
                        mouse(5, x, y);
                    }
                }
                "type" => text(arg),
                "key" => {
                    if let Some(c) = keycode(arg) {
                        key(c, 0, key_chars(arg));
                    }
                }
                "chord" => {
                    let mut flags = 0u64;
                    let mut code = None;
                    for part in arg.split('+') {
                        match part {
                            "cmd" => flags |= 0x100000,
                            "shift" => flags |= 0x20000,
                            "alt" => flags |= 0x80000,
                            "ctrl" => flags |= 0x40000,
                            k => code = keycode(k),
                        }
                    }
                    if let Some(c) = code {
                        key(c, flags, key_chars(arg.rsplit('+').next().unwrap_or("")));
                    }
                }
                "wait" => {}
                _ => eprintln!("[capture] unknown action {step}"),
            }
            let ms = if cmd == "wait" { arg.parse().unwrap_or(300) } else { 250 };
            pause(ms).await;
        }
    }
}

#[cfg(target_os = "macos")]
pub use imp::run;

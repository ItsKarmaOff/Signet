//! Reads what the system does on a double-click of a title bar (macOS: Desktop & Dock setting, Linux: GNOME setting).
//! The parsing is in `signet_core::titlebar`.

use signet_core::titlebar::TitlebarAction;

/// The current action. macOS reads the preferences on every call (the user can change them while the app runs).
/// Linux returns the value cached by the last background read and starts a new one.
pub fn action() -> TitlebarAction {
    #[cfg(target_os = "macos")]
    {
        crate::mac::titlebar_action()
    }
    #[cfg(target_os = "linux")]
    {
        linux::action()
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        TitlebarAction::Zoom
    }
}

/// Starts the first background read (Linux only, no-op elsewhere).
pub fn warm_up() {
    #[cfg(target_os = "linux")]
    linux::refresh();
}

#[cfg(target_os = "linux")]
mod linux {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
    use std::time::{Duration, Instant};

    use signet_core::titlebar::{TitlebarAction, gnome_action};

    static CACHED: AtomicU8 = AtomicU8::new(0);
    static BUSY: AtomicBool = AtomicBool::new(false);

    fn encode(a: TitlebarAction) -> u8 {
        match a {
            TitlebarAction::Zoom => 0,
            TitlebarAction::Fill => 1,
            TitlebarAction::Minimize => 2,
            TitlebarAction::Menu => 3,
            TitlebarAction::Nothing => 4,
        }
    }

    fn decode(v: u8) -> TitlebarAction {
        match v {
            2 => TitlebarAction::Minimize,
            3 => TitlebarAction::Menu,
            4 => TitlebarAction::Nothing,
            1 => TitlebarAction::Fill,
            _ => TitlebarAction::Zoom,
        }
    }

    /// `gsettings get ...` with a 1 second limit. `None` when the tool is missing, fails or is too slow.
    fn read() -> Option<String> {
        let mut child = Command::new("gsettings")
            .args(["get", "org.gnome.desktop.wm.preferences", "action-double-click-titlebar"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let start = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(status)) if status.success() => break,
                Ok(Some(_)) | Err(_) => return None,
                Ok(None) if start.elapsed() > Duration::from_secs(1) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            }
        }
        let mut out = String::new();
        child.stdout.take()?.read_to_string(&mut out).ok()?;
        Some(out)
    }

    /// Reads the setting on a short-lived thread and caches it. Never blocks the caller.
    pub fn refresh() {
        if BUSY.swap(true, Ordering::AcqRel) {
            return;
        }
        let spawned = std::thread::Builder::new().name("titlebar-setting".into()).spawn(|| {
            CACHED.store(encode(gnome_action(read().as_deref())), Ordering::Relaxed);
            BUSY.store(false, Ordering::Release);
        });
        if spawned.is_err() {
            BUSY.store(false, Ordering::Release);
        }
    }

    pub fn action() -> TitlebarAction {
        refresh();
        decode(CACHED.load(Ordering::Relaxed))
    }
}

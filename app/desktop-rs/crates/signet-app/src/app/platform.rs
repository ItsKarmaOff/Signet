//! Port of `app/platform.ts`: per-platform window chrome.

use gpui::{Pixels, TitlebarOptions, WindowBackgroundAppearance, point, px};

pub const IS_MAC: bool = cfg!(target_os = "macos");

/// Title bar and background of a window. Mac: transparent title bar with the traffic lights placed over the UI
/// (`lights` = position in points), translucent background when `vibrancy`. Elsewhere: the system frame.
pub fn window_chrome(title: &str, lights: Option<(f32, f32)>, vibrancy: bool) -> (Option<TitlebarOptions>, WindowBackgroundAppearance) {
    if IS_MAC {
        let pos: Option<gpui::Point<Pixels>> = lights.map(|(x, y)| point(px(x), px(y)));
        (
            Some(TitlebarOptions { title: Some(title.to_string().into()), appears_transparent: true, traffic_light_position: pos }),
            if vibrancy { WindowBackgroundAppearance::Blurred } else { WindowBackgroundAppearance::Opaque },
        )
    } else {
        (Some(TitlebarOptions { title: Some(title.to_string().into()), appears_transparent: false, traffic_light_position: None }), WindowBackgroundAppearance::Opaque)
    }
}

/// Folder where downloads go by default.
pub fn system_downloads() -> std::path::PathBuf {
    dirs::download_dir().or_else(|| dirs::home_dir().map(|h| h.join("Downloads"))).unwrap_or_else(std::env::temp_dir)
}

/// Opens a URL or a file with the system handler.
pub fn open_external(target: &str) {
    #[cfg(target_os = "macos")]
    let prog = "open";
    #[cfg(target_os = "windows")]
    let prog = "explorer";
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    let prog = "xdg-open";
    let _ = std::process::Command::new(prog).arg(target).spawn();
}

/// Shows a file in the file manager.
pub fn reveal(path: &str) {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").args(["-R", path]).spawn();
    }
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("explorer").arg(format!("/select,{path}")).spawn();
    }
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    {
        let dir = std::path::Path::new(path).parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
        open_external(&dir);
    }
}

/// Puts text on the system clipboard (GPUI clipboard is reached through the app context by the caller; this is the
/// context-free fallback used from the browser core).
pub fn copy_text(text: &str) {
    #[cfg(target_os = "macos")]
    {
        use std::io::Write;
        if let Ok(mut c) = std::process::Command::new("pbcopy").stdin(std::process::Stdio::piped()).spawn() {
            if let Some(mut i) = c.stdin.take() {
                let _ = i.write_all(text.as_bytes());
            }
            let _ = c.wait();
        }
    }
    #[cfg(target_os = "windows")]
    {
        use std::io::Write;
        if let Ok(mut c) = std::process::Command::new("clip").stdin(std::process::Stdio::piped()).spawn() {
            if let Some(mut i) = c.stdin.take() {
                let _ = i.write_all(text.as_bytes());
            }
            let _ = c.wait();
        }
    }
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    {
        use std::io::Write;
        for prog in [("wl-copy", vec![]), ("xclip", vec!["-selection", "clipboard"])] {
            if let Ok(mut c) = std::process::Command::new(prog.0).args(&prog.1).stdin(std::process::Stdio::piped()).spawn() {
                if let Some(mut i) = c.stdin.take() {
                    let _ = i.write_all(text.as_bytes());
                }
                let _ = c.wait();
                return;
            }
        }
    }
}

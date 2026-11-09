//! Port of `dev/capture.ts` and `dev/probes.ts`: options applied to the main window, `SIGNET_EVAL`, and
//! `SIGNET_CAPTURE=<target>:<abs path.png>` (open the window, wait, save a screenshot, quit).
//!
//! Electron composes web views into `capturePage`. Here the OS captures each window by id (`screencapture -l`, so other
//! windows on screen do not matter) and the floating child windows of the main window (command bar, popovers, find bar...)
//! are composited at their offsets. Images are at the screen scale (2x on Retina), like Electron.

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use gpui::{App, AsyncApp};
use signet_core::model::OpenOptions;
use signet_core::types::{DevToolsDock, MediaInfo, PaletteMode, PopoverRequest, Rect};

use super::{flag, var};
use crate::browser::{Browser, Core};
use crate::windows::{Cmd, Win};

fn num(name: &str) -> Option<u64> {
    var(name).and_then(|v| v.parse().ok())
}

async fn sleep(cx: &AsyncApp, ms: u64) {
    cx.background_executor().timer(Duration::from_millis(ms)).await;
}

/// Runs `f` on the core after `ms`.
fn later(cx: &AsyncApp, b: &Rc<Browser>, ms: u64, f: impl FnOnce(&mut Core) + 'static) {
    let b = b.clone();
    cx.spawn(async move |cx| {
        sleep(cx, ms).await;
        f(&mut b.core.borrow_mut());
    })
    .detach();
}

/// Options of the main window, applied before the shot: SIGNET_COMPACT, SIGNET_MEDIA, SIGNET_FLOAT=toolbar,
/// SIGNET_DEVTOOLS=right|bottom|window, SIGNET_OPEN=<url> (+ SIGNET_OPEN_BG=<ms>), SIGNET_SITE=1.
fn apply_main_options(b: &Rc<Browser>, cx: &AsyncApp) {
    let mut c = b.core.borrow_mut();
    if flag("SIGNET_COMPACT") {
        c.set_compact(true);
    }
    if flag("SIGNET_MEDIA") {
        let other = c.state().tabs.values().find(|t| t.url.contains("news.ycombinator.com")).map(|t| t.id.clone());
        if let Some(tab_id) = other {
            c.phase3.media.debug = Some(MediaInfo {
                tab_id,
                title: "Nuits blanches - Épisode 12".into(),
                artist: Some("Radio Exemple".into()),
                artwork: var("SIGNET_MEDIA_ART").filter(|a| !a.is_empty()),
                site: "example.fm".into(),
                playing: true,
                muted: false,
                can_skip: true,
            });
            c.changed();
        }
    }
    if var("SIGNET_FLOAT").as_deref() == Some("toolbar") {
        c.set_toolbar_hover(true);
    }
    drop(c);
    let dock = match var("SIGNET_DEVTOOLS").as_deref() {
        Some("right") => Some(DevToolsDock::Right),
        Some("bottom") => Some(DevToolsDock::Bottom),
        Some("window") => Some(DevToolsDock::Window),
        _ => None,
    };
    if let Some(dock) = dock {
        later(cx, b, 1000, move |c| {
            if let Some(id) = c.active_tab_id() {
                c.set_dev_tools_dock(&id, dock);
            }
        });
    }
    // Test hook: SIGNET_RELOAD=<ms>,<ms>... reloads the active tab (the same path as Cmd+R) at those delays.
    if let Some(list) = var("SIGNET_RELOAD") {
        for ms in list.split(',').filter_map(|m| m.trim().parse::<u64>().ok()) {
            later(cx, b, ms, |c| c.reload_tab(None, false));
        }
    }
    if let Some(url) = var("SIGNET_OPEN") {
        let bg = num("SIGNET_OPEN_BG").unwrap_or(0);
        let id = b.core.borrow_mut().open_tab(&url, OpenOptions { activate: bg == 0, ..Default::default() });
        if bg > 0 {
            later(cx, b, bg, move |c| c.activate_tab(&id));
        }
    }
    if flag("SIGNET_SITE") {
        later(cx, b, 2500, |c| c.open_site_panel(Rect::new(400.0, 10.0, 28.0, 28.0)));
    }
}

/// HUD hooks of the main window: SIGNET_FIND=<query> (opens the bar and types), SIGNET_QUIT_HUD=<hold ms> (simulates a
/// held Cmd+Q without quitting: the gauge is captured at that time), SIGNET_TOAST=1 (clears today's tabs: archive toast),
/// SIGNET_ZOOM=1 (zoom in once, bubble open).
fn apply_hud_options(b: &Rc<Browser>, cx: &AsyncApp, wait: u64) {
    if let Some(q) = var("SIGNET_FIND") {
        later(cx, b, 800, move |c| {
            c.show_find();
            c.find.state.query = q.clone();
            c.find(&q, signet_core::api::FindOptions { forward: true, find_next: false });
            c.show_find();
        });
    }
    if let Some(hold) = num("SIGNET_QUIT_HUD") {
        let start = wait.saturating_sub(hold + 300);
        let b2 = b.clone();
        cx.spawn(async move |cx| {
            sleep(cx, start).await;
            for i in 0..(hold / 40) {
                b2.core.borrow_mut().quit_key_down(i > 0);
                sleep(cx, 40).await;
            }
        })
        .detach();
    }
    if flag("SIGNET_TOAST") {
        later(cx, b, 1500, |c| c.clear_today());
    }
    if let Some(v) = var("SIGNET_PW_PROMPT") {
        later(cx, b, 1500, move |c| c.debug_password_prompt(v == "update"));
    }
    if flag("SIGNET_ZOOM") {
        later(cx, b, 1500, |c| {
            if let Some(id) = c.active_tab_id() {
                c.zoom_page(&id, 1);
                c.open_zoom_bubble(&id);
            }
        });
    }
}

/// `SIGNET_EVAL=<script>`: runs in the active tab, the result is printed (`EVAL ...`).
async fn run_probes(b: &Rc<Browser>, cx: &AsyncApp) {
    let Some(js) = var("SIGNET_EVAL") else { return };
    let view = {
        let c = b.core.borrow();
        c.active_tab_id().and_then(|id| c.views.view_of(&id))
    };
    if let Some(v) = view {
        let params = serde_json::json!({ "expression": js, "returnByValue": true, "awaitPromise": true }).to_string();
        v.execute_devtools_method("Runtime.evaluate", &params, |r| println!("EVAL {}", r.unwrap_or_else(|e| format!("error {e}"))));
        sleep(cx, 800).await;
    }
}

/// Anchors of the tab row popovers (window coordinates; the tab row sits at the top in the horizontal layout).
fn popover_of(kind: &str, b: &Rc<Browser>) -> Option<(PopoverRequest, Rect)> {
    Some(match kind {
        "folder" => (PopoverRequest::Folder { folder_id: "dossier-bleu".into() }, Rect::new(326.0, 6.0, 28.0, 28.0)),
        "downloads" => {
            let w = b.core.borrow().win.as_ref().map_or(1280.0, |w| w.size.0);
            (PopoverRequest::Downloads, Rect::new(w - 150.0, 6.0, 28.0, 28.0))
        }
        "space-form" => (PopoverRequest::SpaceForm { space_id: None }, Rect::new(90.0, 6.0, 130.0, 30.0)),
        "spaces" => (PopoverRequest::Spaces, Rect::new(78.0, 6.0, 130.0, 30.0)),
        "live-folder" => (PopoverRequest::LiveFolder { folder_id: None }, Rect::new(78.0, 6.0, 140.0, 28.0)),
        "icon-folder" => (PopoverRequest::Icon { target: signet_core::types::IconTarget::Folder { folder_id: "dossier-bleu".into() } }, Rect::new(326.0, 6.0, 28.0, 28.0)),
        "rename-folder" => (PopoverRequest::Rename { target: signet_core::types::RenameTarget::Folder { folder_id: "dossier-bleu".into() } }, Rect::new(326.0, 6.0, 28.0, 28.0)),
        _ => return None,
    })
}

/// Entry point, called once the main window was requested.
pub fn start(browser: &Rc<Browser>, cx: &mut App) {
    let Some((target, section, file)) = super::capture_spec() else { return };
    let b = browser.clone();
    cx.spawn(async move |cx| {
        for _ in 0..100 {
            if b.core.borrow().win.is_some() {
                break;
            }
            sleep(cx, 100).await;
        }
        let ok = run(&b, cx, &target, section, &file).await;
        if !ok {
            eprintln!("[capture] failed for {target}");
        }
        b.core.borrow().send(Cmd::Quit);
    })
    .detach();
}

async fn run(b: &Rc<Browser>, cx: &AsyncApp, target: &str, section: Option<String>, file: &Path) -> bool {
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (b, cx, target, section, file);
        eprintln!("[capture] only implemented on macOS");
        false
    }
    #[cfg(target_os = "macos")]
    {
        use mac::*;
        let main_wait = num("SIGNET_WAIT").unwrap_or(3500);
        let side_wait = num("SIGNET_WAIT").unwrap_or(1500);
        // The welcome window opens by itself on a fresh profile, before this point: it must not count as already there.
        let before: std::collections::HashSet<isize> = windows().iter().filter(|w| target != "welcome" || (w.w - 640.0).abs() > 1.0).map(|w| w.num).collect();
        let is_main = matches!(target, "main" | "palette" | "find");
        if is_main {
            apply_main_options(b, cx);
            apply_hud_options(b, cx, main_wait);
        }
        let mut other: Option<isize> = None;
        let wait;
        match target {
            "main" => wait = main_wait,
            "palette" => {
                let (mode, anchor) = match var("SIGNET_PALETTE_MODE").as_deref() {
                    Some("edit-url") => (PaletteMode::EditUrl, Some(Rect::new(372.0, 12.0, 560.0, 28.0))),
                    Some("split") => (PaletteMode::Split, None),
                    _ => (PaletteMode::NewTab, None),
                };
                later(cx, b, 800, move |c| c.open_palette(mode, anchor));
                wait = main_wait;
            }
            "find" => {
                later(cx, b, (main_wait / 2).max(1500), |c| c.show_find());
                wait = main_wait;
            }
            "settings" | "library" | "welcome" | "passkey" | "boost" => {
                if target == "library" {
                    super::seed::seed_history(&mut b.core.borrow_mut().history);
                }
                let (win, sec) = match target {
                    "settings" => (Win::Settings, section.clone()),
                    "library" => (Win::Library, section.clone()),
                    "welcome" => (Win::Welcome, None),
                    "passkey" => (Win::Passkey, None),
                    _ => (Win::Boost, Some(section.clone().unwrap_or_else(|| "wikipedia.org".into()))),
                };
                if win == Win::Boost {
                    // Registers the editor so that the app resends its state to the window (like the real entry points).
                    b.core.borrow_mut().open_boost(sec.as_deref().unwrap_or_default());
                } else {
                    b.core.borrow().send(Cmd::OpenWin(win, sec));
                }
                wait = side_wait;
            }
            "little" => {
                b.core.borrow_mut().open_little_window(&var("SIGNET_OPEN").unwrap_or_else(|| "https://example.com/".into()));
                wait = num("SIGNET_WAIT").unwrap_or(3500);
            }
            _ => {
                b.core.borrow_mut().open_private_window();
                wait = side_wait;
            }
        }
        sleep(cx, wait).await;

        if is_main {
            run_probes(b, cx).await;
            if var("SIGNET_SPACE_FORM").is_some() {
                let id = b.core.borrow().active_space().id.clone();
                b.core.borrow_mut().edit_space(Some(id));
                sleep(cx, 500).await;
            }
            if let Some(kind) = var("SIGNET_POPOVER")
                && let Some((req, anchor)) = popover_of(&kind, b)
            {
                b.core.borrow_mut().open_popover(req, anchor);
                sleep(cx, 700).await;
            }
        }

        let main_num = main_window_number(b);
        if var("SIGNET_CAPTURE_DEBUG").is_some() {
            for w in windows() {
                eprintln!("[capture] window {} {:?} parent {} visible {} at ({:.0},{:.0}) {}x{}", w.num, w.title, w.parent, w.visible, w.x, w.top, w.w, w.h);
            }
        }
        if !is_main {
            for _ in 0..50 {
                other = windows().into_iter().filter(|w| !before.contains(&w.num) && w.visible && w.parent != main_num.unwrap_or(-1) || (target == "passkey" && !before.contains(&w.num) && w.visible)).max_by(|a, c| (a.w * a.h).total_cmp(&(c.w * c.h))).map(|w| w.num);
                if other.is_some() {
                    break;
                }
                sleep(cx, 100).await;
            }
        }
        let dark = is_dark();
        let site = flag("SIGNET_SITE");
        let float = var("SIGNET_FLOAT").as_deref() == Some("toolbar");
        let sibling = |suffix: &str| PathBuf::from(file.to_string_lossy().trim_end_matches(".png").to_string() + "-" + suffix + ".png");

        // Side captures, next to the requested file: site panel, floating toolbar, detached developer tools.
        if is_main {
            let all = windows();
            let child = |name: &str| all.iter().find(|w| w.visible && w.title == name && Some(w.parent) == main_num);
            if site && let Some(w) = child("overlay") {
                save_window(w.num, &sibling("site"), None);
            }
            if float && let Some(w) = child("toolbar") {
                save_window(w.num, &sibling("float"), None);
            }
            let tools = signet_core::i18n::t("devtools.title");
            if let Some(w) = all.iter().find(|w| w.visible && w.title == tools && w.parent == 0) {
                save_window(w.num, &sibling("devtools"), None);
            }
        }

        let Some(win) = (if is_main { main_num } else { other }) else {
            eprintln!("[capture] window not found (main {main_num:?}, others {:?})", windows().iter().map(|w| (w.num, w.title.clone(), w.parent)).collect::<Vec<_>>());
            return false;
        };
        run_actions(cx, win).await;
        let mut skip: Vec<&str> = Vec::new();
        if site {
            skip.push("overlay");
        }
        if float {
            skip.push("toolbar");
        }
        compose_and_save(win, file, dark, &skip)
    }
}

/// `SIGNET_ACTIONS`: synthetic input on the captured window (see `drive.rs`).
#[cfg(target_os = "macos")]
async fn run_actions(cx: &AsyncApp, win: isize) {
    let Some(script) = var("SIGNET_ACTIONS") else { return };
    if let Some(w) = mac::windows().into_iter().find(|w| w.num == win) {
        mac::focus_window(win);
        sleep(cx, 400).await;
        super::drive::run(cx, &script, (0.0, 0.0), win).await;
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use std::path::Path;
    use std::process::Command;
    use std::rc::Rc;

    use image::{imageops, Rgba, RgbaImage};
    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, Bool};
    use objc2_foundation::{NSRect, NSString};
    use raw_window_handle::RawWindowHandle;

    use crate::browser::Browser;

    /// Titles of the floating surfaces (children of the main window).
    const SURFACES: [&str; 7] = ["overlay", "sidebar", "toolbar", "find", "toast", "preview", "quit"];

    pub struct W {
        pub num: isize,
        pub title: String,
        /// Frame in screen points, top-left origin.
        pub x: f64,
        pub top: f64,
        pub w: f64,
        pub h: f64,
        pub parent: isize,
        pub visible: bool,
    }

    pub fn windows() -> Vec<W> {
        let mut out = Vec::new();
        unsafe {
            let screens: *mut AnyObject = msg_send![objc2::class!(NSScreen), screens];
            let first: *mut AnyObject = msg_send![screens, firstObject];
            let primary_h = if first.is_null() {
                0.0
            } else {
                let f: NSRect = msg_send![first, frame];
                f.size.height
            };
            let app: *mut AnyObject = msg_send![objc2::class!(NSApplication), sharedApplication];
            let arr: *mut AnyObject = msg_send![app, windows];
            let n: usize = msg_send![arr, count];
            for i in 0..n {
                let w: *mut AnyObject = msg_send![arr, objectAtIndex: i];
                let num: isize = msg_send![w, windowNumber];
                let title: Option<Retained<NSString>> = msg_send![w, title];
                let f: NSRect = msg_send![w, frame];
                let parent: *mut AnyObject = msg_send![w, parentWindow];
                let pnum: isize = if parent.is_null() { 0 } else { msg_send![parent, windowNumber] };
                let vis: Bool = msg_send![w, isVisible];
                out.push(W {
                    num,
                    title: title.map(|t| t.to_string()).unwrap_or_default(),
                    x: f.origin.x,
                    top: primary_h - f.origin.y - f.size.height,
                    w: f.size.width,
                    h: f.size.height,
                    parent: pnum,
                    visible: vis.as_bool(),
                });
            }
        }
        out
    }

    /// Makes the window key (and the app active) so that synthetic input reaches it.
    pub fn focus_window(num: isize) {
        unsafe {
            let app: *mut AnyObject = msg_send![objc2::class!(NSApplication), sharedApplication];
            let _: () = msg_send![app, activateIgnoringOtherApps: Bool::YES];
            let arr: *mut AnyObject = msg_send![app, windows];
            let n: usize = msg_send![arr, count];
            for i in 0..n {
                let w: *mut AnyObject = msg_send![arr, objectAtIndex: i];
                let wn: isize = msg_send![w, windowNumber];
                if wn == num {
                    let nil: *mut AnyObject = std::ptr::null_mut();
                    let _: () = msg_send![w, makeKeyAndOrderFront: nil];
                }
            }
        }
    }

    pub fn main_window_number(b: &Rc<Browser>) -> Option<isize> {
        let raw = b.core.borrow().views.parent?;
        let RawWindowHandle::AppKit(a) = raw else { return None };
        unsafe {
            let view = a.ns_view.as_ptr() as *mut AnyObject;
            let win: *mut AnyObject = msg_send![view, window];
            if win.is_null() {
                return None;
            }
            let n: isize = msg_send![win, windowNumber];
            Some(n)
        }
    }

    pub fn is_dark() -> bool {
        match super::var("SIGNET_THEME").as_deref() {
            Some("dark") => true,
            Some("light") => false,
            _ => Command::new("defaults").args(["read", "-g", "AppleInterfaceStyle"]).output().map(|o| String::from_utf8_lossy(&o.stdout).contains("Dark")).unwrap_or(false),
        }
    }

    /// `screencapture -l<id>` (no shadow, no sound).
    fn grab(num: isize, to: &Path) -> Option<RgbaImage> {
        let st = Command::new("screencapture").args(["-x", "-o", &format!("-l{num}")]).arg(to).status().ok()?;
        if !st.success() || !to.exists() {
            eprintln!("[capture] screencapture failed for window {num}: {st}");
            return None;
        }
        let img = image::open(to).ok()?.to_rgba8();
        let _ = std::fs::remove_file(to);
        Some(img)
    }

    fn tmp_for(out: &Path, tag: &str) -> std::path::PathBuf {
        out.with_file_name(format!("sg-capture-{}-{tag}.png", std::process::id()))
    }

    /// One window, as is.
    pub fn save_window(num: isize, out: &Path, _bg: Option<Rgba<u8>>) -> bool {
        match grab(num, &tmp_for(out, "w")) {
            Some(img) => img.save(out).is_ok(),
            None => false,
        }
    }

    /// A window with its visible floating children composited on top, over the stand-in sidebar color when translucent.
    pub fn compose_and_save(num: isize, out: &Path, dark: bool, skip: &[&str]) -> bool {
        let all = windows();
        let Some(main) = all.iter().find(|w| w.num == num) else { return false };
        let Some(mut base) = grab(num, &tmp_for(out, "main")) else { return false };
        let scale = f64::from(base.width()) / main.w.max(1.0);
        // Blurred scrims (command bar, popovers) only exist on screen: SIGNET_CAPTURE_MODE=screen grabs the window's
        // screen region instead (needs the window unobstructed), which shows the real composition.
        if super::var("SIGNET_CAPTURE_MODE").as_deref() == Some("screen") {
            let r = format!("{},{},{},{}", main.x.round(), main.top.round(), main.w.round(), main.h.round());
            let tmp = tmp_for(out, "screen");
            let ok = Command::new("screencapture").args(["-x", "-o", "-R", &r]).arg(&tmp).status().map(|s| s.success()).unwrap_or(false);
            if ok && let Ok(img) = image::open(&tmp) {
                let _ = std::fs::remove_file(&tmp);
                return img.to_rgba8().save(out).is_ok();
            }
        }
        // The window is translucent (vibrancy): flatten it over the same stand-in color as the Electron harness.
        let fill = if dark { Rgba([0x2a, 0x2a, 0x2f, 255]) } else { Rgba([0xd9, 0xd9, 0xde, 255]) };
        let mut bg = RgbaImage::from_pixel(base.width(), base.height(), fill);
        imageops::overlay(&mut bg, &base, 0, 0);
        base = bg;
        for name in SURFACES {
            if skip.contains(&name) {
                continue;
            }
            for c in all.iter().filter(|w| w.parent == num && w.visible && w.title == name) {
                if let Some(img) = grab(c.num, &tmp_for(out, name)) {
                    if super::var("SIGNET_CAPTURE_DEBUG").is_some() {
                        let _ = img.save(out.with_file_name(format!("debug-{name}.png")));
                        eprintln!("[capture] child {name} {}x{} alpha(0,0)={}", img.width(), img.height(), img.get_pixel(0, 0)[3]);
                    }
                    let (dx, dy) = (((c.x - main.x) * scale).round() as i64, ((c.top - main.top) * scale).round() as i64);
                    // `screencapture -l` of an attached child window may return the whole window group: keep only the
                    // child's own rectangle (pasting the full image at the child's offset would draw a clone of the window).
                    let img = if (f64::from(img.width()) - c.w * scale).abs() > 4.0 || (f64::from(img.height()) - c.h * scale).abs() > 4.0 {
                        let (cw, ch) = (((c.w * scale).round() as u32).min(img.width()), ((c.h * scale).round() as u32).min(img.height()));
                        imageops::crop_imm(&img, dx.max(0) as u32, dy.max(0) as u32, cw, ch).to_image()
                    } else {
                        img
                    };
                    imageops::overlay(&mut base, &img, dx, dy);
                }
            }
        }
        base.save(out).is_ok()
    }
}

//! System integration: default browser, updates, proxy, relaunch, downloads folder, browsing data.
//! Port of `setDefaultBrowser` (`app/platform.ts`), `features/updates.ts`, `openProxySettings` and `relaunchApp`
//! (`features/engine.ts`), `chooseDownloadDir` and `clearData` (`windows/settings.ts`).
//!
//! Native dialogs (message boxes, folder picker) are run by the OS tools (osascript, zenity or kdialog, PowerShell)
//! on a helper thread, because GPUI prompts need an `App` context the browser core does not have. The answer comes
//! back through a queue read by the next `tick`.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use signet_core::i18n::{t, t_args};
use signet_core::model::OpenOptions;
use signet_core::types::BrowsingDataKind;
use signet_features::http::UreqHttp;
use signet_features::updates::{self, CheckResult, Outcome, Tracker};
use signet_web::{ClearData, ProfileKind};

use crate::browser::Core;
use crate::windows::Cmd;

/// Version of this build.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

// ------------------------------------------------------------------------------------------------ pure logic

/// Whether this copy runs from an installed bundle or package rather than from the build folder (updates and
/// relaunch only make sense then: in development a relaunch would detach from `cargo run`).
pub fn is_packaged_path(exe: &Path) -> bool {
    let s = exe.to_string_lossy();
    if cfg!(target_os = "macos") && s.contains(".app/Contents/MacOS") {
        return true;
    }
    !exe.components().any(|c| c.as_os_str() == "target")
}

fn is_packaged() -> bool {
    std::env::current_exe().map(|e| is_packaged_path(&e)).unwrap_or(false)
}

/// Escapes a string for an AppleScript literal.
pub fn applescript_quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Profiles that hold browsing data: the shared one and every isolated space's.
fn profiles(core: &Core) -> Vec<ProfileKind> {
    let mut p = vec![ProfileKind::persistent("default")];
    p.extend(core.state().spaces.iter().filter(|s| s.is_isolated()).map(|s| ProfileKind::persistent(format!("space-{}", s.id))));
    p
}

/// What `clear_profile_data` wipes for the cache and cookies kinds.
pub fn clear_plan(kinds: &[BrowsingDataKind]) -> Option<ClearData> {
    let cache = kinds.contains(&BrowsingDataKind::Cache);
    let cookies = kinds.contains(&BrowsingDataKind::Cookies);
    if !cache && !cookies {
        return None;
    }
    Some(ClearData { cookies, cache, http_auth: cookies, certificate_exceptions: false, connections: cookies })
}

// ------------------------------------------------------------------------------------------------ native dialogs

/// Message box with one or two buttons; the index of the chosen button, None if closed. Blocks: run it on a thread.
fn message_box(message: &str, detail: &str, buttons: &[String]) -> Option<usize> {
    #[cfg(target_os = "macos")]
    {
        // The default (rightmost) button is the first one given: the primary action.
        let order = buttons.iter().rev().map(|b| applescript_quote(b)).collect::<Vec<_>>().join(", ");
        let script = format!(
            "display dialog {} with title {} buttons {{{}}} default button {}",
            applescript_quote(detail),
            applescript_quote(message),
            order,
            applescript_quote(buttons.first()?)
        );
        let out = Command::new("osascript").args(["-e", &script]).output().ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        let got = text.split("button returned:").nth(1)?.trim();
        return buttons.iter().position(|b| b == got);
    }
    #[cfg(target_os = "windows")]
    {
        let kind = if buttons.len() > 1 { "YesNo" } else { "OK" };
        let ps = format!(
            "Add-Type -AssemblyName PresentationFramework; [System.Windows.MessageBox]::Show('{}', '{}', '{}')",
            detail.replace('\'', "''"),
            message.replace('\'', "''"),
            kind
        );
        let out = Command::new("powershell").args(["-NoProfile", "-Command", &ps]).output().ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        return match text.trim() {
            "Yes" | "OK" => Some(0),
            "No" => Some(1),
            _ => None,
        };
    }
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    {
        let text = format!("{message}\n\n{detail}");
        let zenity = if buttons.len() > 1 {
            Command::new("zenity").args(["--question", "--title", "Signet", "--text", &text, "--ok-label", &buttons[0], "--cancel-label", &buttons[1]]).status()
        } else {
            Command::new("zenity").args(["--info", "--title", "Signet", "--text", &text, "--ok-label", &buttons[0]]).status()
        };
        match zenity {
            Ok(s) => return if s.success() { Some(0) } else if buttons.len() > 1 && s.code() == Some(1) { Some(1) } else { None },
            Err(_) => {}
        }
        let kd = if buttons.len() > 1 {
            Command::new("kdialog").args(["--yesno", &text, "--title", "Signet", "--yes-label", &buttons[0], "--no-label", &buttons[1]]).status()
        } else {
            Command::new("kdialog").args(["--msgbox", &text, "--title", "Signet"]).status()
        };
        kd.ok().map(|s| if s.success() { 0 } else { 1 })
    }
}

/// Folder picker opened on `start`; None if cancelled or no tool is available. Blocks: run it on a thread.
fn pick_folder(title: &str, button: &str, start: &Path) -> Option<PathBuf> {
    let _ = button;
    let start = start.to_string_lossy().into_owned();
    #[cfg(target_os = "macos")]
    let out = {
        let script = format!(
            "POSIX path of (choose folder with prompt {} default location POSIX file {})",
            applescript_quote(title),
            applescript_quote(&start)
        );
        Command::new("osascript").args(["-e", &script]).stderr(Stdio::null()).output().ok()?
    };
    #[cfg(target_os = "windows")]
    let out = {
        let ps = format!(
            "Add-Type -AssemblyName System.Windows.Forms; $d = New-Object System.Windows.Forms.FolderBrowserDialog; $d.Description = '{}'; $d.SelectedPath = '{}'; if ($d.ShowDialog() -eq 'OK') {{ $d.SelectedPath }}",
            title.replace('\'', "''"),
            start.replace('\'', "''")
        );
        Command::new("powershell").args(["-NoProfile", "-Command", &ps]).stderr(Stdio::null()).output().ok()?
    };
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    let out = {
        let with_slash = if start.ends_with('/') { start.clone() } else { format!("{start}/") };
        match Command::new("zenity").args(["--file-selection", "--directory", "--title", title, "--filename", &with_slash]).stderr(Stdio::null()).output() {
            Ok(o) => o,
            Err(_) => Command::new("kdialog").args(["--getexistingdirectory", &start, "--title", title]).stderr(Stdio::null()).output().ok()?,
        }
    };
    if !out.status.success() {
        return None;
    }
    let picked = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!picked.is_empty()).then(|| PathBuf::from(picked))
}

// ------------------------------------------------------------------------------------------------ default browser

#[cfg(target_os = "macos")]
mod mac {
    use std::ffi::{CStr, CString, c_char, c_void};

    type CFStringRef = *const c_void;
    const UTF8: u32 = 0x0800_0100;

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFStringCreateWithCString(alloc: *const c_void, s: *const c_char, encoding: u32) -> CFStringRef;
        fn CFStringGetCString(s: CFStringRef, buf: *mut c_char, size: isize, encoding: u32) -> u8;
        fn CFRelease(r: *const c_void);
        fn CFBundleGetMainBundle() -> *const c_void;
        fn CFBundleGetIdentifier(b: *const c_void) -> CFStringRef;
    }

    #[link(name = "CoreServices", kind = "framework")]
    unsafe extern "C" {
        fn LSSetDefaultHandlerForURLScheme(scheme: CFStringRef, bundle_id: CFStringRef) -> i32;
        fn LSCopyDefaultHandlerForURLScheme(scheme: CFStringRef) -> CFStringRef;
    }

    /// Bundle id used when the binary runs outside a bundle (development).
    const FALLBACK_ID: &str = "dev.signet.app.gpui";

    fn cfstring(s: &str) -> Option<CFStringRef> {
        let c = CString::new(s).ok()?;
        // SAFETY: valid NUL-terminated string, null allocator = default.
        let r = unsafe { CFStringCreateWithCString(std::ptr::null(), c.as_ptr(), UTF8) };
        (!r.is_null()).then_some(r)
    }

    fn to_string(r: CFStringRef) -> Option<String> {
        let mut buf = [0 as c_char; 512];
        // SAFETY: `buf` is writable for its length; `r` is a live CFString.
        let ok = unsafe { CFStringGetCString(r, buf.as_mut_ptr(), buf.len() as isize, UTF8) };
        // SAFETY: CFStringGetCString NUL-terminates on success.
        (ok != 0).then(|| unsafe { CStr::from_ptr(buf.as_ptr()) }.to_string_lossy().into_owned())
    }

    /// Identifier of the running bundle.
    pub fn bundle_id() -> String {
        // SAFETY: the main bundle is owned by CoreFoundation (get rule), the identifier too.
        let id = unsafe {
            let b = CFBundleGetMainBundle();
            if b.is_null() { std::ptr::null() } else { CFBundleGetIdentifier(b) }
        };
        if id.is_null() { FALLBACK_ID.to_string() } else { to_string(id).filter(|s| !s.is_empty()).unwrap_or_else(|| FALLBACK_ID.to_string()) }
    }

    pub fn set_default(scheme: &str, bundle: &str) -> bool {
        let (Some(s), Some(b)) = (cfstring(scheme), cfstring(bundle)) else { return false };
        // SAFETY: both strings are live CFStrings, released below.
        let status = unsafe { LSSetDefaultHandlerForURLScheme(s, b) };
        // SAFETY: created above with the create rule.
        unsafe {
            CFRelease(s);
            CFRelease(b);
        }
        status == 0
    }

    pub fn default_handler(scheme: &str) -> Option<String> {
        let s = cfstring(scheme)?;
        // SAFETY: `s` is live; the result follows the copy rule and is released after reading.
        let r = unsafe { LSCopyDefaultHandlerForURLScheme(s) };
        // SAFETY: created above.
        unsafe { CFRelease(s) };
        if r.is_null() {
            return None;
        }
        let out = to_string(r);
        // SAFETY: copy rule.
        unsafe { CFRelease(r) };
        out
    }
}

/// Desktop file of the app on Linux packages.
#[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
const DESKTOP_FILE: &str = "signet.desktop";

/// Whether the app is the system's default browser (http and https).
pub fn is_default_browser() -> bool {
    #[cfg(target_os = "macos")]
    {
        let id = mac::bundle_id().to_lowercase();
        ["http", "https"].iter().all(|s| mac::default_handler(s).is_some_and(|h| h.to_lowercase() == id))
    }
    #[cfg(target_os = "windows")]
    {
        false // Windows 10+ only lets the user choose in Settings; the registration is done by the installer
    }
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    {
        Command::new("xdg-settings").args(["check", "default-web-browser", DESKTOP_FILE]).output().map(|o| String::from_utf8_lossy(&o.stdout).trim() == "yes").unwrap_or(false)
    }
}

// ------------------------------------------------------------------------------------------------ state

enum Event {
    Check(CheckResult, bool),
    /// The user chose to open this page (release page of an update).
    OpenPage(String),
    Folder(PathBuf),
}

/// State owned by this feature (a field of `Phase3`).
#[derive(Default)]
pub struct State {
    tracker: Tracker,
    events: Arc<Mutex<Vec<Event>>>,
    next_check: Option<Instant>,
    picking_folder: bool,
}

impl State {
    fn push(events: &Arc<Mutex<Vec<Event>>>, e: Event) {
        if let Ok(mut q) = events.lock() {
            q.push(e);
        }
    }
}

impl Core {
    /// Makes the app the system's default browser (http and https links).
    pub fn set_default_browser(&mut self) {
        #[cfg(target_os = "macos")]
        {
            // macOS asks the user to confirm the change itself.
            let id = mac::bundle_id();
            let ok = ["http", "https"].iter().all(|s| mac::set_default(s, &id));
            self.log(format!("set_default_browser {id}: {ok}"));
        }
        #[cfg(target_os = "windows")]
        {
            // Windows 10 and 11 protect the choice: the installer registers the app, the user picks it in Settings.
            crate::app::platform::open_external("ms-settings:defaultapps");
        }
        #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
        {
            let ok = Command::new("xdg-settings").args(["set", "default-web-browser", DESKTOP_FILE]).status().map(|s| s.success()).unwrap_or(false);
            let _ = Command::new("xdg-mime").args(["default", DESKTOP_FILE, "x-scheme-handler/http", "x-scheme-handler/https"]).status();
            self.log(format!("set_default_browser {DESKTOP_FILE}: {ok}"));
        }
    }

    /// Help > Check for Updates: every outcome is shown.
    pub fn check_for_updates(&mut self) {
        self.start_update_check(true);
    }

    fn start_update_check(&mut self, manual: bool) {
        let events = self.phase3.system.events.clone();
        if !is_packaged() {
            if manual {
                let (msg, detail) = (t("updates.devOnly"), t("updates.devOnlyDetail"));
                let ok = vec![t("updates.ok")];
                std::thread::spawn(move || {
                    message_box(&msg, &detail, &ok);
                });
            }
            return;
        }
        if !self.phase3.system.tracker.begin() {
            return;
        }
        std::thread::spawn(move || {
            let result = updates::check(&UreqHttp, VERSION);
            State::push(&events, Event::Check(result, manual));
        });
    }

    /// Shows what a finished check found.
    fn finish_update_check(&mut self, result: CheckResult, manual: bool) {
        let events = self.phase3.system.events.clone();
        match self.phase3.system.tracker.finish(result, manual) {
            Outcome::Nothing => {}
            Outcome::ShowUpToDate => {
                let (msg, detail) = (t("updates.upToDate"), t_args("updates.upToDateDetail", &[("version", VERSION)]));
                let ok = vec![t("updates.ok")];
                std::thread::spawn(move || {
                    message_box(&msg, &detail, &ok);
                });
            }
            Outcome::ShowFailed => {
                let (msg, detail) = (t("updates.failed"), t("updates.failedDetail"));
                let ok = vec![t("updates.ok")];
                std::thread::spawn(move || {
                    message_box(&msg, &detail, &ok);
                });
            }
            Outcome::Offer(rel) => {
                // The new version is downloaded by hand from its release page (no in-app installer yet).
                let msg = t_args("updates.available", &[("version", &rel.version)]);
                let detail = t_args("updates.availableDetail", &[("current", VERSION)]);
                let buttons = vec![t("updates.download"), t("updates.later")];
                let url = rel.url;
                std::thread::spawn(move || {
                    if message_box(&msg, &detail, &buttons) == Some(0) {
                        State::push(&events, Event::OpenPage(url));
                    }
                });
            }
        }
    }

    /// Restarts the app (graphics acceleration and similar settings apply at launch). Packaged app only.
    pub fn relaunch_app(&mut self) {
        let Ok(exe) = std::env::current_exe() else { return };
        if !is_packaged_path(&exe) {
            self.log("relaunch_app: not packaged, ignored");
            return;
        }
        // An AppImage must be started again through its own path, not the mounted executable.
        let exe = std::env::var("APPIMAGE").ok().filter(|a| !a.is_empty()).map(PathBuf::from).unwrap_or(exe);
        let args: Vec<String> = std::env::args().skip(1).collect();
        let argv = crate::phase3::backup::relaunch_argv(&exe, &args, cfg!(target_os = "windows"));
        let Some((prog, rest)) = argv.split_first() else { return };
        if Command::new(prog).args(rest).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().is_ok() {
            self.send(Cmd::Quit);
        }
    }

    /// Opens the operating system's proxy settings.
    pub fn open_proxy_settings(&mut self) {
        #[cfg(target_os = "macos")]
        crate::app::platform::open_external("x-apple.systempreferences:com.apple.Network-Settings.extension");
        #[cfg(target_os = "windows")]
        crate::app::platform::open_external("ms-settings:network-proxy");
        #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
        {
            // First settings screen found that has a proxy page (GNOME, KDE, Cinnamon, XFCE has none).
            const TOOLS: [(&str, &[&str]); 4] = [
                ("gnome-control-center", &["network"]),
                ("systemsettings", &["kcm_proxy"]),
                ("cinnamon-settings", &["network"]),
                ("unity-control-center", &["network"]),
            ];
            for (cmd, args) in TOOLS {
                if Command::new(cmd).args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().is_ok() {
                    return;
                }
            }
            self.log("open_proxy_settings: no settings tool found");
        }
    }

    /// "Choose a folder" dialog opened on the current downloads folder; the choice becomes the downloads folder.
    pub fn choose_download_dir(&mut self) {
        if self.phase3.system.picking_folder {
            return;
        }
        self.phase3.system.picking_folder = true;
        let events = self.phase3.system.events.clone();
        let start = self.download_dir();
        let (title, button) = (t("downloads.chooseDir"), t("downloads.chooseDirButton"));
        std::thread::spawn(move || match pick_folder(&title, &button, &start) {
            Some(dir) => State::push(&events, Event::Folder(dir)),
            None => State::push(&events, Event::Folder(PathBuf::new())),
        });
    }

    /// Erases the chosen kinds of browsing data. Cache and cookies are wiped from every profile.
    pub fn clear_browsing_data(&mut self, kinds: &[BrowsingDataKind]) {
        if kinds.contains(&BrowsingDataKind::History) {
            self.history.clear();
        }
        if kinds.contains(&BrowsingDataKind::Archive) {
            self.store.data.state.archive.clear();
        }
        if kinds.contains(&BrowsingDataKind::Downloads) {
            self.clear_downloads();
        }
        if let Some(plan) = clear_plan(kinds) {
            for p in profiles(self) {
                signet_web::clear_profile_data(&p, plan, || {});
            }
            if plan.cookies {
                // Web storage has no profile-wide call: wipe the origin of every open page.
                let mut origins: Vec<(String, String)> = Vec::new();
                for (id, tab) in &self.state().tabs {
                    if let Ok(u) = url::Url::parse(&tab.url)
                        && matches!(u.scheme(), "http" | "https")
                    {
                        origins.push((id.clone(), u.origin().ascii_serialization()));
                    }
                }
                for (id, origin) in origins {
                    if let Some(v) = self.views.view_of(&id) {
                        v.clear_site_data(&origin, |_| {});
                    }
                }
            }
        }
        if kinds.contains(&BrowsingDataKind::Permissions) {
            self.store.data.state.site_permissions.clear();
        }
        self.store.schedule(signet_core::core::store::SAVE_DELAY);
        self.changed();
    }
}

// ------------------------------------------------------------------------------------------------ hooks

/// Called once after the browser core exists and the web layer is initialized.
pub(super) fn startup(core: &mut Core) {
    schedule_checks(core);
}

/// Called for every CEF event, before the core handles it.
pub(super) fn web_event(_core: &mut Core, _ev: &signet_web::WebEvent) {}

/// Automatic checks: a little after launch, then every 6 hours, following the setting. Packaged app only.
fn schedule_checks(core: &mut Core) {
    let on = core.state().settings.check_updates != Some(false) && is_packaged();
    let st = &mut core.phase3.system;
    match (on, st.next_check) {
        (true, None) => st.next_check = Some(Instant::now() + updates::FIRST_CHECK),
        (false, Some(_)) => st.next_check = None,
        _ => {}
    }
}

/// Called on every core tick (about once per second or at the next store deadline).
pub(super) fn tick(core: &mut Core) {
    schedule_checks(core);
    if core.phase3.system.next_check.is_some_and(|t| Instant::now() >= t) {
        core.phase3.system.next_check = Some(Instant::now() + updates::CHECK_EVERY);
        core.start_update_check(false);
    }
    let events: Vec<Event> = core.phase3.system.events.lock().map(|mut q| std::mem::take(&mut *q)).unwrap_or_default();
    for e in events {
        match e {
            Event::Check(result, manual) => core.finish_update_check(result, manual),
            Event::OpenPage(url) => {
                core.open_tab(&url, OpenOptions { activate: true, after_tab_id: None, opener_id: None });
                core.send(Cmd::ShowMain);
            }
            Event::Folder(dir) => {
                core.phase3.system.picking_folder = false;
                if !dir.as_os_str().is_empty() {
                    core.store.data.state.settings.download_dir = Some(dir.to_string_lossy().into_owned());
                    core.changed();
                }
            }
        }
    }
}

/// Called before quitting (flush files, stop threads).
pub(super) fn quit(_core: &mut Core) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packaged_detection() {
        assert!(!is_packaged_path(Path::new("/home/me/Signet/app/desktop-rs/target/debug/signet")));
        assert!(is_packaged_path(Path::new("/opt/Signet/signet")));
        if cfg!(target_os = "macos") {
            assert!(is_packaged_path(Path::new("/Applications/Signet.app/Contents/MacOS/signet")));
        }
    }

    #[test]
    fn shell_words_are_quoted() {
        assert_eq!(applescript_quote("say \"hi\" \\"), "\"say \\\"hi\\\" \\\\\"");
    }

    #[test]
    fn clear_plan_covers_cache_and_cookies() {
        assert_eq!(clear_plan(&[BrowsingDataKind::History, BrowsingDataKind::Permissions]), None);
        let c = clear_plan(&[BrowsingDataKind::Cache]).unwrap();
        assert!(c.cache && !c.cookies && !c.http_auth && !c.connections);
        let c = clear_plan(&[BrowsingDataKind::Cookies]).unwrap();
        assert!(!c.cache && c.cookies && c.http_auth && c.connections && !c.certificate_exceptions);
        let c = clear_plan(&[BrowsingDataKind::Cache, BrowsingDataKind::Cookies]).unwrap();
        assert!(c.cache && c.cookies);
    }
}

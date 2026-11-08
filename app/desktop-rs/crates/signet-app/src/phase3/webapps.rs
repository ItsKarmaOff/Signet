//! PHASE3: installed sites (`signet_features::webapps`), port of `main/features/webapps.ts`.
//!
//! - Registry: `webapps.json` through `signet_features::webapps::WebApps`; icons in `<data>/webapps/<id>.png` (1024 px
//!   master from the install card) and `<id>.64.png` (bar and Settings, made with `sips` on macOS).
//! - Install: "Install as app" (`prompt_install_web_app`, called by the menu command router) reads the page with
//!   `PAGE_SCRIPT` (its result comes back on the page channel `webapp:page`, read here in `web_event`), builds the
//!   proposal on a worker thread (`inspect`: manifest and icons over HTTP), then shows the `webapp` overlay. The
//!   card's answer (`install_web_app`) saves the app, opens its window and writes the launcher (worker thread).
//! - Launchers: macOS `.app` shim (Info.plist, `launcher` script, icns through `sips` and `iconutil`, `lsregister`)
//!   in `~/Applications/Signet Apps`; Linux `.desktop` file in `~/.local/share/applications`; Windows `.url` shortcut
//!   in the Start Menu (no icon: `.url` needs an `.ico`). They hand `signet://webapp/<id>` (packaged macOS app, through
//!   `open -b`) or `--signet-webapp=<id>` (development) to the running app: see `launch_web_app_from`.
//! - Windows: an app window is an auxiliary window (id from `WEBAPP_ID_BASE`) whose manager reports creation, size,
//!   focus and closing through the `web_app_window_*` methods; the page view is a child of that window at
//!   `x=0, y=38, w=W, h=H-38` (bar of the `WebApp` screen). The window kind itself (`AuxKind::WebApp`) is requested
//!   in `request_window`, the only seam left (see the report). Events of these views are read here, not by the core.

// Entry points called by modules or windows that are wired by other agents (see the report).
#![allow(dead_code)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use raw_window_handle::RawWindowHandle;
use signet_core::api::UiEvent;
use signet_core::model::OpenOptions;
use signet_core::types::{OverlayPayload, Rect, WebApp, WebAppInfo, WebAppSpace, WebAppState, WebAppsInfo, Zone};
use signet_features::http::UreqHttp;
use signet_features::webapps::{self as feat, PageInfo, Proposal, TabFacts, WebApps, WEBAPP_BAR};
use signet_web::{Bounds, Disposition, ProfileKind, ViewId, ViewOptions, WebEvent, WebView};

use crate::browser::Core;
use crate::windows::Cmd;

/// Auxiliary window ids of the app windows start here, far from the ones `phase3/windows.rs` hands out.
pub const WEBAPP_ID_BASE: u64 = 1 << 40;
/// Time to wait for a page that is still loading, then for the script's answer.
const LOAD_WAIT: Duration = Duration::from_secs(3);
const ANSWER_WAIT: Duration = Duration::from_secs(6);

/// An open app window.
struct AppWin {
    id: u64,
    app_id: String,
    start_url: String,
    raw: Option<RawWindowHandle>,
    size: (f64, f64),
    view: Option<WebView>,
    url: String,
    title: String,
    loading: bool,
    can_go_back: bool,
    /// Last state sent to the bar (JSON).
    last: String,
}

/// What the page inspection waits for.
struct Inspecting {
    seq: u64,
    tab: String,
    view: ViewId,
    started: Instant,
    script_sent: bool,
    spawned: bool,
}

/// A proposal shown in the install card.
struct Pending {
    token: String,
    tab_id: String,
    proposal: Proposal,
}

enum Inbound {
    Proposal(u64, Proposal),
    /// A launcher was written or removed: the Settings list must be re-read.
    LaunchersChanged,
}

/// State owned by this feature (a field of `Phase3`).
#[derive(Default)]
pub struct State {
    apps: Option<WebApps>,
    windows: Vec<AppWin>,
    next_id: u64,
    focused: Option<u64>,
    seq: u64,
    inspecting: Option<Inspecting>,
    pending: Option<Pending>,
    inbox: Arc<Mutex<Vec<Inbound>>>,
    small: HashMap<String, String>,
    bounds_dirty: bool,
}

// ---------------------------------------------------------------------------------------------- pure logic

fn is_http(url: &str) -> bool {
    let l = url.to_ascii_lowercase();
    l.starts_with("http://") || l.starts_with("https://")
}

/// Frame of the page view in an app window of size `size`.
pub(crate) fn page_bounds(size: (f64, f64)) -> Bounds {
    let bar = f64::from(WEBAPP_BAR);
    Bounds::new(0.0, bar as f32, size.0 as f32, (size.1 - bar).max(0.0) as f32)
}

/// Bounds worth remembering (a window that was not squeezed to nothing).
pub(crate) fn valid_bounds(r: &Rect) -> bool {
    [r.x, r.y, r.width, r.height].iter().all(|n| n.is_finite()) && r.width >= 200.0 && r.height >= 150.0
}

/// Script that posts the page description on `webapp:page`, tagged with the request number.
pub(crate) fn page_js(seq: u64) -> String {
    format!("window.__signetPost('webapp:page',JSON.stringify({{seq:{seq},info:{}}}))", feat::PAGE_SCRIPT.trim())
}

/// Decodes the PNG data URL sent by the install card (bounded).
pub(crate) fn decode_png_data_url(url: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    let rest = url.strip_prefix("data:image/png;base64,")?;
    if url.len() >= 16_000_000 {
        return None;
    }
    let bytes = base64::engine::general_purpose::STANDARD.decode(rest.trim()).ok()?;
    bytes.starts_with(&[0x89, b'P', b'N', b'G']).then_some(bytes)
}

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_default()
}

/// Folder of the launchers. `SIGNET_WEBAPPS_DIR` overrides it (development checks).
pub(crate) fn launchers_dir() -> PathBuf {
    let env = std::env::var("SIGNET_WEBAPPS_DIR").ok();
    if let Some(p) = env.as_deref().filter(|e| !e.is_empty()) {
        return PathBuf::from(p);
    }
    #[cfg(target_os = "macos")]
    {
        feat::launchers_folder(&home(), None)
    }
    #[cfg(target_os = "windows")]
    {
        dirs::data_dir().unwrap_or_else(home).join("Microsoft").join("Windows").join("Start Menu").join("Programs").join("Signet Apps")
    }
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    {
        dirs::data_dir().unwrap_or_else(|| home().join(".local").join("share")).join("applications")
    }
}

/// Launcher path of an app whose launcher name is `bundle`.
pub(crate) fn launcher_path(dir: &Path, bundle: &str, id: &str) -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        let _ = id;
        dir.join(format!("{bundle}.app"))
    }
    #[cfg(target_os = "windows")]
    {
        let _ = id;
        dir.join(format!("{bundle}.url"))
    }
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    {
        let _ = bundle;
        dir.join(format!("{}{id}.desktop", feat::BUNDLE_PREFIX))
    }
}

/// True if the launcher at `path` is the one of app `id` (an app that is not ours is never touched).
pub(crate) fn owns_launcher(path: &Path, id: &str) -> bool {
    #[cfg(target_os = "macos")]
    {
        std::fs::read_to_string(path.join("Contents").join("Info.plist")).is_ok_and(|t| feat::plist_belongs_to(&t, id))
    }
    #[cfg(target_os = "windows")]
    {
        std::fs::read_to_string(path).is_ok_and(|t| t.contains(&format!("URL=signet://webapp/{id}")))
    }
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    {
        std::fs::read_to_string(path).is_ok_and(|t| t.contains(&format!("X-Signet-WebApp={id}")))
    }
}

fn remove_path(p: &Path) {
    if p.is_dir() {
        let _ = std::fs::remove_dir_all(p);
    } else {
        let _ = std::fs::remove_file(p);
    }
}

/// Resizes a PNG with `sips` (macOS). Other platforms have no resizer without an image crate: `false`.
fn resize_png(src: &Path, dst: &Path, size: u32) -> bool {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("/usr/bin/sips")
            .args(["-z", &size.to_string(), &size.to_string()])
            .arg(src)
            .arg("--out")
            .arg(dst)
            .output()
            .is_ok_and(|o| o.status.success())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (src, dst, size);
        false
    }
}

/// Writes (or rewrites) the launcher of an app. Blocking: run it off the main thread.
pub(crate) fn write_launcher(a: &WebApp, dir: &Path, icon: Option<&Path>) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let final_path = launcher_path(dir, &a.bundle, &a.id);
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe_s = exe.to_string_lossy().into_owned();
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::fs::PermissionsExt;
        let tmp = dir.join(format!(".{}.tmp.app", a.id));
        let _ = std::fs::remove_dir_all(&tmp);
        let macos = tmp.join("Contents").join("MacOS");
        let res = tmp.join("Contents").join("Resources");
        std::fs::create_dir_all(&macos).map_err(|e| e.to_string())?;
        std::fs::create_dir_all(&res).map_err(|e| e.to_string())?;
        std::fs::write(tmp.join("Contents").join("Info.plist"), feat::plist(a)).map_err(|e| e.to_string())?;
        std::fs::write(tmp.join("Contents").join("PkgInfo"), "APPL????").map_err(|e| e.to_string())?;
        let packaged = exe_s.contains(".app/Contents/MacOS/");
        let script = if packaged {
            feat::mac_script(a, None)
        } else {
            // Development: the launcher starts the binary, which hands over to the open instance.
            format!("#!/bin/sh\n# Signet app launcher ({}).\n{} {} >/dev/null 2>&1 &\n", a.name.replace(['\r', '\n'], " "), feat::shq(&exe_s), feat::shq(&format!("{}{}", feat::LAUNCH_ARG, a.id)))
        };
        let script_path = macos.join("launcher");
        std::fs::write(&script_path, script).map_err(|e| e.to_string())?;
        std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
        if let Some(png) = icon {
            let _ = write_icns(png, &res.join("app.icns"));
        }
        if final_path.exists() {
            if !owns_launcher(&final_path, &a.id) {
                let _ = std::fs::remove_dir_all(&tmp);
                return Err(format!("{} already exists", final_path.display()));
            }
            let _ = std::fs::remove_dir_all(&final_path);
        }
        std::fs::rename(&tmp, &final_path).map_err(|e| e.to_string())?;
        let lsregister = "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister";
        let _ = std::process::Command::new(lsregister).arg("-f").arg(&final_path).output();
        Ok(())
    }
    #[cfg(target_os = "windows")]
    {
        let _ = (&exe_s, icon);
        if final_path.exists() && !owns_launcher(&final_path, &a.id) {
            return Err(format!("{} already exists", final_path.display()));
        }
        std::fs::write(&final_path, feat::windows_url_shortcut(a, None)).map_err(|e| e.to_string())
    }
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    {
        if final_path.exists() && !owns_launcher(&final_path, &a.id) {
            return Err(format!("{} already exists", final_path.display()));
        }
        let icon_s = icon.map(|p| p.to_string_lossy().into_owned());
        std::fs::write(&final_path, feat::desktop_entry(a, &exe_s, icon_s.as_deref())).map_err(|e| e.to_string())
    }
}

/// `.icns` from the 1024 px master: an iconset made with `sips`, then `iconutil`.
#[cfg(target_os = "macos")]
fn write_icns(master: &Path, dest: &Path) -> Result<(), String> {
    let tmp = std::env::temp_dir().join(format!("signet-icon-{}", std::process::id() as u64 ^ signet_core::ids::now_ms() as u64));
    let set = tmp.join("app.iconset");
    std::fs::create_dir_all(&set).map_err(|e| e.to_string())?;
    let sizes: [(&str, u32); 10] = [("16x16", 16), ("16x16@2x", 32), ("32x32", 32), ("32x32@2x", 64), ("128x128", 128), ("128x128@2x", 256), ("256x256", 256), ("256x256@2x", 512), ("512x512", 512), ("512x512@2x", 1024)];
    for (name, px) in sizes {
        resize_png(master, &set.join(format!("icon_{name}.png")), px);
    }
    let ok = std::process::Command::new("/usr/bin/iconutil").args(["-c", "icns"]).arg(&set).arg("-o").arg(dest).output().is_ok_and(|o| o.status.success());
    let _ = std::fs::remove_dir_all(&tmp);
    if ok { Ok(()) } else { Err("iconutil failed".into()) }
}

// ---------------------------------------------------------------------------------------------- core side

impl Core {
    fn apps_ref(&self) -> Option<&WebApps> {
        self.phase3.webapps.apps.as_ref()
    }

    fn icon_dir(&self) -> PathBuf {
        self.data_dir.join("webapps")
    }

    fn notify_web_apps(&self) {
        self.emit(UiEvent::WebAppsChanged);
    }

    /// 64 px icon (data URL) for the bar and Settings; empty if the app has none.
    fn small_icon(&mut self, id: &str) -> String {
        use base64::Engine;
        if let Some(u) = self.phase3.webapps.small.get(id) {
            return u.clone();
        }
        let dir = self.icon_dir();
        let small = dir.join(format!("{id}.64.png"));
        let bytes = std::fs::read(&small).ok().or_else(|| std::fs::read(dir.join(format!("{id}.png"))).ok().filter(|b| b.len() < 300_000));
        let url = bytes.map(|b| format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(b))).unwrap_or_default();
        self.phase3.webapps.small.insert(id.to_string(), url.clone());
        url
    }

    // ------------------------------------------------------------------ queries

    /// Installed apps for the command bar: `(id, name)`.
    pub fn web_app_names(&self) -> Vec<(String, String)> {
        self.apps_ref().map(WebApps::names).unwrap_or_default()
    }

    /// Installed app whose site contains this address, for "Open in app": `(id, name)`.
    pub fn web_app_for(&self, url: &str) -> Option<(String, String)> {
        self.apps_ref()?.app_for(url).map(|a| (a.id.clone(), a.name.clone()))
    }

    pub fn list_web_apps(&mut self) -> WebAppsInfo {
        let Some(apps) = self.apps_ref() else { return WebAppsInfo::default() };
        let apps: Vec<WebApp> = apps.apps.clone();
        let dir = launchers_dir();
        let spaces = self.state().spaces.clone();
        let mut out = Vec::new();
        for a in apps {
            let space = a.space_id.as_ref().and_then(|sid| spaces.iter().find(|s| &s.id == sid)).filter(|s| s.is_isolated()).map(|s| WebAppSpace { name: s.name.clone(), icon: s.icon.clone() });
            let windows = self.phase3.webapps.windows.iter().filter(|w| w.app_id == a.id).count() as u32;
            out.push(WebAppInfo {
                icon: self.small_icon(&a.id),
                launcher: launcher_path(&dir, &a.bundle, &a.id).exists(),
                id: a.id,
                name: a.name,
                start_url: a.start_url,
                space,
                windows,
            });
        }
        let h = home().to_string_lossy().into_owned();
        let shown = dir.to_string_lossy().into_owned();
        let folder = if !h.is_empty() && shown.starts_with(&h) { shown.replacen(&h, "~", 1) } else { shown };
        WebAppsInfo { apps: out, folder }
    }

    // ------------------------------------------------------------------ launching

    /// Launcher, Settings, command bar: the app's window brought to the front, or a new one on the start address.
    pub fn open_web_app(&mut self, id: &str) {
        let Some(a) = self.apps_ref().and_then(|x| x.find(id)).cloned() else {
            self.send(Cmd::ShowMain);
            self.log(format!("web app {id} is not installed"));
            return;
        };
        if let Some(w) = self.phase3.webapps.windows.iter().rev().find(|w| w.app_id == id) {
            self.send(Cmd::FocusAux(w.id));
            return;
        }
        self.open_web_app_window(&a, &a.start_url);
    }

    /// A received launch address (`signet://webapp/<id>` or `--signet-webapp=<id>`); false if it is not one.
    pub fn launch_web_app_from(&mut self, arg: &str) -> bool {
        let Some(id) = feat::parse_launch(arg) else { return false };
        self.open_web_app(&id);
        true
    }

    /// Opens a tab's page in an installed app; a day tab closes (the page changed window).
    pub fn move_tab_to_web_app(&mut self, tab_id: &str, app_id: &str) {
        let Some(tab_url) = self.state().tabs.get(tab_id).map(|t| t.url.clone()) else { return };
        let Some(a) = self.apps_ref().and_then(|x| x.find(app_id)).cloned() else { return };
        let url = if feat::in_scope(&a, &tab_url) { tab_url } else { a.start_url.clone() };
        self.open_web_app_window(&a, &url);
        if self.zone_of(tab_id) == Some(Zone::Today) {
            self.close_tab(tab_id);
        }
    }

    fn open_web_app_window(&mut self, a: &WebApp, url: &str) {
        let ws = &mut self.phase3.webapps;
        ws.next_id += 1;
        let id = WEBAPP_ID_BASE + ws.next_id;
        ws.windows.push(AppWin { id, app_id: a.id.clone(), start_url: url.to_string(), raw: None, size: (0.0, 0.0), view: None, url: url.to_string(), title: String::new(), loading: true, can_go_back: false, last: String::new() });
        ws.focused = Some(id);
        let cascade = ws.windows.iter().rev().skip(1).find(|w| w.app_id == a.id).map(|_| true).unwrap_or(false);
        let bounds = a.bounds.filter(valid_bounds);
        self.request_window(id, &a.name, bounds, cascade);
        self.notify_web_apps();
    }

    /// The one seam with the window manager: it opens an auxiliary window `id` showing
    /// `signet_ui::screens::windows::webapp::WebApp` (1060x760 or `bounds`, minimum 380x300, title `title`, traffic
    /// lights at (14, 12), cascade 24 px when `cascade`), then calls the `web_app_window_*` methods below.
    fn request_window(&mut self, id: u64, title: &str, bounds: Option<Rect>, cascade: bool) {
        let bounds = bounds.map(|b| (b.x as f32, b.y as f32, b.width as f32, b.height as f32));
        self.send(Cmd::OpenAux { id, kind: crate::windows::aux::AuxKind::WebApp { title: title.to_string(), bounds, cascade } });
    }

    // ------------------------------------------------------------------ window callbacks (window manager)

    /// True if the auxiliary window `id` is an app window (route the `on_aux_*` callbacks here).
    pub fn web_app_owns_window(&self, id: u64) -> bool {
        self.phase3.webapps.windows.iter().any(|w| w.id == id)
    }

    /// The window exists: the page view is created as its child.
    pub fn web_app_window_created(&mut self, id: u64, raw: Option<RawWindowHandle>, size: (f64, f64)) {
        let profile = {
            let Some(w) = self.phase3.webapps.windows.iter().find(|w| w.id == id) else { return };
            let app = self.apps_ref().and_then(|x| x.find(&w.app_id));
            match app.and_then(|a| a.space_id.as_ref()).and_then(|sid| self.state().spaces.iter().find(|s| &s.id == sid)).filter(|s| s.is_isolated()) {
                Some(s) => ProfileKind::persistent(format!("space-{}", s.id)),
                None => ProfileKind::persistent("default"),
            }
        };
        let Some(w) = self.phase3.webapps.windows.iter_mut().find(|w| w.id == id) else { return };
        w.raw = raw;
        w.size = size;
        if let Some(parent) = raw {
            let opts = ViewOptions { profile, url: w.start_url.clone(), bounds: page_bounds(size), visible: true, transparent_background: false };
            match WebView::create(parent, opts) {
                Ok(v) => w.view = Some(v),
                Err(e) => eprintln!("[webapp] cannot create the view of window {id}: {e}"),
            }
        }
        self.phase3.webapps.focused = Some(id);
        self.web_app_push(id, true);
    }

    pub fn web_app_window_resized(&mut self, id: u64, size: (f64, f64)) {
        let Some(w) = self.phase3.webapps.windows.iter_mut().find(|w| w.id == id) else { return };
        w.size = size;
        if let Some(v) = w.view {
            v.set_bounds(page_bounds(size));
        }
    }

    /// Frame of the window on screen (size and position), kept for the next launch.
    pub fn web_app_window_bounds(&mut self, id: u64, r: Rect) {
        if !valid_bounds(&r) {
            return;
        }
        let Some(app_id) = self.phase3.webapps.windows.iter().find(|w| w.id == id).map(|w| w.app_id.clone()) else { return };
        if let Some(a) = self.phase3.webapps.apps.as_mut().and_then(|x| x.apps.iter_mut().find(|a| a.id == app_id)) {
            a.bounds = Some(r);
            self.phase3.webapps.bounds_dirty = true;
        }
    }

    pub fn web_app_window_active(&mut self, id: u64, active: bool) {
        if !active || !self.web_app_owns_window(id) {
            return;
        }
        self.phase3.webapps.focused = Some(id);
        if let Some(v) = self.phase3.webapps.windows.iter().find(|w| w.id == id).and_then(|w| w.view) {
            v.focus();
        }
        self.web_app_push(id, true);
    }

    pub fn web_app_window_closed(&mut self, id: u64) {
        let ws = &mut self.phase3.webapps;
        let Some(ix) = ws.windows.iter().position(|w| w.id == id) else { return };
        let w = ws.windows.remove(ix);
        if let Some(v) = w.view {
            v.close();
        }
        if ws.focused == Some(id) {
            ws.focused = ws.windows.last().map(|w| w.id);
        }
        self.notify_web_apps();
    }

    // ------------------------------------------------------------------ bar of an app window (SignetApi)

    fn focused_web_app(&self) -> Option<usize> {
        let ws = &self.phase3.webapps;
        ws.focused.and_then(|f| ws.windows.iter().position(|w| w.id == f)).or_else(|| ws.windows.len().checked_sub(1))
    }

    /// State of the bar. `force`: send even if unchanged.
    fn web_app_push(&mut self, id: u64, force: bool) {
        let hue = self.hue();
        let Some(ix) = self.phase3.webapps.windows.iter().position(|w| w.id == id) else { return };
        let app_id = self.phase3.webapps.windows[ix].app_id.clone();
        let Some(a) = self.apps_ref().and_then(|x| x.find(&app_id)).cloned() else { return };
        let icon = self.small_icon(&a.id);
        let w = &mut self.phase3.webapps.windows[ix];
        let st = WebAppState {
            app_id: a.id.clone(),
            name: a.name.clone(),
            icon,
            url: w.url.clone(),
            title: w.title.clone(),
            theme_color: a.theme_color.clone(),
            loading: w.loading,
            can_go_back: w.can_go_back,
            in_scope: feat::in_scope(&a, &w.url),
            hue,
        };
        let json = serde_json::to_string(&st).unwrap_or_default();
        if !force && w.last == json {
            return;
        }
        w.last = json;
        // The bars share one event stream: only the focused window's state is sent (see the report).
        if self.focused_web_app().and_then(|f| self.phase3.webapps.windows.get(f)).is_some_and(|f| f.id == id) {
            self.emit(UiEvent::WebAppState(st));
        }
    }

    /// The bar subscribed: send it the state of the focused window.
    pub fn web_app_ready(&mut self) {
        if let Some(id) = self.focused_web_app().map(|i| self.phase3.webapps.windows[i].id) {
            self.web_app_push(id, true);
        }
    }

    pub fn web_app_home(&mut self) {
        let Some(ix) = self.focused_web_app() else { return };
        let w = &self.phase3.webapps.windows[ix];
        let start = self.apps_ref().and_then(|x| x.find(&w.app_id)).map(|a| a.start_url.clone());
        if let (Some(v), Some(u)) = (w.view, start) {
            v.load_url(&u);
        }
    }

    pub fn web_app_copy_url(&mut self) {
        if let Some(ix) = self.focused_web_app() {
            let url = self.phase3.webapps.windows[ix].url.clone();
            crate::app::platform::copy_text(&url);
        }
    }

    /// "Open in Signet": the page moves to a tab of the main window (it is reloaded: a CEF view cannot change
    /// window), the app window closes.
    pub fn web_app_open_in_main(&mut self) {
        let Some(ix) = self.focused_web_app() else { return };
        let (id, url) = {
            let w = &self.phase3.webapps.windows[ix];
            (w.id, w.url.clone())
        };
        self.send(Cmd::ShowMain);
        self.open_tab(&url, OpenOptions { activate: true, after_tab_id: None, opener_id: None });
        self.send(Cmd::CloseAux(id));
        self.web_app_window_closed(id);
    }

    // ------------------------------------------------------------------ install

    /// "Install as app...": reads the page's manifest and icons, then offers the installation (`webapp` overlay).
    /// If an app already covers the page, the page goes there directly.
    pub fn prompt_install_web_app(&mut self, tab: Option<&str>) {
        let Some(id) = tab.map(str::to_string).or_else(|| self.active_tab_id()) else { return };
        let Some(url) = self.state().tabs.get(&id).map(|t| t.url.clone()) else { return };
        if !is_http(&url) {
            self.log("install as app: the page is not a website");
            return;
        }
        if let Some((app_id, _)) = self.web_app_for(&url) {
            return self.move_tab_to_web_app(&id, &app_id);
        }
        let Some(view) = self.views.view_of(&id) else { return };
        self.send(Cmd::ShowMain);
        let displayed = signet_core::model::displayed_tab_ids(self.state(), self.active_space());
        if !displayed.contains(&id) {
            self.activate_tab(&id);
        }
        let ws = &mut self.phase3.webapps;
        ws.seq += 1;
        ws.inspecting = Some(Inspecting { seq: ws.seq, tab: id, view: view.id(), started: Instant::now(), script_sent: false, spawned: false });
        self.web_app_advance_inspect();
    }

    /// Sends the page script once the page has loaded (or after 3 s), gives up after 6 s.
    fn web_app_advance_inspect(&mut self) {
        let Some(i) = self.phase3.webapps.inspecting.as_ref() else { return };
        let (seq, tab, elapsed, sent, spawned) = (i.seq, i.tab.clone(), i.started.elapsed(), i.script_sent, i.spawned);
        let Some(view) = self.views.view_of(&tab) else {
            self.phase3.webapps.inspecting = None;
            return;
        };
        if spawned {
            return;
        }
        if !sent {
            if !view.is_loading() || elapsed > LOAD_WAIT {
                view.execute_js(&page_js(seq));
                if let Some(i) = self.phase3.webapps.inspecting.as_mut() {
                    i.script_sent = true;
                }
            }
        } else if elapsed > ANSWER_WAIT {
            self.web_app_spawn_inspect(None);
        }
    }

    /// Builds the proposal on a worker thread from what the page said (or nothing).
    fn web_app_spawn_inspect(&mut self, page: Option<PageInfo>) {
        let Some(i) = self.phase3.webapps.inspecting.as_mut() else { return };
        i.spawned = true;
        let (seq, tab_id) = (i.seq, i.tab.clone());
        let Some(tab) = self.state().tabs.get(&tab_id).cloned() else { return };
        let live = self.views.view_of(&tab_id).map(|v| v.url()).filter(|u| is_http(u));
        let page_url = live.unwrap_or_else(|| tab.url.clone());
        let space_id = signet_core::model::locate(self.state(), &tab_id).and_then(|l| l.space).and_then(|ix| self.state().spaces.get(ix)).filter(|s| s.is_isolated()).map(|s| s.id.clone());
        let inbox = self.phase3.webapps.inbox.clone();
        std::thread::spawn(move || {
            let facts = TabFacts { page_url: &page_url, title: &tab.title, favicon: tab.favicon.as_deref(), space_id: space_id.as_deref() };
            let proposal = feat::inspect(&UreqHttp, &facts, page.as_ref());
            if let Ok(mut q) = inbox.lock() {
                q.push(Inbound::Proposal(seq, proposal));
            }
        });
    }

    /// A proposal is ready: show the card over the tab.
    fn web_app_show_card(&mut self, seq: u64, proposal: Proposal) {
        let Some(tab_id) = self.phase3.webapps.inspecting.as_ref().filter(|i| i.seq == seq).map(|i| i.tab.clone()) else { return };
        self.phase3.webapps.inspecting = None;
        if !self.state().tabs.contains_key(&tab_id) {
            return;
        }
        let token = signet_core::ids::new_id();
        let card = self.panes.iter().find(|p| p.tab_id == tab_id).map(|p| p.card.clone()).unwrap_or_else(|| self.content_area.clone());
        let payload = OverlayPayload::Webapp { token: token.clone(), name: proposal.name.clone(), url: proposal.start_url.clone(), icon: proposal.icon.clone(), maskable: proposal.maskable, hue: self.hue(), card };
        self.phase3.webapps.pending = Some(Pending { token, tab_id, proposal });
        self.ov.payload = Some(payload);
        self.ov.leaving = false;
        self.changed_view();
        self.emit_overlay();
    }

    /// Answer of the install card: saves the app, opens its window on the page, then creates its launcher.
    pub fn install_web_app(&mut self, token: &str, name: &str, icon: &str) {
        let Some(p) = self.phase3.webapps.pending.take_if(|p| p.token == token) else { return };
        self.close_overlay(false);
        let dir = launchers_dir();
        let Some(apps) = self.phase3.webapps.apps.as_mut() else { return };
        let exists_dir = dir.clone();
        let app = apps.install(&p.proposal, name, &move |b| exists_dir.join(if cfg!(target_os = "macos") { format!("{b}.app") } else { format!("{b}.url") }).exists(), &|_| false);
        let icons = self.icon_dir();
        let _ = std::fs::create_dir_all(&icons);
        let master = icons.join(format!("{}.png", app.id));
        let mut icon_path = None;
        if let Some(bytes) = decode_png_data_url(icon) {
            if std::fs::write(&master, &bytes).is_ok() {
                resize_png(&master, &icons.join(format!("{}.64.png", app.id)), 64);
                icon_path = Some(master);
            }
        }
        self.phase3.webapps.small.remove(&app.id);
        if self.state().tabs.contains_key(&p.tab_id) {
            self.move_tab_to_web_app(&p.tab_id, &app.id);
        } else {
            self.open_web_app_window(&app, &app.start_url.clone());
        }
        self.web_app_write_launcher_async(app, icon_path);
        self.notify_web_apps();
    }

    fn web_app_write_launcher_async(&self, app: WebApp, icon: Option<PathBuf>) {
        let inbox = self.phase3.webapps.inbox.clone();
        let debug = self.debug;
        std::thread::spawn(move || {
            if let Err(e) = write_launcher(&app, &launchers_dir(), icon.as_deref()) {
                eprintln!("[webapp] launcher not created: {e}");
            } else if debug {
                println!("[webapp] launcher written for {}", app.name);
            }
            if let Ok(mut q) = inbox.lock() {
                q.push(Inbound::LaunchersChanged);
            }
        });
    }

    // ------------------------------------------------------------------ rename, reveal, remove

    pub fn rename_web_app(&mut self, id: &str, name: &str) {
        let dir = launchers_dir();
        let Some(apps) = self.phase3.webapps.apps.as_mut() else { return };
        let (d1, d2, id_s) = (dir.clone(), dir.clone(), id.to_string());
        let exists = move |b: &str| launcher_path(&d1, b, "").exists();
        let own = move |b: &str| owns_launcher(&launcher_path(&d2, b, &id_s), &id_s);
        let Some((old_bundle, _)) = apps.rename(id, name, &exists, &own) else { return };
        let Some(app) = apps.find(id).cloned() else { return };
        let old = launcher_path(&dir, &old_bundle, id);
        let new = launcher_path(&dir, &app.bundle, id);
        if old != new && owns_launcher(&old, id) {
            remove_path(&old);
        }
        let ids: Vec<u64> = self.phase3.webapps.windows.iter().filter(|w| w.app_id == id).map(|w| w.id).collect();
        for w in ids {
            self.web_app_push(w, true);
        }
        let icon = Some(self.icon_dir().join(format!("{id}.png"))).filter(|p| p.exists());
        self.web_app_write_launcher_async(app, icon);
        self.notify_web_apps();
    }

    /// Shows the launcher in the file manager; recreated first if it was deleted.
    pub fn reveal_web_app(&mut self, id: &str) {
        let dir = launchers_dir();
        let Some(app) = self.apps_ref().and_then(|x| x.find(id)).cloned() else { return };
        let path = launcher_path(&dir, &app.bundle, id);
        if !path.exists() {
            let icon = Some(self.icon_dir().join(format!("{id}.png"))).filter(|p| p.exists());
            if let Err(e) = write_launcher(&app, &dir, icon.as_deref()) {
                self.log(format!("launcher not recreated: {e}"));
            }
            self.notify_web_apps();
        }
        if path.exists() {
            crate::app::platform::reveal(&path.to_string_lossy());
        }
    }

    /// Uninstalls: closes its windows, deletes its launcher (if it is really ours) and its icons.
    pub fn remove_web_app(&mut self, id: &str) {
        let Some(app) = self.apps_ref().and_then(|x| x.find(id)).cloned() else { return };
        let open: Vec<u64> = self.phase3.webapps.windows.iter().filter(|w| w.app_id == id).map(|w| w.id).collect();
        for w in open {
            self.send(Cmd::CloseAux(w));
            self.web_app_window_closed(w);
        }
        if let Some(apps) = self.phase3.webapps.apps.as_mut() {
            apps.remove(id);
        }
        self.phase3.webapps.small.remove(id);
        let path = launcher_path(&launchers_dir(), &app.bundle, id);
        if owns_launcher(&path, id) {
            remove_path(&path);
        }
        let icons = self.icon_dir();
        let _ = std::fs::remove_file(icons.join(format!("{id}.png")));
        let _ = std::fs::remove_file(icons.join(format!("{id}.64.png")));
        self.notify_web_apps();
    }

    fn web_app_drain(&mut self) {
        let items: Vec<Inbound> = self.phase3.webapps.inbox.lock().map(|mut q| std::mem::take(&mut *q)).unwrap_or_default();
        for it in items {
            match it {
                Inbound::Proposal(seq, p) => self.web_app_show_card(seq, p),
                Inbound::LaunchersChanged => self.notify_web_apps(),
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------- hooks

/// Called once after the browser core exists and the web layer is initialized.
pub(super) fn startup(core: &mut Core) {
    core.phase3.webapps.apps = Some(WebApps::load(&core.data_dir));
}

fn win_of(core: &Core, view: ViewId) -> Option<usize> {
    core.phase3.webapps.windows.iter().position(|w| w.view.is_some_and(|v| v.id() == view))
}

/// Called for every CEF event, before the core handles it.
pub(super) fn web_event(core: &mut Core, ev: &WebEvent) {
    match ev {
        WebEvent::PageMessage { view, channel, payload, is_main_frame, .. } if channel == "webapp:page" => {
            let Some(i) = core.phase3.webapps.inspecting.as_ref() else { return };
            if *view != i.view || !*is_main_frame || i.spawned {
                return;
            }
            let Ok(v) = serde_json::from_str::<serde_json::Value>(payload) else { return };
            if v.get("seq").and_then(serde_json::Value::as_u64) != Some(i.seq) {
                return;
            }
            let page = v.get("info").and_then(feat::clean_page);
            core.web_app_spawn_inspect(page);
        }
        WebEvent::LoadEnd { view, is_main_frame: true, .. } if core.phase3.webapps.inspecting.as_ref().is_some_and(|i| i.view == *view) => core.web_app_advance_inspect(),
        WebEvent::Closed { view } => {
            if let Some(ix) = win_of(core, *view) {
                core.phase3.webapps.windows[ix].view = None;
            }
        }
        WebEvent::TitleChanged { view, title } => {
            if let Some(ix) = win_of(core, *view) {
                core.phase3.webapps.windows[ix].title = title.clone();
                let id = core.phase3.webapps.windows[ix].id;
                core.web_app_push(id, false);
            }
        }
        WebEvent::AddressChanged { view, url, is_main_frame: true } => {
            if let Some(ix) = win_of(core, *view) {
                let w = &mut core.phase3.webapps.windows[ix];
                if url.starts_with("data:") || url == "about:blank" {
                    return;
                }
                w.url = url.clone();
                if let Some(v) = w.view {
                    w.can_go_back = v.can_go_back();
                }
                let id = w.id;
                core.web_app_push(id, false);
            }
        }
        WebEvent::LoadingState { view, is_loading, can_go_back, .. } => {
            if let Some(ix) = win_of(core, *view) {
                let w = &mut core.phase3.webapps.windows[ix];
                w.loading = *is_loading;
                w.can_go_back = *can_go_back;
                let id = w.id;
                core.web_app_push(id, false);
            }
        }
        WebEvent::NewWindowRequested { view, url, disposition, .. } => {
            let Some(ix) = win_of(core, *view) else { return };
            if url.is_empty() || url == "about:blank" {
                return;
            }
            let app_id = core.phase3.webapps.windows[ix].app_id.clone();
            let app = core.apps_ref().and_then(|x| x.find(&app_id)).cloned();
            match (app, disposition) {
                (Some(a), Disposition::CurrentTab) => {
                    if let Some(v) = core.phase3.webapps.windows[ix].view {
                        v.load_url(url);
                    }
                    let _ = a;
                }
                // Within the app's site: another window of the app; elsewhere, a Signet tab.
                (Some(a), _) if feat::in_scope(&a, url) => core.open_web_app_window(&a, url),
                _ => {
                    core.send(Cmd::ShowMain);
                    core.open_tab(url, OpenOptions { activate: true, after_tab_id: None, opener_id: None });
                }
            }
        }
        _ => {}
    }
}

/// Called on every core tick (about once per second or at the next store deadline).
pub(super) fn tick(core: &mut Core) {
    core.web_app_drain();
    core.web_app_advance_inspect();
    if std::mem::take(&mut core.phase3.webapps.bounds_dirty)
        && let Some(a) = core.phase3.webapps.apps.as_ref()
    {
        a.save();
    }
}

/// Called before quitting (flush files, stop threads).
pub(super) fn quit(core: &mut Core) {
    if let Some(a) = core.phase3.webapps.apps.as_ref() {
        a.save();
    }
    for w in std::mem::take(&mut core.phase3.webapps.windows) {
        if let Some(v) = w.view {
            v.close();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_view_sits_under_the_bar() {
        let b = page_bounds((1000.0, 700.0));
        assert_eq!((b.x, b.y, b.width, b.height), (0.0, 38.0, 1000.0, 662.0));
        assert_eq!(page_bounds((300.0, 20.0)).height, 0.0);
    }

    #[test]
    fn bounds_must_be_sane() {
        assert!(valid_bounds(&Rect::new(10.0, 10.0, 800.0, 600.0)));
        assert!(!valid_bounds(&Rect::new(0.0, 0.0, 100.0, 600.0)));
        assert!(!valid_bounds(&Rect::new(f64::NAN, 0.0, 800.0, 600.0)));
    }

    #[test]
    fn page_script_posts_a_tagged_result() {
        let js = page_js(7);
        assert!(js.starts_with("window.__signetPost('webapp:page',JSON.stringify({seq:7,info:(() =>"));
        assert!(js.contains("manifest"));
    }

    #[test]
    fn png_data_urls() {
        use base64::Engine;
        let png = base64::engine::general_purpose::STANDARD.encode([0x89, b'P', b'N', b'G', 1, 2]);
        assert_eq!(decode_png_data_url(&format!("data:image/png;base64,{png}")).map(|b| b.len()), Some(6));
        assert!(decode_png_data_url("data:image/jpeg;base64,AAAA").is_none());
        let junk = base64::engine::general_purpose::STANDARD.encode(b"not a png");
        assert!(decode_png_data_url(&format!("data:image/png;base64,{junk}")).is_none());
    }

    #[test]
    fn launcher_ownership_is_checked() {
        let dir = std::env::temp_dir().join(format!("signet-wa-test-{}", std::process::id()));
        let id = "123e4567-e89b-12d3-a456-426614174000";
        let app = WebApp { id: id.into(), name: "My App".into(), start_url: "https://a.com/".into(), scope: "https://a.com/".into(), space_id: None, theme_color: None, bounds: None, bundle: "My App".into(), installed_at: 1 };
        let path = launcher_path(&dir, &app.bundle, id);
        assert!(!owns_launcher(&path, id));
        write_launcher(&app, &dir, None).expect("launcher");
        assert!(path.exists() && owns_launcher(&path, id));
        assert!(!owns_launcher(&path, "ffffffff-e89b-12d3-a456-426614174000"));
        assert!(path.to_string_lossy().contains(if cfg!(target_os = "macos") { "My App.app" } else if cfg!(target_os = "windows") { "My App.url" } else { feat::BUNDLE_PREFIX }));
        remove_path(&path);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

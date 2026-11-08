//! PHASE3: site panel data, per-site permissions and data, page tools (save, PDF) and the menu command router
//! (`phase3_command`). Port of `browser/sitepanel.ts` and `features/pagetools.ts`.
//!
//! The Electron save dialogs have no equivalent here yet: files go to the download folder under a unique name and
//! the result is revealed in the file manager.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use indexmap::IndexMap;
use signet_core::types::{PermissionDecision, PermissionState, SiteBoostFlags, SiteInfo, SitePermissionEntry, SitePermissionRow};

use crate::browser::Core;

/// Permissions shown in the panel. `media` covers camera and microphone.
const PERMISSIONS: [&str; 6] = ["media", "geolocation", "notifications", "clipboard-read", "midi", "display-capture"];
/// The engine's names behind the `media` row (older data stored `media` itself).
const MEDIA_KEYS: [&str; 3] = ["media", "camera", "microphone"];
/// How long a PDF export is awaited before giving up on revealing it.
const PDF_WAIT: Duration = Duration::from_secs(60);

// ---------------------------------------------------------------------------------------------- pure helpers

/// File name valid everywhere: no `:` or brackets (IPv6), reserved Windows names suffixed, no trailing dot or space.
pub(super) fn safe_file_base(name: &str) -> String {
    let mut out: String = name.chars().map(|c| if matches!(c, '[' | ']' | ':') { '-' } else { c }).collect();
    out.truncate(out.trim_end_matches(['.', ' ']).len());
    let stem = out.split('.').next().unwrap_or("").to_ascii_lowercase();
    let reserved = matches!(stem.as_str(), "con" | "prn" | "aux" | "nul")
        || ((stem.starts_with("com") || stem.starts_with("lpt")) && stem.len() == 4 && stem.as_bytes()[3].is_ascii_digit() && stem.as_bytes()[3] != b'0');
    if reserved {
        out.push('_');
    }
    out
}

/// Safe file name taken from the page title (or its host), without extension.
pub(super) fn base_name(title: &str, url: &str) -> String {
    let host = url::Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_default();
    let raw = if title.trim().is_empty() { if host.is_empty() { "page" } else { host.as_str() } } else { title.trim() };
    let cleaned: String = raw.chars().map(|c| if matches!(c, '/' | '\\' | '?' | '%' | '*' | ':' | '|' | '"' | '<' | '>') || c.is_control() { '-' } else { c }).collect();
    let collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let cut: String = collapsed.chars().take(120).collect();
    let name = safe_file_base(cut.trim());
    if name.is_empty() { "page".into() } else { name }
}

/// True if the page can be printed or saved (a web page or a file, not an internal page).
pub(super) fn can_save(url: &str) -> bool {
    let l = url.to_ascii_lowercase();
    ["http:", "https:", "file:", "data:", "view-source:"].iter().any(|p| l.starts_with(p))
}

/// Origin of a web page, or `None` (internal pages, empty tab).
pub(super) fn origin_of(url: &str) -> Option<String> {
    let u = url::Url::parse(url).ok()?;
    matches!(u.scheme(), "http" | "https").then(|| u.origin().ascii_serialization())
}

/// State of one permission row of the panel for an origin's stored decisions.
pub(super) fn permission_state(stored: Option<&IndexMap<String, PermissionDecision>>, id: &str) -> PermissionState {
    let Some(m) = stored else { return PermissionState::Ask };
    let keys: &[&str] = if id == "media" { &MEDIA_KEYS } else { std::slice::from_ref(&id) };
    let found: Vec<PermissionDecision> = keys.iter().filter_map(|k| m.get(*k).copied()).collect();
    if found.contains(&PermissionDecision::Deny) {
        PermissionState::Deny
    } else if found.contains(&PermissionDecision::Allow) {
        PermissionState::Allow
    } else {
        PermissionState::Ask
    }
}

/// Engine permission names behind a panel row.
fn stored_keys(permission: &str) -> Vec<&str> {
    if permission == "media" { MEDIA_KEYS.to_vec() } else { vec![permission] }
}

/// A command of the menus handled by phase 3 (File and View menus, page context menu).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PageCommand {
    Capture,
    Reader,
    Translate,
    SavePage,
    ExportPdf,
    InstallApp,
    CustomizeSite,
    Responsive,
}

impl PageCommand {
    pub(super) fn parse(id: &str) -> Option<PageCommand> {
        Some(match id {
            "capture" => Self::Capture,
            "reader" => Self::Reader,
            "translate" => Self::Translate,
            "savePage" => Self::SavePage,
            "exportPdf" => Self::ExportPdf,
            "installApp" => Self::InstallApp,
            "customizeSite" => Self::CustomizeSite,
            "responsive" => Self::Responsive,
            _ => return None,
        })
    }
}

// ---------------------------------------------------------------------------------------------- state

enum Outcome {
    SiteDataCleared { origin: String, ok: bool },
    PageSaved { path: PathBuf, ok: bool },
}

/// State owned by this feature (a field of `Phase3`).
#[derive(Default)]
pub struct State {
    outbox: Rc<RefCell<Vec<Outcome>>>,
    /// PDF exports in progress: the engine gives no completion callback, so the file is awaited.
    pdfs: Vec<(PathBuf, Instant)>,
}

impl Core {
    /// What the site panel shows for the active tab; `None` for internal pages and empty tabs.
    pub fn site_info(&self) -> Option<SiteInfo> {
        let id = self.active_tab_id()?;
        let tab = self.state().tabs.get(&id)?;
        let origin = origin_of(&tab.url)?;
        let site = signet_core::site::site_of(&tab.url);
        let stored = self.state().site_permissions.get(&origin);
        let boost = self.state().boosts.get(&site);
        Some(SiteInfo {
            tab_id: id,
            origin,
            site,
            secure: tab.url.starts_with("https:"),
            permissions: PERMISSIONS
                .iter()
                .map(|p| SitePermissionRow { id: (*p).to_string(), label: signet_core::i18n::t(&format!("sitePanel.{p}")), state: permission_state(stored, p) })
                .collect(),
            boost: SiteBoostFlags { exists: boost.is_some(), enabled: boost.is_some_and(|b| b.enabled) },
            // The ad blocker (phase3 net) counts per tab; wire it here when it exposes the number.
            blocked: 0,
            favicon: tab.favicon.clone(),
        })
    }

    pub fn list_site_permissions(&mut self) -> Vec<SitePermissionEntry> {
        self.state()
            .site_permissions
            .iter()
            .flat_map(|(origin, m)| m.iter().map(move |(p, d)| SitePermissionEntry { origin: origin.clone(), permission: p.clone(), decision: *d }))
            .collect()
    }

    pub fn reset_site_permissions(&mut self, origin: Option<&str>, permission: Option<&str>) {
        let st = &mut self.store.data.state;
        match (origin, permission) {
            (None, _) => st.site_permissions.clear(),
            (Some(o), None) => {
                st.site_permissions.shift_remove(o);
            }
            (Some(o), Some(p)) => {
                if let Some(m) = st.site_permissions.get_mut(o) {
                    for key in stored_keys(p) {
                        m.shift_remove(key);
                    }
                    if m.is_empty() {
                        st.site_permissions.shift_remove(o);
                    }
                }
            }
        }
        self.changed();
    }

    pub fn set_site_permission(&mut self, origin: &str, permission: &str, state: PermissionState) {
        if origin_of(origin).is_none() {
            return;
        }
        let decision = match state {
            PermissionState::Allow => Some(PermissionDecision::Allow),
            PermissionState::Deny => Some(PermissionDecision::Deny),
            PermissionState::Ask => None,
        };
        let m = self.store.data.state.site_permissions.entry(origin.to_string()).or_default();
        // The `media` row covers the camera and the microphone, which the engine asks for separately.
        let keys: Vec<&str> = if permission == "media" { vec!["camera", "microphone"] } else { vec![permission] };
        if permission == "media" {
            m.shift_remove("media");
        }
        for key in keys {
            match decision {
                Some(d) => {
                    m.insert(key.to_string(), d);
                }
                None => {
                    m.shift_remove(key);
                }
            }
        }
        if m.is_empty() {
            self.store.data.state.site_permissions.shift_remove(origin);
        }
        self.changed();
    }

    /// Clears the site's cookies and storage (through the active tab's profile), then reloads the tab if it shows it.
    pub fn clear_site_data(&mut self, origin: &str) {
        if origin_of(origin).is_none() {
            return;
        }
        let Some(view) = self.active_tab_id().and_then(|id| self.views.view_of(&id)).or_else(|| self.views.ids().iter().find_map(|id| self.views.view_of(id))) else {
            self.log("clear_site_data: no page to reach the profile");
            return;
        };
        let outbox = self.phase3.page_tools.outbox.clone();
        let origin = origin.to_string();
        view.clear_site_data(&origin.clone(), move |r| outbox.borrow_mut().push(Outcome::SiteDataCleared { origin, ok: r.is_ok() }));
    }

    /// Menu commands of the page tools (capture, reader, translate, save, install app, customize, responsive).
    pub fn phase3_command(&mut self, id: &str, tab: Option<&str>) {
        let Some(cmd) = PageCommand::parse(id) else {
            self.log(format!("phase3_command: unknown command {id}"));
            return;
        };
        let Some(tab) = tab.map(str::to_string).or_else(|| self.active_tab_id()) else { return };
        match cmd {
            PageCommand::Capture => self.start_capture(Some(&tab)),
            PageCommand::Reader => self.toggle_reader(&tab),
            PageCommand::Translate => self.toggle_translation(&tab),
            PageCommand::SavePage => self.save_page(&tab),
            PageCommand::ExportPdf => self.save_pdf(&tab),
            PageCommand::CustomizeSite => {
                if let Some(url) = self.state().tabs.get(&tab).map(|t| t.url.clone())
                    && origin_of(&url).is_some()
                {
                    let site = signet_core::site::site_of(&url);
                    self.open_boost(&site);
                }
            }
            PageCommand::InstallApp => self.prompt_install_web_app(Some(&tab)),
            PageCommand::Responsive => self.toggle_responsive(&tab),
        }
    }

    /// Where a saved file goes: the download folder, under a unique name.
    fn save_target(&self, name: &str) -> PathBuf {
        let dir = self.download_dir();
        let _ = std::fs::create_dir_all(&dir);
        signet_features::downloads::unique_path(&dir, name, &Default::default(), &|p| p.exists())
    }

    /// Saves the page as a single web archive (MHTML), which keeps its images and styles in one file.
    pub fn save_page(&mut self, tab: &str) {
        let (Some(view), Some(t)) = (self.views.view_of(tab), self.state().tabs.get(tab)) else { return };
        if !can_save(&t.url) {
            return;
        }
        let path = self.save_target(&format!("{}.mhtml", base_name(&t.title, &t.url)));
        let outbox = self.phase3.page_tools.outbox.clone();
        let p = path.clone();
        view.save_page_mhtml(&path.to_string_lossy(), move |r| outbox.borrow_mut().push(Outcome::PageSaved { path: p, ok: r.is_ok() }));
    }

    /// Saves only the page's current HTML.
    pub fn save_page_html(&mut self, tab: &str) {
        let (Some(view), Some(t)) = (self.views.view_of(tab), self.state().tabs.get(tab)) else { return };
        if !can_save(&t.url) {
            return;
        }
        let path = self.save_target(&format!("{}.html", base_name(&t.title, &t.url)));
        let outbox = self.phase3.page_tools.outbox.clone();
        let p = path.clone();
        view.save_page_html(&path.to_string_lossy(), move |r| outbox.borrow_mut().push(Outcome::PageSaved { path: p, ok: r.is_ok() }));
    }

    /// Exports the page to PDF.
    pub fn save_pdf(&mut self, tab: &str) {
        let (Some(view), Some(t)) = (self.views.view_of(tab), self.state().tabs.get(tab)) else { return };
        if !can_save(&t.url) {
            return;
        }
        let path = self.save_target(&format!("{}.pdf", base_name(&t.title, &t.url)));
        view.print_to_pdf(&path.to_string_lossy());
        self.phase3.page_tools.pdfs.push((path, Instant::now()));
    }
}

/// Called once after the browser core exists and the web layer is initialized.
pub(super) fn startup(_core: &mut Core) {}

/// Called for every CEF event, before the core handles it.
pub(super) fn web_event(core: &mut Core, _ev: &signet_web::WebEvent) {
    drain(core);
}

/// Called on every core tick (about once per second or at the next store deadline).
pub(super) fn tick(core: &mut Core) {
    drain(core);
    let now = Instant::now();
    let pdfs = std::mem::take(&mut core.phase3.page_tools.pdfs);
    for (path, since) in pdfs {
        if path.exists() {
            crate::app::platform::reveal(&path.to_string_lossy());
        } else if now.duration_since(since) < PDF_WAIT {
            core.phase3.page_tools.pdfs.push((path, since));
        }
    }
}

/// Called before quitting (flush files, stop threads).
pub(super) fn quit(_core: &mut Core) {}

fn drain(core: &mut Core) {
    let outcomes: Vec<Outcome> = std::mem::take(&mut *core.phase3.page_tools.outbox.borrow_mut());
    for o in outcomes {
        match o {
            Outcome::SiteDataCleared { origin, ok } => {
                if !ok {
                    core.log(format!("clear_site_data failed for {origin}"));
                    continue;
                }
                let active = core.active_tab_id();
                if let Some(id) = active
                    && core.state().tabs.get(&id).and_then(|t| origin_of(&t.url)).as_deref() == Some(origin.as_str())
                {
                    core.reload_tab(Some(&id), true);
                }
            }
            Outcome::PageSaved { path, ok } => {
                if ok {
                    crate::app::platform::reveal(&path.to_string_lossy());
                } else {
                    core.log(format!("save page failed: {}", path.display()));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_base_is_valid_everywhere() {
        assert_eq!(safe_file_base("a:b[c]"), "a-b-c-");
        assert_eq!(safe_file_base("name. . "), "name");
        assert_eq!(safe_file_base("con"), "con_");
        assert_eq!(safe_file_base("NUL.txt"), "NUL.txt_");
        assert_eq!(safe_file_base("com1"), "com1_");
        assert_eq!(safe_file_base("com0"), "com0");
        assert_eq!(safe_file_base("console"), "console");
    }

    #[test]
    fn base_name_from_title_or_host() {
        assert_eq!(base_name("  My  page: a/b ", "https://x.example/"), "My page- a-b");
        assert_eq!(base_name("", "https://www.example.com/p"), "www.example.com");
        assert_eq!(base_name("", "nonsense"), "page");
        assert_eq!(base_name("   ", ""), "page");
        assert_eq!(base_name(&"x".repeat(300), "").len(), 120);
    }

    #[test]
    fn saving_is_for_web_pages_and_files() {
        assert!(can_save("https://a.example"));
        assert!(can_save("FILE:///tmp/a.html"));
        assert!(can_save("view-source:https://a.example"));
        assert!(!can_save("signet://settings"));
        assert!(!can_save(""));
    }

    #[test]
    fn origin_only_for_http() {
        assert_eq!(origin_of("https://a.example:8443/x?y").as_deref(), Some("https://a.example:8443"));
        assert_eq!(origin_of("http://a.example/").as_deref(), Some("http://a.example"));
        assert!(origin_of("signet://note/1").is_none());
        assert!(origin_of("file:///a").is_none());
        assert!(origin_of("").is_none());
    }

    #[test]
    fn media_row_merges_camera_and_microphone() {
        let mut m: IndexMap<String, PermissionDecision> = IndexMap::new();
        assert_eq!(permission_state(Some(&m), "media"), PermissionState::Ask);
        assert_eq!(permission_state(None, "geolocation"), PermissionState::Ask);
        m.insert("camera".into(), PermissionDecision::Allow);
        assert_eq!(permission_state(Some(&m), "media"), PermissionState::Allow);
        m.insert("microphone".into(), PermissionDecision::Deny);
        assert_eq!(permission_state(Some(&m), "media"), PermissionState::Deny);
        m.insert("geolocation".into(), PermissionDecision::Allow);
        assert_eq!(permission_state(Some(&m), "geolocation"), PermissionState::Allow);
        assert_eq!(permission_state(Some(&m), "midi"), PermissionState::Ask);
    }

    #[test]
    fn legacy_media_key_is_read() {
        let mut m: IndexMap<String, PermissionDecision> = IndexMap::new();
        m.insert("media".into(), PermissionDecision::Deny);
        assert_eq!(permission_state(Some(&m), "media"), PermissionState::Deny);
    }

    #[test]
    fn commands_are_routed_by_id() {
        for (id, cmd) in [
            ("capture", PageCommand::Capture),
            ("reader", PageCommand::Reader),
            ("translate", PageCommand::Translate),
            ("savePage", PageCommand::SavePage),
            ("exportPdf", PageCommand::ExportPdf),
            ("installApp", PageCommand::InstallApp),
            ("customizeSite", PageCommand::CustomizeSite),
            ("responsive", PageCommand::Responsive),
        ] {
            assert_eq!(PageCommand::parse(id), Some(cmd));
        }
        assert_eq!(PageCommand::parse("print"), None);
        assert_eq!(PageCommand::parse(""), None);
    }

    #[test]
    fn media_keys_expand() {
        assert_eq!(stored_keys("media"), vec!["media", "camera", "microphone"]);
        assert_eq!(stored_keys("midi"), vec!["midi"]);
    }
}

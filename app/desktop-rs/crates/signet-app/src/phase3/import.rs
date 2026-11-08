//! Import from other browsers (port of the app side of `main/features/importers/index.ts`; parsers, locations and
//! the planning of spaces are in `signet_features::importers`). Reading the disk, the OS keychain and the SQLite
//! files runs on a worker thread; the result is applied to the state on the main thread by the next `tick`
//! (within about a second), which then shows the summary dialog.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use signet_core::core::history::ImportedEntry;
use signet_core::core::store::State as CoreState;
use signet_core::factory::{new_space, new_tab, CleanDraft};
use signet_core::tree::{new_folder, tab_item};
use signet_core::types::{ImportKind, ImportSourceInfo, Item, SplitItem};
use signet_features::importers::apply::{apply_bookmarks, plan_spaces, ExistingState, ImportCounts, PlannedItem, PlannedTab, SpacesPlan};
use signet_features::importers::chromium::{extension_ids, read_history, read_passwords, ChromiumLogin, HistoryRow, SystemKeys};
use signet_features::importers::sources::{self as src, Env, ImportSource};
use signet_features::importers::types::{ImportedNode, SpacesImport};
use signet_features::importers::{is_permission_error, read_arc, read_chrome_bookmarks, read_safari};
use signet_features::passwords::{KeychainCipher, NewLogin, Passwords};
use signet_features::secrets::KeyringStore;

use super::backup::{dialog, dialog::Level, translate};
use crate::browser::Core;

/// The list of sources is cached so that typing in the command bar does not re-read the disk.
const CACHE: Duration = Duration::from_secs(10);

type Kind = src::ImportKind;

/// What the worker read, per kind.
enum Payload {
    Spaces(SpacesImport),
    Bookmarks(ImportedNode),
    History(Vec<HistoryRow>),
    Passwords(Vec<ChromiumLogin>, usize),
    Extensions(Vec<String>),
}

struct Fetched {
    source: ImportSource,
    items: Vec<(Kind, Result<Payload, String>)>,
}

/// State owned by this feature (a field of `Phase3`).
#[derive(Default)]
pub struct State {
    cache: Option<(Instant, Vec<ImportSourceInfo>)>,
    job: Option<Receiver<Fetched>>,
    /// Extension ids to install from the Chrome Web Store (see `take_import_extensions`).
    extensions: Vec<String>,
}

// ------------------------------------------------------------------ pure parts

fn kind_to_core(k: Kind) -> ImportKind {
    match k {
        Kind::Spaces => ImportKind::Spaces,
        Kind::Bookmarks => ImportKind::Bookmarks,
        Kind::Passwords => ImportKind::Passwords,
        Kind::Extensions => ImportKind::Extensions,
        Kind::History => ImportKind::History,
    }
}

fn kind_from_core(k: ImportKind) -> Kind {
    match k {
        ImportKind::Spaces => Kind::Spaces,
        ImportKind::Bookmarks => Kind::Bookmarks,
        ImportKind::Passwords => Kind::Passwords,
        ImportKind::Extensions => Kind::Extensions,
        ImportKind::History => Kind::History,
    }
}

/// Every source known on this platform (Settings > Import), `available` when the data exists on disk
/// (port of `importSources` in `windows/settings.ts`: the title is the browser name).
fn list_sources(env: &Env) -> Vec<ImportSourceInfo> {
    src::sources_for(env.os)
        .into_iter()
        .map(|def| {
            let kinds: Vec<ImportKind> = src::kinds_of(def.id, env).into_iter().map(kind_to_core).collect();
            ImportSourceInfo { id: def.id.id().to_string(), title: def.name.to_string(), available: !kinds.is_empty(), kinds }
        })
        .collect()
}

fn history_entries(rows: &[HistoryRow]) -> Vec<ImportedEntry> {
    rows.iter()
        .map(|r| ImportedEntry {
            url: r.url.clone(),
            title: r.title.clone(),
            visits: r.visits.clamp(1, i64::from(u32::MAX)) as u32,
            typed: (r.typed > 0).then(|| r.typed.min(i64::from(u32::MAX)) as u32),
            last_visit: r.last_visit as f64,
        })
        .collect()
}

fn new_logins(list: &[ChromiumLogin]) -> Vec<NewLogin> {
    list.iter().map(|l| NewLogin { origin: l.origin.clone(), username: l.username.clone(), password: l.password.clone(), created_at: Some(l.created_at).filter(|c| *c > 0) }).collect()
}

/// Summary line of a bookmarks or spaces import (`summarize`).
fn summarize(c: &ImportCounts, folder: Option<&str>) -> String {
    let t = signet_core::i18n::t_args;
    let n = |v: usize| v.to_string();
    if c.bookmarks > 0 {
        return match folder {
            Some(f) => t("import.summary.bookmarks", &[("count", &n(c.bookmarks)), ("folder", f)]),
            None => t("import.summary.bookmarksBar", &[("count", &n(c.bookmarks))]),
        };
    }
    let parts = [
        t("import.summary.spaces", &[("count", &n(c.spaces))]),
        t("import.summary.pinned", &[("count", &n(c.pinned))]),
        t("import.summary.favorites", &[("count", &n(c.favorites))]),
    ]
    .join(", ");
    let imported = t("import.summary.tabs", &[("count", &n(c.today))]);
    t("import.summary.full", &[("parts", &parts), ("imported", &imported)])
}

fn existing_state(state: &CoreState) -> ExistingState {
    let blank = state.spaces.len() == 1 && {
        let s = &state.spaces[0];
        s.pinned.is_empty() && s.today.is_empty() && s.bookmarks.as_ref().is_none_or(Vec::is_empty) && s.groups.as_ref().is_none_or(|g| g.is_empty())
    };
    ExistingState {
        space_count: state.spaces.len(),
        single_blank_space: blank,
        favorite_urls: state.tabs.values().filter(|t| state.favorites.contains(&t.id)).filter_map(|t| t.pinned_url.clone()).collect(),
    }
}

fn make_tab(state: &mut CoreState, p: &PlannedTab) -> String {
    let mut tab = new_tab(&p.url, &p.title);
    tab.favicon = p.favicon.clone();
    tab.pinned_url = p.pinned_url.clone();
    let id = tab.id.clone();
    state.tabs.insert(id.clone(), tab);
    id
}

fn make_items(state: &mut CoreState, items: &[PlannedItem]) -> Vec<Item> {
    items
        .iter()
        .map(|it| match it {
            PlannedItem::Tab(t) => tab_item(&make_tab(state, t)),
            PlannedItem::Split(tabs) => {
                let ids = tabs.iter().map(|t| make_tab(state, t)).collect();
                Item::Split(SplitItem { id: signet_core::ids::new_id(), tab_ids: ids, ..Default::default() })
            }
            PlannedItem::Folder { name, items } => {
                let mut f = new_folder(name);
                f.items = make_items(state, items);
                Item::Folder(f)
            }
        })
        .collect()
}

/// Writes a plan from `plan_spaces` into the state: new spaces (replacing a blank one), favorites, their tabs.
fn apply_plan(state: &mut CoreState, plan: &SpacesPlan) {
    let mut created = Vec::new();
    for ps in &plan.spaces {
        let mut space = new_space(CleanDraft { name: ps.name.clone(), icon: ps.icon.clone(), hue: f64::from(ps.hue), theme: None });
        space.pinned = make_items(state, &ps.pinned);
        space.today = make_items(state, &ps.today);
        created.push(space);
    }
    for f in &plan.favorites {
        let id = make_tab(state, f);
        state.favorites.push(id);
    }
    if plan.replace_existing && !created.is_empty() {
        state.active_space_id = created[0].id.clone();
        state.spaces = created;
    } else {
        state.spaces.extend(created);
    }
}

// ------------------------------------------------------------------ worker

/// Reads what was asked from a source. Each kind is independent: a failure does not stop the others.
fn read_source(id: ImportSource, wanted: &[Kind], env: &Env) -> Fetched {
    let def = src::def(id);
    let located: Option<PathBuf> = src::locate(id, env);
    let profile = src::profile_of(id, located.as_deref());
    let mut items = Vec::new();
    for &kind in wanted {
        let r = read_kind(id, kind, located.as_deref(), profile.as_deref(), def);
        items.push((kind, r));
    }
    Fetched { source: id, items }
}

fn read_kind(id: ImportSource, kind: Kind, located: Option<&Path>, profile: Option<&Path>, def: &src::SourceDef) -> Result<Payload, String> {
    let t = &translate;
    let e = |x: signet_features::util::Error| x.to_string();
    let no_profile = || "no profile".to_string();
    match kind {
        Kind::Spaces => {
            let data = read_arc(located.ok_or_else(no_profile)?, t).map_err(e)?;
            if data.spaces.is_empty() && data.favorites.is_empty() {
                return Err(signet_core::i18n::t("import.error.noContent"));
            }
            Ok(Payload::Spaces(data))
        }
        Kind::Bookmarks => {
            let path = located.ok_or_else(no_profile)?;
            let folder = if id == ImportSource::Safari {
                read_safari(path, t).map_err(e)?
            } else {
                let name = src::bookmark_folder_name(id).unwrap_or(def.name);
                read_chrome_bookmarks(profile.unwrap_or(path), name, t).map_err(e)?
            };
            folder.map(Payload::Bookmarks).ok_or_else(|| signet_core::i18n::t("import.error.noBookmarks"))
        }
        Kind::History => Ok(Payload::History(read_history(profile.ok_or_else(no_profile)?).map_err(e)?)),
        Kind::Passwords => {
            let names = def.keys.as_ref().ok_or_else(no_profile)?;
            let (logins, failed) = read_passwords(profile.ok_or_else(no_profile)?, names, &SystemKeys).map_err(e)?;
            Ok(Payload::Passwords(logins, failed))
        }
        Kind::Extensions => Ok(Payload::Extensions(extension_ids(profile.ok_or_else(no_profile)?).map_err(e)?)),
    }
}

// ------------------------------------------------------------------ main thread

/// Saved passwords. PHASE3 hand-off: the passwords module owns the live vault; until it exposes it, this opens
/// the file, adds and writes it right away (the module must reload it, or expose `&mut Passwords` here).
fn with_vault<R>(core: &Core, f: impl FnOnce(&mut Passwords) -> R) -> R {
    let mut vault = Passwords::new(&core.data_dir, std::sync::Arc::new(KeychainCipher::new(&KeyringStore)));
    let r = f(&mut vault);
    vault.flush();
    r
}

impl Core {
    /// Sources present on this machine for `SettingsInfo.import_sources` and the command bar (cached 10 s).
    pub fn import_sources(&mut self) -> Vec<ImportSourceInfo> {
        if let Some((at, list)) = &self.phase3.import.cache
            && at.elapsed() < CACHE
        {
            return list.clone();
        }
        let list = list_sources(&Env::current());
        self.phase3.import.cache = Some((Instant::now(), list.clone()));
        list
    }

    /// Imports the checked data from a source (all it has if `kinds` is empty), in display order.
    pub fn run_import(&mut self, source_id: &str, kinds: &[ImportKind]) {
        let Some(id) = ImportSource::from_id(source_id) else { return };
        if self.phase3.import.job.is_some() {
            return;
        }
        let env = Env::current();
        let offered = src::kinds_of(id, &env);
        let wanted: Vec<Kind> = if kinds.is_empty() { offered } else { offered.into_iter().filter(|k| kinds.iter().any(|c| kind_from_core(*c) == *k)).collect() };
        if wanted.is_empty() {
            return;
        }
        let (tx, rx) = channel();
        self.phase3.import.job = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(read_source(id, &wanted, &env));
        });
    }

    /// Chrome Web Store ids found by an import, handed out once: the extensions module installs the ones that are
    /// missing (the import counted them in its summary).
    pub fn take_import_extensions(&mut self) -> Vec<String> {
        std::mem::take(&mut self.phase3.import.extensions)
    }

    /// Applies what the worker read and shows the summary (port of the loop of `runImport`).
    fn finish_import(&mut self, fetched: Fetched) {
        let t = signet_core::i18n::t;
        let def = src::def(fetched.source);
        let mut lines: Vec<String> = Vec::new();
        let mut notes: Vec<String> = Vec::new();
        let mut errors: Vec<String> = Vec::new();
        for (kind, result) in fetched.items {
            let outcome = result.and_then(|payload| self.apply_payload(payload, &mut lines, &mut notes));
            let Err(message) = outcome else { continue };
            if self.import_permission_denied(fetched.source, def.name, &message) {
                return;
            }
            errors.push(signet_core::i18n::t_args("import.kindFailed", &[("kind", &t(&format!("import.kind.{}", kind.id()))), ("error", &message)]));
        }
        self.phase3.import.cache = None;
        let mut detail = lines.iter().chain(errors.iter()).cloned().collect::<Vec<_>>().join("\n");
        if !notes.is_empty() {
            detail.push_str(&format!("\n\n{}", notes.join("\n")));
        }
        if lines.is_empty() {
            dialog::notify(Level::Error, &t("import.failed"), &detail, &t("dialog.ok"));
        } else {
            dialog::notify(if errors.is_empty() { Level::Info } else { Level::Warning }, &t("import.done"), &detail, &t("dialog.ok"));
        }
    }

    fn apply_payload(&mut self, payload: Payload, lines: &mut Vec<String>, notes: &mut Vec<String>) -> Result<(), String> {
        let t = signet_core::i18n::t_args;
        match payload {
            Payload::Spaces(data) => {
                let plan = plan_spaces(&existing_state(self.state()), &data);
                apply_plan(&mut self.store.data.state, &plan);
                lines.push(summarize(&plan.counts, None));
                if plan.counts.truncated {
                    notes.push(signet_core::i18n::t("import.truncated"));
                }
                self.store.schedule_bookmarks();
                self.changed();
            }
            Payload::Bookmarks(folder) => {
                let active = self.state().active_space_id.clone();
                let state = &mut self.store.data.state;
                let Some(space) = state.spaces.iter_mut().find(|s| s.id == active) else { return Err(signet_core::i18n::t("import.error.noBookmarks")) };
                let (counts, in_folder) = apply_bookmarks(space.bookmarks.get_or_insert_with(Vec::new), &folder);
                let folder_name = match (&folder, in_folder) {
                    (ImportedNode::Folder { name, .. }, true) => Some(name.clone()),
                    _ => None,
                };
                // Imported bookmarks must be visible: the bar is shown.
                if state.settings.show_bookmarks_bar == Some(false) {
                    state.settings.show_bookmarks_bar = Some(true);
                }
                lines.push(summarize(&counts, folder_name.as_deref()));
                if counts.truncated {
                    notes.push(signet_core::i18n::t(if counts.bookmarks > 0 { "import.truncatedBookmarks" } else { "import.truncated" }));
                }
                self.store.schedule_bookmarks();
                self.changed();
            }
            Payload::History(rows) => {
                let added = self.history.import_entries(&history_entries(&rows));
                lines.push(t("import.summary.history", &[("count", &added.to_string())]));
            }
            Payload::Passwords(logins, failed) => {
                let (added, existing) = with_vault(self, |v| if v.encryption() { Some(v.add_many(&new_logins(&logins))) } else { None }).ok_or_else(|| signet_core::i18n::t("passwords.error.encryption"))?;
                lines.push(t("import.summary.passwords", &[("count", &added.to_string())]));
                if existing > 0 {
                    notes.push(t("import.passwordsExisting", &[("count", &existing.to_string())]));
                }
                if failed > 0 {
                    let hint = signet_core::i18n::t(if cfg!(target_os = "macos") {
                        "import.passwordsFailedMac"
                    } else if cfg!(target_os = "windows") {
                        "import.passwordsFailedWindows"
                    } else {
                        "import.passwordsFailedLinux"
                    });
                    notes.push(format!("{} {hint}", t("import.passwordsFailed", &[("count", &failed.to_string())])));
                }
            }
            Payload::Extensions(ids) => {
                // PHASE3 hand-off: the extensions module knows which are installed already; it drops those.
                lines.push(t("import.summary.extensions", &[("count", &ids.len().to_string())]));
                self.phase3.import.extensions.extend(ids);
            }
        }
        Ok(())
    }

    /// macOS protects other browsers' data: explains how to grant access. True if the message was a denial (the
    /// import stops there, like in Electron).
    fn import_permission_denied(&mut self, source: ImportSource, name: &str, message: &str) -> bool {
        if !is_permission_error(message) {
            return false;
        }
        let t = signet_core::i18n::t;
        let packaged = if cfg!(debug_assertions) { "detailDev" } else { "detailPackaged" };
        let (title, detail, pane) = if source == ImportSource::Safari {
            (t("import.safariDenied.message"), t(&format!("import.safariDenied.{packaged}")), "Privacy_AllFiles")
        } else if cfg!(target_os = "macos") {
            (
                signet_core::i18n::t_args("import.appDataDenied.message", &[("name", name)]),
                signet_core::i18n::t_args(&format!("import.appDataDenied.{packaged}"), &[("name", name)]),
                "Privacy_FilesAndFolders",
            )
        } else {
            return false;
        };
        if dialog::confirm(Level::Warning, &title, &detail, &t("import.safariDenied.openSettings"), &t("dialog.cancel"), false) {
            crate::app::platform::open_external(&format!("x-apple.systempreferences:com.apple.preference.security?{pane}"));
        }
        true
    }
}

/// Called once after the browser core exists and the web layer is initialized.
pub(super) fn startup(_core: &mut Core) {}

/// Called for every CEF event, before the core handles it.
pub(super) fn web_event(_core: &mut Core, _ev: &signet_web::WebEvent) {}

/// Called on every core tick: applies the result of a finished import.
pub(super) fn tick(core: &mut Core) {
    let polled = match &core.phase3.import.job {
        Some(rx) => rx.try_recv(),
        None => return,
    };
    match polled {
        Ok(fetched) => {
            core.phase3.import.job = None;
            core.finish_import(fetched);
        }
        Err(TryRecvError::Empty) => {}
        Err(TryRecvError::Disconnected) => core.phase3.import.job = None,
    }
}

/// Called before quitting (flush files, stop threads).
pub(super) fn quit(_core: &mut Core) {}

#[cfg(test)]
mod tests {
    use super::*;
    use signet_core::core::store::State as S;
    use signet_features::importers::types::{ImportedSpace, ImportedTab};

    fn tab(u: &str) -> ImportedNode {
        ImportedNode::tab(u, u)
    }

    fn data() -> SpacesImport {
        SpacesImport {
            spaces: vec![ImportedSpace {
                name: "Work".into(),
                icon: "W".into(),
                pinned: vec![tab("https://a.com"), ImportedNode::Folder { name: "F".into(), items: vec![tab("https://b.com")] }],
                today: vec![ImportedNode::Split(vec![ImportedTab { url: "https://c.com".into(), title: "c".into() }, ImportedTab { url: "https://d.com".into(), title: "d".into() }])],
            }],
            favorites: vec![ImportedTab { url: "https://fav.com".into(), title: "fav".into() }],
        }
    }

    fn state_with_blank_space() -> S {
        let mut s = S::default();
        let sp = new_space(CleanDraft { name: "Space".into(), icon: "i".into(), hue: 250.0, theme: None });
        s.active_space_id = sp.id.clone();
        s.spaces.push(sp);
        s
    }

    #[test]
    fn a_blank_state_is_replaced() {
        let mut s = state_with_blank_space();
        let old = s.spaces[0].id.clone();
        let plan = plan_spaces(&existing_state(&s), &data());
        assert!(plan.replace_existing);
        apply_plan(&mut s, &plan);
        assert_eq!(s.spaces.len(), 1);
        assert_ne!(s.spaces[0].id, old);
        assert_eq!(s.active_space_id, s.spaces[0].id);
        assert_eq!(s.spaces[0].name, "Work");
        // a, b (in the folder), c and d (split), favorite
        assert_eq!(s.tabs.len(), 5);
        assert_eq!(s.favorites.len(), 1);
        let fav = &s.tabs[&s.favorites[0]];
        assert_eq!(fav.pinned_url.as_deref(), Some("https://fav.com"));
        assert!(matches!(&s.spaces[0].pinned[1], Item::Folder(f) if f.items.len() == 1));
        assert!(matches!(&s.spaces[0].today[0], Item::Split(sp) if sp.tab_ids.len() == 2));
        assert!(s.tabs.values().all(|t| !t.loaded));
    }

    #[test]
    fn a_used_state_gets_the_spaces_added() {
        let mut s = state_with_blank_space();
        s.spaces[0].pinned.push(tab_item("x"));
        let plan = plan_spaces(&existing_state(&s), &data());
        assert!(!plan.replace_existing);
        apply_plan(&mut s, &plan);
        assert_eq!(s.spaces.len(), 2);
        assert_eq!(s.spaces[1].name, "Work");
        assert_ne!(s.active_space_id, s.spaces[1].id);
    }

    #[test]
    fn known_favorites_are_skipped() {
        let mut s = state_with_blank_space();
        let mut t = new_tab("https://fav.com", "fav");
        t.pinned_url = Some("https://fav.com".into());
        s.favorites.push(t.id.clone());
        s.tabs.insert(t.id.clone(), t);
        let plan = plan_spaces(&existing_state(&s), &data());
        assert!(plan.favorites.is_empty());
    }

    #[test]
    fn history_rows_become_entries() {
        let rows = vec![
            HistoryRow { url: "https://a.com".into(), title: "A".into(), visits: 0, typed: 0, last_visit: 5 },
            HistoryRow { url: "https://b.com".into(), title: "".into(), visits: 3, typed: 2, last_visit: 9 },
        ];
        let e = history_entries(&rows);
        assert_eq!((e[0].visits, e[0].typed, e[0].last_visit), (1, None, 5.0));
        assert_eq!((e[1].visits, e[1].typed), (3, Some(2)));
    }

    #[test]
    fn logins_keep_their_creation_date_when_known() {
        let l = vec![
            ChromiumLogin { origin: "https://a.com".into(), username: "u".into(), password: "p".into(), created_at: 0 },
            ChromiumLogin { origin: "https://b.com".into(), username: "u".into(), password: "p".into(), created_at: 7 },
        ];
        let n = new_logins(&l);
        assert_eq!((n[0].created_at, n[1].created_at), (None, Some(7)));
    }

    #[test]
    fn kinds_round_trip() {
        for k in signet_core::types::IMPORT_KINDS {
            assert_eq!(kind_to_core(kind_from_core(k)), k);
        }
    }

    #[test]
    fn sources_of_an_empty_home_are_unavailable() {
        let home = std::env::temp_dir().join(format!("signet-import-test-{}", std::process::id()));
        let env = Env { os: src::Os::current(), home, local_app_data: None, xdg_config_home: None };
        assert!(list_sources(&env).iter().all(|s| !s.available));
    }

    #[test]
    fn summaries() {
        let c = ImportCounts { bookmarks: 3, ..Default::default() };
        assert!(summarize(&c, Some("Chrome")).contains("Chrome"));
        assert!(!summarize(&c, None).contains("Chrome"));
        let c = ImportCounts { spaces: 2, pinned: 4, favorites: 1, today: 6, ..Default::default() };
        assert!(summarize(&c, None).contains('6'));
    }
}

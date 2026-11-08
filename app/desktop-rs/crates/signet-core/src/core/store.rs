//! Ports `app/desktop/src/main/core/store.ts`: the persisted state (`state.json`, `bookmarks.json`,
//! `tab-history.json`): defaults, loading with migrations and repairs, settings validation, deferred atomic writes.
//!
//! ## Write timing (decision: deadlines driven by the app, no thread)
//!
//! The store never spawns a thread and never sleeps. Changes call `schedule(delay_ms)` (state, 500 ms after a
//! change), `schedule_bookmarks()` (500 ms) or `schedule_history()` (5 s); lazy changes such as a title or icon use
//! `schedule(LAZY_DELAY)` (60 s). Each call only records a deadline (an earlier deadline is never pushed back). The
//! app drives the loop: `next_deadline()` tells when to wake up, `due(now)` / `tick(now)` write what is due, and
//! `flush()` writes everything at once (shutdown). Writes are synchronous, atomic (tmp + fsync + rename) and skipped
//! when the content did not change.

use super::atomic::{read_text, write_file_durable, AtomicWriter};
use crate::devhosts::{alias_target, is_alias_name};
use crate::factory::{clean_space_draft, new_space, new_tab, CleanDraft};
use crate::i18n::{init_locale, t};
use crate::ids::now_ms;
use crate::languages::{default_languages, normalize_languages};
use crate::model::types::ModelData;
use crate::pagesettings::{clean_site_zoom, clean_site_zoom_by_space, is_font_size, is_min_font_size, is_zoom_percent, SiteZoom, SiteZoomBySpace};
use crate::searchengines::{clean_default_engine, clean_engine_config, resolve_engines, EngineConfig};
use crate::shortcuts::{clean_overrides_value, ShortcutOverrides};
use crate::tree::{prune_items, strip_groups, sync_groups};
use crate::types::{Bookmark, Boost, DownloadInfo, DownloadStatus, PermissionDecision, Rect, Settings, Space, SpaceDraft, TabGroup, TabInfo, Zone, APP_ICONS};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashSet;
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};

pub use crate::model::types::ARCHIVE_MAX;

/// Delay of a normal state write after a change (ms).
pub const SAVE_DELAY: i64 = 500;
/// Delay of the tab histories write (ms).
pub const HISTORY_DELAY: i64 = 5_000;
/// Delay for changes that can wait (title, icon): shutdown flushes anyway (ms).
pub const LAZY_DELAY: i64 = 60_000;

pub const SIDEBAR_MIN: f64 = 180.0;
pub const SIDEBAR_MAX: f64 = 420.0;
/// Overshoot tolerated during the gesture (elastic effect); never persisted.
pub const SIDEBAR_OVERSHOOT: f64 = 80.0;
pub const DOWNLOADS_MAX: usize = 30;
pub const HISTORY_MAX_ENTRIES: usize = 50;
/// Maximum cumulative size of a tab's `pageState`, then of all tabs (characters).
pub const HISTORY_TAB_STATE_MAX: usize = 200_000;
pub const HISTORY_TOTAL_STATE_MAX: usize = 3_000_000;
const BOOKMARKS_MAX_DEPTH: usize = 20;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct ArchivedTab {
    /// Unique identifier (absent from archives predating its addition).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub url: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub favicon: Option<String>,
    pub archived_at: i64,
}

/// Navigation history entry of a tab (main process only).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct NavEntry {
    pub url: String,
    pub title: String,
    /// Scroll position, forms... (Chromium base64).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_state: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct TabHistory {
    pub entries: Vec<NavEntry>,
    pub index: usize,
}

/// Closed tab, with its place and its navigation history (in-memory stack, never written to disk).
#[derive(Debug, Clone, PartialEq)]
pub struct ClosedTab {
    pub url: String,
    pub title: String,
    pub space_id: String,
    pub favicon: Option<String>,
    pub pinned_url: Option<String>,
    pub custom_title: Option<String>,
    pub custom_icon: Option<String>,
    /// Original area, folder and rank; the rank counts within the list (area root or folder).
    pub zone: Zone,
    pub folder_id: Option<String>,
    pub index: usize,
    pub history: Option<TabHistory>,
    /// Same value for the tabs of a group closed at once: reopening brings them back together.
    pub batch: Option<u64>,
    /// Header of the closed group (on its first tab).
    pub group: Option<TabGroup>,
    /// Today tab group the tab belonged to: it goes back there if it still exists.
    pub group_id: Option<String>,
}

/// Bounds a history: 50 entries around the active one, `page_state` dropped for the farthest ones beyond the budget.
pub fn trim_history(entries: &[NavEntry], index: i64, budget: usize) -> Option<TabHistory> {
    if entries.is_empty() || index < 0 || index as usize >= entries.len() {
        return None;
    }
    let index = index as usize;
    let mut start = index.saturating_sub(HISTORY_MAX_ENTRIES / 2);
    let end = (start + HISTORY_MAX_ENTRIES).min(entries.len());
    start = end.saturating_sub(HISTORY_MAX_ENTRIES);
    let mut kept: Vec<NavEntry> = entries[start..end].to_vec();
    let active = index - start;
    // Budget consumed starting from the active entry, outward.
    let mut left = budget;
    let mut order: Vec<usize> = (0..kept.len()).collect();
    order.sort_by_key(|i| i.abs_diff(active));
    for i in order {
        let size = kept[i].page_state.as_ref().map_or(0, |s| s.chars().count());
        if size <= left {
            left -= size;
        } else {
            kept[i].page_state = None;
        }
    }
    Some(TabHistory { entries: kept, index: active })
}

fn valid_history(h: &Value) -> Option<TabHistory> {
    let entries = h.get("entries")?.as_array()?;
    let idx = h.get("index")?.as_f64().filter(|n| n.fract() == 0.0)?;
    let parsed: Vec<NavEntry> = entries
        .iter()
        .filter_map(|e| {
            let url = e.get("url")?.as_str().filter(|u| !u.is_empty())?;
            Some(NavEntry {
                url: url.to_string(),
                title: e.get("title").and_then(Value::as_str).unwrap_or("").to_string(),
                page_state: e.get("pageState").and_then(Value::as_str).map(String::from),
            })
        })
        .collect();
    if parsed.len() != entries.len() {
        return None;
    }
    trim_history(&parsed, idx as i64, HISTORY_TAB_STATE_MAX)
}

/// The persisted state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct State {
    pub version: u32,
    pub tabs: IndexMap<String, TabInfo>,
    pub favorites: Vec<String>,
    pub spaces: Vec<Space>,
    pub active_space_id: String,
    pub sidebar_visible: bool,
    #[serde(serialize_with = "crate::types::f64_js")]
    pub sidebar_width: f64,
    pub archive: Vec<ArchivedTab>,
    pub downloads: Vec<DownloadInfo>,
    pub settings: Settings,
    /// Boosts per site (key: `site_of(url)`).
    pub boosts: IndexMap<String, Boost>,
    /// Permission decisions per origin.
    pub site_permissions: IndexMap<String, IndexMap<String, PermissionDecision>>,
    pub bounds: Option<Rect>,
    /// Last size and position of the mini player.
    pub pip_bounds: Option<Rect>,
    /// Zoom level the user chose per site host (not part of backups).
    pub site_zoom: SiteZoom,
    /// Same for each isolated space (key: space id); dropped with the space.
    pub site_zoom_by_space: SiteZoomBySpace,
}

impl Default for State {
    /// An empty shell (no space). Use `initial_state` for a fresh install.
    fn default() -> Self {
        State {
            version: 1,
            tabs: IndexMap::new(),
            favorites: Vec::new(),
            spaces: Vec::new(),
            active_space_id: String::new(),
            sidebar_visible: true,
            sidebar_width: 240.0,
            archive: Vec::new(),
            downloads: Vec::new(),
            settings: Settings::default(),
            boosts: IndexMap::new(),
            site_permissions: IndexMap::new(),
            bounds: None,
            pip_bounds: None,
            site_zoom: SiteZoom::new(),
            site_zoom_by_space: SiteZoomBySpace::new(),
        }
    }
}

/// `clean_space_draft` with the translated default name.
pub fn clean_draft(draft: &SpaceDraft) -> CleanDraft {
    clean_space_draft(draft, &t("defaults.space"))
}

/// State of a fresh install.
pub fn initial_state(system_languages: &[String]) -> State {
    // Starting space: purple to red gradient, half grain.
    let space = new_space(CleanDraft {
        name: t("defaults.space"),
        icon: "\u{1f3e0}".into(),
        hue: 250.0,
        theme: Some(crate::types::SpaceTheme { hues: vec![250.0, 0.0], grain: 0.5 }),
    });
    let settings = Settings { languages: default_languages(system_languages), ..Settings::default() };
    State { active_space_id: space.id.clone(), spaces: vec![space], settings, ..State::default() }
}

/// `Settings` keys validated by `clean_settings`.
pub const CLEAN_SETTING_KEYS: [&str; 14] = [
    "archiveAfterHours",
    "tabSleepMinutes",
    "devAliases",
    "shortcuts",
    "extensionOrder",
    "pinnedExtensions",
    "searchEngine",
    "searchEngines",
    "searchKeywords",
    "downloadDir",
    "fontSize",
    "minimumFontSize",
    "defaultZoom",
    "globalPrivacyControl",
];

/// A partial settings update: the JSON object of the changed fields (camelCase keys).
pub type SettingsPatch = Map<String, Value>;

fn is_int(v: &Value) -> Option<u32> {
    v.as_f64().filter(|n| n.is_finite() && n.fract() == 0.0 && *n >= 0.0 && *n <= f64::from(u32::MAX)).map(|n| n as u32)
}

/// Settings validated field by field: an invalid value in `patch` leaves the one from `base`. Returns `base` with
/// only the `CLEAN_SETTING_KEYS` fields replaced (absent from `patch` = unchanged).
pub fn clean_settings(patch: &SettingsPatch, base: &Settings) -> Settings {
    let mut out = base.clone();
    if let Some(h) = patch.get("archiveAfterHours").and_then(Value::as_f64).filter(|h| h.is_finite() && *h >= 0.0) {
        out.archive_after_hours = h;
    }
    if let Some(m) = patch.get("tabSleepMinutes").and_then(is_int) {
        out.tab_sleep_minutes = Some(m);
    }
    // Local names: valid name, normalized local target, only one per name (the first wins).
    if let Some(list) = patch.get("devAliases").and_then(Value::as_array) {
        let mut seen = HashSet::new();
        let mut aliases = Vec::new();
        for a in list {
            let name = a.get("name").and_then(Value::as_str).map(|s| s.trim().to_lowercase()).unwrap_or_default();
            let target = a.get("target").and_then(Value::as_str).and_then(alias_target);
            let Some(target) = target else { continue };
            if !is_alias_name(&name) || seen.contains(&name) {
                continue;
            }
            seen.insert(name.clone());
            aliases.push(crate::types::DevAlias { name, target });
        }
        out.dev_aliases = if aliases.is_empty() { None } else { Some(aliases) };
    }
    // Shortcuts: the whole set is rejected if inconsistent (invalid, duplicate or reserved shortcut).
    if let Some(s) = patch.get("shortcuts").and_then(|v| clean_overrides_value(v, cfg!(target_os = "macos"))) {
        out.shortcuts = if s.is_empty() { None } else { Some(s) };
    }
    // Extension buttons: valid identifiers only, no duplicates.
    let ids = |v: &Value| -> Option<Vec<String>> {
        let arr = v.as_array()?;
        let mut out: Vec<String> = Vec::new();
        for x in arr {
            if let Some(s) = x.as_str().filter(|s| s.len() == 32 && s.bytes().all(|b| (b'a'..=b'p').contains(&b)))
                && !out.iter().any(|o| o == s)
            {
                out.push(s.to_string());
            }
        }
        Some(out)
    };
    if let Some(o) = patch.get("extensionOrder").and_then(ids) {
        out.extension_order = Some(o);
    }
    if let Some(p) = patch.get("pinnedExtensions").and_then(ids) {
        out.pinned_extensions = Some(p);
    }
    // Search engines: custom ones and keyword overrides cleaned together.
    if patch.get("searchEngines").is_some() || patch.get("searchKeywords").is_some() {
        let engines = patch.get("searchEngines").cloned().unwrap_or_else(|| serde_json::to_value(&base.search_engines).unwrap_or(Value::Null));
        let kw = patch.get("searchKeywords").cloned().unwrap_or_else(|| serde_json::to_value(&base.search_keywords).unwrap_or(Value::Null));
        let cfg = clean_engine_config(&engines, &kw);
        out.search_engines = if cfg.search_engines.is_empty() { None } else { Some(cfg.search_engines) };
        out.search_keywords = if cfg.search_keywords.is_empty() { None } else { Some(cfg.search_keywords) };
    }
    // Default engine: an existing, enabled one (built-in, preset or custom); a deleted one falls back to Google.
    let engines = resolve_engines(&EngineConfig::from_settings(&out), None);
    out.search_engine =
        clean_default_engine(patch.get("searchEngine").and_then(Value::as_str), &engines).or_else(|| clean_default_engine(out.search_engine.as_deref(), &engines));
    // Downloads folder: absolute path; an empty string falls back to the system folder.
    match patch.get("downloadDir").and_then(Value::as_str) {
        Some("") => out.download_dir = None,
        Some(p) if Path::new(p).is_absolute() && p.len() < 4096 => out.download_dir = Some(p.to_string()),
        _ => {}
    }
    // Web pages: values from the choice lists only; anything else keeps the previous one.
    if let Some(v) = patch.get("fontSize").and_then(is_int).filter(|v| is_font_size(*v)) {
        out.font_size = Some(v);
    }
    if let Some(v) = patch.get("minimumFontSize").and_then(is_int).filter(|v| is_min_font_size(*v)) {
        out.minimum_font_size = Some(v);
    }
    if let Some(v) = patch.get("defaultZoom").and_then(is_int).filter(|v| is_zoom_percent(*v)) {
        out.default_zoom = Some(v);
    }
    // Global Privacy Control: on or missing (false is stored as missing, anything else keeps the previous value).
    match patch.get("globalPrivacyControl") {
        Some(Value::Bool(true)) => out.global_privacy_control = Some(true),
        Some(Value::Bool(false)) => out.global_privacy_control = None,
        _ => {}
    }
    out
}

/// Copies the validated fields of `cleaned` onto `target` (a field absent from `cleaned` is removed from `target`).
pub fn apply_cleaned(target: &mut Settings, cleaned: &Settings) {
    target.archive_after_hours = cleaned.archive_after_hours;
    target.tab_sleep_minutes = cleaned.tab_sleep_minutes;
    target.dev_aliases = cleaned.dev_aliases.clone();
    target.shortcuts = cleaned.shortcuts.clone();
    target.extension_order = cleaned.extension_order.clone();
    target.pinned_extensions = cleaned.pinned_extensions.clone();
    target.search_engine = cleaned.search_engine.clone();
    target.search_engines = cleaned.search_engines.clone();
    target.search_keywords = cleaned.search_keywords.clone();
    target.download_dir = cleaned.download_dir.clone();
    target.font_size = cleaned.font_size;
    target.minimum_font_size = cleaned.minimum_font_size;
    target.default_zoom = cleaned.default_zoom;
    target.global_privacy_control = cleaned.global_privacy_control;
}

/// Shortcut overrides helper for callers holding typed settings.
pub fn shortcut_overrides(s: &Settings) -> Option<&ShortcutOverrides> {
    s.shortcuts.as_ref()
}

/// Cleans bookmarks read from disk (shape, depth, lengths).
pub fn prune_bookmarks(items: &Value, depth: usize) -> Vec<Bookmark> {
    let Some(arr) = items.as_array() else { return Vec::new() };
    if depth > BOOKMARKS_MAX_DEPTH {
        return Vec::new();
    }
    arr.iter()
        .filter_map(|i| {
            if !i.is_object() {
                return None;
            }
            let id = i.get("id").and_then(Value::as_str).filter(|s| !s.is_empty()).map(String::from).unwrap_or_else(crate::ids::new_id);
            match i.get("kind").and_then(Value::as_str) {
                Some("link") => {
                    let url = i.get("url").and_then(Value::as_str).filter(|u| !u.is_empty())?;
                    Some(Bookmark::Link {
                        id,
                        url: url.to_string(),
                        title: i.get("title").and_then(Value::as_str).map(|t| t.chars().take(300).collect()).unwrap_or_else(|| url.to_string()),
                        favicon: i.get("favicon").and_then(Value::as_str).filter(|f| !f.is_empty()).map(String::from),
                    })
                }
                Some("folder") => Some(Bookmark::Folder {
                    id,
                    name: i.get("name").and_then(Value::as_str).filter(|n| !n.is_empty()).map(|n| n.chars().take(120).collect()).unwrap_or_else(|| t("defaults.folderFallback")),
                    items: prune_bookmarks(i.get("items").unwrap_or(&Value::Null), depth + 1),
                }),
                _ => None,
            }
        })
        .collect()
}

/// Typed value from a JSON object, field by field: a field of the wrong type is skipped and keeps its default.
fn lenient_struct<T: Serialize + for<'de> Deserialize<'de> + Default>(obj: &Map<String, Value>) -> T {
    if let Ok(v) = serde_json::from_value::<T>(Value::Object(obj.clone())) {
        return v;
    }
    let mut acc = match serde_json::to_value(T::default()) {
        Ok(Value::Object(m)) => m,
        _ => Map::new(),
    };
    for (k, v) in obj {
        let mut trial = acc.clone();
        trial.insert(k.clone(), v.clone());
        if serde_json::from_value::<T>(Value::Object(trial.clone())).is_ok() {
            acc = trial;
        }
    }
    serde_json::from_value(Value::Object(acc)).unwrap_or_default()
}

fn number_of(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        _ => None,
    }
}

fn clean_rect(v: Option<&Value>) -> Option<Rect> {
    let o = v?.as_object()?;
    let f = |k: &str| o.get(k).and_then(Value::as_f64).filter(|n| n.is_finite());
    Some(Rect { x: f("x")?, y: f("y")?, width: f("width")?, height: f("height")? })
}

fn truthy(v: Option<&Value>) -> bool {
    match v {
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|x| x != 0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(_)) | Some(Value::Object(_)) => true,
        _ => false,
    }
}

/// Straightens out a state read from disk (untyped JSON): missing fields, references to unknown tabs.
/// Returns None if `raw` is not an object.
fn sanitize(raw: &Value, system_languages: &[String]) -> Option<State> {
    let raw = raw.as_object()?;
    let base = initial_state(system_languages);
    let mut state = State { version: 1, ..base.clone() };

    // Settings: the defaults overlaid with the file, field by field.
    let raw_settings = raw.get("settings").and_then(Value::as_object);
    let mut merged = match serde_json::to_value(&base.settings) {
        Ok(Value::Object(m)) => m,
        _ => Map::new(),
    };
    if let Some(rs) = raw_settings {
        for (k, v) in rs {
            let mut trial = merged.clone();
            trial.insert(k.clone(), v.clone());
            if serde_json::from_value::<Settings>(Value::Object(trial.clone())).is_ok() {
                merged = trial;
            }
        }
    }
    state.settings = serde_json::from_value(Value::Object(merged.clone())).unwrap_or_else(|_| base.settings.clone());
    // State already on disk without the field (written before the introduction): the user does not need to see it again.
    if !raw_settings.and_then(|s| s.get("onboarded")).is_some_and(Value::is_boolean) {
        state.settings.onboarded = Some(true);
    }
    // Settings written by hand or restored: invalid values reset to the original ones.
    let cleaned = clean_settings(&merged, &base.settings);
    apply_cleaned(&mut state.settings, &cleaned);
    if !APP_ICONS.contains(&state.settings.app_icon.as_str()) {
        state.settings.app_icon = "aurore".into();
    }
    state.site_zoom = clean_site_zoom(raw.get("siteZoom").unwrap_or(&Value::Null));
    state.site_zoom_by_space = clean_site_zoom_by_space(raw.get("siteZoomBySpace").unwrap_or(&Value::Null));
    state.bounds = clean_rect(raw.get("bounds"));
    state.pip_bounds = clean_rect(raw.get("pipBounds"));
    // Migration: without saved languages, follow the system ones.
    state.settings.languages = normalize_languages(&state.settings.languages, system_languages);

    // Tabs.
    if let Some(tabs) = raw.get("tabs").and_then(Value::as_object) {
        for (id, tv) in tabs {
            let Some(t) = tv.as_object() else { continue };
            let s = |k: &str| t.get(k).and_then(Value::as_str);
            let mut tab = new_tab(s("url").unwrap_or(""), s("title").unwrap_or(""));
            tab.id = id.clone();
            tab.favicon = s("favicon").map(String::from);
            tab.pinned_url = s("pinnedUrl").map(String::from);
            tab.last_active = t.get("lastActive").and_then(Value::as_f64).map_or_else(now_ms, |n| n as i64);
            tab.muted = truthy(t.get("muted"));
            tab.custom_title = s("customTitle").map(String::from);
            tab.custom_icon = s("customIcon").map(String::from);
            state.tabs.insert(id.clone(), tab);
        }
    }
    let known_ids: HashSet<String> = state.tabs.keys().cloned().collect();
    let known = |id: &str| known_ids.contains(id);

    // Spaces.
    let mut spaces: Vec<Space> = Vec::new();
    let fallback = t("defaults.folderFallback");
    if let Some(list) = raw.get("spaces").and_then(Value::as_array) {
        for sv in list {
            let Some(so) = sv.as_object() else { continue };
            let mut rest = so.clone();
            let pinned = rest.remove("pinned").unwrap_or(Value::Null);
            let today = rest.remove("today").unwrap_or(Value::Null);
            let bookmarks = rest.remove("bookmarks");
            let groups = rest.remove("groups");
            let mut space: Space = lenient_struct(&rest);
            if space.id.is_empty() {
                space.id = crate::ids::new_id();
            }
            space.pinned = prune_items(&pinned, &known, true, &fallback);
            space.today = prune_items(&today, &known, false, &fallback);
            strip_groups(&mut space.pinned);
            // Group headers: unreadable ones are dropped (and recreated by `sync_groups` if the group has tabs).
            if let Some(g) = groups.as_ref().and_then(Value::as_object) {
                let mut map: IndexMap<String, TabGroup> = IndexMap::new();
                for (gid, gv) in g {
                    let Some(go) = gv.as_object() else { continue };
                    let (Some(name), Some(hue)) = (go.get("name").and_then(Value::as_str), go.get("hue").and_then(Value::as_f64).filter(|h| h.is_finite())) else { continue };
                    map.insert(
                        gid.clone(),
                        TabGroup {
                            name: name.to_string(),
                            icon: go.get("icon").and_then(Value::as_str).map(String::from),
                            hue,
                            open: go.get("open").and_then(Value::as_bool) != Some(false),
                        },
                    );
                }
                space.groups = Some(map);
            }
            sync_groups(&mut space, &state.tabs);
            if let Some(b) = &bookmarks {
                space.bookmarks = Some(prune_bookmarks(b, 0));
            }
            if space.active_tab_id.as_deref().is_some_and(|a| !known(a)) {
                space.active_tab_id = None;
            }
            spaces.push(space);
        }
    }
    if spaces.is_empty() {
        spaces = base.spaces.clone();
    }
    state.spaces = spaces;
    state.active_space_id = raw.get("activeSpaceId").and_then(Value::as_str).filter(|id| state.spaces.iter().any(|s| s.id == *id)).map(String::from).unwrap_or_else(|| state.spaces[0].id.clone());
    state.favorites = raw.get("favorites").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).filter(|id| known(id)).map(String::from).collect()).unwrap_or_default();
    state.sidebar_visible = raw.get("sidebarVisible").and_then(Value::as_bool).unwrap_or(true);
    let w = raw.get("sidebarWidth").and_then(number_of).filter(|n| n.is_finite() && *n != 0.0).unwrap_or(240.0);
    state.sidebar_width = w.clamp(SIDEBAR_MIN, SIDEBAR_MAX);
    state.archive = raw
        .get("archive")
        .and_then(Value::as_array)
        .map(|a| a.iter().take(ARCHIVE_MAX).map(|e| e.as_object().map(lenient_struct::<ArchivedTab>).unwrap_or_default()).collect())
        .unwrap_or_default();
    // A download left in progress when the app closed will not resume.
    state.downloads = raw
        .get("downloads")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_object)
                .take(DOWNLOADS_MAX)
                .map(|o| {
                    let mut d: DownloadInfo = lenient_struct(o);
                    if matches!(d.status, DownloadStatus::Progressing | DownloadStatus::Paused) {
                        d.status = DownloadStatus::Interrupted;
                    }
                    d
                })
                .collect()
        })
        .unwrap_or_default();
    if let Some(b) = raw.get("boosts").and_then(Value::as_object) {
        for (site, v) in b {
            if let Some(o) = v.as_object() {
                state.boosts.insert(site.clone(), lenient_struct(o));
            }
        }
    }
    if let Some(p) = raw.get("sitePermissions").and_then(Value::as_object) {
        for (origin, v) in p {
            let Some(o) = v.as_object() else { continue };
            let mut m = IndexMap::new();
            for (perm, d) in o {
                match d.as_str() {
                    Some("allow") => m.insert(perm.clone(), PermissionDecision::Allow),
                    Some("deny") => m.insert(perm.clone(), PermissionDecision::Deny),
                    _ => None,
                };
            }
            state.site_permissions.insert(origin.clone(), m);
        }
    }
    Some(state)
}

/// Tab fields written to `state.json` (the rest starts from scratch at the next launch).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PersistedTab<'a> {
    id: &'a str,
    url: &'a str,
    title: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    favicon: &'a Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pinned_url: &'a Option<String>,
    last_active: i64,
    muted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    custom_title: &'a Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    custom_icon: &'a Option<String>,
}

/// Options of `Store::open`.
#[derive(Debug, Clone, Default)]
pub struct StoreOptions {
    /// Languages of the system (BCP 47), used for the default site languages and the interface language.
    pub system_languages: Vec<String>,
    /// State handed over by a previous run of the same process (relaunch): used instead of reading `state.json`.
    pub startup_state: Option<Value>,
}

/// Persisted state plus the structures the model works on, with deferred atomic writes.
pub struct Store {
    pub data: ModelData,
    dir: PathBuf,
    file: PathBuf,
    history_file: PathBuf,
    bookmarks_file: PathBuf,
    /// True once bookmarks.json contains the bookmarks: only then can state.json do without them.
    bookmarks_persisted: bool,
    /// Space identifiers at the last serialization: a change forces bookmarks.json to be rewritten.
    space_key: String,
    /// Bookmarks of spaces unknown at load: rewritten as is in bookmarks.json.
    bookmark_orphans: Map<String, Value>,
    bookmarks_locked: bool,
    state_locked: bool,
    bookmarks_dirty: bool,
    history_dirty: bool,
    state_writer: AtomicWriter,
    bookmarks_writer: AtomicWriter,
    history_writer: AtomicWriter,
    state_deadline: Option<i64>,
    bookmarks_deadline: Option<i64>,
    history_deadline: Option<i64>,
}

impl Deref for Store {
    type Target = ModelData;
    fn deref(&self) -> &ModelData {
        &self.data
    }
}
impl DerefMut for Store {
    fn deref_mut(&mut self) -> &mut ModelData {
        &mut self.data
    }
}

fn aside(file: &Path) {
    let mut name = file.as_os_str().to_owned();
    name.push(format!(".corrupt-{}", now_ms()));
    let _ = std::fs::copy(file, PathBuf::from(name));
}

impl Store {
    /// Opens (or creates) the store in `dir`. Never fails: an unreadable or corrupt `state.json` is copied aside and
    /// the app starts from a fresh state; an unreadable one (I/O error) is left alone and not written this session.
    pub fn open(dir: &Path, opts: StoreOptions) -> Store {
        let _ = std::fs::create_dir_all(dir);
        let file = dir.join("state.json");
        let history_file = dir.join("tab-history.json");
        let bookmarks_file = dir.join("bookmarks.json");
        let mut store = Store {
            data: ModelData::default(),
            dir: dir.to_path_buf(),
            state_writer: AtomicWriter::new(&file),
            bookmarks_writer: AtomicWriter::new(&bookmarks_file),
            history_writer: AtomicWriter::new(&history_file),
            file,
            history_file,
            bookmarks_file,
            bookmarks_persisted: false,
            space_key: String::new(),
            bookmark_orphans: Map::new(),
            bookmarks_locked: false,
            state_locked: false,
            bookmarks_dirty: false,
            history_dirty: false,
            state_deadline: None,
            bookmarks_deadline: None,
            history_deadline: None,
        };
        let sys = opts.system_languages.clone();
        let mut legacy = Value::Null;
        let mut text: Option<String> = None;
        if opts.startup_state.is_none() {
            // I/O error (antivirus, lock...): a few retries, then state.json is no longer written during this session.
            for attempt in 0..4 {
                match read_text(&store.file) {
                    Ok(t) => {
                        text = Some(t);
                        store.state_locked = false;
                        break;
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                        store.state_locked = false;
                        break;
                    }
                    Err(_) => {
                        store.state_locked = true;
                        if attempt < 3 {
                            std::thread::sleep(std::time::Duration::from_millis(100));
                        }
                    }
                }
            }
        }
        let sys_refs: Vec<&str> = sys.iter().map(String::as_str).collect();
        let loaded: Result<State, bool> = (|| {
            if store.state_locked {
                return Err(false);
            }
            let raw: Value = match (&opts.startup_state, &text) {
                (Some(v), _) => v.clone(),
                (None, Some(t)) => serde_json::from_str(t).map_err(|_| true)?,
                (None, None) => return Err(false), // absent file
            };
            // Language first: `sanitize` names some default items.
            let pref = raw.get("settings").and_then(|s| s.get("uiLanguage")).and_then(|v| serde_json::from_value(v.clone()).ok());
            init_locale(pref, &sys_refs);
            legacy = raw.get("tabHistory").cloned().unwrap_or(Value::Null);
            sanitize(&raw, &sys).ok_or(true)
        })();
        match loaded {
            Ok(state) => store.data.state = state,
            Err(corrupt) => {
                // Invalid file: keep it aside before starting from scratch (never for a mere read error).
                if corrupt {
                    aside(&store.file);
                }
                init_locale(None, &sys_refs);
                store.data.state = initial_state(&sys);
                // Unreadable or corrupt file: not a new user, no introduction.
                if corrupt || store.state_locked {
                    store.data.state.settings.onboarded = Some(true);
                }
            }
        }
        let pref = store.data.state.settings.ui_language;
        init_locale(pref, &sys_refs);
        store.load_bookmarks();
        store.load_histories(&legacy);
        store
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Bookmarks: bookmarks.json (`{ [spaceId]: Bookmark[] }`) is authoritative for the spaces it mentions; a space
    /// absent from the file keeps the bookmarks from state.json (old format). In that case bookmarks.json is written
    /// right away: state.json only loses its bookmarks after that write succeeds.
    fn load_bookmarks(&mut self) {
        if self.state_locked {
            self.bookmarks_locked = true;
            return;
        }
        let mut saved = Map::new();
        let mut corrupt = false;
        match read_text(&self.bookmarks_file) {
            Ok(text) => match serde_json::from_str::<Value>(&text) {
                Ok(Value::Object(m)) => saved = m,
                _ => corrupt = true,
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) if e.kind() == std::io::ErrorKind::InvalidData => corrupt = true,
            Err(_) => {
                // I/O error: the file may be intact, leave it alone for this session and state.json keeps the bookmarks.
                self.bookmarks_locked = true;
                self.space_key = self.space_ids();
                return;
            }
        }
        if corrupt {
            aside(&self.bookmarks_file);
        }
        let mut migrate = corrupt;
        let ids: HashSet<String> = self.data.state.spaces.iter().map(|s| s.id.clone()).collect();
        for space in &mut self.data.state.spaces {
            match saved.get(&space.id) {
                Some(own) if own.is_array() => space.bookmarks = Some(prune_bookmarks(own, 0)),
                _ => {
                    if space.bookmarks.is_some() {
                        migrate = true; // bookmarks only in state.json
                    }
                }
            }
        }
        // Spaces unknown at load (e.g. state.json lost): kept as is and rewritten, never erased.
        for (id, own) in &saved {
            if !ids.contains(id) && own.is_array() {
                self.bookmark_orphans.insert(id.clone(), own.clone());
            }
        }
        if !self.bookmark_orphans.is_empty() {
            // A single copy, rewritten only if the orphans changed: no accumulation at each launch.
            let name = self.bookmarks_file.with_extension("").with_extension("orphans.json");
            if let Ok(json) = serde_json::to_string(&self.bookmark_orphans) {
                let same = read_text(&name).map(|t| t == json).unwrap_or(false);
                if !same {
                    let _ = write_file_durable(&name, json.as_bytes());
                }
            }
        }
        self.space_key = self.space_ids();
        self.bookmarks_persisted = !migrate;
        if migrate {
            self.bookmarks_dirty = true;
            self.bookmarks_persisted = self.write_bookmarks();
        }
    }

    fn space_ids(&self) -> String {
        self.data.state.spaces.iter().map(|s| s.id.as_str()).collect::<Vec<_>>().join(",")
    }

    /// Per-tab histories: tab-history.json, or failing that the old `tabHistory` field of state.json.
    fn load_histories(&mut self, legacy: &Value) {
        let mut saved: Value = Value::Null;
        let mut migrate = false;
        match read_text(&self.history_file) {
            Ok(text) => saved = serde_json::from_str(&text).unwrap_or(Value::Null),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                saved = legacy.clone();
                migrate = true;
            }
            Err(_) => {}
        }
        if let Some(obj) = saved.as_object() {
            for (id, h) in obj {
                if self.data.state.tabs.contains_key(id)
                    && let Some(v) = valid_history(h)
                {
                    self.data.histories.insert(id.clone(), v);
                }
            }
        }
        // Migration: write the new file right away, since state.json will no longer embed the histories.
        if migrate && !self.data.histories.is_empty() {
            self.history_dirty = true;
            self.write_history();
        }
    }

    // ----------------------------- Scheduling -----------------------------

    fn plan(slot: &mut Option<i64>, delay: i64, bring_forward: bool) {
        let at = now_ms() + delay;
        match slot {
            Some(d) if !bring_forward || *d <= at => {}
            _ => *slot = Some(at),
        }
    }

    /// Plans a state write `delay_ms` from now (use `SAVE_DELAY`; `LAZY_DELAY` for changes that can wait). A later
    /// request never pushes back an earlier deadline.
    pub fn schedule(&mut self, delay_ms: i64) {
        Self::plan(&mut self.state_deadline, delay_ms, true);
    }

    /// To call after any bookmark change (in addition to `schedule`): rewrites bookmarks.json.
    pub fn schedule_bookmarks(&mut self) {
        self.bookmarks_dirty = true;
        Self::plan(&mut self.bookmarks_deadline, SAVE_DELAY, false);
    }

    /// To call after a change to `histories`: rewrites tab-history.json.
    pub fn schedule_history(&mut self) {
        self.history_dirty = true;
        Self::plan(&mut self.history_deadline, HISTORY_DELAY, false);
    }

    /// Earliest planned deadline (ms since epoch), if any: the app can sleep until then.
    pub fn next_deadline(&self) -> Option<i64> {
        [self.state_deadline, self.bookmarks_deadline, self.history_deadline].into_iter().flatten().min()
    }

    /// True if a planned write is due at `now`.
    pub fn due(&self, now: i64) -> bool {
        self.next_deadline().is_some_and(|d| d <= now)
    }

    /// Writes what is due at `now`. Returns true if something was written (or attempted).
    pub fn tick(&mut self, now: i64) -> bool {
        let mut did = false;
        if self.bookmarks_deadline.is_some_and(|d| d <= now) {
            self.bookmarks_deadline = None;
            self.write_bookmarks();
            did = true;
        }
        if self.state_deadline.is_some_and(|d| d <= now) {
            self.state_deadline = None;
            self.write_state();
            did = true;
        }
        if self.history_deadline.is_some_and(|d| d <= now) {
            self.history_deadline = None;
            self.write_history();
            did = true;
        }
        did
    }

    /// Full synchronous write (app shutdown).
    pub fn flush(&mut self) {
        self.state_deadline = None;
        self.bookmarks_deadline = None;
        self.history_deadline = None;
        // Safety net: bookmarks are always rewritten (identical content is skipped), before the state.
        self.bookmarks_dirty = true;
        if self.write_bookmarks() && !self.bookmarks_locked {
            self.bookmarks_persisted = true;
        }
        self.write_state();
        self.history_dirty = true;
        self.write_history();
    }

    // ----------------------------- Serialization -----------------------------

    fn write_state(&mut self) -> bool {
        if self.state_locked {
            return true;
        }
        let payload = self.serialize_state();
        self.state_writer.write(payload)
    }

    fn write_bookmarks(&mut self) -> bool {
        let payload = self.serialize_bookmarks();
        let had = payload.is_some();
        let ok = self.bookmarks_writer.write(payload);
        if had && !ok {
            self.bookmarks_dirty = true; // disk unavailable: retry on the next change
        }
        ok
    }

    fn write_history(&mut self) -> bool {
        let payload = self.serialize_history();
        let had = payload.is_some();
        let ok = self.history_writer.write(payload);
        if had && !ok {
            self.history_dirty = true;
        }
        ok
    }

    fn serialize_state(&mut self) -> Option<String> {
        // Space created, deleted or imported: bookmarks.json must follow, even if the caller forgot `schedule_bookmarks`.
        let key = self.space_ids();
        if key != self.space_key {
            self.space_key = key;
            self.schedule_bookmarks();
        }
        let st = &self.data.state;
        let mut v = serde_json::to_value(st).ok()?;
        let obj = v.as_object_mut()?;
        // Only the persisted fields of tabs, detached ones left out.
        let mut tabs = Map::new();
        for (id, t) in &st.tabs {
            if self.data.detached.contains(id) {
                continue;
            }
            let p = PersistedTab {
                id,
                url: &t.url,
                title: &t.title,
                favicon: &t.favicon,
                pinned_url: &t.pinned_url,
                last_active: t.last_active,
                muted: t.muted,
                custom_title: &t.custom_title,
                custom_icon: &t.custom_icon,
            };
            tabs.insert(id.clone(), serde_json::to_value(p).ok()?);
        }
        obj.insert("tabs".into(), Value::Object(tabs));
        // Bookmarks live in bookmarks.json; private downloads are never written.
        if self.bookmarks_persisted
            && let Some(Value::Array(spaces)) = obj.get_mut("spaces")
        {
            for s in spaces {
                if let Some(o) = s.as_object_mut() {
                    o.remove("bookmarks");
                }
            }
        }
        let downloads: Vec<&DownloadInfo> = st.downloads.iter().filter(|d| d.private != Some(true)).collect();
        obj.insert("downloads".into(), serde_json::to_value(downloads).ok()?);
        serde_json::to_string(&v).ok()
    }

    fn serialize_bookmarks(&mut self) -> Option<String> {
        if self.bookmarks_locked || !self.bookmarks_dirty {
            return None;
        }
        self.bookmarks_dirty = false;
        let mut all = self.bookmark_orphans.clone();
        for s in &self.data.state.spaces {
            if let Some(b) = &s.bookmarks {
                all.insert(s.id.clone(), serde_json::to_value(b).ok()?);
            }
        }
        serde_json::to_string(&all).ok()
    }

    /// Histories: only persisted tabs, global `page_state` budget (most recent first).
    fn serialize_history(&mut self) -> Option<String> {
        if !self.history_dirty || self.state_locked {
            return None;
        }
        self.history_dirty = false;
        let st = &self.data.state;
        let mut ids: Vec<&String> = st.tabs.keys().filter(|id| !self.data.detached.contains(*id)).collect();
        ids.sort_by_key(|id| std::cmp::Reverse(st.tabs[*id].last_active));
        let mut budget = HISTORY_TOTAL_STATE_MAX;
        let mut out = Map::new();
        for id in ids {
            let Some(h) = self.data.histories.get(id) else { continue };
            let Some(trimmed) = trim_history(&h.entries, h.index as i64, budget.min(HISTORY_TAB_STATE_MAX)) else { continue };
            budget = budget.saturating_sub(trimmed.entries.iter().map(|e| e.page_state.as_ref().map_or(0, |s| s.chars().count())).sum());
            out.insert(id.clone(), serde_json::to_value(trimmed).ok()?);
        }
        serde_json::to_string(&out).ok()
    }

    /// True when `state.json` could not be read (I/O error): neither it nor the linked files are written.
    pub fn is_locked(&self) -> bool {
        self.state_locked
    }
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;

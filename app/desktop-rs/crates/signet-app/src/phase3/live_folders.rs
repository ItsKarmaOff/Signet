//! PHASE3: live folders (`signet_features::livefolders`, `feeds`), port of `main/features/livefolders/livefolders.ts`.
//!
//! A live folder is a pinned folder filled by a source (RSS or Atom feed, GitHub pull requests): one tab per item,
//! updated every 15 minutes (first update 8 s after startup). Fetching runs on worker threads (`UreqHttp`, the GitHub
//! token comes from the OS keychain); results go through a mailbox that `tick` and `web_event` drain on the main
//! thread, where the folder tree is updated. The tree rules live in `apply_entries`, a pure function.
//!
//! Deviations from Electron: `SignetApi::save_live_folder` is synchronous, so a new folder is created at once (named
//! after the host, renamed to the feed title when it arrives) and a bad source shows as the folder error instead of
//! an error in the form; the address and the token format are still checked synchronously. "Seen" is updated on the
//! next tick after a tab is activated (the model callback `Env::on_activated` has no access to the core).

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use indexmap::IndexMap;
use signet_core::factory::new_tab;
use signet_core::tree::{folder_by_id_mut, new_folder};
use signet_core::types::{FolderItem, Item, LiveFolder, LiveSource, TabInfo};
use signet_features::feeds::FeedEntry;
use signet_features::http::UreqHttp;
use signet_features::livefolders::{self as feat, Fetched, LiveFolders, FIRST_DELAY, REFRESH_EVERY};
use signet_features::secrets::KeyringStore;

use crate::browser::Core;

/// Result of one worker.
struct Done {
    folder_id: String,
    /// JSON of the source that was fetched: the result is dropped if the folder's source changed meanwhile.
    source_key: String,
    result: Result<Fetched, String>,
    /// First fill after creation or source change: everything is already "seen".
    first: bool,
}

/// State owned by this feature (a field of `Phase3`).
#[derive(Default)]
pub struct State {
    busy: HashSet<String>,
    inbox: Arc<Mutex<Vec<Done>>>,
    started: Option<Instant>,
    first_done: bool,
    next: Option<Instant>,
    /// Folders created without a title yet: renamed to the feed title on their first fill.
    untitled: HashSet<String>,
}

// ---------------------------------------------------------------------------------------------- pure logic

fn translate(key: &str, args: &[(&str, String)]) -> String {
    let a: Vec<(&str, &str)> = args.iter().map(|(k, v)| (*k, v.as_str())).collect();
    signet_core::i18n::t_args(key, &a)
}

/// Live folders of a pinned list, nested ones included.
pub(crate) fn live_folder_ids(items: &[Item]) -> Vec<String> {
    let mut out = Vec::new();
    for item in items {
        if let Item::Folder(f) = item {
            if f.live.is_some() {
                out.push(f.id.clone());
            }
            out.extend(live_folder_ids(&f.items));
        }
    }
    out
}

/// "New" dot on the folder's tabs whose address has not been opened yet.
pub(crate) fn mark_unread(folder: &FolderItem, tabs: &mut IndexMap<String, TabInfo>) {
    let seen: HashSet<&String> = folder.live.as_ref().and_then(|l| l.seen.as_ref()).map(|s| s.iter().collect()).unwrap_or_default();
    for item in &folder.items {
        if let Item::Tab(t) = item
            && let Some(tab) = tabs.get_mut(&t.tab_id)
        {
            let url = tab.pinned_url.clone().unwrap_or_else(|| tab.url.clone());
            tab.live_unread = Some(!seen.contains(&url));
        }
    }
}

/// Puts the items in the folder: one tab per address (existing tabs are kept), newest first. An item gone from the
/// source is removed unless its tab is displayed. Returns the ids of the tabs removed from the state (their views
/// must be closed by the caller).
pub(crate) fn apply_entries(folder: &mut FolderItem, tabs: &mut IndexMap<String, TabInfo>, active: Option<&str>, shown: &HashSet<String>, all: &[FeedEntry], first: bool, now: i64) -> Vec<String> {
    let entries = feat::select_entries(all);
    let urls: Vec<String> = entries.iter().map(|e| e.url.clone()).collect();

    let mut by_url: IndexMap<String, Item> = IndexMap::new();
    let mut others: Vec<Item> = Vec::new();
    for item in std::mem::take(&mut folder.items) {
        match &item {
            Item::Tab(t) if tabs.contains_key(&t.tab_id) => {
                let tab = &tabs[&t.tab_id];
                by_url.insert(tab.pinned_url.clone().unwrap_or_else(|| tab.url.clone()), item);
            }
            _ => others.push(item),
        }
    }
    let mut next: Vec<Item> = Vec::new();
    for e in &entries {
        if let Some(existing) = by_url.shift_remove(&e.url) {
            if let Item::Tab(t) = &existing
                && let Some(tab) = tabs.get_mut(&t.tab_id)
                && !tab.loaded
            {
                // Source title until the page has loaded (it then provides its own).
                tab.title = e.title.clone();
            }
            next.push(existing);
        } else {
            let mut tab = new_tab(&e.url, &e.title);
            tab.pinned_url = Some(e.url.clone());
            let id = tab.id.clone();
            tabs.insert(id.clone(), tab);
            next.push(Item::tab(id));
        }
    }
    // Gone from the source: kept only if displayed (active tab, panel of a split view).
    let mut forgotten = Vec::new();
    for (_, item) in by_url {
        if let Item::Tab(t) = &item {
            if active == Some(t.tab_id.as_str()) || shown.contains(&t.tab_id) {
                next.push(item);
            } else {
                tabs.shift_remove(&t.tab_id);
                forgotten.push(t.tab_id.clone());
            }
        }
    }
    next.extend(others);
    folder.items = next;
    if let Some(live) = folder.live.as_mut() {
        let previous = live.seen.take().unwrap_or_default();
        live.seen = Some(feat::next_seen(&previous, &urls, first));
        live.refreshed_at = Some(now);
        live.error = None;
        live.refreshing = None;
    }
    mark_unread(folder, tabs);
    forgotten
}

// ---------------------------------------------------------------------------------------------- core side

impl Core {
    /// Folder with this id in any space.
    fn live_folder_mut(&mut self, id: &str) -> Option<&mut FolderItem> {
        self.store.data.state.spaces.iter_mut().find_map(|s| folder_by_id_mut(&mut s.pinned, id))
    }

    fn live_ids(&self) -> Vec<String> {
        self.state().spaces.iter().flat_map(|s| live_folder_ids(&s.pinned)).collect()
    }

    /// Reads the source on a worker thread; the result lands in the mailbox.
    fn live_spawn(&mut self, folder_id: &str, source: LiveSource, first: bool) {
        let inbox = self.phase3.live_folders.inbox.clone();
        let (id, key) = (folder_id.to_string(), serde_json::to_string(&source).unwrap_or_default());
        self.phase3.live_folders.busy.insert(id.clone());
        std::thread::spawn(move || {
            let lf = LiveFolders { http: &UreqHttp, secrets: &KeyringStore, t: &translate };
            let result = lf.fetch_source(&source);
            if let Ok(mut q) = inbox.lock() {
                q.push(Done { folder_id: id, source_key: key, result, first });
            }
        });
    }

    pub fn save_live_folder(&mut self, folder_id: Option<&str>, source: LiveSource) -> Option<String> {
        let lf = LiveFolders { http: &UreqHttp, secrets: &KeyringStore, t: &translate };
        let raw = serde_json::to_value(&source).unwrap_or_default();
        let Some(source) = lf.normalize(&raw) else { return Some(translate("liveFolders.error.address", &[])) };
        let live = LiveFolder { source: source.clone(), refreshed_at: None, error: None, refreshing: Some(true), seen: Some(Vec::new()) };
        let id = match folder_id {
            Some(id) => {
                let Some(f) = self.live_folder_mut(id) else { return None };
                f.live = Some(live);
                id.to_string()
            }
            None => {
                let name: String = lf.folder_name(&source, "").chars().take(80).collect();
                let name = if name.is_empty() { signet_core::i18n::t("defaults.folder") } else { name };
                let mut folder = new_folder(&name);
                folder.live = Some(live);
                let id = folder.id.clone();
                let idx = signet_core::model::active_space_idx(self.state());
                self.store.data.state.spaces[idx].pinned.insert(0, Item::Folder(folder));
                if matches!(source, LiveSource::Feed { .. }) {
                    self.phase3.live_folders.untitled.insert(id.clone());
                }
                id
            }
        };
        self.live_spawn(&id, source, true);
        self.changed();
        None
    }

    pub fn refresh_live_folder(&mut self, folder_id: &str) {
        if self.phase3.live_folders.busy.contains(folder_id) {
            return;
        }
        let Some(source) = self.live_folder_mut(folder_id).and_then(|f| f.live.as_mut()).map(|l| {
            l.refreshing = Some(true);
            l.source.clone()
        }) else {
            return;
        };
        self.live_spawn(folder_id, source, false);
        self.changed_view();
    }

    pub fn has_github_token(&mut self) -> bool {
        LiveFolders { http: &UreqHttp, secrets: &KeyringStore, t: &translate }.has_token()
    }

    /// Checks the token with GitHub (blocking, 15 s at most) then stores it in the keychain; error message or `None`.
    pub fn set_github_token(&mut self, token: Option<&str>) -> Option<String> {
        let err = LiveFolders { http: &UreqHttp, secrets: &KeyringStore, t: &translate }.set_token(token);
        if err.is_none() && token.is_some() {
            // GitHub folders waiting for a token are filled right away.
            for id in self.live_ids() {
                if self.live_folder_mut(&id).and_then(|f| f.live.as_ref()).is_some_and(|l| matches!(l.source, LiveSource::Github { .. })) {
                    self.refresh_live_folder(&id);
                }
            }
        }
        err
    }

    /// Takes the finished fetches and updates the folders.
    fn live_drain(&mut self) {
        let done: Vec<Done> = self.phase3.live_folders.inbox.lock().map(|mut q| std::mem::take(&mut *q)).unwrap_or_default();
        if done.is_empty() {
            return;
        }
        let now = signet_core::ids::now_ms();
        let shown: HashSet<String> = self.panes.iter().map(|p| p.tab_id.clone()).collect();
        let mut persist = false;
        for d in done {
            self.phase3.live_folders.busy.remove(&d.folder_id);
            let untitled = self.phase3.live_folders.untitled.remove(&d.folder_id);
            let active = self.active_tab_id();
            let st = &mut self.store.data.state;
            let Some(folder) = st.spaces.iter_mut().find_map(|s| folder_by_id_mut(&mut s.pinned, &d.folder_id)) else { continue };
            let Some(live) = folder.live.as_mut() else { continue };
            live.refreshing = None;
            // Source changed during the request: the result no longer applies to it.
            if serde_json::to_string(&live.source).unwrap_or_default() != d.source_key {
                continue;
            }
            match d.result {
                Ok(f) => {
                    if untitled && !f.title.trim().is_empty() {
                        folder.name = f.title.trim().chars().take(80).collect();
                    }
                    let gone = apply_entries(folder, &mut st.tabs, active.as_deref(), &shown, &f.entries, d.first, now);
                    persist = true;
                    for id in gone {
                        self.views.destroy(&id);
                    }
                }
                Err(msg) => {
                    live.error = Some(msg);
                    persist = true;
                }
            }
        }
        if persist {
            self.changed();
        } else {
            self.changed_view();
        }
    }

    /// The displayed tab comes from a live folder: it is no longer "new".
    fn live_mark_active_seen(&mut self) {
        let Some(id) = self.active_tab_id() else { return };
        let Some(tab) = self.state().tabs.get(&id) else { return };
        if tab.live_unread != Some(true) {
            return;
        }
        let url = tab.pinned_url.clone().unwrap_or_else(|| tab.url.clone());
        let mut touched = false;
        for fid in self.live_ids() {
            let holds = self.live_folder_mut(&fid).is_some_and(|f| f.items.iter().any(|i| matches!(i, Item::Tab(t) if t.tab_id == id)));
            if !holds {
                continue;
            }
            if let Some(live) = self.live_folder_mut(&fid).and_then(|f| f.live.as_mut()) {
                let mut seen = live.seen.take().unwrap_or_default();
                feat::mark_seen(&mut seen, &url);
                live.seen = Some(seen);
                touched = true;
            }
        }
        if touched {
            if let Some(t) = self.store.data.state.tabs.get_mut(&id) {
                t.live_unread = Some(false);
            }
            self.changed();
        }
    }

    fn live_refresh_all(&mut self) {
        for id in self.live_ids() {
            self.refresh_live_folder(&id);
        }
    }
}

// ---------------------------------------------------------------------------------------------- hooks

/// Called once after the browser core exists and the web layer is initialized: "new" dots now, first update later.
pub(super) fn startup(core: &mut Core) {
    for id in core.live_ids() {
        let tabs = &mut core.store.data.state.tabs;
        if let Some(f) = core.store.data.state.spaces.iter().find_map(|s| signet_core::tree::folder_by_id(&s.pinned, &id)) {
            mark_unread(f, tabs);
        }
    }
    core.phase3.live_folders.started = Some(Instant::now());
    core.changed_view();
}

/// Called for every CEF event, before the core handles it.
pub(super) fn web_event(core: &mut Core, _ev: &signet_web::WebEvent) {
    // Events arrive often: draining a mailbox is cheap and keeps the results from waiting for the next tick.
    if core.phase3.live_folders.busy.is_empty() {
        return;
    }
    core.live_drain();
}

/// Called on every core tick (about once per second or at the next store deadline).
pub(super) fn tick(core: &mut Core) {
    core.live_drain();
    core.live_mark_active_seen();
    let Some(started) = core.phase3.live_folders.started else { return };
    let now = Instant::now();
    if !core.phase3.live_folders.first_done {
        if now.duration_since(started) >= FIRST_DELAY {
            core.phase3.live_folders.first_done = true;
            core.phase3.live_folders.next = Some(now + REFRESH_EVERY);
            core.live_refresh_all();
        }
    } else if core.phase3.live_folders.next.is_some_and(|n| now >= n) {
        core.phase3.live_folders.next = Some(now + REFRESH_EVERY);
        core.live_refresh_all();
    }
}

/// Called before quitting (flush files, stop threads). Workers are detached and end on their own (15 s timeout).
pub(super) fn quit(core: &mut Core) {
    core.phase3.live_folders.busy.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(url: &str, title: &str, date: i64) -> FeedEntry {
        FeedEntry { url: url.into(), title: title.into(), date }
    }

    fn live_folder() -> FolderItem {
        let mut f = new_folder("F");
        f.live = Some(LiveFolder { source: LiveSource::Feed { url: "https://x.com/feed".into() }, refreshed_at: None, error: Some("old".into()), refreshing: Some(true), seen: Some(vec![]) });
        f
    }

    #[test]
    fn first_fill_creates_seen_tabs_newest_first() {
        let mut f = live_folder();
        let mut tabs = IndexMap::new();
        let all = vec![entry("https://a", "A", 1), entry("https://b", "B", 3), entry("https://a", "dup", 2)];
        let gone = apply_entries(&mut f, &mut tabs, None, &HashSet::new(), &all, true, 42);
        assert!(gone.is_empty());
        assert_eq!(tabs.len(), 2);
        let urls: Vec<String> = f.items.iter().filter_map(|i| if let Item::Tab(t) = i { Some(tabs[&t.tab_id].url.clone()) } else { None }).collect();
        assert_eq!(urls, vec!["https://b", "https://a"]);
        assert!(tabs.values().all(|t| t.live_unread == Some(false) && t.pinned_url.is_some()));
        let live = f.live.as_ref().unwrap();
        assert_eq!((live.refreshed_at, live.error.clone(), live.refreshing), (Some(42), None, None));
        assert_eq!(live.seen.as_ref().unwrap().len(), 2);
    }

    #[test]
    fn update_keeps_tabs_marks_new_and_drops_gone() {
        let mut f = live_folder();
        let mut tabs = IndexMap::new();
        apply_entries(&mut f, &mut tabs, None, &HashSet::new(), &[entry("https://a", "A", 1), entry("https://b", "B", 2)], true, 1);
        let a_id = tabs.values().find(|t| t.url == "https://a").unwrap().id.clone();
        let b_id = tabs.values().find(|t| t.url == "https://b").unwrap().id.clone();
        // b disappears (but is the active tab), c is new, a stays.
        let gone = apply_entries(&mut f, &mut tabs, Some(&b_id), &HashSet::new(), &[entry("https://a", "A2", 1), entry("https://c", "C", 5)], false, 2);
        assert!(gone.is_empty() && tabs.contains_key(&b_id) && tabs.contains_key(&a_id));
        let c = tabs.values().find(|t| t.url == "https://c").unwrap();
        assert_eq!(c.live_unread, Some(true));
        assert_eq!(tabs[&a_id].live_unread, Some(false));
        assert_eq!(tabs[&a_id].title, "A2");
        // Next update with nothing displayed: b is removed.
        let gone = apply_entries(&mut f, &mut tabs, None, &HashSet::new(), &[entry("https://a", "A", 1), entry("https://c", "C", 5)], false, 3);
        assert_eq!(gone, vec![b_id.clone()]);
        assert!(!tabs.contains_key(&b_id));
        assert_eq!(f.items.len(), 2);
    }

    #[test]
    fn other_items_are_kept_after_the_entries() {
        let mut f = live_folder();
        f.items.push(Item::Folder(new_folder("inner")));
        let mut tabs = IndexMap::new();
        apply_entries(&mut f, &mut tabs, None, &HashSet::new(), &[entry("https://a", "A", 0)], true, 1);
        assert!(matches!(f.items[0], Item::Tab(_)) && matches!(f.items[1], Item::Folder(_)));
    }

    #[test]
    fn ids_of_nested_live_folders() {
        let mut inner = live_folder();
        inner.id = "in".into();
        let mut outer = new_folder("o");
        outer.id = "out".into();
        outer.items.push(Item::Folder(inner));
        let ids = live_folder_ids(&[Item::Folder(outer), Item::Folder(live_folder())]);
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&"in".to_string()));
    }
}

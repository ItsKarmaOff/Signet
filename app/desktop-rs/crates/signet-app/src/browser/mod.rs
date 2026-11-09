//! Port of `browser/browser.ts`: the composition root of the browser. `Core` owns the store, the history, the CEF
//! views and every piece of window state; the other modules of this folder add behavior to it (`impl Core`), one per
//! TypeScript file. `Browser` is the thin handle the interface holds (`Rc<dyn SignetApi>`).
//!
//! Everything runs on the GPUI main thread. `Core` is in a `RefCell`; its methods never call back into the API, and
//! UI events go through the `Bus` (a channel), so there is no re-entrancy.

pub mod api;
pub mod bus;
pub mod commands;
pub mod context_menu;
pub mod downloads;
pub mod events;
pub mod find;
pub mod folders;
pub mod layout;
pub mod overlay;
pub mod palette;
pub mod quit;
pub mod settings;
pub mod shell;
pub mod sleep;
pub mod spaces;
pub mod splitview;
pub mod tabs;
pub mod undo;
pub mod views;
pub mod win;

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use futures::channel::mpsc::UnboundedSender;
use signet_core::api::{ArchivedNotice, UiEvent};
use signet_core::core::history::History;
use signet_core::core::store::{State, Store, LAZY_DELAY, SAVE_DELAY};
use signet_core::ids::{new_id, now_ms};
use signet_core::model::{Model, ModelEnv};
use signet_core::types::{Rect, Snapshot, StatePatch};
use signet_core::urls::SearchState;

use crate::windows::{Cmd, Desired};
use bus::Bus;
use views::Views;

/// Table of the app shortcuts the key filter checks while a page has focus (rebuilt when shortcuts change).
pub type KeyTable = Arc<Mutex<HashMap<String, String>>>;

/// Main window facts the browser needs.
pub struct MainWin {
    pub size: (f64, f64),
    /// Screen position of the content area (top left).
    pub origin: (f64, f64),
    pub fullscreen: bool,
    pub active: bool,
    /// Set once GPUI started its first frame: until then no page view is created, so the window shows up first.
    pub first_frame: bool,
}

/// Overlay state (`overlay.ts`).
#[derive(Default)]
pub struct OverlayState {
    pub payload: Option<signet_core::types::OverlayPayload>,
    pub ready: bool,
    pub leaving: bool,
    pub peek_tab: Option<String>,
}

#[derive(Default)]
pub struct FindData {
    pub tab_id: Option<String>,
    pub state: signet_core::types::FindState,
    pub session: u64,
    pub request: i32,
}

/// What an "undo" announcement can put back.
pub enum UndoAction {
    Archive(signet_core::model::ArchiveUndo),
    Removed(signet_core::model::RemovedUndo),
    Space(signet_core::model::SpaceUndo),
}

#[derive(Default)]
pub struct Notices {
    pub next: u64,
    pub current: Option<(ArchivedNotice, UndoAction)>,
}

/// Pending broadcast work (`broadcast.ts`).
#[derive(Default)]
pub struct Dirty {
    pub layout: bool,
    pub persist: bool,
    pub fast: bool,
    pub needs_full: bool,
    pub patches: HashMap<String, serde_json::Map<String, serde_json::Value>>,
    pub soft_scheduled: bool,
    pub menu_key: String,
    pub sent_tabs: std::collections::HashSet<String>,
}

#[derive(Default)]
pub struct Hover {
    pub sidebar: bool,
    pub toolbar: bool,
}

/// Quit confirmation state (`app/quit.ts`).
#[derive(Default)]
pub struct QuitState {
    pub hud: signet_core::types::QuitHudState,
    pub shown: bool,
    pub confirmed: bool,
}

pub struct Core {
    pub store: Store,
    pub history: History,
    pub search: SearchState,
    pub bus: Bus,
    pub cmds: UnboundedSender<Cmd>,
    pub views: Views,
    pub win: Option<MainWin>,
    pub panes: Vec<signet_core::types::Pane>,
    pub dividers: Vec<signet_core::types::Divider>,
    pub content_area: Rect,
    pub shown_tabs: Vec<String>,
    pub live_sidebar_width: Option<f64>,
    pub desired: Desired,
    pub dirty: Dirty,
    pub ov: OverlayState,
    pub find: FindData,
    pub notices: Notices,
    pub downloads: downloads::Downloads,
    pub fullscreen: Option<String>,
    pub hover: Hover,
    pub quit: QuitState,
    pub quit_hold: quit::Hold,
    pub quit_last_cmd: i64,
    pub quit_timings: Option<std::sync::mpsc::Receiver<(Option<i64>, Option<i64>)>>,
    pub recording_shortcuts: bool,
    pub debug: bool,
    pub data_dir: PathBuf,
    pub key_table: KeyTable,
    pub hidden_since: HashMap<String, i64>,
    pub status_url: String,
    pub dragging: bool,
    pub zoom: HashMap<String, f64>,
    /// Typed navigations waiting for their commit, per tab.
    pub typed: HashMap<String, String>,
    /// Addresses promoted to https:// from a typed input: http fallback possible if they fail.
    pub upgraded: HashMap<String, String>,
    pub last_switch: Option<(String, String, i64)>,
    pub started: std::time::Instant,
    pub quitting: bool,
    /// The module quit hooks have run (they run once, before the flush and the view teardown).
    pub quit_hooks_ran: bool,
    pub settings_section: Option<String>,
    /// Windows other than the main one that are open (by key).
    pub open_windows: std::collections::HashSet<String>,
    pub phase3: crate::phase3::Phase3,
    pub focus_pending: bool,
    /// URLs received before the main window's first frame, opened right after it.
    pub pending_urls: Vec<String>,
    pub preview: Option<Rect>,
    pub suggest: palette::SuggestCache,
    /// Slide, Peek and overlay window state of the layout (see `layout.rs`).
    pub lx: layout::LayoutExtra,
    /// Self-test only (`SIGNET_SELFTEST`): web events seen, as text.
    pub trace: std::rc::Rc<std::cell::RefCell<Option<Vec<String>>>>,
}

/// Effects the model needs (port of the `Model` constructor options in `browser.ts`).
pub struct Env<'a> {
    pub views: &'a mut Views,
    /// Histories copied by `save_history`, committed to the store after the rule ran.
    pub saved: Vec<(String, signet_core::core::store::TabHistory)>,
}

impl ModelEnv for Env<'_> {
    fn new_id(&mut self) -> String {
        new_id()
    }
    fn now(&mut self) -> i64 {
        now_ms()
    }
    fn default_folder_name(&mut self) -> String {
        signet_core::i18n::t("defaults.folder")
    }
    fn default_space_name(&mut self) -> String {
        signet_core::i18n::t("defaults.space")
    }
    fn on_activated(&mut self, _space: &signet_core::types::Space, _tab_id: &str) {
        // PHASE3: live folders mark the tab as seen (`liveFolders.onActivated`).
    }
    fn destroy_view(&mut self, state: &mut State, tab_id: &str) {
        self.views.destroy(tab_id);
        if let Some(t) = state.tabs.get_mut(tab_id) {
            t.loaded = false;
            t.loading = false;
            t.audible = false;
            t.can_go_back = false;
            t.can_go_forward = false;
        }
    }
    fn save_history(&mut self, tab_id: &str) {
        if let Some(h) = crate::phase3::data::capture(self.views, tab_id) {
            self.saved.push((tab_id.to_string(), h));
        }
    }
    fn reconcile_sessions(&mut self, tab_ids: &[String]) {
        for id in tab_ids {
            self.views.destroy(id);
        }
    }
}

impl Core {
    pub fn new(store: Store, history: History, cmds: UnboundedSender<Cmd>, data_dir: PathBuf, key_table: KeyTable, debug: bool) -> Core {
        let mut search = SearchState::default();
        let s = &store.data.state.settings;
        search.set_search_engine(s.search_engine.as_deref());
        search.set_search_engines(&signet_core::searchengines::EngineConfig::from_settings(s));
        search.set_language(s.languages.first().map(String::as_str));
        search.set_search_locale(s.languages.first().map(String::as_str).unwrap_or("en"));
        let downloads = downloads::Downloads::new(store.data.state.downloads.clone());
        Core {
            store,
            history,
            search,
            bus: Bus::default(),
            cmds,
            views: Views::default(),
            win: None,
            panes: Vec::new(),
            dividers: Vec::new(),
            content_area: Rect::default(),
            shown_tabs: Vec::new(),
            live_sidebar_width: None,
            desired: Desired::default(),
            dirty: Dirty { needs_full: true, ..Default::default() },
            ov: OverlayState::default(),
            find: FindData::default(),
            notices: Notices::default(),
            downloads,
            fullscreen: None,
            hover: Hover::default(),
            quit: QuitState::default(),
            quit_hold: quit::Hold::new(),
            quit_last_cmd: 0,
            quit_timings: None,
            recording_shortcuts: false,
            debug,
            data_dir,
            key_table,
            hidden_since: HashMap::new(),
            status_url: String::new(),
            dragging: false,
            zoom: HashMap::new(),
            typed: HashMap::new(),
            upgraded: HashMap::new(),
            last_switch: None,
            started: std::time::Instant::now(),
            quitting: false,
            quit_hooks_ran: false,
            settings_section: None,
            open_windows: Default::default(),
            phase3: Default::default(),
            focus_pending: false,
            pending_urls: Vec::new(),
            preview: None,
            suggest: Default::default(),
            lx: Default::default(),
            trace: Default::default(),
        }
    }

    pub fn log(&self, msg: impl AsRef<str>) {
        if self.debug {
            println!("[{:>7.2}s] {}", self.started.elapsed().as_secs_f64(), msg.as_ref());
        }
    }

    /// Self-test trace line (no-op unless `SIGNET_SELFTEST` is set).
    pub fn note(&self, line: impl Into<String>) {
        if let Some(t) = self.trace.borrow_mut().as_mut() {
            let mut line: String = line.into();
            line.truncate(400);
            println!("[trace] {line}");
            t.push(line);
        }
    }

    pub fn emit(&self, e: UiEvent) {
        if self.trace.borrow().is_some() {
            match &e {
                UiEvent::SidebarSlide(v) => self.note(format!("slide {v:.3}")),
                UiEvent::OverlayShow(_) => self.note("overlay-show"),
                _ => {}
            }
        }
        self.bus.emit(e);
    }

    pub fn send(&self, c: Cmd) {
        let _ = self.cmds.unbounded_send(c);
    }

    /// Runs a rule of the model with the view effects wired in.
    pub fn model<R>(&mut self, f: impl FnOnce(&mut Model) -> R) -> R {
        let mut env = Env { views: &mut self.views, saved: Vec::new() };
        let mut m = Model::new(&mut self.store.data, &mut env);
        let r = f(&mut m);
        let saved = std::mem::take(&mut env.saved);
        if !saved.is_empty() {
            self.commit_histories(saved);
        }
        r
    }

    pub fn state(&self) -> &State {
        &self.store.data.state
    }

    pub fn active_tab_id(&self) -> Option<String> {
        signet_core::model::active_space(self.state()).active_tab_id.clone()
    }

    pub fn active_space(&self) -> &signet_core::types::Space {
        signet_core::model::active_space(self.state())
    }

    pub fn hue(&self) -> f64 {
        self.active_space().hue
    }

    pub fn horizontal(&self) -> bool {
        signet_core::model::is_horizontal(self.state())
    }

    pub fn sidebar_right(&self) -> bool {
        signet_core::model::is_sidebar_right(self.state())
    }

    pub fn sidebar_on(&self) -> bool {
        signet_core::model::is_sidebar_on(self.state())
    }

    pub fn bookmarks_visible(&self) -> bool {
        self.state().settings.show_bookmarks_bar != Some(false)
    }

    // ------------------------------------------------------------------ broadcast

    /// A gesture changed the state: relayout, send the state, schedule the save.
    pub fn changed(&mut self) {
        self.dirty.layout = true;
        self.dirty.persist = true;
        self.dirty.fast = true;
        self.dirty.needs_full = true;
        self.apply();
    }

    /// Like `changed` for state that is not persisted (overlay open, hover...).
    pub fn changed_view(&mut self) {
        self.dirty.layout = true;
        self.dirty.needs_full = true;
        self.apply();
    }

    /// Engine event on a tab: patch the fields, batch the broadcast over 32 ms.
    pub fn patch_tab(&mut self, tab_id: &str, fields: serde_json::Map<String, serde_json::Value>, persisted: bool, lazy: bool) {
        let Some(tab) = self.store.data.state.tabs.get_mut(tab_id) else { return };
        let Ok(serde_json::Value::Object(mut cur)) = serde_json::to_value(&*tab) else { return };
        let mut changed = false;
        for (k, v) in &fields {
            if cur.get(k) != Some(v) && !(v.is_null() && !cur.contains_key(k)) {
                cur.insert(k.clone(), v.clone());
                changed = true;
            }
        }
        if !changed {
            return;
        }
        if let Ok(next) = serde_json::from_value(serde_json::Value::Object(cur)) {
            *tab = next;
        }
        let entry = self.dirty.patches.entry(tab_id.to_string()).or_default();
        for (k, v) in fields {
            entry.insert(k, v);
        }
        if persisted {
            self.dirty.persist = true;
            if !lazy {
                self.dirty.fast = true;
            }
        }
        if !self.dirty.soft_scheduled {
            self.dirty.soft_scheduled = true;
            self.send(Cmd::SoftApply);
        }
    }

    /// Convenience: patch one JSON field.
    pub fn patch_one(&mut self, tab_id: &str, key: &str, value: serde_json::Value, persisted: bool, lazy: bool) {
        let mut m = serde_json::Map::new();
        m.insert(key.to_string(), value);
        self.patch_tab(tab_id, m, persisted, lazy);
    }

    pub fn snapshot(&self) -> Snapshot {
        let st = self.state();
        let tabs = st.tabs.iter().filter(|(id, _)| !self.store.data.detached.contains(*id)).map(|(k, v)| (k.clone(), v.clone())).collect();
        let spaces = st
            .spaces
            .iter()
            .map(|s| if s.id == st.active_space_id || s.bookmarks.is_none() { s.clone() } else { signet_core::types::Space { bookmarks: None, ..s.clone() } })
            .collect();
        let displayed = signet_core::model::displayed_tab_ids(st, self.active_space());
        Snapshot {
            tabs,
            favorites: st.favorites.clone(),
            spaces,
            active_space_id: st.active_space_id.clone(),
            sidebar_visible: self.sidebar_on() || self.lx.progress.is_some(),
            sidebar_width: self.live_sidebar_width.unwrap_or(st.sidebar_width),
            sidebar_slide: self.slide_plan(),
            panes: self.panes.clone(),
            settings: st.settings.clone(),
            downloads: self.downloads.list(),
            sidebar_floating: self.desired.sidebar.is_some() && !self.sidebar_on(),
            toolbar_floating: self.desired.toolbar.is_some() && st.settings.compact_mode,
            dividers: self.dividers.clone(),
            media: self.phase3.media_for(&displayed),
            boosted_sites: st.boosts.values().filter(|b| b.enabled).map(|b| b.site.clone()).collect(),
            overlay_open: self.ov.payload.is_some() || self.ov.leaving,
        }
    }

    /// Runs the pending cycle: group invariant, layout, state to the UIs, deferred save.
    pub fn apply(&mut self) {
        self.dirty.soft_scheduled = false;
        let spaces_len = self.store.data.state.spaces.len();
        for i in 0..spaces_len {
            let tabs = self.store.data.state.tabs.clone();
            signet_core::tree::sync_groups(&mut self.store.data.state.spaces[i], &tabs);
        }
        if self.dirty.layout {
            self.dirty.layout = false;
            self.layout_update();
        }
        let persist = std::mem::take(&mut self.dirty.persist);
        let fast = std::mem::take(&mut self.dirty.fast);
        let patches = std::mem::take(&mut self.dirty.patches);
        let only_patches = !self.dirty.needs_full && !patches.is_empty() && patches.keys().all(|id| self.dirty.sent_tabs.contains(id));
        if only_patches {
            let tabs = patches.into_iter().map(|(k, v)| (k, serde_json::Value::Object(v))).collect();
            self.emit(UiEvent::StatePatch(StatePatch { tabs }));
        } else if self.dirty.needs_full || !patches.is_empty() {
            self.dirty.needs_full = false;
            let snap = self.snapshot();
            self.dirty.sent_tabs = snap.tabs.keys().cloned().collect();
            self.emit(UiEvent::State(Box::new(snap)));
        }
        let key = self.menu_key();
        if key != self.dirty.menu_key {
            self.dirty.menu_key = key;
            self.send(Cmd::RebuildMenu);
        }
        self.sync_surfaces();
        if persist {
            self.store.schedule(if fast { SAVE_DELAY } else { LAZY_DELAY });
        }
    }

    /// States that check View menu items: the menu is rebuilt only when this changes.
    pub fn menu_key(&self) -> String {
        let s = &self.state().settings;
        // The Develop menu follows developer mode and the settings of the active tab.
        let develop = match (s.dev_mode == Some(true), self.active_tab_id()) {
            (true, Some(tab)) => format!("{tab}{:?}{}", self.dev_overrides(&tab), self.views.view_of(&tab).is_some()),
            (dev, _) => dev.to_string(),
        };
        format!(
            "{}|{}|{}|{}|{}|{}|{}|{}",
            self.sidebar_on(),
            s.compact_mode,
            self.horizontal(),
            self.bookmarks_visible(),
            serde_json::to_string(&s.shortcuts).unwrap_or_default(),
            signet_core::i18n::current_locale().id(),
            develop,
            self.mods_all_disabled()
        )
    }

    /// Declares the floating surfaces the window manager must show.
    pub fn sync_surfaces(&mut self) {
        let d = self.desired.clone();
        self.send(Cmd::Sync(d));
    }

    // ------------------------------------------------------------------ store ticks

    pub fn tick(&mut self) {
        let now = now_ms();
        self.store.tick(now);
        self.history.tick(now);
        self.flush_suggestions();
    }

    /// Milliseconds until the next store/history write is due (None: nothing scheduled).
    pub fn next_wake(&self) -> Option<i64> {
        [self.store.next_deadline(), self.history.next_deadline()].into_iter().flatten().min()
    }

    pub fn flush(&mut self) {
        self.store.data.state.downloads = self.downloads.persisted();
        self.store.flush();
        self.history.flush();
    }
}

/// The handle given to the interface and to the event loop.
pub struct Browser {
    pub core: RefCell<Core>,
    pub bus: Bus,
}

impl Browser {
    pub fn new(core: Core) -> Browser {
        let bus = core.bus.clone();
        Browser { core: RefCell::new(core), bus }
    }
}

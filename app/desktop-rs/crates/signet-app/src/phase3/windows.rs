//! Little Signet, private windows and notes (ports of `main/windows/little.ts` and `private.ts`; SPEC "Little Signet",
//! "Private windows").
//!
//! Each window is a GPUI window (`windows/aux.rs`) that hosts its own CEF views, so the core's `Views` does not know
//! them: this module keeps a `ViewId` to tab map and handles the CEF events of its views itself (the core ignores
//! views that are not tabs, apart from downloads). The tabs live in `store.state.tabs` and `store.data.detached`
//! like in Electron (never persisted, never in the snapshot).
//!
//! The screens call `SignetApi` methods without a window id, so the target is the focused auxiliary window (tracked
//! from activation events), else the most recent one of that kind.

use std::collections::HashMap;

use raw_window_handle::RawWindowHandle;
use signet_core::api::UiEvent;
use signet_core::factory::new_tab;
use signet_core::ids::new_id;
use signet_core::model::OpenOptions;
use signet_core::types::{LittleState, PrivateState, PrivateTab, Rect, TabInfo};
use signet_features::internal;
use signet_web::{Bounds, Disposition, ProfileKind, ViewId, ViewOptions, WebEvent, WebView};

use crate::browser::commands::{event_key, key_string};
use crate::browser::layout::DOC_BASE;
use crate::browser::Core;
use crate::windows::aux::AuxKind;
use crate::windows::Cmd;

// ---------------------------------------------------------------------------------------------------------------
// Pure logic
// ---------------------------------------------------------------------------------------------------------------

const PAD: f64 = 6.0;
/// Little Signet: bar height below the margin.
const LITTLE_BAR: f64 = 40.0;
/// Private, tabs on top: tab row and gap before the card.
const PRIVATE_TOP: f64 = 44.0;
/// Private: address bar at the top of the card.
const PRIVATE_BAR: f64 = 40.0;
const BLANK: &str = "about:blank";

fn rect(x: f64, y: f64, w: f64, h: f64) -> Rect {
    Rect::new(x, y, w.max(0.0), h.max(0.0))
}

/// Native view rectangle of a Little Signet window: `x=6, y=46, w=W-12, h=H-52`.
pub fn little_rect(w: f64, h: f64) -> Rect {
    rect(PAD, PAD + LITTLE_BAR, w - PAD * 2.0, h - PAD * 2.0 - LITTLE_BAR)
}

/// Card of a private window (address bar included): tabs on top, or a sidebar of width `side`.
pub fn private_card(w: f64, h: f64, side: Option<f64>) -> Rect {
    match side {
        None => rect(PAD, PRIVATE_TOP, w - PAD * 2.0, h - PRIVATE_TOP - PAD),
        Some(s) => rect(s, PAD, w - s - PAD, h - PAD * 2.0),
    }
}

/// Native view rectangle of a private window: the card under its address bar.
pub fn private_view_rect(w: f64, h: f64, side: Option<f64>) -> Rect {
    let c = private_card(w, h, side);
    rect(c.x, c.y + PRIVATE_BAR, c.width, c.height - PRIVATE_BAR)
}

/// The whole window while the page is fullscreen, else the given rectangle.
pub fn fit_rect(full: bool, win: (f64, f64), r: Rect) -> Rect {
    if full { rect(0.0, 0.0, win.0, win.1) } else { r }
}

/// Index where a new tab goes: right after `after`, else at the end.
pub fn insert_index(tabs: &[String], after: Option<&str>) -> usize {
    match after.and_then(|a| tabs.iter().position(|t| t == a)) {
        Some(i) => i + 1,
        None => tabs.len(),
    }
}

/// Tab that becomes active when the one at `removed` leaves `tabs` (already without it): the neighbor that takes its place.
pub fn after_close(tabs: &[String], removed: usize) -> Option<String> {
    if tabs.is_empty() { None } else { tabs.get(removed.min(tabs.len() - 1)).cloned() }
}

/// Tab reached by Ctrl+Tab (step 1) or Ctrl+Shift+Tab (step -1), wrapping around.
pub fn cycle_target(tabs: &[String], active: Option<&str>, step: i64) -> Option<String> {
    if tabs.len() < 2 {
        return None;
    }
    let i = tabs.iter().position(|t| Some(t.as_str()) == active)? as i64;
    let n = tabs.len() as i64;
    tabs.get((i + step).rem_euclid(n) as usize).cloned()
}

/// Cmd+1 to Cmd+8: nth tab; Cmd+9: the last.
pub fn goto_target(tabs: &[String], n: usize) -> Option<String> {
    if n == 0 || tabs.is_empty() {
        return None;
    }
    if n >= 9 { tabs.last().cloned() } else { tabs.get(n - 1).cloned() }
}

/// State of a private window's bar.
pub fn private_state<'a>(tabs: impl Fn(&str) -> Option<&'a TabInfo>, order: &[String], active: Option<&str>, side: Option<f64>) -> PrivateState {
    let cur = active.and_then(|a| tabs(a));
    PrivateState {
        tabs: order
            .iter()
            .filter_map(|id| tabs(id))
            .map(|t| PrivateTab { id: t.id.clone(), url: t.url.clone(), title: t.title.clone(), favicon: t.favicon.clone(), loading: t.loading, audible: t.audible })
            .collect(),
        active_id: active.map(str::to_string),
        sidebar_width: side,
        url: cur.map(|t| t.url.clone()).unwrap_or_default(),
        loading: cur.is_some_and(|t| t.loading),
        can_go_back: cur.is_some_and(|t| t.can_go_back),
        can_go_forward: cur.is_some_and(|t| t.can_go_forward),
    }
}

/// State of a Little Signet bar.
pub fn little_state(tab: &TabInfo, hue: f64) -> LittleState {
    LittleState {
        tab_id: tab.id.clone(),
        url: tab.url.clone(),
        title: tab.title.clone(),
        favicon: tab.favicon.clone(),
        theme_color: tab.theme_color.clone(),
        loading: tab.loading,
        can_go_back: tab.can_go_back,
        can_go_forward: tab.can_go_forward,
        hue,
    }
}

/// What a menu or shortcut command does when an auxiliary window is frontmost.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    CloseWindow,
    CloseTab,
    NewTab,
    Cycle(i64),
    Goto(usize),
    FocusAddress,
    Back,
    Forward,
    Reload(bool),
    CopyUrl,
    Zoom(i32),
    Print,
    ViewSource,
    /// No effect (find in page, address bar of a Little Signet, developer tools).
    Swallow,
    /// Main window command: it comes back to the front, then the core runs the command.
    ToMain,
    /// Not specific to the window: the core runs it as usual.
    Pass,
}

/// Port of the menu handlers of `app/menu.ts` for a frontmost Little Signet or private window.
pub fn classify(id: &str, private: bool) -> Action {
    match id {
        "closeWindow" => Action::CloseWindow,
        "closeTab" => if private { Action::CloseTab } else { Action::CloseWindow },
        "newTab" if private => Action::NewTab,
        "nextTab" if private => Action::Cycle(1),
        "prevTab" if private => Action::Cycle(-1),
        "openAddress" if private => Action::FocusAddress,
        "back" => Action::Back,
        "forward" => Action::Forward,
        "reload" => Action::Reload(false),
        "forceReload" => Action::Reload(true),
        "copyUrl" => Action::CopyUrl,
        "zoomIn" => Action::Zoom(1),
        "zoomOut" => Action::Zoom(-1),
        "actualSize" => Action::Zoom(0),
        "print" => Action::Print,
        "viewSource" => Action::ViewSource,
        "openAddress" | "find" | "findNext" | "findPrev" | "devTools" | "jsConsole" => Action::Swallow,
        "newTab" | "reopenTab" | "bookmark" | "pinTab" | "openSplit" | "nextTab" | "prevTab" | "nextSpace" | "prevSpace" | "toggleSidebar" | "compactMode" | "tabLayout" | "bookmarksBar" => Action::ToMain,
        other => match other.strip_prefix("goToTab:").and_then(|n| n.parse().ok()) {
            Some(n) if private => Action::Goto(n),
            Some(_) => Action::ToMain,
            None if other.starts_with("goToSpace:") => Action::ToMain,
            None => Action::Pass,
        },
    }
}

fn web(r: Rect) -> Bounds {
    Bounds::new(r.x as f32, r.y as f32, r.width.max(1.0) as f32, r.height.max(1.0) as f32)
}

// ---------------------------------------------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------------------------------------------

struct LittleWin {
    tab: String,
    view: Option<WebView>,
    /// Last state sent to the bar (JSON): avoids sending the same one again.
    last_sent: String,
    /// Tab fullscreen: the page covers the window.
    full: bool,
}

struct PrivateWin {
    /// In-memory profile of the window.
    session: String,
    tabs: Vec<String>,
    active: Option<String>,
    views: HashMap<String, WebView>,
    /// Give the keyboard to the page at the next sync.
    focus_page: bool,
    last_sent: String,
    full: Option<String>,
}

enum Kind {
    Little(LittleWin),
    Private(PrivateWin),
}

struct Aux {
    raw: Option<RawWindowHandle>,
    size: (f64, f64),
    kind: Kind,
}

/// State owned by this feature (a field of `Phase3`).
#[derive(Default)]
pub struct State {
    next_id: u64,
    wins: HashMap<u64, Aux>,
    by_view: HashMap<ViewId, (u64, String)>,
    /// Frontmost auxiliary window (None while the main window or another app is in front).
    focused: Option<u64>,
    /// Closed views of a private window waiting for their `Closed` event: view to session.
    closing: HashMap<ViewId, String>,
}

impl State {
    /// Auxiliary window an API call without window id means: the focused one if it is of this kind, else the newest.
    fn current(&self, private: bool) -> Option<u64> {
        let of_kind = |id: &u64| self.wins.get(id).is_some_and(|a| matches!(a.kind, Kind::Private(_)) == private);
        self.focused.filter(of_kind).or_else(|| self.wins.keys().copied().filter(of_kind).max())
    }

    fn count(&self, private: bool) -> usize {
        self.wins.values().filter(|a| matches!(a.kind, Kind::Private(_)) == private).count()
    }

    fn owner_of_tab(&self, tab: &str) -> Option<u64> {
        self.by_view.values().find(|(_, t)| t == tab).map(|(w, _)| *w).or_else(|| {
            self.wins.iter().find(|(_, a)| match &a.kind {
                Kind::Private(p) => p.tabs.iter().any(|t| t == tab),
                Kind::Little(l) => l.tab == tab,
            })
            .map(|(id, _)| *id)
        })
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Little Signet
// ---------------------------------------------------------------------------------------------------------------

impl Core {
    fn aux_new_id(&mut self) -> u64 {
        self.phase3.windows.next_id += 1;
        self.phase3.windows.next_id
    }

    /// Creates a detached tab (never persisted, not in the snapshot).
    fn aux_new_tab(&mut self, url: &str) -> String {
        let tab = new_tab(url, "");
        let id = tab.id.clone();
        self.store.data.state.tabs.insert(id.clone(), tab);
        self.store.data.detached.insert(id.clone());
        id
    }

    fn aux_forget_tab(&mut self, tab: &str) {
        self.store.data.state.tabs.shift_remove(tab);
        self.store.data.detached.remove(tab);
        self.upgraded.remove(tab);
        self.typed.remove(tab);
    }

    fn aux_patch(&mut self, tab: &str, f: impl FnOnce(&mut TabInfo)) {
        if let Some(t) = self.store.data.state.tabs.get_mut(tab) {
            f(t);
        }
    }

    /// Opens a Little Signet window on `url`.
    pub fn open_little_window(&mut self, url: &str) {
        let url = url.trim();
        if url.is_empty() || self.open_shortcut(url) {
            return;
        }
        let tab = self.aux_new_tab(url);
        let id = self.aux_new_id();
        self.phase3.windows.wins.insert(id, Aux { raw: None, size: (760.0, 820.0), kind: Kind::Little(LittleWin { tab, view: None, last_sent: String::new(), full: false }) });
        self.send(Cmd::OpenAux { id, kind: AuxKind::Little });
    }

    /// The bar is subscribed (or reloaded): forget the last send and send the state again.
    pub fn little_ready(&mut self) {
        if let Some(id) = self.phase3.windows.current(false) {
            if let Some(Aux { kind: Kind::Little(l), .. }) = self.phase3.windows.wins.get_mut(&id) {
                l.last_sent.clear();
            }
            self.push_little(id, true);
        }
    }

    /// "Open in Signet": the tab goes to the main window (its page reloads there: a CEF view cannot change windows).
    pub fn little_open_in_main(&mut self) {
        let Some(id) = self.phase3.windows.current(false) else { return };
        let Some(Aux { kind: Kind::Little(l), .. }) = self.phase3.windows.wins.get(&id) else { return };
        let tab = l.tab.clone();
        let tab_ok = self.state().tabs.contains_key(&tab);
        // The tab survives: only its window and its view go.
        if let Some(Aux { kind: Kind::Little(l), .. }) = self.phase3.windows.wins.get_mut(&id) {
            l.tab.clear();
        }
        self.aux_close_window(id);
        if !tab_ok {
            return;
        }
        self.aux_patch(&tab, |t| {
            t.loaded = false;
            t.loading = false;
        });
        self.model(|m| m.adopt(&tab));
        self.send(Cmd::ShowMain);
        self.request_focus();
        self.changed();
    }

    pub fn little_copy_url(&mut self) {
        let Some(id) = self.phase3.windows.current(false) else { return };
        let Some(Aux { kind: Kind::Little(l), .. }) = self.phase3.windows.wins.get(&id) else { return };
        if let Some(t) = self.state().tabs.get(&l.tab) {
            crate::app::platform::copy_text(&t.url);
        }
    }

    fn push_little(&mut self, id: u64, force: bool) {
        let allowed = force || self.phase3.windows.focused == Some(id) || self.phase3.windows.count(false) == 1;
        let hue = self.hue();
        let Some(Aux { kind: Kind::Little(l), .. }) = self.phase3.windows.wins.get(&id) else { return };
        let Some(tab) = self.state().tabs.get(&l.tab) else { return };
        let state = little_state(tab, hue);
        let json = serde_json::to_string(&state).unwrap_or_default();
        if l.last_sent == json || !allowed {
            return;
        }
        if let Some(Aux { kind: Kind::Little(l), .. }) = self.phase3.windows.wins.get_mut(&id) {
            l.last_sent = json;
        }
        self.emit(UiEvent::LittleState(state));
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Private windows
// ---------------------------------------------------------------------------------------------------------------

impl Core {
    /// Self-test: (view, window id, tab) of the views of Little Signet and private windows, and the window count.
    pub fn selftest_aux(&self) -> (Vec<(signet_web::ViewId, u64, String)>, usize) {
        (self.phase3.windows.by_view.iter().map(|(v, (w, t))| (*v, *w, t.clone())).collect(), self.phase3.windows.wins.len())
    }

    pub fn open_private_window(&mut self) {
        self.open_private_with("");
    }

    /// "Open in private window": a new window whose first tab goes to `url`.
    pub fn open_private_url(&mut self, url: &str) {
        if self.open_shortcut(url) {
            return;
        }
        self.open_private_with(url.trim());
    }

    fn open_private_with(&mut self, url: &str) {
        let tab = self.aux_new_tab(url);
        let id = self.aux_new_id();
        let pw = PrivateWin { session: new_id(), tabs: vec![tab.clone()], active: Some(tab), views: HashMap::new(), focus_page: true, last_sent: String::new(), full: None };
        self.phase3.windows.wins.insert(id, Aux { raw: None, size: (1100.0, 780.0), kind: Kind::Private(pw) });
        let vertical = self.private_side().is_some();
        self.send(Cmd::OpenAux { id, kind: AuxKind::Private { vertical } });
    }

    /// Sidebar width of the main window's layout, None for tabs on top (private windows follow it).
    fn private_side(&self) -> Option<f64> {
        if self.horizontal() { None } else { Some(self.live_sidebar_width.unwrap_or(self.state().sidebar_width)) }
    }

    fn private_win(&mut self, id: u64) -> Option<&mut PrivateWin> {
        match self.phase3.windows.wins.get_mut(&id) {
            Some(Aux { kind: Kind::Private(p), .. }) => Some(p),
            _ => None,
        }
    }

    pub fn get_private_state(&mut self) -> Option<PrivateState> {
        let id = self.phase3.windows.current(true)?;
        let state = self.build_private_state(id)?;
        let json = serde_json::to_string(&state).unwrap_or_default();
        if let Some(p) = self.private_win(id) {
            p.last_sent = json;
        }
        Some(state)
    }

    fn build_private_state(&self, id: u64) -> Option<PrivateState> {
        let Some(Aux { kind: Kind::Private(p), .. }) = self.phase3.windows.wins.get(&id) else { return None };
        Some(private_state(|id| self.state().tabs.get(id), &p.tabs, p.active.as_deref(), self.private_side()))
    }

    fn push_private(&mut self, id: u64) {
        let allowed = self.phase3.windows.focused == Some(id) || self.phase3.windows.count(true) == 1;
        let Some(state) = self.build_private_state(id) else { return };
        let json = serde_json::to_string(&state).unwrap_or_default();
        let Some(p) = self.private_win(id) else { return };
        if p.last_sent == json || !allowed {
            return;
        }
        p.last_sent = json;
        self.emit(UiEvent::PrivateState(state));
    }

    /// Adds a tab to a private window (no view until it has an address).
    fn private_add(&mut self, win: u64, url: &str, activate: bool, after: Option<&str>) -> Option<String> {
        self.private_win(win)?;
        let tab = self.aux_new_tab(url);
        let p = self.private_win(win)?;
        let at = insert_index(&p.tabs, after);
        p.tabs.insert(at, tab.clone());
        if activate {
            p.active = Some(tab.clone());
            p.focus_page = true;
        }
        Some(tab)
    }

    pub fn private_new_tab(&mut self) {
        if let Some(win) = self.phase3.windows.current(true) {
            self.private_add(win, "", true, None);
            self.sync_private(win);
        }
    }

    pub fn private_activate(&mut self, tab_id: &str) {
        let Some(win) = self.phase3.windows.owner_of_tab(tab_id) else { return };
        let Some(p) = self.private_win(win) else { return };
        if !p.tabs.iter().any(|t| t == tab_id) || p.active.as_deref() == Some(tab_id) {
            return;
        }
        p.active = Some(tab_id.to_string());
        p.focus_page = true;
        self.sync_private(win);
    }

    pub fn private_close_tab(&mut self, tab_id: &str) {
        let Some(win) = self.phase3.windows.owner_of_tab(tab_id) else { return };
        let session;
        let last;
        {
            let Some(p) = self.private_win(win) else { return };
            let Some(i) = p.tabs.iter().position(|t| t == tab_id) else { return };
            p.tabs.remove(i);
            session = p.session.clone();
            last = p.tabs.is_empty();
            if p.active.as_deref() == Some(tab_id) {
                p.active = after_close(&p.tabs, i);
                p.focus_page = true;
            }
            if p.full.as_deref() == Some(tab_id) {
                p.full = None;
            }
            if let Some(v) = p.views.remove(tab_id) {
                self.aux_close_view(v, Some(&session));
            }
        }
        self.aux_forget_tab(tab_id);
        if last {
            self.aux_close_window(win);
        } else {
            self.sync_private(win);
        }
    }

    /// Address typed in the bar: the displayed tab goes there, then the page takes focus.
    pub fn private_navigate(&mut self, input: &str) {
        let input = input.trim();
        let Some(win) = self.phase3.windows.current(true) else { return };
        let Some(tab) = self.private_win(win).and_then(|p| p.active.clone()) else { return };
        if input.is_empty() {
            return;
        }
        if let Some(p) = self.private_win(win) {
            p.focus_page = true;
        }
        self.aux_navigate(&tab, input);
        self.sync_private(win);
    }

    /// Places views, shows the displayed tab's view (created once the tab has an address), gives focus, pushes the state.
    fn sync_private(&mut self, id: u64) {
        let side = self.private_side();
        let Some(Aux { raw, size, kind: Kind::Private(p) }) = self.phase3.windows.wins.get(&id) else { return };
        let (raw, size, session, active, full) = (*raw, *size, p.session.clone(), p.active.clone(), p.full.clone());
        if raw.is_none() {
            return;
        }
        let area = private_view_rect(size.0, size.1, side);
        if let Some(a) = &active {
            let url = self.state().tabs.get(a).map(|t| t.url.clone()).unwrap_or_default();
            let has_view = self.private_win(id).is_some_and(|p| p.views.contains_key(a));
            if !url.is_empty() && !has_view {
                let placed = fit_rect(full.as_deref() == Some(a.as_str()), size, area);
                if let Some(v) = self.aux_create_view(id, a, ProfileKind::private(session.clone()), &url, placed, true)
                    && let Some(p) = self.private_win(id)
                {
                    p.views.insert(a.clone(), v);
                    self.aux_patch(a, |t| t.loaded = true);
                }
            }
        }
        let mut focus: Option<WebView> = None;
        if let Some(p) = self.private_win(id) {
            let want_focus = std::mem::take(&mut p.focus_page);
            for (tab, v) in &p.views {
                let shown = active.as_deref() == Some(tab.as_str());
                if shown {
                    let r = web(fit_rect(full.as_deref() == Some(tab.as_str()), size, area));
                    if v.bounds() != r {
                        v.set_bounds(r);
                    }
                    if want_focus {
                        focus = Some(*v);
                    }
                }
                if v.is_visible() != shown {
                    v.set_visible(shown);
                }
            }
        }
        if let Some(v) = focus {
            v.focus();
        }
        self.push_private(id);
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Window lifecycle (called by `windows/aux.rs`)
// ---------------------------------------------------------------------------------------------------------------

impl Core {
    /// The GPUI window exists: its CEF views can be created.
    pub fn on_aux_created(&mut self, id: u64, raw: Option<RawWindowHandle>, size: (f64, f64)) {
        if self.web_app_owns_window(id) {
            return self.web_app_window_created(id, raw, size);
        }
        let Some(a) = self.phase3.windows.wins.get_mut(&id) else { return };
        a.raw = raw;
        a.size = size;
        let little = match &a.kind {
            Kind::Little(l) => Some(l.tab.clone()),
            Kind::Private(_) => None,
        };
        self.phase3.windows.focused = Some(id);
        self.log(format!("aux window {id} created {}x{}", size.0, size.1));
        match little {
            Some(tab) => {
                let url = self.state().tabs.get(&tab).map(|t| t.url.clone()).unwrap_or_default();
                let target = if url.is_empty() { BLANK.to_string() } else { internal::resolve_internal(DOC_BASE, &url) };
                let r = little_rect(size.0, size.1);
                if let Some(v) = self.aux_create_view(id, &tab, ProfileKind::persistent("default"), &target, r, true) {
                    if let Some(Aux { kind: Kind::Little(l), .. }) = self.phase3.windows.wins.get_mut(&id) {
                        l.view = Some(v);
                    }
                    self.aux_patch(&tab, |t| t.loaded = true);
                    v.focus();
                }
                self.push_little(id, false);
            }
            None => self.sync_private(id),
        }
    }

    pub fn on_aux_resized(&mut self, id: u64, size: (f64, f64)) {
        if self.web_app_owns_window(id) {
            return self.web_app_window_resized(id, size);
        }
        let Some(a) = self.phase3.windows.wins.get_mut(&id) else { return };
        if a.size == size {
            return;
        }
        a.size = size;
        match &a.kind {
            Kind::Little(l) => {
                if let Some(v) = l.view {
                    v.set_bounds(web(fit_rect(l.full, size, little_rect(size.0, size.1))));
                }
            }
            Kind::Private(_) => self.sync_private(id),
        }
    }

    pub fn on_aux_active(&mut self, id: u64, active: bool) {
        if self.web_app_owns_window(id) {
            return self.web_app_window_active(id, active);
        }
        if active {
            self.phase3.windows.focused = Some(id);
            // The bars share one event stream: show this window's state.
            match self.phase3.windows.wins.get_mut(&id).map(|a| &mut a.kind) {
                Some(Kind::Little(l)) => {
                    l.last_sent.clear();
                    self.push_little(id, true);
                }
                Some(Kind::Private(p)) => {
                    p.last_sent.clear();
                    self.push_private(id);
                }
                None => {}
            }
        } else if self.phase3.windows.focused == Some(id) {
            self.phase3.windows.focused = None;
        }
    }

    /// The window is gone (closed by the user or by us): its tabs and views go, a private session is dropped.
    pub fn on_aux_closed(&mut self, id: u64) {
        if self.web_app_owns_window(id) {
            return self.web_app_window_closed(id);
        }
        let Some(a) = self.phase3.windows.wins.remove(&id) else { return };
        if self.phase3.windows.focused == Some(id) {
            self.phase3.windows.focused = None;
        }
        match a.kind {
            Kind::Little(l) => {
                if let Some(v) = l.view {
                    self.aux_close_view(v, None);
                }
                if !l.tab.is_empty() {
                    self.aux_forget_tab(&l.tab);
                }
            }
            Kind::Private(p) => {
                let had_views = !p.views.is_empty();
                for v in p.views.values() {
                    self.aux_close_view(*v, Some(&p.session));
                }
                for t in &p.tabs {
                    self.aux_forget_tab(t);
                }
                if !had_views {
                    signet_web::drop_profile(&ProfileKind::private(p.session));
                }
            }
        }
        self.log(format!("aux window {id} closed"));
    }

    /// Closes a window from the core (last private tab, Cmd+W).
    fn aux_close_window(&mut self, id: u64) {
        self.on_aux_closed(id);
        self.send(Cmd::CloseAux(id));
    }

    fn aux_create_view(&mut self, win: u64, tab: &str, profile: ProfileKind, url: &str, bounds: Rect, visible: bool) -> Option<WebView> {
        let raw = self.phase3.windows.wins.get(&win)?.raw?;
        let target = internal::resolve_internal(DOC_BASE, url);
        let opts = ViewOptions { profile, url: target.clone(), bounds: web(bounds), visible, transparent_background: false };
        match WebView::create(raw, opts) {
            Ok(v) => {
                self.phase3.windows.by_view.insert(v.id(), (win, tab.to_string()));
                if matches!(self.phase3.windows.wins.get(&win).map(|a| &a.kind), Some(Kind::Private(_))) {
                    self.pages_register_private_view(v.id(), tab);
                }
                self.log(format!("aux view created for {tab} -> {target}"));
                Some(v)
            }
            Err(e) => {
                eprintln!("[windows] cannot create the view of {tab}: {e}");
                None
            }
        }
    }

    /// Takes a view down. For a private session, the profile is dropped once every one of its views is closed.
    fn aux_close_view(&mut self, v: WebView, session: Option<&str>) {
        self.phase3.windows.by_view.remove(&v.id());
        self.pages_unregister_view(v.id());
        if let Some(s) = session {
            self.phase3.windows.closing.insert(v.id(), s.to_string());
        }
        v.close();
    }

    /// `Closed` event of a view: a private profile whose views are all gone can go.
    fn aux_view_closed(&mut self, view: ViewId) {
        let Some(session) = self.phase3.windows.closing.remove(&view) else { return };
        let still = self.phase3.windows.closing.values().any(|s| *s == session)
            || self.phase3.windows.wins.values().any(|a| matches!(&a.kind, Kind::Private(p) if p.session == session && !p.views.is_empty()));
        if !still {
            signet_web::drop_profile(&ProfileKind::private(session));
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Navigation and commands
// ---------------------------------------------------------------------------------------------------------------

impl Core {
    /// View of the tab of the frontmost auxiliary window (None: the main window is the target).
    pub fn aux_target_view(&self) -> Option<WebView> {
        let tab = self.aux_target_tab()?;
        self.aux_view_of(&tab)
    }

    /// Tab of the frontmost auxiliary window (None: the main window is the target).
    pub fn aux_target_tab(&self) -> Option<String> {
        let id = self.phase3.windows.focused?;
        match &self.phase3.windows.wins.get(&id)?.kind {
            Kind::Little(l) => Some(l.tab.clone()),
            Kind::Private(p) => p.active.clone(),
        }
    }

    /// View of a tab that lives in a Little Signet or private window (for modules that answer page messages).
    pub fn aux_view_of(&self, tab: &str) -> Option<WebView> {
        let win = self.phase3.windows.owner_of_tab(tab)?;
        match &self.phase3.windows.wins.get(&win)?.kind {
            Kind::Little(l) => l.view,
            Kind::Private(p) => p.views.get(tab).copied(),
        }
    }

    #[allow(dead_code)]
    /// True if the tab belongs to a private window.
    pub fn is_private_tab(&self, tab: &str) -> bool {
        self.phase3.windows.owner_of_tab(tab).is_some_and(|w| matches!(self.phase3.windows.wins.get(&w).map(|a| &a.kind), Some(Kind::Private(_))))
    }

    /// Runs a menu or shortcut command for the frontmost auxiliary window. True if it was handled; false: the core
    /// runs it (after the main window was brought back for main window commands).
    pub fn aux_command(&mut self, command: &str) -> bool {
        let Some(win) = self.phase3.windows.focused else { return false };
        let private = match self.phase3.windows.wins.get(&win).map(|a| &a.kind) {
            Some(Kind::Private(_)) => true,
            Some(Kind::Little(_)) => false,
            None => return false,
        };
        let action = classify(command, private);
        let tab = self.aux_target_tab();
        let view = self.aux_target_view();
        match action {
            Action::Pass => return false,
            Action::ToMain => {
                self.send(Cmd::ShowMain);
                return false;
            }
            Action::Swallow => {}
            Action::CloseWindow => self.aux_close_window(win),
            Action::CloseTab => match tab {
                Some(t) => self.private_close_tab(&t),
                None => self.aux_close_window(win),
            },
            Action::NewTab => self.private_new_tab(),
            Action::Cycle(step) => {
                if let Some(p) = self.private_win(win) {
                    let next = cycle_target(&p.tabs, p.active.as_deref(), step);
                    if let Some(n) = next {
                        p.active = Some(n);
                        p.focus_page = true;
                        self.sync_private(win);
                    }
                }
            }
            Action::Goto(n) => {
                if let Some(p) = self.private_win(win)
                    && let Some(t) = goto_target(&p.tabs, n)
                    && p.active.as_deref() != Some(t.as_str())
                {
                    p.active = Some(t);
                    p.focus_page = true;
                    self.sync_private(win);
                }
            }
            Action::FocusAddress => self.emit(UiEvent::PrivateFocusAddress),
            Action::Back => view.iter().for_each(|v| v.back()),
            Action::Forward => view.iter().for_each(|v| v.forward()),
            Action::Reload(force) => view.iter().for_each(|v| if force { v.reload_ignore_cache() } else { v.reload() }),
            Action::CopyUrl => {
                if let Some(t) = tab.and_then(|t| self.state().tabs.get(&t)) {
                    crate::app::platform::copy_text(&t.url);
                }
            }
            Action::Zoom(delta) => {
                if let Some(v) = view {
                    let level = if delta == 0 {
                        (f64::from(self.state().settings.default_zoom.unwrap_or(100)) / 100.0).ln() / 1.2f64.ln()
                    } else {
                        v.zoom_level() + f64::from(delta)
                    };
                    v.set_zoom_level(level.clamp(-7.6, 9.0));
                }
            }
            Action::Print => view.iter().for_each(|v| v.print()),
            Action::ViewSource => view.iter().for_each(|v| v.view_source()),
        }
        true
    }

    /// Typed address of an auxiliary window's tab (port of `Core::navigate`, without the main window's bookkeeping).
    fn aux_navigate(&mut self, tab: &str, input: &str) {
        let url = if let Some((_, query, url)) = self.search.keyword_search(input)
            && !query.is_empty()
        {
            internal::refuse_engine_input(&url)
        } else {
            let aliased = signet_core::devhosts::resolve_alias(input, self.state().settings.dev_aliases.as_deref().unwrap_or(&[]));
            let typed = if aliased.is_some() { None } else { signet_core::urls::to_url(input) };
            let url = aliased.or_else(|| typed.clone()).unwrap_or_else(|| self.search.engine_search_url(input));
            if let Some(t) = typed
                && signet_core::urls::is_upgraded_input(input, Some(&t))
            {
                self.upgraded.insert(tab.to_string(), t);
            }
            internal::refuse_engine_input(&url)
        };
        self.aux_load(tab, &url);
    }

    fn aux_load(&mut self, tab: &str, url: &str) {
        if !self.state().tabs.contains_key(tab) || signet_core::urls::is_ui_address(url) || self.open_shortcut(url) {
            return;
        }
        self.aux_patch(tab, |t| t.url = url.to_string());
        if let Some(v) = self.aux_view_of(tab) {
            v.load_url(&internal::resolve_internal(DOC_BASE, url));
        }
    }

    /// Ticks and layout changes: private windows follow the main window's tab layout.
    pub fn sync_aux_windows(&mut self) {
        let ids: Vec<u64> = self.phase3.windows.wins.keys().copied().collect();
        for id in ids {
            match self.phase3.windows.wins.get(&id).map(|a| &a.kind) {
                Some(Kind::Private(_)) => self.sync_private(id),
                Some(Kind::Little(_)) => self.push_little(id, false),
                None => {}
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// CEF events of the views of these windows
// ---------------------------------------------------------------------------------------------------------------

impl Core {
    fn aux_event(&mut self, win: u64, tab: &str, ev: &WebEvent) {
        let private = matches!(self.phase3.windows.wins.get(&win).map(|a| &a.kind), Some(Kind::Private(_)));
        match ev {
            WebEvent::TitleChanged { title, .. } => {
                let title = internal::display_url(DOC_BASE, title);
                self.aux_patch(tab, |t| t.title = title.clone());
                if !private && let Some(url) = self.state().tabs.get(tab).map(|t| t.url.clone()) {
                    self.history.update(&url, Some(&title), None);
                }
            }
            WebEvent::AddressChanged { url, is_main_frame: true, .. } => {
                let shown = internal::display_url(DOC_BASE, url);
                let known = self.state().tabs.get(tab).is_some_and(|t| !t.url.is_empty());
                if shown.starts_with("data:") || shown == BLANK && known {
                    return;
                }
                let (back, fwd) = self.aux_view_of(tab).map(|v| (v.can_go_back(), v.can_go_forward())).unwrap_or((false, false));
                self.aux_patch(tab, |t| {
                    t.url = shown.clone();
                    t.can_go_back = back;
                    t.can_go_forward = fwd;
                });
                let typed = self.typed.remove(tab).is_some();
                if !private
                    && !shown.starts_with("signet://")
                    && let Some(t) = self.state().tabs.get(tab).cloned()
                {
                    self.history.record(&shown, &t.title, t.favicon.as_deref(), typed);
                }
            }
            WebEvent::FaviconUrls { urls, .. } => {
                if let Some(first) = urls.first() {
                    self.aux_patch(tab, |t| t.favicon = Some(first.clone()));
                    if !private && let Some(url) = self.state().tabs.get(tab).map(|t| t.url.clone()) {
                        self.history.update(&url, None, Some(first));
                    }
                }
            }
            WebEvent::LoadingState { is_loading, can_go_back, can_go_forward, .. } => {
                self.aux_patch(tab, |t| {
                    t.loading = *is_loading;
                    t.can_go_back = *can_go_back;
                    t.can_go_forward = *can_go_forward;
                });
            }
            WebEvent::LoadStart { is_main_frame: true, .. } => self.aux_patch(tab, |t| t.loading = true),
            WebEvent::LoadEnd { is_main_frame: true, .. } => {
                self.upgraded.remove(tab);
                self.aux_patch(tab, |t| t.loading = false);
            }
            WebEvent::LoadError { code, url, is_main_frame: true, .. } => {
                if *code == -3 {
                    return;
                }
                if let Some(pending) = self.upgraded.remove(tab)
                    && signet_core::urls::is_https_fallback_error(*code)
                    && let Some(http) = signet_core::urls::http_fallback(&pending, url)
                {
                    self.aux_load(tab, &http);
                    return;
                }
                self.aux_patch(tab, |t| t.loading = false);
            }
            WebEvent::NewWindowRequested { url, disposition, .. } => {
                if url.is_empty() || url == BLANK || self.open_shortcut(url) {
                    return;
                }
                if private {
                    let activate = !matches!(disposition, Disposition::BackgroundTab);
                    self.private_add(win, url, activate, Some(tab));
                    self.sync_private(win);
                } else {
                    self.aux_load(tab, url);
                }
                return;
            }
            WebEvent::FullscreenChanged { fullscreen, .. } => {
                match self.phase3.windows.wins.get_mut(&win).map(|a| &mut a.kind) {
                    Some(Kind::Little(l)) => l.full = *fullscreen,
                    Some(Kind::Private(p)) => p.full = fullscreen.then(|| tab.to_string()),
                    None => {}
                }
                if let Some(a) = self.phase3.windows.wins.get(&win) {
                    let size = a.size;
                    match &a.kind {
                        Kind::Little(l) => {
                            if let Some(v) = l.view {
                                v.set_bounds(web(fit_rect(l.full, size, little_rect(size.0, size.1))));
                            }
                        }
                        Kind::Private(_) => self.sync_private(win),
                    }
                }
                return;
            }
            WebEvent::AudioPlayingChanged { playing, .. } => self.aux_patch(tab, |t| t.audible = *playing),
            WebEvent::RenderProcessGone { .. } => self.aux_patch(tab, |t| t.loading = false),
            WebEvent::PreKey { event, handled: true, .. } => {
                let Some(key) = event_key(event) else { return };
                let ks = key_string(event.cmd, event.ctrl, event.alt, event.shift, &key);
                let id = self.key_table.lock().ok().and_then(|t| t.get(&ks).cloned());
                if let Some(id) = id {
                    self.phase3.windows.focused = Some(win);
                    if !self.aux_command(&id) {
                        self.run_command(&id);
                    }
                }
                return;
            }
            WebEvent::FocusGained { .. } => {
                if self.phase3.windows.focused != Some(win) {
                    self.phase3.windows.focused = Some(win);
                }
                return;
            }
            WebEvent::PageMessage { view, frame_url, is_main_frame, channel, payload, frame_id, top_url } => {
                self.on_page_message(*view, tab, frame_id, frame_url, top_url, *is_main_frame, channel, payload);
                return;
            }
            _ => return,
        }
        match self.phase3.windows.wins.get(&win).map(|a| &a.kind) {
            Some(Kind::Little(_)) => self.push_little(win, false),
            Some(Kind::Private(_)) => self.push_private(win),
            None => {}
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Notes
// ---------------------------------------------------------------------------------------------------------------

impl Core {
    /// New note or Easel (`signet_features::notes`, served by `signet_pages`), opened in a new tab of the main window.
    pub fn create_note(&mut self, kind: &str) {
        if let Some(address) = self.create_note_doc(kind) {
            self.send(Cmd::ShowMain);
            self.open_tab(&address, OpenOptions { activate: true, ..Default::default() });
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Hooks
// ---------------------------------------------------------------------------------------------------------------

/// Called once after the browser core exists and the web layer is initialized.
pub(super) fn startup(_core: &mut Core) {}

/// Called for every CEF event, before the core handles it. The core ignores views it does not know (except
/// downloads), so the events of our views need no extra marking.
pub(super) fn web_event(core: &mut Core, ev: &WebEvent) {
    let view = ev.view();
    if matches!(ev, WebEvent::Closed { .. }) {
        core.aux_view_closed(view);
    }
    let Some((win, tab)) = core.phase3.windows.by_view.get(&view).cloned() else { return };
    core.aux_event(win, &tab, ev);
}

/// Called on every core tick (about once per second or at the next store deadline).
pub(super) fn tick(core: &mut Core) {
    core.sync_aux_windows();
}

/// Called before quitting (flush files, stop threads).
pub(super) fn quit(core: &mut Core) {
    let ids: Vec<u64> = core.phase3.windows.wins.keys().copied().collect();
    for id in ids {
        core.on_aux_closed(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn little_view_rectangle() {
        let r = little_rect(760.0, 820.0);
        assert_eq!((r.x, r.y, r.width, r.height), (6.0, 46.0, 748.0, 768.0));
        let tiny = little_rect(5.0, 10.0);
        assert_eq!((tiny.width, tiny.height), (0.0, 0.0));
    }

    #[test]
    fn private_view_rectangles() {
        let top = private_view_rect(1100.0, 780.0, None);
        assert_eq!((top.x, top.y, top.width, top.height), (6.0, 84.0, 1088.0, 690.0));
        let side = private_view_rect(1100.0, 780.0, Some(250.0));
        assert_eq!((side.x, side.y, side.width, side.height), (250.0, 46.0, 844.0, 728.0));
        let card = private_card(1100.0, 780.0, Some(250.0));
        assert_eq!((card.x, card.y, card.width, card.height), (250.0, 6.0, 844.0, 768.0));
    }

    #[test]
    fn fullscreen_covers_the_window() {
        let r = little_rect(400.0, 300.0);
        assert_eq!(fit_rect(false, (400.0, 300.0), r), r);
        let f = fit_rect(true, (400.0, 300.0), r);
        assert_eq!((f.x, f.y, f.width, f.height), (0.0, 0.0, 400.0, 300.0));
    }

    #[test]
    fn new_tab_goes_after_its_opener() {
        let tabs = ids(&["a", "b", "c"]);
        assert_eq!(insert_index(&tabs, Some("b")), 2);
        assert_eq!(insert_index(&tabs, Some("zz")), 3);
        assert_eq!(insert_index(&tabs, None), 3);
    }

    #[test]
    fn neighbor_takes_over_when_a_tab_closes() {
        assert_eq!(after_close(&ids(&["a", "c"]), 1).as_deref(), Some("c"));
        assert_eq!(after_close(&ids(&["a", "b"]), 2).as_deref(), Some("b"));
        assert_eq!(after_close(&ids(&[]), 0), None);
    }

    #[test]
    fn cycling_wraps_and_needs_two_tabs() {
        let tabs = ids(&["a", "b", "c"]);
        assert_eq!(cycle_target(&tabs, Some("c"), 1).as_deref(), Some("a"));
        assert_eq!(cycle_target(&tabs, Some("a"), -1).as_deref(), Some("c"));
        assert_eq!(cycle_target(&ids(&["a"]), Some("a"), 1), None);
        assert_eq!(cycle_target(&tabs, None, 1), None);
    }

    #[test]
    fn goto_number_and_last() {
        let tabs = ids(&["a", "b", "c"]);
        assert_eq!(goto_target(&tabs, 2).as_deref(), Some("b"));
        assert_eq!(goto_target(&tabs, 5), None);
        assert_eq!(goto_target(&tabs, 9).as_deref(), Some("c"));
        assert_eq!(goto_target(&tabs, 0), None);
    }

    #[test]
    fn private_state_follows_the_active_tab() {
        let mut tabs = HashMap::new();
        for (id, url) in [("a", ""), ("b", "https://x.test")] {
            let mut t = new_tab(url, "");
            t.id = id.into();
            t.loading = id == "b";
            t.can_go_back = id == "b";
            tabs.insert(id.to_string(), t);
        }
        let s = private_state(|id| tabs.get(id), &ids(&["a", "b", "gone"]), Some("b"), Some(240.0));
        assert_eq!(s.tabs.len(), 2);
        assert_eq!((s.active_id.as_deref(), s.url.as_str(), s.loading, s.can_go_back, s.sidebar_width), (Some("b"), "https://x.test", true, true, Some(240.0)));
        let empty = private_state(|id| tabs.get(id), &ids(&["a"]), Some("a"), None);
        assert_eq!((empty.url.as_str(), empty.loading, empty.sidebar_width), ("", false, None));
    }

    #[test]
    fn little_state_copies_the_tab() {
        let mut t = new_tab("https://x.test", "X");
        t.can_go_forward = true;
        let s = little_state(&t, 120.0);
        assert_eq!((s.url.as_str(), s.title.as_str(), s.can_go_forward, s.hue), ("https://x.test", "X", true, 120.0));
    }

    #[test]
    fn commands_of_the_frontmost_window() {
        assert_eq!(classify("closeTab", false), Action::CloseWindow);
        assert_eq!(classify("closeTab", true), Action::CloseTab);
        assert_eq!(classify("newTab", true), Action::NewTab);
        assert_eq!(classify("newTab", false), Action::ToMain);
        assert_eq!(classify("openAddress", true), Action::FocusAddress);
        assert_eq!(classify("openAddress", false), Action::Swallow);
        assert_eq!(classify("find", true), Action::Swallow);
        assert_eq!(classify("goToTab:3", true), Action::Goto(3));
        assert_eq!(classify("goToTab:3", false), Action::ToMain);
        assert_eq!(classify("goToSpace:2", true), Action::ToMain);
        assert_eq!(classify("reload", false), Action::Reload(false));
        assert_eq!(classify("settings", true), Action::Pass);
        assert_eq!(classify("newPrivate", false), Action::Pass);
    }
}

//! Port of `browser/tab-actions.ts` (opening, navigation, closing, activation, page commands) and `typed-nav.ts`.

use signet_core::ids::now_ms;
use signet_core::model::OpenOptions;
use signet_core::routes::match_route;
use signet_core::types::{Item, PaletteMode, Zone};
use signet_core::urls::{is_ui_address, is_upgraded_input, to_url};
use signet_features::internal;

use super::layout::DOC_BASE;
use super::Core;

impl Core {
    pub fn activate_tab(&mut self, tab_id: &str) {
        let Some(before) = self.model(|m| m.activate(tab_id)) else { return };
        if let Some(b) = before.filter(|b| b != tab_id) {
            self.last_switch = Some((tab_id.to_string(), b, now_ms()));
        }
        self.request_focus();
        self.changed();
    }

    pub fn cycle_tab(&mut self, step: i64) {
        if let Some(id) = self.model(|m| m.cycle_target(step)) {
            self.activate_tab(&id);
        }
    }

    pub fn goto_number(&mut self, n: usize) {
        let horizontal = self.horizontal();
        if let Some(id) = self.model(|m| m.number_target(n, horizontal)) {
            self.activate_tab(&id);
        }
    }

    /// Opens a tab (`openerId` makes it join the opener's group).
    pub fn open_tab(&mut self, url: &str, opts: OpenOptions) -> String {
        let activate = opts.activate;
        let id = self.model(|m| m.open(url, &opts));
        if activate {
            self.request_focus();
        }
        self.changed();
        id
    }

    pub fn duplicate_tab(&mut self, tab_id: &str) {
        if let Some(t) = self.state().tabs.get(tab_id).cloned() {
            self.open_tab(&t.url, OpenOptions { activate: true, after_tab_id: Some(tab_id.to_string()), opener_id: None });
        }
    }

    /// Opens in the space of a matching routing rule, otherwise in the active space.
    pub fn open_routed(&mut self, url: &str, always: bool) -> String {
        let rule = match_route(url, &self.state().settings.routes).cloned();
        let target = rule.and_then(|r| self.state().spaces.iter().position(|s| s.id == r.space_id));
        let active_idx = self.state().spaces.iter().position(|s| s.id == self.state().active_space_id).unwrap_or(0);
        match target {
            Some(i) if always || i != active_idx => {
                let id = self.model(|m| m.open_in(i, url));
                self.request_focus();
                self.changed();
                id
            }
            _ => self.open_tab(url, OpenOptions { activate: true, ..Default::default() }),
        }
    }

    /// Link from outside (second launch, default browser): routed space in the main window, otherwise Little Signet.
    pub fn open_external(&mut self, url: &str) {
        // `signet://webapp/<id>` and `--signet-webapp=<id>` launch an installed site.
        if self.launch_web_app_from(url) {
            return;
        }
        if self.open_shortcut(url) {
            return;
        }
        if match_route(url, &self.state().settings.routes).is_some() {
            self.send(crate::windows::Cmd::ShowMain);
            self.open_routed(url, true);
        } else {
            self.open_little_window(url);
        }
    }

    /// Address that opens an app screen (settings, library) rather than a tab. True if handled.
    pub fn open_shortcut(&mut self, url: &str) -> bool {
        match internal::shortcut(url) {
            Some(internal::Shortcut::Settings(section)) => {
                self.open_settings_window(section.as_deref());
                true
            }
            Some(internal::Shortcut::Library(section)) => {
                self.open_library_window(section.as_deref());
                true
            }
            None => false,
        }
    }

    /// Opens the chosen page: in the active tab (`EditUrl`), a split pane (`Split`) or a new tab.
    pub fn open_url(&mut self, input: &str, mode: PaletteMode, typed: bool) {
        let url = internal::refuse_engine_input(input);
        if self.open_shortcut(&url) {
            return;
        }
        let active = self.active_tab_id();
        if mode == PaletteMode::EditUrl
            && let Some(a) = active
        {
            self.load_url(&a, &url, typed);
            return;
        }
        let id = if mode == PaletteMode::Split { self.open_in_split(&url) } else { self.open_routed(&url, false) };
        if typed {
            self.typed.insert(id, internal::resolve_internal(DOC_BASE, &url));
        }
    }

    pub fn load_url(&mut self, tab_id: &str, url: &str, typed: bool) {
        self.upgraded.remove(tab_id);
        self.typed.remove(tab_id);
        if !self.state().tabs.contains_key(tab_id) || is_ui_address(url) || self.open_shortcut(url) {
            return;
        }
        let target = internal::resolve_internal(DOC_BASE, url);
        if let Some(t) = self.store.data.state.tabs.get_mut(tab_id) {
            t.url = url.to_string();
        }
        if typed {
            self.typed.insert(tab_id.to_string(), target.clone());
        }
        if let Some(v) = self.views.view_of(tab_id) {
            v.load_url(&target);
        }
        self.changed();
    }

    /// Address typed in the toolbar.
    pub fn navigate(&mut self, tab_id: &str, input: &str) {
        if let Some((_, query, url)) = self.search.keyword_search(input)
            && !query.is_empty()
        {
            return self.load_url(tab_id, &internal::refuse_engine_input(&url), false);
        }
        let aliased = signet_core::devhosts::resolve_alias(input, self.state().settings.dev_aliases.as_deref().unwrap_or(&[]));
        let typed = if aliased.is_some() { None } else { to_url(input) };
        let url = aliased.or_else(|| typed.clone()).unwrap_or_else(|| self.search.engine_search_url(input));
        self.load_url(tab_id, &internal::refuse_engine_input(&url), false);
        if let Some(t) = typed
            && is_upgraded_input(input, Some(&t))
        {
            self.upgraded.insert(tab_id.to_string(), t);
        }
    }

    pub fn reset_pinned(&mut self, tab_id: &str) {
        if let Some(home) = self.state().tabs.get(tab_id).and_then(|t| t.pinned_url.clone()) {
            self.load_url(tab_id, &home, false);
        }
    }

    pub fn close_tab(&mut self, tab_id: &str) {
        if self.find.tab_id.as_deref() == Some(tab_id) {
            self.close_find(false);
        }
        if self.model(|m| m.close(tab_id)) {
            self.changed();
        }
    }

    pub fn reopen_closed(&mut self) {
        if self.model(|m| m.reopen()).is_some() {
            self.changed();
        }
    }

    /// Archives all of the active space's day tabs; the interface offers to undo.
    pub fn clear_today(&mut self) {
        let Some(cleared) = self.model(|m| m.clear_today()) else { return };
        let count = cleared.count;
        self.changed();
        self.announce(signet_core::api::ArchiveKind::Archived, count, String::new(), super::UndoAction::Archive(cleared.undo));
    }

    /// Archives day tabs left too long without activation.
    pub fn sweep_archive(&mut self) {
        let busy: std::collections::HashSet<String> = self.state().tabs.iter().filter(|(_, t)| t.audible).map(|(k, _)| k.clone()).collect();
        let n = self.model(|m| m.sweep(&|id| busy.contains(id)));
        if n > 0 {
            self.changed();
        }
    }

    pub fn go_back(&mut self, tab: Option<&str>) {
        if let Some(v) = self.target_view(tab) {
            v.back();
        }
    }

    pub fn go_forward(&mut self, tab: Option<&str>) {
        if let Some(v) = self.target_view(tab) {
            v.forward();
        }
    }

    pub fn reload_tab(&mut self, tab: Option<&str>, force: bool) {
        if let Some(v) = self.target_view(tab) {
            if force { v.reload_ignore_cache() } else { v.reload() }
        }
    }

    /// Toolbar button: stops the load in progress, otherwise reloads.
    pub fn reload_or_stop(&mut self, tab: Option<&str>) {
        if let Some(v) = self.target_view(tab) {
            if v.is_loading() { v.stop() } else { v.reload() }
        }
    }

    pub fn target_view(&self, tab: Option<&str>) -> Option<signet_web::WebView> {
        if tab.is_none()
            && let Some(v) = self.aux_target_view()
        {
            return Some(v);
        }
        let id = tab.map(str::to_string).or_else(|| self.active_tab_id())?;
        self.views.view_of(&id)
    }

    pub fn toggle_mute(&mut self, tab_id: &str) {
        let Some(t) = self.store.data.state.tabs.get_mut(tab_id) else { return };
        t.muted = !t.muted;
        let muted = t.muted;
        if let Some(v) = self.views.view_of(tab_id) {
            v.set_audio_muted(muted);
        }
        self.changed();
    }

    pub fn copy_url(&mut self, tab: Option<&str>) {
        let aux = if tab.is_none() { self.aux_target_tab() } else { None };
        let id = tab.map(str::to_string).or(aux).or_else(|| self.active_tab_id());
        if let Some(t) = id.and_then(|i| self.state().tabs.get(&i)) {
            crate::app::platform::copy_text(&t.url);
        }
    }

    /// Zoom by `delta` levels; 0 restores the default (Settings > Appearance, else 100 percent).
    pub fn zoom_page(&mut self, tab_id: &str, delta: i32) {
        let Some(v) = self.views.view_of(tab_id) else { return };
        let level = if delta == 0 {
            let pct = f64::from(self.state().settings.default_zoom.unwrap_or(100));
            (pct / 100.0).ln() / 1.2f64.ln()
        } else {
            v.zoom_level() + f64::from(delta)
        };
        let level = level.clamp(-7.6, 9.0);
        v.set_zoom_level(level);
        let pct = (1.2f64.powf(level) * 100.0).round() as u32;
        self.patch_one(tab_id, "zoom", serde_json::json!(pct), false, false);
        self.open_zoom_bubble(tab_id);
    }

    pub fn pin_toggle(&mut self, tab_id: &str) {
        if self.model(|m| m.toggle_pin(tab_id)) {
            self.changed();
        }
    }

    pub fn favorite_toggle(&mut self, tab_id: &str) {
        if self.model(|m| m.toggle_favorite(tab_id)) {
            self.changed();
        }
    }

    pub fn move_tab_step(&mut self, tab_id: &str, dir: i32) {
        if self.model(|m| m.move_tab_step(tab_id, dir)) {
            self.changed();
        }
    }

    /// Takes a pinned/favorite tab's view down (the tab stays, unloaded).
    pub fn unload_tab(&mut self, tab_id: &str) {
        self.views.destroy(tab_id);
        if let Some(t) = self.store.data.state.tabs.get_mut(tab_id) {
            t.loaded = false;
        }
        self.changed();
    }

    /// Where a tab lives (for menus).
    pub fn zone_of(&self, tab_id: &str) -> Option<Zone> {
        signet_core::model::locate(self.state(), tab_id).map(|l| l.zone)
    }

    pub fn is_split_item(&self, tab_id: &str) -> Option<String> {
        match signet_core::model::locate(self.state(), tab_id)?.item {
            Item::Split(s) => Some(s.id),
            _ => None,
        }
    }
}

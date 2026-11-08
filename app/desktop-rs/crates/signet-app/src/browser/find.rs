//! Port of `browser/find.ts`: find in page.

use signet_core::api::{FindOptions, UiEvent};
use signet_core::types::FindState;

use super::Core;
use crate::windows::{Cmd, Surface};

impl Core {
    /// Cmd+F: opens the bar on the active tab, or gives it focus again.
    pub fn show_find(&mut self) {
        let Some(tab) = self.active_tab_id() else { return };
        if self.win.is_none() || self.ov.payload.is_some() || self.views.view_of(&tab).is_none() {
            return;
        }
        if self.find.tab_id.as_deref() != Some(&tab) {
            self.close_find(false);
            self.find.session += 1;
            self.find.tab_id = Some(tab.clone());
            self.find.state = FindState { session: self.find.session, query: String::new(), active: 0, matches: 0, hue: self.hue(), closing: None };
        }
        self.emit(UiEvent::FindState(self.find.state.clone()));
        self.changed_view();
        // Already open and the keyboard went to the page since: take it back.
        self.send(Cmd::MakeKey(Surface::Find));
    }

    /// The find bar view subscribed (its window is created after the first state): send it the current state.
    pub fn find_ready(&mut self) {
        if self.find.tab_id.is_some() {
            self.emit(UiEvent::FindState(self.find.state.clone()));
        }
    }

    /// Cmd+G / Shift+Cmd+G: next or previous match; opens the bar if nothing is being searched.
    pub fn find_step(&mut self, forward: bool) {
        let query = self.find.state.query.clone();
        match (&self.find.tab_id, query.is_empty()) {
            (Some(t), false) => {
                if let Some(v) = self.views.view_of(t) {
                    v.find(&query, forward, false, true);
                }
            }
            _ => self.show_find(),
        }
    }

    pub fn find(&mut self, query: &str, opts: FindOptions) {
        let Some(tab) = self.find.tab_id.clone() else { return };
        self.find.state.query = query.to_string();
        let Some(v) = self.views.view_of(&tab) else { return };
        if query.is_empty() {
            v.stop_find(true);
            self.find.state.active = 0;
            self.find.state.matches = 0;
            self.emit(UiEvent::FindState(self.find.state.clone()));
        } else {
            v.find(query, opts.forward, false, opts.find_next);
        }
    }

    /// Closes the bar (Escape, cross) and gives the focus back to the page. `refocus` asks for the page focus.
    pub fn close_find(&mut self, refocus: bool) {
        let Some(tab) = self.find.tab_id.take() else { return };
        if let Some(v) = self.views.view_of(&tab) {
            v.stop_find(true);
        }
        self.find.state.closing = Some(true);
        self.emit(UiEvent::FindState(FindState { closing: Some(true), ..self.find.state.clone() }));
        if refocus {
            self.request_focus();
        }
        self.changed_view();
    }

    pub fn on_find_result(&mut self, tab: &str, count: i32, ordinal: i32, final_update: bool) {
        if !final_update || self.find.tab_id.as_deref() != Some(tab) {
            return;
        }
        self.find.state.active = ordinal.max(0) as u32;
        self.find.state.matches = count.max(0) as u32;
        self.emit(UiEvent::FindState(self.find.state.clone()));
    }
}

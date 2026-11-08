//! Ports `app/desktop/src/main/model/opening.ts`: opening tabs, where the new tab is placed among the day tabs.

use super::locate::{active_space_idx, locate};
use super::Model;
use crate::factory::new_tab;
use crate::tree::{group_range, insert_today, tab_item};
use crate::types::{Item, NewTabPosition, TabItem, Zone};

#[derive(Debug, Clone, Default)]
pub struct OpenOptions {
    pub activate: bool,
    /// Right after this day tab (otherwise at the top or bottom, per the setting).
    pub after_tab_id: Option<String>,
    /// Tab the link was opened from: if it is among the current space's day tabs, the new tab joins its group.
    pub opener_id: Option<String>,
}

impl Model<'_> {
    /// Where a new tab goes among the day tabs ("New tabs" setting): at the top, or at the end.
    pub fn new_tab_index(&self, space_idx: usize) -> usize {
        if self.data.state.settings.new_tab_position == Some(NewTabPosition::Bottom) { self.data.state.spaces[space_idx].today.len() } else { 0 }
    }

    /// Creates a tab in the active space. If it comes from a day tab of that space (`opener_id`), it joins its group
    /// (created if needed), at the end; otherwise it is placed after `after_tab_id`, or at the top or bottom per the
    /// setting. Returns the new tab id.
    pub fn open(&mut self, url: &str, opts: &OpenOptions) -> String {
        let tab = new_tab(url, "");
        let id = tab.id.clone();
        self.data.state.tabs.insert(id.clone(), tab);
        let space_idx = active_space_idx(&self.data.state);
        let opener = opts.opener_id.as_deref().and_then(|o| locate(&self.data.state, o));
        match opener.filter(|o| o.zone == Zone::Today && o.space == Some(space_idx) && !o.item.is_folder()) {
            Some(op) => {
                let group = match op.item.group() {
                    Some(g) => g.to_string(),
                    None => self.env.new_id(),
                };
                let today = &mut self.data.state.spaces[space_idx].today;
                today[op.index].set_group(Some(group.clone()));
                let at = group_range(today, &group).map_or(op.index + 1, |r| r.1);
                today.insert(at, Item::Tab(TabItem { tab_id: id.clone(), group: Some(group) }));
            }
            None => {
                let after = opts.after_tab_id.as_deref().and_then(|a| locate(&self.data.state, a));
                let index = match after {
                    Some(a) if a.zone == Zone::Today && a.space == Some(space_idx) => a.index + 1,
                    _ => self.new_tab_index(space_idx),
                };
                insert_today(&mut self.data.state.spaces[space_idx].today, index, tab_item(&id), None);
            }
        }
        if opts.activate {
            self.set_active(space_idx, Some(&id));
        }
        id
    }

    /// Creates an active tab in another space (routing rule), which becomes the active space.
    pub fn open_in(&mut self, target_idx: usize, url: &str) -> String {
        let tab = new_tab(url, "");
        let id = tab.id.clone();
        self.data.state.tabs.insert(id.clone(), tab);
        let at = self.new_tab_index(target_idx);
        insert_today(&mut self.data.state.spaces[target_idx].today, at, tab_item(&id), None);
        self.data.state.active_space_id = self.data.state.spaces[target_idx].id.clone();
        self.set_active(target_idx, Some(&id));
        id
    }

    /// Gives a detached tab a place (among the active space's day tabs, like a new tab) and activates it.
    pub fn adopt(&mut self, tab_id: &str) {
        self.data.detached.remove(tab_id);
        let space_idx = active_space_idx(&self.data.state);
        let at = self.new_tab_index(space_idx);
        insert_today(&mut self.data.state.spaces[space_idx].today, at, tab_item(tab_id), None);
        self.env.reconcile_sessions(&[tab_id.to_string()]);
        self.set_active(space_idx, Some(tab_id));
    }
}

//! Ports `app/desktop/src/main/model/removal.ts`: removing tabs (closing, unloading pinned tabs, destroying views and data).

use super::locate::locate;
use super::Model;
use crate::types::Item;

impl Model<'_> {
    /// Destroys the views then deletes the tabs and their history.
    pub fn drop_tabs(&mut self, ids: &[String]) {
        for id in ids {
            self.env.destroy_view(&mut self.data.state, id);
            self.data.state.tabs.shift_remove(id);
            self.data.histories.shift_remove(id);
        }
    }

    /// Removes the tab from its list, destroys its view and deletes it, then picks a new active tab.
    pub fn remove(&mut self, tab_id: &str) {
        let loc = self.extract(tab_id, false);
        self.drop_tabs(&[tab_id.to_string()]);
        if let Some(loc) = loc {
            self.repick(&loc, &[tab_id.to_string()], true);
        }
    }

    /// Closes the tab. A pinned tab is only unloaded and reset to its "home" address; the others join the reopen stack
    /// then disappear. False if the tab is not found.
    pub fn close(&mut self, tab_id: &str) -> bool {
        let Some(tab) = self.data.state.tabs.get(tab_id) else { return false };
        let pinned_url = tab.pinned_url.clone();
        let Some(mut loc) = locate(&self.data.state, tab_id) else { return false };
        if let Some(home) = pinned_url {
            // Closing a pinned tab = unloading it and resetting it to its "home" URL.
            // In a split view, take it out first: otherwise the layout would immediately recreate it.
            if matches!(loc.item, Item::Split(_)) {
                self.remove_from_split(tab_id);
                loc = locate(&self.data.state, tab_id).unwrap_or(loc);
            }
            self.env.destroy_view(&mut self.data.state, tab_id);
            if let Some(t) = self.data.state.tabs.get_mut(tab_id) {
                t.url = home;
            }
            self.repick(&loc, &[tab_id.to_string()], false);
            return true;
        }
        self.remember(tab_id, Some(&loc), None, None);
        self.remove(tab_id);
        true
    }
}

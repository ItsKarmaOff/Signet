//! Port of `browser/sleep.ts`: tabs hidden for a long time are put to sleep (view destroyed, address kept).

use signet_core::ids::now_ms;

use super::Core;

const DEFAULT_SLEEP_MINUTES: u32 = 30;

impl Core {
    /// Periodic check (every minute while the main window exists).
    pub fn discard_idle(&mut self) {
        if self.win.is_none() || self.dragging {
            return;
        }
        if self.downloads.any_active() {
            return;
        }
        let minutes = self.state().settings.tab_sleep_minutes.unwrap_or(DEFAULT_SLEEP_MINUTES);
        if minutes == 0 {
            return;
        }
        let limit = now_ms() - i64::from(minutes) * 60_000;
        let ids: Vec<String> = self.hidden_since.iter().filter(|(_, since)| **since <= limit).map(|(id, _)| id.clone()).collect();
        let mut any = false;
        for id in ids {
            if !self.sleepable(&id) {
                continue;
            }
            self.log(format!("sleep: {id} hidden for {} min", (now_ms() - self.hidden_since.get(&id).copied().unwrap_or(0)) / 60_000));
            self.views.destroy(&id);
            self.hidden_since.remove(&id);
            if let Some(t) = self.store.data.state.tabs.get_mut(&id) {
                t.loaded = false;
                t.loading = false;
            }
            any = true;
        }
        if any {
            self.dirty.needs_full = true;
            self.apply();
        }
    }

    /// A tab that may be put to sleep now: hidden, quiet, not special.
    fn sleepable(&self, id: &str) -> bool {
        let Some(tab) = self.state().tabs.get(id) else { return false };
        self.views.view_of(id).is_some()
            && !self.store.data.detached.contains(id)
            && !self.shown_tabs.iter().any(|s| s == id)
            && self.ov.peek_tab.as_deref() != Some(id)
            && !tab.audible
            && !tab.loading
            && !self.phase3.keeps_awake(id)
            && !self.devtools_open(id)
            && !self.pip_owns(id)
    }
}

//! Ports `app/desktop/src/main/model/navigation.ts`: keyboard navigation between tabs, in the order the bar shows them.

use super::locate::active_space;
use crate::core::store::State;
use crate::tree::item_tab_ids;
use crate::types::Item;

/// Tab ids in the order the bar shows them: favorites, pinned, then day tabs (folders and groups flattened).
pub fn tab_ids_in_order(state: &State) -> Vec<String> {
    let space = active_space(state);
    let mut v = state.favorites.clone();
    v.extend(space.pinned.iter().chain(space.today.iter()).flat_map(item_tab_ids));
    v
}

/// Next or previous tab: favorites, pinned, then day tabs.
pub fn cycle_target(state: &State, step: i64) -> Option<String> {
    let space = active_space(state);
    let ids = tab_ids_in_order(state);
    if ids.is_empty() {
        return None;
    }
    let n = ids.len() as i64;
    let i = space.active_tab_id.as_ref().and_then(|a| ids.iter().position(|t| t == a)).map_or(-1, |i| i as i64);
    let target = if i < 0 { if step < 0 { n - 1 } else { 0 } } else { (i + step).rem_euclid(n) };
    ids.get(target as usize).cloned()
}

/// Cmd+1 to Cmd+8: nth tab as the bar shows it (favorites, pinned, day); Cmd+9: the last. A collapsed folder hides its
/// tabs (and in the "tabs on top" layout, all folders are collapsed).
pub fn number_target(state: &State, n: usize, horizontal: bool) -> Option<String> {
    fn open(items: &[Item], horizontal: bool) -> Vec<String> {
        items
            .iter()
            .flat_map(|i| match i {
                Item::Folder(f) => if f.open && !horizontal { open(&f.items, horizontal) } else { vec![] },
                other => item_tab_ids(other),
            })
            .collect()
    }
    let space = active_space(state);
    let mut ids = state.favorites.clone();
    ids.extend(open(&space.pinned, horizontal));
    ids.extend(open(&space.today, horizontal));
    if n >= 9 { ids.last().cloned() } else { n.checked_sub(1).and_then(|i| ids.get(i).cloned()) }
}

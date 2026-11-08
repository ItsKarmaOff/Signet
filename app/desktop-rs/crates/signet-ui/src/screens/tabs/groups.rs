//! Port of `tabs/groups.ts`: today tab groups are runs of consecutive items carrying the same `group`
//! (header in `Space.groups`). Pure functions, shared by the sidebar and the tab row.

use signet_core::types::{DragSource, DropTarget, Item, Zone};

/// Insertion marker of a row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Indicator {
    Before,
    After,
}

/// Drop target, with `into` = id of the folder (or group) receiving the item, to highlight its row.
#[derive(Clone, Debug, PartialEq)]
pub struct Drop {
    pub target: DropTarget,
    pub into: Option<String>,
}

impl Drop {
    pub fn new(zone: Zone, space_id: &str, index: usize) -> Self {
        Drop { target: DropTarget { zone, space_id: space_id.to_string(), folder_id: None, group_id: None, index }, into: None }
    }
    pub fn folder(mut self, id: Option<String>) -> Self {
        self.target.folder_id = id;
        self
    }
    pub fn group(mut self, id: Option<String>) -> Self {
        self.target.group_id = id;
        self
    }
    pub fn into(mut self, id: String) -> Self {
        self.into = Some(id);
        self
    }
}

/// Group label of an item (None for a folder, a missing item or an ungrouped item).
pub fn group_of(item: Option<&Item>) -> Option<&str> {
    item.and_then(|i| i.group())
}

/// A whole group, or a single item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
    /// None: single item, no group.
    pub group: Option<String>,
    pub start: usize,
    pub end: usize,
}

/// Splits the list into runs: a whole group (2 members or more), or a single item.
pub fn runs_of(items: &[Item]) -> Vec<Run> {
    let mut runs = Vec::new();
    let mut i = 0;
    while i < items.len() {
        let group = group_of(items.get(i));
        let mut end = i + 1;
        if let Some(g) = group {
            while end < items.len() && group_of(items.get(end)) == Some(g) {
                end += 1;
            }
        }
        match group {
            Some(g) if end - i > 1 => runs.push(Run { group: Some(g.to_string()), start: i, end }),
            _ => runs.push(Run { group: None, start: i, end: i + 1 }),
        }
        i = end;
    }
    runs
}

/// Where the insertion marker goes in today tabs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TodayMark {
    /// Can be -1 (empty list).
    pub index: i64,
    pub side: Indicator,
    /// The marker goes after the whole group card.
    pub card: bool,
}

/// Where to draw the insertion marker in today tabs: "after" the last member when dropping at the end of a group to
/// join it (or at the end of the list), otherwise "before" the targeted item.
pub fn today_mark(items: &[Item], drop: Option<&Drop>) -> Option<TodayMark> {
    let d = drop?;
    if d.target.zone != Zone::Today || d.target.folder_id.is_some() || d.into.is_some() {
        return None;
    }
    let idx = d.target.index;
    // End of list with no group targeted and last item in a group: land after the card, without joining it.
    if d.target.group_id.is_none() && !items.is_empty() && idx >= items.len() && group_of(items.last()).is_some() {
        return Some(TodayMark { index: items.len() as i64 - 1, side: Indicator::After, card: true });
    }
    let joins_end = match &d.target.group_id {
        Some(g) => group_of(idx.checked_sub(1).and_then(|p| items.get(p))) == Some(g.as_str()) && group_of(items.get(idx)) != Some(g.as_str()),
        None => false,
    };
    if joins_end || idx >= items.len() {
        return Some(TodayMark { index: idx as i64 - 1, side: Indicator::After, card: false });
    }
    Some(TodayMark { index: idx as i64, side: Indicator::Before, card: false })
}

/// Dragging a whole group over another group: never insert in the middle, only before or after it depending on the
/// hovered half of its card. `top`/`extent`: the card's final position and size along the axis (without in-progress
/// offsets), `pointer`: pointer position along the same axis. 4 px dead band around the middle.
pub fn around_run(top: f32, extent: f32, run: &Run, pointer: f32, current: Option<usize>) -> usize {
    let mid = top + extent / 2.0;
    if let Some(c) = current
        && (c == run.start || c == run.end)
        && (pointer - mid).abs() <= 4.0
    {
        return c;
    }
    if pointer < mid { run.start } else { run.end }
}

/// Dragging a block (whole group or folder that will become a group): it never lands in the middle of a group.
pub fn is_block(src: Option<&DragSource>) -> bool {
    matches!(src, Some(DragSource::Group { .. }) | Some(DragSource::Folder { .. }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use signet_core::types::{FolderItem, TabItem};

    fn tab(id: &str, group: Option<&str>) -> Item {
        Item::Tab(TabItem { tab_id: id.into(), group: group.map(|g| g.to_string()) })
    }
    fn folder(id: &str) -> Item {
        Item::Folder(FolderItem { id: id.into(), name: id.into(), ..Default::default() })
    }
    fn run(group: Option<&str>, start: usize, end: usize) -> Run {
        Run { group: group.map(|g| g.to_string()), start, end }
    }
    fn drop(zone: Zone, index: usize) -> Drop {
        Drop::new(zone, "s", index)
    }

    #[test]
    fn group_of_reads_a_tab_group_nothing_for_a_folder_or_none() {
        assert_eq!(group_of(Some(&tab("a", Some("g")))), Some("g"));
        assert_eq!(group_of(Some(&tab("a", None))), None);
        assert_eq!(group_of(Some(&folder("f"))), None);
        assert_eq!(group_of(None), None);
    }

    #[test]
    fn runs_of_empty_list() {
        assert!(runs_of(&[]).is_empty());
    }

    #[test]
    fn runs_of_single_items_without_a_group() {
        assert_eq!(runs_of(&[tab("a", None), tab("b", None)]), vec![run(None, 0, 1), run(None, 1, 2)]);
    }

    #[test]
    fn runs_of_groups_consecutive_items_of_the_same_group() {
        let items = [tab("a", None), tab("b", Some("g")), tab("c", Some("g")), tab("d", None)];
        assert_eq!(runs_of(&items), vec![run(None, 0, 1), run(Some("g"), 1, 3), run(None, 3, 4)]);
    }

    #[test]
    fn runs_of_single_member_group_is_a_single_item() {
        assert_eq!(runs_of(&[tab("a", Some("g")), tab("b", None)]), vec![run(None, 0, 1), run(None, 1, 2)]);
    }

    #[test]
    fn runs_of_neighbor_groups_stay_separate_and_a_split_group_gives_two_runs() {
        let items = [tab("a", Some("g")), tab("b", Some("g")), tab("c", Some("h")), tab("d", Some("h"))];
        assert_eq!(runs_of(&items), vec![run(Some("g"), 0, 2), run(Some("h"), 2, 4)]);
        let items = [tab("a", Some("g")), tab("b", None), tab("c", Some("g")), tab("d", Some("g"))];
        assert_eq!(runs_of(&items), vec![run(None, 0, 1), run(None, 1, 2), run(Some("g"), 2, 4)]);
    }

    #[test]
    fn runs_of_a_folder_breaks_a_group() {
        let items = [tab("a", Some("g")), folder("f"), tab("b", Some("g"))];
        assert!(runs_of(&items).iter().all(|r| r.group.is_none()));
    }

    fn items() -> Vec<Item> {
        vec![tab("a", None), tab("b", Some("g")), tab("c", Some("g")), tab("d", None)]
    }

    #[test]
    fn today_mark_null_outside_today_in_a_folder_or_when_entering_an_item() {
        let items = items();
        assert_eq!(today_mark(&items, None), None);
        assert_eq!(today_mark(&items, Some(&drop(Zone::Pinned, 0))), None);
        assert_eq!(today_mark(&items, Some(&drop(Zone::Today, 0).folder(Some("f".into())))), None);
        assert_eq!(today_mark(&items, Some(&drop(Zone::Today, 0).into("x".into()))), None);
    }

    #[test]
    fn today_mark_before_the_targeted_item_by_default() {
        let items = items();
        assert_eq!(today_mark(&items, Some(&drop(Zone::Today, 1))), Some(TodayMark { index: 1, side: Indicator::Before, card: false }));
        assert_eq!(today_mark(&items, Some(&drop(Zone::Today, 0))), Some(TodayMark { index: 0, side: Indicator::Before, card: false }));
    }

    #[test]
    fn today_mark_after_the_last_item_at_the_end_of_the_list() {
        let items = items();
        assert_eq!(today_mark(&items, Some(&drop(Zone::Today, 4))), Some(TodayMark { index: 3, side: Indicator::After, card: false }));
        assert_eq!(today_mark(&[], Some(&drop(Zone::Today, 0))), Some(TodayMark { index: -1, side: Indicator::After, card: false }));
    }

    #[test]
    fn today_mark_end_of_list_with_last_item_in_a_group_lands_after_the_card() {
        let list = [tab("a", None), tab("b", Some("g")), tab("c", Some("g"))];
        assert_eq!(today_mark(&list, Some(&drop(Zone::Today, 3))), Some(TodayMark { index: 2, side: Indicator::After, card: true }));
    }

    #[test]
    fn today_mark_joining_the_end_of_a_group_is_after_its_last_member() {
        let items = items();
        let d = drop(Zone::Today, 3).group(Some("g".into()));
        assert_eq!(today_mark(&items, Some(&d)), Some(TodayMark { index: 2, side: Indicator::After, card: false }));
    }

    #[test]
    fn today_mark_group_targeted_but_continuing_at_the_index_is_before() {
        let items = items();
        let d = drop(Zone::Today, 2).group(Some("g".into()));
        assert_eq!(today_mark(&items, Some(&d)), Some(TodayMark { index: 2, side: Indicator::Before, card: false }));
    }

    // Card 100 px tall at top = 100 (middle 150); the same numbers serve the horizontal axis.
    fn rn() -> Run {
        run(Some("g"), 2, 5)
    }

    #[test]
    fn around_run_before_in_the_first_half_after_otherwise() {
        assert_eq!(around_run(100.0, 100.0, &rn(), 120.0, None), 2);
        assert_eq!(around_run(100.0, 100.0, &rn(), 180.0, None), 5);
    }

    #[test]
    fn around_run_follows_the_horizontal_axis() {
        // 200 px wide at left = 0 (middle 100).
        assert_eq!(around_run(0.0, 200.0, &rn(), 50.0, None), 2);
        assert_eq!(around_run(0.0, 200.0, &rn(), 150.0, None), 5);
    }

    #[test]
    fn around_run_right_in_the_middle_is_after() {
        assert_eq!(around_run(100.0, 100.0, &rn(), 150.0, None), 5);
    }

    #[test]
    fn around_run_dead_band_keeps_the_current_target_if_it_touches_the_group() {
        assert_eq!(around_run(100.0, 100.0, &rn(), 148.0, Some(5)), 5);
        assert_eq!(around_run(100.0, 100.0, &rn(), 152.0, Some(2)), 2);
        // Outside the dead band or unrelated target: decide.
        assert_eq!(around_run(100.0, 100.0, &rn(), 140.0, Some(5)), 2);
        assert_eq!(around_run(100.0, 100.0, &rn(), 149.0, Some(9)), 2);
    }

    #[test]
    fn is_block_true_for_a_dragged_group_or_folder() {
        assert!(is_block(Some(&DragSource::Group { id: "g".into() })));
        assert!(is_block(Some(&DragSource::Folder { id: "f".into() })));
        assert!(!is_block(Some(&DragSource::Tab { tab_id: "t".into() })));
        assert!(!is_block(Some(&DragSource::Split { id: "s".into() })));
        assert!(!is_block(None));
    }
}

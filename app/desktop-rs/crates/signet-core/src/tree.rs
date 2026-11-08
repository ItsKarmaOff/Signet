//! Ports `packages/shared/src/tree.ts` and `app/desktop/src/main/core/tree.ts`: tree utilities for item lists
//! (tabs, split views, folders).
//!
//! The TS code hands out references into lists; Rust uses index paths instead. A `Slot` is the path of indices
//! from the root list to an item (every index but the last designates a folder).

use crate::ids::new_id;
use crate::split::{sanitize_split_item, MAX_PANES};
use crate::types::{FolderItem, GithubQuery, Item, LiveFolder, LiveSource, Space, SplitItem, SplitLayout, TabGroup, TabInfo};
use indexmap::IndexMap;
use serde_json::Value;
use std::collections::HashSet;

pub type Folder = FolderItem;

/// Maximum display depth of folders (beyond it, the UI no longer indents).
pub const MAX_FOLDER_DEPTH: usize = 6;

/// Position of an item in the tree: indices from the root list. Every index but the last is a folder.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Slot {
    pub path: Vec<usize>,
}

impl Slot {
    /// Rank of the item in its list.
    pub fn index(&self) -> usize {
        self.path.last().copied().unwrap_or(0)
    }
    /// Path of folders (indices) leading to the list that contains the item; empty = root list.
    pub fn folders(&self) -> &[usize] {
        &self.path[..self.path.len().saturating_sub(1)]
    }
    /// Path of the parent folder, if any (same as `folders()` when not empty).
    pub fn parent(&self) -> Option<&[usize]> {
        let f = self.folders();
        if f.is_empty() { None } else { Some(f) }
    }
}

pub fn tab_item(tab_id: &str) -> Item {
    Item::tab(tab_id)
}

/// Ids of an item's tabs, descending into folders.
pub fn item_tab_ids(item: &Item) -> Vec<String> {
    match item {
        Item::Tab(t) => vec![t.tab_id.clone()],
        Item::Split(s) => s.tab_ids.clone(),
        Item::Folder(f) => f.items.iter().flat_map(item_tab_ids).collect(),
    }
}

/// Ids of the tabs of a whole list.
pub fn list_tab_ids(items: &[Item]) -> Vec<String> {
    items.iter().flat_map(item_tab_ids).collect()
}

/// The item at `path` (see `Slot`).
pub fn item_at<'a>(items: &'a [Item], path: &[usize]) -> Option<&'a Item> {
    let (first, rest) = path.split_first()?;
    let item = items.get(*first)?;
    if rest.is_empty() { Some(item) } else { item_at(&item.as_folder()?.items, rest) }
}

pub fn item_at_mut<'a>(items: &'a mut [Item], path: &[usize]) -> Option<&'a mut Item> {
    let (first, rest) = path.split_first()?;
    let item = items.get_mut(*first)?;
    if rest.is_empty() { Some(item) } else { item_at_mut(&mut item.as_folder_mut()?.items, rest) }
}

/// The list reached through a path of folder indices, from a root `Vec`.
pub fn list_in<'a>(root: &'a Vec<Item>, folders: &[usize]) -> Option<&'a Vec<Item>> {
    match folders.split_first() {
        None => Some(root),
        Some((first, rest)) => list_in(&root.get(*first)?.as_folder()?.items, rest),
    }
}

pub fn list_in_mut<'a>(root: &'a mut Vec<Item>, folders: &[usize]) -> Option<&'a mut Vec<Item>> {
    match folders.split_first() {
        None => Some(root),
        Some((first, rest)) => list_in_mut(&mut root.get_mut(*first)?.as_folder_mut()?.items, rest),
    }
}

/// Depth-first traversal in display order; `f` returning `true` stops the traversal (and `walk` returns `true`).
pub fn walk(items: &[Item], f: &mut dyn FnMut(&Slot, &Item) -> bool) -> bool {
    fn go(items: &[Item], prefix: &mut Vec<usize>, f: &mut dyn FnMut(&Slot, &Item) -> bool) -> bool {
        for (i, item) in items.iter().enumerate() {
            prefix.push(i);
            if f(&Slot { path: prefix.clone() }, item) {
                prefix.pop();
                return true;
            }
            if let Item::Folder(folder) = item
                && go(&folder.items, prefix, f)
            {
                prefix.pop();
                return true;
            }
            prefix.pop();
        }
        false
    }
    go(items, &mut Vec::new(), f)
}

pub fn find_slot(items: &[Item], pred: impl Fn(&Item) -> bool) -> Option<Slot> {
    let mut found = None;
    walk(items, &mut |slot, item| {
        if pred(item) {
            found = Some(slot.clone());
            true
        } else {
            false
        }
    });
    found
}

/// Position of the item (single tab or split view) that contains the tab.
pub fn find_tab_slot(items: &[Item], tab_id: &str) -> Option<Slot> {
    find_slot(items, |i| !i.is_folder() && item_tab_ids(i).iter().any(|t| t == tab_id))
}

pub fn find_folder_slot(items: &[Item], folder_id: &str) -> Option<Slot> {
    find_slot(items, |i| matches!(i, Item::Folder(f) if f.id == folder_id))
}

pub fn find_split_slot(items: &[Item], split_id: &str) -> Option<Slot> {
    find_slot(items, |i| matches!(i, Item::Split(s) if s.id == split_id))
}

/// The folder with this id, anywhere in the tree.
pub fn folder_by_id<'a>(items: &'a [Item], id: &str) -> Option<&'a FolderItem> {
    let slot = find_folder_slot(items, id)?;
    item_at(items, &slot.path)?.as_folder()
}

pub fn folder_by_id_mut<'a>(items: &'a mut [Item], id: &str) -> Option<&'a mut FolderItem> {
    let slot = find_folder_slot(items, id)?;
    item_at_mut(items, &slot.path)?.as_folder_mut()
}

/// True if `id` designates `folder` itself or one of its descendants.
pub fn folder_contains(folder: &FolderItem, id: &str) -> bool {
    folder.id == id || folder.items.iter().any(|i| matches!(i, Item::Folder(f) if folder_contains(f, id)))
}

pub fn remove_at(list: &mut Vec<Item>, index: usize) -> Item {
    list.remove(index)
}

/// Inserts at `index` (clamped to the list size).
pub fn insert_at(list: &mut Vec<Item>, index: usize, item: Item) {
    list.insert(index.min(list.len()), item);
}

/* ---------- Tab groups (day tabs) ---------- */

pub fn group_of(item: Option<&Item>) -> Option<&str> {
    item.and_then(Item::group)
}

/// Group label of an item (None for folders).
pub fn group_of_item(item: &Item) -> Option<&str> {
    item.group()
}

/// `[start, end[` bounds of the run of items of the group in the list.
pub fn group_range(list: &[Item], id: &str) -> Option<(usize, usize)> {
    let start = list.iter().position(|i| i.group() == Some(id))?;
    let mut end = start;
    while end < list.len() && list[end].group() == Some(id) {
        end += 1;
    }
    Some((start, end))
}

/// Keeps the group invariant: a single consecutive run of at least two items per group.
/// Members detached from their run and groups reduced to one item lose the label.
pub fn normalize_groups(list: &mut [Item]) {
    let mut seen: HashSet<String> = HashSet::new();
    let mut i = 0;
    while i < list.len() {
        let Some(id) = list[i].group().map(String::from) else {
            i += 1;
            continue;
        };
        let mut end = i;
        while end < list.len() && list[end].group() == Some(id.as_str()) {
            end += 1;
        }
        if seen.contains(&id) || end - i < 2 {
            for item in &mut list[i..end] {
                item.set_group(None);
            }
        } else {
            seen.insert(id);
        }
        i = end;
    }
}

/// Hues offered to new groups, in order: blue, green, purple, orange, pink, turquoise, yellow, red.
pub const GROUP_HUES: [f64; 8] = [215.0, 140.0, 265.0, 28.0, 330.0, 180.0, 48.0, 0.0];

/// First hue that no other group of the space uses (otherwise wrap around the list).
pub fn pick_group_hue(groups: &IndexMap<String, TabGroup>) -> f64 {
    GROUP_HUES.iter().copied().find(|h| !groups.values().any(|g| g.hue == *h)).unwrap_or(GROUP_HUES[groups.len() % GROUP_HUES.len()])
}

/// Default title of a group: that of the tab that opened it (chosen name, page title, otherwise the domain).
fn default_group_name(item: &Item, tabs: &IndexMap<String, TabInfo>) -> String {
    let Some(tab) = item_tab_ids(item).first().and_then(|id| tabs.get(id)) else { return String::new() };
    let title = tab.custom_title.as_deref().filter(|t| !t.is_empty()).unwrap_or(&tab.title);
    if !title.is_empty() {
        return title.chars().take(80).collect();
    }
    match url::Url::parse(&tab.url) {
        Ok(u) => u.host_str().map(|h| h.strip_prefix("www.").unwrap_or(h).to_string()).unwrap_or_default(),
        Err(_) => String::new(),
    }
}

/// Group invariant of the space, then headers: one per live group (created as needed), none for a vanished group.
pub fn sync_groups(space: &mut Space, tabs: &IndexMap<String, TabInfo>) {
    normalize_groups(&mut space.today);
    let mut live: IndexMap<String, usize> = IndexMap::new();
    for (i, item) in space.today.iter().enumerate() {
        if let Some(id) = item.group() {
            live.entry(id.to_string()).or_insert(i);
        }
    }
    let groups = space.groups.get_or_insert_with(IndexMap::new);
    groups.retain(|id, _| live.contains_key(id));
    for (id, first) in &live {
        if !groups.contains_key(id) {
            let g = TabGroup { name: default_group_name(&space.today[*first], tabs), icon: None, hue: pick_group_hue(groups), open: true };
            groups.insert(id.clone(), g);
        }
    }
    if groups.is_empty() {
        space.groups = None;
    }
}

/// Insertion position that cuts no group: in the middle of a group, go just after it.
pub fn outside_groups(list: &[Item], index: usize) -> usize {
    let at = index.min(list.len());
    let around = if at > 0 { list[at - 1].group() } else { None };
    match around {
        Some(g) if list.get(at).and_then(Item::group) == Some(g) => group_range(list, g).map(|r| r.1).unwrap_or(at),
        _ => at,
    }
}

/// Single tabs and split views of a list, descending into folders (in display order).
pub fn leaves_of(items: &[Item]) -> Vec<Item> {
    items.iter().flat_map(|i| if let Item::Folder(f) = i { leaves_of(&f.items) } else { vec![i.clone()] }).collect()
}

/// Removes any group label (lists outside the day tabs).
pub fn strip_groups(items: &mut [Item]) {
    for item in items {
        match item {
            Item::Folder(f) => strip_groups(&mut f.items),
            other => other.set_group(None),
        }
    }
}

/// Inserts an item into the day tabs. It joins `group_id` if it touches that group at that spot, or the group
/// surrounding it if it lands in the middle of a run; otherwise it belongs to no group.
pub fn insert_today(list: &mut Vec<Item>, index: usize, mut item: Item, group_id: Option<&str>) {
    let at = index.min(list.len());
    if !item.is_folder() {
        let prev = if at > 0 { list[at - 1].group() } else { None };
        let next = list.get(at).and_then(Item::group);
        let new = if group_id.is_some() && (prev == group_id || next == group_id) {
            group_id.map(String::from)
        } else if prev.is_some() && prev == next {
            prev.map(String::from)
        } else {
            None
        };
        item.set_group(new);
    }
    list.insert(at, item);
}

/// All folders, with their readable path "Parent / Child".
pub fn folder_paths(items: &[Item]) -> Vec<(Slot, String)> {
    let mut out = Vec::new();
    fn go(items: &[Item], prefix: &mut Vec<usize>, names: &mut Vec<String>, out: &mut Vec<(Slot, String)>) {
        for (i, item) in items.iter().enumerate() {
            if let Item::Folder(f) = item {
                prefix.push(i);
                names.push(f.name.clone());
                out.push((Slot { path: prefix.clone() }, names.join(" / ")));
                go(&f.items, prefix, names, out);
                names.pop();
                prefix.pop();
            }
        }
    }
    go(items, &mut Vec::new(), &mut Vec::new(), &mut out);
    out
}

pub fn new_folder(name: &str) -> FolderItem {
    FolderItem { id: new_id(), name: name.into(), open: true, ..Default::default() }
}

/// Validated source of a live folder (read from disk or coming from the UI).
pub fn clean_live_source(value: &Value) -> Option<LiveSource> {
    let kind = value.get("kind")?.as_str()?;
    if kind == "feed" {
        let url = value.get("url")?.as_str()?.trim();
        let ok = regex::Regex::new(r"(?i)^https?://\S+$").ok()?.is_match(url);
        return ok.then(|| LiveSource::Feed { url: url.chars().take(2000).collect() });
    }
    if kind == "github" {
        return match value.get("query")?.as_str()? {
            "review-requested" => Some(LiveSource::Github { query: GithubQuery::ReviewRequested }),
            "mine" => Some(LiveSource::Github { query: GithubQuery::Mine }),
            _ => None,
        };
    }
    None
}

/// State of a live folder re-read from disk: valid source, addresses seen; never "update in progress".
pub fn clean_live_folder(value: &Value) -> Option<LiveFolder> {
    let source = clean_live_source(value.get("source")?)?;
    let mut live = LiveFolder { source, refreshed_at: None, error: None, refreshing: None, seen: None };
    if let Some(n) = value.get("refreshedAt").and_then(Value::as_f64).filter(|n| n.is_finite()) {
        live.refreshed_at = Some(n as i64);
    }
    if let Some(e) = value.get("error").and_then(Value::as_str).filter(|e| !e.is_empty()) {
        live.error = Some(e.chars().take(300).collect());
    }
    if let Some(seen) = value.get("seen").and_then(Value::as_array) {
        let all: Vec<String> = seen.iter().filter_map(|u| u.as_str().map(String::from)).collect();
        let skip = all.len().saturating_sub(200);
        live.seen = Some(all[skip..].to_vec());
    }
    Some(live)
}

fn str_list(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default()
}

/// Cleans a list read from disk (untyped JSON): keeps known tabs, removes empty split views, turns single-tab ones
/// into a tab, normalizes folders (or flattens them if `folders` is false). `folder_fallback_name`: (translated)
/// name of an unnamed folder.
pub fn prune_items(items: &Value, known: &dyn Fn(&str) -> bool, folders: bool, folder_fallback_name: &str) -> Vec<Item> {
    let Some(arr) = items.as_array() else { return Vec::new() };
    let group_of_value = |v: &Value| v.get("group").and_then(Value::as_str).filter(|g| !g.is_empty()).map(String::from);
    let mut out = Vec::new();
    for item in arr {
        if !item.is_object() {
            continue;
        }
        match item.get("kind").and_then(Value::as_str) {
            Some("tab") => {
                if let Some(id) = item.get("tabId").and_then(Value::as_str).filter(|t| known(t)) {
                    out.push(Item::Tab(crate::types::TabItem { tab_id: id.into(), group: group_of_value(item) }));
                }
            }
            Some("split") => {
                let raw_ids = str_list(item.get("tabIds"));
                let tab_ids: Vec<String> = raw_ids.iter().filter(|t| known(t)).take(MAX_PANES).cloned().collect();
                if tab_ids.is_empty() {
                    continue;
                }
                if tab_ids.len() == 1 {
                    out.push(Item::Tab(crate::types::TabItem { tab_id: tab_ids[0].clone(), group: group_of_value(item) }));
                    continue;
                }
                let layout = match item.get("layout").and_then(Value::as_str) {
                    Some("columns") => Some(SplitLayout::Columns),
                    Some("rows") => Some(SplitLayout::Rows),
                    Some("grid") => Some(SplitLayout::Grid),
                    _ => None,
                };
                let mut sizes = item.get("sizes").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_f64).collect::<Vec<_>>());
                // `sizes` with a non-numeric entry is not usable.
                if let (Some(s), Some(a)) = (&sizes, item.get("sizes").and_then(Value::as_array))
                    && s.len() != a.len()
                {
                    sizes = None;
                }
                if tab_ids.len() != raw_ids.len() {
                    sizes = None;
                }
                let mut split = SplitItem {
                    id: item.get("id").and_then(Value::as_str).map(String::from).unwrap_or_default(),
                    group: group_of_value(item),
                    tab_ids,
                    layout,
                    sizes,
                };
                sanitize_split_item(&mut split);
                out.push(Item::Split(split));
            }
            Some("folder") => {
                let inner = prune_items(item.get("items").unwrap_or(&Value::Null), known, folders, folder_fallback_name);
                if !folders {
                    out.extend(inner);
                    continue;
                }
                out.push(Item::Folder(FolderItem {
                    id: item.get("id").and_then(Value::as_str).filter(|s| !s.is_empty()).map(String::from).unwrap_or_else(new_id),
                    name: item.get("name").and_then(Value::as_str).filter(|s| !s.is_empty()).unwrap_or(folder_fallback_name).to_string(),
                    icon: item.get("icon").and_then(Value::as_str).filter(|s| !s.is_empty()).map(|s| s.chars().take(16).collect()),
                    open: item.get("open").and_then(Value::as_bool) != Some(false),
                    items: inner,
                    hue: item.get("hue").and_then(Value::as_f64).filter(|h| h.is_finite()).map(|h| ((h + 0.5).floor()) % 360.0),
                    live: item.get("live").and_then(clean_live_folder),
                }));
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tab(id: &str) -> Item {
        Item::tab(id)
    }
    fn gtab(id: &str, g: &str) -> Item {
        Item::Tab(crate::types::TabItem { tab_id: id.into(), group: Some(g.into()) })
    }
    fn folder(id: &str, name: &str, items: Vec<Item>) -> Item {
        Item::Folder(FolderItem { id: id.into(), name: name.into(), open: true, items, ..Default::default() })
    }
    fn split(id: &str, ids: &[&str]) -> Item {
        Item::Split(SplitItem { id: id.into(), tab_ids: ids.iter().map(|s| s.to_string()).collect(), ..Default::default() })
    }
    /// a, [F1: b, [F2: c], {S: d x}], e
    fn sample() -> Vec<Item> {
        vec![tab("a"), folder("F1", "One", vec![tab("b"), folder("F2", "Two", vec![tab("c")]), split("S", &["d", "x"])]), tab("e")]
    }
    fn label(i: &Item) -> String {
        match i {
            Item::Tab(t) => t.tab_id.clone(),
            Item::Split(s) => s.id.clone(),
            Item::Folder(f) => f.id.clone(),
        }
    }

    #[test]
    fn tab_ids_descend() {
        assert_eq!(item_tab_ids(&tab("a")), ["a"]);
        assert_eq!(item_tab_ids(&split("S", &["x", "y"])), ["x", "y"]);
        assert_eq!(item_tab_ids(&sample()[1]), ["b", "c", "d", "x"]);
    }

    #[test]
    fn walk_order_and_stop() {
        let items = sample();
        let mut seen = Vec::new();
        walk(&items, &mut |_, i| {
            seen.push(label(i));
            false
        });
        assert_eq!(seen, ["a", "F1", "b", "F2", "c", "S", "e"]);
        let mut stop = Vec::new();
        let r = walk(&items, &mut |_, i| {
            stop.push(i.is_folder());
            i.is_folder()
        });
        assert!(r);
        assert_eq!(stop, [false, true]);
    }

    #[test]
    fn slots() {
        let items = sample();
        let c = find_tab_slot(&items, "c").unwrap();
        assert_eq!(c.path, [1, 1, 0]);
        assert_eq!(c.index(), 0);
        let parent = item_at(&items, c.parent().unwrap()).unwrap();
        assert_eq!(label(parent), "F2");
        assert_eq!(label(item_at(&items, &find_tab_slot(&items, "x").unwrap().path).unwrap()), "S");
        assert!(find_tab_slot(&items, "zzz").is_none());
        assert_eq!(find_folder_slot(&items, "F2").unwrap().index(), 1);
        assert!(find_folder_slot(&items, "nope").is_none());
        assert_eq!(find_split_slot(&items, "S").unwrap().folders(), [1]);
        assert!(find_split_slot(&items, "F1").is_none());
    }

    #[test]
    fn contains_leaves_paths() {
        let items = sample();
        let f = items[1].as_folder().unwrap();
        assert!(folder_contains(f, "F1") && folder_contains(f, "F2") && !folder_contains(f, "a"));
        assert_eq!(leaves_of(&items).iter().map(label).collect::<Vec<_>>(), ["a", "b", "c", "S", "e"]);
        assert_eq!(folder_paths(&items).iter().map(|p| p.1.as_str()).collect::<Vec<_>>(), ["One", "One / Two"]);
    }

    #[test]
    fn insert_remove_clamp() {
        let mut l = vec![tab("a"), tab("b")];
        assert_eq!(remove_at(&mut l, 0), tab("a"));
        insert_at(&mut l, 99, tab("z"));
        insert_at(&mut l, 0, tab("y"));
        assert_eq!(list_tab_ids(&l), ["y", "b", "z"]);
        let a = new_folder("New folder");
        let b = new_folder("Mon dossier");
        assert!(a.open && a.items.is_empty() && a.name == "New folder" && b.name == "Mon dossier" && a.id != b.id);
        assert_eq!(serde_json::to_value(tab_item("q")).unwrap(), json!({"kind":"tab","tabId":"q"}));
    }

    #[test]
    fn group_ranges_and_normalize() {
        let l = vec![tab("a"), gtab("b", "g"), gtab("c", "g"), tab("d")];
        assert_eq!(group_range(&l, "g"), Some((1, 3)));
        assert_eq!(group_range(&l, "nope"), None);
        let mut l = vec![gtab("a", "g"), tab("b"), gtab("c", "g"), gtab("d", "h"), gtab("e", "h")];
        normalize_groups(&mut l);
        assert_eq!(l.iter().map(|i| i.group().map(String::from)).collect::<Vec<_>>(), [None, None, None, Some("h".into()), Some("h".into())]);
        let mut l = vec![gtab("a", "g"), gtab("b", "g"), tab("c"), gtab("d", "g"), gtab("e", "g")];
        normalize_groups(&mut l);
        assert_eq!(l.iter().map(|i| i.group().map(String::from)).collect::<Vec<_>>(), [Some("g".into()), Some("g".into()), None, None, None]);
    }

    #[test]
    fn hue_picking() {
        let g = |hue| TabGroup { name: String::new(), icon: None, hue, open: true };
        assert_eq!(pick_group_hue(&IndexMap::new()), GROUP_HUES[0]);
        let mut m = IndexMap::new();
        m.insert("a".to_string(), g(GROUP_HUES[0]));
        assert_eq!(pick_group_hue(&m), GROUP_HUES[1]);
        let all: IndexMap<String, TabGroup> = GROUP_HUES.iter().enumerate().map(|(i, h)| (i.to_string(), g(*h))).collect();
        assert_eq!(pick_group_hue(&all), GROUP_HUES[0]);
    }

    #[test]
    fn outside_and_insert_today() {
        let l = vec![tab("a"), gtab("b", "g"), gtab("c", "g"), gtab("d", "g"), tab("e")];
        assert_eq!(outside_groups(&l, 2), 4);
        assert_eq!(outside_groups(&l, 1), 1);
        assert_eq!(outside_groups(&l, 4), 4);
        assert_eq!(outside_groups(&l, 99), 5);
        let mut l = vec![gtab("a", "g"), gtab("b", "g"), tab("c")];
        insert_today(&mut l, 1, tab("x"), None);
        assert_eq!(l[1].group(), Some("g"));
        insert_today(&mut l, 4, tab("y"), None);
        assert_eq!(l[4].group(), None);
        insert_today(&mut l, 3, gtab("z", "old"), Some("g"));
        assert_eq!(l[3].group(), Some("g"));
        insert_today(&mut l, 5, gtab("w", "g"), Some("h"));
        assert_eq!(l[5].group(), None);
    }

    #[test]
    fn strip() {
        let mut l = vec![gtab("a", "g"), folder("F", "F", vec![gtab("b", "g")])];
        strip_groups(&mut l);
        assert!(!serde_json::to_string(&l).unwrap().contains("group"));
    }

    fn info(id: &str, url: &str, title: &str, custom: Option<&str>) -> TabInfo {
        TabInfo { id: id.into(), url: url.into(), title: title.into(), custom_title: custom.map(String::from), ..Default::default() }
    }

    #[test]
    fn sync_creates_headers_and_removes_orphans() {
        let mut tabs = IndexMap::new();
        tabs.insert("a".to_string(), info("a", "https://www.example.com/x", "", Some("")));
        tabs.insert("b".to_string(), info("b", "https://b.org", "B", None));
        let mut groups = IndexMap::new();
        groups.insert("old".to_string(), TabGroup { name: "x".into(), icon: None, hue: 1.0, open: true });
        let mut space = Space { id: "s".into(), today: vec![gtab("a", "g"), gtab("b", "g"), gtab("c", "solo")], groups: Some(groups), ..Default::default() };
        sync_groups(&mut space, &tabs);
        let g = space.groups.as_ref().unwrap();
        assert_eq!(g.keys().collect::<Vec<_>>(), ["g"]);
        assert_eq!((g["g"].name.as_str(), g["g"].hue, g["g"].open), ("example.com", GROUP_HUES[0], true));
        assert_eq!(space.today[2].group(), None);
        let mut s2 = Space { today: vec![tab("a")], ..Default::default() };
        sync_groups(&mut s2, &IndexMap::new());
        assert!(s2.groups.is_none());
    }

    fn known(id: &str) -> bool {
        ["a", "b", "c"].contains(&id)
    }

    #[test]
    fn prune_basics() {
        assert!(prune_items(&Value::Null, &known, true, "Folder").is_empty());
        assert!(prune_items(&json!({}), &known, true, "Folder").is_empty());
        let out = prune_items(&json!([{"kind":"tab","tabId":"a"},{"kind":"tab","tabId":"zzz"},null,3,{"kind":"bidule"}]), &known, true, "Folder");
        assert_eq!(out, vec![tab("a")]);
    }

    #[test]
    fn prune_splits() {
        let p = |v: Value| prune_items(&v, &known, true, "Folder");
        assert!(p(json!([{"kind":"split","id":"S","tabIds":["zzz"]}])).is_empty());
        assert_eq!(p(json!([{"kind":"split","id":"S","tabIds":["a","zzz"]}])), vec![tab("a")]);
        assert_eq!(p(json!([{"kind":"split","id":"S","tabIds":["a","zzz"],"group":"g"}])), vec![gtab("a", "g")]);
        assert!(p(json!([{"kind":"split","id":"S"}])).is_empty());
        let all = |_: &str| true;
        let out = prune_items(&json!([{"kind":"split","id":"S","tabIds":["1","2","3","4","5"],"layout":"columns","sizes":[1,1]}]), &all, true, "Folder");
        let s = out[0].as_split().unwrap();
        assert_eq!(s.tab_ids, ["1", "2", "3", "4"]);
        assert!(s.layout.is_none() && s.sizes.is_none());
    }

    #[test]
    fn prune_folders() {
        let p = |v: Value, folders| prune_items(&v, &known, folders, "Folder");
        let out = p(json!([{"kind":"folder","items":[{"kind":"tab","tabId":"a"},{"kind":"tab","tabId":"zzz"}],"icon":"123456789012345678","hue":400.4}]), true);
        let f = out[0].as_folder().unwrap();
        assert_eq!(f.name, "Folder");
        assert_eq!(f.id.len(), 36);
        assert!(f.open);
        assert_eq!(f.items, vec![tab("a")]);
        assert!(f.icon.as_ref().unwrap().chars().count() <= 16);
        assert_eq!(f.hue, Some(40.0));
        let out = p(json!([{"kind":"folder","id":"X","name":"N","open":false,"items":[]}]), true);
        let f = out[0].as_folder().unwrap();
        assert_eq!((f.id.as_str(), f.name.as_str(), f.open), ("X", "N", false));
        let live = json!({"source":{"kind":"feed","url":"https://e.com/rss"},"refreshing":true,"refreshedAt":5});
        let out = p(json!([{"kind":"folder","id":"X","name":"N","items":[],"live":live}]), true);
        let l = out[0].as_folder().unwrap().live.clone().unwrap();
        assert_eq!(l, LiveFolder { source: LiveSource::Feed { url: "https://e.com/rss".into() }, refreshed_at: Some(5), error: None, refreshing: None, seen: None });
        let bad = p(json!([{"kind":"folder","id":"X","name":"N","items":[],"live":{"source":{"kind":"feed","url":"javascript:1"}}}]), true);
        assert!(bad[0].as_folder().unwrap().live.is_none());
        let flat = p(json!([{"kind":"tab","tabId":"a"},{"kind":"folder","id":"F","name":"F","items":[{"kind":"tab","tabId":"b"},{"kind":"folder","id":"G","name":"G","items":[{"kind":"tab","tabId":"c"}]}]}]), false);
        assert_eq!(flat, vec![tab("a"), tab("b"), tab("c")]);
    }
}

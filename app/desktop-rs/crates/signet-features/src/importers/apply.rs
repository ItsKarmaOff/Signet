//! Turning imported items into things the app can store (port of `importers/apply.ts`). The state mutation
//! itself (spaces, tabs) is done by the app with signet-core types from the `SpacePlan` built here.
use std::collections::HashSet;


use super::types::*;

pub const MAX_IMPORTED_TABS: usize = 2000;
pub const MAX_IMPORTED_BOOKMARKS: usize = 20000;
pub const HUES: [u32; 10] = [250, 330, 150, 25, 190, 285, 45, 215, 90, 0];

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ImportCounts {
    pub spaces: usize,
    pub pinned: usize,
    pub favorites: usize,
    pub today: usize,
    pub bookmarks: usize,
    pub truncated: bool,
}

/// Provisional favicon until the page loads.
pub fn provisional_favicon(url: &str) -> Option<String> {
    let u = url::Url::parse(url).ok()?;
    Some(format!("https://icons.duckduckgo.com/ip3/{}.ico", u.host_str()?))
}

pub use crate::bookmarks::Bookmark;

/// Browser favorites as bookmarks: directly in the bar if it is empty, otherwise in a folder at the end.
/// Returns the counts (with `inFolder`) and appends to `bar`.
pub fn apply_bookmarks(bar: &mut Vec<Bookmark>, folder: &ImportedNode) -> (ImportCounts, bool) {
    let mut count = 0usize;
    let mut truncated = false;
    fn convert(nodes: &[ImportedNode], count: &mut usize, truncated: &mut bool) -> Vec<Bookmark> {
        let mut out = Vec::new();
        for n in nodes {
            match n {
                ImportedNode::Folder { name, items } => out.push(crate::bookmarks::folder(name, convert(items, count, truncated))),
                ImportedNode::Tab(_) | ImportedNode::Split(_) => {
                    let tabs: Vec<&ImportedTab> = match n { ImportedNode::Tab(t) => vec![t], ImportedNode::Split(t) => t.iter().collect(), _ => vec![] };
                    for t in tabs {
                        if *count >= MAX_IMPORTED_BOOKMARKS {
                            *truncated = true;
                            continue;
                        }
                        *count += 1;
                        out.push(crate::bookmarks::link(&t.url, &t.title, provisional_favicon(&t.url).as_deref()));
                    }
                }
            }
        }
        out
    }
    let in_folder = !bar.is_empty() || !matches!(folder, ImportedNode::Folder { .. });
    match (in_folder, folder) {
        (false, ImportedNode::Folder { items, .. }) => bar.extend(convert(items, &mut count, &mut truncated)),
        _ => bar.extend(convert(std::slice::from_ref(folder), &mut count, &mut truncated)),
    }
    (ImportCounts { bookmarks: count, truncated, ..Default::default() }, in_folder)
}

/// A tab to create (unloaded), with its provisional favicon.
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedTab {
    pub url: String,
    pub title: String,
    pub favicon: Option<String>,
    /// Set for pinned and favorite tabs.
    pub pinned_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlannedItem {
    Tab(PlannedTab),
    Folder { name: String, items: Vec<PlannedItem> },
    Split(Vec<PlannedTab>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlannedSpace {
    pub name: String,
    pub icon: String,
    pub hue: u32,
    pub pinned: Vec<PlannedItem>,
    pub today: Vec<PlannedItem>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SpacesPlan {
    pub spaces: Vec<PlannedSpace>,
    pub favorites: Vec<PlannedTab>,
    /// True when the existing state was a single blank space: the app replaces its spaces by `spaces`.
    pub replace_existing: bool,
    pub counts: ImportCounts,
}

/// What the app tells about its current state.
pub struct ExistingState {
    pub space_count: usize,
    /// The only space is empty (no pinned tab, today tab, bookmark or group).
    pub single_blank_space: bool,
    /// Pinned URLs of tabs already in the favorites.
    pub favorite_urls: Vec<String>,
}

struct Builder {
    count: usize,
    truncated: bool,
}

impl Builder {
    fn tab(&mut self, t: &ImportedTab, pinned: bool) -> Option<PlannedTab> {
        if self.count >= MAX_IMPORTED_TABS {
            self.truncated = true;
            return None;
        }
        self.count += 1;
        Some(PlannedTab { url: t.url.clone(), title: t.title.clone(), favicon: provisional_favicon(&t.url), pinned_url: pinned.then(|| t.url.clone()) })
    }

    fn items(&mut self, nodes: &[ImportedNode], pinned: bool) -> Vec<PlannedItem> {
        let mut out = Vec::new();
        for n in nodes {
            match n {
                ImportedNode::Tab(t) => out.extend(self.tab(t, pinned).map(PlannedItem::Tab)),
                ImportedNode::Split(tabs) => {
                    let ts: Vec<PlannedTab> = tabs.iter().filter_map(|t| self.tab(t, pinned)).collect();
                    if ts.len() < 2 { out.extend(ts.into_iter().map(PlannedItem::Tab)) } else { out.push(PlannedItem::Split(ts)) }
                }
                ImportedNode::Folder { name, items } => out.push(PlannedItem::Folder { name: name.clone(), items: self.items(items, pinned) }),
            }
        }
        out
    }
}

/// Imported spaces and favorites, within the tab limit. A blank state is replaced, otherwise spaces are added.
pub fn plan_spaces(existing: &ExistingState, data: &SpacesImport) -> SpacesPlan {
    let mut b = Builder { count: 0, truncated: false };
    let mut counts = ImportCounts::default();
    let blank = existing.single_blank_space;
    let base = if blank { 0 } else { existing.space_count };
    let mut spaces = Vec::new();
    for (i, sp) in data.spaces.iter().enumerate() {
        let before = b.count;
        let pinned = b.items(&sp.pinned, true);
        counts.pinned += b.count - before;
        let mid = b.count;
        let today = b.items(&sp.today, false);
        counts.today += b.count - mid;
        spaces.push(PlannedSpace { name: sp.name.clone(), icon: sp.icon.clone(), hue: HUES[(base + i) % HUES.len()], pinned, today });
    }
    let mut known: HashSet<String> = existing.favorite_urls.iter().cloned().collect();
    let mut favorites = Vec::new();
    for f in &data.favorites {
        if known.contains(&f.url) {
            continue;
        }
        let Some(t) = b.tab(f, true) else { break };
        known.insert(f.url.clone());
        favorites.push(t);
        counts.favorites += 1;
    }
    counts.spaces = spaces.len();
    counts.truncated = b.truncated;
    SpacesPlan { replace_existing: blank && !spaces.is_empty(), spaces, favorites, counts }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tab(u: &str, t: &str) -> ImportedNode { ImportedNode::tab(u, t) }
    fn it(u: &str, t: &str) -> ImportedTab { ImportedTab { url: u.into(), title: t.into() } }

    fn folder() -> ImportedNode {
        ImportedNode::Folder { name: "Chrome".into(), items: vec![tab("https://a.com", "A"), ImportedNode::Folder { name: "Sous".into(), items: vec![tab("https://b.com", "B")] }] }
    }

    #[test]
    fn empty_bar_content_goes_directly() {
        let mut bar = vec![];
        let (c, in_folder) = apply_bookmarks(&mut bar, &folder());
        assert_eq!((c.bookmarks, in_folder, c.truncated), (2, false, false));
        assert!(matches!(&bar[0], Bookmark::Link { title, url, favicon, .. } if title == "A" && url == "https://a.com" && favicon.as_deref() == Some("https://icons.duckduckgo.com/ip3/a.com.ico")));
        assert!(matches!(&bar[1], Bookmark::Folder { .. }));
    }

    #[test]
    fn non_empty_bar_gets_a_folder() {
        let mut bar = vec![Bookmark::Link { id: "x".into(), url: "https://x.com".into(), title: "x".into(), favicon: None }];
        let (_, in_folder) = apply_bookmarks(&mut bar, &folder());
        assert!(in_folder);
        assert_eq!(bar.len(), 2);
        assert!(matches!(&bar[1], Bookmark::Folder { name, .. } if name == "Chrome"));
    }

    #[test]
    fn split_gives_one_bookmark_per_tab_and_bad_url_no_favicon() {
        let mut bar = vec![];
        let node = ImportedNode::Folder { name: "F".into(), items: vec![ImportedNode::Split(vec![it("https://a.com", "a"), it("pas une url", "b")])] };
        let (c, _) = apply_bookmarks(&mut bar, &node);
        assert_eq!(c.bookmarks, 2);
        assert!(matches!(&bar[1], Bookmark::Link { favicon: None, .. }));
        let json = serde_json::to_value(&bar[1]).unwrap();
        assert!(json.get("favicon").is_none());
        assert_eq!(json["kind"], "link");
    }

    fn data() -> SpacesImport {
        SpacesImport {
            spaces: vec![ImportedSpace {
                name: "Arc".into(),
                icon: "\u{1F308}".into(),
                pinned: vec![tab("https://p.com", "https://p.com"), ImportedNode::Folder { name: "F".into(), items: vec![tab("https://q.com", "q")] }, ImportedNode::Split(vec![it("https://s1.com", "s1"), it("https://s2.com", "s2")])],
                today: vec![tab("https://t.com", "t")],
            }],
            favorites: vec![it("https://fav.com", "Fav")],
        }
    }

    #[test]
    fn blank_state_is_replaced() {
        let p = plan_spaces(&ExistingState { space_count: 1, single_blank_space: true, favorite_urls: vec![] }, &data());
        assert!(p.replace_existing);
        assert_eq!(p.counts, ImportCounts { spaces: 1, pinned: 4, favorites: 1, today: 1, bookmarks: 0, truncated: false });
        assert_eq!(p.spaces[0].hue, 250);
        assert!(matches!(&p.spaces[0].pinned[2], PlannedItem::Split(t) if t.len() == 2));
        match &p.spaces[0].pinned[0] { PlannedItem::Tab(t) => assert_eq!(t.pinned_url.as_deref(), Some("https://p.com")), _ => panic!() }
        match &p.spaces[0].today[0] { PlannedItem::Tab(t) => assert_eq!(t.pinned_url, None), _ => panic!() }
    }

    #[test]
    fn non_blank_state_adds() {
        let p = plan_spaces(&ExistingState { space_count: 1, single_blank_space: false, favorite_urls: vec![] }, &data());
        assert!(!p.replace_existing);
        assert_eq!(p.spaces[0].hue, 330);
    }

    #[test]
    fn known_favorites_ignored() {
        let p = plan_spaces(&ExistingState { space_count: 1, single_blank_space: false, favorite_urls: vec!["https://fav.com".into()] }, &SpacesImport { spaces: vec![], favorites: data().favorites });
        assert_eq!(p.counts.favorites, 0);
    }

    #[test]
    fn tab_cap() {
        let many = SpacesImport {
            spaces: vec![ImportedSpace { name: "Big".into(), icon: "i".into(), pinned: vec![], today: (0..MAX_IMPORTED_TABS + 50).map(|i| tab(&format!("https://x.com/{i}"), "t")).collect() }],
            favorites: vec![it("https://f.com", "f")],
        };
        let p = plan_spaces(&ExistingState { space_count: 1, single_blank_space: true, favorite_urls: vec![] }, &many);
        assert!(p.counts.truncated);
        assert_eq!(p.counts.today, MAX_IMPORTED_TABS);
        assert_eq!(p.counts.favorites, 0);
    }
}

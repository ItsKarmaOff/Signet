//! Ports every `*.test.ts` of `app/desktop/src/main/model`.

use super::testkit::*;
use super::*;
use crate::core::store::ArchivedTab;
use crate::split::DropEdge;
use crate::types::{DragSource, DropTarget, Item, SplitItem, SplitLayout, SpaceDraft, SpaceProfile, SpaceRoute, TabGroup, Zone};
use std::collections::HashSet;

fn set(ids: &[&str]) -> HashSet<String> {
    ids.iter().map(|s| s.to_string()).collect()
}
fn v(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|s| s.to_string()).collect()
}
fn dt(zone: Zone, space: &str, folder: Option<&str>, group: Option<&str>, index: usize) -> DropTarget {
    DropTarget { zone, space_id: space.into(), folder_id: folder.map(String::from), group_id: group.map(String::from), index }
}
fn today(index: usize) -> DropTarget {
    dt(Zone::Today, "s1", None, None, index)
}
fn dtab(id: &str) -> DragSource {
    DragSource::Tab { tab_id: id.into() }
}
fn loc(zone: Zone, space: usize, index: usize, item: Item) -> Location {
    Location {
        zone,
        space: Some(space),
        list: Some(ListRef::root(space, zone)),
        index,
        item,
        shrunk: false,
    }
}

// ----------------------------------------------------------------- activation

#[test]
fn set_active_dates_and_notifies() {
    let mut w = world(spec().tabs(&["a"]).spaces(vec![sp(vec![], vec![tab("a")])]));
    w.env.now = 5000;
    w.m().set_active(0, Some("a"));
    assert_eq!(w.s1().active_tab_id.as_deref(), Some("a"));
    assert_eq!(w.st().tabs["a"].last_active, 5000);
    assert_eq!(w.env.activated, [("s1".to_string(), "a".to_string())]);
}

#[test]
fn set_active_none_notifies_nobody() {
    let mut w = world(spec().tabs(&["a"]).spaces(vec![sp(vec![], vec![tab("a")]).active("a")]));
    w.m().set_active(0, None);
    assert_eq!(w.s1().active_tab_id, None);
    assert!(w.env.activated.is_empty());
}

#[test]
fn activate_switches_space_and_returns_before() {
    let mut w = world(spec().tabs(&["a", "b"]).spaces(vec![sp(vec![], vec![tab("a")]).active("a"), sp(vec![], vec![tab("b")])]));
    assert_eq!(w.m().activate("b"), Some(Some("a".to_string())));
    assert_eq!(w.st().active_space_id, "s2");
    assert_eq!(w.space("s2").active_tab_id.as_deref(), Some("b"));
}

#[test]
fn activate_favorite_keeps_space() {
    let mut w = world(spec().tabs(&["fav"]).favorites(&["fav"]).spaces(vec![sp(vec![], vec![]), sp(vec![], vec![])]).active("s2"));
    w.m().activate("fav");
    assert_eq!(w.st().active_space_id, "s2");
    assert_eq!(w.space("s2").active_tab_id.as_deref(), Some("fav"));
}

#[test]
fn activate_unknown() {
    let mut w = world(spec().tabs(&["a"]).spaces(vec![sp(vec![], vec![tab("a")]).active("a")]));
    assert_eq!(w.m().activate("x"), None);
    assert_eq!(w.s1().active_tab_id.as_deref(), Some("a"));
}

#[test]
fn pick_next_rules() {
    let mut w = world(spec().tabs(&["a", "b", "c", "d", "p"]).spaces(vec![sp(vec![tab("p")], vec![tab("a"), tab("b"), tab("c"), tab("d")])]));
    let list = w.s1().today.clone();
    let m = w.m();
    assert_eq!(m.pick_next(0, &list, 1, true, &set(&["b"])).as_deref(), Some("c"));
    assert_eq!(m.pick_next(0, &list, 4, true, &set(&["d"])).as_deref(), Some("c"));
    assert_eq!(m.pick_next(0, &list, 1, false, &set(&[])).as_deref(), Some("b"));
    assert_eq!(m.pick_next(0, &list, 1, false, &set(&["b"])).as_deref(), Some("c"));
    assert_eq!(m.pick_next(0, &list, 0, true, &set(&["a", "b", "c", "d"])).as_deref(), Some("p"));
    assert_eq!(m.pick_next(0, &list, 0, true, &set(&["a", "b", "c", "d", "p"])), None);
}

#[test]
fn pick_next_split_and_folder() {
    let mut s = world(spec().tabs(&["x", "y"]).spaces(vec![sp(vec![], vec![split("sp", &["x", "y"])])]));
    let list = s.s1().today.clone();
    assert_eq!(s.m().pick_next(0, &list, 0, true, &set(&["x"])).as_deref(), Some("y"));
    let mut w = world(spec().tabs(&["a", "in"]).spaces(vec![sp(vec![folder("f", vec![tab("in")])], vec![tab("a")])]));
    let list = w.s1().today.clone();
    assert_eq!(w.m().pick_next(0, &list, 0, true, &set(&["a"])).as_deref(), Some("in"));
}

#[test]
fn after_favorite_rules() {
    let mut w = world(spec().tabs(&["fav", "a", "b"]).favorites(&["fav"]).spaces(vec![sp(vec![], vec![tab("a"), tab("b")])]));
    for (id, loaded, last) in [("fav", true, 50), ("a", true, 10), ("b", false, 90)] {
        w.tab(id).loaded = loaded;
        w.tab(id).last_active = last;
    }
    assert_eq!(w.m().after_favorite(0, &set(&[])).as_deref(), Some("fav"));
    assert_eq!(w.m().after_favorite(0, &set(&["fav"])).as_deref(), Some("a"));

    let mut w = world(spec().tabs(&["a", "b", "p"]).spaces(vec![sp(vec![tab("p")], vec![tab("a"), tab("b")])]));
    assert_eq!(w.m().after_favorite(0, &set(&[])).as_deref(), Some("a"));

    let mut w = world(spec().tabs(&["fav"]).favorites(&["fav"]));
    assert_eq!(w.m().after_favorite(0, &set(&[])), None);
}

#[test]
fn repick_neighbor_takes_over() {
    let mut w = world(spec().tabs(&["a", "b", "c"]).spaces(vec![sp(vec![], vec![tab("a"), tab("b"), tab("c")]).active("b")]));
    let l = w.m().extract("b", false).unwrap();
    w.m().repick(&l, &v(&["b"]), true);
    assert_eq!(w.s1().active_tab_id.as_deref(), Some("c"));
}

#[test]
fn repick_other_active_untouched() {
    let mut w = world(spec().tabs(&["a", "b"]).spaces(vec![sp(vec![], vec![tab("a"), tab("b")]).active("a")]));
    let l = w.m().extract("b", false).unwrap();
    w.m().repick(&l, &v(&["b"]), true);
    assert_eq!(w.s1().active_tab_id.as_deref(), Some("a"));
}

#[test]
fn repick_still_reachable() {
    let mut w = world(spec().tabs(&["a"]).favorites(&["a"]).spaces(vec![sp(vec![], vec![]).active("a")]));
    w.m().repick(&loc(Zone::Today, 0, 0, tab("a")), &v(&["a"]), true);
    assert_eq!(w.s1().active_tab_id.as_deref(), Some("a"));
}

#[test]
fn repick_other_space_falls_back_to_pinned() {
    let mut w = world(spec().tabs(&["m", "p2"]).spaces(vec![sp(vec![], vec![tab("m")]), sp(vec![tab("p2")], vec![]).active("m")]));
    let l = w.m().extract("m", false).unwrap();
    w.m().repick(&l, &v(&["m"]), true);
    assert_eq!(w.space("s2").active_tab_id.as_deref(), Some("p2"));
}

#[test]
fn repick_removed_favorite() {
    let mut w = world(spec().tabs(&["fav", "a", "b"]).favorites(&["fav"]).spaces(vec![sp(vec![], vec![tab("a"), tab("b")]).active("fav")]));
    w.tab("fav").loaded = true;
    w.tab("a").loaded = true;
    w.tab("a").last_active = 5;
    w.tab("b").loaded = true;
    w.tab("b").last_active = 9;
    let l = w.m().extract("fav", false).unwrap();
    w.m().repick(&l, &v(&["fav"]), true);
    assert_eq!(w.s1().active_tab_id.as_deref(), Some("b"));
}

#[test]
fn repick_unload() {
    let mut w = world(spec().tabs(&["a", "b"]).spaces(vec![sp(vec![], vec![tab("a"), tab("b")]).active("a")]));
    let l = w.m().locate("a").unwrap();
    w.m().repick(&l, &v(&["a"]), false);
    assert_eq!(w.s1().active_tab_id.as_deref(), Some("b"));
}

// -------------------------------------------------------------------- opening

#[test]
fn open_at_top() {
    let mut w = world(spec().tabs(&["a", "b"]).spaces(vec![sp(vec![], vec![tab("a"), tab("b")])]));
    let id = w.m().open("https://x.test/", &OpenOptions::default());
    assert_eq!(shape(&w.s1().today), [id.clone(), "a".into(), "b".into()]);
    assert_eq!(w.st().tabs[&id].url, "https://x.test/");
    assert_eq!(w.s1().active_tab_id, None);
}

#[test]
fn open_at_bottom() {
    let mut w = world(spec().tabs(&["a"]).spaces(vec![sp(vec![], vec![tab("a")])]));
    w.data.state.settings.new_tab_position = Some(crate::types::NewTabPosition::Bottom);
    let id = w.m().open("https://x.test/", &OpenOptions::default());
    assert_eq!(shape(&w.s1().today), ["a".to_string(), id]);
}

#[test]
fn open_activate() {
    let mut w = world(spec());
    let id = w.m().open("https://x.test/", &OpenOptions { activate: true, ..Default::default() });
    assert_eq!(w.s1().active_tab_id.as_deref(), Some(id.as_str()));
    assert_eq!(w.env.activated, [("s1".to_string(), id)]);
}

#[test]
fn open_after_tab() {
    let mut w = world(spec().tabs(&["a", "b", "c"]).spaces(vec![sp(vec![], vec![tab("a"), tab("b"), tab("c")])]));
    let id = w.m().open("https://x.test/", &OpenOptions { after_tab_id: Some("b".into()), ..Default::default() });
    assert_eq!(shape(&w.s1().today), ["a".to_string(), "b".into(), id, "c".into()]);
    let mut w = world(spec().tabs(&["p", "a"]).spaces(vec![sp(vec![tab("p")], vec![tab("a")])]));
    let id = w.m().open("https://x.test/", &OpenOptions { after_tab_id: Some("p".into()), ..Default::default() });
    assert_eq!(shape(&w.s1().today), [id, "a".to_string()]);
}

#[test]
fn open_from_opener() {
    let mut w = world(spec().tabs(&["a", "b", "c"]).spaces(vec![sp(vec![], vec![tab("a"), tab("b"), tab("c")])]));
    let id = w.m().open("https://x.test/", &OpenOptions { opener_id: Some("b".into()), ..Default::default() });
    let group = w.s1().today[1].group().unwrap().to_string();
    assert_eq!(shape(&w.s1().today), ["a".to_string(), format!("b^{group}"), format!("{id}^{group}"), "c".into()]);

    let mut w = world(spec().tabs(&["a", "b", "c"]).spaces(vec![sp(vec![], vec![gtab("a", "g"), gtab("b", "g"), tab("c")])]));
    let id = w.m().open("https://x.test/", &OpenOptions { opener_id: Some("a".into()), ..Default::default() });
    assert_eq!(shape(&w.s1().today), ["a^g".to_string(), "b^g".into(), format!("{id}^g"), "c".into()]);

    let mut w = world(spec().tabs(&["a", "b"]).spaces(vec![sp(vec![], vec![split("sp", &["a", "b"])])]));
    let id = w.m().open("https://x.test/", &OpenOptions { opener_id: Some("a".into()), ..Default::default() });
    let g = w.s1().today[0].group().unwrap().to_string();
    assert_eq!(shape(&w.s1().today), [format!("{{sp: a b}}^{g}"), format!("{id}^{g}")]);
}

#[test]
fn open_with_pinned_or_foreign_opener() {
    let mut w = world(spec().tabs(&["p", "o", "a"]).spaces(vec![sp(vec![tab("p")], vec![tab("a")]), sp(vec![], vec![tab("o")])]));
    let id1 = w.m().open("https://x.test/1", &OpenOptions { opener_id: Some("p".into()), ..Default::default() });
    let id2 = w.m().open("https://x.test/2", &OpenOptions { opener_id: Some("o".into()), ..Default::default() });
    assert_eq!(shape(&w.s1().today), [id2, id1, "a".to_string()]);
}

#[test]
fn open_in_other_space() {
    let mut w = world(spec().tabs(&["a", "b"]).spaces(vec![sp(vec![], vec![tab("a")]), sp(vec![], vec![tab("b")])]));
    let id = w.m().open_in(1, "https://x.test/");
    assert_eq!(shape(&w.space("s2").today), [id.clone(), "b".to_string()]);
    assert_eq!(w.st().active_space_id, "s2");
    assert_eq!(w.space("s2").active_tab_id.as_deref(), Some(id.as_str()));
    assert_eq!(shape(&w.s1().today), ["a"]);
}

#[test]
fn adopt_detached() {
    let mut w = world(spec().tabs(&["a", "peek"]).spaces(vec![sp(vec![], vec![tab("a")])]));
    w.data.detached.insert("peek".into());
    w.m().adopt("peek");
    assert!(!w.data.detached.contains("peek"));
    assert_eq!(shape(&w.s1().today), ["peek", "a"]);
    assert_eq!(w.s1().active_tab_id.as_deref(), Some("peek"));
    assert_eq!(w.env.reconciled, [v(&["peek"])]);
}

// -------------------------------------------------------------------- removal

#[test]
fn close_day_tab() {
    let mut w = world(spec().tabs(&["a", "b", "c"]).spaces(vec![sp(vec![], vec![tab("a"), tab("b"), tab("c")]).active("b")]));
    assert!(w.m().close("b"));
    assert_eq!(shape(&w.s1().today), ["a", "c"]);
    assert!(!w.st().tabs.contains_key("b"));
    assert_eq!(w.s1().active_tab_id.as_deref(), Some("c"));
    assert_eq!(w.data.closed.len(), 1);
    assert_eq!(w.env.destroyed, ["b"]);
}

#[test]
fn view_destroyed_before_tab_leaves_state() {
    let mut w = world(spec().tabs(&["a"]).spaces(vec![sp(vec![], vec![tab("a")])]));
    w.m().close("a");
    assert_eq!(w.env.destroyed_existed, [true]);
}

#[test]
fn close_last_active() {
    let mut w = world(spec().tabs(&["a"]).spaces(vec![sp(vec![], vec![tab("a")]).active("a")]));
    w.m().close("a");
    assert_eq!(w.s1().active_tab_id, None);
    assert!(!world(spec()).m().close("x"));
}

#[test]
fn close_pinned_unloads() {
    let mut w = world(spec().tabs(&["p", "q", "a"]).spaces(vec![sp(vec![tab("p"), tab("q")], vec![tab("a")]).active("p")]));
    w.tab("p").pinned_url = Some("https://home.test/".into());
    w.tab("p").url = "https://ailleurs.test/".into();
    w.tab("q").pinned_url = Some("https://q.test/".into());
    assert!(w.m().close("p"));
    assert_eq!(shape(&w.s1().pinned), ["p", "q"]);
    assert_eq!(w.st().tabs["p"].url, "https://home.test/");
    assert_eq!(w.env.destroyed, ["p"]);
    assert!(w.data.closed.is_empty());
    assert_eq!(w.s1().active_tab_id.as_deref(), Some("q"));
}

#[test]
fn close_only_pinned_active_picks_nothing() {
    let mut w = world(spec().tabs(&["p", "a"]).spaces(vec![sp(vec![tab("p")], vec![tab("a")]).active("p")]));
    w.tab("p").pinned_url = Some("https://home.test/".into());
    w.m().close("p");
    assert_eq!(w.s1().active_tab_id, None);
}

#[test]
fn close_pinned_in_split_leaves_it_first() {
    let mut w = world(spec().tabs(&["p", "q"]).spaces(vec![sp(vec![split("sp", &["p", "q"])], vec![]).active("p")]));
    w.tab("p").pinned_url = Some("https://home.test/".into());
    w.tab("q").pinned_url = Some("https://q.test/".into());
    w.m().close("p");
    assert_eq!(shape(&w.s1().pinned), ["q", "p"]);
}

#[test]
fn remove_and_drop() {
    let mut w = world(spec().tabs(&["a", "b"]).spaces(vec![sp(vec![], vec![tab("a"), tab("b")]).active("a")]));
    w.data.histories.insert("a".into(), Default::default());
    w.m().remove("a");
    assert!(!w.st().tabs.contains_key("a"));
    assert!(!w.data.histories.contains_key("a"));
    assert!(w.data.closed.is_empty());
    assert_eq!(w.s1().active_tab_id.as_deref(), Some("b"));

    let mut w = world(spec().tabs(&["a", "b", "c"]).spaces(vec![sp(vec![], vec![split("sp", &["a", "b"]), tab("c")]).active("a")]));
    w.m().remove("a");
    assert_eq!(shape(&w.s1().today), ["b", "c"]);
    assert_eq!(w.s1().active_tab_id.as_deref(), Some("b"));

    let mut w = world(spec().tabs(&["a", "b"]));
    w.data.histories.insert("b".into(), Default::default());
    w.m().drop_tabs(&v(&["a", "b"]));
    assert_eq!(w.env.destroyed, ["a", "b"]);
    assert!(w.st().tabs.is_empty());
    assert!(w.data.histories.is_empty());
}

#[test]
fn close_in_pinned_folder_remembers_folder() {
    let mut w = world(spec().tabs(&["a"]).spaces(vec![sp(vec![folder("f", vec![tab("a")])], vec![])]));
    w.tab("a").pinned_url = None;
    w.m().close("a");
    assert_eq!(w.data.closed[0].folder_id.as_deref(), Some("f"));
}

// ----------------------------------------------------------------- navigation

fn nav_world() -> World {
    world(
        spec().tabs(&["f1", "p1", "in1", "in2", "t1", "t2"]).favorites(&["f1"]).spaces(vec![sp(vec![tab("p1"), folder("f", vec![tab("in1"), tab("in2")])], vec![tab("t1"), tab("t2")]).active("t1")]),
    )
}

#[test]
fn cycle_targets() {
    let mut w = nav_world();
    assert_eq!(cycle_target(w.st(), 1).as_deref(), Some("t2"));
    assert_eq!(cycle_target(w.st(), -1).as_deref(), Some("in2"));
    w.space_mut("s1").active_tab_id = Some("t2".into());
    assert_eq!(cycle_target(w.st(), 1).as_deref(), Some("f1"));
    w.space_mut("s1").active_tab_id = Some("f1".into());
    assert_eq!(cycle_target(w.st(), -1).as_deref(), Some("t2"));
    w.space_mut("s1").active_tab_id = None;
    assert_eq!(cycle_target(w.st(), 1).as_deref(), Some("f1"));
    assert_eq!(cycle_target(w.st(), -1).as_deref(), Some("t2"));
    assert_eq!(cycle_target(world(spec()).st(), 1), None);
}

#[test]
fn number_targets() {
    let mut w = nav_world();
    assert_eq!(number_target(w.st(), 1, false).as_deref(), Some("f1"));
    assert_eq!(number_target(w.st(), 3, false).as_deref(), Some("in1"));
    assert_eq!(number_target(w.st(), 6, false).as_deref(), Some("t2"));
    assert_eq!(number_target(w.st(), 9, false).as_deref(), Some("t2"));
    assert_eq!(number_target(w.st(), 8, false), None);
    assert_eq!(number_target(w.st(), 3, true).as_deref(), Some("t1"));
    w.space_mut("s1").pinned[1].as_folder_mut().unwrap().open = false;
    assert_eq!(number_target(w.st(), 3, false).as_deref(), Some("t1"));
}

// --------------------------------------------------------------------- locate

fn locate_world() -> World {
    world(
        spec()
            .tabs(&["fav", "p1", "p2", "t1", "t2", "in"])
            .favorites(&["fav"])
            .spaces(vec![sp(vec![tab("p1"), folder("f", vec![tab("in")])], vec![tab("t1"), split("sp", &["p2", "t2"])]), sp(vec![], vec![])]),
    )
}

#[test]
fn locate_cases() {
    let w = locate_world();
    let l = locate(w.st(), "fav").unwrap();
    assert_eq!((l.zone, l.space, l.index), (Zone::Favorites, None, 0));
    assert_eq!((locate(w.st(), "p1").unwrap().zone, locate(w.st(), "p1").unwrap().index), (Zone::Pinned, 0));
    let t1 = locate(w.st(), "t1").unwrap();
    assert_eq!((t1.zone, t1.index, t1.space), (Zone::Today, 0, Some(0)));
    let inl = locate(w.st(), "in").unwrap();
    assert_eq!(inl.zone, Zone::Pinned);
    assert_eq!(inl.list.unwrap().folder.as_deref(), Some("f"));
    let t2 = locate(w.st(), "t2").unwrap();
    assert!(matches!(t2.item, Item::Split(_)));
    assert_eq!(t2.index, 1);
    assert!(locate(w.st(), "nope").is_none());
    assert!(matches!(find_split(w.st(), "sp").unwrap().item, Item::Split(_)));
    assert!(find_split(w.st(), "x").is_none());
    assert_eq!(find_folder(w.st(), "f").unwrap().zone, Zone::Pinned);
    assert!(find_folder(w.st(), "x").is_none());
    assert!(reachable(w.st(), w.space("s2"), "fav"));
    assert!(reachable(w.st(), w.space("s1"), "t1"));
    assert!(!reachable(w.st(), w.space("s2"), "t1"));
}

#[test]
fn space_of_group_cases() {
    let g = world(spec().tabs(&["a", "b"]).spaces(vec![sp(vec![], vec![]), sp(vec![], vec![gtab("a", "g"), gtab("b", "g")])]));
    assert_eq!(space_of_group(g.st(), "g"), Some(1));
    assert_eq!(space_of_group(g.st(), "zz"), None);
}

#[test]
fn active_space_and_displayed() {
    let mut w = world(spec().spaces(vec![sp(vec![], vec![]), sp(vec![], vec![])]).active("s2"));
    assert_eq!(active_space(w.st()).id, "s2");
    w.data.state.active_space_id = "disparu".into();
    assert_eq!(active_space(w.st()).id, "s1");

    let mut w = world(spec().tabs(&["a", "b", "c"]).spaces(vec![sp(vec![], vec![tab("a"), split("sp", &["b", "c"])]).active("a")]));
    assert_eq!(displayed_tab_ids(w.st(), w.s1()), ["a"]);
    assert!(displayed_split(w.st(), w.s1()).is_none());
    w.space_mut("s1").active_tab_id = Some("c".into());
    assert_eq!(displayed_tab_ids(w.st(), w.s1()), ["b", "c"]);
    assert_eq!(displayed_split(w.st(), w.s1()).unwrap().id, "sp");

    let w = world(spec().tabs(&["fav"]).favorites(&["fav"]).spaces(vec![sp(vec![], vec![]).active("fav")]));
    assert_eq!(displayed_tab_ids(w.st(), w.s1()), ["fav"]);

    let mut w = world(spec().tabs(&["a"]).spaces(vec![sp(vec![], vec![tab("a")])]));
    assert!(displayed_tab_ids(w.st(), w.s1()).is_empty());
    w.space_mut("s1").active_tab_id = Some("disparu".into());
    assert!(displayed_tab_ids(w.st(), w.s1()).is_empty());
}

// ---------------------------------------------------------------------- lists

#[test]
fn extract_cases() {
    let mut w = world(spec().tabs(&["a", "b"]).spaces(vec![sp(vec![], vec![tab("a"), tab("b")])]));
    assert_eq!(w.m().extract("a", false).unwrap().index, 0);
    assert_eq!(shape(&w.s1().today), ["b"]);

    let mut w = world(spec().tabs(&["f"]).favorites(&["f"]));
    assert_eq!(w.m().extract("f", false).unwrap().zone, Zone::Favorites);
    assert!(w.st().favorites.is_empty());

    let mut w = world(spec().tabs(&["a", "b", "c"]).spaces(vec![sp(
        vec![],
        vec![Item::Split(SplitItem { id: "sp".into(), tab_ids: v(&["a", "b", "c"]), sizes: Some(vec![0.5, 0.25, 0.25]), ..Default::default() })],
    )]));
    let l = w.m().extract("b", false).unwrap();
    assert!(l.shrunk);
    assert_eq!(shape(&w.s1().today), ["{sp: a c}"]);
    assert!(w.s1().today[0].as_split().unwrap().sizes.is_none());

    let mut w = world(spec().tabs(&["a", "b"]).spaces(vec![sp(vec![], vec![gsplit("sp", &["a", "b"], "g")])]));
    w.m().extract("a", false);
    assert_eq!(shape(&w.s1().today), ["b^g"]);

    let mut w = world(spec().tabs(&["a", "b"]).spaces(vec![sp(vec![], vec![split("sp", &["a", "b"])])]));
    w.m().extract("a", true);
    assert!(w.s1().today.is_empty());
    assert!(world(spec()).m().extract("x", false).is_none());
}

fn abc() -> World {
    world(spec().tabs(&["a", "b", "c"]).spaces(vec![sp(vec![], vec![tab("a"), tab("b"), tab("c")])]))
}

#[test]
fn move_in_same_list() {
    let mut w = abc();
    assert!(w.m().move_item(&dtab("a"), &today(3)));
    assert_eq!(shape(&w.s1().today), ["b", "c", "a"]);
    let mut w = abc();
    w.m().move_item(&dtab("c"), &today(0));
    assert_eq!(shape(&w.s1().today), ["c", "a", "b"]);
}

#[test]
fn move_pin_and_unpin() {
    let mut w = world(spec().tabs(&["a"]).spaces(vec![sp(vec![], vec![tab("a")])]));
    w.m().move_item(&dtab("a"), &dt(Zone::Pinned, "s1", None, None, 0));
    assert_eq!(shape(&w.s1().pinned), ["a"]);
    assert_eq!(w.st().tabs["a"].pinned_url.as_deref(), Some("https://a.test/"));

    let mut w = world(spec().tabs(&["a"]).spaces(vec![sp(vec![tab("a")], vec![])]));
    w.tab("a").pinned_url = Some("https://home.test/".into());
    w.m().move_item(&dtab("a"), &today(0));
    assert_eq!(shape(&w.s1().today), ["a"]);
    assert_eq!(w.st().tabs["a"].pinned_url, None);
}

#[test]
fn move_favorites() {
    let mut w = world(spec().tabs(&["a", "f"]).favorites(&["f"]).spaces(vec![sp(vec![], vec![tab("a")])]));
    w.m().move_item(&dtab("a"), &dt(Zone::Favorites, "s1", None, None, 1));
    assert_eq!(w.st().favorites, ["f", "a"]);
    assert!(w.st().tabs["a"].pinned_url.is_some());

    let mut w = world(spec().tabs(&["x", "y", "z"]).favorites(&["x", "y", "z"]));
    w.m().move_item(&dtab("x"), &dt(Zone::Favorites, "s1", None, None, 3));
    assert_eq!(w.st().favorites, ["y", "z", "x"]);
}

#[test]
fn move_to_other_space() {
    let mut w = world(spec().tabs(&["a", "b"]).spaces(vec![sp(vec![], vec![tab("a"), tab("b")]).active("a"), sp(vec![], vec![])]));
    w.m().move_item(&dtab("a"), &dt(Zone::Today, "s2", None, None, 0));
    assert_eq!(shape(&w.space("s2").today), ["a"]);
    assert_eq!(w.env.reconciled, [v(&["a"])]);
    assert_eq!(w.space("s1").active_tab_id.as_deref(), Some("b"));
}

#[test]
fn move_into_folder_and_live_folder() {
    let mut w = world(spec().tabs(&["a", "in"]).spaces(vec![sp(vec![folder("f", vec![tab("in")])], vec![tab("a")])]));
    w.m().move_item(&dtab("a"), &dt(Zone::Pinned, "s1", Some("f"), None, 0));
    assert_eq!(shape(&w.s1().pinned), ["[f: a in]"]);

    let mut w = world(spec().tabs(&["a"]).spaces(vec![sp(vec![folder_with("live", vec![], |f| f.live = Some(live("x")))], vec![tab("a")])]));
    assert!(!w.m().move_item(&dtab("a"), &dt(Zone::Pinned, "s1", Some("live"), None, 0)));
    assert_eq!(shape(&w.s1().today), ["a"]);
}

#[test]
fn move_into_group() {
    let mut w = world(spec().tabs(&["a", "b", "c"]).spaces(vec![sp(vec![], vec![tab("a"), gtab("b", "g"), gtab("c", "g")])]));
    w.m().move_item(&dtab("a"), &dt(Zone::Today, "s1", None, Some("g"), 2));
    assert_eq!(shape(&w.s1().today), ["b^g", "a^g", "c^g"]);
}

#[test]
fn move_unknown() {
    let mut w = world(spec().tabs(&["a"]).spaces(vec![sp(vec![], vec![tab("a")])]));
    assert!(!w.m().move_item(&dtab("x"), &today(0)));
    assert!(!w.m().move_item(&dtab("a"), &dt(Zone::Pinned, "s1", Some("absent"), None, 0)));
}

#[test]
fn move_split_folder_group() {
    let mut w = world(spec().tabs(&["a", "b", "c"]).spaces(vec![sp(vec![], vec![split("sp", &["a", "b"]), tab("c")])]));
    w.m().move_item(&DragSource::Split { id: "sp".into() }, &dt(Zone::Pinned, "s1", None, None, 0));
    assert_eq!(shape(&w.s1().pinned), ["{sp: a b}"]);
    assert!(w.st().tabs["a"].pinned_url.is_some() && w.st().tabs["b"].pinned_url.is_some());

    let mut w = world(spec().tabs(&["a", "b", "c"]).spaces(vec![sp(vec![], vec![split("sp", &["a", "b", "c"])])]));
    w.m().move_item(&dtab("a"), &today(1));
    assert_eq!(shape(&w.s1().today), ["{sp: b c}", "a"]);

    let mut w = world(spec().tabs(&["a"]).spaces(vec![sp(vec![folder("f", vec![folder("g", vec![tab("a")])])], vec![])]));
    assert!(!w.m().move_item(&DragSource::Folder { id: "f".into() }, &dt(Zone::Pinned, "s1", Some("g"), None, 0)));
    assert!(!w.m().move_item(&DragSource::Folder { id: "f".into() }, &dt(Zone::Pinned, "s1", Some("f"), None, 0)));

    let mut w = world(spec().tabs(&["a", "b"]).spaces(vec![sp(vec![folder("f", vec![tab("a")]), tab("b")], vec![])]));
    w.m().move_item(&DragSource::Folder { id: "f".into() }, &dt(Zone::Pinned, "s1", None, None, 2));
    assert_eq!(shape(&w.s1().pinned), ["b", "[f: a]"]);

    let mut w = world(spec().tabs(&["a"]).spaces(vec![sp(vec![folder("f", vec![tab("a")])], vec![])]));
    assert!(!w.m().move_item(&DragSource::Folder { id: "f".into() }, &dt(Zone::Favorites, "s1", None, None, 0)));

    let mut w = world(spec().tabs(&["a", "b"]).spaces(vec![sp(vec![], vec![split("sp", &["a", "b"])])]));
    assert!(!w.m().move_item(&DragSource::Split { id: "sp".into() }, &dt(Zone::Favorites, "s1", None, None, 0)));
}

#[test]
fn move_group_block() {
    let mut w = world(spec().tabs(&["a", "b", "c", "d", "e"]).spaces(vec![sp(
        vec![],
        vec![gtab("a", "g"), gtab("b", "g"), gtab("c", "h"), gtab("d", "h"), tab("e")],
    )
    .groups(&[("g", TabGroup { name: "g".into(), icon: None, hue: 1.0, open: true })])]));
    w.m().move_item(&DragSource::Group { id: "g".into() }, &today(3));
    assert_eq!(shape(&w.s1().today), ["c^h", "d^h", "a^g", "b^g", "e"]);

    let mut w = world(spec().tabs(&["a", "b"]).spaces(vec![
        sp(vec![], vec![gtab("a", "g"), gtab("b", "g")]).groups(&[("g", TabGroup { name: "Mon groupe".into(), icon: None, hue: 1.0, open: true })]),
        sp(vec![], vec![]),
    ]));
    w.m().move_item(&DragSource::Group { id: "g".into() }, &dt(Zone::Today, "s2", None, None, 0));
    assert!(w.space("s1").groups.as_ref().is_none_or(|g| !g.contains_key("g")));
    assert_eq!(w.space("s2").groups.as_ref().unwrap()["g"].name, "Mon groupe");
    assert_eq!(w.env.reconciled, [v(&["a", "b"])]);
}

#[test]
fn pin_group_and_unpin_folder() {
    let mut w = world(spec().tabs(&["a", "b"]).spaces(vec![sp(vec![], vec![gtab("a", "g"), gtab("b", "g")]).groups(&[(
        "g",
        TabGroup { name: "Lecture".into(), icon: Some("\u{1f4da}".into()), hue: 33.0, open: false },
    )])]));
    assert!(w.m().pin_group("g", None));
    assert!(w.s1().today.is_empty());
    let f = w.s1().pinned[0].as_folder().unwrap();
    assert_eq!((f.name.as_str(), f.hue, f.open, f.icon.as_deref()), ("Lecture", Some(33.0), false, Some("\u{1f4da}")));
    let s = shape(&w.s1().pinned);
    assert!(s[0].starts_with("[id") && s[0].ends_with(": a b]"), "{s:?}");
    assert_eq!(w.st().tabs["a"].pinned_url.as_deref(), Some("https://a.test/"));

    let mut w = world(spec().tabs(&["a"]).spaces(vec![sp(vec![], vec![gtab("a", "g"), gtab("a", "g")])]));
    w.tab("a").title = "Titre A".into();
    w.m().pin_group("g", None);
    assert_eq!(w.s1().pinned[0].as_folder().unwrap().name, "Titre A");

    let mut w = world(spec().tabs(&["a"]).spaces(vec![sp(vec![], vec![gtab("a", "g")])]));
    assert!(!w.m().pin_group("zz", None));
    assert!(!w.m().pin_group("g", Some(&dt(Zone::Pinned, "s1", Some("absent"), None, 0))));

    let mut w = world(spec().tabs(&["a", "b"]).spaces(vec![sp(vec![folder_with("f", vec![tab("a"), folder("sub", vec![tab("b")])], |f| {
        f.hue = Some(12.0);
        f.open = false
    })], vec![])]));
    w.tab("a").pinned_url = Some("x".into());
    w.tab("b").pinned_url = Some("y".into());
    assert!(w.m().unpin_folder("f", None));
    assert!(w.s1().pinned.is_empty());
    let (a, b) = (&w.s1().today[0], &w.s1().today[1]);
    assert_eq!((item_id(a), item_id(b)), ("a".to_string(), "b".to_string()));
    assert_eq!(a.group(), b.group());
    let h = &w.s1().groups.as_ref().unwrap()[a.group().unwrap()];
    assert_eq!((h.name.as_str(), h.hue, h.open), ("f", 12.0, false));
    assert_eq!(w.st().tabs["a"].pinned_url, None);

    let mut w = world(spec().spaces(vec![sp(vec![folder("f", vec![])], vec![])]));
    assert!(!w.m().unpin_folder("f", None));
    assert!(!w.m().unpin_folder("zz", None));
}

fn item_id(i: &Item) -> String {
    match i {
        Item::Tab(t) => t.tab_id.clone(),
        Item::Split(s) => s.id.clone(),
        Item::Folder(f) => f.id.clone(),
    }
}

#[test]
fn list_commands() {
    let mut w = world(spec().tabs(&["a", "p"]).spaces(vec![sp(vec![tab("p")], vec![tab("a")])]));
    assert!(w.m().toggle_pin("a"));
    assert_eq!(shape(&w.s1().pinned), ["p", "a"]);
    assert!(w.m().toggle_pin("a"));
    assert_eq!(shape(&w.s1().today), ["a"]);

    let mut w = world(spec().tabs(&["a", "b"]).spaces(vec![sp(vec![], vec![split("sp", &["a", "b"])])]));
    w.m().toggle_pin("a");
    assert_eq!(shape(&w.s1().pinned), ["{sp: a b}"]);

    let mut w = world(spec().tabs(&["a", "b"]).spaces(vec![sp(vec![], vec![tab("a"), tab("b")])]));
    w.m().toggle_favorite("b");
    assert_eq!(w.st().favorites, ["b"]);
    w.m().toggle_favorite("b");
    assert!(w.st().favorites.is_empty());
    assert_eq!(shape(&w.s1().today), ["b", "a"]);

    let mut w = abc();
    assert!(w.m().move_tab_step("b", -1));
    assert_eq!(shape(&w.s1().today), ["b", "a", "c"]);
    assert!(!w.m().move_tab_step("b", -1));
    assert!(!w.m().move_tab_step("c", 1));
    assert!(w.m().move_tab_step("a", 1));
    assert_eq!(shape(&w.s1().today), ["b", "c", "a"]);

    let mut w = world(spec().tabs(&["a", "b"]).spaces(vec![sp(vec![folder("f", vec![tab("a"), tab("b")])], vec![])]));
    w.m().move_tab_step("b", -1);
    assert_eq!(shape(&w.s1().pinned), ["[f: b a]"]);

    let mut w = world(spec().tabs(&["a", "x"]).spaces(vec![sp(vec![], vec![tab("a")]), sp(vec![], vec![tab("x")])]));
    w.m().move_tab_to_space("a", "s2");
    assert_eq!(shape(&w.space("s2").today), ["a", "x"]);
}

// --------------------------------------------------------------------- splits

fn split_of(w: &World, i: usize) -> SplitItem {
    w.s1().today[i].as_split().unwrap().clone()
}

#[test]
fn can_split_cases() {
    let mut w = world(spec().tabs(&["a", "b", "c", "d"]).spaces(vec![sp(vec![], vec![tab("a"), split("sp", &["b", "c", "d"])]).active("a")]));
    assert!(w.m().can_split());
    w.space_mut("s1").active_tab_id = Some("b".into());
    assert!(w.m().can_split());

    let mut w = world(spec().tabs(&["a", "b", "c", "d", "f"]).favorites(&["f"]).spaces(vec![sp(vec![], vec![split("sp", &["a", "b", "c", "d"])]).active("a")]));
    assert!(!w.m().can_split());
    w.space_mut("s1").active_tab_id = Some("f".into());
    assert!(!w.m().can_split());
    w.space_mut("s1").active_tab_id = None;
    assert!(!w.m().can_split());
}

#[test]
fn split_with_cases() {
    let mut w = world(spec().tabs(&["a", "b", "c"]).spaces(vec![sp(vec![], vec![tab("a"), tab("b"), tab("c")]).active("a")]));
    assert!(w.m().split_with("c", None));
    let s = shape(&w.s1().today);
    assert!(s[0].starts_with("{id") && s[0].ends_with(": a c}") && s[1] == "b", "{s:?}");

    let mut w = world(spec().tabs(&["a", "b"]).spaces(vec![sp(vec![], vec![tab("a"), tab("b")]).active("a")]));
    w.m().split_with("b", Some(DropEdge::Left));
    assert_eq!(split_of(&w, 0).tab_ids, ["b", "a"]);

    let mut w = world(spec().tabs(&["a", "b"]).spaces(vec![sp(vec![], vec![gtab("a", "g"), tab("b")]).active("a")]));
    w.m().split_with("b", None);
    assert_eq!(split_of(&w, 0).group.as_deref(), Some("g"));

    let mut w = world(spec().tabs(&["a", "b", "c"]).spaces(vec![sp(
        vec![],
        vec![Item::Split(SplitItem { id: "sp".into(), tab_ids: v(&["a", "b"]), sizes: Some(vec![0.7, 0.3]), ..Default::default() }), tab("c")],
    )
    .active("a")]));
    w.m().split_with("c", None);
    assert_eq!(split_of(&w, 0).tab_ids, ["a", "b", "c"]);
    assert!(split_of(&w, 0).sizes.is_none());

    let mut w = world(spec().tabs(&["p", "a"]).spaces(vec![sp(vec![tab("p")], vec![tab("a")]).active("p")]));
    w.m().split_with("a", None);
    assert_eq!(w.st().tabs["a"].pinned_url.as_deref(), Some("https://a.test/"));
    assert!(w.s1().today.is_empty());

    let mut w = world(spec().tabs(&["a", "p"]).spaces(vec![sp(vec![tab("p")], vec![tab("a")]).active("a")]));
    w.tab("p").pinned_url = Some("x".into());
    w.m().split_with("p", None);
    assert_eq!(w.st().tabs["p"].pinned_url, None);
}

#[test]
fn split_with_refusals() {
    let mut w = world(spec().tabs(&["a", "b", "f", "peek", "c", "d", "e"]).favorites(&["f"]).spaces(vec![sp(
        vec![],
        vec![split("sp", &["a", "b"]), tab("c"), tab("d"), tab("e")],
    )
    .active("a")]));
    w.data.detached.insert("peek".into());
    for id in ["a", "f", "b", "peek", "zz"] {
        assert!(!w.m().split_with(id, None), "{id}");
    }
    w.m().split_with("c", None);
    w.m().split_with("d", None);
    assert!(!w.m().split_with("e", None));
}

#[test]
fn split_with_reconciles() {
    let mut w = world(spec().tabs(&["a", "b", "c"]).spaces(vec![sp(vec![], vec![tab("a"), tab("b"), tab("c")]).active("a")]));
    w.m().split_with("b", None);
    assert_eq!(w.env.reconciled, [v(&["b"])]);
}

#[test]
fn split_undo_and_adjust() {
    let mut w = world(spec().tabs(&["a", "b"]).spaces(vec![sp(vec![], vec![gsplit("sp", &["a", "b"], "g")])]));
    assert!(w.m().unsplit("sp"));
    assert_eq!(shape(&w.s1().today), ["a^g", "b^g"]);
    assert!(!w.m().unsplit("sp"));

    let mut w = world(spec().tabs(&["a", "b", "c"]).spaces(vec![sp(vec![], vec![split("sp", &["a", "b", "c"])])]));
    assert!(w.m().remove_from_split("a"));
    assert_eq!(shape(&w.s1().today), ["{sp: b c}", "a"]);
    assert!(!w.m().remove_from_split("a"));

    let mut w = world(spec().tabs(&["a", "b", "c"]).spaces(vec![sp(vec![], vec![gsplit("sp", &["a", "b", "c"], "g")])]));
    w.m().remove_from_split("a");
    assert_eq!(shape(&w.s1().today), ["{sp: b c}^g", "a^g"]);

    let mut w = world(spec().tabs(&["a", "b", "c"]).spaces(vec![sp(
        vec![],
        vec![Item::Split(SplitItem { id: "sp".into(), tab_ids: v(&["a", "b"]), sizes: Some(vec![0.6, 0.4]), ..Default::default() }), tab("c")],
    )]));
    assert!(w.m().set_layout("sp", SplitLayout::Rows));
    assert_eq!(split_of(&w, 0).layout, Some(SplitLayout::Rows));
    assert!(split_of(&w, 0).sizes.is_none());
    assert!(!w.m().set_layout("sp", SplitLayout::Grid));
    assert!(!w.m().set_layout("zz", SplitLayout::Rows));

    let mut w = world(spec().tabs(&["a", "b"]).spaces(vec![sp(vec![], vec![split("sp", &["a", "b"])])]));
    assert!(w.m().set_sizes("sp", &[300.0, 100.0]));
    let sizes = split_of(&w, 0).sizes.unwrap();
    assert!((sizes.iter().sum::<f64>() - 1.0).abs() < 1e-9);
    assert!(sizes[0] > sizes[1]);
    assert!(!w.m().set_sizes("zz", &[1.0, 1.0]));
}

// -------------------------------------------------------------------- archive

const HOUR: i64 = 3_600_000;

#[test]
fn archive_tab_cases() {
    let mut w = world(spec().tabs(&["a", "b"]).spaces(vec![sp(vec![], vec![tab("a"), tab("b")]).active("a")]));
    w.tab("a").title = "A".into();
    w.tab("a").favicon = Some("ico".into());
    let entry = w.m().archive_tab("a").unwrap();
    assert_eq!((entry.url.as_str(), entry.title.as_str(), entry.favicon.as_deref(), entry.archived_at), ("https://a.test/", "A", Some("ico"), w.env.now));
    assert_eq!(w.st().archive[0], entry);
    assert_eq!(shape(&w.s1().today), ["b"]);
    assert!(!w.st().tabs.contains_key("a"));
    assert_eq!(w.s1().active_tab_id.as_deref(), Some("b"));

    let mut w = world(spec().tabs(&["a"]).spaces(vec![sp(vec![], vec![tab("a")])]));
    w.data.state.archive = (0..ARCHIVE_MAX).map(|i| ArchivedTab { url: format!("u{i}"), ..Default::default() }).collect();
    w.m().archive_tab("a");
    assert_eq!(w.st().archive.len(), ARCHIVE_MAX);
    assert_eq!(w.st().archive[0].url, "https://a.test/");
    assert!(world(spec()).m().archive_tab("x").is_none());
}

fn clear_world() -> World {
    world(spec().tabs(&["a", "b", "c", "p"]).spaces(vec![sp(vec![tab("p")], vec![gtab("a", "g"), gtab("b", "g"), tab("c")])
        .groups(&[("g", TabGroup { name: "G".into(), icon: None, hue: 1.0, open: true })])
        .active("b")]))
}

#[test]
fn clear_today_archives_all() {
    let mut w = clear_world();
    let cleared = w.m().clear_today().unwrap();
    assert_eq!(cleared.count, 3);
    assert!(w.s1().today.is_empty());
    assert_eq!(w.st().archive.len(), 3);
    assert_eq!(w.st().tabs.keys().collect::<Vec<_>>(), ["p"]);
    assert_eq!(w.s1().active_tab_id.as_deref(), Some("p"));
    assert!(w.env.saved.contains(&"a".to_string()));
    assert!(world(spec()).m().clear_today().is_none());
}

#[test]
fn clear_today_undo() {
    let mut w = clear_world();
    w.data.histories.insert("a".into(), crate::core::store::TabHistory { entries: vec![], index: 0 });
    let cleared = w.m().clear_today().unwrap();
    assert!(w.m().restore_archive(&cleared.undo));
    assert_eq!(shape(&w.s1().today), ["a^g", "b^g", "c"]);
    assert_eq!(w.s1().groups.as_ref().unwrap()["g"].name, "G");
    assert_eq!(w.s1().active_tab_id.as_deref(), Some("b"));
    assert!(w.st().archive.is_empty());
    assert!(w.data.histories.contains_key("a"));
    assert!(!w.st().tabs["a"].loaded);
}

#[test]
fn clear_today_undo_keeps_new_active() {
    let mut w = clear_world();
    let cleared = w.m().clear_today().unwrap();
    w.m().open("https://new.test/", &OpenOptions { activate: true, ..Default::default() });
    let fresh = w.s1().active_tab_id.clone();
    w.m().restore_archive(&cleared.undo);
    assert_eq!(w.s1().active_tab_id, fresh);
    assert_eq!(w.s1().today.len(), 4);

    let mut w = world(spec().tabs(&["a"]).spaces(vec![sp(vec![], vec![tab("a")]), sp(vec![], vec![])]));
    let cleared = w.m().clear_today().unwrap();
    w.data.state.spaces.remove(0);
    assert!(!w.m().restore_archive(&cleared.undo));
}

fn old(w: &mut World, id: &str, hours: i64) {
    w.tab(id).last_active = 1_000_000 - hours * HOUR;
}
fn never(_: &str) -> bool {
    false
}

#[test]
fn sweep_cases() {
    let mut w = world(spec().tabs(&["a", "b", "c"]).spaces(vec![sp(vec![], vec![tab("a"), tab("b"), tab("c")])]));
    old(&mut w, "a", 20);
    old(&mut w, "b", 1);
    old(&mut w, "c", 30);
    assert_eq!(w.m().sweep(&never), 2);
    assert_eq!(shape(&w.s1().today), ["b"]);
    let mut urls: Vec<_> = w.st().archive.iter().map(|e| e.url.clone()).collect();
    urls.sort();
    assert_eq!(urls, ["https://a.test/", "https://c.test/"]);

    let mut w = world(spec().tabs(&["a", "p"]).spaces(vec![sp(vec![tab("p")], vec![tab("a")]).active("a")]));
    old(&mut w, "a", 40);
    old(&mut w, "p", 40);
    assert_eq!(w.m().sweep(&never), 0);
    assert_eq!(w.s1().today.len(), 1);

    let mut w = world(spec().tabs(&["a"]).spaces(vec![sp(vec![], vec![tab("a")])]));
    old(&mut w, "a", 400);
    w.data.state.settings.archive_after_hours = 0.0;
    assert_eq!(w.m().sweep(&never), 0);

    let mut w = world(spec().tabs(&["a", "b"]).spaces(vec![sp(vec![], vec![tab("a"), tab("b")])]));
    old(&mut w, "a", 40);
    old(&mut w, "b", 40);
    assert_eq!(w.m().sweep(&|id| id == "a"), 1);
    assert_eq!(shape(&w.s1().today), ["a"]);
}

#[test]
fn sweep_groups_splits_spaces() {
    let mut w = world(spec().tabs(&["a", "b", "c", "d"]).spaces(vec![sp(vec![], vec![gtab("a", "g"), gtab("b", "g"), gtab("c", "h"), gtab("d", "h")])]));
    for (id, h) in [("a", 40), ("b", 40), ("c", 1), ("d", 40)] {
        old(&mut w, id, h);
    }
    assert_eq!(w.m().sweep(&never), 2);
    assert_eq!(shape(&w.s1().today), ["c^h", "d^h"]);

    let mut w = world(spec().tabs(&["a", "b"]).spaces(vec![sp(vec![], vec![gtab("a", "g"), gtab("b", "g")]).active("b")]));
    old(&mut w, "a", 40);
    old(&mut w, "b", 40);
    assert_eq!(w.m().sweep(&never), 0);

    let mut w = world(spec().tabs(&["a", "b"]).spaces(vec![sp(vec![], vec![split("sp", &["a", "b"])]).active("a")]));
    old(&mut w, "a", 40);
    old(&mut w, "b", 40);
    assert_eq!(w.m().sweep(&never), 0);

    let mut w = world(spec().tabs(&["a", "b"]).spaces(vec![sp(vec![], vec![tab("a")]), sp(vec![], vec![tab("b")])]));
    old(&mut w, "a", 40);
    old(&mut w, "b", 40);
    assert_eq!(w.m().sweep(&never), 2);
}

// --------------------------------------------------------------------- closed

#[test]
fn remember_cases() {
    let mut w = world(spec().tabs(&["a", "b"]).spaces(vec![sp(vec![], vec![tab("b"), tab("a")])]));
    w.tab("a").title = "A".into();
    w.tab("a").favicon = Some("ico".into());
    w.tab("a").custom_title = Some("Perso".into());
    w.data.histories.insert("a".into(), crate::core::store::TabHistory { entries: vec![crate::core::store::NavEntry { url: "https://a.test/".into(), title: "A".into(), page_state: None }], index: 0 });
    let l = w.m().locate("a");
    w.m().remember("a", l.as_ref(), None, None);
    let c = &w.data.closed[0];
    assert_eq!((c.url.as_str(), c.title.as_str(), c.favicon.as_deref(), c.custom_title.as_deref()), ("https://a.test/", "A", Some("ico"), Some("Perso")));
    assert_eq!((c.space_id.as_str(), c.zone, c.index), ("s1", Zone::Today, 1));
    assert_eq!(c.history.as_ref().unwrap().index, 0);
    assert_eq!(w.env.saved, ["a"]);

    let mut w = world(spec().tabs(&["a", "b"]).spaces(vec![sp(vec![folder("f", vec![tab("a"), tab("b")])], vec![])]));
    let l = w.m().locate("b");
    w.m().remember("b", l.as_ref(), None, None);
    let c = &w.data.closed[0];
    assert_eq!((c.zone, c.folder_id.as_deref(), c.index), (Zone::Pinned, Some("f"), 1));

    let mut w = world(spec().tabs(&["a"]).spaces(vec![sp(vec![], vec![gtab("a", "g")])]));
    let l = w.m().locate("a");
    w.m().remember("a", l.as_ref(), Some(7), Some(&TabGroup { name: "Groupe".into(), icon: None, hue: 1.0, open: true }));
    let c = &w.data.closed[0];
    assert_eq!((c.group_id.as_deref(), c.batch, c.group.as_ref().unwrap().name.as_str()), (Some("g"), Some(7), "Groupe"));

    let mut w = world(spec().tabs(&["a"]));
    w.m().remember("a", None, None, None);
    let c = &w.data.closed[0];
    assert_eq!((c.space_id.as_str(), c.zone, c.index), ("s1", Zone::Today, 0));

    let mut w = world(spec());
    w.m().remember("x", None, None, None);
    assert!(w.data.closed.is_empty());
}

#[test]
fn closed_stack_capped() {
    let ids: Vec<String> = (0..CLOSED_MAX + 5).map(|i| format!("t{i}")).collect();
    let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
    let mut w = world(spec().tabs(&refs));
    for id in &ids {
        w.m().remember(id, None, None, None);
    }
    assert_eq!(w.data.closed.len(), CLOSED_MAX);
    assert_eq!(w.data.closed[0].url, format!("https://{}.test/", ids.last().unwrap()));
}

#[test]
fn reopen_cases() {
    assert_eq!(world(spec()).m().reopen(), None);

    let mut w = world(spec().tabs(&["a", "b", "c"]).spaces(vec![sp(vec![], vec![tab("a"), tab("b"), tab("c")])]));
    w.data.histories.insert("b".into(), crate::core::store::TabHistory { entries: vec![crate::core::store::NavEntry { url: "https://b.test/".into(), title: "b".into(), page_state: None }], index: 0 });
    let l = w.m().locate("b");
    w.m().remember("b", l.as_ref(), None, None);
    w.m().remove("b");
    assert_eq!(shape(&w.s1().today), ["a", "c"]);
    assert_eq!(w.m().reopen(), Some(0));
    let reopened = item_id(&w.s1().today[1]);
    assert_eq!(shape(&w.s1().today)[0], "a");
    assert_eq!(shape(&w.s1().today)[2], "c");
    assert_eq!(w.st().tabs[&reopened].url, "https://b.test/");
    assert_eq!(w.data.histories[&reopened].index, 0);
    assert_eq!(w.s1().active_tab_id.as_deref(), Some(reopened.as_str()));
}

#[test]
fn reopen_other_space_and_favorites() {
    let mut w = world(spec().tabs(&["a", "x"]).spaces(vec![sp(vec![], vec![tab("a")]), sp(vec![], vec![tab("x")])]).active("s2"));
    let l = w.m().locate("a");
    w.m().remember("a", l.as_ref(), None, None);
    w.m().remove("a");
    w.m().reopen();
    assert_eq!(w.st().active_space_id, "s1");
    assert_eq!(w.space("s1").today.len(), 1);

    let mut w = world(spec().tabs(&["f1", "f2"]).favorites(&["f1", "f2"]).spaces(vec![sp(vec![], vec![]), sp(vec![], vec![])]).active("s2"));
    let l = w.m().locate("f1");
    w.m().remember("f1", l.as_ref(), None, None);
    w.m().remove("f1");
    w.m().reopen();
    assert_eq!(w.st().favorites.len(), 2);
    assert_eq!(w.st().active_space_id, "s2");
    assert_eq!(w.space("s2").active_tab_id.as_deref(), Some(w.st().favorites[0].as_str()));
}

#[test]
fn reopen_pinned() {
    let mut w = world(spec().tabs(&["a", "b"]).spaces(vec![sp(vec![folder("f", vec![tab("a"), tab("b")])], vec![])]));
    w.tab("a").pinned_url = Some("https://home.test/".into());
    w.tab("b").pinned_url = Some("https://b.test/".into());
    let l = w.m().locate("a");
    w.m().remember("a", l.as_ref(), None, None);
    w.m().remove("a");
    w.m().reopen();
    let f = w.s1().pinned[0].as_folder().unwrap().clone();
    assert_eq!(f.items.len(), 2);
    let first = item_id(&f.items[0]);
    assert_ne!(first, "b");
    assert_eq!(w.st().tabs[&first].pinned_url.as_deref(), Some("https://home.test/"));

    let mut w = world(spec().tabs(&["a", "p"]).spaces(vec![sp(vec![tab("p"), folder("f", vec![tab("a")])], vec![])]));
    let l = w.m().locate("a");
    w.m().remember("a", l.as_ref(), None, None);
    w.space_mut("s1").pinned.pop();
    w.m().reopen();
    assert_eq!(w.s1().pinned.len(), 2);
    assert!(matches!(w.s1().pinned[1], Item::Tab(_)));

    let mut w = world(spec().tabs(&["a", "x"]).spaces(vec![sp(vec![tab("a")], vec![]), sp(vec![], vec![tab("x")])]));
    let l = w.m().locate("a");
    w.m().remember("a", l.as_ref(), None, None);
    w.data.state.spaces.remove(0);
    w.data.state.active_space_id = "s2".into();
    w.m().reopen();
    assert_eq!(w.space("s2").today.len(), 2);
}

#[test]
fn reopen_group_whole() {
    let mut w = world(spec().tabs(&["a", "b", "c"]).spaces(vec![sp(vec![], vec![gtab("a", "g"), gtab("b", "g"), tab("c")])
        .groups(&[("g", TabGroup { name: "Mon groupe".into(), icon: None, hue: 4.0, open: true })])]));
    assert!(w.m().close_group(0, "g").is_some());
    assert_eq!(shape(&w.s1().today), ["c"]);
    assert_eq!(w.m().reopen(), Some(0));
    assert_eq!(w.s1().today.len(), 3);
    let g0 = w.s1().today[0].group().unwrap().to_string();
    assert_eq!(w.s1().today[1].group(), Some(g0.as_str()));
    assert_eq!(w.s1().groups.as_ref().unwrap()[&g0].name, "Mon groupe");
    assert_eq!(w.s1().active_tab_id.as_deref(), Some(item_id(&w.s1().today[0]).as_str()));
}

#[test]
fn reopen_batch_without_header() {
    let mut w = world(spec().tabs(&["a", "b"]).spaces(vec![sp(vec![folder("f", vec![tab("a"), tab("b")])], vec![])]));
    w.m().delete_folder("f", true);
    assert_eq!(w.data.closed.len(), 2);
    assert_eq!(w.m().reopen(), Some(1));
    assert_eq!(w.data.closed.len(), 1);
    assert_eq!(w.data.closed[0].batch, None);
}

#[test]
fn snapshots_and_batches() {
    let mut w = world(spec().tabs(&["a", "b"]));
    w.data.histories.insert("a".into(), Default::default());
    let snap = w.m().snapshot_tabs(&v(&["a", "b", "inconnu"]));
    assert_eq!(snap.tabs.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["a", "b"]);
    w.data.state.tabs.shift_remove("a");
    w.data.histories.shift_remove("a");
    w.tab("b").title = "modified".into();
    w.m().restore_tabs(&snap);
    let a = &w.st().tabs["a"];
    assert!(!a.loaded && !a.loading && !a.audible);
    assert!(w.data.histories.contains_key("a"));
    assert_eq!(w.st().tabs["b"].title, "modified");

    let mut w = world(spec().tabs(&["a", "b", "c"]));
    w.m().remember("a", None, Some(1), None);
    w.m().remember("b", None, Some(2), None);
    w.m().remember("c", None, None, None);
    w.m().drop_batch(1);
    assert_eq!(w.data.closed.iter().map(|c| c.batch).collect::<Vec<_>>(), [None, Some(2)]);

    let mut w = world(spec());
    assert_eq!(w.m().next_batch(), 1);
    assert_eq!(w.m().next_batch(), 2);
}

// -------------------------------------------------------------------- folders

#[test]
fn create_folder_cases() {
    let mut w = world(spec().tabs(&["p"]).spaces(vec![sp(vec![tab("p")], vec![])]));
    let created = w.m().create_folder(None);
    assert_eq!(created.name, "Folder");
    assert_eq!(shape(&w.s1().pinned), [format!("[{}: ]", created.id), "p".to_string()]);

    let mut w = world(spec().spaces(vec![sp(vec![folder_with("f", vec![], |f| f.open = false)], vec![])]));
    let created = w.m().create_folder(Some("f"));
    assert_eq!(shape(&w.s1().pinned), [format!("[f: [{}: ]]", created.id)]);
    assert!(w.s1().pinned[0].as_folder().unwrap().open);
}

#[test]
fn folder_settings() {
    let mut w = world(spec().spaces(vec![sp(vec![folder_with("f", vec![], |f| {
        f.hue = Some(10.0);
        f.icon = Some("\u{1f4c1}".into())
    })], vec![])]));
    assert!(w.m().rename_folder("f", "  Travail  "));
    assert_eq!(w.s1().pinned[0].as_folder().unwrap().name, "Travail");
    w.m().rename_folder("f", &"x".repeat(200));
    assert_eq!(w.s1().pinned[0].as_folder().unwrap().name.chars().count(), 80);
    assert!(!w.m().rename_folder("f", "   "));
    assert!(!w.m().rename_folder("zz", "ok"));
    w.m().set_folder_hue("f", Some(200.0));
    assert_eq!(w.s1().pinned[0].as_folder().unwrap().hue, Some(200.0));
    w.m().set_folder_hue("f", None);
    assert_eq!(w.s1().pinned[0].as_folder().unwrap().hue, None);
    w.m().set_folder_icon("f", Some("\u{1f525}"));
    assert_eq!(w.s1().pinned[0].as_folder().unwrap().icon.as_deref(), Some("\u{1f525}"));
    w.m().set_folder_icon("f", None);
    assert_eq!(w.s1().pinned[0].as_folder().unwrap().icon, None);
    w.m().toggle_folder("f", None);
    assert!(!w.s1().pinned[0].as_folder().unwrap().open);
    w.m().toggle_folder("f", Some(true));
    assert!(w.s1().pinned[0].as_folder().unwrap().open);
    assert!(!w.m().toggle_folder("zz", None));
}

#[test]
fn delete_folder_without_tabs() {
    let mut w = world(spec().tabs(&["a", "b", "p"]).spaces(vec![sp(vec![tab("p"), folder("f", vec![tab("a"), tab("b")])], vec![])]));
    assert_eq!(w.m().delete_folder("f", false), Some(None));
    assert_eq!(shape(&w.s1().pinned), ["p", "a", "b"]);
    assert!(w.env.destroyed.is_empty());
    assert_eq!(world(spec()).m().delete_folder("zz", true), None);
}

#[test]
fn delete_folder_with_tabs_and_undo() {
    let mut w = world(spec().tabs(&["a", "b", "p"]).spaces(vec![sp(vec![tab("p"), folder("f", vec![tab("a"), tab("b")])], vec![]).active("a")]));
    let removed = w.m().delete_folder("f", true).unwrap().unwrap();
    assert_eq!(removed.count, 2);
    assert_eq!(shape(&w.s1().pinned), ["p"]);
    assert_eq!(w.st().tabs.keys().collect::<Vec<_>>(), ["p"]);
    assert_eq!(w.env.destroyed, ["a", "b"]);
    assert_eq!(w.data.closed.len(), 2);
    assert_eq!(w.data.closed[0].batch, w.data.closed[1].batch);
    assert_eq!(w.data.closed[0].url, "https://a.test/");
    assert_eq!(w.s1().active_tab_id.as_deref(), Some("p"));

    assert!(w.m().restore_removed(&removed.undo));
    assert_eq!(shape(&w.s1().pinned), ["[f: a b]", "p"]);
    let mut keys: Vec<_> = w.st().tabs.keys().cloned().collect();
    keys.sort();
    assert_eq!(keys, ["a", "b", "p"]);
    assert!(w.data.closed.is_empty());
    assert_eq!(w.s1().active_tab_id.as_deref(), Some("a"));
}

#[test]
fn delete_nested_folder_and_edge_cases() {
    let mut w = world(spec().tabs(&["a", "b"]).spaces(vec![sp(vec![folder("outer", vec![tab("a"), folder("inner", vec![tab("b")])])], vec![])]));
    let removed = w.m().delete_folder("inner", true).unwrap().unwrap();
    assert_eq!(shape(&w.s1().pinned), ["[outer: a]"]);
    w.m().restore_removed(&removed.undo);
    assert_eq!(shape(&w.s1().pinned), ["[outer: a [inner: b]]"]);

    let mut w = world(spec().tabs(&["a"]).spaces(vec![sp(vec![folder("f", vec![tab("a")])], vec![]), sp(vec![], vec![])]));
    let removed = w.m().delete_folder("f", true).unwrap().unwrap();
    w.data.state.spaces.remove(0);
    assert!(!w.m().restore_removed(&removed.undo));

    let mut w = world(spec().spaces(vec![sp(vec![folder("f", vec![])], vec![])]));
    assert_eq!(w.m().delete_folder("f", true), Some(None));
}

#[test]
fn add_to_folder_cases() {
    let mut w = world(spec().tabs(&["a", "x"]).spaces(vec![sp(vec![folder_with("f", vec![tab("x")], |f| f.open = false)], vec![tab("a")])]));
    let r = w.m().add_to_folder("a", Some("f")).unwrap();
    assert_eq!((r.created, r.moved), (false, true));
    assert_eq!(shape(&w.s1().pinned), ["[f: x a]"]);
    assert!(w.s1().pinned[0].as_folder().unwrap().open);

    let mut w = world(spec().tabs(&["a"]).spaces(vec![sp(vec![], vec![tab("a")])]));
    let r = w.m().add_to_folder("a", None).unwrap();
    assert!(r.created);
    assert_eq!(shape(&w.s1().pinned), [format!("[{}: a]", r.folder_id)]);
    assert!(w.s1().today.is_empty());

    let mut w = world(spec().tabs(&["f"]).favorites(&["f"]));
    assert!(w.m().add_to_folder("f", None).is_none());
    assert!(w.m().add_to_folder("zz", None).is_none());

    let mut w = world(spec().tabs(&["a", "b"]).spaces(vec![sp(vec![folder("f", vec![])], vec![split("sp", &["a", "b"])])]));
    w.m().add_to_folder("a", Some("f"));
    assert_eq!(shape(&w.s1().pinned), ["[f: {sp: a b}]"]);
}

// --------------------------------------------------------------------- groups

fn group_world() -> World {
    world(spec().tabs(&["a", "b"]).spaces(vec![sp(vec![], vec![gtab("a", "g"), gtab("b", "g")]).groups(&[("g", header("Groupe"))])]))
}

#[test]
fn group_headers() {
    let mut w = group_world();
    assert_eq!(w.m().group_header("g").unwrap().name, "Groupe");
    assert!(w.m().group_header("zz").is_none());
    assert!(w.m().rename_group("g", "  Lecture "));
    assert_eq!(w.m().group_header("g").unwrap().name, "Lecture");
    assert!(!w.m().rename_group("g", " "));
    assert!(!w.m().rename_group("zz", "x"));
    w.m().set_group_hue("g", 99.0);
    w.m().set_group_icon("g", Some("\u{1f3b5}"));
    w.m().toggle_group("g", None);
    let h = w.m().group_header("g").unwrap().clone();
    assert_eq!((h.hue, h.icon.as_deref(), h.open), (99.0, Some("\u{1f3b5}"), false));
    w.m().set_group_icon("g", None);
    w.m().toggle_group("g", Some(true));
    let h = w.m().group_header("g").unwrap().clone();
    assert_eq!((h.icon, h.open), (None, true));
}

#[test]
fn leave_and_ungroup() {
    let mut w = world(spec().tabs(&["a", "b", "c", "d"]).spaces(vec![sp(vec![], vec![gtab("a", "g"), gtab("b", "g"), gtab("c", "g"), tab("d")])]));
    assert!(w.m().leave_group("a"));
    assert_eq!(shape(&w.s1().today), ["b^g", "c^g", "a", "d"]);

    let mut w = world(spec().tabs(&["a", "f"]).favorites(&["f"]).spaces(vec![sp(vec![], vec![tab("a")])]));
    assert!(!w.m().leave_group("a"));
    assert!(!w.m().leave_group("f"));

    let mut w = world(spec().tabs(&["a", "b"]).spaces(vec![sp(vec![], vec![gtab("a", "g"), gtab("b", "g")])]));
    w.m().ungroup("g");
    assert_eq!(shape(&w.s1().today), ["a", "b"]);
}

fn close_group_world() -> World {
    world(spec().tabs(&["a", "b", "c", "d"]).spaces(vec![sp(vec![], vec![tab("d"), gtab("a", "g"), gtab("b", "g"), tab("c")])
        .groups(&[("g", header("Mon groupe"))])
        .active("a")]))
}

#[test]
fn close_group_and_undo() {
    let mut w = close_group_world();
    let removed = w.m().close_group(0, "g").unwrap();
    assert_eq!(removed.count, 2);
    assert_eq!(shape(&w.s1().today), ["d", "c"]);
    let mut keys: Vec<_> = w.st().tabs.keys().cloned().collect();
    keys.sort();
    assert_eq!(keys, ["c", "d"]);
    assert_eq!(w.env.destroyed, ["a", "b"]);
    assert_eq!(w.data.closed.len(), 2);
    assert_eq!(w.data.closed[0].group.as_ref().unwrap().name, "Mon groupe");
    assert_eq!(w.data.closed[0].url, "https://a.test/");
    assert_eq!(w.s1().active_tab_id.as_deref(), Some("c"));

    assert!(w.m().restore_removed(&removed.undo));
    assert_eq!(shape(&w.s1().today), ["d", "a^g", "b^g", "c"]);
    assert_eq!(w.s1().groups.as_ref().unwrap()["g"].name, "Mon groupe");
    assert_eq!(w.s1().active_tab_id.as_deref(), Some("a"));
    assert!(w.data.closed.is_empty());
    assert_eq!(w.st().tabs.len(), 4);
}

#[test]
fn close_unknown_group() {
    let mut w = close_group_world();
    assert!(w.m().close_group(0, "zz").is_none());
    assert_eq!(w.m().next_batch(), 1);
}

// ---------------------------------------------------------------------- spaces

#[test]
fn space_creation_and_settings() {
    let mut w = world(spec());
    let space = w.m().create_space(&SpaceDraft { name: "  Travail ".into(), icon: "\u{1f4bc}".into(), hue: 400.0, theme: None });
    assert_eq!(w.st().spaces.last().unwrap(), &space);
    assert_eq!((space.name.as_str(), space.icon.as_str(), space.hue, space.active_tab_id.clone()), ("Travail", "\u{1f4bc}", 40.0, None));
    assert!(space.pinned.is_empty() && space.today.is_empty());

    let s = w.m().create_space(&SpaceDraft::default());
    assert_eq!((s.name.as_str(), s.icon.as_str()), ("Espace", "\u{2728}"));

    let mut w = world(spec());
    assert!(w.m().update_space("s1", &SpaceDraft { name: "Neuf".into(), icon: "N".into(), hue: 12.0, theme: Some(crate::types::SpaceTheme { hues: vec![10.0, 200.0], grain: 0.5 }) }));
    assert!(w.s1().theme.is_some());
    w.m().update_space("s1", &SpaceDraft { name: "Neuf".into(), icon: "N".into(), hue: 12.0, theme: None });
    assert!(w.s1().theme.is_none());
    assert!(!w.m().update_space("zz", &SpaceDraft { name: "x".into(), icon: "x".into(), hue: 1.0, theme: None }));

    let mut w = world(spec().spaces(vec![sp(vec![], vec![]), sp(vec![], vec![])]));
    assert!(w.m().switch_to_space("s2"));
    assert_eq!(w.st().active_space_id, "s2");
    assert!(!w.m().switch_to_space("zz"));
    assert_eq!(w.st().active_space_id, "s2");
}

#[test]
fn space_profile_and_tab_ids() {
    let mut w = world(spec().tabs(&["p", "a", "in"]).spaces(vec![sp(vec![tab("p"), folder("f", vec![tab("in")])], vec![tab("a")])]));
    assert!(w.m().set_space_profile("s1", SpaceProfile::Isolated));
    assert_eq!(w.s1().profile, Some(SpaceProfile::Isolated));
    assert_eq!(w.env.reconciled, [v(&["p", "in", "a"])]);
    assert!(!w.m().set_space_profile("zz", SpaceProfile::Isolated));

    let w = world(spec().tabs(&["p", "a", "in"]).spaces(vec![sp(vec![folder("f", vec![tab("in")]), tab("p")], vec![tab("a")])]));
    assert_eq!(space_tab_ids(w.s1()), ["in", "p", "a"]);
}

#[test]
fn routes() {
    let mut w = world(spec().spaces(vec![sp(vec![], vec![]), sp(vec![], vec![])]));
    assert!(w.m().add_route("https://GitHub.com/orgs", "s2"));
    let route = w.st().settings.routes[0].clone();
    assert_eq!(route.space_id, "s2");
    w.m().add_route(&route.pattern, "s1");
    assert_eq!(w.st().settings.routes.len(), 1);
    assert_eq!(w.st().settings.routes[0].space_id, "s1");

    let mut w = world(spec());
    assert!(!w.m().add_route("  ", "s1"));
    assert!(!w.m().add_route("github.com", "zz"));
    assert!(w.st().settings.routes.is_empty());

    let mut w = world(spec());
    w.data.state.settings.routes = vec![
        SpaceRoute { id: "r1".into(), pattern: "a.com".into(), space_id: "s1".into() },
        SpaceRoute { id: "r2".into(), pattern: "b.com".into(), space_id: "s1".into() },
    ];
    w.m().remove_route("r1");
    assert_eq!(w.st().settings.routes.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), ["r2"]);
}

#[test]
fn space_order_and_navigation() {
    let mut w = world(spec().spaces(vec![sp(vec![], vec![]), sp(vec![], vec![]), sp(vec![], vec![])]));
    assert!(w.m().move_space("s2", -1));
    assert_eq!(w.st().spaces.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), ["s2", "s1", "s3"]);
    assert!(!w.m().move_space("s2", -1));
    assert!(!w.m().move_space("s3", 1));
    assert_eq!(w.st().active_space_id, "s1");

    let w = world(spec().spaces(vec![sp(vec![], vec![]), sp(vec![], vec![]), sp(vec![], vec![])]).active("s3"));
    assert_eq!(space_cycle_target(w.st(), 1), "s1");
    assert_eq!(space_cycle_target(w.st(), -1), "s2");
    let w = world(spec().spaces(vec![sp(vec![], vec![]), sp(vec![], vec![])]).active("s2"));
    assert_eq!(space_step_target(w.st(), 1), None);
    assert_eq!(space_step_target(w.st(), -1).as_deref(), Some("s1"));
    let w = world(spec().spaces(vec![sp(vec![], vec![]), sp(vec![], vec![])]));
    assert_eq!(space_number_target(w.st(), 2).as_deref(), Some("s2"));
    assert_eq!(space_number_target(w.st(), 3), None);
}

fn delete_world() -> World {
    let mut w = world(spec().tabs(&["a", "b", "x"]).spaces(vec![sp(vec![], vec![tab("a"), tab("b")]).active("a"), sp(vec![], vec![tab("x")])]));
    w.data.state.settings.routes = vec![
        SpaceRoute { id: "r1".into(), pattern: "a.com".into(), space_id: "s1".into() },
        SpaceRoute { id: "r2".into(), pattern: "b.com".into(), space_id: "s2".into() },
    ];
    w
}

#[test]
fn delete_space_rules() {
    assert!(world(spec()).m().delete_space("s1").is_none());
    assert!(delete_world().m().delete_space("zz").is_none());

    let mut w = delete_world();
    let removed = w.m().delete_space("s1").unwrap();
    assert_eq!(removed.tab_count, 2);
    assert_eq!(w.st().spaces.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), ["s2"]);
    assert_eq!(w.st().active_space_id, "s2");
    assert_eq!(w.st().tabs.keys().collect::<Vec<_>>(), ["x"]);
    assert_eq!(w.st().settings.routes.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), ["r2"]);
    assert_eq!(w.env.destroyed, ["a", "b"]);
    assert_eq!(w.data.closed.len(), 2);
    assert_eq!(w.data.closed[0].batch, w.data.closed[1].batch);

    let mut w = delete_world();
    w.m().delete_space("s2");
    assert_eq!(w.st().active_space_id, "s1");
}

#[test]
fn delete_space_undo() {
    let mut w = delete_world();
    let removed = w.m().delete_space("s1").unwrap();
    assert!(w.m().restore_space(&removed.undo));
    assert_eq!(w.st().spaces.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), ["s1", "s2"]);
    assert_eq!(w.st().active_space_id, "s1");
    let mut keys: Vec<_> = w.st().tabs.keys().cloned().collect();
    keys.sort();
    assert_eq!(keys, ["a", "b", "x"]);
    let mut routes: Vec<_> = w.st().settings.routes.iter().map(|r| r.id.clone()).collect();
    routes.sort();
    assert_eq!(routes, ["r1", "r2"]);
    assert!(w.data.closed.is_empty());
    assert_eq!(shape(&w.space("s1").today), ["a", "b"]);
    assert!(!w.m().restore_space(&removed.undo));
    assert_eq!(w.st().spaces.len(), 2);
}

#[test]
fn chrome_modes() {
    let mut w = world(spec());
    assert!(!is_horizontal(w.st()));
    assert!(is_sidebar_on(w.st()));
    w.data.state.settings.tab_layout = Some(crate::types::TabLayout::Horizontal);
    assert!(is_horizontal(w.st()) && !is_sidebar_on(w.st()));
    w.data.state.settings.sidebar_side = Some(crate::types::SidebarSide::Right);
    assert!(!is_sidebar_right(w.st()));
    w.data.state.settings.tab_layout = None;
    assert!(is_sidebar_right(w.st()));
    w.data.state.settings.compact_mode = true;
    assert!(!is_sidebar_on(w.st()));
}

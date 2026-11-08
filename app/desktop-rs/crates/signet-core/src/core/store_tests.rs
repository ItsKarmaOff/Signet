use super::*;
use crate::model::types::ARCHIVE_MAX;
use crate::types::Item;
use serde_json::json;
use std::fs;

struct Env {
    dir: tempfile::TempDir,
}

fn env() -> Env {
    Env { dir: tempfile::tempdir().unwrap() }
}

impl Env {
    fn file(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }
    fn write(&self, name: &str, data: Value) {
        let text = match data {
            Value::String(s) => s,
            other => other.to_string(),
        };
        fs::write(self.file(name), text).unwrap();
    }
    fn write_raw(&self, name: &str, text: &str) {
        fs::write(self.file(name), text).unwrap();
    }
    fn read(&self, name: &str) -> Value {
        serde_json::from_str(&fs::read_to_string(self.file(name)).unwrap()).unwrap()
    }
    fn open(&self) -> Store {
        Store::open(self.dir.path(), StoreOptions { system_languages: vec!["en-US".into()], startup_state: None })
    }
    fn files(&self, prefix: &str) -> Vec<String> {
        fs::read_dir(self.dir.path()).unwrap().filter_map(Result::ok).map(|e| e.file_name().to_string_lossy().to_string()).filter(|f| f.starts_with(prefix)).collect()
    }
}

fn tab_rec(id: &str) -> Value {
    json!({"id": id, "url": "https://example.com", "title": format!("T {id}")})
}

fn tab_item(id: &str) -> Item {
    Item::tab(id)
}

// ---- introduction

#[test]
fn intro_fresh_state() {
    assert_eq!(env().open().state.settings.onboarded, Some(false));
}

#[test]
fn intro_existing_without_field() {
    let e = env();
    e.write("state.json", json!({"version": 1, "settings": {"adblock": true}}));
    assert_eq!(e.open().state.settings.onboarded, Some(true));
}

#[test]
fn intro_pending_kept() {
    let e = env();
    e.write("state.json", json!({"version": 1, "settings": {"onboarded": false}}));
    assert_eq!(e.open().state.settings.onboarded, Some(false));
}

#[test]
fn intro_corrupt_file() {
    let e = env();
    e.write_raw("state.json", "{pas du json");
    assert_eq!(e.open().state.settings.onboarded, Some(true));
}

// ---- startup

#[test]
fn startup_without_file() {
    let e = env();
    let s = e.open();
    assert_eq!(s.state.version, 1);
    assert_eq!(s.state.spaces.len(), 1);
    assert_eq!(s.state.active_space_id, s.state.spaces[0].id);
    let sp = &s.state.spaces[0];
    assert_eq!((sp.name.as_str(), sp.icon.as_str()), ("Personal", "\u{1f3e0}"));
    assert!(sp.pinned.is_empty() && sp.today.is_empty());
    assert_eq!(s.state.sidebar_width, 240.0);
    assert_eq!(s.state.settings.archive_after_hours, 12.0);
    assert!(s.state.settings.adblock);
    assert_eq!(s.state.settings.app_icon, "aurore");
    assert_eq!(s.state.settings.languages, ["en-US", "en"]);
    assert!(e.files("state.json.corrupt").is_empty());
}

#[test]
fn startup_corrupt_json_copied_aside() {
    let e = env();
    e.write_raw("state.json", "{pas du json");
    let s = e.open();
    assert_eq!(s.state.spaces.len(), 1);
    let copies = e.files("state.json.corrupt-");
    assert_eq!(copies.len(), 1);
    assert_eq!(fs::read_to_string(e.file(&copies[0])).unwrap(), "{pas du json");
}

#[test]
fn startup_valid_json_not_object() {
    let e = env();
    e.write_raw("state.json", "null");
    assert_eq!(e.open().state.spaces.len(), 1);
    assert_eq!(e.files("state.json.corrupt-").len(), 1);
}

#[test]
fn startup_loads_valid_state() {
    let e = env();
    e.write(
        "state.json",
        json!({
            "version": 1,
            "tabs": {"a": tab_rec("a"), "b": {"id":"b","url":"https://b.org","title":"T b"}},
            "favorites": ["a"],
            "spaces": [{"id":"s1","name":"Work","icon":"\u{1f4bc}","hue":10,"pinned":[{"kind":"tab","tabId":"a"}],"today":[{"kind":"tab","tabId":"b"}],"activeTabId":"b"}],
            "activeSpaceId": "s1",
            "settings": {"archiveAfterHours": 3, "languages": ["fr-FR"]}
        }),
    );
    let s = e.open();
    assert_eq!(s.state.active_space_id, "s1");
    let mut keys: Vec<_> = s.state.tabs.keys().cloned().collect();
    keys.sort();
    assert_eq!(keys, ["a", "b"]);
    let b = &s.state.tabs["b"];
    assert_eq!((b.id.as_str(), b.url.as_str(), b.title.as_str(), b.loading, b.loaded), ("b", "https://b.org", "T b", false, false));
    assert_eq!(s.state.favorites, ["a"]);
    assert_eq!(s.state.settings.archive_after_hours, 3.0);
    assert_eq!(s.state.settings.languages, ["fr-FR"]);
    assert!(s.state.settings.adblock);
    assert_eq!(s.state.spaces[0].active_tab_id.as_deref(), Some("b"));
}

// ---- normalization

#[test]
fn unknown_tab_references_removed() {
    let e = env();
    e.write(
        "state.json",
        json!({
            "tabs": {"a": tab_rec("a")},
            "favorites": ["a", "ghost"],
            "spaces": [{"id":"s1","name":"S","icon":"x","hue":1,
                "pinned":[{"kind":"tab","tabId":"ghost"},{"kind":"split","id":"sp","tabIds":["a","ghost"]}],
                "today":[{"kind":"tab","tabId":"ghost"}],"activeTabId":"ghost"}],
            "activeSpaceId": "s1"
        }),
    );
    let s = e.open();
    let sp = &s.state.spaces[0];
    assert_eq!(s.state.favorites, ["a"]);
    assert_eq!(sp.pinned, vec![tab_item("a")]);
    assert!(sp.today.is_empty());
    assert_eq!(sp.active_tab_id, None);
}

#[test]
fn today_folders_flattened_pinned_kept() {
    let e = env();
    let folder = |id: &str| json!({"kind":"folder","id":id,"name":"F","open":true,"items":[{"kind":"tab","tabId":"a"}]});
    e.write("state.json", json!({"tabs":{"a":tab_rec("a")},"spaces":[{"id":"s1","name":"S","icon":"x","hue":1,"pinned":[folder("p")],"today":[folder("t")]}],"activeSpaceId":"s1"}));
    let s = e.open();
    assert_eq!(s.state.spaces[0].pinned[0].as_folder().unwrap().id, "p");
    assert_eq!(s.state.spaces[0].today, vec![tab_item("a")]);
}

#[test]
fn groups_pinned_stripped_today_synced() {
    let e = env();
    e.write(
        "state.json",
        json!({"tabs":{"a":tab_rec("a"),"b":tab_rec("b"),"c":tab_rec("c")},
            "spaces":[{"id":"s1","name":"S","icon":"x","hue":1,
              "pinned":[{"kind":"tab","tabId":"a","group":"g"}],
              "today":[{"kind":"tab","tabId":"b","group":"h"},{"kind":"tab","tabId":"c","group":"h"}]}],
            "activeSpaceId":"s1"}),
    );
    let s = e.open();
    let sp = &s.state.spaces[0];
    assert_eq!(sp.pinned, vec![tab_item("a")]);
    assert_eq!(sp.groups.as_ref().unwrap().keys().collect::<Vec<_>>(), ["h"]);
}

#[test]
fn unreadable_group_header_is_repaired() {
    let e = env();
    e.write(
        "state.json",
        json!({"tabs":{"a":{"id":"a","url":"","title":"Titre"},"b":tab_rec("b")},
            "spaces":[{"id":"s1","name":"S","icon":"x","hue":1,"pinned":[],
              "today":[{"kind":"tab","tabId":"a","group":"g"},{"kind":"tab","tabId":"b","group":"g"}],
              "groups":{"g":{"name":3}}}],"activeSpaceId":"s1"}),
    );
    let s = e.open();
    let g = &s.state.spaces[0].groups.as_ref().unwrap()["g"];
    assert_eq!((g.name.as_str(), g.open), ("Titre", true));
}

#[test]
fn bad_spaces_fall_back() {
    let e = env();
    e.write("state.json", json!({"tabs":{},"spaces":[],"activeSpaceId":"x"}));
    let a = e.open();
    assert_eq!(a.state.spaces.len(), 1);
    assert_eq!(a.state.active_space_id, a.state.spaces[0].id);
    e.write("state.json", json!({"tabs":{},"spaces":[null,3,{"id":"s2","name":"N","icon":"i","hue":2}],"activeSpaceId":"zzz"}));
    let b = e.open();
    assert_eq!(b.state.spaces.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), ["s2"]);
    assert_eq!(b.state.active_space_id, "s2");
    assert!(b.state.spaces[0].pinned.is_empty() && b.state.spaces[0].today.is_empty());
}

#[test]
fn sidebar_width_bounded() {
    let e = env();
    e.write("state.json", json!({"sidebarWidth": 5000}));
    assert_eq!(e.open().state.sidebar_width, SIDEBAR_MAX);
    e.write("state.json", json!({"sidebarWidth": 10}));
    assert_eq!(e.open().state.sidebar_width, SIDEBAR_MIN);
    e.write("state.json", json!({"sidebarWidth": "abc"}));
    assert_eq!(e.open().state.sidebar_width, 240.0);
}

#[test]
fn downloads_and_archive_capped() {
    let e = env();
    let mut downloads: Vec<Value> = (0..DOWNLOADS_MAX + 5)
        .map(|i| json!({"id": i.to_string(), "status": if i == 0 { "progressing" } else if i == 1 { "paused" } else { "completed" }}))
        .collect();
    downloads.push(json!("x"));
    let archive: Vec<Value> = (0..ARCHIVE_MAX + 10).map(|i| json!({"i": i})).collect();
    e.write("state.json", json!({"downloads": downloads, "archive": archive}));
    let s = e.open();
    assert_eq!(s.state.downloads.len(), DOWNLOADS_MAX);
    let st: Vec<_> = s.state.downloads.iter().take(3).map(|d| d.status).collect();
    assert_eq!(st, [DownloadStatus::Interrupted, DownloadStatus::Interrupted, DownloadStatus::Completed]);
    assert_eq!(s.state.archive.len(), ARCHIVE_MAX);
    e.write("state.json", json!({"downloads": "oops", "archive": "oops"}));
    let t = e.open();
    assert!(t.state.downloads.is_empty() && t.state.archive.is_empty());
}

#[test]
fn settings_repairs() {
    let e = env();
    e.write(
        "state.json",
        json!({"settings": {"appIcon":"inconnue","routes":"x","archiveAfterHours":-5,"tabSleepMinutes":1.5,"languages":[]},
            "pipBounds": {"x":1,"y":2,"width":null,"height":4}}),
    );
    let s = e.open();
    assert_eq!(s.state.settings.app_icon, "aurore");
    assert!(s.state.settings.routes.is_empty());
    assert_eq!(s.state.settings.archive_after_hours, 12.0);
    assert_eq!(s.state.settings.tab_sleep_minutes, None);
    assert_eq!(s.state.settings.languages, ["en-US", "en"]);
    assert_eq!(s.state.pip_bounds, None);
    e.write("state.json", json!({"pipBounds": {"x":1,"y":2,"width":3,"height":4}}));
    assert_eq!(e.open().state.pip_bounds, Some(Rect::new(1.0, 2.0, 3.0, 4.0)));
}

#[test]
fn site_zoom_cleaned() {
    let e = env();
    e.write("state.json", json!({"siteZoom": {"a.com": 2, "b.com": "big", "c.com": 99}}));
    let z = e.open().state.site_zoom.clone();
    assert_eq!(z.keys().collect::<Vec<_>>(), ["a.com"]);
    e.write("state.json", json!({}));
    assert!(e.open().state.site_zoom.is_empty());
    e.write("state.json", json!({"siteZoomBySpace": {"s1": {"a.com": 2, "b.com": "big"}, "s2": {}, "s3": 5}}));
    let mut store = e.open();
    assert_eq!(store.state.site_zoom_by_space.keys().collect::<Vec<_>>(), ["s1"]);
    store.state.site_zoom_by_space.insert("s9".into(), [("z.com".to_string(), 1.0)].into_iter().collect());
    let v = serde_json::to_value(&store.state).unwrap();
    assert_eq!(v["siteZoomBySpace"], json!({"s1": {"a.com": 2.0}, "s9": {"z.com": 1.0}}));
}

#[test]
fn tabs_keep_known_fields_only() {
    let e = env();
    e.write(
        "state.json",
        json!({"tabs": {"a": {"id":"a","url":"https://example.com","title":"T a","loading":true,"audible":true,"muted":1,
            "customTitle":"C","customIcon":"\u{1f525}","pinnedUrl":"https://p.com","lastActive":42,"inconnu":1}, "bad": "x"}}),
    );
    let s = e.open();
    assert_eq!(s.state.tabs.keys().collect::<Vec<_>>(), ["a"]);
    let t = &s.state.tabs["a"];
    assert!(!t.loading && !t.audible && t.muted);
    assert_eq!((t.custom_title.as_deref(), t.custom_icon.as_deref(), t.pinned_url.as_deref(), t.last_active), (Some("C"), Some("\u{1f525}"), Some("https://p.com"), 42));
}

#[test]
fn tab_history_removed_from_state() {
    let e = env();
    e.write("state.json", json!({"tabs":{"a":tab_rec("a")},"tabHistory":{"a":{"entries":[{"url":"https://x.com","title":""}],"index":0}}}));
    let mut s = e.open();
    s.flush();
    assert!(e.read("state.json").get("tabHistory").is_none());
}

// ---- bookmarks

fn link(id: &str) -> Value {
    json!({"kind":"link","id":id,"url":"https://x.com","title":id})
}
fn with_space(extra: Value) -> Value {
    let mut sp = json!({"id":"s1","name":"S","icon":"x","hue":1,"pinned":[],"today":[]});
    if let (Some(o), Some(x)) = (sp.as_object_mut(), extra.as_object()) {
        for (k, v) in x {
            o.insert(k.clone(), v.clone());
        }
    }
    json!({"tabs":{},"spaces":[sp],"activeSpaceId":"s1"})
}

#[test]
fn bookmarks_migration() {
    let e = env();
    e.write("state.json", with_space(json!({"bookmarks":[link("b1")]})));
    let s = e.open();
    assert_eq!(s.state.spaces[0].bookmarks.as_ref().unwrap().len(), 1);
    assert_eq!(e.read("bookmarks.json"), json!({"s1": [link("b1")]}));
}

#[test]
fn bookmarks_file_is_authoritative() {
    let e = env();
    e.write("state.json", with_space(json!({"bookmarks":[link("old")]})));
    e.write("bookmarks.json", json!({"s1": [link("new")]}));
    let s = e.open();
    assert_eq!(serde_json::to_value(s.state.spaces[0].bookmarks.as_ref().unwrap()).unwrap(), json!([link("new")]));
}

#[test]
fn bookmarks_cleaned() {
    let e = env();
    e.write("state.json", with_space(json!({})));
    e.write("bookmarks.json", json!({"s1": [link("ok"), {"kind":"link","id":"x"}, 5, {"kind":"folder","name":"","items":[link("in")]}]}));
    let s = e.open();
    let b = s.state.spaces[0].bookmarks.as_ref().unwrap();
    assert_eq!(b.len(), 2);
    match &b[1] {
        Bookmark::Folder { name, items, .. } => {
            assert_eq!(name, "Folder");
            assert_eq!(items.len(), 1);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn bookmarks_corrupt_file() {
    let e = env();
    e.write("state.json", with_space(json!({"bookmarks":[link("b1")]})));
    e.write_raw("bookmarks.json", "{{");
    let s = e.open();
    assert_eq!(e.files("bookmarks.json.corrupt-").len(), 1);
    assert_eq!(s.state.spaces[0].bookmarks.as_ref().unwrap().len(), 1);
    assert_eq!(e.read("bookmarks.json"), json!({"s1": [link("b1")]}));
}

#[test]
fn bookmarks_orphans_kept() {
    let e = env();
    e.write("state.json", with_space(json!({})));
    e.write("bookmarks.json", json!({"s1": [], "lost": [link("keep")]}));
    let mut s = e.open();
    assert_eq!(e.read("bookmarks.orphans.json"), json!({"lost": [link("keep")]}));
    s.flush();
    assert_eq!(e.read("bookmarks.json")["lost"], json!([link("keep")]));
}

// ---- writing

#[test]
fn flush_writes_clean_state() {
    let e = env();
    e.write(
        "state.json",
        json!({"tabs":{"a":tab_rec("a"),"b":tab_rec("b")},
            "spaces":[{"id":"s1","name":"S","icon":"x","hue":1,"pinned":[],"today":[{"kind":"tab","tabId":"a"}],
                "bookmarks":[{"kind":"link","id":"l","url":"https://x.com","title":"t"}]}],
            "activeSpaceId":"s1"}),
    );
    let mut s = e.open();
    s.detached.insert("b".into());
    s.state.downloads.push(DownloadInfo { id: "d1".into(), private: Some(true), ..Default::default() });
    s.state.downloads.push(DownloadInfo { id: "d2".into(), ..Default::default() });
    s.flush();
    let saved = e.read("state.json");
    assert_eq!(saved["tabs"].as_object().unwrap().keys().collect::<Vec<_>>(), ["a"]);
    assert!(saved["tabs"]["a"].get("loading").is_none());
    assert!(saved["spaces"][0].get("bookmarks").is_none());
    assert_eq!(saved["downloads"].as_array().unwrap().iter().map(|d| d["id"].as_str().unwrap()).collect::<Vec<_>>(), ["d2"]);
    assert_eq!(e.read("bookmarks.json")["s1"].as_array().unwrap().len(), 1);
}

#[test]
fn round_trip() {
    let e = env();
    let mut s = e.open();
    let tab = new_tab("https://rt.com", "RT");
    let id = tab.id.clone();
    s.state.tabs.insert(id.clone(), tab);
    s.state.spaces[0].today.push(tab_item(&id));
    s.state.spaces[0].active_tab_id = Some(id.clone());
    s.flush();
    let again = e.open();
    assert_eq!(again.state.spaces[0].today, vec![tab_item(&id)]);
    assert_eq!((again.state.tabs[&id].url.as_str(), again.state.tabs[&id].title.as_str()), ("https://rt.com", "RT"));
    assert_eq!(again.state.spaces[0].active_tab_id.as_deref(), Some(id.as_str()));
}

#[test]
fn schedule_writes_after_delay() {
    let e = env();
    let mut s = e.open();
    s.schedule(100);
    let at = s.next_deadline().unwrap();
    assert!(!s.due(at - 1));
    assert!(!s.tick(at - 1));
    assert!(!e.file("state.json").exists());
    assert!(s.due(at));
    assert!(s.tick(at));
    assert!(e.file("state.json").exists());
    assert_eq!(s.next_deadline(), None);
}

#[test]
fn earlier_deadline_is_kept() {
    let e = env();
    let mut s = e.open();
    s.schedule(100);
    let first = s.next_deadline().unwrap();
    s.schedule(60_000);
    assert_eq!(s.next_deadline(), Some(first));
}

// ---- per-tab histories

fn hist() -> Value {
    json!({"entries":[{"url":"https://a.com","title":"A"},{"url":"https://b.com","title":"B","pageState":"xyz"}],"index":1})
}

#[test]
fn history_migration() {
    let e = env();
    e.write("state.json", json!({"tabs":{"a":tab_rec("a")},"tabHistory":{"a":hist(),"ghost":hist()}}));
    let s = e.open();
    let h = &s.histories["a"];
    assert_eq!(h.index, 1);
    assert_eq!(h.entries[0], NavEntry { url: "https://a.com".into(), title: "A".into(), page_state: None });
    assert_eq!(h.entries[1].page_state.as_deref(), Some("xyz"));
    assert!(!s.histories.contains_key("ghost"));
    assert_eq!(e.read("tab-history.json").as_object().unwrap().keys().collect::<Vec<_>>(), ["a"]);
}

#[test]
fn history_file_takes_precedence() {
    let e = env();
    e.write("state.json", json!({"tabs":{"a":tab_rec("a"),"b":tab_rec("b")}}));
    e.write("tab-history.json", json!({"a": hist(), "b": {"entries":[{"url":""}],"index":0}}));
    let s = e.open();
    assert!(s.histories.contains_key("a"));
    assert!(!s.histories.contains_key("b"));
}

// ---- trim_history

fn mk(n: usize, state: &str) -> Vec<NavEntry> {
    (0..n).map(|i| NavEntry { url: format!("https://x.com/{i}"), title: i.to_string(), page_state: if state.is_empty() { None } else { Some(state.to_string()) } }).collect()
}

#[test]
fn trim_bounds() {
    assert!(trim_history(&[], 0, HISTORY_TAB_STATE_MAX).is_none());
    assert!(trim_history(&mk(3, ""), 3, HISTORY_TAB_STATE_MAX).is_none());
    assert!(trim_history(&mk(3, ""), -1, HISTORY_TAB_STATE_MAX).is_none());
    let t = trim_history(&mk(200, ""), 100, HISTORY_TAB_STATE_MAX).unwrap();
    assert_eq!(t.entries.len(), HISTORY_MAX_ENTRIES);
    assert_eq!(t.entries[t.index].url, "https://x.com/100");
    assert_eq!(t.entries[0].url, "https://x.com/75");
    assert_eq!(trim_history(&mk(200, ""), 0, HISTORY_TAB_STATE_MAX).unwrap().index, 0);
    let end = trim_history(&mk(200, ""), 199, HISTORY_TAB_STATE_MAX).unwrap();
    assert_eq!((end.entries.len(), end.index), (50, 49));
}

#[test]
fn trim_budget_outward() {
    let t = trim_history(&mk(5, &"x".repeat(100)), 2, 250).unwrap();
    assert_eq!(t.entries.iter().filter(|e| e.page_state.is_some()).count(), 2);
    assert!(t.entries[2].page_state.is_some());
}

// ---- drafts and settings

#[test]
fn draft_translated_default_name() {
    let d = clean_draft(&SpaceDraft::default());
    assert_eq!((d.name.as_str(), d.icon.as_str(), d.hue), ("Personal", "\u{2728}", 0.0));
}

fn base() -> Settings {
    Settings { archive_after_hours: 12.0, languages: vec!["en".into()], ..Settings::default() }
}
fn patch(v: Value) -> SettingsPatch {
    v.as_object().unwrap().clone()
}

#[test]
fn clean_invalid_keeps_base() {
    let b = base();
    assert_eq!(clean_settings(&patch(json!({"archiveAfterHours": -1})), &b).archive_after_hours, 12.0);
    assert_eq!(clean_settings(&patch(json!({"archiveAfterHours": null})), &b).archive_after_hours, 12.0);
    assert_eq!(clean_settings(&patch(json!({"archiveAfterHours": 0})), &b).archive_after_hours, 0.0);
    assert_eq!(clean_settings(&patch(json!({"archiveAfterHours": 24})), &b).archive_after_hours, 24.0);
    assert_eq!(clean_settings(&patch(json!({"tabSleepMinutes": 2.5})), &b).tab_sleep_minutes, None);
    assert_eq!(clean_settings(&patch(json!({"tabSleepMinutes": 30})), &b).tab_sleep_minutes, Some(30));
}

#[test]
fn clean_gpc() {
    let b = base();
    let on = Settings { global_privacy_control: Some(true), ..base() };
    assert_eq!(clean_settings(&patch(json!({"globalPrivacyControl": true})), &b).global_privacy_control, Some(true));
    assert_eq!(clean_settings(&patch(json!({"globalPrivacyControl": false})), &on).global_privacy_control, None);
    assert_eq!(clean_settings(&patch(json!({"globalPrivacyControl": "yes"})), &on).global_privacy_control, Some(true));
    assert_eq!(clean_settings(&patch(json!({"globalPrivacyControl": 1})), &b).global_privacy_control, None);
    let mut target = on.clone();
    apply_cleaned(&mut target, &clean_settings(&patch(json!({"globalPrivacyControl": false})), &on));
    assert_eq!(target.global_privacy_control, None);
}

#[test]
fn clean_page_settings() {
    let b = base();
    let o = clean_settings(&patch(json!({"fontSize":20,"minimumFontSize":12,"defaultZoom":125})), &b);
    assert_eq!((o.font_size, o.minimum_font_size, o.default_zoom), (Some(20), Some(12), Some(125)));
    let o = clean_settings(&patch(json!({"fontSize":15,"minimumFontSize":7,"defaultZoom":101})), &b);
    assert_eq!((o.font_size, o.minimum_font_size, o.default_zoom), (None, None, None));
    assert_eq!(clean_settings(&patch(json!({"fontSize":"20","defaultZoom":null})), &b).font_size, None);
    let cur = Settings { font_size: Some(24), default_zoom: Some(150), ..base() };
    let o = clean_settings(&patch(json!({"fontSize":13,"defaultZoom":0})), &cur);
    assert_eq!((o.font_size, o.default_zoom), (Some(24), Some(150)));
    let mut stored = Settings { font_size: Some(13), ..base() };
    let stored_patch = patch(serde_json::to_value(&stored).unwrap());
    apply_cleaned(&mut stored, &clean_settings(&stored_patch, &base()));
    assert_eq!(stored.font_size, None);
}

#[test]
fn clean_search_engine() {
    let b = base();
    assert_eq!(clean_settings(&patch(json!({"searchEngine":"bing"})), &b).search_engine.as_deref(), Some("bing"));
    assert_eq!(clean_settings(&patch(json!({"searchEngine":"nope"})), &b).search_engine, None);
    let mut target = Settings { search_engine: Some("nope".into()), ..base() };
    let p = patch(serde_json::to_value(&target).unwrap());
    apply_cleaned(&mut target, &clean_settings(&p, &base()));
    assert_eq!(target.search_engine, None);
}

#[test]
fn clean_custom_engines() {
    let b = base();
    let docs = json!({"id":"custom-abc123","name":"Docs","keyword":"d","template":"https://docs.example/s?q=%s"});
    let with = clean_settings(
        &patch(json!({"searchEngines": [docs, {"id":"custom-dup999","name":"Docs","keyword":"d","template":"https://docs.example/s?q=%s"}, {"id":"x","name":"Docs","keyword":"d","template":"https://docs.example/s?q=%s"}], "searchEngine": "custom-abc123"})),
        &b,
    );
    assert_eq!(with.search_engines.as_ref().unwrap().len(), 1);
    assert_eq!(with.search_engine.as_deref(), Some("custom-abc123"));
    let deleted = clean_settings(&patch(json!({"searchEngines": []})), &with);
    assert_eq!(deleted.search_engines, None);
    assert_eq!(deleted.search_engine, None);
    let off = clean_settings(&patch(json!({"searchKeywords": {"youtube": {"disabled": true}, "bing": {"keyword": "bb"}}})), &b);
    assert_eq!(serde_json::to_value(off.search_keywords.as_ref().unwrap()).unwrap(), json!({"youtube":{"disabled":true},"bing":{"keyword":"bb"}}));
    assert_eq!(clean_settings(&patch(json!({"searchEngine":"youtube"})), &off).search_engine, None);
}

#[test]
fn clean_download_dir() {
    let b = base();
    let abs = if cfg!(windows) { "D:\\Fichiers" } else { "/Volumes/Disque/Fichiers" };
    assert_eq!(clean_settings(&patch(json!({"downloadDir": abs})), &b).download_dir.as_deref(), Some(abs));
    assert_eq!(clean_settings(&patch(json!({"downloadDir": "relatif/dossier"})), &b).download_dir, None);
    let with = Settings { download_dir: Some(abs.into()), ..base() };
    assert_eq!(clean_settings(&patch(json!({"downloadDir": ""})), &with).download_dir, None);
    assert_eq!(clean_settings(&patch(json!({})), &with).download_dir.as_deref(), Some(abs));
}

#[test]
fn clean_leaves_other_fields() {
    let b = base();
    assert_eq!(clean_settings(&patch(json!({"adblock": false})), &b), b);
}

#[test]
fn clean_dev_aliases() {
    let b = base();
    let out = clean_settings(
        &patch(json!({"devAliases": [{"name":" App ","target":"localhost:3000"},{"name":"app","target":"localhost:4000"},{"name":"bad name","target":"localhost:1"},{"name":"x","target":"https://evil.com"}]})),
        &b,
    )
    .dev_aliases
    .unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].name, "app");
    let with = Settings { dev_aliases: Some(vec![crate::types::DevAlias { name: "a".into(), target: "http://localhost:1".into() }]), ..base() };
    assert_eq!(clean_settings(&patch(json!({"devAliases": []})), &with).dev_aliases, None);
}

#[test]
fn clean_extension_ids() {
    let id = "a".repeat(32);
    let out = clean_settings(&patch(json!({"extensionOrder": [id, id, "court", 5, "q".repeat(32)], "pinnedExtensions": [id]})), &base());
    assert_eq!(out.extension_order, Some(vec![id.clone()]));
    assert_eq!(out.pinned_extensions, Some(vec![id]));
}

#[test]
fn apply_cleaned_copies_and_removes() {
    let mut target = Settings { archive_after_hours: 1.0, tab_sleep_minutes: Some(5), ..base() };
    let cleaned = Settings { archive_after_hours: 7.0, ..base() };
    apply_cleaned(&mut target, &cleaned);
    assert_eq!(target.archive_after_hours, 7.0);
    assert_eq!(target.tab_sleep_minutes, None);
    assert!(target.adblock);
}

// ---- real Electron state (skipped when absent)

#[test]
fn parses_real_electron_files_when_present() {
    let Some(base) = dirs::config_dir().map(|d| d.join("Signet")) else { return };
    let real = base.join("state.json");
    if !real.exists() {
        return;
    }
    let Ok(text) = fs::read_to_string(&real) else { return };
    let raw: Value = serde_json::from_str(&text).expect("state.json is valid JSON");
    let state = sanitize(&raw, &["en-US".to_string()]).expect("sanitize");
    // Every tab of the file survives the round trip through the typed state.
    assert_eq!(state.tabs.len(), raw["tabs"].as_object().map_or(0, |t| t.len()));
    assert!(!state.spaces.is_empty());
    // Strict typed parse of the whole file must also work (no repair needed on a file written by Electron).
    let strict: Result<State, _> = serde_json::from_value(raw.clone());
    assert!(strict.is_ok(), "{:?}", strict.err());
}

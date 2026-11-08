//! Ports `app/desktop/src/main/model/testkit.ts`: a minimal state, spied effects and a compact way to write lists.

use super::types::{ModelData, ModelEnv};
use super::Model;
use crate::core::store::State;
use crate::types::{FolderItem, Item, LiveFolder, Space, SplitItem, TabGroup, TabInfo, TabItem};

pub fn tab(id: &str) -> Item {
    Item::tab(id)
}
pub fn gtab(id: &str, group: &str) -> Item {
    Item::Tab(TabItem { tab_id: id.into(), group: Some(group.into()) })
}
pub fn folder(id: &str, items: Vec<Item>) -> Item {
    Item::Folder(FolderItem { id: id.into(), name: id.into(), open: true, items, ..Default::default() })
}
pub fn folder_with(id: &str, items: Vec<Item>, f: impl FnOnce(&mut FolderItem)) -> Item {
    let mut it = FolderItem { id: id.into(), name: id.into(), open: true, items, ..Default::default() };
    f(&mut it);
    Item::Folder(it)
}
pub fn live(url: &str) -> LiveFolder {
    LiveFolder { source: crate::types::LiveSource::Feed { url: url.into() }, refreshed_at: None, error: None, refreshing: None, seen: None }
}
pub fn split(id: &str, ids: &[&str]) -> Item {
    Item::Split(SplitItem { id: id.into(), tab_ids: ids.iter().map(|s| s.to_string()).collect(), ..Default::default() })
}
pub fn gsplit(id: &str, ids: &[&str], group: &str) -> Item {
    Item::Split(SplitItem { id: id.into(), tab_ids: ids.iter().map(|s| s.to_string()).collect(), group: Some(group.into()), ..Default::default() })
}
pub fn header(name: &str) -> TabGroup {
    TabGroup { name: name.into(), icon: None, hue: 10.0, open: true }
}

/// Compact list notation: `a`, `a^g` (group g), `[f: a b]` (folder), `{s: a b}` (split view).
pub fn shape(list: &[Item]) -> Vec<String> {
    list.iter()
        .map(|item| match item {
            Item::Tab(t) => match &t.group {
                Some(g) => format!("{}^{}", t.tab_id, g),
                None => t.tab_id.clone(),
            },
            Item::Split(s) => format!("{{{}: {}}}{}", s.id, s.tab_ids.join(" "), s.group.as_ref().map(|g| format!("^{g}")).unwrap_or_default()),
            Item::Folder(f) => format!("[{}: {}]", f.id, shape(&f.items).join(" ")),
        })
        .collect()
}

#[derive(Default)]
pub struct SpaceSpec {
    pub pinned: Vec<Item>,
    pub today: Vec<Item>,
    pub active: Option<String>,
    pub groups: Vec<(String, TabGroup)>,
}

pub fn sp(pinned: Vec<Item>, today: Vec<Item>) -> SpaceSpec {
    SpaceSpec { pinned, today, ..Default::default() }
}
impl SpaceSpec {
    pub fn active(mut self, id: &str) -> Self {
        self.active = Some(id.into());
        self
    }
    pub fn groups(mut self, g: &[(&str, TabGroup)]) -> Self {
        self.groups = g.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
        self
    }
}

#[derive(Default)]
pub struct Spec {
    pub tabs: Vec<String>,
    pub favorites: Vec<String>,
    pub spaces: Vec<SpaceSpec>,
    pub active: Option<String>,
}

pub fn spec() -> Spec {
    Spec::default()
}
impl Spec {
    pub fn tabs(mut self, ids: &[&str]) -> Self {
        self.tabs = ids.iter().map(|s| s.to_string()).collect();
        self
    }
    pub fn favorites(mut self, ids: &[&str]) -> Self {
        self.favorites = ids.iter().map(|s| s.to_string()).collect();
        self
    }
    pub fn spaces(mut self, s: Vec<SpaceSpec>) -> Self {
        self.spaces = s;
        self
    }
    pub fn active(mut self, id: &str) -> Self {
        self.active = Some(id.into());
        self
    }
}

/// Effects requested by the model, in order, with a deterministic clock and id generator.
#[derive(Default)]
pub struct TestEnv {
    pub seq: u32,
    pub now: i64,
    pub destroyed: Vec<String>,
    /// For each destroyed view: did the tab still exist in the state at that moment?
    pub destroyed_existed: Vec<bool>,
    pub saved: Vec<String>,
    pub reconciled: Vec<Vec<String>>,
    pub activated: Vec<(String, String)>,
}

impl ModelEnv for TestEnv {
    fn new_id(&mut self) -> String {
        self.seq += 1;
        format!("id{}", self.seq)
    }
    fn now(&mut self) -> i64 {
        self.now
    }
    fn default_folder_name(&mut self) -> String {
        "Folder".into()
    }
    fn default_space_name(&mut self) -> String {
        "Espace".into()
    }
    fn on_activated(&mut self, space: &Space, tab_id: &str) {
        self.activated.push((space.id.clone(), tab_id.to_string()));
    }
    fn destroy_view(&mut self, state: &mut State, tab_id: &str) {
        self.destroyed.push(tab_id.to_string());
        self.destroyed_existed.push(state.tabs.contains_key(tab_id));
        if let Some(t) = state.tabs.get_mut(tab_id) {
            t.loaded = false;
        }
    }
    fn save_history(&mut self, tab_id: &str) {
        self.saved.push(tab_id.to_string());
    }
    fn reconcile_sessions(&mut self, tab_ids: &[String]) {
        self.reconciled.push(tab_ids.to_vec());
    }
}

pub struct World {
    pub data: ModelData,
    pub env: TestEnv,
}

pub fn world(spec: Spec) -> World {
    let mut state = State::default();
    for id in &spec.tabs {
        let mut t = crate::factory::new_tab(&format!("https://{id}.test/"), id);
        t.id = id.clone();
        t.last_active = 0;
        state.tabs.insert(id.clone(), t);
    }
    // Favorites and tabs mentioned only by lists exist too (like `tabs: [...]` in the TS specs).
    state.favorites = spec.favorites.clone();
    let specs = if spec.spaces.is_empty() { vec![SpaceSpec::default()] } else { spec.spaces };
    for (i, s) in specs.into_iter().enumerate() {
        state.spaces.push(Space {
            id: format!("s{}", i + 1),
            name: format!("Espace {}", i + 1),
            icon: "x".into(),
            hue: 100.0,
            pinned: s.pinned,
            today: s.today,
            active_tab_id: s.active,
            groups: if s.groups.is_empty() { None } else { Some(s.groups.into_iter().collect()) },
            ..Default::default()
        });
    }
    state.active_space_id = spec.active.unwrap_or_else(|| state.spaces[0].id.clone());
    state.archive = Vec::new();
    state.settings.routes = Vec::new();
    state.settings.archive_after_hours = 12.0;
    World { data: ModelData { state, ..Default::default() }, env: TestEnv { now: 1_000_000, ..Default::default() } }
}

impl World {
    pub fn m(&mut self) -> Model<'_> {
        Model::new(&mut self.data, &mut self.env)
    }
    pub fn st(&self) -> &State {
        &self.data.state
    }
    pub fn space(&self, id: &str) -> &Space {
        self.data.state.spaces.iter().find(|s| s.id == id).expect("space")
    }
    pub fn s1(&self) -> &Space {
        self.space("s1")
    }
    pub fn space_mut(&mut self, id: &str) -> &mut Space {
        self.data.state.spaces.iter_mut().find(|s| s.id == id).expect("space")
    }
    pub fn tab(&mut self, id: &str) -> &mut TabInfo {
        self.data.state.tabs.get_mut(id).expect("tab")
    }
}

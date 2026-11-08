//! Tolerant reading of Arc's `StorableSidebar.json` (port of `importers/arc.ts`).
use std::collections::{HashMap, HashSet};

use regex::Regex;
use serde_json::{Map, Value};

use super::types::*;
use crate::util::{err, Result, Translate};

type Obj = Map<String, Value>;
const MAX_DEPTH: usize = 8;

/// Objects of an array that alternates identifier and object, indexed by `id` (insertion order kept).
fn index_by_id(list: Option<&Value>) -> (Vec<String>, HashMap<String, &Obj>) {
    let mut order = Vec::new();
    let mut map = HashMap::new();
    if let Some(arr) = list.and_then(|l| l.as_array()) {
        for v in arr {
            if let Some(o) = v.as_object()
                && let Some(id) = o.get("id").and_then(|i| i.as_str())
                    && map.insert(id.to_string(), o).is_none() {
                        order.push(id.to_string());
                    }
        }
    }
    (order, map)
}

fn is_emoji(s: &str) -> bool {
    use std::sync::OnceLock;
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\p{Extended_Pictographic}").expect("static regex")).is_match(s)
}

/// First text value that is an emoji (descending into objects).
fn emoji_of(v: &Value, depth: usize) -> Option<String> {
    match v {
        Value::String(s) => is_emoji(s).then(|| s.clone()),
        Value::Number(n) => {
            let c = n.as_u64().filter(|n| *n > 0x1f && *n <= 0x10ffff).and_then(|n| char::from_u32(n as u32))?;
            let s = c.to_string();
            is_emoji(&s).then_some(s)
        }
        Value::Object(o) if depth <= 3 => o.values().find_map(|x| emoji_of(x, depth + 1)),
        Value::Array(a) if depth <= 3 => a.iter().find_map(|x| emoji_of(x, depth + 1)),
        _ => None,
    }
}

fn root_id(container_ids: Option<&Value>, label: &str) -> Option<String> {
    let arr = container_ids?.as_array()?;
    let i = arr.iter().position(|v| v.as_str() == Some(label))?;
    arr.get(i + 1)?.as_str().map(String::from)
}

struct Ctx<'a> {
    items: HashMap<String, &'a Obj>,
    t: Translate<'a>,
}

impl<'a> Ctx<'a> {
    fn children(&self, item: &Obj) -> Vec<&'a Obj> {
        item.get("childrenIds")
            .and_then(|c| c.as_array())
            .map(|a| a.iter().filter_map(|id| id.as_str().and_then(|i| self.items.get(i).copied())).collect())
            .unwrap_or_default()
    }

    fn tab_of(&self, item: &Obj) -> Option<ImportedTab> {
        let tab = item.get("data")?.as_object()?.get("tab")?.as_object()?;
        let url = tab.get("savedURL")?.as_str().filter(|u| is_web_url(u))?.trim().to_string();
        let title = [item.get("title"), tab.get("savedTitle")]
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str())
            .find(|s| !s.trim().is_empty())
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| url.clone());
        Some(ImportedTab { url, title })
    }

    fn convert(&self, item: &Obj, depth: usize, seen: &mut HashSet<String>, folders: bool) -> Vec<ImportedNode> {
        let Some(id) = item.get("id").and_then(|i| i.as_str()) else { return vec![] };
        if seen.contains(id) || depth > MAX_DEPTH {
            return vec![];
        }
        seen.insert(id.to_string());
        let empty = Obj::new();
        let data = item.get("data").and_then(|d| d.as_object()).unwrap_or(&empty);
        let truthy = |k: &str| data.get(k).is_some_and(|v| !matches!(v, Value::Null | Value::Bool(false)));
        if truthy("tab") {
            return self.tab_of(item).map(ImportedNode::Tab).into_iter().collect();
        }
        if truthy("list") {
            let inner: Vec<ImportedNode> = self.children(item).into_iter().flat_map(|c| self.convert(c, depth + 1, seen, folders)).collect();
            if !folders {
                return inner;
            }
            let name = item.get("title").and_then(|t| t.as_str()).map(str::trim).filter(|s| !s.is_empty()).map(String::from).unwrap_or_else(|| (self.t)("defaults.folderFallback", &[]));
            return vec![ImportedNode::Folder { name, items: inner }];
        }
        if truthy("splitView") {
            let mut tabs = Vec::new();
            for c in self.children(item) {
                let cid = c.get("id").map(|v| v.as_str().map(String::from).unwrap_or_else(|| v.to_string())).unwrap_or_default();
                if seen.contains(&cid) {
                    continue;
                }
                seen.insert(cid);
                if let Some(t) = self.tab_of(c) {
                    tabs.push(t);
                }
            }
            return match tabs.len() {
                0 => vec![],
                1 => vec![ImportedNode::Tab(tabs.remove(0))],
                _ => {
                    let rest = tabs.split_off(3.min(tabs.len()));
                    let mut out = vec![ImportedNode::Split(tabs)];
                    out.extend(rest.into_iter().map(ImportedNode::Tab));
                    out
                }
            };
        }
        if truthy("itemContainer") {
            return self.children(item).into_iter().flat_map(|c| self.convert(c, depth + 1, seen, folders)).collect();
        }
        vec![]
    }

    fn tree(&self, id: Option<&str>, folders: bool) -> Vec<ImportedNode> {
        match id.and_then(|i| self.items.get(i)) {
            Some(root) => self.convert(root, 0, &mut HashSet::new(), folders),
            None => vec![],
        }
    }
}

pub fn parse_spaces_file(json: &Value, t: Translate) -> Result<SpacesImport> {
    let containers: Vec<&Value> = json.get("sidebar").and_then(|s| s.get("containers")).and_then(|c| c.as_array()).map(|a| a.iter().collect()).unwrap_or_default();
    let Some(useful) = containers.iter().filter_map(|c| c.as_object()).find(|c| c.get("spaces").is_some_and(|s| s.is_array()) && c.get("items").is_some_and(|i| i.is_array())) else {
        return err(t("import.error.format", &[]));
    };
    let (_, items) = index_by_id(useful.get("items"));
    let (space_order, spaces) = index_by_id(useful.get("spaces"));
    let ctx = Ctx { items, t };

    let mut out = Vec::new();
    for (n, key) in space_order.iter().enumerate() {
        let sp = spaces[key];
        let name = sp.get("title").and_then(|t| t.as_str()).map(str::trim).filter(|s| !s.is_empty()).map(String::from).unwrap_or_else(|| t("defaults.spaceNumbered", &[("number", (n + 1).to_string())]));
        let icon = sp.get("customInfo").and_then(|i| i.get("iconType")).and_then(|v| emoji_of(v, 0)).unwrap_or_else(|| "\u{2728}".into());
        out.push(ImportedSpace {
            name,
            icon,
            pinned: ctx.tree(root_id(sp.get("containerIDs"), "pinned").as_deref(), true),
            today: ctx.tree(root_id(sp.get("containerIDs"), "unpinned").as_deref(), false),
        });
    }

    let mut favorites: Vec<ImportedTab> = Vec::new();
    let mut seen_urls = HashSet::new();
    for c in &containers {
        let Some(tops) = c.as_object().and_then(|o| o.get("topAppsContainerIDs")).and_then(|t| t.as_array()) else { continue };
        for id in tops.iter().filter_map(|v| v.as_str()) {
            for node in ctx.tree(Some(id), false) {
                if let ImportedNode::Tab(tab) = node
                    && seen_urls.insert(tab.url.clone()) {
                        favorites.push(tab);
                    }
            }
        }
    }
    Ok(SpacesImport { spaces: out, favorites })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tr(k: &str, p: &[(&str, String)]) -> String {
        match k {
            "defaults.folderFallback" => "Folder".into(),
            "defaults.spaceNumbered" => format!("Space {}", p[0].1),
            _ => k.into(),
        }
    }

    fn tab_item(id: &str, url: &str, title: &str) -> Value {
        json!({"id": id, "title": title, "data": {"tab": {"savedURL": url, "savedTitle": format!("saved {id}")}}})
    }

    fn file(items: Vec<Value>, space: Value) -> Value {
        let mut sp = json!({"id":"sp1","title":"Travail","customInfo":{"iconType":{"emoji_v2":"\u{1F4BC}"}},"containerIDs":["pinned","rootP","unpinned","rootU"]});
        if let Some(o) = space.as_object() { for (k, v) in o { sp[k] = v.clone(); } }
        json!({"sidebar":{"containers":[{"global":{}},{"spaces":[sp],"items":items,"topAppsContainerIDs":[{"default":true},"rootTop"]}]}})
    }

    fn items() -> Vec<Value> {
        vec![
            json!({"id":"rootP","data":{"itemContainer":{}},"childrenIds":["t1","f1","sv"]}),
            tab_item("t1", "https://one.com", "Un"),
            json!({"id":"f1","title":"Folder","data":{"list":{}},"childrenIds":["t2","t3"]}),
            tab_item("t2", "https://two.com", ""),
            tab_item("t3", "javascript:alert(1)", ""),
            json!({"id":"sv","data":{"splitView":{}},"childrenIds":["s1","s2","s3","s4","s5"]}),
            tab_item("s1", "https://s1.com", ""), tab_item("s2", "https://s2.com", ""), tab_item("s3", "https://s3.com", ""),
            tab_item("s4", "https://s4.com", ""), tab_item("s5", "https://s5.com", ""),
            json!({"id":"rootU","data":{"itemContainer":{}},"childrenIds":["f2","t9"]}),
            json!({"id":"f2","title":"Jour","data":{"list":{}},"childrenIds":["t8"]}),
            tab_item("t8", "https://eight.com", ""), tab_item("t9", "https://nine.com", ""),
            json!({"id":"rootTop","data":{"itemContainer":{}},"childrenIds":["top1","top2","top3"]}),
            tab_item("top1", "https://fav.com", ""), tab_item("top2", "https://fav.com", ""), tab_item("top3", "https://fav2.com", ""),
        ]
    }

    #[test]
    fn format_errors() {
        assert!(parse_spaces_file(&json!({}), &tr).is_err());
        assert!(parse_spaces_file(&json!(null), &tr).is_err());
        assert!(parse_spaces_file(&json!({"sidebar":{"containers":[{"spaces":[]}]}}), &tr).is_err());
    }

    #[test]
    fn pinned_tree() {
        let r = parse_spaces_file(&file(items(), json!({})), &tr).unwrap();
        assert_eq!(r.spaces.len(), 1);
        assert_eq!((r.spaces[0].name.as_str(), r.spaces[0].icon.as_str()), ("Travail", "\u{1F4BC}"));
        let p = &r.spaces[0].pinned;
        assert_eq!(p[0], ImportedNode::tab("https://one.com", "Un"));
        assert_eq!(p[1], ImportedNode::Folder { name: "Folder".into(), items: vec![ImportedNode::tab("https://two.com", "saved t2")] });
        match &p[2] { ImportedNode::Split(t) => assert_eq!(t.iter().map(|x| x.url.as_str()).collect::<Vec<_>>(), vec!["https://s1.com", "https://s2.com", "https://s3.com"]), _ => panic!("split") }
        let rest: Vec<_> = p[3..].iter().map(|n| match n { ImportedNode::Tab(t) => t.url.clone(), _ => String::new() }).collect();
        assert_eq!(rest, vec!["https://s4.com", "https://s5.com"]);
    }

    #[test]
    fn today_flattened_and_favorites_dedup() {
        let r = parse_spaces_file(&file(items(), json!({})), &tr).unwrap();
        let today: Vec<_> = r.spaces[0].today.iter().map(|n| match n { ImportedNode::Tab(t) => t.url.clone(), _ => "x".into() }).collect();
        assert_eq!(today, vec!["https://eight.com", "https://nine.com"]);
        assert_eq!(r.favorites.iter().map(|f| f.url.as_str()).collect::<Vec<_>>(), vec!["https://fav.com", "https://fav2.com"]);
    }

    #[test]
    fn defaults_and_code_point_emoji() {
        let r = parse_spaces_file(&file(items(), json!({"title":"  ","customInfo":{}})), &tr).unwrap();
        assert_eq!((r.spaces[0].name.as_str(), r.spaces[0].icon.as_str()), ("Space 1", "\u{2728}"));
        let r = parse_spaces_file(&file(items(), json!({"customInfo":{"iconType":{"emoji":0x1f600}}})), &tr).unwrap();
        assert_eq!(r.spaces[0].icon, "\u{1F600}");
    }

    #[test]
    fn cycles_do_not_loop() {
        let lp = vec![
            json!({"id":"rootP","data":{"itemContainer":{}},"childrenIds":["rootP","ghost","f"]}),
            json!({"id":"f","data":{"list":{}},"childrenIds":["rootP","f"]}),
        ];
        let r = parse_spaces_file(&file(lp, json!({})), &tr).unwrap();
        assert_eq!(r.spaces[0].pinned, vec![ImportedNode::Folder { name: "Folder".into(), items: vec![] }]);
    }

    #[test]
    fn alternating_arrays() {
        let alt: Vec<Value> = items().into_iter().flat_map(|it| [json!(it["id"].clone()), it]).collect();
        let r = parse_spaces_file(&file(alt, json!({})), &tr).unwrap();
        assert_eq!(r.spaces[0].pinned.len(), 5);
    }
}

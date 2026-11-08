//! Favorites from Chrome, Brave, Edge (JSON) and Safari (plist) (port of `importers/bookmarks.ts`).
use regex::Regex;
use serde_json::{Map, Value};

use super::types::*;
use crate::util::{err, Result, Translate};

const MAX_DEPTH: usize = 20;

fn nonblank(v: Option<&Value>) -> Option<&str> {
    v.and_then(|x| x.as_str()).map(str::trim).filter(|s| !s.is_empty())
}

fn chrome_node(n: &Value, depth: usize, t: Translate) -> Vec<ImportedNode> {
    let Some(o) = n.as_object() else { return vec![] };
    if depth > MAX_DEPTH {
        return vec![];
    }
    let ty = o.get("type").and_then(|x| x.as_str());
    if ty == Some("url") {
        let Some(url) = o.get("url").and_then(|u| u.as_str()).filter(|u| is_web_url(u)) else { return vec![] };
        let url = url.trim();
        return vec![ImportedNode::tab(url, nonblank(o.get("name")).unwrap_or(url))];
    }
    if ty == Some("folder") || o.get("children").is_some_and(|c| c.is_array()) {
        let items = o.get("children").and_then(|c| c.as_array()).map(|a| a.iter().flat_map(|c| chrome_node(c, depth + 1, t)).collect()).unwrap_or_default();
        let name = nonblank(o.get("name")).map(String::from).unwrap_or_else(|| t("defaults.folderFallback", &[]));
        return vec![ImportedNode::Folder { name, items }];
    }
    vec![]
}

/// Content of the `Bookmarks` file as a folder named after the browser (`None` if nothing to import).
pub fn parse_chrome_bookmarks(json: &Value, browser_name: &str, t: Translate) -> Result<Option<ImportedNode>> {
    let Some(roots) = json.get("roots").and_then(|r| r.as_object()) else { return err(t("import.error.format", &[])) };
    let content = |key: &str| -> Vec<ImportedNode> {
        match roots.get(key).and_then(|r| r.get("children")).and_then(|c| c.as_array()) {
            Some(a) => prune_empty(a.iter().flat_map(|c| chrome_node(c, 0, t)).collect()),
            None => vec![],
        }
    };
    let mut items = content("bookmark_bar");
    for (key, name) in [("other", t("import.folder.otherBookmarks", &[])), ("synced", t("import.folder.syncedBookmarks", &[]))] {
        let sub = content(key);
        if !sub.is_empty() {
            items.push(ImportedNode::Folder { name, items: sub });
        }
    }
    Ok((!items.is_empty()).then(|| ImportedNode::Folder { name: browser_name.into(), items }))
}

fn safari_node(n: &Value, depth: usize, t: Translate) -> Vec<ImportedNode> {
    let Some(o) = n.as_object() else { return vec![] };
    if depth > MAX_DEPTH {
        return vec![];
    }
    let title = o.get("Title").and_then(|x| x.as_str()).unwrap_or("");
    match o.get("WebBookmarkType").and_then(|x| x.as_str()) {
        Some("WebBookmarkTypeLeaf") => {
            let Some(url) = o.get("URLString").and_then(|u| u.as_str()).filter(|u| is_web_url(u)) else { return vec![] };
            let url = url.trim();
            let dict_title = o.get("URIDictionary").and_then(|d| d.get("title")).and_then(|x| x.as_str()).map(str::trim).filter(|s| !s.is_empty());
            let title = dict_title.or(Some(title.trim()).filter(|s| !s.is_empty())).unwrap_or(url);
            vec![ImportedNode::tab(url, title)]
        }
        Some("WebBookmarkTypeList") => {
            if title == "com.apple.ReadingList" || title == "History" || o.get("WebBookmarkIdentifier").and_then(|x| x.as_str()) == Some("History") {
                return vec![];
            }
            let items = o.get("Children").and_then(|c| c.as_array()).map(|a| a.iter().flat_map(|c| safari_node(c, depth + 1, t)).collect()).unwrap_or_default();
            let name = if title.is_empty() { t("defaults.folderFallback", &[]) } else { title.to_string() };
            vec![ImportedNode::Folder { name, items }]
        }
        _ => vec![],
    }
}

/// Safari plist (already converted to JSON) as a "Safari" folder.
pub fn parse_safari_bookmarks(json: &Value, t: Translate) -> Result<Option<ImportedNode>> {
    let Some(children) = json.get("Children").and_then(|c| c.as_array()) else { return err(t("import.error.format", &[])) };
    let top = |title: &str| -> Vec<ImportedNode> {
        match children.iter().find(|c| c.get("Title").and_then(|x| x.as_str()) == Some(title)) {
            Some(list) => prune_empty(list.get("Children").and_then(|c| c.as_array()).map(|a| a.iter().flat_map(|c| safari_node(c, 0, t)).collect()).unwrap_or_default()),
            None => vec![],
        }
    };
    let mut items = top("BookmarksBar");
    let menu = top("BookmarksMenu");
    if !menu.is_empty() {
        items.push(ImportedNode::Folder { name: t("import.folder.bookmarksMenu", &[]), items: menu });
    }
    Ok((!items.is_empty()).then(|| ImportedNode::Folder { name: "Safari".into(), items }))
}

// ---------------------------------------------------------------- XML plist

struct Tok {
    close: bool,
    tag: String,
    self_close: bool,
    text: Option<String>,
}

fn decode(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

/// XML plist to a JSON value. Fallback when `plutil -convert json` rejects the file.
pub fn parse_xml_plist(xml: &str) -> Value {
    let re = Regex::new(r"<(/?)([A-Za-z]+)\s*(/?)>|([^<]+)").expect("static regex");
    let toks: Vec<Tok> = re
        .captures_iter(xml)
        .map(|c| Tok {
            close: c.get(1).is_some_and(|m| !m.as_str().is_empty()),
            tag: c.get(2).map(|m| m.as_str().to_string()).unwrap_or_default(),
            self_close: c.get(3).is_some_and(|m| !m.as_str().is_empty()),
            text: c.get(4).map(|m| decode(m.as_str())),
        })
        .collect();
    let mut pos = 0usize;
    value(&toks, &mut pos, 0)
}

fn value(toks: &[Tok], pos: &mut usize, depth: usize) -> Value {
    while *pos < toks.len() && (toks[*pos].text.is_some() || toks[*pos].tag == "plist") {
        *pos += 1;
    }
    let Some(t) = toks.get(*pos) else { return Value::Null };
    *pos += 1;
    if depth > 60 || t.close {
        return Value::Null;
    }
    match t.tag.as_str() {
        tag @ ("dict" | "array") => {
            let is_dict = tag == "dict";
            let mut obj = Map::new();
            let mut arr = Vec::new();
            if t.self_close {
                return if is_dict { Value::Object(obj) } else { Value::Array(arr) };
            }
            let mut key = String::new();
            while *pos < toks.len() {
                let n = &toks[*pos];
                if n.text.is_some() {
                    *pos += 1;
                    continue;
                }
                if n.close && n.tag == tag {
                    *pos += 1;
                    break;
                }
                if is_dict && n.tag == "key" {
                    *pos += 1;
                    key = match toks.get(*pos) { Some(Tok { text: Some(s), .. }) => { *pos += 1; s.clone() } _ => String::new() };
                    *pos += 1;
                    continue;
                }
                let v = value(toks, pos, depth + 1);
                if is_dict { obj.insert(key.clone(), v); } else { arr.push(v); }
            }
            if is_dict { Value::Object(obj) } else { Value::Array(arr) }
        }
        "true" => Value::Bool(true),
        "false" => Value::Bool(false),
        tag => {
            if t.self_close {
                return Value::String(String::new());
            }
            let content = match toks.get(*pos) { Some(Tok { text: Some(s), .. }) => { *pos += 1; s.clone() } _ => String::new() };
            if toks.get(*pos).is_some_and(|n| n.close && n.tag == tag) {
                *pos += 1;
            }
            if tag == "integer" || tag == "real" {
                return content.trim().parse::<f64>().ok().and_then(|f| if f.fract() == 0.0 && f.abs() < 9e15 { Some(Value::from(f as i64)) } else { serde_json::Number::from_f64(f).map(Value::Number) }).unwrap_or(Value::Null);
            }
            Value::String(content)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tr(k: &str, _: &[(&str, String)]) -> String {
        match k {
            "defaults.folderFallback" => "Folder".into(),
            "import.folder.otherBookmarks" => "Other Bookmarks".into(),
            "import.folder.bookmarksMenu" => "Bookmarks Menu".into(),
            _ => k.into(),
        }
    }
    fn url(n: &str, u: &str) -> Value { json!({"type":"url","name":n,"url":u}) }

    #[test]
    fn chrome_structure() {
        let j = json!({"roots":{
            "bookmark_bar":{"children":[url("A","https://a.com"),{"type":"folder","name":"Folder","children":[url("B","https://b.com"),url("JS","javascript:alert(1)")]},{"type":"folder","name":"Vide","children":[]},url("","https://noname.com")]},
            "other":{"children":[url("O","https://o.com")]},"synced":{"children":[]}}});
        let out = parse_chrome_bookmarks(&j, "Brave", &tr).unwrap().unwrap();
        let ImportedNode::Folder { name, items } = out else { panic!() };
        assert_eq!(name, "Brave");
        assert_eq!(items.len(), 4);
        assert_eq!(items[1], ImportedNode::Folder { name: "Folder".into(), items: vec![ImportedNode::tab("https://b.com", "B")] });
        assert_eq!(items[2], ImportedNode::tab("https://noname.com", "https://noname.com"));
        assert!(matches!(&items[3], ImportedNode::Folder { name, .. } if name == "Other Bookmarks"));
    }

    #[test]
    fn chrome_empty_error_and_depth() {
        assert_eq!(parse_chrome_bookmarks(&json!({"roots":{"bookmark_bar":{"children":[]}}}), "Chrome", &tr).unwrap(), None);
        assert!(parse_chrome_bookmarks(&json!({}), "Chrome", &tr).is_err());
        assert!(parse_chrome_bookmarks(&json!(null), "Chrome", &tr).is_err());
        let mut node = url("deep", "https://deep.com");
        for _ in 0..100 { node = json!({"type":"folder","name":"d","children":[node]}); }
        assert_eq!(parse_chrome_bookmarks(&json!({"roots":{"bookmark_bar":{"children":[node]}}}), "C", &tr).unwrap(), None);
    }

    #[test]
    fn safari() {
        let leaf = |t: &str, u: &str| json!({"WebBookmarkType":"WebBookmarkTypeLeaf","URLString":u,"URIDictionary":{"title":t}});
        let list = |t: &str, c: Vec<Value>| json!({"WebBookmarkType":"WebBookmarkTypeList","Title":t,"Children":c});
        let j = json!({"Children":[
            list("BookmarksBar", vec![leaf("A","https://a.com"), list("Sous", vec![leaf("B","https://b.com")]), list("com.apple.ReadingList", vec![leaf("R","https://r.com")])]),
            list("BookmarksMenu", vec![leaf("M","https://m.com")])]});
        let ImportedNode::Folder { name, items } = parse_safari_bookmarks(&j, &tr).unwrap().unwrap() else { panic!() };
        assert_eq!(name, "Safari");
        assert_eq!(items.len(), 3);
        assert!(matches!(&items[1], ImportedNode::Folder { name, .. } if name == "Sous"));
        assert!(matches!(&items[2], ImportedNode::Folder { name, items } if name == "Bookmarks Menu" && items == &vec![ImportedNode::tab("https://m.com", "M")]));
        assert!(parse_safari_bookmarks(&json!({}), &tr).is_err());
        assert_eq!(parse_safari_bookmarks(&json!({"Children":[]}), &tr).unwrap(), None);
    }

    #[test]
    fn xml_plist() {
        let plist = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>Title</key><string>A &amp; B</string>
  <key>Count</key><integer>3</integer>
  <key>Ratio</key><real>1.5</real>
  <key>On</key><true/>
  <key>Off</key><false/>
  <key>Empty</key><string/>
  <key>Children</key><array><dict><key>URLString</key><string>https://a.com</string></dict><array/></array>
  <key>Nested</key><dict/>
</dict></plist>"#;
        assert_eq!(parse_xml_plist(plist), json!({"Title":"A & B","Count":3,"Ratio":1.5,"On":true,"Off":false,"Empty":"","Children":[{"URLString":"https://a.com"},[]],"Nested":{}}));
        assert_eq!(parse_xml_plist(""), Value::Null);
    }
}

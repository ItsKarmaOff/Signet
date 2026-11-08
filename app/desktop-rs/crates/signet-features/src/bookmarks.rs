//! Bookmarks (port of `main/features/bookmarks.ts` and `packages/shared/src/mobile/bookmarks-html.ts`):
//! tree operations on one space's bar, cleaning of what is read from disk, Netscape HTML import and export.
//! Persistence (`bookmarks.json`, `{ [spaceId]: Bookmark[] }`) belongs to the store in signet-core.
use regex::Regex;
use serde_json::Value;
use std::sync::OnceLock;

use crate::util::{clip, Translate};

pub const BOOKMARK_BAR: u32 = 32;
pub const MAX_DEPTH: usize = 20;
/// Nesting limit of imported files (`MAX_BOOKMARK_DEPTH` in the shared mobile code).
pub const MAX_BOOKMARK_DEPTH: usize = 6;

pub use signet_core::types::{Bookmark, BookmarkTarget};

/// New link bookmark (fresh id).
pub fn link(url: &str, title: &str, favicon: Option<&str>) -> Bookmark {
    Bookmark::Link { id: new_id(), title: title.into(), url: url.into(), favicon: favicon.map(String::from) }
}

/// New folder bookmark (fresh id).
pub fn folder(name: &str, items: Vec<Bookmark>) -> Bookmark {
    Bookmark::Folder { id: new_id(), name: name.into(), items }
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn norm(url: &str) -> &str {
    url.strip_suffix('/').unwrap_or(url)
}

/// Cleans bookmarks read from disk (shape, depth, lengths).
pub fn prune_bookmarks(items: &Value, depth: usize, t: Translate) -> Vec<Bookmark> {
    let Some(arr) = items.as_array().filter(|_| depth <= MAX_DEPTH) else { return vec![] };
    arr.iter()
        .filter_map(|i| {
            let o = i.as_object()?;
            let id = o.get("id").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(String::from).unwrap_or_else(new_id);
            match o.get("kind").and_then(|k| k.as_str()) {
                Some("link") => {
                    let url = o.get("url").and_then(|v| v.as_str()).filter(|s| !s.is_empty())?;
                    Some(Bookmark::Link {
                        id,
                        url: url.into(),
                        title: o.get("title").and_then(|v| v.as_str()).map(|s| clip(s, 300)).unwrap_or_else(|| url.into()),
                        favicon: o.get("favicon").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(String::from),
                    })
                }
                Some("folder") => Some(Bookmark::Folder {
                    id,
                    name: o.get("name").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(|s| clip(s, 120)).unwrap_or_else(|| t("defaults.folderFallback", &[])),
                    items: prune_bookmarks(o.get("items").unwrap_or(&Value::Null), depth + 1, t),
                }),
                _ => None,
            }
        })
        .collect()
}

/// True if the address can become a bookmark (not a blank page or a data address).
pub fn bookmarkable(url: &str) -> bool {
    let l = url.to_ascii_lowercase();
    ["http:", "https:", "file:", "signet:"].iter().any(|p| l.starts_with(p))
}

/// Path of indexes to a bookmark.
type Path = Vec<usize>;

fn find_path(list: &[Bookmark], pred: &dyn Fn(&Bookmark) -> bool) -> Option<Path> {
    for (i, item) in list.iter().enumerate() {
        if pred(item) {
            return Some(vec![i]);
        }
        if let Bookmark::Folder { items, .. } = item
            && let Some(mut p) = find_path(items, pred) {
                p.insert(0, i);
                return Some(p);
            }
    }
    None
}

fn list_at<'a>(root: &'a mut Vec<Bookmark>, parent: &[usize]) -> Option<&'a mut Vec<Bookmark>> {
    let mut cur = root;
    for i in parent {
        match cur.get_mut(*i)? {
            Bookmark::Folder { items, .. } => cur = items,
            _ => return None,
        }
    }
    Some(cur)
}

fn get<'a>(root: &'a [Bookmark], path: &[usize]) -> Option<&'a Bookmark> {
    let mut cur = root;
    let mut node = None;
    for i in path {
        let n = cur.get(*i)?;
        node = Some(n);
        if let Bookmark::Folder { items, .. } = n { cur = items } else { cur = &[] }
    }
    node
}

/// True if `id` designates `folder` itself or one of its subfolders.
fn contains(folder: &Bookmark, id: &str) -> bool {
    match folder {
        Bookmark::Folder { id: fid, items, .. } => fid == id || items.iter().any(|i| contains(i, id)),
        _ => false,
    }
}

pub fn find<'a>(root: &'a [Bookmark], id: &str) -> Option<&'a Bookmark> {
    find_path(root, &|b| b.id() == id).and_then(|p| get(root, &p))
}

/// Bookmark for this address, wherever it is filed.
pub fn find_url<'a>(root: &'a [Bookmark], url: &str) -> Option<&'a Bookmark> {
    if url.is_empty() {
        return None;
    }
    let n = norm(url);
    find_path(root, &|b| matches!(b, Bookmark::Link { url, .. } if norm(url) == n)).and_then(|p| get(root, &p))
}

/// What a tab gives to a bookmark.
pub struct TabInfo<'a> {
    pub url: &'a str,
    pub title: &'a str,
    pub custom_title: Option<&'a str>,
    pub favicon: Option<&'a str>,
}

fn link_for(tab: &TabInfo) -> Option<Bookmark> {
    if !bookmarkable(tab.url) {
        return None;
    }
    let title = tab.custom_title.filter(|s| !s.is_empty()).or(Some(tab.title).filter(|s| !s.is_empty())).unwrap_or(tab.url);
    Some(link(tab.url, title, tab.favicon.filter(|f| !f.is_empty())))
}

fn target_list<'a>(root: &'a mut Vec<Bookmark>, folder_id: Option<&str>) -> Option<&'a mut Vec<Bookmark>> {
    let Some(fid) = folder_id else { return Some(root) };
    let p = find_path(root, &|b| b.id() == fid)?;
    list_at(root, &p)
}

/// Star and cmd+D: adds the page at the end of the bar, or removes its bookmark. Returns true if changed.
pub fn toggle(root: &mut Vec<Bookmark>, tab: &TabInfo) -> bool {
    if let Some(p) = find_path(root, &|b| matches!(b, Bookmark::Link { url, .. } if norm(url) == norm(tab.url))).filter(|_| !tab.url.is_empty()) {
        let (last, parent) = p.split_last().expect("non-empty path");
        if let Some(l) = list_at(root, parent) {
            l.remove(*last);
            return true;
        }
        return false;
    }
    match link_for(tab) {
        Some(l) => {
            root.push(l);
            true
        }
        None => false,
    }
}

/// Tab dropped on the bar (or into a folder).
pub fn add(root: &mut Vec<Bookmark>, tab: &TabInfo, target: &BookmarkTarget) -> bool {
    let Some(link) = link_for(tab) else { return false };
    let Some(list) = target_list(root, target.folder_id.as_deref()) else { return false };
    let at = target.index.min(list.len());
    list.insert(at, link);
    true
}

/// Moves a bookmark. Never a folder into itself or one of its subfolders.
pub fn move_to(root: &mut Vec<Bookmark>, id: &str, target: &BookmarkTarget) -> bool {
    let Some(from) = find_path(root, &|b| b.id() == id) else { return false };
    if target_list_exists(root, target.folder_id.as_deref()).is_none() {
        return false;
    }
    if let (Some(node), Some(fid)) = (get(root, &from), &target.folder_id)
        && contains(node, fid) {
            return false;
        }
    let (idx, parent) = from.split_last().expect("non-empty path");
    let parent = parent.to_vec();
    let same_list = match &target.folder_id {
        None => parent.is_empty(),
        Some(fid) => find_path(root, &|b| b.id() == fid.as_str()).is_some_and(|p| p == parent),
    };
    let Some(src) = list_at(root, &parent) else { return false };
    let item = src.remove(*idx);
    let mut index = target.index;
    if same_list && *idx < index {
        index -= 1;
    }
    let Some(dst) = target_list(root, target.folder_id.as_deref()) else { return false };
    let at = index.min(dst.len());
    dst.insert(at, item);
    true
}

fn target_list_exists(root: &[Bookmark], folder_id: Option<&str>) -> Option<()> {
    match folder_id {
        None => Some(()),
        Some(fid) => find_path(root, &|b| b.id() == fid && matches!(b, Bookmark::Folder { .. })).map(|_| ()),
    }
}

pub fn rename(root: &mut Vec<Bookmark>, id: &str, name: &str) -> bool {
    let clean = name.trim();
    let Some(p) = find_path(root, &|b| b.id() == id) else { return false };
    if clean.is_empty() {
        return false;
    }
    let (last, parent) = p.split_last().expect("non-empty path");
    match list_at(root, parent).and_then(|l| l.get_mut(*last)) {
        Some(Bookmark::Link { title, .. }) => *title = clip(clean, 300),
        Some(Bookmark::Folder { name, .. }) => *name = clip(clean, 120),
        None => return false,
    }
    true
}

pub fn remove(root: &mut Vec<Bookmark>, id: &str) -> bool {
    let Some(p) = find_path(root, &|b| b.id() == id) else { return false };
    let (last, parent) = p.split_last().expect("non-empty path");
    match list_at(root, parent) {
        Some(l) => {
            l.remove(*last);
            true
        }
        None => false,
    }
}

/// New folder at the end of the bar; returns its id.
pub fn new_folder(root: &mut Vec<Bookmark>, name: &str) -> String {
    let f = folder(name, vec![]);
    let id = f.id().to_string();
    root.push(f);
    id
}

/// Addresses of all the links of a folder (subfolders included), in the order tabs should open (reversed).
pub fn open_all_urls(folder: &Bookmark) -> Vec<String> {
    fn walk(items: &[Bookmark], out: &mut Vec<String>) {
        for i in items {
            match i { Bookmark::Link { url, .. } => out.push(url.clone()), Bookmark::Folder { items, .. } => walk(items, out) }
        }
    }
    let mut v = Vec::new();
    if let Bookmark::Folder { items, .. } = folder { walk(items, &mut v) }
    v.reverse();
    v
}

// ---------------------------------------------------------------- HTML

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn unescape_html(s: &str) -> String {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)&(#x[0-9a-f]+|#\d+|[a-z]+);").expect("static regex"))
        .replace_all(s, |c: &regex::Captures| {
            let m = c[0].to_string();
            let code = &c[1];
            if let Some(rest) = code.strip_prefix('#') {
                let n = if rest.starts_with(['x', 'X']) { u32::from_str_radix(&rest[1..], 16).ok() } else { rest.parse::<u32>().ok() };
                return n.filter(|n| *n > 0 && *n <= 0x10ffff).and_then(char::from_u32).map(|c| c.to_string()).unwrap_or(m);
            }
            match code.to_lowercase().as_str() { "amp" => "&".into(), "lt" => "<".into(), "gt" => ">".into(), "quot" => "\"".into(), "apos" => "'".into(), "nbsp" => " ".into(), _ => m }
        })
        .into_owned()
}

fn is_web_url(url: &str) -> bool {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)^https?://[^\s/]+").expect("static regex")).is_match(url)
}

/// Bookmarks to an HTML file (Netscape format). `now_ms` is the added date.
pub fn export_bookmarks_html(tree: &[Bookmark], now_ms: i64) -> String {
    let stamp = now_ms.div_euclid(1000);
    let mut lines: Vec<String> = Vec::new();
    fn walk(nodes: &[Bookmark], indent: &str, stamp: i64, lines: &mut Vec<String>) {
        for n in nodes {
            match n {
                Bookmark::Folder { name, items, .. } => {
                    lines.push(format!("{indent}<DT><H3 ADD_DATE=\"{stamp}\">{}</H3>", escape_html(name)));
                    lines.push(format!("{indent}<DL><p>"));
                    walk(items, &format!("{indent}    "), stamp, lines);
                    lines.push(format!("{indent}</DL><p>"));
                }
                Bookmark::Link { url, title, .. } if is_web_url(url) => {
                    lines.push(format!("{indent}<DT><A HREF=\"{}\" ADD_DATE=\"{stamp}\">{}</A>", escape_html(url), escape_html(if title.is_empty() { url } else { title })));
                }
                _ => {}
            }
        }
    }
    walk(tree, "    ", stamp, &mut lines);
    let mut out = vec![
        "<!DOCTYPE NETSCAPE-Bookmark-file-1>".to_string(),
        "<!-- This is an automatically generated file. It will be read and overwritten. DO NOT EDIT! -->".into(),
        "<META HTTP-EQUIV=\"Content-Type\" CONTENT=\"text/html; charset=UTF-8\">".into(),
        "<TITLE>Bookmarks</TITLE>".into(),
        "<H1>Bookmarks</H1>".into(),
        "<DL><p>".into(),
    ];
    out.extend(lines);
    out.push("</DL><p>".into());
    out.push(String::new());
    out.join("\n")
}

/// HTML bookmarks file to a tree (new ids). Tolerant: unknown tags ignored, non-web links dropped, nesting limited.
pub fn parse_bookmarks_html(html: &str) -> Vec<Bookmark> {
    static TAG: OnceLock<Regex> = OnceLock::new();
    static HREF: OnceLock<Regex> = OnceLock::new();
    static WS: OnceLock<Regex> = OnceLock::new();
    let tag = TAG.get_or_init(|| Regex::new(r"(?i)<(/?)(dl|h3|a)\b([^>]*)>([^<]*)").expect("static regex"));
    let href_re = HREF.get_or_init(|| Regex::new(r#"(?i)\bhref\s*=\s*(?:"([^"]*)"|'([^']*)')"#).expect("static regex"));
    let ws = WS.get_or_init(|| Regex::new(r"\s+").expect("static regex"));

    // Folder paths are tracked by index chains in `root`.
    let mut root: Vec<Bookmark> = Vec::new();
    // Stack entries: Some(path of folder) or None (list without a folder).
    let mut stack: Vec<Option<Vec<usize>>> = Vec::new();
    let mut depth = 0usize;
    let mut pending: Option<Vec<usize>> = None;

    fn current_path(stack: &[Option<Vec<usize>>]) -> Vec<usize> {
        stack.iter().rev().find_map(|f| f.clone()).unwrap_or_default()
    }
    for m in tag.captures_iter(html) {
        let closing = &m[1] == "/";
        let name = m[2].to_lowercase();
        match (name.as_str(), closing) {
            ("dl", true) => {
                if let Some(Some(_)) = stack.pop() {
                    depth -= 1;
                }
            }
            ("dl", false) => match pending.take() {
                Some(p) => {
                    stack.push(Some(p));
                    depth += 1;
                }
                None => stack.push(None),
            },
            ("h3", false) => {
                let n = unescape_html(&m[4]).trim().to_string();
                let folder = Bookmark::Folder { id: new_id(), name: if n.is_empty() { "\u{2026}".into() } else { n }, items: vec![] };
                if depth + 1 < MAX_BOOKMARK_DEPTH {
                    let cp = current_path(&stack);
                    if let Some(list) = list_at(&mut root, &cp) {
                        list.push(folder);
                        let mut np = cp;
                        np.push(list.len() - 1);
                        pending = Some(np);
                    }
                } else {
                    pending = None;
                }
            }
            ("a", false) => {
                let url = href_re.captures(&m[3]).map(|h| unescape_html(h.get(1).or(h.get(2)).map(|x| x.as_str()).unwrap_or("")).trim().to_string()).unwrap_or_default();
                if !is_web_url(&url) {
                    continue;
                }
                let t = ws.replace_all(&unescape_html(&m[4]), " ").trim().to_string();
                let link = Bookmark::Link { id: new_id(), title: if t.is_empty() { url.clone() } else { t }, url, favicon: None };
                let cp = current_path(&stack);
                if let Some(list) = list_at(&mut root, &cp) {
                    list.push(link);
                }
            }
            _ => {}
        }
    }
    root
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tr(k: &str, _: &[(&str, String)]) -> String { if k == "defaults.folderFallback" { "Folder".into() } else { k.into() } }
    fn link(id: &str, title: &str, url: &str) -> Bookmark { Bookmark::Link { id: id.into(), title: title.into(), url: url.into(), favicon: None } }
    fn folder(id: &str, name: &str, items: Vec<Bookmark>) -> Bookmark { Bookmark::Folder { id: id.into(), name: name.into(), items } }
    fn tab(url: &str) -> TabInfo<'_> { TabInfo { url, title: "T", custom_title: None, favicon: None } }

    fn tree() -> Vec<Bookmark> {
        vec![
            link("a", "Tom & \"Jerry\" <b>", "https://example.com/?a=1&b=2"),
            folder("f", "Work", vec![link("b", "Docs", "https://docs.example.com"), folder("g", "Deep", vec![])]),
            link("c", "Bad", "javascript:alert(1)"),
        ]
    }

    fn strip(nodes: &[Bookmark]) -> Value {
        Value::Array(nodes.iter().map(|n| match n {
            Bookmark::Link { title, url, .. } => json!({"title": title, "url": url}),
            Bookmark::Folder { name, items, .. } => json!({"name": name, "items": strip(items)}),
        }).collect())
    }

    #[test]
    fn html_export() {
        let html = export_bookmarks_html(&tree(), 1_700_000_000_000);
        assert!(html.starts_with("<!DOCTYPE NETSCAPE-Bookmark-file-1>"));
        assert!(html.contains("<H3 ADD_DATE=\"1700000000\">Work</H3>"));
        assert!(html.contains("HREF=\"https://example.com/?a=1&amp;b=2\""));
        assert!(html.contains("Tom &amp; &quot;Jerry&quot; &lt;b&gt;"));
        assert!(!html.contains("javascript:"));
    }

    #[test]
    fn html_round_trip() {
        let back = parse_bookmarks_html(&export_bookmarks_html(&tree(), 0));
        assert_eq!(strip(&back), json!([
            {"title":"Tom & \"Jerry\" <b>","url":"https://example.com/?a=1&b=2"},
            {"name":"Work","items":[{"title":"Docs","url":"https://docs.example.com"},{"name":"Deep","items":[]}]}
        ]));
        assert_ne!(back[0].id(), "a");
    }

    #[test]
    fn html_browser_style() {
        let html = "<dl><p><dt><h3>News</h3><dl><p><dt><a href='https://a.test/x'>A &#38; B</a><dt><a href=\"ftp://nope\">ftp</a></dl><p></dl>";
        assert_eq!(strip(&parse_bookmarks_html(html)), json!([{"name":"News","items":[{"title":"A & B","url":"https://a.test/x"}]}]));
        assert!(parse_bookmarks_html("not html at all").is_empty());
    }

    #[test]
    fn html_depth_limit() {
        let mut html = String::new();
        for i in 0..10 { html += &format!("<DT><H3>L{i}</H3><DL><p>"); }
        html += "<DT><A HREF=\"https://deep.test\">deep</A>";
        html += &"</DL><p>".repeat(10);
        let mut depth = 0;
        let parsed = parse_bookmarks_html(&html);
        let mut node = parsed.first();
        while let Some(Bookmark::Folder { items, .. }) = node {
            depth += 1;
            node = items.iter().find(|n| matches!(n, Bookmark::Folder { .. })).or(items.first());
        }
        assert!(depth <= 6, "depth {depth}");
        assert!(html.contains("deep.test"));
        let all = serde_json::to_string(&strip(&parsed)).unwrap();
        assert!(all.contains("deep.test"));
    }

    #[test]
    fn prune_cleans() {
        let raw = json!([
            {"kind":"link","id":"x","url":"https://a.com","title":"t".repeat(400),"favicon":""},
            {"kind":"link","url":""},
            {"kind":"folder","name":"","items":[{"kind":"link","url":"https://b.com"}]},
            {"kind":"weird"}, 7
        ]);
        let out = prune_bookmarks(&raw, 0, &tr);
        assert_eq!(out.len(), 2);
        assert!(matches!(&out[0], Bookmark::Link { title, favicon: None, .. } if title.len() == 300));
        assert!(matches!(&out[1], Bookmark::Folder { name, items, .. } if name == "Folder" && matches!(&items[0], Bookmark::Link { title, .. } if title == "https://b.com")));
        assert!(prune_bookmarks(&json!("x"), 0, &tr).is_empty());
    }

    #[test]
    fn toggle_add_remove() {
        let mut bar = vec![];
        assert!(toggle(&mut bar, &tab("https://a.com/")));
        assert_eq!(bar.len(), 1);
        assert!(find_url(&bar, "https://a.com").is_some());
        assert!(toggle(&mut bar, &tab("https://a.com")));
        assert!(bar.is_empty());
        assert!(!toggle(&mut bar, &tab("about:blank")));
        assert!(toggle(&mut bar, &TabInfo { url: "https://b.com", title: "", custom_title: Some("Custom"), favicon: Some("f.ico") }));
        assert!(matches!(&bar[0], Bookmark::Link { title, favicon, .. } if title == "Custom" && favicon.as_deref() == Some("f.ico")));
    }

    #[test]
    fn add_into_folder_and_move() {
        let mut bar = tree();
        assert!(add(&mut bar, &tab("https://n.com"), &BookmarkTarget { folder_id: Some("f".into()), index: 0 }));
        assert!(matches!(find(&bar, "f"), Some(Bookmark::Folder { items, .. }) if items.len() == 3 && matches!(&items[0], Bookmark::Link { url, .. } if url == "https://n.com")));
        assert!(!add(&mut bar, &tab("https://n.com"), &BookmarkTarget { folder_id: Some("nope".into()), index: 0 }));
        // Folder into its own subfolder: refused.
        assert!(!move_to(&mut bar, "f", &BookmarkTarget { folder_id: Some("g".into()), index: 0 }));
        // Link a into folder g, then reorder at root.
        assert!(move_to(&mut bar, "a", &BookmarkTarget { folder_id: Some("g".into()), index: 0 }));
        assert!(matches!(find(&bar, "g"), Some(Bookmark::Folder { items, .. }) if items.len() == 1));
        assert!(move_to(&mut bar, "c", &BookmarkTarget { folder_id: None, index: 0 }));
        assert_eq!(bar[0].id(), "c");
        // Same-list move: index shifts when the source is before the target.
        assert!(move_to(&mut bar, "c", &BookmarkTarget { folder_id: None, index: 2 }));
        assert_eq!(bar.iter().map(|b| b.id()).collect::<Vec<_>>(), vec!["f", "c"]);
        assert!(!move_to(&mut bar, "ghost", &BookmarkTarget { folder_id: None, index: 0 }));
    }

    #[test]
    fn rename_remove_folder_openall() {
        let mut bar = tree();
        assert!(rename(&mut bar, "b", "  New  "));
        assert!(matches!(find(&bar, "b"), Some(Bookmark::Link { title, .. }) if title == "New"));
        assert!(!rename(&mut bar, "b", "   "));
        assert!(rename(&mut bar, "g", "Renamed"));
        assert!(remove(&mut bar, "g"));
        assert!(find(&bar, "g").is_none());
        let id = new_folder(&mut bar, "New folder");
        assert_eq!(bar.last().unwrap().id(), id);
        let all = open_all_urls(find(&bar, "f").unwrap());
        assert_eq!(all, vec!["https://docs.example.com"]);
        assert!(!remove(&mut bar, "ghost"));
    }

    #[test]
    fn json_format_matches_electron() {
        let b = link("x", "t", "https://a.com");
        assert_eq!(serde_json::to_value(&b).unwrap(), json!({"kind":"link","id":"x","title":"t","url":"https://a.com"}));
        let back: Bookmark = serde_json::from_value(json!({"kind":"folder","id":"f","name":"n","items":[]})).unwrap();
        assert_eq!(back, folder("f", "n", vec![]));
    }
}

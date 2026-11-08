//! Notes and Easels: which document a page may touch, and the cleaning of what it saves
//! (port of `senderDoc`, `save` and `cleanItems` of `main/features/notes/notes.ts`, and `shared/notes.ts` limits).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

use crate::scripts::AppPage;

pub const TITLE_MAX: usize = 200;
/// Maximum size of a note (HTML), in UTF-16 units like the JS `length`.
pub const NOTE_MAX: usize = 2 * 1024 * 1024;
/// Maximum size of an Easel (JSON, images included).
pub const EASEL_MAX: usize = 30 * 1024 * 1024;
const ITEMS_MAX: usize = 5000;
const POINTS_MAX: usize = 20000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NoteKind {
    Note,
    Easel,
}

/// The document an app page is opened on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteRef {
    pub kind: NoteKind,
    /// Lowercase UUID.
    pub id: String,
}

/// `8-4-4-4-12` hex digits, any case.
pub fn is_note_id(s: &str) -> bool {
    let parts: Vec<&str> = s.split('-').collect();
    parts.len() == 5
        && [8, 4, 4, 4, 12].iter().zip(&parts).all(|(n, p)| p.len() == *n && p.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// Document designated by the real address of a page: the document address itself (`signet://note/<id>`, what
/// `signet-pages` serves), or an app page of `pages` on a document route (`<app page>#note/<id>`), or None.
pub fn note_of_app_url(url: &str, pages: &[AppPage]) -> Option<NoteRef> {
    if let Some(doc) = parse_document_url(url) {
        return Some(doc);
    }
    let u = Url::parse(url).ok()?;
    let host = match u.port() {
        Some(p) => format!("{}:{p}", u.host_str()?),
        None => u.host_str()?.to_owned(),
    };
    let app = pages.iter().any(|p| format!("{}:", u.scheme()) == p.protocol && host == p.host && u.path() == p.pathname);
    if !app {
        return None;
    }
    let frag = u.fragment()?;
    let (kind, id) = frag.split_once('/')?;
    let kind = match kind.to_ascii_lowercase().as_str() {
        "note" => NoteKind::Note,
        "easel" => NoteKind::Easel,
        _ => return None,
    };
    is_note_id(id).then(|| NoteRef { kind, id: id.to_ascii_lowercase() })
}

/// Strict form of [`parse_note_address`] for a frame URL: no surrounding spaces, scheme exactly `signet`.
fn parse_document_url(url: &str) -> Option<NoteRef> {
    if url != url.trim() {
        return None;
    }
    parse_note_address(url)
}

/// `signet://note/<id>` or `signet://easel/<id>` to the document it names.
pub fn parse_note_address(url: &str) -> Option<NoteRef> {
    let rest = url.trim().strip_prefix("signet://")?;
    let rest = rest.strip_suffix('/').unwrap_or(rest);
    let (kind, id) = rest.split_once('/')?;
    let kind = match kind.to_ascii_lowercase().as_str() {
        "note" => NoteKind::Note,
        "easel" => NoteKind::Easel,
        _ => return None,
    };
    is_note_id(id).then(|| NoteRef { kind, id: id.to_ascii_lowercase() })
}

fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// `s.slice(0, n)` of JS (UTF-16 units; a cut surrogate pair is dropped).
fn js_slice(s: &str, n: usize) -> String {
    let mut count = 0;
    let mut out = String::new();
    for c in s.chars() {
        count += c.len_utf16();
        if count > n {
            break;
        }
        out.push(c);
    }
    out
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EaselView {
    pub x: f64,
    pub y: f64,
    pub zoom: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EaselItem {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub html: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub src: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub points: Option<Vec<f64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vw: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vh: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<f64>,
}

/// A save request, validated.
#[derive(Debug, Clone, PartialEq)]
pub enum NotesPatch {
    Note { title: String, html: String },
    Easel { title: String, items: Vec<EaselItem>, view: EaselView },
}

/// Finite bounded number, otherwise `fallback`.
fn num(v: Option<&Value>, min: f64, max: f64, fallback: f64) -> f64 {
    match v.and_then(Value::as_f64) {
        Some(n) if n.is_finite() => n.clamp(min, max),
        _ => fallback,
    }
}

fn color_ok(s: &str) -> bool {
    let hex = s.strip_prefix('#').is_some_and(|h| (3..=8).contains(&h.len()) && h.bytes().all(|b| b.is_ascii_hexdigit()));
    let var = s.strip_prefix("var(--").and_then(|r| r.strip_suffix(')')).is_some_and(|n| {
        !n.is_empty() && n.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    });
    hex || var
}

fn image_src_ok(s: &str) -> bool {
    let lower: String = s.chars().take(40).collect::<String>().to_ascii_lowercase();
    ["data:image/png;base64,", "data:image/jpeg;base64,", "data:image/webp;base64,", "data:image/gif;base64,"]
        .iter()
        .any(|p| lower.starts_with(p))
}

/// Easel items coming from the page, validated (types, bounds, known fields only).
pub fn clean_items(value: Option<&Value>) -> Vec<EaselItem> {
    let Some(Value::Array(list)) = value else { return Vec::new() };
    let mut out = Vec::new();
    for (index, raw) in list.iter().take(ITEMS_MAX).enumerate() {
        let Value::Object(v) = raw else { continue };
        let kind = match v.get("type").and_then(Value::as_str) {
            Some(k @ ("text" | "image" | "stroke")) => k,
            _ => continue,
        };
        let w = num(v.get("w"), 1.0, 1e5, 100.0);
        let h = num(v.get("h"), 1.0, 1e5, 100.0);
        let mut item = EaselItem {
            id: match v.get("id").and_then(Value::as_str) {
                Some(id) => js_slice(id, 64),
                None => format!("item-{index}"),
            },
            kind: kind.to_owned(),
            x: num(v.get("x"), -1e6, 1e6, 0.0),
            y: num(v.get("y"), -1e6, 1e6, 0.0),
            w,
            h,
            color: None,
            html: None,
            src: None,
            points: None,
            vw: None,
            vh: None,
            size: None,
        };
        if let Some(c) = v.get("color").and_then(Value::as_str).filter(|c| color_ok(c)) {
            item.color = Some(c.to_owned());
        }
        match kind {
            "text" => item.html = Some(v.get("html").and_then(Value::as_str).unwrap_or("").to_owned()),
            "image" => match v.get("src").and_then(Value::as_str) {
                Some(src) if image_src_ok(src) => item.src = Some(src.to_owned()),
                _ => continue,
            },
            _ => {
                let Some(Value::Array(points)) = v.get("points") else { continue };
                if points.len() < 2 {
                    continue;
                }
                item.points = Some(points.iter().take(POINTS_MAX).map(|p| num(Some(p), -1e6, 1e6, 0.0)).collect());
                item.vw = Some(num(v.get("vw"), 1.0, 1e5, w));
                item.vh = Some(num(v.get("vh"), 1.0, 1e5, h));
                item.size = Some(num(v.get("size"), 0.5, 64.0, 3.0));
            }
        }
        out.push(item);
    }
    out
}

/// Validates the patch of `notes:save` for a document of `kind`; None when it is refused (the page gets `false`).
pub fn clean_patch(kind: NoteKind, patch: &Value) -> Option<NotesPatch> {
    let Value::Object(p) = patch else { return None };
    let title = p.get("title").and_then(Value::as_str).map(|t| js_slice(t.trim(), TITLE_MAX)).unwrap_or_default();
    match kind {
        NoteKind::Note => {
            let html = p.get("html").and_then(Value::as_str).unwrap_or("");
            (utf16_len(html) <= NOTE_MAX).then(|| NotesPatch::Note { title, html: html.to_owned() })
        }
        NoteKind::Easel => {
            if !p.contains_key("items") {
                return None;
            }
            let v = p.get("view").and_then(Value::as_object);
            let view = EaselView {
                x: num(v.and_then(|v| v.get("x")), -1e7, 1e7, 0.0),
                y: num(v.and_then(|v| v.get("y")), -1e7, 1e7, 0.0),
                zoom: num(v.and_then(|v| v.get("zoom")), 0.1, 8.0, 1.0),
            };
            let items = clean_items(p.get("items"));
            let size = serde_json::to_string(&items).map(|s| s.len()).unwrap_or(usize::MAX);
            (size <= EASEL_MAX).then_some(NotesPatch::Easel { title, items, view })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const ID: &str = "0b9d0f3e-1c1a-4a53-9a6f-2f6d2b1c9e11";

    #[test]
    fn document_address_is_the_page() {
        let r = note_of_app_url(&format!("signet://note/{ID}"), &[]).unwrap();
        assert_eq!((r.kind, r.id.as_str()), (NoteKind::Note, ID));
        assert_eq!(note_of_app_url(&format!("signet://easel/{}/", ID.to_uppercase()), &[]).unwrap().kind, NoteKind::Easel);
        for url in [format!("signet://note/{ID}x"), format!(" signet://note/{ID}"), format!("https://note/{ID}"), "signet://note/nope".to_owned()] {
            assert_eq!(note_of_app_url(&url, &[]), None, "{url}");
        }
    }

    fn pages() -> Vec<AppPage> {
        vec![AppPage::new("signet:", "app", "/index.html")]
    }

    #[test]
    fn route_of_an_app_url() {
        let ok = note_of_app_url(&format!("signet://app/index.html#note/{ID}"), &pages()).unwrap();
        assert_eq!(ok, NoteRef { kind: NoteKind::Note, id: ID.into() });
        let upper = note_of_app_url(&format!("signet://app/index.html#EASEL/{}", ID.to_uppercase()), &pages()).unwrap();
        assert_eq!(upper, NoteRef { kind: NoteKind::Easel, id: ID.into() });
    }

    #[test]
    fn refuses_other_pages_and_routes() {
        for url in [
            format!("https://evil.example/index.html#note/{ID}"),
            format!("signet://evil/index.html#note/{ID}"),
            format!("signet://app/other.html#note/{ID}"),
            format!("signet://app/index.html#note/{}x", &ID[..35]),
            "signet://app/index.html#note/not-an-id".to_owned(),
            "signet://app/index.html".to_owned(),
            format!("signet://app/index.html#folder/{ID}"),
            "not a url".to_owned(),
        ] {
            assert_eq!(note_of_app_url(&url, &pages()), None, "{url}");
        }
    }

    #[test]
    fn dev_server_pages_with_port() {
        let pages = vec![AppPage::new("http:", "localhost:5173", "/")];
        assert!(note_of_app_url(&format!("http://localhost:5173/#note/{ID}"), &pages).is_some());
        assert!(note_of_app_url(&format!("http://localhost:5174/#note/{ID}"), &pages).is_none());
    }

    #[test]
    fn note_addresses() {
        assert_eq!(parse_note_address(&format!("signet://note/{ID}")).unwrap().kind, NoteKind::Note);
        assert_eq!(parse_note_address(&format!(" signet://easel/{ID}/ ")).unwrap().kind, NoteKind::Easel);
        assert!(parse_note_address("signet://note/zzz").is_none());
        assert!(parse_note_address(&format!("https://note/{ID}")).is_none());
    }

    #[test]
    fn note_patch() {
        let p = clean_patch(NoteKind::Note, &json!({ "title": "  hi  ", "html": "<p>x</p>" })).unwrap();
        assert_eq!(p, NotesPatch::Note { title: "hi".into(), html: "<p>x</p>".into() });
        let long = "a".repeat(300);
        match clean_patch(NoteKind::Note, &json!({ "title": long, "html": 5 })).unwrap() {
            NotesPatch::Note { title, html } => {
                assert_eq!(title.len(), TITLE_MAX);
                assert_eq!(html, "");
            }
            other => panic!("{other:?}"),
        }
        assert!(clean_patch(NoteKind::Note, &json!("x")).is_none());
        assert!(clean_patch(NoteKind::Note, &json!({ "title": "t", "html": "a".repeat(NOTE_MAX + 1) })).is_none());
        assert!(clean_patch(NoteKind::Note, &json!({ "title": "t", "html": "a".repeat(NOTE_MAX) })).is_some());
    }

    #[test]
    fn note_size_counts_utf16_units() {
        // 1M astral characters are 2M UTF-16 units: at the limit, one more is refused.
        let at_limit = "\u{1F600}".repeat(NOTE_MAX / 2);
        assert!(clean_patch(NoteKind::Note, &json!({ "html": at_limit })).is_some());
        let over = "\u{1F600}".repeat(NOTE_MAX / 2 + 1);
        assert!(clean_patch(NoteKind::Note, &json!({ "html": over })).is_none());
    }

    #[test]
    fn easel_patch_needs_items_and_clamps() {
        assert!(clean_patch(NoteKind::Easel, &json!({ "title": "t" })).is_none());
        let p = clean_patch(
            NoteKind::Easel,
            &json!({ "title": "e", "items": [], "view": { "x": 1e12, "y": "a", "zoom": 100 } }),
        )
        .unwrap();
        assert_eq!(p, NotesPatch::Easel { title: "e".into(), items: vec![], view: EaselView { x: 1e7, y: 0.0, zoom: 8.0 } });
    }

    #[test]
    fn easel_items_are_cleaned() {
        let items = clean_items(Some(&json!([
            { "id": "a", "type": "text", "x": 5, "y": -2e9, "w": 0, "h": 7, "html": "<b>x</b>", "color": "#fff", "evil": 1 },
            { "type": "text", "color": "red" },
            { "type": "image", "src": "javascript:alert(1)" },
            { "type": "image", "src": "data:image/PNG;base64,AAAA" },
            { "type": "stroke", "points": [1] },
            { "type": "stroke", "points": [0, 0, 10, 10], "size": 1000, "color": "var(--ink)" },
            { "type": "video" },
            "nope"
        ])));
        assert_eq!(items.len(), 4);
        assert_eq!((items[0].x, items[0].y, items[0].w, items[0].h), (5.0, -1e6, 1.0, 7.0));
        assert_eq!(items[0].color.as_deref(), Some("#fff"));
        assert_eq!(items[1].color, None);
        assert_eq!(items[1].id, "item-1");
        assert!(items[2].src.is_some());
        assert_eq!(items[3].size, Some(64.0));
        assert_eq!(items[3].color.as_deref(), Some("var(--ink)"));
        assert_eq!(items[3].vw, Some(100.0));
        let json = serde_json::to_value(&items[0]).unwrap();
        assert!(json.get("evil").is_none() && json.get("src").is_none());
    }

    #[test]
    fn item_count_is_capped() {
        let many: Vec<Value> = (0..6000).map(|_| json!({ "type": "text" })).collect();
        assert_eq!(clean_items(Some(&Value::Array(many))).len(), ITEMS_MAX);
    }
}

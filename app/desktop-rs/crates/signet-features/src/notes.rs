//! Notes and Easels storage (port of `main/features/notes/notes.ts` and `shared/notes.ts`): one JSON file per
//! document in `<data>/notes/<id>.json`, validation of what the page sends, size limits.
use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::OnceLock;

use crate::util::{clip, now_ms, read_json, write_json, Result};

pub const NOTE_MAX: usize = 2 * 1024 * 1024;
pub const EASEL_MAX: usize = 30 * 1024 * 1024;
const TITLE_MAX: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NoteKind {
    Note,
    Easel,
}

impl NoteKind {
    pub fn as_str(self) -> &'static str {
        match self { NoteKind::Note => "note", NoteKind::Easel => "easel" }
    }
}

fn id_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$").expect("static regex"))
}

pub fn is_note_id(id: &str) -> bool {
    id_re().is_match(id)
}

/// Displayed address: `signet://note/<id>`.
pub fn note_address(kind: NoteKind, id: &str) -> String {
    format!("signet://{}/{}", kind.as_str(), id)
}

/// `signet://note/<id>` to (kind, id); `None` for any other address.
pub fn parse_note_address(url: &str) -> Option<(NoteKind, String)> {
    static R: OnceLock<Regex> = OnceLock::new();
    let r = R.get_or_init(|| Regex::new(r"(?i)^signet://(note|easel)/([0-9a-f-]{36})/?$").expect("static regex"));
    let c = r.captures(url.trim())?;
    is_note_id(&c[2]).then(|| (if c[1].eq_ignore_ascii_case("note") { NoteKind::Note } else { NoteKind::Easel }, c[2].to_lowercase()))
}

/// Real address of a document: the app's UI route. `base` is the UI address (for example `signetui://app/index.html`).
pub fn note_app_url(base: &str, kind: NoteKind, id: &str) -> String {
    format!("{base}#{}/{id}", kind.as_str())
}

/// `signet://note/<id>` to the real address to load.
pub fn resolve_note_address(base: &str, url: &str) -> Option<String> {
    // No UI page: documents are loaded at their own address.
    if base.is_empty() {
        return None;
    }
    parse_note_address(url).map(|(k, id)| note_app_url(base, k, &id))
}

/// Document designated by the real address of a UI page (`<base>#note/<id>`).
pub fn note_of_app_url(base: &str, url: &str) -> Option<(NoteKind, String)> {
    let rest = url.strip_prefix(base)?.strip_prefix('#')?;
    let (k, id) = rest.split_once('/')?;
    let kind = match k.to_ascii_lowercase().as_str() { "note" => NoteKind::Note, "easel" => NoteKind::Easel, _ => return None };
    is_note_id(id).then(|| (kind, id.to_lowercase()))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum NotesDoc {
    #[serde(rename_all = "camelCase")]
    Note { id: String, title: String, html: String, created_at: i64, updated_at: i64 },
    #[serde(rename_all = "camelCase")]
    Easel { id: String, title: String, items: Vec<EaselItem>, view: View, created_at: i64, updated_at: i64 },
}

impl NotesDoc {
    pub fn id(&self) -> &str {
        match self { NotesDoc::Note { id, .. } | NotesDoc::Easel { id, .. } => id }
    }
    pub fn kind(&self) -> NoteKind {
        match self { NotesDoc::Note { .. } => NoteKind::Note, NotesDoc::Easel { .. } => NoteKind::Easel }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct View {
    pub x: f64,
    pub y: f64,
    pub zoom: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct EaselItem {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
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
    pub color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<f64>,
}

/// What the page sends to save.
#[derive(Debug, Clone)]
pub enum NotesPatch {
    Note { title: String, html: String },
    Easel { title: String, items: Value, view: Option<Value> },
}

fn num(v: Option<&Value>, min: f64, max: f64, fallback: f64) -> f64 {
    match v.and_then(|x| x.as_f64()) {
        Some(n) if n.is_finite() => n.clamp(min, max),
        _ => fallback,
    }
}

/// Easel items coming from the page, validated (types, bounds, known fields only).
pub fn clean_items(value: &Value) -> Vec<EaselItem> {
    let Some(arr) = value.as_array() else { return vec![] };
    let color_re = Regex::new(r"(?i)^(#[0-9a-f]{3,8}|var\(--[a-z0-9-]+\))$").expect("static regex");
    let img_re = Regex::new(r"(?i)^data:image/(png|jpeg|webp|gif);base64,").expect("static regex");
    let mut out = Vec::new();
    for raw in arr.iter().take(5000) {
        let Some(o) = raw.as_object() else { continue };
        let kind = match o.get("type").and_then(|t| t.as_str()) { Some(k @ ("text" | "image" | "stroke")) => k, _ => continue };
        let mut item = EaselItem {
            id: o.get("id").and_then(|v| v.as_str()).map(|s| clip(s, 64)).unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            kind: kind.into(),
            x: num(o.get("x"), -1e6, 1e6, 0.0),
            y: num(o.get("y"), -1e6, 1e6, 0.0),
            w: num(o.get("w"), 1.0, 1e5, 100.0),
            h: num(o.get("h"), 1.0, 1e5, 100.0),
            ..Default::default()
        };
        if let Some(c) = o.get("color").and_then(|v| v.as_str())
            && color_re.is_match(c) { item.color = Some(c.into()); }
        match kind {
            "text" => item.html = Some(o.get("html").and_then(|v| v.as_str()).unwrap_or("").to_string()),
            "image" => match o.get("src").and_then(|v| v.as_str()) {
                Some(s) if img_re.is_match(s) => item.src = Some(s.into()),
                _ => continue,
            },
            _ => {
                let Some(pts) = o.get("points").and_then(|v| v.as_array()).filter(|p| p.len() >= 2) else { continue };
                item.points = Some(pts.iter().take(20000).map(|p| num(Some(p), -1e6, 1e6, 0.0)).collect());
                item.vw = Some(num(o.get("vw"), 1.0, 1e5, item.w));
                item.vh = Some(num(o.get("vh"), 1.0, 1e5, item.h));
                item.size = Some(num(o.get("size"), 0.5, 64.0, 3.0));
            }
        }
        out.push(item);
    }
    out
}

/// Documents on disk.
pub struct Notes {
    dir: PathBuf,
}

impl Notes {
    pub fn new(data_dir: &Path) -> Self {
        Notes { dir: data_dir.join("notes") }
    }

    fn file(&self, id: &str) -> Option<PathBuf> {
        is_note_id(id).then(|| self.dir.join(format!("{id}.json")))
    }

    /// Document by id, or `None` (missing, unreadable or inconsistent).
    pub fn read(&self, id: &str) -> Option<NotesDoc> {
        let doc: NotesDoc = read_json(&self.file(id)?)?;
        (doc.id() == id).then_some(doc)
    }

    pub fn list(&self) -> Vec<NotesDoc> {
        let Ok(rd) = fs::read_dir(&self.dir) else { return vec![] };
        let mut docs: Vec<NotesDoc> = rd
            .flatten()
            .filter_map(|e| read_json::<NotesDoc>(&e.path()))
            .collect();
        docs.sort_by(|a, b| a.id().cmp(b.id()));
        docs
    }

    pub fn write(&self, doc: &NotesDoc) -> Result<()> {
        match self.file(doc.id()) {
            Some(f) => write_json(&f, doc),
            None => crate::util::err("invalid note id"),
        }
    }

    /// Creates an empty document and returns its id (the app opens `note_address` in a tab).
    pub fn create(&self, kind: NoteKind) -> Result<String> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = now_ms();
        let doc = match kind {
            NoteKind::Note => NotesDoc::Note { id: id.clone(), title: String::new(), html: String::new(), created_at: now, updated_at: now },
            NoteKind::Easel => NotesDoc::Easel { id: id.clone(), title: String::new(), items: vec![], view: View { x: 0.0, y: 0.0, zoom: 1.0 }, created_at: now, updated_at: now },
        };
        self.write(&doc)?;
        Ok(id)
    }

    /// New Easel holding an image (capture tool), scaled down to 900 px wide at most.
    pub fn create_easel_with_image(&self, png_data_url: &str, width: f64, height: f64) -> Result<String> {
        let id = uuid::Uuid::new_v4().to_string();
        let scale = (900.0 / width.max(1.0)).min(1.0);
        let item = serde_json::json!([{ "id": uuid::Uuid::new_v4().to_string(), "type": "image", "x": 60, "y": 60,
            "w": (width * scale).round(), "h": (height * scale).round(), "src": png_data_url }]);
        let now = now_ms();
        self.write(&NotesDoc::Easel { id: id.clone(), title: String::new(), items: clean_items(&item), view: View { x: 0.0, y: 0.0, zoom: 1.0 }, created_at: now, updated_at: now })?;
        Ok(id)
    }

    /// Deletes the file (the app closes the open tabs of `note_address`).
    pub fn remove(&self, id: &str) {
        if let Some(f) = self.file(id) {
            let _ = fs::remove_file(f);
        }
    }

    /// Saves what the page sent; false when refused (too large, wrong shape).
    pub fn save(&self, kind: NoteKind, id: &str, patch: &NotesPatch) -> bool {
        let prev = self.read(id);
        let now = now_ms();
        let created_at = match &prev { Some(NotesDoc::Note { created_at, .. }) | Some(NotesDoc::Easel { created_at, .. }) => *created_at, None => now };
        let doc = match (kind, patch) {
            (NoteKind::Note, NotesPatch::Note { title, html }) => {
                if html.chars().count() > NOTE_MAX {
                    return false;
                }
                NotesDoc::Note { id: id.into(), title: clip(title.trim(), TITLE_MAX), html: html.clone(), created_at, updated_at: now }
            }
            (NoteKind::Easel, NotesPatch::Easel { title, items, view }) => {
                let v = view.as_ref();
                let doc = NotesDoc::Easel {
                    id: id.into(),
                    title: clip(title.trim(), TITLE_MAX),
                    items: clean_items(items),
                    view: View {
                        x: num(v.and_then(|v| v.get("x")), -1e7, 1e7, 0.0),
                        y: num(v.and_then(|v| v.get("y")), -1e7, 1e7, 0.0),
                        zoom: num(v.and_then(|v| v.get("zoom")), 0.1, 8.0, 1.0),
                    },
                    created_at,
                    updated_at: now,
                };
                if serde_json::to_string(&doc).map(|s| s.len()).unwrap_or(usize::MAX) > EASEL_MAX {
                    return false;
                }
                doc
            }
            _ => return false,
        };
        self.write(&doc).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    const ID: &str = "123e4567-e89b-12d3-a456-426614174000";

    #[test]
    fn addresses() {
        assert_eq!(note_address(NoteKind::Note, ID), format!("signet://note/{ID}"));
        assert_eq!(parse_note_address(&format!("signet://EASEL/{}/", ID.to_uppercase())), Some((NoteKind::Easel, ID.into())));
        assert_eq!(parse_note_address("signet://note/xyz"), None);
        let base = "signetui://app/index.html";
        assert_eq!(resolve_note_address(base, &format!("signet://note/{ID}")), Some(format!("{base}#note/{ID}")));
        assert_eq!(note_of_app_url(base, &format!("{base}#easel/{ID}")), Some((NoteKind::Easel, ID.into())));
        assert_eq!(note_of_app_url(base, "https://x.com/#note/1"), None);
    }

    #[test]
    fn items_are_cleaned() {
        let items = clean_items(&json!([
            {"type":"text","x":5,"y":1e9,"w":0,"h":10,"html":"<b>a</b>","color":"#fff"},
            {"type":"image","src":"javascript:1"},
            {"type":"image","src":"data:image/png;base64,AAAA"},
            {"type":"stroke","points":[1]},
            {"type":"stroke","points":[1,2,3,4],"size":999},
            {"type":"weird"}, 3
        ]));
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].y, 1e6);
        assert_eq!(items[0].w, 1.0);
        assert_eq!(items[0].color.as_deref(), Some("#fff"));
        assert_eq!(items[2].size, Some(64.0));
        assert!(clean_items(&json!("nope")).is_empty());
    }

    #[test]
    fn save_and_read() {
        let dir = tempfile::tempdir().unwrap();
        let n = Notes::new(dir.path());
        let id = n.create(NoteKind::Note).unwrap();
        assert!(n.save(NoteKind::Note, &id, &NotesPatch::Note { title: "  Hi  ".into(), html: "<p>x</p>".into() }));
        match n.read(&id) {
            Some(NotesDoc::Note { title, html, .. }) => { assert_eq!(title, "Hi"); assert_eq!(html, "<p>x</p>"); }
            _ => panic!("expected note"),
        }
        assert!(!n.save(NoteKind::Note, &id, &NotesPatch::Note { title: "".into(), html: "x".repeat(NOTE_MAX + 1) }));
        assert!(!n.save(NoteKind::Easel, &id, &NotesPatch::Note { title: "".into(), html: "".into() }));
        assert_eq!(n.list().len(), 1);
        n.remove(&id);
        assert!(n.read(&id).is_none());
        assert!(n.read("../etc/passwd").is_none());
    }

    #[test]
    fn easel_roundtrip_matches_electron_json() {
        let dir = tempfile::tempdir().unwrap();
        let n = Notes::new(dir.path());
        let id = n.create(NoteKind::Easel).unwrap();
        assert!(n.save(NoteKind::Easel, &id, &NotesPatch::Easel { title: "E".into(), items: json!([{"type":"text","html":"a"}]), view: Some(json!({"x":1,"y":2,"zoom":100})) }));
        let raw = fs::read_to_string(dir.path().join("notes").join(format!("{id}.json"))).unwrap();
        let v: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(v["kind"], "easel");
        assert_eq!(v["view"]["zoom"], 8.0);
        assert!(v.get("createdAt").is_some());
    }
}

//! Backup and restore (port of `main/features/backup.ts`): the `.signetbackup` format (gzip JSON), password
//! sealing (scrypt + AES-256-GCM), collecting sections from the profile files and applying a backup to them.
//! Files written by the Electron app restore here and the reverse, since both use the same JSON.
//!
//! Everything works on the profile folder (`state.json`, `bookmarks.json`, `history.json`, `mods.json`, `notes/`,
//! `passwords.json`) as `serde_json::Value`. What needs signet-core (settings cleaning, space gradient validation)
//! or the running app (flushing in-memory stores, relaunch, extensions list, vault cipher) comes in through `Hooks`.
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::util::{now_ms, read_json, write_atomic, Error, Result, Translate};

pub const FORMAT: &str = "signetbackup";
pub const VERSION: i64 = 1;
/// Safety copies kept (made just before each import).
pub const SAFETY_KEEP: usize = 3;
/// Maximum size of a backup file read (decompressed).
pub const MAX_BYTES: u64 = 512 * 1024 * 1024;

pub const BACKUP_SECTIONS: [&str; 13] = ["settings", "favorites", "spaces", "today", "bookmarks", "history", "archive", "boosts", "permissions", "mods", "notes", "passwords", "extensions"];

type Obj = Map<String, Value>;

fn str_of(v: Option<&Value>, max: usize) -> String {
    v.and_then(|x| x.as_str()).map(|s| s.chars().take(max).collect()).unwrap_or_default()
}
fn is_webish(url: &str) -> bool {
    let l = url.to_ascii_lowercase();
    ["http:", "https:", "file:", "signet:"].iter().any(|p| l.starts_with(p))
}
fn is_uuid(s: &str) -> bool {
    crate::notes::is_note_id(s)
}
fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Profile files read or written by the backup.
pub struct Paths {
    pub dir: PathBuf,
}

impl Paths {
    pub fn new(dir: &Path) -> Self {
        Paths { dir: dir.to_path_buf() }
    }
    pub fn state(&self) -> PathBuf { self.dir.join("state.json") }
    pub fn bookmarks(&self) -> PathBuf { self.dir.join("bookmarks.json") }
    pub fn history(&self) -> PathBuf { self.dir.join("history.json") }
    pub fn passwords(&self) -> PathBuf { self.dir.join("passwords.json") }
    pub fn mods(&self) -> PathBuf { self.dir.join("mods.json") }
    pub fn notes(&self) -> PathBuf { self.dir.join("notes") }
    pub fn safety(&self) -> PathBuf { self.dir.join("backups") }
    pub fn pending_extensions(&self) -> PathBuf { self.dir.join("pending-extensions.json") }
}

fn write_json(path: &Path, v: &Value) -> Result<()> {
    write_atomic(path, v.to_string().as_bytes())
}

// ---------------------------------------------------------------- password sealing

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Sealed {
    pub kdf: String,
    pub salt: String,
    pub iv: String,
    pub tag: String,
    pub data: String,
}

fn b64() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

fn derive(password: &str, salt: &[u8]) -> Option<[u8; 32]> {
    // N = 2^15, r = 8, p = 1, as the TS `scryptSync`.
    let params = scrypt::Params::new(15, 8, 1, 32).ok()?;
    let mut key = [0u8; 32];
    scrypt::scrypt(password.as_bytes(), salt, &params, &mut key).ok()?;
    Some(key)
}

pub fn seal(value: &Value, password: &str) -> Result<Sealed> {
    let mut salt = [0u8; 16];
    let mut iv = [0u8; 12];
    rand::rngs::OsRng.fill_bytes(&mut salt);
    rand::rngs::OsRng.fill_bytes(&mut iv);
    let key = derive(password, &salt).ok_or_else(|| Error::Msg("scrypt".into()))?;
    let c = Aes256Gcm::new_from_slice(&key).map_err(|e| Error::Msg(e.to_string()))?;
    let mut ct = c.encrypt(Nonce::from_slice(&iv), value.to_string().as_bytes()).map_err(|_| Error::Msg("encrypt".into()))?;
    let tag = ct.split_off(ct.len() - 16);
    Ok(Sealed { kdf: "scrypt".into(), salt: b64().encode(salt), iv: b64().encode(iv), tag: b64().encode(tag), data: b64().encode(ct) })
}

/// Decrypts; `None` if the password is wrong or the block is damaged.
pub fn unseal(s: &Sealed, password: &str) -> Option<Value> {
    let salt = b64().decode(&s.salt).ok()?;
    let iv = b64().decode(&s.iv).ok()?;
    let mut data = b64().decode(&s.data).ok()?;
    data.extend(b64().decode(&s.tag).ok()?);
    if iv.len() != 12 {
        return None;
    }
    let key = derive(password, &salt)?;
    let plain = Aes256Gcm::new_from_slice(&key).ok()?.decrypt(Nonce::from_slice(&iv), data.as_ref()).ok()?;
    serde_json::from_slice(&plain).ok()
}

// ---------------------------------------------------------------- tabs and items

/// Saved tab content, or `None` for a tab without address.
pub fn tab_data(tab: Option<&Value>) -> Option<Value> {
    let t = tab?.as_object()?;
    let url = t.get("url")?.as_str().filter(|u| !u.is_empty())?;
    let mut d = json!({ "url": url, "title": t.get("title").and_then(|x| x.as_str()).unwrap_or("") });
    for k in ["favicon", "pinnedUrl", "customTitle", "customIcon"] {
        if let Some(s) = t.get(k).and_then(|x| x.as_str()).filter(|s| !s.is_empty()) {
            d[k] = json!(s);
        }
    }
    if t.get("muted").and_then(|m| m.as_bool()) == Some(true) {
        d["muted"] = json!(true);
    }
    Some(d)
}

/// Sidebar items with their tabs in clear.
pub fn items_data(items: &Value, tabs: &Obj) -> Vec<Value> {
    let mut out = Vec::new();
    for item in items.as_array().into_iter().flatten() {
        let Some(o) = item.as_object() else { continue };
        let group = o.get("group").and_then(|g| g.as_str()).filter(|g| !g.is_empty());
        match o.get("kind").and_then(|k| k.as_str()) {
            Some("tab") => {
                if let Some(tab) = o.get("tabId").and_then(|i| i.as_str()).and_then(|id| tab_data(tabs.get(id))) {
                    let mut v = json!({ "kind": "tab", "tab": tab });
                    if let Some(g) = group { v["group"] = json!(g); }
                    out.push(v);
                }
            }
            Some("split") => {
                let list: Vec<Value> = o.get("tabIds").and_then(|t| t.as_array()).into_iter().flatten().filter_map(|id| id.as_str().and_then(|id| tab_data(tabs.get(id)))).collect();
                if list.len() >= 2 {
                    let mut v = json!({ "kind": "split", "tabs": list });
                    for k in ["layout", "sizes"] {
                        if let Some(x) = o.get(k) { v[k] = x.clone(); }
                    }
                    if let Some(g) = group { v["group"] = json!(g); }
                    out.push(v);
                }
            }
            _ => {
                let mut v = json!({ "kind": "folder", "name": o.get("name").cloned().unwrap_or(json!("")), "open": o.get("open").cloned().unwrap_or(json!(true)),
                    "items": items_data(o.get("items").unwrap_or(&Value::Null), tabs) });
                for k in ["icon", "hue", "live"] {
                    if let Some(x) = o.get(k) { v[k] = x.clone(); }
                }
                out.push(v);
            }
        }
    }
    out
}

/// Validated saved tab, or `None` (missing address or unexpected scheme).
pub fn clean_tab(v: &Value) -> Option<Value> {
    let o = v.as_object()?;
    let url = str_of(o.get("url"), 8000);
    if url.is_empty() || !is_webish(&url) {
        return None;
    }
    let mut d = json!({ "url": url, "title": str_of(o.get("title"), 1000) });
    let favicon = str_of(o.get("favicon"), 200_000);
    if !favicon.is_empty() { d["favicon"] = json!(favicon); }
    let pinned = str_of(o.get("pinnedUrl"), 8000);
    if !pinned.is_empty() && is_webish(&pinned) { d["pinnedUrl"] = json!(pinned); }
    let ct = str_of(o.get("customTitle"), 200);
    if !ct.is_empty() { d["customTitle"] = json!(ct); }
    let ci = str_of(o.get("customIcon"), 16);
    if !ci.is_empty() { d["customIcon"] = json!(ci); }
    if o.get("muted").and_then(|m| m.as_bool()) == Some(true) { d["muted"] = json!(true); }
    Some(d)
}

/// Creates the tab in the state (new identifier, like `newTab`) and returns its id.
fn add_tab(tabs: &mut Obj, data: &Value) -> String {
    let id = new_id();
    let mut t = json!({
        "id": id, "url": data["url"], "title": data["title"], "loading": false, "canGoBack": false, "canGoForward": false,
        "loaded": false, "audible": false, "muted": data.get("muted").and_then(|m| m.as_bool()).unwrap_or(false), "lastActive": now_ms()
    });
    for k in ["favicon", "pinnedUrl", "customTitle", "customIcon"] {
        if let Some(x) = data.get(k) { t[k] = x.clone(); }
    }
    tabs.insert(id.clone(), t);
    id
}

/// Rebuilds items (new tabs); `pinned`: tabs take their "home" address. `folder_name` is the default folder name.
pub fn build_items(list: &Value, tabs: &mut Obj, pinned: bool, depth: usize, folder_name: &str) -> Vec<Value> {
    let Some(arr) = list.as_array().filter(|_| depth <= 8) else { return vec![] };
    let mut out = Vec::new();
    for raw in arr.iter().take(2000) {
        let Some(o) = raw.as_object() else { continue };
        let group = if pinned { String::new() } else { str_of(o.get("group"), 64) };
        match o.get("kind").and_then(|k| k.as_str()) {
            Some("tab") => {
                let Some(mut data) = o.get("tab").and_then(clean_tab) else { continue };
                if pinned && data.get("pinnedUrl").is_none() { data["pinnedUrl"] = data["url"].clone(); }
                let mut v = json!({ "kind": "tab", "tabId": add_tab(tabs, &data) });
                if !group.is_empty() { v["group"] = json!(group); }
                out.push(v);
            }
            Some("split") if o.get("tabs").is_some_and(|t| t.is_array()) => {
                let datas: Vec<Value> = o["tabs"].as_array().into_iter().flatten().filter_map(clean_tab).take(4).collect();
                let ids: Vec<String> = datas.iter().map(|d| add_tab(tabs, d)).collect();
                if ids.len() < 2 { continue; }
                let mut v = json!({ "kind": "split", "id": new_id(), "tabIds": ids });
                if let Some(l) = o.get("layout").and_then(|l| l.as_str()).filter(|l| *l == "rows" || *l == "grid") { v["layout"] = json!(l); }
                if !group.is_empty() { v["group"] = json!(group); }
                out.push(v);
            }
            Some("folder") if pinned => {
                let name = str_of(o.get("name"), 200);
                let mut f = json!({ "kind": "folder", "id": new_id(), "name": if name.is_empty() { folder_name.to_string() } else { name },
                    "open": o.get("open").and_then(|x| x.as_bool()) != Some(false), "items": build_items(o.get("items").unwrap_or(&Value::Null), tabs, true, depth + 1, folder_name) });
                let icon = str_of(o.get("icon"), 16);
                if !icon.is_empty() { f["icon"] = json!(icon); }
                if let Some(h) = o.get("hue").and_then(|h| h.as_f64()).filter(|h| h.is_finite()) { f["hue"] = json!((h.round() as i64) % 360); }
                // Dynamic folder: only the source is kept, revalidated when the state loads.
                if let Some(live) = o.get("live").and_then(|l| l.as_object()) {
                    f["live"] = json!({ "source": live.get("source").cloned().unwrap_or(Value::Null) });
                }
                out.push(f);
            }
            _ => {}
        }
    }
    out
}

/// Addresses of the tabs in a list of items.
pub fn urls_of(items: &Value, tabs: &Obj, into: &mut HashSet<String>) {
    for item in items.as_array().into_iter().flatten() {
        let url_of = |id: &Value| id.as_str().and_then(|i| tabs.get(i)).and_then(|t| t.get("url")).and_then(|u| u.as_str()).unwrap_or("").to_string();
        match item.get("kind").and_then(|k| k.as_str()) {
            Some("tab") => { into.insert(url_of(&item["tabId"])); }
            Some("split") => { for id in item["tabIds"].as_array().into_iter().flatten() { into.insert(url_of(id)); } }
            _ => urls_of(item.get("items").unwrap_or(&Value::Null), tabs, into),
        }
    }
}

fn first_url(item: &Value, tabs: &Obj) -> String {
    let url_of = |id: &Value| id.as_str().and_then(|i| tabs.get(i)).and_then(|t| t.get("url")).and_then(|u| u.as_str()).unwrap_or("").to_string();
    match item.get("kind").and_then(|k| k.as_str()) {
        Some("tab") => url_of(&item["tabId"]),
        Some("split") => item["tabIds"].get(0).map(url_of).unwrap_or_default(),
        _ => format!("folder:{}", item.get("name").and_then(|n| n.as_str()).unwrap_or("")),
    }
}

fn referenced(state: &Obj) -> HashSet<String> {
    let mut ids: HashSet<String> = state.get("favorites").and_then(|f| f.as_array()).into_iter().flatten().filter_map(|x| x.as_str().map(String::from)).collect();
    fn walk(items: &Value, ids: &mut HashSet<String>) {
        for i in items.as_array().into_iter().flatten() {
            match i.get("kind").and_then(|k| k.as_str()) {
                Some("tab") => { if let Some(t) = i["tabId"].as_str() { ids.insert(t.into()); } }
                Some("split") => { for t in i["tabIds"].as_array().into_iter().flatten().filter_map(|x| x.as_str()) { ids.insert(t.into()); } }
                _ => walk(i.get("items").unwrap_or(&Value::Null), ids),
            }
        }
    }
    for s in state.get("spaces").and_then(|s| s.as_array()).into_iter().flatten() {
        walk(s.get("pinned").unwrap_or(&Value::Null), &mut ids);
        walk(s.get("today").unwrap_or(&Value::Null), &mut ids);
    }
    ids
}

// ---------------------------------------------------------------- container

/// Parses a `.signetbackup` file's bytes; `None` if it is not a valid backup of this version or older.
pub fn parse_bytes(buf: &[u8]) -> Option<Value> {
    if buf.len() as u64 > MAX_BYTES {
        return None;
    }
    let text = if buf.starts_with(&[0x1f, 0x8b]) {
        let mut out = Vec::new();
        GzDecoder::new(buf).take(MAX_BYTES + 1).read_to_end(&mut out).ok()?;
        if out.len() as u64 > MAX_BYTES { return None; }
        out
    } else {
        buf.to_vec()
    };
    let file: Value = serde_json::from_slice(&text).ok()?;
    let ok = file.get("format").and_then(|f| f.as_str()) == Some(FORMAT)
        && file.get("version").and_then(|v| v.as_i64()).is_some_and(|v| v <= VERSION)
        && file.get("sections").is_some_and(|s| s.is_object());
    ok.then_some(file)
}

pub fn parse_file(path: &Path) -> Option<Value> {
    parse_bytes(&fs::read(path).ok()?)
}

pub fn to_bytes(file: &Value) -> Result<Vec<u8>> {
    let mut e = GzEncoder::new(Vec::new(), Compression::default());
    e.write_all(file.to_string().as_bytes())?;
    Ok(e.finish()?)
}

/// Writes a backup file atomically (tmp then rename: a failure does not destroy the previous backup).
pub fn write_file(path: &Path, file: &Value) -> Result<()> {
    write_atomic(path, &to_bytes(file)?)
}

fn count_items(items: &Value) -> usize {
    items.as_array().map(|a| a.iter().map(|i| match i.get("kind").and_then(|k| k.as_str()) {
        Some("tab") => 1,
        Some("split") => i.get("tabs").and_then(|t| t.as_array()).map(|t| t.len()).unwrap_or(0),
        _ => count_items(i.get("items").unwrap_or(&Value::Null)),
    }).sum()).unwrap_or(0)
}

/// Number of items in each part, for the checkbox labels (-1 = unknown until decrypted).
pub fn counts(file: &Value) -> Map<String, Value> {
    let s = file.get("sections").and_then(|s| s.as_object()).cloned().unwrap_or_default();
    let mut out = Map::new();
    let len = |k: &str| s.get(k).and_then(|v| v.as_array()).map(|a| a.len());
    if s.contains_key("settings") { out.insert("settings".into(), json!(1)); }
    for k in ["favorites", "spaces", "history", "archive", "notes", "extensions"] {
        if let Some(n) = len(k) { out.insert(k.into(), json!(n)); }
    }
    if let Some(a) = s.get("today").and_then(|v| v.as_array()) {
        out.insert("today".into(), json!(a.iter().map(|x| count_items(x.get("items").unwrap_or(&Value::Null))).sum::<usize>()));
    }
    if let Some(a) = s.get("bookmarks").and_then(|v| v.as_array()) {
        fn walk(l: &Value) -> usize {
            l.as_array().map(|a| a.iter().map(|x| if x.get("kind").and_then(|k| k.as_str()) == Some("folder") { walk(x.get("items").unwrap_or(&Value::Null)) } else if x.is_object() { 1 } else { 0 }).sum()).unwrap_or(0)
        }
        out.insert("bookmarks".into(), json!(a.iter().map(|x| walk(x.get("bookmarks").unwrap_or(&Value::Null))).sum::<usize>()));
    }
    for k in ["boosts", "permissions"] {
        if let Some(o) = s.get(k).and_then(|v| v.as_object()) { out.insert(k.into(), json!(o.len())); }
    }
    if let Some(m) = s.get("mods").and_then(|m| m.get("mods")).and_then(|m| m.as_array()) { out.insert("mods".into(), json!(m.len())); }
    if s.contains_key("passwords") { out.insert("passwords".into(), json!(-1)); }
    if let Some(k) = file.get("keychain").and_then(|k| k.as_array()) { out.insert("passwords".into(), json!(k.len())); }
    out
}

// ---------------------------------------------------------------- collect

/// What the backup needs from the running app.
pub trait Hooks {
    /// Writes to disk everything pending in memory (stores, history, passwords, mods, notes).
    fn flush_all(&self) {}
    /// Installed extensions as `(id, name)`.
    fn extensions(&self) -> Vec<(String, String)> { vec![] }
    /// Decrypted vault logins to seal in the backup (`origin`, `username`, `password`, `createdAt`, `updatedAt`).
    fn plain_logins(&self) -> Vec<Value> { vec![] }
    /// Encrypts a password for `passwords.json` on this machine (base64).
    fn encrypt_password(&self, _plain: &str) -> Option<String> { None }
    /// True if the vault can encrypt.
    fn encryption_available(&self) -> bool { true }
    /// Validated settings (`cleanSettings` of signet-core); `current` is the profile's settings.
    fn clean_settings(&self, incoming: Value, _current: &Value) -> Value { incoming }
    /// Validated space theme (`cleanSpaceDraft` of signet-core), `None` for a solid color.
    fn clean_theme(&self, theme: &Value, _name: &str, _icon: &str, _hue: i64) -> Option<Value> { Some(theme.clone()).filter(|t| t.is_object()) }
    fn app_version(&self) -> String { env!("CARGO_PKG_VERSION").into() }
}

fn read_notes(paths: &Paths) -> Vec<Value> {
    let Ok(rd) = fs::read_dir(paths.notes()) else { return vec![] };
    let mut names: Vec<_> = rd.flatten().filter(|e| e.file_name().to_str().is_some_and(|n| n.len() == 41 && n.ends_with(".json") && n[..36].chars().all(|c| c.is_ascii_hexdigit() || c == '-'))).collect();
    names.sort_by_key(|e| e.file_name());
    names.iter().filter_map(|e| read_json::<Value>(&e.path())).filter(|d| d.get("id").and_then(|i| i.as_str()).is_some_and(is_uuid)).collect()
}

fn shell(s: &Value) -> Value {
    json!({ "id": s.get("id"), "name": s.get("name"), "icon": s.get("icon"), "hue": s.get("hue") })
}

/// Builds the backup of the requested sections from the profile files. Passwords are sealed with `password`.
pub fn collect(paths: &Paths, wanted: &BTreeSet<String>, password: Option<&str>, hooks: &dyn Hooks) -> Result<Value> {
    let state: Value = read_json(&paths.state()).unwrap_or_else(|| json!({}));
    let empty = Obj::new();
    let tabs = state.get("tabs").and_then(|t| t.as_object()).unwrap_or(&empty);
    let spaces: Vec<Value> = state.get("spaces").and_then(|s| s.as_array()).cloned().unwrap_or_default();
    let mut sections = Obj::new();
    let w = |k: &str| wanted.contains(k);
    if w("settings")
        && let Some(s) = state.get("settings").filter(|s| s.is_object()) { sections.insert("settings".into(), s.clone()); }
    if w("favorites") {
        let fav: Vec<Value> = state.get("favorites").and_then(|f| f.as_array()).into_iter().flatten().filter_map(|id| id.as_str().and_then(|i| tab_data(tabs.get(i)))).collect();
        sections.insert("favorites".into(), json!(fav));
    }
    if w("spaces") {
        let list: Vec<Value> = spaces.iter().map(|s| {
            let mut v = shell(s);
            for k in ["theme", "profile"] {
                if let Some(x) = s.get(k).filter(|x| !x.is_null()) { v[k] = x.clone(); }
            }
            v["pinned"] = json!(items_data(s.get("pinned").unwrap_or(&Value::Null), tabs));
            v
        }).collect();
        sections.insert("spaces".into(), json!(list));
    }
    if w("today") {
        let list: Vec<Value> = spaces.iter().map(|s| {
            let mut v = json!({ "space": shell(s), "items": items_data(s.get("today").unwrap_or(&Value::Null), tabs) });
            if let Some(g) = s.get("groups").filter(|g| !g.is_null()) { v["groups"] = g.clone(); }
            v
        }).collect();
        sections.insert("today".into(), json!(list));
    }
    if w("bookmarks") {
        let saved: Value = read_json(&paths.bookmarks()).unwrap_or_else(|| json!({}));
        let list: Vec<Value> = spaces.iter().map(|s| {
            let id = s.get("id").and_then(|i| i.as_str()).unwrap_or("");
            let bm = match saved.get(id) { Some(b) if b.is_array() => b.clone(), _ => s.get("bookmarks").cloned().filter(|b| b.is_array()).unwrap_or(json!([])) };
            json!({ "space": shell(s), "bookmarks": bm })
        }).collect();
        sections.insert("bookmarks".into(), json!(list));
    }
    if w("history") {
        let h: Vec<Value> = read_json::<Vec<Value>>(&paths.history()).unwrap_or_default().into_iter().filter(|x| x.is_object()).collect();
        sections.insert("history".into(), json!(h));
    }
    if w("archive") { sections.insert("archive".into(), state.get("archive").filter(|a| a.is_array()).cloned().unwrap_or(json!([]))); }
    if w("boosts") { sections.insert("boosts".into(), state.get("boosts").filter(|a| a.is_object()).cloned().unwrap_or(json!({}))); }
    if w("permissions") { sections.insert("permissions".into(), state.get("sitePermissions").filter(|a| a.is_object()).cloned().unwrap_or(json!({}))); }
    if w("mods") {
        let m: Value = read_json(&paths.mods()).unwrap_or_else(|| json!({}));
        let list: Vec<Value> = m.get("mods").and_then(|l| l.as_array()).into_iter().flatten().filter(|x| x.is_object()).cloned().collect();
        sections.insert("mods".into(), json!({ "disabled": m.get("disabled").and_then(|d| d.as_bool()) == Some(true), "mods": list }));
    }
    if w("notes") { sections.insert("notes".into(), json!(read_notes(paths))); }
    if w("extensions") {
        sections.insert("extensions".into(), json!(hooks.extensions().into_iter().map(|(id, name)| json!({"id": id, "name": name})).collect::<Vec<_>>()));
    }
    if w("passwords")
        && let Some(pw) = password {
            sections.insert("passwords".into(), serde_json::to_value(seal(&json!(hooks.plain_logins()), pw)?)?);
        }
    Ok(json!({ "format": FORMAT, "version": VERSION, "createdAt": now_ms(), "appVersion": hooks.app_version(), "sections": sections }))
}

/// Parts and their counts for Settings (`info()`); passwords count the vault's logins.
pub fn info(paths: &Paths, hooks: &dyn Hooks) -> Result<Map<String, Value>> {
    hooks.flush_all();
    let all: BTreeSet<String> = BACKUP_SECTIONS.iter().map(|s| s.to_string()).collect();
    let mut c = counts(&collect(paths, &all, None, hooks)?);
    let n = read_json::<Value>(&paths.passwords()).and_then(|v| v.get("logins").and_then(|l| l.as_array()).map(|l| l.len())).unwrap_or(0);
    c.insert("passwords".into(), json!(n));
    Ok(c)
}

// ---------------------------------------------------------------- export

/// Exports the ticked sections to `dest`; the caller has already authenticated the user when passwords are included.
/// Returns an error key or `None`.
pub fn export_to(paths: &Paths, dest: &Path, sections: &[String], password: Option<&str>, hooks: &dyn Hooks) -> Option<&'static str> {
    let wanted: BTreeSet<String> = sections.iter().filter(|s| BACKUP_SECTIONS.contains(&s.as_str())).cloned().collect();
    if wanted.is_empty() {
        return Some("backup.error.nothing");
    }
    if wanted.contains("passwords") && password.is_none_or(|p| p.chars().count() < 6) {
        return Some("backup.error.passwordShort");
    }
    hooks.flush_all();
    match collect(paths, &wanted, password, hooks).and_then(|f| write_file(dest, &f)) {
        Ok(()) => None,
        Err(_) => Some("backup.error.write"),
    }
}

/// Default file name: `<name> YYYY-MM-DD.signetbackup`.
pub fn default_file_name(base: &str, now: chrono::NaiveDate) -> String {
    format!("{base} {}.signetbackup", now.format("%Y-%m-%d"))
}

// ---------------------------------------------------------------- safety copies

/// Writes the safety copy of the current state before an import.
pub fn write_safety(paths: &Paths, hooks: &dyn Hooks) -> Result<()> {
    let dir = paths.safety();
    fs::create_dir_all(&dir)?;
    let all: BTreeSet<String> = BACKUP_SECTIONS.iter().filter(|s| **s != "passwords").map(|s| s.to_string()).collect();
    let mut file = collect(paths, &all, None, hooks)?;
    if let Some(saved) = read_json::<Value>(&paths.passwords()).and_then(|v| v.get("logins").and_then(|l| l.as_array().cloned())).filter(|l| !l.is_empty()) {
        file["keychain"] = json!(saved.into_iter().filter(|x| x.is_object()).collect::<Vec<_>>());
    }
    let stamp = now_ms();
    write_file(&dir.join(format!("safety-{stamp}.signetbackup")), &file)?;
    if paths.passwords().exists() {
        fs::copy(paths.passwords(), dir.join(format!("passwords-{stamp}.json")))?;
        let mut olds: Vec<(u64, PathBuf)> = fs::read_dir(&dir)?.flatten().filter_map(|e| {
            let n = e.file_name().into_string().ok()?;
            let num = n.strip_prefix("passwords-")?.strip_suffix(".json")?.parse::<u64>().ok()?;
            Some((num, e.path()))
        }).collect();
        olds.sort_by_key(|a| std::cmp::Reverse(a.0));
        for (_, p) in olds.into_iter().skip(SAFETY_KEEP) { let _ = fs::remove_file(p); }
    }
    for old in safety_list(paths).into_iter().skip(SAFETY_KEEP) {
        let _ = fs::remove_file(dir.join(old.0));
    }
    Ok(())
}

/// Safety copies, newest first: `(file name, createdAt)`.
pub fn safety_list(paths: &Paths) -> Vec<(String, i64)> {
    let Ok(rd) = fs::read_dir(paths.safety()) else { return vec![] };
    let mut v: Vec<(String, i64)> = rd.flatten().filter_map(|e| {
        let n = e.file_name().into_string().ok()?;
        let ts = n.strip_prefix("safety-")?.strip_suffix(".signetbackup")?.parse::<i64>().ok()?;
        Some((n, ts))
    }).collect();
    v.sort_by_key(|a| std::cmp::Reverse(a.1));
    v
}

/// Reads a safety copy chosen from the list (by its file name).
pub fn open_safety(paths: &Paths, name: &str) -> Option<Value> {
    let ok = name.strip_prefix("safety-").and_then(|r| r.strip_suffix(".signetbackup")).is_some_and(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()));
    if !ok { return None; }
    parse_file(&paths.safety().join(name))
}

// ---------------------------------------------------------------- import

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Add,
    Replace,
}

/// Checks a backup read from disk against the wanted sections and decrypts the passwords when needed.
/// Returns `(wanted sections, plain logins, keychain entries)` or an error key.
pub struct Prepared {
    pub wanted: BTreeSet<String>,
    pub logins: Option<Vec<Value>>,
    pub keychain: Option<Vec<Value>>,
}

pub fn prepare(file: &Value, sections: &[String], password: Option<&str>, hooks: &dyn Hooks) -> std::result::Result<Prepared, &'static str> {
    let secs = file.get("sections").and_then(|s| s.as_object()).ok_or("backup.error.invalid")?;
    let wanted: BTreeSet<String> = sections.iter().filter(|s| BACKUP_SECTIONS.contains(&s.as_str()) && secs.contains_key(s.as_str())).cloned().collect();
    if wanted.is_empty() {
        return Err("backup.error.nothing");
    }
    let (mut logins, mut keychain) = (None, None);
    if wanted.contains("passwords") {
        if let Some(k) = file.get("keychain").and_then(|k| k.as_array()) {
            keychain = Some(k.iter().filter(|l| l.get("origin").is_some_and(|o| o.is_string()) && l.get("password").is_some_and(|p| p.is_string())).cloned().collect());
        } else if let Some(s) = secs.get("passwords") {
            let sealed: Sealed = serde_json::from_value(s.clone()).map_err(|_| "backup.error.wrongPassword")?;
            let plain = unseal(&sealed, password.unwrap_or("")).ok_or("backup.error.wrongPassword")?;
            let arr = plain.as_array().ok_or("backup.error.wrongPassword")?;
            logins = Some(arr.iter().filter(|l| l.is_object()).map(|l| json!({
                "origin": str_of(l.get("origin"), 4000), "username": str_of(l.get("username"), 1000), "password": str_of(l.get("password"), 1000),
                "createdAt": l.get("createdAt"), "updatedAt": l.get("updatedAt"),
            })).collect());
            if !hooks.encryption_available() {
                return Err("backup.error.encryption");
            }
        }
    }
    Ok(Prepared { wanted, logins, keychain })
}

fn read_obj_or_empty(path: &Path) -> Value {
    read_json::<Value>(path).filter(|v| v.is_object()).unwrap_or_else(|| json!({}))
}

/// Applies the sections to the profile files. The app relaunches afterwards (nothing in memory may overwrite it).
#[allow(clippy::too_many_lines)]
pub fn apply_sections(paths: &Paths, file: &Value, prepared: &Prepared, mode: Mode, hooks: &dyn Hooks, t: Translate) -> Result<()> {
    let s = file.get("sections").and_then(|s| s.as_object()).cloned().unwrap_or_default();
    let wanted = &prepared.wanted;
    let replace = mode == Mode::Replace;
    let mut state = read_obj_or_empty(&paths.state());
    let st = state.as_object_mut().ok_or_else(|| Error::Msg("state".into()))?;
    if !st.get("tabs").is_some_and(|t| t.is_object()) { st.insert("tabs".into(), json!({})); }
    if !st.get("spaces").is_some_and(|t| t.is_array()) { st.insert("spaces".into(), json!([])); }
    if !st.get("favorites").is_some_and(|t| t.is_array()) { st.insert("favorites".into(), json!([])); }
    let mut tabs: Obj = st.remove("tabs").and_then(|t| if let Value::Object(o) = t { Some(o) } else { None }).unwrap_or_default();
    let mut spaces: Vec<Value> = st.remove("spaces").and_then(|t| if let Value::Array(a) = t { Some(a) } else { None }).unwrap_or_default();
    let mut favorites: Vec<Value> = st.remove("favorites").and_then(|t| if let Value::Array(a) = t { Some(a) } else { None }).unwrap_or_default();
    let folder_name = t("defaults.folder", &[]);
    let space_name = t("defaults.space", &[]);

    // Settings: always replaced; unpacked extension folders never come from a backup.
    if wanted.contains("settings")
        && let Some(incoming) = s.get("settings").filter(|x| x.is_object()) {
            let current = st.get("settings").cloned().filter(|x| x.is_object()).unwrap_or_else(|| json!({}));
            let mut inc = incoming.clone();
            if let Some(o) = inc.as_object_mut() { o.remove("unpackedExtensions"); }
            let mut cleaned = hooks.clean_settings(inc, &current);
            if let (Some(c), Some(keep)) = (cleaned.as_object_mut(), current.get("unpackedExtensions")) { c.insert("unpackedExtensions".into(), keep.clone()); }
            st.insert("settings".into(), cleaned);
        }

    if wanted.contains("favorites")
        && let Some(list) = s.get("favorites").and_then(|f| f.as_array()) {
            let have: HashSet<String> = if replace { HashSet::new() } else { favorites.iter().filter_map(|id| id.as_str().and_then(|i| tabs.get(i)).and_then(|t| t.get("url")).and_then(|u| u.as_str()).map(String::from)).collect() };
            let mut added: Vec<Value> = Vec::new();
            for d in list.iter().filter_map(clean_tab) {
                if have.contains(d["url"].as_str().unwrap_or("")) { continue; }
                let mut d = d;
                if d.get("pinnedUrl").is_none() { d["pinnedUrl"] = d["url"].clone(); }
                added.push(json!(add_tab(&mut tabs, &d)));
            }
            favorites = if replace { added } else { favorites.into_iter().chain(added).collect() };
        }

    if wanted.contains("spaces")
        && let Some(list) = s.get("spaces").and_then(|l| l.as_array()) {
            let mut next: Vec<Value> = if replace { vec![] } else { spaces.clone() };
            for raw in list.iter().filter(|x| x.is_object()) {
                let id = raw.get("id").and_then(|i| i.as_str()).filter(|i| is_uuid(i)).map(String::from).unwrap_or_else(new_id);
                let existing_ix = spaces.iter().position(|sp| sp.get("id").and_then(|i| i.as_str()) == Some(&id));
                let pinned = build_items(raw.get("pinned").unwrap_or(&Value::Null), &mut tabs, true, 0, &folder_name);
                if let (Some(_), false) = (existing_ix, replace) {
                    let pos = next.iter().position(|sp| sp.get("id").and_then(|i| i.as_str()) == Some(&id));
                    if let Some(pos) = pos {
                        let mut have = HashSet::new();
                        urls_of(next[pos].get("pinned").unwrap_or(&Value::Null), &tabs, &mut have);
                        let add: Vec<Value> = pinned.into_iter().filter(|i| !have.contains(&first_url(i, &tabs))).collect();
                        if let Some(arr) = next[pos].get_mut("pinned").and_then(|p| p.as_array_mut()) { arr.extend(add); }
                    }
                    continue;
                }
                let name = { let n = str_of(raw.get("name"), 40); if n.is_empty() { space_name.clone() } else { n } };
                let icon = { let i = str_of(raw.get("icon"), 16); if i.is_empty() { "\u{2728}".to_string() } else { i } };
                let hue = raw.get("hue").and_then(|h| h.as_f64()).filter(|h| h.is_finite()).map(|h| (h.round() as i64) % 360).unwrap_or(250);
                let mut space = existing_ix.map(|i| spaces[i].clone()).filter(|x| x.is_object()).unwrap_or_else(|| json!({}));
                space["id"] = json!(id);
                space["name"] = json!(name);
                space["icon"] = json!(icon);
                space["hue"] = json!(hue);
                space["pinned"] = json!(pinned);
                if space.get("today").is_none() { space["today"] = json!([]); }
                space["activeTabId"] = Value::Null;
                let theme = raw.get("theme").filter(|t| t.is_object()).and_then(|th| hooks.clean_theme(th, &name, &icon, hue));
                match theme { Some(th) => space["theme"] = th, None => { if let Some(o) = space.as_object_mut() { o.remove("theme"); } } }
                if raw.get("profile").and_then(|p| p.as_str()) == Some("isolated") { space["profile"] = json!("isolated"); } else if let Some(o) = space.as_object_mut() { o.remove("profile"); }
                next.push(space);
            }
            if !next.is_empty() { spaces = next; }
        }
    let active = st.get("activeSpaceId").and_then(|a| a.as_str()).map(String::from);
    if !spaces.iter().any(|sp| sp.get("id").and_then(|i| i.as_str()) == active.as_deref()) && wanted.contains("spaces")
        && let Some(first) = spaces.first().and_then(|sp| sp.get("id")).cloned() { st.insert("activeSpaceId".into(), first); }

    let target = |shell: &Value, spaces: &[Value], active: &Option<String>| -> Option<usize> {
        let sid = shell.get("id").and_then(|i| i.as_str());
        let sname = shell.get("name").and_then(|i| i.as_str());
        spaces.iter().position(|sp| sid.is_some() && sp.get("id").and_then(|i| i.as_str()) == sid)
            .or_else(|| spaces.iter().position(|sp| sname.is_some() && sp.get("name").and_then(|i| i.as_str()) == sname))
            .or_else(|| spaces.iter().position(|sp| sp.get("id").and_then(|i| i.as_str()) == active.as_deref()))
            .or(if spaces.is_empty() { None } else { Some(0) })
    };
    let active_now = st.get("activeSpaceId").and_then(|a| a.as_str()).map(String::from);

    if wanted.contains("today")
        && let Some(list) = s.get("today").and_then(|l| l.as_array()) {
            let mut cleared = HashSet::new();
            for entry in list.iter().filter(|x| x.is_object()) {
                let Some(ix) = target(entry.get("space").unwrap_or(&Value::Null), &spaces, &active_now) else { continue };
                if replace && cleared.insert(ix) { spaces[ix]["today"] = json!([]); }
                let mut have = HashSet::new();
                urls_of(spaces[ix].get("today").unwrap_or(&Value::Null), &tabs, &mut have);
                let built = build_items(entry.get("items").unwrap_or(&Value::Null), &mut tabs, false, 0, &folder_name);
                let add: Vec<Value> = built.into_iter().filter(|i| !have.contains(&first_url(i, &tabs))).collect();
                if !spaces[ix].get("today").is_some_and(|t| t.is_array()) { spaces[ix]["today"] = json!([]); }
                if let Some(arr) = spaces[ix]["today"].as_array_mut() { arr.extend(add); }
                if let Some(g) = entry.get("groups").and_then(|g| g.as_object()) {
                    let mut merged = spaces[ix].get("groups").and_then(|g| g.as_object()).cloned().unwrap_or_default();
                    for (k, v) in g { merged.insert(k.clone(), v.clone()); }
                    spaces[ix]["groups"] = Value::Object(merged);
                }
            }
        }

    // Tabs that nothing references anymore (replaced) leave the state.
    st.insert("favorites".into(), json!(favorites));
    let mut tmp_state = Obj::new();
    tmp_state.insert("favorites".into(), json!(st["favorites"]));
    tmp_state.insert("spaces".into(), json!(spaces));
    let used = referenced(&tmp_state);
    tabs.retain(|id, _| used.contains(id));
    for sp in spaces.iter_mut() {
        if let Some(a) = sp.get("activeTabId").and_then(|a| a.as_str())
            && !used.contains(a) { sp["activeTabId"] = Value::Null; }
    }

    if wanted.contains("archive")
        && let Some(list) = s.get("archive").and_then(|a| a.as_array()) {
            let incoming: Vec<Value> = list.iter().filter(|a| a.get("url").and_then(|u| u.as_str()).is_some_and(is_webish)).cloned().collect();
            let current: Vec<Value> = if replace { vec![] } else { st.get("archive").and_then(|a| a.as_array()).into_iter().flatten().filter(|x| x.is_object()).cloned().collect() };
            let key = |a: &Value| a.get("id").and_then(|i| i.as_str()).map(String::from).unwrap_or_else(|| format!("{}:{}", a.get("archivedAt").map(|x| x.to_string()).unwrap_or_default(), a.get("url").and_then(|u| u.as_str()).unwrap_or("")));
            let have: HashSet<String> = current.iter().map(key).collect();
            let mut all: Vec<Value> = current.into_iter().chain(incoming.into_iter().filter(|a| !have.contains(&key(a)))).collect();
            all.sort_by(|a, b| b.get("archivedAt").and_then(|x| x.as_f64()).unwrap_or(0.0).partial_cmp(&a.get("archivedAt").and_then(|x| x.as_f64()).unwrap_or(0.0)).unwrap_or(std::cmp::Ordering::Equal));
            all.truncate(500);
            st.insert("archive".into(), json!(all));
        }
    for (sec, key) in [("boosts", "boosts"), ("permissions", "sitePermissions")] {
        if wanted.contains(sec)
            && let Some(inc) = s.get(sec).and_then(|b| b.as_object()) {
                let merged = if replace { inc.clone() } else {
                    let mut m = inc.clone();
                    if let Some(cur) = st.get(key).and_then(|c| c.as_object()) { for (k, v) in cur { m.insert(k.clone(), v.clone()); } }
                    m
                };
                st.insert(key.into(), Value::Object(merged));
            }
    }

    // Bookmarks: bookmarks.json, per space.
    if wanted.contains("bookmarks")
        && let Some(list) = s.get("bookmarks").and_then(|b| b.as_array()) {
            let mut saved = read_obj_or_empty(&paths.bookmarks());
            let mut cleared: HashSet<String> = HashSet::new();
            for entry in list.iter().filter(|x| x.is_object()) {
                let Some(ix) = target(entry.get("space").unwrap_or(&Value::Null), &spaces, &active_now) else { continue };
                let Some(bms) = entry.get("bookmarks").and_then(|b| b.as_array()) else { continue };
                let sid = spaces[ix].get("id").and_then(|i| i.as_str()).unwrap_or("").to_string();
                let current: Vec<Value> = if replace && !cleared.contains(&sid) { vec![] } else { saved.get(&sid).and_then(|c| c.as_array()).cloned().unwrap_or_default() };
                cleared.insert(sid.clone());
                let key_of = |b: &Value| if b.get("kind").and_then(|k| k.as_str()) == Some("folder") { format!("folder:{}", b.get("name").and_then(|n| n.as_str()).unwrap_or("")) } else { str_of(b.get("url"), 8000) };
                let have: HashSet<String> = current.iter().map(key_of).collect();
                let add: Vec<Value> = bms.iter().filter(|b| b.is_object() && !have.contains(&key_of(b))).cloned().collect();
                saved[&sid] = json!(current.into_iter().chain(add).collect::<Vec<_>>());
                if let Some(o) = spaces[ix].as_object_mut() { o.remove("bookmarks"); }
            }
            write_json(&paths.bookmarks(), &saved)?;
        }

    st.insert("tabs".into(), Value::Object(tabs));
    st.insert("spaces".into(), json!(spaces));
    write_json(&paths.state(), &state)?;

    if wanted.contains("history")
        && let Some(list) = s.get("history").and_then(|h| h.as_array()) {
            let current: Vec<Value> = if replace { vec![] } else { read_json::<Vec<Value>>(&paths.history()).unwrap_or_default().into_iter().filter(|x| x.is_object()).collect() };
            let mut order: Vec<String> = Vec::new();
            let mut by_url: HashMap<String, Value> = HashMap::new();
            for h in current { let u = str_of(h.get("url"), 100_000); order.push(u.clone()); by_url.insert(u, h); }
            for h in list.iter().filter(|h| h.get("url").and_then(|u| u.as_str()).is_some_and(is_webish)) {
                let url = h["url"].as_str().unwrap_or("").to_string();
                let visits = h.get("visits").and_then(|v| v.as_f64()).map(|v| v as i64).filter(|v| *v > 0).unwrap_or(1);
                let typed = h.get("typed").and_then(|v| v.as_f64()).filter(|v| *v > 0.0).map(|v| v.floor() as i64);
                let last = h.get("lastVisit").and_then(|v| v.as_i64()).unwrap_or(0);
                if let Some(mine) = by_url.get_mut(&url) {
                    let mv = mine.get("visits").and_then(|v| v.as_i64()).unwrap_or(0);
                    let mt = mine.get("typed").and_then(|v| v.as_i64()).unwrap_or(0);
                    let ml = mine.get("lastVisit").and_then(|v| v.as_i64()).unwrap_or(0);
                    mine["visits"] = json!(mv + visits);
                    if mt > 0 || typed.is_some() { mine["typed"] = json!(mt + typed.unwrap_or(0)); }
                    mine["lastVisit"] = json!(ml.max(last));
                } else {
                    let mut e = json!({ "url": url, "title": str_of(h.get("title"), 1000), "visits": visits, "lastVisit": last });
                    let fav = str_of(h.get("favicon"), 200_000);
                    if !fav.is_empty() { e["favicon"] = json!(fav); }
                    if let Some(tp) = typed { e["typed"] = json!(tp); }
                    order.push(url.clone());
                    by_url.insert(url, e);
                }
            }
            let mut all: Vec<Value> = order.iter().filter_map(|u| by_url.remove(u)).collect();
            all.sort_by(|a, b| b.get("lastVisit").and_then(|x| x.as_i64()).unwrap_or(0).cmp(&a.get("lastVisit").and_then(|x| x.as_i64()).unwrap_or(0)));
            all.truncate(5000);
            write_json(&paths.history(), &json!(all))?;
        }

    if wanted.contains("mods")
        && let Some(m) = s.get("mods").and_then(|m| m.as_object()).filter(|m| m.get("mods").is_some_and(|x| x.is_array())) {
            let current = if replace { json!({"disabled": false, "mods": []}) } else { read_obj_or_empty(&paths.mods()) };
            let list: Vec<Value> = current.get("mods").and_then(|l| l.as_array()).cloned().unwrap_or_default();
            let key = |m: &Value| format!("{}\n{}", m.get("name").and_then(|n| n.as_str()).unwrap_or("undefined"), m.get("css").and_then(|n| n.as_str()).unwrap_or("undefined"));
            let have: HashSet<String> = list.iter().map(key).collect();
            let added: Vec<Value> = m["mods"].as_array().into_iter().flatten().filter(|x| x.get("css").is_some_and(|c| c.is_string()) && !have.contains(&key(x)))
                .map(|x| { let n = str_of(x.get("name"), 200); json!({"id": new_id(), "name": if n.is_empty() { "Mod".to_string() } else { n }, "css": str_of(x.get("css"), 500_000), "enabled": x.get("enabled").and_then(|e| e.as_bool()) != Some(false)}) }).collect();
            let disabled = if replace { m.get("disabled").and_then(|d| d.as_bool()) == Some(true) } else { current.get("disabled").and_then(|d| d.as_bool()) == Some(true) };
            write_json(&paths.mods(), &json!({"version": 1, "disabled": disabled, "mods": list.into_iter().chain(added).collect::<Vec<_>>()}))?;
        }

    if wanted.contains("notes")
        && let Some(list) = s.get("notes").and_then(|n| n.as_array()) {
            let dir = paths.notes();
            fs::create_dir_all(&dir)?;
            for doc in list {
                let Some(id) = doc.get("id").and_then(|i| i.as_str()).filter(|i| is_uuid(i)) else { continue };
                if !matches!(doc.get("kind").and_then(|k| k.as_str()), Some("note" | "easel")) { continue; }
                let path = dir.join(format!("{}.json", id.to_lowercase()));
                if !replace && path.exists() { continue; }
                write_json(&path, doc)?;
            }
        }

    if let Some(logins) = &prepared.logins {
        let raw = if replace { json!({"logins": []}) } else { read_obj_or_empty(&paths.passwords()) };
        let mut list: Vec<Value> = raw.get("logins").and_then(|l| l.as_array()).map(|l| l.iter().filter(|x| x.is_object()).cloned().collect()).unwrap_or_default();
        let key = |o: &Value, u: &Value| format!("{}\n{}", o.as_str().unwrap_or(""), u.as_str().unwrap_or(""));
        let mut have: HashSet<String> = list.iter().map(|l| key(&l["origin"], &l["username"])).collect();
        let now = now_ms();
        let origin_ok = regex::Regex::new(r"(?i)^https?://[^/]+$").map_err(|e| Error::Msg(e.to_string()))?;
        for l in logins {
            let origin = l["origin"].as_str().unwrap_or("");
            let pw = l["password"].as_str().unwrap_or("");
            let k = key(&l["origin"], &l["username"]);
            if !origin_ok.is_match(origin) || pw.is_empty() || have.contains(&k) { continue; }
            have.insert(k);
            let Some(enc) = hooks.encrypt_password(pw) else { continue };
            list.push(json!({ "id": new_id(), "origin": origin, "username": l["username"], "password": enc,
                "createdAt": l.get("createdAt").and_then(|c| c.as_i64()).filter(|c| *c > 0).unwrap_or(now), "updatedAt": l.get("updatedAt").and_then(|c| c.as_i64()).filter(|c| *c > 0).unwrap_or(now) }));
        }
        write_json(&paths.passwords(), &json!({"version": 1, "logins": list}))?;
    }

    if let Some(keychain) = &prepared.keychain {
        let current: Vec<Value> = if replace { vec![] } else { read_obj_or_empty(&paths.passwords()).get("logins").and_then(|l| l.as_array()).map(|l| l.iter().filter(|x| x.is_object()).cloned().collect()).unwrap_or_default() };
        let key = |l: &Value| format!("{}\n{}", l.get("origin").and_then(|o| o.as_str()).unwrap_or(""), l.get("username").and_then(|o| o.as_str()).unwrap_or(""));
        let have: HashSet<String> = current.iter().map(key).collect();
        let added: Vec<Value> = keychain.iter().filter(|l| !have.contains(&key(l))).cloned().collect();
        write_json(&paths.passwords(), &json!({"version": 1, "logins": current.into_iter().chain(added).collect::<Vec<_>>()}))?;
    }

    // Extensions: installed at the next launch (network), from the store.
    if wanted.contains("extensions")
        && let Some(list) = s.get("extensions").and_then(|e| e.as_array()) {
            let re = regex::Regex::new(r"^[a-p]{32}$").map_err(|e| Error::Msg(e.to_string()))?;
            let ids: Vec<String> = list.iter().map(|e| str_of(e.get("id"), 64)).filter(|id| re.is_match(id)).collect();
            if !ids.is_empty() { write_json(&paths.pending_extensions(), &json!(ids))?; }
        }
    Ok(())
}

/// Full import: safety copy, then apply. Returns an error key or `None`; the caller relaunches the app.
pub fn import(paths: &Paths, file: &Value, prepared: &Prepared, mode: Mode, hooks: &dyn Hooks, t: Translate) -> Option<&'static str> {
    hooks.flush_all();
    if write_safety(paths, hooks).is_err() {
        return Some("backup.error.safety");
    }
    if apply_sections(paths, file, prepared, mode, hooks, t).is_err() {
        return Some("backup.error.import");
    }
    None
}

/// At launch: extension ids left by an import (then forgotten). The app installs the ones that are missing.
pub fn take_pending_extensions(paths: &Paths) -> Vec<String> {
    let Some(Value::Array(ids)) = read_json::<Value>(&paths.pending_extensions()) else { return vec![] };
    let _ = fs::remove_file(paths.pending_extensions());
    ids.into_iter().filter_map(|i| i.as_str().map(String::from)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct H;
    impl Hooks for H {
        fn encrypt_password(&self, p: &str) -> Option<String> { Some(format!("enc:{p}")) }
        fn plain_logins(&self) -> Vec<Value> { vec![json!({"origin":"https://a.com","username":"u","password":"pw\u{e9}\u{20ac}","createdAt":5})] }
        fn extensions(&self) -> Vec<(String, String)> { vec![("a".repeat(32), "Ext".into())] }
    }
    fn tr(k: &str, _: &[(&str, String)]) -> String { match k { "defaults.folder" => "New folder".into(), "defaults.space" => "Space".into(), _ => k.into() } }
    fn tab(id: &str) -> Value { json!({"id": id, "url": format!("https://{id}.com"), "title": id.to_uppercase()}) }
    fn s(v: &[&str]) -> Vec<String> { v.iter().map(|x| x.to_string()).collect() }

    #[test]
    fn seal_roundtrip_and_failures() {
        let value = json!({"logins":[{"origin":"https://a.com","username":"u","password":"\u{e9}\u{20ac}"}]});
        let sealed = seal(&value, "correct horse").unwrap();
        assert_eq!(sealed.kdf, "scrypt");
        assert!(!serde_json::to_string(&sealed).unwrap().contains("a.com"));
        assert_eq!(unseal(&sealed, "correct horse"), Some(value));
        assert_eq!(unseal(&sealed, "autre"), None);
        assert_eq!(unseal(&Sealed { data: b64().encode("x"), ..sealed.clone() }, "correct horse"), None);
        assert_eq!(unseal(&Sealed { tag: b64().encode([0u8; 16]), ..sealed.clone() }, "correct horse"), None);
        let a = seal(&json!("the same secret, encrypted twice"), "pw").unwrap();
        let b = seal(&json!("the same secret, encrypted twice"), "pw").unwrap();
        assert!(a.salt != b.salt && a.iv != b.iv && a.data != b.data);
    }

    #[test]
    fn tab_and_items_data() {
        assert_eq!(tab_data(None), None);
        assert_eq!(tab_data(Some(&json!({"id":"a","url":"","title":"A"}))), None);
        assert_eq!(tab_data(Some(&tab("a"))), Some(json!({"url":"https://a.com","title":"A"})));
        let full = json!({"url":"https://a.com","title":"A","favicon":"f","pinnedUrl":"p","customTitle":"c","customIcon":"i","muted":true});
        assert_eq!(tab_data(Some(&full)), Some(full.clone()));
        let tabs: Obj = ["a", "b", "c"].iter().map(|i| (i.to_string(), tab(i))).collect();
        let items = json!([
            {"kind":"tab","tabId":"a","group":"g"}, {"kind":"tab","tabId":"ghost"},
            {"kind":"split","id":"s","tabIds":["b","ghost"],"layout":"rows"},
            {"kind":"split","id":"s2","tabIds":["b","c"],"layout":"rows","sizes":[1,2]},
            {"kind":"folder","id":"f","name":"F","open":false,"items":[{"kind":"tab","tabId":"c"}]}
        ]);
        let out = items_data(&items, &tabs);
        assert_eq!(out.len(), 3);
        assert_eq!(out[0], json!({"kind":"tab","tab":{"url":"https://a.com","title":"A"},"group":"g"}));
        assert_eq!((out[1]["layout"].clone(), out[1]["sizes"].clone()), (json!("rows"), json!([1, 2])));
        assert_eq!((out[2]["name"].clone(), out[2]["open"].clone()), (json!("F"), json!(false)));
        assert!(!serde_json::to_string(&out).unwrap().contains("tabId"));
    }

    #[test]
    fn clean_tab_rules() {
        assert_eq!(clean_tab(&json!(null)), None);
        assert_eq!(clean_tab(&json!({})), None);
        for u in ["javascript:alert(1)", "data:text/html,x", "chrome://gpu"] { assert_eq!(clean_tab(&json!({"url": u})), None, "{u}"); }
        assert_eq!(clean_tab(&json!({"url":"signet://settings"})).unwrap()["title"], "");
        assert!(clean_tab(&json!({"url":"file:///x"})).is_some());
        let t = clean_tab(&json!({"url":"https://a.com","title":"t".repeat(2000),"customTitle":"c".repeat(300),"customIcon":"i".repeat(40),"muted":"yes","pinnedUrl":"javascript:1"})).unwrap();
        assert_eq!(t["title"].as_str().unwrap().len(), 1000);
        assert_eq!(t["customTitle"].as_str().unwrap().len(), 200);
        assert_eq!(t["customIcon"].as_str().unwrap().len(), 16);
        assert!(t.get("muted").is_none() && t.get("pinnedUrl").is_none());
        let p = clean_tab(&json!({"url":"https://a.com","pinnedUrl":"https://p.com","muted":true})).unwrap();
        assert_eq!((p["pinnedUrl"].clone(), p["muted"].clone()), (json!("https://p.com"), json!(true)));
    }

    #[test]
    fn build_items_rules() {
        let data = |u: &str| json!({"url": u, "title": u});
        let mut tabs = Obj::new();
        let items = build_items(&json!([{"kind":"tab","tab":data("https://a.com"),"group":"g"}]), &mut tabs, false, 0, "F");
        assert_eq!(items[0]["group"], "g");
        let id = items[0]["tabId"].as_str().unwrap();
        assert_eq!((tabs[id]["url"].clone(), tabs[id]["muted"].clone()), (json!("https://a.com"), json!(false)));

        let mut tabs = Obj::new();
        let items = build_items(&json!([
            {"kind":"tab","tab":data("https://a.com"),"group":"g"},
            {"kind":"folder","name":"","icon":"\u{1F600}","open":false,"hue":400,"live":{"source":{"kind":"github","query":"mine"},"junk":1},"items":[{"kind":"tab","tab":data("https://b.com")}]}
        ]), &mut tabs, true, 0, "New folder");
        assert!(items[0].get("group").is_none());
        assert_eq!(tabs[items[0]["tabId"].as_str().unwrap()]["pinnedUrl"], "https://a.com");
        let f = &items[1];
        assert_eq!((f["name"].clone(), f["icon"].clone(), f["open"].clone(), f["hue"].clone()), (json!("New folder"), json!("\u{1F600}"), json!(false), json!(40)));
        assert_eq!(f["live"], json!({"source":{"kind":"github","query":"mine"}}));
        assert_eq!(f["items"].as_array().unwrap().len(), 1);

        assert!(build_items(&json!([{"kind":"folder","name":"F","items":[]}]), &mut Obj::new(), false, 0, "F").is_empty());
        let split = |n: usize, layout: Option<&str>| { let mut v = json!({"kind":"split","tabs":(0..n).map(|i| data(&format!("https://{i}.com"))).collect::<Vec<_>>()}); if let Some(l) = layout { v["layout"] = json!(l); } v };
        let mut tabs = Obj::new();
        assert!(build_items(&json!([split(1, None)]), &mut tabs, false, 0, "F").is_empty());
        let r = build_items(&json!([split(6, Some("grid"))]), &mut tabs, false, 0, "F");
        assert_eq!((r[0]["tabIds"].as_array().unwrap().len(), r[0]["layout"].clone()), (4, json!("grid")));
        assert!(build_items(&json!([split(2, Some("bizarre"))]), &mut tabs, false, 0, "F")[0].get("layout").is_none());
        assert!(build_items(&json!([{"kind":"split","tabs":[data("https://a.com"), data("javascript:1")]}]), &mut tabs, false, 0, "F").is_empty());

        assert!(build_items(&json!("x"), &mut Obj::new(), false, 0, "F").is_empty());
        assert!(build_items(&json!([null, 3, {"kind":"tab"}, {"kind":"tab","tab":{"url":"javascript:1"}}, {"kind":"inconnu"}]), &mut Obj::new(), false, 0, "F").is_empty());
        let mut deep = json!([{"kind":"tab","tab":data("https://deep.com")}]);
        for _ in 0..12 { deep = json!([{"kind":"folder","name":"d","items":deep}]); }
        let out = build_items(&deep, &mut Obj::new(), true, 0, "F");
        let (mut depth, mut cur) = (0, out);
        while cur.first().and_then(|f| f.get("kind")) == Some(&json!("folder")) { depth += 1; cur = cur[0]["items"].as_array().cloned().unwrap_or_default(); }
        assert!(depth <= 9 && cur.is_empty());
        let big: Vec<Value> = (0..2500).map(|i| json!({"kind":"tab","tab":data(&format!("https://x.com/{i}"))})).collect();
        assert_eq!(build_items(&json!(big), &mut Obj::new(), false, 0, "F").len(), 2000);
    }

    #[test]
    fn urls_of_collects() {
        let tabs: Obj = ["a", "b", "c"].iter().map(|i| (i.to_string(), tab(i))).collect();
        let items = json!([{"kind":"tab","tabId":"a"},{"kind":"split","id":"s","tabIds":["b","ghost"]},{"kind":"folder","id":"f","name":"F","open":true,"items":[{"kind":"tab","tabId":"c"}]}]);
        let mut set = HashSet::new();
        urls_of(&items, &tabs, &mut set);
        let mut v: Vec<_> = set.into_iter().collect();
        v.sort();
        assert_eq!(v, vec!["", "https://a.com", "https://b.com", "https://c.com"]);
    }

    #[test]
    fn container_parsing() {
        let valid = json!({"format":"signetbackup","version":1,"createdAt":1,"appVersion":"1","sections":{"favorites":[]}});
        assert_eq!(parse_bytes(&to_bytes(&valid).unwrap()), Some(valid.clone()));
        assert_eq!(parse_bytes(valid.to_string().as_bytes()), Some(valid.clone()));
        let with = |k: &str, v: Value| { let mut x = valid.clone(); x[k] = v; x.to_string() };
        assert_eq!(parse_bytes(with("format", json!("autre")).as_bytes()), None);
        assert_eq!(parse_bytes(with("version", json!(2)).as_bytes()), None);
        assert_eq!(parse_bytes(with("version", json!("1")).as_bytes()), None);
        assert_eq!(parse_bytes(with("sections", json!([])).as_bytes()), None);
        assert_eq!(parse_bytes(b"null"), None);
        assert_eq!(parse_bytes(b"{oops"), None);
        assert_eq!(parse_bytes(&[0x1f, 0x8b, 1, 2, 3]), None);
    }

    fn fixture(dir: &Path) {
        let state = json!({
            "tabs": {"t1": tab("t1"), "t2": tab("t2"), "t3": tab("t3")},
            "favorites": ["t1"],
            "spaces": [{"id": "123e4567-e89b-12d3-a456-426614174000", "name": "Work", "icon": "W", "hue": 10, "pinned": [{"kind":"tab","tabId":"t2"}], "today": [{"kind":"tab","tabId":"t3"}], "activeTabId": "t2"}],
            "activeSpaceId": "123e4567-e89b-12d3-a456-426614174000",
            "settings": {"theme": "dark", "unpackedExtensions": ["/x"]},
            "archive": [{"id":"a1","url":"https://old.com","archivedAt": 5}],
            "boosts": {"https://a.com": {}}, "sitePermissions": {"https://a.com": {"camera": "allow"}}
        });
        fs::write(dir.join("state.json"), state.to_string()).unwrap();
        fs::write(dir.join("bookmarks.json"), json!({"123e4567-e89b-12d3-a456-426614174000": [{"kind":"link","id":"b","title":"B","url":"https://b.com"}]}).to_string()).unwrap();
        fs::write(dir.join("history.json"), json!([{"url":"https://h.com","title":"H","visits":2,"lastVisit":9}]).to_string()).unwrap();
        fs::write(dir.join("mods.json"), json!({"version":1,"disabled":false,"mods":[{"id":"m","name":"M","css":"a{}","enabled":true}]}).to_string()).unwrap();
        fs::write(dir.join("passwords.json"), json!({"version":1,"logins":[{"id":"1","origin":"https://a.com","username":"u","password":"x"}]}).to_string()).unwrap();
        fs::create_dir_all(dir.join("notes")).unwrap();
        fs::write(dir.join("notes/123e4567-e89b-12d3-a456-426614174111.json"), json!({"id":"123e4567-e89b-12d3-a456-426614174111","kind":"note","title":"N","html":"","createdAt":1,"updatedAt":1}).to_string()).unwrap();
    }

    #[test]
    fn collect_counts_and_export() {
        let d = tempfile::tempdir().unwrap();
        fixture(d.path());
        let paths = Paths::new(d.path());
        let all: BTreeSet<String> = BACKUP_SECTIONS.iter().map(|s| s.to_string()).collect();
        let f = collect(&paths, &all, Some("secret1"), &H).unwrap();
        assert_eq!(f["format"], "signetbackup");
        let c = counts(&f);
        assert_eq!(c["settings"], 1);
        assert_eq!((c["favorites"].clone(), c["spaces"].clone(), c["today"].clone(), c["bookmarks"].clone()), (json!(1), json!(1), json!(1), json!(1)));
        assert_eq!((c["history"].clone(), c["archive"].clone(), c["boosts"].clone(), c["permissions"].clone(), c["mods"].clone(), c["notes"].clone(), c["extensions"].clone()), (json!(1), json!(1), json!(1), json!(1), json!(1), json!(1), json!(1)));
        assert_eq!(c["passwords"], -1);
        assert_eq!(f["sections"]["spaces"][0]["pinned"][0]["tab"]["url"], "https://t2.com");
        assert!(!f.to_string().contains("pw\u{e9}"));
        assert_eq!(info(&paths, &H).unwrap()["passwords"], 1);

        let dest = d.path().join("out.signetbackup");
        assert_eq!(export_to(&paths, &dest, &s(&["bogus"]), None, &H), Some("backup.error.nothing"));
        assert_eq!(export_to(&paths, &dest, &s(&["passwords"]), Some("short"), &H), Some("backup.error.passwordShort"));
        assert_eq!(export_to(&paths, &dest, &s(&["history", "passwords"]), Some("longenough"), &H), None);
        assert_eq!(&fs::read(&dest).unwrap()[..2], &[0x1f, 0x8b]);
        assert!(parse_file(&dest).is_some());
        assert_eq!(default_file_name("Signet backup", chrono::NaiveDate::from_ymd_opt(2026, 3, 4).unwrap()), "Signet backup 2026-03-04.signetbackup");
    }

    #[test]
    fn restore_into_a_fresh_profile_replace() {
        let src = tempfile::tempdir().unwrap();
        fixture(src.path());
        let all: BTreeSet<String> = BACKUP_SECTIONS.iter().map(|s| s.to_string()).collect();
        let file = collect(&Paths::new(src.path()), &all, Some("secret1"), &H).unwrap();
        let dst = tempfile::tempdir().unwrap();
        let paths = Paths::new(dst.path());
        let prepared = prepare(&file, &s(&BACKUP_SECTIONS), Some("secret1"), &H).unwrap();
        assert_eq!(prepared.logins.as_ref().unwrap().len(), 1);
        assert_eq!(import(&paths, &file, &prepared, Mode::Replace, &H, &tr), None);
        let state: Value = read_json(&paths.state()).unwrap();
        assert_eq!(state["spaces"][0]["name"], "Work");
        assert_eq!(state["spaces"][0]["pinned"].as_array().unwrap().len(), 1);
        let pinned_tab = state["spaces"][0]["pinned"][0]["tabId"].as_str().unwrap();
        assert_eq!(state["tabs"][pinned_tab]["pinnedUrl"], "https://t2.com");
        assert_eq!(state["tabs"].as_object().unwrap().len(), 3);
        assert_eq!(state["favorites"].as_array().unwrap().len(), 1);
        assert_eq!(state["settings"]["theme"], "dark");
        assert_eq!(state["sitePermissions"]["https://a.com"]["camera"], "allow");
        assert_eq!(state["archive"][0]["url"], "https://old.com");
        let bm: Value = read_json(&paths.bookmarks()).unwrap();
        assert_eq!(bm["123e4567-e89b-12d3-a456-426614174000"][0]["url"], "https://b.com");
        let hist: Vec<Value> = read_json(&paths.history()).unwrap();
        assert_eq!(hist[0]["url"], "https://h.com");
        let mods: Value = read_json(&paths.mods()).unwrap();
        assert_eq!(mods["mods"][0]["css"], "a{}");
        let pw: Value = read_json(&paths.passwords()).unwrap();
        assert_eq!(pw["logins"][0]["password"], "enc:pw\u{e9}\u{20ac}");
        assert!(paths.notes().join("123e4567-e89b-12d3-a456-426614174111.json").exists());
        assert_eq!(take_pending_extensions(&paths), vec!["a".repeat(32)]);
        assert!(take_pending_extensions(&paths).is_empty());
    }

    #[test]
    fn add_mode_merges_and_keeps_settings_extensions() {
        let src = tempfile::tempdir().unwrap();
        fixture(src.path());
        let all: BTreeSet<String> = BACKUP_SECTIONS.iter().filter(|s| **s != "passwords").map(|s| s.to_string()).collect();
        let file = collect(&Paths::new(src.path()), &all, None, &H).unwrap();
        let dst = tempfile::tempdir().unwrap();
        fixture(dst.path());
        let mut st: Value = read_json(&dst.path().join("state.json")).unwrap();
        st["settings"] = json!({"unpackedExtensions": ["/mine"], "theme": "light"});
        fs::write(dst.path().join("state.json"), st.to_string()).unwrap();
        let paths = Paths::new(dst.path());
        let prepared = prepare(&file, &s(&BACKUP_SECTIONS), None, &H).unwrap();
        assert_eq!(import(&paths, &file, &prepared, Mode::Add, &H, &tr), None);
        let state: Value = read_json(&paths.state()).unwrap();
        // Same space id: pinned URL already known is not duplicated; today and favorites likewise.
        assert_eq!(state["spaces"].as_array().unwrap().len(), 1);
        assert_eq!(state["spaces"][0]["pinned"].as_array().unwrap().len(), 1);
        assert_eq!(state["favorites"].as_array().unwrap().len(), 1);
        assert_eq!(state["settings"]["theme"], "dark");
        assert_eq!(state["settings"]["unpackedExtensions"], json!(["/mine"]));
        let hist: Vec<Value> = read_json(&paths.history()).unwrap();
        assert_eq!((hist.len(), hist[0]["visits"].clone()), (1, json!(4)));
        assert_eq!(safety_list(&paths).len(), 1);
        let name = safety_list(&paths)[0].0.clone();
        let safety = open_safety(&paths, &name).unwrap();
        assert!(safety["keychain"].as_array().unwrap().len() == 1);
        assert!(open_safety(&paths, "../evil").is_none());
    }

    #[test]
    fn prepare_errors() {
        let src = tempfile::tempdir().unwrap();
        fixture(src.path());
        let wanted: BTreeSet<String> = ["passwords".to_string(), "history".to_string()].into();
        let file = collect(&Paths::new(src.path()), &wanted, Some("secret1"), &H).unwrap();
        assert_eq!(prepare(&file, &s(&["passwords"]), Some("wrong"), &H).err(), Some("backup.error.wrongPassword"));
        assert_eq!(prepare(&file, &s(&["favorites"]), None, &H).err(), Some("backup.error.nothing"));
        assert!(prepare(&file, &s(&["history"]), None, &H).is_ok());
    }

    #[test]
    fn safety_copies_are_pruned() {
        let d = tempfile::tempdir().unwrap();
        fixture(d.path());
        let paths = Paths::new(d.path());
        for _ in 0..5 {
            write_safety(&paths, &H).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(3));
        }
        assert_eq!(safety_list(&paths).len(), SAFETY_KEEP);
        let pw = fs::read_dir(paths.safety()).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().starts_with("passwords-")).count();
        assert_eq!(pw, SAFETY_KEEP);
    }
}

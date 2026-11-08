//! Site permission policy (port of the logic of `main/web/permissions.ts`): decisions per origin, defaults,
//! media grouping, external protocol rules. Dialogs and CEF hooks are the app's job.
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Decision {
    Allow,
    Deny,
}

/// Same shape as `state.sitePermissions`: origin -> permission -> decision.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(transparent)]
pub struct Table(pub BTreeMap<String, BTreeMap<String, Decision>>);

/// Allowed without asking.
pub const FREE: &[&str] = &["clipboard-sanitized-write", "fullscreen", "pointerLock"];
/// Permissions we know how to ask for.
pub const ASKABLE: &[&str] = &["media", "camera", "microphone", "geolocation", "notifications", "clipboard-read", "midi", "display-capture"];
/// Schemes never handed to an external application.
pub const BLOCKED_EXTERNAL: &[&str] =
    &["javascript:", "file:", "data:", "vbscript:", "blob:", "about:", "view-source:", "devtools:", "signet:", "signetui:", "chrome:"];

/// Usable http(s) origin, or `None` (opaque origin `null`, about:, data:, file:...).
pub fn web_origin(value: &str) -> Option<String> {
    if value.is_empty() {
        return None;
    }
    let u = Url::parse(value).ok()?;
    matches!(u.scheme(), "http" | "https").then(|| u.origin().ascii_serialization())
}

/// Origin of a URL, or the raw string if unusable.
pub fn origin_of(url: &str) -> String {
    match Url::parse(url) {
        Ok(u) if u.origin().is_tuple() => u.origin().ascii_serialization(),
        Ok(_) => "null".into(),
        Err(_) => url.to_string(),
    }
}

/// Origin to attach a decision to: first usable candidate, else the top-level page's, else `None` (deny).
pub fn origin_for(candidates: &[&str], top_url: Option<&str>) -> Option<String> {
    candidates.iter().find_map(|c| web_origin(c)).or_else(|| top_url.and_then(web_origin))
}

/// True if an external protocol may be proposed to the user (never web or internal schemes).
pub fn external_allowed(url: &str) -> bool {
    match Url::parse(url) {
        Ok(u) => {
            let p = format!("{}:", u.scheme());
            !(BLOCKED_EXTERNAL.contains(&p.as_str()) || p == "http:" || p == "https:")
        }
        Err(_) => false,
    }
}

/// Concrete permissions targeted by a request (`media` becomes camera and/or microphone).
pub fn names(permission: &str, media_types: &[&str]) -> Vec<String> {
    if permission != "media" {
        return if ASKABLE.contains(&permission) { vec![permission.to_string()] } else { vec![] };
    }
    let mut out: Vec<String> = Vec::new();
    for t in media_types {
        let n = match *t { "video" => "camera", "audio" => "microphone", _ => continue };
        if !out.iter().any(|x| x == n) {
            out.push(n.into());
        }
    }
    if out.is_empty() { vec!["camera".into(), "microphone".into()] } else { out }
}

/// What to do with a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Granted,
    Denied,
    /// Unknown: ask the user.
    Ask,
}

/// State of the persisted table (normal profile) plus the in-memory tables of private windows.
#[derive(Debug, Default)]
pub struct Permissions {
    pub table: Table,
    dirty: bool,
}

impl Permissions {
    pub fn new(table: Table) -> Self {
        Permissions { table, dirty: false }
    }

    /// True (once) if the table changed and should be saved to `state.json`.
    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    /// `media` groups camera and microphone: denied if either is denied, granted if both are.
    /// `private`: ephemeral table of a private window; denials of the normal table still apply.
    pub fn get(&self, origin: &str, permission: &str, private: Option<&Table>) -> Option<Decision> {
        let own = get_in(private.unwrap_or(&self.table), origin, permission);
        if private.is_some() && own != Some(Decision::Deny) && get_in(&self.table, origin, permission) == Some(Decision::Deny) {
            return Some(Decision::Deny);
        }
        own
    }

    /// Records a decision (`None` = ask again, forgets it). `private` writes in the given table instead.
    pub fn set(&mut self, origin: &str, permission: &str, state: Option<Decision>, private: Option<&mut Table>) {
        if origin == "null" {
            return;
        }
        let is_main = private.is_none();
        let table = match private {
            Some(t) => t,
            None => &mut self.table,
        };
        set_in(table, origin, permission, state);
        if is_main {
            self.dirty = true;
        }
    }

    /// All remembered decisions, sorted.
    pub fn list_all(&self) -> Vec<(String, String, Decision)> {
        let mut out: Vec<_> = self.table.0.iter().flat_map(|(o, e)| e.iter().map(move |(p, d)| (o.clone(), p.clone(), *d))).collect();
        out.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        out
    }

    /// Forgets one decision, all of an origin, or everything.
    pub fn reset(&mut self, origin: Option<&str>, permission: Option<&str>) {
        match (origin, permission) {
            (None, _) => self.table.0.clear(),
            (Some(o), None) => {
                self.table.0.remove(o);
            }
            (Some(o), Some(p)) => {
                self.set(o, p, None, None);
                return;
            }
        }
        self.dirty = true;
    }

    pub fn list(&self, origin: &str) -> BTreeMap<String, Decision> {
        self.table.0.get(origin).cloned().unwrap_or_default()
    }

    /// Decision for a request: denied if any known part is denied, granted if all are, else ask.
    pub fn verdict(&self, origin: &str, permission: &str, media_types: &[&str], private: Option<&Table>) -> Verdict {
        if FREE.contains(&permission) {
            return Verdict::Granted;
        }
        let n = names(permission, media_types);
        if n.is_empty() {
            return Verdict::Denied;
        }
        let known: Vec<_> = n.iter().map(|x| self.get(origin, x, private)).collect();
        if known.contains(&Some(Decision::Deny)) {
            Verdict::Denied
        } else if known.iter().all(|k| *k == Some(Decision::Allow)) {
            Verdict::Granted
        } else {
            Verdict::Ask
        }
    }

    /// Synchronous permission check (no prompt): free permissions, or every part already allowed.
    pub fn check(&self, origin: Option<&str>, permission: &str, media_type: Option<&str>, private: Option<&Table>) -> bool {
        if FREE.contains(&permission) {
            return true;
        }
        let Some(origin) = origin else { return false };
        let mt: Vec<&str> = media_type.into_iter().collect();
        let n = names(permission, &mt);
        !n.is_empty() && n.iter().all(|x| self.get(origin, x, private) == Some(Decision::Allow))
    }

    /// Dialog message key for a request: `media` when both camera and microphone are asked.
    pub fn dialog_key(names: &[String]) -> String {
        if names.len() == 2 && names.iter().any(|n| n == "camera") { "media".into() } else { names.first().cloned().unwrap_or_default() }
    }
}

fn get_in(table: &Table, origin: &str, permission: &str) -> Option<Decision> {
    let entry = table.0.get(origin)?;
    if permission == "media" {
        let states = [entry.get("camera").or(entry.get("media")), entry.get("microphone").or(entry.get("media"))];
        if states.contains(&Some(&Decision::Deny)) {
            return Some(Decision::Deny);
        }
        return states.iter().all(|s| *s == Some(&Decision::Allow)).then_some(Decision::Allow);
    }
    entry.get(permission).copied()
}

fn set_in(table: &mut Table, origin: &str, permission: &str, state: Option<Decision>) {
    let list: &[&str] = if permission == "media" { &["camera", "microphone"] } else { &[] };
    let entry = table.0.entry(origin.to_string()).or_default();
    let mut apply = |name: &str| match state {
        None => {
            entry.remove(name);
        }
        Some(d) => {
            entry.insert(name.to_string(), d);
        }
    };
    if list.is_empty() {
        apply(permission);
    } else {
        for n in list {
            apply(n);
        }
        entry.remove("media");
    }
    if entry.is_empty() {
        table.0.remove(origin);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Decision::*;

    #[test]
    fn origin_of_cases() {
        assert_eq!(origin_of("https://example.com/a?b#c"), "https://example.com");
        assert_eq!(origin_of("http://localhost:3000/x"), "http://localhost:3000");
        assert_eq!(origin_of("pas une url"), "pas une url");
        assert_eq!(origin_of(""), "");
    }

    #[test]
    fn web_origin_only_http() {
        assert_eq!(web_origin("https://a.com/x").as_deref(), Some("https://a.com"));
        assert_eq!(web_origin("data:text/plain,x"), None);
        assert_eq!(web_origin("null"), None);
        assert_eq!(origin_for(&["", "about:blank"], Some("https://top.com/p")).as_deref(), Some("https://top.com"));
        assert_eq!(origin_for(&[], Some("file:///a")), None);
    }

    #[test]
    fn media_groups() {
        let mut p = Permissions::default();
        p.set("https://a.com", "media", Some(Allow), None);
        assert_eq!(p.get("https://a.com", "media", None), Some(Allow));
        assert_eq!(p.get("https://a.com", "camera", None), Some(Allow));
        p.set("https://a.com", "microphone", Some(Deny), None);
        assert_eq!(p.get("https://a.com", "media", None), Some(Deny));
        assert!(p.take_dirty());
        p.reset(Some("https://a.com"), Some("microphone"));
        assert_eq!(p.get("https://a.com", "media", None), None);
        p.reset(Some("https://a.com"), Some("camera"));
        assert!(p.table.0.is_empty());
    }

    #[test]
    fn opaque_origin_ignored() {
        let mut p = Permissions::default();
        p.set("null", "geolocation", Some(Allow), None);
        assert!(p.table.0.is_empty());
    }

    #[test]
    fn private_inherits_denials() {
        let mut p = Permissions::default();
        p.set("https://a.com", "geolocation", Some(Deny), None);
        let mut t = Table::default();
        assert_eq!(p.get("https://a.com", "geolocation", Some(&t)), Some(Deny));
        p.set("https://b.com", "geolocation", Some(Allow), Some(&mut t));
        assert_eq!(p.get("https://b.com", "geolocation", Some(&t)), Some(Allow));
        assert_eq!(p.get("https://b.com", "geolocation", None), None);
        assert!(!p.take_dirty() || p.table.0.len() == 1);
    }

    #[test]
    fn verdicts() {
        let mut p = Permissions::default();
        assert_eq!(p.verdict("https://a.com", "fullscreen", &[], None), Verdict::Granted);
        assert_eq!(p.verdict("https://a.com", "unknown-thing", &[], None), Verdict::Denied);
        assert_eq!(p.verdict("https://a.com", "geolocation", &[], None), Verdict::Ask);
        p.set("https://a.com", "camera", Some(Allow), None);
        assert_eq!(p.verdict("https://a.com", "media", &["video"], None), Verdict::Granted);
        assert_eq!(p.verdict("https://a.com", "media", &[], None), Verdict::Ask);
        p.set("https://a.com", "microphone", Some(Deny), None);
        assert_eq!(p.verdict("https://a.com", "media", &[], None), Verdict::Denied);
        assert!(p.check(Some("https://a.com"), "camera", None, None));
        assert!(!p.check(None, "camera", None, None));
        assert!(p.check(None, "fullscreen", None, None));
    }

    #[test]
    fn list_sorted_and_names() {
        let mut p = Permissions::default();
        p.set("https://b.com", "midi", Some(Allow), None);
        p.set("https://a.com", "notifications", Some(Deny), None);
        assert_eq!(p.list_all()[0].0, "https://a.com");
        assert_eq!(names("media", &["audio"]), vec!["microphone".to_string()]);
        assert_eq!(names("media", &[]), vec!["camera".to_string(), "microphone".to_string()]);
        assert!(names("bogus", &[]).is_empty());
        assert_eq!(Permissions::dialog_key(&names("media", &[])), "media");
    }

    #[test]
    fn external() {
        assert!(external_allowed("mailto:a@b.c"));
        assert!(external_allowed("zoommtg://x"));
        assert!(!external_allowed("https://a.com"));
        assert!(!external_allowed("javascript:alert(1)"));
        assert!(!external_allowed("signet://settings"));
        assert!(!external_allowed("nope"));
    }

    #[test]
    fn json_format() {
        let t: Table = serde_json::from_str(r#"{"https://a.com":{"camera":"allow","midi":"deny"}}"#).unwrap();
        assert_eq!(t.0["https://a.com"]["midi"], Deny);
    }
}

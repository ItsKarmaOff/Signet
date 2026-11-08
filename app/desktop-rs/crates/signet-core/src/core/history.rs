//! Ports `app/desktop/src/main/core/history.ts`: the browsing history file (`history.json`), search and frecency.
//!
//! Persistence follows the same deadline model as the store: `record` and friends call `schedule`, the app calls
//! `tick(now)` from its loop (or `flush()` on shutdown). Nothing is written before the file was loaded once.

use super::atomic::{read_text, AtomicWriter};
use super::rank::{history_score, ScoreInput};
use crate::ids::now_ms;
use crate::types::HistoryEntry;
use crate::urls::fold;
use indexmap::IndexMap;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const MAX_ENTRIES: usize = 5000;
const FAVICON_MAX: usize = 200_000;

fn ignored(url: &str) -> bool {
    let u = url.to_lowercase();
    ["about:", "data:", "devtools:"].iter().any(|p| u.starts_with(p))
}

/// Oversized `data:` icons are not kept in the history.
fn cap_icon(f: Option<&str>) -> Option<String> {
    f.filter(|f| !(f.starts_with("data:") && f.len() > FAVICON_MAX)).map(String::from)
}

/// An entry to import from another browser.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportedEntry {
    pub url: String,
    pub title: String,
    pub visits: u32,
    pub typed: Option<u32>,
    pub last_visit: f64,
}

#[derive(Debug)]
pub struct History {
    /// Insertion order follows recency: the first entry is the oldest.
    entries: IndexMap<String, HistoryEntry>,
    /// Folded keys (title, url) per entry URL, never written to disk.
    keys: HashMap<String, (String, String)>,
    file: PathBuf,
    /// Loaded on demand: until then nothing is written, so an empty history never replaces the file.
    loaded: bool,
    /// Unreadable file (I/O error): it is not written during this session.
    locked: bool,
    writer: AtomicWriter,
    /// Deadline (ms) of the planned write.
    deadline: Option<i64>,
}

impl History {
    /// `history.json` inside the data folder.
    pub fn new(data_dir: &Path) -> Self {
        let file = data_dir.join("history.json");
        History { entries: IndexMap::new(), keys: HashMap::new(), writer: AtomicWriter::new(&file), file, loaded: false, locked: false, deadline: None }
    }

    /// Reads the file, only once; called at the top of every method that touches entries.
    fn load(&mut self) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        match read_text(&self.file) {
            Ok(text) => match serde_json::from_str::<serde_json::Value>(&text) {
                Ok(serde_json::Value::Array(list)) => {
                    for v in list {
                        if v.get("url").and_then(|u| u.as_str()).is_none() {
                            continue;
                        }
                        if let Ok(e) = serde_json::from_value::<HistoryEntry>(v) {
                            self.entries.insert(e.url.clone(), e);
                        }
                    }
                }
                Ok(_) => self.locked = true,
                Err(_) => {
                    // Corrupt file: copy it aside before the next write replaces it.
                    self.entries.clear();
                    let mut aside = self.file.clone().into_os_string();
                    aside.push(format!(".corrupt-{}", now_ms()));
                    let _ = std::fs::copy(&self.file, PathBuf::from(aside));
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) if e.kind() == std::io::ErrorKind::InvalidData => {
                self.entries.clear();
                let mut aside = self.file.clone().into_os_string();
                aside.push(format!(".corrupt-{}", now_ms()));
                let _ = std::fs::copy(&self.file, PathBuf::from(aside));
            }
            Err(_) => self.locked = true, // unreadable, maybe intact: leave it alone
        }
    }

    /// Loads the history during idle time (the palette will not wait for the read).
    pub fn preload(&mut self) {
        self.load();
    }

    fn key(&mut self, url: &str) -> (String, String) {
        if let Some(k) = self.keys.get(url) {
            return k.clone();
        }
        let e = &self.entries[url];
        let k = (fold(&e.title), fold(&e.url));
        self.keys.insert(url.to_string(), k.clone());
        k
    }

    pub fn size(&mut self) -> usize {
        self.load();
        self.entries.len()
    }

    pub fn clear(&mut self) {
        self.load();
        self.entries.clear();
        self.keys.clear();
        self.flush();
    }

    /// `typed`: the navigation was started from the command bar, which counts as a typed visit.
    pub fn record(&mut self, url: &str, title: &str, favicon: Option<&str>, typed: bool) {
        if url.is_empty() || ignored(url) {
            return;
        }
        self.load();
        let previous = self.entries.shift_remove(url);
        self.keys.remove(url);
        let prev_typed = previous.as_ref().and_then(|p| p.typed).unwrap_or(0);
        let entry = HistoryEntry {
            url: url.to_string(),
            title: if !title.is_empty() { title.to_string() } else { previous.as_ref().map(|p| p.title.clone()).filter(|t| !t.is_empty()).unwrap_or_else(|| url.to_string()) },
            favicon: cap_icon(favicon).or_else(|| previous.as_ref().and_then(|p| p.favicon.clone())),
            visits: previous.as_ref().map_or(0, |p| p.visits) + 1,
            typed: if prev_typed > 0 || typed { Some(prev_typed + u32::from(typed)) } else { None },
            last_visit: now_ms(),
        };
        self.entries.insert(url.to_string(), entry);
        if self.entries.len() > MAX_ENTRIES {
            self.entries.shift_remove_index(0);
        }
        self.schedule(2000);
    }

    /// Adds another browser's history: an already known page keeps its most recent visit and accumulates visits.
    /// Recency order is rebuilt, then only the 5,000 most recent pages are kept. Returns the number of new pages.
    pub fn import_entries(&mut self, list: &[ImportedEntry]) -> usize {
        self.load();
        let mut added = 0;
        for e in list {
            if e.url.is_empty() || ignored(&e.url) || !e.last_visit.is_finite() {
                continue;
            }
            if let Some(known) = self.entries.get_mut(&e.url) {
                known.visits += e.visits;
                if let Some(t) = e.typed.filter(|t| *t > 0) {
                    known.typed = Some(known.typed.unwrap_or(0) + t);
                }
                if (e.last_visit as i64) > known.last_visit {
                    known.last_visit = e.last_visit as i64;
                }
                if (known.title.is_empty() || known.title == known.url) && !e.title.is_empty() {
                    known.title = e.title.clone();
                    self.keys.remove(&e.url);
                }
                continue;
            }
            let typed = e.typed.filter(|t| *t > 0);
            self.entries.insert(
                e.url.clone(),
                HistoryEntry {
                    url: e.url.clone(),
                    title: if e.title.is_empty() { e.url.clone() } else { e.title.clone() },
                    favicon: None,
                    visits: e.visits.max(1),
                    typed,
                    last_visit: e.last_visit as i64,
                },
            );
            added += 1;
        }
        let mut sorted: Vec<HistoryEntry> = self.entries.values().cloned().collect();
        sorted.sort_by_key(|e| e.last_visit);
        let skip = sorted.len().saturating_sub(MAX_ENTRIES);
        self.entries = sorted.into_iter().skip(skip).map(|e| (e.url.clone(), e)).collect();
        self.flush();
        added
    }

    pub fn update(&mut self, url: &str, title: Option<&str>, favicon: Option<&str>) {
        self.load();
        let Some(entry) = self.entries.get_mut(url) else { return };
        let mut changed = false;
        if let Some(t) = title.filter(|t| !t.is_empty() && *t != entry.title) {
            entry.title = t.to_string();
            self.keys.remove(url);
            changed = true;
        }
        if let Some(icon) = cap_icon(favicon).filter(|i| entry.favicon.as_deref() != Some(i.as_str())) {
            entry.favicon = Some(icon);
            changed = true;
        }
        if changed {
            self.schedule(60_000); // title/icon only: deferred write (shutdown flushes anyway)
        }
    }

    /// History page (most recent to oldest), filtered ignoring accents.
    pub fn list(&mut self, query: &str, offset: usize, limit: usize) -> Vec<HistoryEntry> {
        self.load();
        let q = fold(query.trim());
        let urls: Vec<String> = self.entries.keys().cloned().collect();
        let mut out = Vec::new();
        let mut skipped = 0;
        for url in urls.iter().rev() {
            if out.len() >= limit {
                break;
            }
            if !q.is_empty() {
                let (t, u) = self.key(url);
                if !t.contains(&q) && !u.contains(&q) {
                    continue;
                }
            }
            if skipped < offset {
                skipped += 1;
            } else {
                out.push(self.entries[url].clone());
            }
        }
        out
    }

    /// The entry of exactly this URL.
    pub fn get(&mut self, url: &str) -> Option<&HistoryEntry> {
        self.load();
        self.entries.get(url)
    }

    pub fn remove(&mut self, url: &str) {
        self.load();
        if self.entries.shift_remove(url).is_some() {
            self.keys.remove(url);
            self.schedule(2000);
        }
    }

    /// Entries whose title or URL contains the query, best first (match quality weighted by frecency).
    pub fn search(&mut self, query: &str, exclude: &std::collections::HashSet<String>, limit: usize) -> Vec<HistoryEntry> {
        self.load();
        if limit == 0 {
            return Vec::new();
        }
        let q = fold(query);
        let now = now_ms();
        // Selects the best `limit` by insertion; on equal score, insertion order is kept (stable).
        let mut top: Vec<(String, f64)> = Vec::new();
        let urls: Vec<String> = self.entries.keys().cloned().collect();
        for url in urls {
            if exclude.contains(&url) {
                continue;
            }
            let (t, u) = self.key(&url);
            let e = &self.entries[&url];
            let s = history_score(&q, &ScoreInput { title: &t, url: &u, visits: e.visits, typed: e.typed.unwrap_or(0), last_visit: e.last_visit }, now);
            if s == 0.0 {
                continue;
            }
            if top.len() == limit && s <= top[limit - 1].1 {
                continue;
            }
            let mut i = top.len();
            while i > 0 && top[i - 1].1 < s {
                i -= 1;
            }
            top.insert(i, (url, s));
            if top.len() > limit {
                top.pop();
            }
        }
        top.into_iter().map(|(u, _)| self.entries[&u].clone()).collect()
    }

    fn payload(&self) -> Option<String> {
        if self.loaded && !self.locked {
            let list: Vec<&HistoryEntry> = self.entries.values().collect();
            serde_json::to_string(&list).ok()
        } else {
            None
        }
    }

    /// Synchronous write (shutdown, clearing). No effect if the history was never loaded: the file stays intact.
    pub fn flush(&mut self) {
        self.deadline = None;
        let p = self.payload();
        self.writer.write(p);
    }

    /// Plans a write `delay` ms from now; an earlier planned deadline is kept, a later one is brought forward.
    pub fn schedule(&mut self, delay: i64) {
        let at = now_ms() + delay;
        if self.deadline.is_none_or(|d| at < d) {
            self.deadline = Some(at);
        }
    }

    /// Deadline (ms since epoch) of the planned write, if any: the app can sleep until then.
    pub fn next_deadline(&self) -> Option<i64> {
        self.deadline
    }

    /// True if a planned write is due at `now`.
    pub fn due(&self, now: i64) -> bool {
        self.deadline.is_some_and(|d| d <= now)
    }

    /// Writes if the planned deadline passed. Returns true if a write was attempted.
    pub fn tick(&mut self, now: i64) -> bool {
        if self.due(now) {
            self.flush();
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::fs;

    fn new() -> (tempfile::TempDir, History) {
        let dir = tempfile::tempdir().unwrap();
        let h = History::new(dir.path());
        (dir, h)
    }
    fn urls(list: &[HistoryEntry]) -> Vec<&str> {
        list.iter().map(|e| e.url.as_str()).collect()
    }
    fn none() -> HashSet<String> {
        HashSet::new()
    }

    #[test]
    fn record_ignores_internal_addresses() {
        let (_d, mut h) = new();
        for u in ["", "about:blank", "data:text/html,hi", "devtools://devtools/x"] {
            h.record(u, "t", None, false);
        }
        assert_eq!(h.size(), 0);
    }

    #[test]
    fn record_accumulates_and_moves_to_front() {
        let (_d, mut h) = new();
        h.record("https://a.com", "A", None, false);
        h.record("https://b.com", "B", None, false);
        h.record("https://a.com", "", None, false);
        let l = h.list("", 0, 10);
        assert_eq!((l[0].url.as_str(), l[0].title.as_str(), l[0].visits), ("https://a.com", "A", 2));
        assert_eq!(urls(&l), ["https://a.com", "https://b.com"]);
    }

    #[test]
    fn typed_counts() {
        let (_d, mut h) = new();
        h.record("https://a.com", "A", None, false);
        assert_eq!(h.get("https://a.com").unwrap().typed, None);
        h.record("https://a.com", "A", None, true);
        h.record("https://a.com", "A", None, true);
        h.record("https://a.com", "A", None, false);
        let e = h.get("https://a.com").unwrap();
        assert_eq!((e.visits, e.typed), (4, Some(2)));
    }

    #[test]
    fn default_title_and_cap() {
        let (_d, mut h) = new();
        h.record("https://a.com", "", None, false);
        assert_eq!(h.list("", 0, 1)[0].title, "https://a.com");
        let (_d2, mut h) = new();
        for i in 0..5001 {
            h.record(&format!("https://x.com/{i}"), "", None, false);
        }
        assert_eq!(h.size(), 5000);
        assert_eq!(h.list("", 4999, 1)[0].url, "https://x.com/1");
    }

    #[test]
    fn big_data_favicons_dropped() {
        let (_d, mut h) = new();
        h.record("https://a.com", "A", Some(&format!("data:image/png;base64,{}", "x".repeat(200_001))), false);
        h.record("https://b.com", "B", Some("https://b.com/favicon.ico"), false);
        assert_eq!(h.list("a.com", 0, 1)[0].favicon, None);
        assert_eq!(h.list("b.com", 0, 1)[0].favicon.as_deref(), Some("https://b.com/favicon.ico"));
    }

    fn fill(h: &mut History) {
        h.record("https://cafe.com", "Le Caf\u{e9}", None, false);
        h.record("https://github.com/x", "GitHub", None, false);
        h.record("https://example.org", "Exemple", None, false);
    }

    #[test]
    fn list_and_filter() {
        let (_d, mut h) = new();
        fill(&mut h);
        assert_eq!(urls(&h.list("", 0, 10)), ["https://example.org", "https://github.com/x", "https://cafe.com"]);
        assert_eq!(urls(&h.list("", 1, 1)), ["https://github.com/x"]);
        assert_eq!(urls(&h.list("CAFE", 0, 10)), ["https://cafe.com"]);
        assert_eq!(urls(&h.list("le caf\u{e9}", 0, 10)), ["https://cafe.com"]);
        assert_eq!(urls(&h.list("github.com", 0, 10)), ["https://github.com/x"]);
        assert!(h.list("zzz", 0, 10).is_empty());
    }

    #[test]
    fn search_exclusions_and_limit() {
        let (_d, mut h) = new();
        fill(&mut h);
        let ex: HashSet<String> = ["https://cafe.com".to_string()].into();
        let mut r: Vec<String> = h.search("o", &ex, 10).iter().map(|e| e.url.clone()).collect();
        r.sort();
        assert_eq!(r, ["https://example.org", "https://github.com/x"]);
        assert_eq!(h.search("o", &none(), 1).len(), 1);
        assert!(h.search("o", &none(), 0).is_empty());
    }

    #[test]
    fn search_ranking() {
        let (_d, mut h) = new();
        h.record("https://a.com/1", "doc", None, false);
        h.record("https://a.com/2", "doc", None, false);
        h.record("https://a.com/1", "doc", None, false);
        h.record("https://a.com/1", "doc", None, false);
        assert_eq!(urls(&h.search("doc", &none(), 5)), ["https://a.com/1", "https://a.com/2"]);

        let (_d, mut h) = new();
        for _ in 0..3 {
            h.record("https://blog.com/why-youtube-is-big", "Why youtube is big", None, false);
        }
        h.record("https://www.youtube.com/", "YouTube", None, false);
        assert_eq!(urls(&h.search("yout", &none(), 5)), ["https://www.youtube.com/", "https://blog.com/why-youtube-is-big"]);

        let (_d, mut h) = new();
        for _ in 0..20 {
            h.record("https://links.com/doc", "doc", None, false);
        }
        for _ in 0..5 {
            h.record("https://typed.com/doc", "doc", None, true);
        }
        assert_eq!(urls(&h.search("doc", &none(), 5)), ["https://typed.com/doc", "https://links.com/doc"]);

        let (_d, mut h) = new();
        h.record("https://a.com/doc", "doc", None, false);
        for _ in 0..4 {
            h.record("https://b.com/doc", "doc", None, false);
        }
        assert_eq!(urls(&h.search("doc", &none(), 5)), ["https://b.com/doc", "https://a.com/doc"]);
    }

    fn imp(url: &str, title: &str, visits: u32, typed: Option<u32>, last: f64) -> ImportedEntry {
        ImportedEntry { url: url.into(), title: title.into(), visits, typed, last_visit: last }
    }

    #[test]
    fn search_frequency_and_recency() {
        let (_d, mut h) = new();
        let now = now_ms() as f64;
        h.import_entries(&[imp("https://b.com/stale", "doc", 1, None, now - 400.0 * 86_400_000.0), imp("https://b.com/fresh", "doc", 1, None, now)]);
        assert_eq!(urls(&h.search("doc", &none(), 5)), ["https://b.com/fresh", "https://b.com/stale"]);
    }

    #[test]
    fn update_and_remove() {
        let (_d, mut h) = new();
        h.record("https://a.com", "Ancien", None, false);
        h.update("https://a.com", Some("Nouveau"), None);
        assert_eq!(h.search("nouveau", &none(), 5).len(), 1);
        assert_eq!(h.search("ancien", &none(), 5).len(), 0);
        h.remove("https://a.com");
        assert_eq!(h.size(), 0);
    }

    #[test]
    fn import_merges() {
        let (_d, mut h) = new();
        h.record("https://a.com", "", None, false);
        let t = h.list("", 0, 1)[0].last_visit;
        let added = h.import_entries(&[
            imp("https://a.com", "Titre A", 3, None, (t + 1000) as f64),
            imp("https://b.com", "", 0, None, 5.0),
            imp("about:blank", "x", 1, None, 1.0),
            imp("https://c.com", "C", 1, None, f64::NAN),
        ]);
        assert_eq!(added, 1);
        assert_eq!(h.size(), 2);
        let a = h.list("a.com", 0, 1)[0].clone();
        assert_eq!((a.title.as_str(), a.visits, a.last_visit), ("Titre A", 4, t + 1000));
        let b = h.list("b.com", 0, 1)[0].clone();
        assert_eq!((b.title.as_str(), b.visits, b.last_visit), ("https://b.com", 1, 5));
        assert_eq!(urls(&h.list("", 0, 10)), ["https://a.com", "https://b.com"]);
    }

    #[test]
    fn import_keeps_typed() {
        let (_d, mut h) = new();
        h.record("https://a.com", "A", None, true);
        h.import_entries(&[imp("https://a.com", "A", 2, Some(3), 1.0), imp("https://b.com", "B", 2, Some(4), 2.0), imp("https://c.com", "C", 2, None, 3.0)]);
        assert_eq!(h.get("https://a.com").unwrap().typed, Some(4));
        assert_eq!(h.get("https://b.com").unwrap().typed, Some(4));
        assert_eq!(h.get("https://c.com").unwrap().typed, None);
    }

    #[test]
    fn file_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.json");
        fs::write(&path, r#"[{"url":"https://old.com","title":"Old","visits":7,"lastVisit":1}]"#).unwrap();
        let mut h = History::new(dir.path());
        let e = h.get("https://old.com").unwrap().clone();
        assert_eq!((e.visits, e.typed, e.favicon), (7, None, None));
        h.record("https://old.com", "Old", None, true);
        h.flush();
        let mut again = History::new(dir.path());
        let e = again.get("https://old.com").unwrap();
        assert_eq!((e.visits, e.typed), (8, Some(1)));
    }

    #[test]
    fn clear_empties_file() {
        let (d, mut h) = new();
        h.record("https://a.com", "A", None, false);
        h.flush();
        h.clear();
        assert_eq!(fs::read_to_string(d.path().join("history.json")).unwrap(), "[]");
    }

    #[test]
    fn corrupt_file_is_copied_aside() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("history.json"), "{oops").unwrap();
        let mut h = History::new(dir.path());
        assert_eq!(h.size(), 0);
        let n = fs::read_dir(dir.path()).unwrap().filter_map(Result::ok).filter(|e| e.file_name().to_string_lossy().starts_with("history.json.corrupt-")).count();
        assert_eq!(n, 1);
    }

    #[test]
    fn invalid_entries_ignored() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("history.json"), r#"[{"url":"https://a.com","title":"A","visits":1,"lastVisit":1},null,{"title":"sans url"},3]"#).unwrap();
        assert_eq!(History::new(dir.path()).size(), 1);
    }

    #[test]
    fn no_file_without_load() {
        let (d, mut h) = new();
        h.flush();
        assert!(!d.path().join("history.json").exists());
    }

    #[test]
    fn deferred_write_fires_after_delay() {
        let (d, mut h) = new();
        h.record("https://a.com", "A", None, false);
        let at = h.next_deadline().unwrap();
        assert!(!h.tick(at - 1));
        assert!(!d.path().join("history.json").exists());
        assert!(h.tick(at));
        assert!(d.path().join("history.json").exists());
        assert_eq!(h.next_deadline(), None);
    }
}

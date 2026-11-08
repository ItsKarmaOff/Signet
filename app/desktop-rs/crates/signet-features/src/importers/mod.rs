//! Importers (port of `main/features/importers`). Pure parsers plus disk readers; the app wires dialogs,
//! history/password/extension stores and the state mutation (see `apply`).
pub mod apply;
pub mod arc;
pub mod bookmarks;
pub mod chromium;
pub mod sources;
pub mod types;

use std::path::Path;
use std::process::Command;

use crate::util::{err, Result, Translate};
use types::ImportedNode;

/// True for errors that mean the OS protects the data (macOS Files & Folders / Full Disk Access).
pub fn is_permission_error(message: &str) -> bool {
    let l = message.to_lowercase();
    l.contains("not permitted") || l.contains("permission") || l.contains("eperm") || l.contains("eacces")
}

/// Safari favorites via `plutil` (macOS only): JSON first, XML plist as a fallback.
pub fn read_safari(path: &Path, t: Translate) -> Result<Option<ImportedNode>> {
    let plutil = |fmt: &str| -> Result<String> {
        let out = Command::new("plutil").args(["-convert", fmt, "-o", "-"]).arg(path).output()?;
        if out.status.success() { Ok(String::from_utf8_lossy(&out.stdout).into_owned()) } else { err(String::from_utf8_lossy(&out.stderr).into_owned()) }
    };
    match plutil("json").and_then(|s| serde_json::from_str(&s).map_err(Into::into)).and_then(|v| bookmarks::parse_safari_bookmarks(&v, t)) {
        Ok(r) => Ok(r),
        Err(e) if is_permission_error(&e.to_string()) => Err(e),
        Err(_) => bookmarks::parse_safari_bookmarks(&bookmarks::parse_xml_plist(&plutil("xml1")?), t),
    }
}

/// Reads and parses the `Bookmarks` file of a Chromium profile.
pub fn read_chrome_bookmarks(profile_or_file: &Path, browser_name: &str, t: Translate) -> Result<Option<ImportedNode>> {
    let file = if profile_or_file.is_dir() { profile_or_file.join("Bookmarks") } else { profile_or_file.to_path_buf() };
    let json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(file)?)?;
    bookmarks::parse_chrome_bookmarks(&json, browser_name, t)
}

/// Reads Arc's `StorableSidebar.json`.
pub fn read_arc(path: &Path, t: Translate) -> Result<types::SpacesImport> {
    let json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path)?)?;
    arc::parse_spaces_file(&json, t)
}

/// Merges history pages: a known page adds up visits and keeps the most recent visit; at most `limit` pages kept.
pub fn merge_history(existing: &mut Vec<chromium::HistoryRow>, incoming: Vec<chromium::HistoryRow>, limit: usize) -> usize {
    let mut added = 0;
    for r in incoming {
        match existing.iter_mut().find(|e| e.url == r.url) {
            Some(e) => {
                e.visits += r.visits;
                e.typed += r.typed;
                if r.last_visit > e.last_visit {
                    e.last_visit = r.last_visit;
                    if !r.title.is_empty() { e.title = r.title; }
                }
            }
            None => {
                existing.push(r);
                added += 1;
            }
        }
    }
    existing.sort_by_key(|a| std::cmp::Reverse(a.last_visit));
    existing.truncate(limit);
    added
}

#[cfg(test)]
mod tests {
    use super::*;
    use chromium::HistoryRow;

    fn row(u: &str, v: i64, t: i64) -> HistoryRow { HistoryRow { url: u.into(), title: u.into(), visits: v, typed: 0, last_visit: t } }

    #[test]
    fn history_merge() {
        let mut h = vec![row("a", 1, 10)];
        let added = merge_history(&mut h, vec![row("a", 2, 20), row("b", 1, 5), row("c", 1, 30)], 2);
        assert_eq!(added, 2);
        assert_eq!(h.iter().map(|r| r.url.as_str()).collect::<Vec<_>>(), vec!["c", "a"]);
        assert_eq!(h[1].visits, 3);
    }

    #[test]
    fn permission_detection() {
        assert!(is_permission_error("Operation not permitted"));
        assert!(is_permission_error("EACCES: x"));
        assert!(!is_permission_error("file not found"));
    }

    #[test]
    fn readers_from_fixtures() {
        let d = tempfile::tempdir().unwrap();
        let tr = |k: &str, _: &[(&str, String)]| k.to_string();
        std::fs::write(d.path().join("Bookmarks"), r#"{"roots":{"bookmark_bar":{"children":[{"type":"url","name":"A","url":"https://a.com"}]}}}"#).unwrap();
        let n = read_chrome_bookmarks(d.path(), "Chrome", &tr).unwrap().unwrap();
        assert!(matches!(n, ImportedNode::Folder { ref name, .. } if name == "Chrome"));
        assert!(read_chrome_bookmarks(&d.path().join("missing"), "Chrome", &tr).is_err());
    }
}

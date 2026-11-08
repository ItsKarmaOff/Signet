//! Unpacked extensions (SPEC v31): load from a folder, reload on change, remember the folders. Port of `unpacked.ts`.
//!
//! The list of folders is `settings.unpackedExtensions` (owned by signet-core): the app passes it in and writes back
//! the list returned by [`Unpacked::add`] / [`Unpacked::remove`]. Folder watching uses `notify`; a file event only
//! schedules a check (see `reload`), which reloads when a file really changed. Loading in the engine is the app's job:
//! the host emits an effect, the web layer loads the folder and reports back.

use crate::manifest::Manifest;
use crate::registry::{id_of_folder, read_manifest, UnpackedInfo};
use crate::reload::{is_ignored, scan_folder, ReloadScheduler};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};

/// Delay after the last file change before checking for a real change (a save often touches several files).
pub const RELOAD_DELAY_MS: u64 = 400;

pub struct Entry {
    pub path: PathBuf,
    /// Identifier once loaded (derived from the path or the `key`: it does not change from one reload to the next).
    pub id: Option<String>,
    pub name: String,
    pub error: Option<String>,
    pub reloaded_at: Option<i64>,
    scheduler: ReloadScheduler,
    watcher: Option<RecommendedWatcher>,
    events: Option<Receiver<PathBuf>>,
}

#[derive(Default)]
pub struct Unpacked {
    entries: HashMap<PathBuf, Entry>,
    order: Vec<PathBuf>,
}

/// What a successful read of a folder gives.
#[derive(Debug, Clone)]
pub struct Loadable {
    pub id: String,
    pub path: PathBuf,
    pub manifest: Manifest,
}

fn display_path(path: &Path) -> String {
    match dirs_home() {
        Some(home) if path.starts_with(&home) => format!("~{}", path.strip_prefix(&home).map(|p| format!("{}{}", std::path::MAIN_SEPARATOR, p.display())).unwrap_or_default()),
        _ => path.display().to_string(),
    }
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

impl Unpacked {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers the folders remembered from the last run (no load yet).
    pub fn restore(&mut self, paths: &[String]) {
        for p in paths {
            self.entry(Path::new(p));
        }
    }

    fn entry(&mut self, path: &Path) -> &mut Entry {
        if !self.entries.contains_key(path) {
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string());
            self.entries.insert(
                path.to_path_buf(),
                Entry { path: path.to_path_buf(), id: None, name, error: None, reloaded_at: None, scheduler: ReloadScheduler::new(RELOAD_DELAY_MS), watcher: None, events: None },
            );
            self.order.push(path.to_path_buf());
        }
        self.entries.get_mut(path).expect("just inserted")
    }

    /// Checks a folder before adding it: it must contain `manifest.json`.
    pub fn check_folder(path: &Path) -> Result<(), String> {
        if path.join("manifest.json").exists() {
            Ok(())
        } else {
            Err(format!("The folder {} has no manifest.json", path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()))
        }
    }

    /// Remembers the folder and reads its manifest. Returns the new `unpackedExtensions` list and what to load.
    pub fn add(&mut self, current: &[String], path: &Path) -> (Vec<String>, Result<Loadable, String>) {
        let mut list = current.to_vec();
        let s = path.to_string_lossy().into_owned();
        if !list.contains(&s) {
            list.push(s);
        }
        (list, self.read(path))
    }

    /// Reads the manifest of a remembered folder and updates its entry (error or name); the engine loads the result.
    pub fn read(&mut self, path: &Path) -> Result<Loadable, String> {
        let e = self.entry(path);
        // Snapshot BEFORE loading: a file written while loading triggers one more (safe) reload instead of being lost.
        if let Ok(s) = scan_folder(path) {
            e.scheduler.baseline(s);
        }
        match read_manifest(path) {
            Ok(manifest) => {
                let id = id_of_folder(path, &manifest);
                let e = self.entry(path);
                e.name = manifest.name.clone();
                Ok(Loadable { id, path: path.to_path_buf(), manifest })
            }
            Err(err) => {
                let e = self.entry(path);
                e.id = None;
                e.error = Some(err.clone());
                Err(err)
            }
        }
    }

    /// The web layer loaded the extension (`reloaded` when it was a reload).
    pub fn loaded(&mut self, path: &Path, id: &str, name: &str, reloaded: bool, now_unix_ms: i64) {
        let e = self.entry(path);
        e.id = Some(id.to_string());
        e.name = name.to_string();
        e.error = None;
        if reloaded {
            e.reloaded_at = Some(now_unix_ms);
        }
        self.watch(path);
    }

    /// The engine refused to load it (invalid manifest...): the message is shown in Settings; the next change tries again.
    pub fn load_failed(&mut self, path: &Path, error: &str) {
        let e = self.entry(path);
        e.id = None;
        e.error = Some(error.to_string());
        self.watch(path);
    }

    /// Watches the folder: a change reloads the extension (even after a failure, to recover once fixed).
    fn watch(&mut self, path: &Path) {
        let Some(e) = self.entries.get_mut(path) else { return };
        if e.watcher.is_some() || !path.exists() {
            return;
        }
        let (tx, rx) = channel();
        let root = path.to_path_buf();
        let watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(ev) = res {
                for p in ev.paths {
                    let rel = p.strip_prefix(&root).unwrap_or(&p).to_string_lossy().into_owned();
                    if !is_ignored(&rel) {
                        let _ = tx.send(p);
                    }
                }
            }
        });
        if let Ok(mut w) = watcher {
            if w.watch(path, RecursiveMode::Recursive).is_ok() {
                e.watcher = Some(w);
                e.events = Some(rx);
            }
        }
    }

    /// Feeds a file event by hand (tests, or a watcher owned elsewhere).
    pub fn file_event(&mut self, path: &Path, now_ms: u64) {
        if let Some(e) = self.entries.get(path) {
            e.scheduler.notify(now_ms);
        }
    }

    /// Drains the watchers and runs the debounced checks; returns the folders whose files really changed and must be
    /// reloaded (unload, wait for the worker to stop, load).
    pub fn poll(&mut self, now_ms: u64) -> Vec<PathBuf> {
        let mut due = Vec::new();
        for path in self.order.clone() {
            let Some(e) = self.entries.get_mut(&path) else { continue };
            if let Some(rx) = &e.events {
                while rx.try_recv().is_ok() {
                    e.scheduler.notify(now_ms);
                }
            }
            if e.scheduler.take_due(now_ms) {
                let mut reload = false;
                let root = path.clone();
                e.scheduler.check(&mut || scan_folder(&root), &mut || reload = true);
                if reload {
                    due.push(path);
                }
            }
        }
        due
    }

    /// Folder of a loaded unpacked extension, or None.
    pub fn path_of(&self, id: &str) -> Option<&Path> {
        self.entries.values().find(|e| e.id.as_deref() == Some(id)).map(|e| e.path.as_path())
    }

    /// State of an unpacked extension (Settings), from its folder.
    pub fn state_of(&self, path: &Path) -> UnpackedInfo {
        let e = self.entries.get(path);
        UnpackedInfo { path: path.display().to_string(), display: display_path(path), error: e.and_then(|e| e.error.clone()), reloaded_at: e.and_then(|e| e.reloaded_at) }
    }

    /// Extensions remembered but not loaded (folder not found, invalid manifest): name and state.
    pub fn failures(&self, remembered: &[String]) -> Vec<(String, UnpackedInfo)> {
        remembered
            .iter()
            .filter(|p| self.entries.get(Path::new(p)).is_none_or(|e| e.id.is_none()))
            .map(|p| {
                let path = Path::new(p);
                (self.entries.get(path).map(|e| e.name.clone()).unwrap_or_else(|| path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()), self.state_of(path))
            })
            .collect()
    }

    /// Forgets the folder (its files stay on disk). Returns the new list and the id that must be unloaded and forgotten.
    pub fn remove(&mut self, current: &[String], path: &Path) -> (Vec<String>, Option<String>) {
        let s = path.to_string_lossy().into_owned();
        let list: Vec<String> = current.iter().filter(|p| **p != s).cloned().collect();
        self.order.retain(|p| p != path);
        let id = self.entries.remove(path).and_then(|e| {
            e.scheduler.dispose();
            e.id
        });
        (list, id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;

    fn folder(extra: &str) -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        fs::write(d.path().join("manifest.json"), json!({ "manifest_version": 3, "name": "Dev ext", "version": "0.1" }).to_string()).unwrap();
        fs::write(d.path().join("content.js"), extra).unwrap();
        d
    }

    #[test]
    fn a_folder_needs_a_manifest() {
        let d = tempfile::tempdir().unwrap();
        assert!(Unpacked::check_folder(d.path()).unwrap_err().contains("has no manifest.json"));
        let f = folder("v1");
        assert!(Unpacked::check_folder(f.path()).is_ok());
    }

    #[test]
    fn add_remembers_the_folder_and_reads_the_manifest() {
        let f = folder("v1");
        let mut u = Unpacked::new();
        let (list, loadable) = u.add(&[], f.path());
        assert_eq!(list, [f.path().to_string_lossy().into_owned()]);
        let l = loadable.unwrap();
        assert_eq!(l.manifest.name, "Dev ext");
        assert_eq!(l.id, crate::id::id_from_path(f.path()));
        // Adding twice does not duplicate.
        assert_eq!(u.add(&list, f.path()).0.len(), 1);
    }

    #[test]
    fn an_invalid_manifest_is_listed_with_its_error() {
        let d = tempfile::tempdir().unwrap();
        fs::write(d.path().join("manifest.json"), "{ nope").unwrap();
        let mut u = Unpacked::new();
        let (list, r) = u.add(&[], d.path());
        assert!(r.is_err());
        let failed = u.failures(&list);
        assert_eq!(failed.len(), 1);
        assert!(failed[0].1.error.as_deref().unwrap().contains("not valid JSON"));
    }

    #[test]
    fn loaded_state_and_removal() {
        let f = folder("v1");
        let mut u = Unpacked::new();
        let (list, l) = u.add(&[], f.path());
        let l = l.unwrap();
        u.loaded(f.path(), &l.id, &l.manifest.name, false, 0);
        assert_eq!(u.path_of(&l.id), Some(f.path()));
        assert!(u.failures(&list).is_empty());
        u.loaded(f.path(), &l.id, "Dev ext", true, 1234);
        assert_eq!(u.state_of(f.path()).reloaded_at, Some(1234));
        let (list, id) = u.remove(&list, f.path());
        assert!(list.is_empty());
        assert_eq!(id.as_deref(), Some(l.id.as_str()));
        assert!(f.path().join("manifest.json").exists(), "the folder is never erased");
        assert!(u.path_of(&l.id).is_none());
    }

    #[test]
    fn a_real_change_reloads_once_and_an_event_that_changes_nothing_does_not() {
        let f = folder("v1");
        let mut u = Unpacked::new();
        let (_, l) = u.add(&[], f.path());
        let l = l.unwrap();
        u.loaded(f.path(), &l.id, "Dev ext", false, 0);
        // Only the events fed by hand count: the real watcher delivers its own, late and system-dependent ones
        // (Windows), which would move the fake clock's debounce.
        if let Some(e) = u.entries.get_mut(f.path()) {
            e.watcher = None;
            e.events = None;
        }
        // Replayed event, nothing changed.
        u.file_event(f.path(), 0);
        assert!(u.poll(100).is_empty(), "debounced");
        assert!(u.poll(500).is_empty(), "no real change");
        // A real change.
        fs::write(f.path().join("content.js"), "version two, longer").unwrap();
        u.file_event(f.path(), 1000);
        u.file_event(f.path(), 1100);
        assert!(u.poll(1200).is_empty());
        assert_eq!(u.poll(1600), vec![f.path().to_path_buf()]);
        assert!(u.poll(5000).is_empty());
    }

    #[test]
    fn display_uses_tilde_for_the_home_folder() {
        if let Some(home) = dirs_home() {
            assert!(display_path(&home.join("proj").join("ext")).starts_with('~'));
        }
        assert_eq!(display_path(Path::new("/definitely/elsewhere")), "/definitely/elsewhere");
    }

    #[test]
    fn the_real_watcher_sees_a_write() {
        let f = folder("v1");
        let mut u = Unpacked::new();
        let (_, l) = u.add(&[], f.path());
        let l = l.unwrap();
        u.loaded(f.path(), &l.id, "Dev ext", false, 0);
        std::thread::sleep(std::time::Duration::from_millis(300));
        fs::write(f.path().join("content.js"), "changed for real, and longer").unwrap();
        let mut due = Vec::new();
        for i in 0..60u64 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            due = u.poll(i * 100 + 10_000);
            if !due.is_empty() {
                break;
            }
        }
        // The watcher backend is platform dependent; the debounce needs two polls after the event arrives.
        if due.is_empty() {
            due = u.poll(1_000_000);
        }
        assert_eq!(due, vec![f.path().to_path_buf()]);
    }
}

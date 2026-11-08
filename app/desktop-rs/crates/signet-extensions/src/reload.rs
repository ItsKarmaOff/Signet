//! Reload logic of an unpacked extension, free of any engine so it can be tested with fakes. Port of
//! `unpacked-reload.ts` and `action-relay.ts`.
//!
//! Why this exists: on macOS the folder watcher replays the events of files written just before the watch started, so an
//! extension used to be reloaded about 400 ms after it was loaded, while its worker was still starting. A file event now
//! only schedules a CHECK; the extension reloads when a file really differs from the snapshot taken before the last load,
//! never while another reload runs, and events that arrive meanwhile are coalesced into one more check.

use regex::Regex;
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::Path;
use std::sync::OnceLock;

/// Files whose change does not reload the extension.
pub fn is_ignored(relative: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"(^|[\\/])(\.git|node_modules)([\\/]|$)|\.DS_Store$|~$|\.sw[po]$").expect("static regex"));
    re.is_match(relative)
}

/// Relative path to `size:mtimeMs`.
pub type Snapshot = BTreeMap<String, String>;

pub const MAX_SCANNED_FILES: usize = 20000;

/// Size and modification time of every file of a folder (the watcher's "did anything really change" reference).
pub fn scan_folder(root: &Path) -> std::io::Result<Snapshot> {
    fn walk(dir: &Path, rel: &str, out: &mut Snapshot) -> std::io::Result<()> {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let relative = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
            if is_ignored(&relative) || out.len() >= MAX_SCANNED_FILES {
                continue;
            }
            let ty = entry.file_type()?;
            if ty.is_dir() {
                walk(&entry.path(), &relative, out)?;
            } else if ty.is_file() {
                if let Ok(meta) = entry.metadata() {
                    let mtime = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs_f64() * 1000.0).unwrap_or(0.0);
                    out.insert(relative, format!("{}:{}", meta.len(), mtime));
                }
            }
        }
        Ok(())
    }
    let mut out = Snapshot::new();
    walk(root, "", &mut out)?;
    Ok(out)
}

pub fn same_snapshot(a: &Snapshot, b: &Snapshot) -> bool {
    a == b
}

/// Debounced "did anything really change" scheduler. Time is a parameter (`now_ms`).
pub struct ReloadScheduler {
    delay: u64,
    snapshot: RefCell<Option<Snapshot>>,
    due: Cell<Option<u64>>,
    busy: Cell<bool>,
    again: Cell<bool>,
    disposed: Cell<bool>,
}

impl ReloadScheduler {
    pub fn new(delay_ms: u64) -> Self {
        ReloadScheduler {
            delay: delay_ms,
            snapshot: RefCell::new(None),
            due: Cell::new(None),
            busy: Cell::new(false),
            again: Cell::new(false),
            disposed: Cell::new(false),
        }
    }

    /// Records the state the loaded extension was built from. Take it BEFORE loading, so a write during the load is not lost.
    pub fn baseline(&self, snapshot: Snapshot) {
        *self.snapshot.borrow_mut() = Some(snapshot);
    }

    pub fn running(&self) -> bool {
        self.busy.get()
    }

    /// A watcher event happened (any file): debounce.
    pub fn notify(&self, now_ms: u64) {
        if self.disposed.get() {
            return;
        }
        self.due.set(Some(now_ms + self.delay));
    }

    /// True once the debounce has elapsed (clears it): the caller then runs `check`.
    pub fn take_due(&self, now_ms: u64) -> bool {
        match self.due.get() {
            Some(t) if t <= now_ms && !self.disposed.get() => {
                self.due.set(None);
                true
            }
            _ => false,
        }
    }

    /// Next instant a check is due, if any.
    pub fn next_due(&self) -> Option<u64> {
        self.due.get()
    }

    pub fn dispose(&self) {
        self.disposed.set(true);
        self.due.set(None);
    }

    /// Compares the folder with the snapshot and reloads if it differs. Safe to call at any time, re-entrantly included:
    /// a call during a reload only asks for one more check.
    pub fn check(&self, scan: &mut dyn FnMut() -> std::io::Result<Snapshot>, reload: &mut dyn FnMut()) {
        if self.disposed.get() {
            return;
        }
        if self.busy.get() {
            self.again.set(true);
            return;
        }
        self.busy.set(true);
        loop {
            self.again.set(false);
            let current = match scan() {
                Ok(c) => c,
                Err(_) => break, // folder unreadable: try again at the next event
            };
            // No baseline (the load failed): any event may be the fix, so reload.
            let same = self.snapshot.borrow().as_ref().is_some_and(|s| same_snapshot(s, &current));
            if !same {
                *self.snapshot.borrow_mut() = Some(current);
                reload();
                if self.disposed.get() {
                    break;
                }
            }
            if !self.again.get() {
                break;
            }
        }
        self.busy.set(false);
    }
}

/// Order of a safe reload: unload, wait for the worker to stop, then load; never load over a live registration.
pub struct SafeReloadDeps<'a> {
    pub is_loaded: &'a mut dyn FnMut() -> bool,
    pub unload: &'a mut dyn FnMut(),
    pub worker_stopped: &'a mut dyn FnMut(),
    pub load: &'a mut dyn FnMut(),
}

pub fn safe_reload(deps: SafeReloadDeps) {
    if (deps.is_loaded)() {
        (deps.unload)();
        (deps.worker_stopped)();
    }
    (deps.load)();
}

// ---- Action relay ----

/// How long the toolbar has to answer before the host opens the popup itself.
pub const ACK_TIMEOUT_MS: u64 = 200;

/// What to do for a shortcut that must open a button's popup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelayOutcome {
    /// The toolbar was asked (token): wait for `ack` or for `expire`.
    Pending(String),
    /// No toolbar to ask: run the fallback now.
    Fallback,
}

/// An extension shortcut that must open a button's popup: the toolbar knows where the button is, so it is asked first.
/// No acknowledgement in time (toolbar hidden, compact mode, window without one) runs the fallback instead.
#[derive(Default)]
pub struct ActionRelay {
    pending: HashMap<String, (u64, String)>,
    count: u64,
}

impl ActionRelay {
    pub fn new() -> Self {
        Self::default()
    }

    /// `send(token, extension_id)` asks the toolbar of the right window; it returns false when there is none.
    pub fn request(&mut self, extension_id: &str, now_ms: u64, send: impl FnOnce(&str, &str) -> bool) -> RelayOutcome {
        self.count += 1;
        let token = format!("a{}", self.count);
        if !send(&token, extension_id) {
            return RelayOutcome::Fallback;
        }
        self.pending.insert(token.clone(), (now_ms + ACK_TIMEOUT_MS, extension_id.to_string()));
        RelayOutcome::Pending(token)
    }

    /// The toolbar takes the request. True once per token, and only before the timeout.
    pub fn ack(&mut self, token: &str, now_ms: u64) -> bool {
        match self.pending.get(token) {
            Some((deadline, _)) if *deadline > now_ms => {
                self.pending.remove(token);
                true
            }
            _ => false,
        }
    }

    /// Requests whose time ran out: the extensions to run the fallback for.
    pub fn expire(&mut self, now_ms: u64) -> Vec<String> {
        let due: Vec<String> = self.pending.iter().filter(|(_, (d, _))| *d <= now_ms).map(|(t, _)| t.clone()).collect();
        let mut tokens = due;
        tokens.sort();
        tokens.into_iter().filter_map(|t| self.pending.remove(&t).map(|(_, id)| id)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;

    fn snap(items: &[(&str, &str)]) -> Snapshot {
        items.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    struct World {
        s: ReloadScheduler,
        disk: Rc<RefCell<Snapshot>>,
        log: Rc<RefCell<Vec<&'static str>>>,
    }

    fn setup(initial: &[(&str, &str)]) -> World {
        let s = ReloadScheduler::new(400);
        s.baseline(snap(initial));
        World { s, disk: Rc::new(RefCell::new(snap(initial))), log: Rc::new(RefCell::new(vec![])) }
    }

    impl World {
        fn write(&self, file: &str, sig: &str) {
            self.disk.borrow_mut().insert(file.into(), sig.into());
        }
        fn fire(&self, now: u64) {
            if self.s.take_due(now) {
                let disk = self.disk.clone();
                let log = self.log.clone();
                self.s.check(&mut || Ok(disk.borrow().clone()), &mut || log.borrow_mut().push("reload"));
            }
        }
    }

    #[test]
    fn same_snapshot_compares_files_and_signatures() {
        assert!(same_snapshot(&snap(&[("a", "1")]), &snap(&[("a", "1")])));
        assert!(!same_snapshot(&snap(&[("a", "1")]), &snap(&[("a", "2")])));
        assert!(!same_snapshot(&snap(&[("a", "1")]), &snap(&[("a", "1"), ("b", "1")])));
    }

    #[test]
    fn ignores_events_that_change_nothing() {
        let w = setup(&[("manifest.json", "1:1")]);
        w.s.notify(0);
        w.s.notify(10);
        w.fire(1000);
        assert!(w.log.borrow().is_empty());
    }

    #[test]
    fn reloads_once_for_a_burst_of_real_changes() {
        let w = setup(&[("a.js", "1:1")]);
        w.write("a.js", "2:2");
        w.s.notify(0);
        w.s.notify(5);
        w.s.notify(9);
        w.fire(100);
        assert!(w.log.borrow().is_empty(), "debounced");
        w.fire(500);
        assert_eq!(*w.log.borrow(), ["reload"]);
        w.s.notify(600);
        w.fire(1100);
        assert_eq!(*w.log.borrow(), ["reload"]);
    }

    #[test]
    fn never_reloads_while_a_reload_runs_then_checks_once_more() {
        let w = setup(&[("a.js", "1:1")]);
        w.write("a.js", "2:2");
        w.s.notify(0);
        assert!(w.s.take_due(500));
        let disk = w.disk.clone();
        let log = w.log.clone();
        let s = &w.s;
        let mut first = true;
        s.check(&mut || Ok(disk.borrow().clone()), &mut || {
            log.borrow_mut().push("reload");
            if first {
                first = false;
                assert!(s.running());
                // A file changes and a check is requested while the reload runs.
                disk.borrow_mut().insert("a.js".into(), "3:3".into());
                s.check(&mut || Ok(disk.borrow().clone()), &mut || log.borrow_mut().push("nested"));
            }
        });
        assert_eq!(*w.log.borrow(), ["reload", "reload"]);
        assert!(!w.s.running());
    }

    #[test]
    fn does_not_reload_after_dispose() {
        let w = setup(&[("a.js", "1:1")]);
        w.write("a.js", "2:2");
        w.s.dispose();
        w.s.notify(0);
        w.fire(1000);
        assert!(w.log.borrow().is_empty());
    }

    #[test]
    fn a_failed_load_has_no_baseline_so_any_event_reloads() {
        let s = ReloadScheduler::new(400);
        let mut n = 0;
        s.check(&mut || Ok(snap(&[("a", "1")])), &mut || n += 1);
        assert_eq!(n, 1);
    }

    #[test]
    fn safe_reload_unloads_waits_for_the_worker_then_loads() {
        let order = RefCell::new(Vec::new());
        safe_reload(SafeReloadDeps {
            is_loaded: &mut || true,
            unload: &mut || order.borrow_mut().push("unload"),
            worker_stopped: &mut || order.borrow_mut().push("worker-stopped"),
            load: &mut || order.borrow_mut().push("load"),
        });
        assert_eq!(*order.borrow(), ["unload", "worker-stopped", "load"]);
    }

    #[test]
    fn safe_reload_loads_directly_when_not_loaded() {
        let order = RefCell::new(Vec::new());
        safe_reload(SafeReloadDeps {
            is_loaded: &mut || false,
            unload: &mut || order.borrow_mut().push("unload"),
            worker_stopped: &mut || order.borrow_mut().push("w"),
            load: &mut || order.borrow_mut().push("load"),
        });
        assert_eq!(*order.borrow(), ["load"]);
    }

    #[test]
    fn ignored_files() {
        for f in [".git/HEAD", "node_modules/x/y.js", "a/.DS_Store", "file.js~", ".a.swp", "b.swo"] {
            assert!(is_ignored(f), "{f}");
        }
        assert!(!is_ignored("src/content.js"));
    }

    #[test]
    fn scan_folder_lists_real_files_only() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("manifest.json"), "{}").unwrap();
        fs::create_dir(dir.path().join(".git")).unwrap();
        fs::write(dir.path().join(".git/HEAD"), "x").unwrap();
        fs::create_dir(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/a.js"), "1").unwrap();
        let s = scan_folder(dir.path()).unwrap();
        assert_eq!(s.keys().cloned().collect::<Vec<_>>(), ["manifest.json", "src/a.js"]);
        fs::write(dir.path().join("src/a.js"), "12").unwrap();
        assert!(!same_snapshot(&s, &scan_folder(dir.path()).unwrap()));
    }

    // ---- ActionRelay ----

    #[test]
    fn relay_does_nothing_else_when_the_toolbar_acknowledges_in_time() {
        let mut relay = ActionRelay::new();
        let mut sent = Vec::new();
        let RelayOutcome::Pending(token) = relay.request("ext", 0, |t, id| {
            sent.push((t.to_string(), id.to_string()));
            true
        }) else {
            panic!("pending")
        };
        assert_eq!(sent[0].1, "ext");
        assert!(relay.ack(&token, 10));
        assert!(relay.expire(ACK_TIMEOUT_MS * 2).is_empty());
    }

    #[test]
    fn relay_falls_back_after_the_timeout_and_a_late_ack_is_refused() {
        let mut relay = ActionRelay::new();
        let RelayOutcome::Pending(token) = relay.request("ext", 0, |_, _| true) else { panic!() };
        assert!(relay.expire(ACK_TIMEOUT_MS - 1).is_empty());
        assert_eq!(relay.expire(ACK_TIMEOUT_MS), vec!["ext".to_string()]);
        assert!(!relay.ack(&token, ACK_TIMEOUT_MS));
    }

    #[test]
    fn relay_falls_back_at_once_without_a_toolbar() {
        let mut relay = ActionRelay::new();
        assert_eq!(relay.request("ext", 0, |_, _| false), RelayOutcome::Fallback);
    }

    #[test]
    fn relay_tokens_are_independent_and_acked_once() {
        let mut relay = ActionRelay::new();
        let RelayOutcome::Pending(a) = relay.request("a", 0, |_, _| true) else { panic!() };
        let RelayOutcome::Pending(_b) = relay.request("b", 0, |_, _| true) else { panic!() };
        assert!(relay.ack(&a, 1));
        assert!(!relay.ack(&a, 2));
        assert!(!relay.ack("unknown", 2));
        assert_eq!(relay.expire(ACK_TIMEOUT_MS), vec!["b".to_string()]);
    }
}

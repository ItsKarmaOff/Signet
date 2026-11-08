//! Downloads (port of the model of `main/web/downloads.ts`): the list shown in the Library, file naming, progress.
//! The CEF download callbacks, save dialog and Dock progress belong to the app, which drives this model.
use std::collections::HashSet;
use std::path::{Path, PathBuf};


/// Entries kept in the list.
pub const DOWNLOADS_MAX: usize = 30;
/// Minimum delay between two progress pushes to the UI.
pub const PUSH_INTERVAL_MS: i64 = 250;

pub use signet_core::types::DownloadStatus;

pub use signet_core::types::DownloadInfo;

/// Folder where downloads go: the one from Settings if it still exists, otherwise the system's Downloads folder.
pub fn download_dir(setting: Option<&str>, system_downloads: &Path) -> PathBuf {
    match setting.filter(|d| !d.is_empty()).map(PathBuf::from) {
        Some(d) if d.is_dir() => d,
        _ => system_downloads.to_path_buf(),
    }
}

/// `name.ext` becomes `name (1).ext`, `name (2).ext`... as long as the file exists or a download reserves it.
pub fn unique_path(dir: &Path, filename: &str, reserved: &HashSet<PathBuf>, exists: &dyn Fn(&Path) -> bool) -> PathBuf {
    let name = safe_file_name(filename);
    let p = Path::new(&name);
    let ext = p.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| name.clone());
    let mut path = dir.join(&name);
    let mut n = 1;
    while exists(&path) || reserved.contains(&path) {
        path = dir.join(format!("{stem} ({n}){ext}"));
        n += 1;
    }
    path
}

/// File name without path separators or control characters (a server-suggested name is not trusted).
pub fn safe_file_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("");
    let clean: String = base.chars().filter(|c| !c.is_control()).collect();
    let clean = clean.trim().trim_matches('.').to_string();
    if clean.is_empty() { "download".into() } else { clean }
}

/// Dock progress bar value: `-1.0` with no download, `2.0` (indeterminate) if a size is unknown.
pub fn progress(list: &[DownloadInfo], active: &HashSet<String>) -> f64 {
    let act: Vec<&DownloadInfo> = list.iter().filter(|d| active.contains(&d.id)).collect();
    if act.is_empty() {
        return -1.0;
    }
    if act.iter().any(|d| d.total_bytes == 0) {
        return 2.0;
    }
    let total: u64 = act.iter().map(|d| d.total_bytes).sum();
    act.iter().map(|d| d.received_bytes).sum::<u64>() as f64 / total as f64
}

/// The downloads list with the rules of the Electron `Downloads` class.
#[derive(Default)]
pub struct DownloadList {
    pub list: Vec<DownloadInfo>,
    /// Ids of downloads in progress (not finished).
    pub active: HashSet<String>,
    last_push: i64,
}

impl DownloadList {
    pub fn new(list: Vec<DownloadInfo>) -> Self {
        DownloadList { list, ..Default::default() }
    }

    /// Paths reserved by downloads in progress.
    pub fn reserved(&self) -> HashSet<PathBuf> {
        self.list.iter().filter(|d| self.active.contains(&d.id)).map(|d| PathBuf::from(&d.path)).collect()
    }

    /// Registers a new download at the top of the list.
    pub fn start(&mut self, path: &Path, url: &str, total_bytes: u64, private: bool, now: i64) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        self.list.insert(0, DownloadInfo {
            id: id.clone(),
            filename: path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default(),
            path: path.to_string_lossy().into_owned(),
            url: url.into(),
            status: DownloadStatus::Progressing,
            received_bytes: 0,
            total_bytes,
            started_at: now,
            private: private.then_some(true),
        });
        self.list.truncate(DOWNLOADS_MAX);
        self.active.insert(id.clone());
        id
    }

    /// Progress update. Returns `(push, persist)`: a state change is never held back by the delay.
    pub fn update(&mut self, id: &str, received: u64, total: u64, interrupted: bool, paused: bool, now: i64) -> Option<(bool, bool)> {
        let d = self.list.iter_mut().find(|d| d.id == id)?;
        let before = d.status;
        d.received_bytes = received;
        d.total_bytes = total;
        d.status = if interrupted { DownloadStatus::Interrupted } else if paused { DownloadStatus::Paused } else { DownloadStatus::Progressing };
        if d.status == before && now - self.last_push < PUSH_INTERVAL_MS {
            return Some((false, false));
        }
        self.last_push = now;
        Some((true, d.status != before))
    }

    /// The path chosen in the save dialog (known once the dialog is closed).
    pub fn set_path(&mut self, id: &str, path: &Path) {
        if let Some(d) = self.list.iter_mut().find(|d| d.id == id) {
            d.path = path.to_string_lossy().into_owned();
            d.filename = path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
        }
    }

    /// Download finished. `dialog_cancelled`: the save dialog was cancelled, nothing downloaded: the entry disappears.
    /// `private_window_closed`: finished after its private window closed: it does not stay in the list.
    pub fn done(&mut self, id: &str, status: DownloadStatus, received: u64, total: u64, dialog_cancelled: bool, private_window_closed: bool) {
        self.active.remove(id);
        if dialog_cancelled || private_window_closed {
            self.list.retain(|d| d.id != id);
            return;
        }
        if let Some(d) = self.list.iter_mut().find(|d| d.id == id) {
            d.received_bytes = received;
            d.total_bytes = total;
            d.status = status;
        }
    }

    /// Private window closed: its finished downloads leave the list. Returns true if something was removed.
    pub fn drop_private_finished(&mut self) -> bool {
        let n = self.list.len();
        let active = self.active.clone();
        self.list.retain(|d| !(d.private == Some(true) && !active.contains(&d.id)));
        self.list.len() != n
    }

    /// Removes everything except downloads in progress.
    pub fn clear(&mut self) {
        let active = self.active.clone();
        self.list.retain(|d| active.contains(&d.id));
    }

    /// Retry: `None` if unknown or completed; otherwise the URL to restart (the row is replaced) unless `resumable`.
    pub fn retry(&mut self, id: &str, resumable: bool) -> Option<Retry> {
        let info = self.list.iter().find(|d| d.id == id)?.clone();
        if info.status == DownloadStatus::Completed {
            return None;
        }
        if resumable && self.active.contains(id) {
            return Some(Retry::Resume);
        }
        self.list.retain(|d| d.id != id);
        self.active.remove(id);
        Some(Retry::Restart(info.url))
    }

    pub fn progress(&self) -> f64 {
        progress(&self.list, &self.active)
    }

    pub fn path_of(&self, id: &str) -> Option<&str> {
        self.list.iter().find(|d| d.id == id).map(|d| d.path.as_str())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Retry {
    Resume,
    Restart(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unique_names() {
        let d = Path::new("/dl");
        let taken: HashSet<PathBuf> = ["/dl/a.zip", "/dl/a (1).zip"].iter().map(PathBuf::from).collect();
        let exists = |p: &Path| taken.contains(p);
        assert_eq!(unique_path(d, "b.txt", &HashSet::new(), &exists), PathBuf::from("/dl/b.txt"));
        assert_eq!(unique_path(d, "a.zip", &HashSet::new(), &exists), PathBuf::from("/dl/a (2).zip"));
        let reserved: HashSet<PathBuf> = [PathBuf::from("/dl/b.txt")].into();
        assert_eq!(unique_path(d, "b.txt", &reserved, &exists), PathBuf::from("/dl/b (1).txt"));
        assert_eq!(unique_path(d, "noext", &HashSet::new(), &|p| p == Path::new("/dl/noext")), PathBuf::from("/dl/noext (1)"));
    }

    #[test]
    fn server_names_are_not_trusted() {
        assert_eq!(safe_file_name("../../etc/passwd"), "passwd");
        assert_eq!(safe_file_name("C:\\x\\evil.exe"), "evil.exe");
        assert_eq!(safe_file_name("a\u{0}b\n.txt"), "ab.txt");
        assert_eq!(safe_file_name(""), "download");
        assert_eq!(safe_file_name(".."), "download");
    }

    #[test]
    fn download_folder_fallback() {
        let d = tempfile::tempdir().unwrap();
        let sys = Path::new("/system/Downloads");
        assert_eq!(download_dir(Some(d.path().to_str().unwrap()), sys), d.path());
        assert_eq!(download_dir(Some("/does/not/exist"), sys), sys);
        assert_eq!(download_dir(None, sys), sys);
        assert_eq!(download_dir(Some(""), sys), sys);
    }

    #[test]
    fn list_lifecycle() {
        let mut l = DownloadList::default();
        let a = l.start(Path::new("/dl/a.zip"), "https://x/a", 100, false, 1);
        assert_eq!((l.list[0].filename.as_str(), l.list[0].status), ("a.zip", DownloadStatus::Progressing));
        assert_eq!(l.reserved(), [PathBuf::from("/dl/a.zip")].into());
        assert_eq!(l.progress(), 0.0);
        // Plain progress is throttled, a state change is not.
        assert_eq!(l.update(&a, 10, 100, false, false, 1000), Some((true, false)));
        assert_eq!(l.update(&a, 20, 100, false, false, 1100), Some((false, false)));
        assert_eq!(l.update(&a, 20, 100, false, true, 1110), Some((true, true)));
        assert_eq!(l.list[0].status, DownloadStatus::Paused);
        assert_eq!(l.update("ghost", 0, 0, false, false, 0), None);
        l.update(&a, 50, 100, false, false, 5000);
        assert_eq!(l.progress(), 0.5);
        l.update(&a, 50, 0, false, false, 9000);
        assert_eq!(l.progress(), 2.0);
        l.done(&a, DownloadStatus::Completed, 100, 100, false, false);
        assert_eq!(l.progress(), -1.0);
        assert_eq!(l.list[0].status, DownloadStatus::Completed);
        assert!(l.reserved().is_empty());
        assert_eq!(l.retry(&a, false), None);
    }

    #[test]
    fn cancel_retry_clear_private() {
        let mut l = DownloadList::default();
        let a = l.start(Path::new("/dl/a"), "https://x/a", 0, false, 1);
        l.done(&a, DownloadStatus::Cancelled, 0, 0, true, false);
        assert!(l.list.is_empty());
        let b = l.start(Path::new("/dl/b"), "https://x/b", 0, false, 2);
        l.done(&b, DownloadStatus::Interrupted, 5, 0, false, false);
        assert_eq!(l.retry(&b, false), Some(Retry::Restart("https://x/b".into())));
        assert!(l.list.is_empty());
        let c = l.start(Path::new("/dl/c"), "https://x/c", 0, false, 3);
        l.update(&c, 1, 0, true, false, 100);
        assert_eq!(l.retry(&c, true), Some(Retry::Resume));
        let p = l.start(Path::new("/dl/p"), "https://x/p", 0, true, 4);
        l.done(&p, DownloadStatus::Completed, 1, 1, false, false);
        assert!(l.drop_private_finished());
        assert!(l.list.iter().all(|d| d.private != Some(true)));
        let q = l.start(Path::new("/dl/q"), "https://x/q", 0, true, 5);
        l.done(&q, DownloadStatus::Completed, 1, 1, false, true);
        assert!(l.list.iter().all(|d| d.id != q));
        let d2 = l.start(Path::new("/dl/d"), "https://x/d", 0, false, 6);
        l.done(&c, DownloadStatus::Completed, 1, 1, false, false);
        l.clear();
        assert_eq!(l.list.iter().map(|d| d.id.clone()).collect::<Vec<_>>(), vec![d2]);
    }

    #[test]
    fn list_is_capped_and_json_matches_electron() {
        let mut l = DownloadList::default();
        for i in 0..DOWNLOADS_MAX + 5 {
            l.start(Path::new(&format!("/dl/{i}")), "https://x", 1, false, i as i64);
        }
        assert_eq!(l.list.len(), DOWNLOADS_MAX);
        let v = serde_json::to_value(&l.list[0]).unwrap();
        assert!(v.get("receivedBytes").is_some() && v.get("startedAt").is_some() && v.get("private").is_none());
        l.set_path(&l.list[0].id.clone(), Path::new("/chosen/new.bin"));
        assert_eq!(l.list[0].filename, "new.bin");
        let back: DownloadInfo = serde_json::from_value(serde_json::json!({"id":"1","filename":"f","path":"/p","url":"u","status":"progressing","receivedBytes":1,"totalBytes":0,"startedAt":2,"private":true})).unwrap();
        assert_eq!(back.private, Some(true));
    }
}

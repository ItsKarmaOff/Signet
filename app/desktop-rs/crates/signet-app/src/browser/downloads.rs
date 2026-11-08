//! Port of `web/downloads.ts` wiring: CEF download events to the `DownloadList` model, files in the Downloads folder.

use std::collections::HashMap;
use std::path::PathBuf;

use signet_core::ids::now_ms;
use signet_core::types::{DownloadInfo, DownloadStatus};
use signet_features::downloads::{download_dir, unique_path, DownloadList, Retry};
use signet_web::{DownloadProgress, DownloadRequest, DownloadState};

use super::Core;
use crate::app::platform;

pub struct Downloads {
    pub model: DownloadList,
    /// CEF download id to list id.
    ids: HashMap<u32, String>,
}

impl Downloads {
    pub fn new(list: Vec<DownloadInfo>) -> Self {
        Downloads { model: DownloadList::new(list), ids: HashMap::new() }
    }

    pub fn list(&self) -> Vec<DownloadInfo> {
        self.model.list.clone()
    }

    /// The list to write to `state.json` (private downloads are never written).
    pub fn persisted(&self) -> Vec<DownloadInfo> {
        self.model.list.iter().filter(|d| d.private != Some(true)).cloned().collect()
    }

    pub fn any_active(&self) -> bool {
        !self.model.active.is_empty()
    }
}

impl Core {
    pub fn download_dir(&self) -> PathBuf {
        download_dir(self.state().settings.download_dir.as_deref(), &platform::system_downloads())
    }

    pub fn on_download_starting(&mut self, req: DownloadRequest) {
        let dir = self.download_dir();
        let _ = std::fs::create_dir_all(&dir);
        let reserved = self.downloads.model.reserved();
        let path = unique_path(&dir, &req.suggested_name, &reserved, &|p| p.exists());
        let id = self.downloads.model.start(&path, &req.url, req.total_bytes.max(0) as u64, false, now_ms());
        self.downloads.ids.insert(req.id, id);
        self.log(format!("download {} -> {}", req.url, path.display()));
        req.save_to(path.to_string_lossy().into_owned());
        self.changed_view_state();
    }

    pub fn on_download_progress(&mut self, p: DownloadProgress) {
        let Some(id) = self.downloads.ids.get(&p.id).cloned() else { return };
        match p.state {
            DownloadState::InProgress => {
                let interrupted = false;
                if let Some((push, _)) = self.downloads.model.update(&id, p.received_bytes.max(0) as u64, p.total_bytes.max(0) as u64, interrupted, p.paused, now_ms())
                    && push
                {
                    self.changed_view_state();
                }
            }
            state => {
                let status = match state {
                    DownloadState::Complete => DownloadStatus::Completed,
                    DownloadState::Canceled => DownloadStatus::Cancelled,
                    _ => DownloadStatus::Interrupted,
                };
                if !p.path.is_empty() {
                    self.downloads.model.set_path(&id, std::path::Path::new(&p.path));
                }
                self.downloads.model.done(&id, status, p.received_bytes.max(0) as u64, p.total_bytes.max(0) as u64, false, false);
                self.downloads.ids.remove(&p.id);
                self.store.schedule(signet_core::core::store::SAVE_DELAY);
                self.changed_view_state();
            }
        }
    }

    /// State only (downloads list changed): send the snapshot, schedule the save for finished ones.
    fn changed_view_state(&mut self) {
        self.store.data.state.downloads = self.downloads.persisted();
        self.dirty.needs_full = true;
        self.apply();
    }

    pub fn open_download(&mut self, id: &str) {
        if let Some(p) = self.downloads.model.path_of(id) {
            platform::open_external(p);
        }
    }

    pub fn reveal_download(&mut self, id: &str) {
        if let Some(p) = self.downloads.model.path_of(id) {
            platform::reveal(p);
        }
    }

    pub fn cancel_download(&mut self, id: &str) {
        if let Some((cef, _)) = self.downloads.ids.iter().find(|(_, v)| v.as_str() == id) {
            signet_web::cancel_download(*cef);
        }
    }

    pub fn retry_download(&mut self, id: &str) {
        let cef = self.downloads.ids.iter().find(|(_, v)| v.as_str() == id).map(|(k, _)| *k);
        match self.downloads.model.retry(id, cef.is_some()) {
            Some(Retry::Resume) => {
                if let Some(c) = cef {
                    signet_web::resume_download(c);
                }
            }
            Some(Retry::Restart(url)) => {
                if let Some(v) = self.target_view(None) {
                    v.start_download(&url);
                }
                self.changed_view_state();
            }
            None => {}
        }
    }

    pub fn clear_downloads(&mut self) {
        self.downloads.model.clear();
        self.changed_view_state();
    }
}

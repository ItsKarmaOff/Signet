//! Ports `packages/shared/src/types/library.ts`.

use serde::{Deserialize, Serialize};

/// Maximum number of history entries returned per query.
pub const HISTORY_QUERY_MAX: usize = 500;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct HistoryEntry {
    pub url: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub favicon: Option<String>,
    pub visits: u32,
    /// Times the page was reached from the command bar (typed or picked), like Chrome's `typed_count`. Missing = 0.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub typed: Option<u32>,
    pub last_visit: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct ArchivedEntry {
    /// Stable identifier: unique per entry (`${archivedAt}:${url}` for old ones).
    pub id: String,
    pub url: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub favicon: Option<String>,
    pub archived_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LibrarySection {
    History,
    Archive,
    Downloads,
    Spaces,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum DownloadStatus {
    #[default]
    Progressing,
    Paused,
    Completed,
    Cancelled,
    Interrupted,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct DownloadInfo {
    pub id: String,
    pub filename: String,
    /// Full path of the file on disk.
    pub path: String,
    pub url: String,
    pub status: DownloadStatus,
    pub received_bytes: u64,
    /// 0 if the size is unknown.
    pub total_bytes: u64,
    pub started_at: i64,
    /// Launched from a private window: never written to disk.
    #[serde(rename = "private", skip_serializing_if = "Option::is_none")]
    pub private: Option<bool>,
}

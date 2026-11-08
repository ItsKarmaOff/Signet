//! Ports `app/desktop/src/shared/types/data.ts`.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// Data an import can take from another browser (`spaces`: Arc spaces, pinned tabs and favorites).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImportKind {
    Spaces,
    Bookmarks,
    Passwords,
    Extensions,
    History,
}

pub const IMPORT_KINDS: [ImportKind; 5] =
    [ImportKind::Spaces, ImportKind::Bookmarks, ImportKind::Passwords, ImportKind::Extensions, ImportKind::History];

/// Parts of a backup (`.signetbackup`), in Settings order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BackupSection {
    Settings,
    Favorites,
    Spaces,
    Today,
    Bookmarks,
    History,
    Archive,
    Boosts,
    Permissions,
    Mods,
    Notes,
    Passwords,
    Extensions,
}

pub const BACKUP_SECTIONS: [BackupSection; 13] = [
    BackupSection::Settings,
    BackupSection::Favorites,
    BackupSection::Spaces,
    BackupSection::Today,
    BackupSection::Bookmarks,
    BackupSection::History,
    BackupSection::Archive,
    BackupSection::Boosts,
    BackupSection::Permissions,
    BackupSection::Mods,
    BackupSection::Notes,
    BackupSection::Passwords,
    BackupSection::Extensions,
];

/// Import: add to existing data or replace it (settings are always replaced).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BackupMode {
    Add,
    Replace,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SafetyCopy {
    pub name: String,
    pub created_at: i64,
}

/// Settings Backup section: item count per part (current) and safety copies.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct BackupInfo {
    pub counts: IndexMap<BackupSection, i64>,
    /// Copies made before each import, from newest to oldest.
    pub safety: Vec<SafetyCopy>,
}

/// Backup file that has been read, awaiting import: parts present (`passwords`: -1, count unknown before decryption).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct BackupSummary {
    pub token: String,
    pub created_at: i64,
    pub app_version: String,
    pub counts: IndexMap<BackupSection, i64>,
    /// Safety copy: passwords are restored without a backup password.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_sealed: Option<bool>,
}

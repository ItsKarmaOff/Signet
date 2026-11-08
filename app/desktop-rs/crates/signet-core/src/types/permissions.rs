//! Ports `app/desktop/src/shared/types/permissions.ts`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PermissionState {
    Ask,
    Allow,
    Deny,
}

/// Remembered decision (`allow` or `deny`; `ask` is never stored).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PermissionDecision {
    Allow,
    Deny,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SitePermissionEntry {
    pub origin: String,
    pub permission: String,
    pub decision: PermissionDecision,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SitePermissionRow {
    pub id: String,
    pub label: String,
    pub state: PermissionState,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SiteBoostFlags {
    pub exists: bool,
    pub enabled: bool,
}

/// What the site panel (click on the padlock) shows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SiteInfo {
    pub tab_id: String,
    pub origin: String,
    /// Registrable domain, like `site_of()`.
    pub site: String,
    pub secure: bool,
    pub permissions: Vec<SitePermissionRow>,
    pub boost: SiteBoostFlags,
    /// Requests blocked by the ad blocker since this tab last loaded.
    pub blocked: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub favicon: Option<String>,
}

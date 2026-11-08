//! Ports `app/desktop/src/shared/types/extensions.ts`.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ExtensionCommand {
    pub name: String,
    pub description: String,
    pub shortcut: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct UnpackedInfo {
    pub path: String,
    pub display: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reloaded_at: Option<i64>,
}

/// Installed extension.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct ExtensionInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    /// Icon as a data URL, if the extension declares one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// True if the extension has a toolbar button.
    pub has_action: bool,
    /// Keyboard commands from the manifest, with the shortcut each one really has.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commands: Option<Vec<ExtensionCommand>>,
    /// Unpacked extension, loaded from a folder (empty id if it could not be loaded).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unpacked: Option<UnpackedInfo>,
}

/// Blocked-request counts shown on extension buttons: tab id, then extension id, then count (always above 0).
pub type ExtensionActionCounts = IndexMap<String, IndexMap<String, u32>>;

/// Toolbar button state set by `action.enable/disable/setBadgeTextColor`. Only what differs from the defaults.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct ExtensionActionState {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_color: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ExtensionActionEntry {
    #[serde(flatten)]
    pub state: ExtensionActionState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tabs: Option<IndexMap<String, ExtensionActionState>>,
}

pub type ExtensionActionStates = IndexMap<String, ExtensionActionEntry>;

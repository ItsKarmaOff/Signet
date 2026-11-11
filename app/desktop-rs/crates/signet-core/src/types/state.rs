//! Ports `app/desktop/src/shared/types/state.ts`.

use super::library::DownloadInfo;
use super::settings::Settings;
use super::spaces::Space;
use super::tabs::{Divider, MediaInfo, Pane, TabInfo};
use crate::slide::SlidePlan;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// Everything the main window needs to render itself.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Snapshot {
    pub tabs: IndexMap<String, TabInfo>,
    pub favorites: Vec<String>,
    pub spaces: Vec<Space>,
    pub active_space_id: String,
    pub sidebar_visible: bool,
    #[serde(serialize_with = "super::f64_js")]
    pub sidebar_width: f64,
    /// Sidebar slide in progress; None at rest.
    pub sidebar_slide: Option<SlidePlan>,
    /// Areas occupied by the displayed web views, in window coordinates.
    pub panes: Vec<Pane>,
    pub settings: Settings,
    /// Newest to oldest, 30 max.
    pub downloads: Vec<DownloadInfo>,
    /// True when the hidden sidebar is shown floating.
    pub sidebar_floating: bool,
    /// True when, in compact mode, the toolbar is shown floating.
    pub toolbar_floating: bool,
    /// Dividers of the displayed split view.
    pub dividers: Vec<Divider>,
    /// Most recent media of a tab that is not displayed.
    pub media: Option<MediaInfo>,
    /// Sites that have an active Boost, for the toolbar indicator.
    pub boosted_sites: Vec<String>,
    /// True while the overlay covers the main window.
    pub overlay_open: bool,
    /// Corner radius of the main window (macOS); None when unknown or in fullscreen. The web cards are concentric with it.
    pub window_corner: Option<f64>,
}

/// Partial state update: only the changed fields of tabs already sent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct StatePatch {
    pub tabs: IndexMap<String, serde_json::Value>,
}

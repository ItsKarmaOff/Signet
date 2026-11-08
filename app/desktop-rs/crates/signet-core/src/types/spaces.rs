//! Ports `packages/shared/src/types/spaces.ts`.

use super::tabs::Item;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// Source of a live folder.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum LiveSource {
    Feed { url: String },
    Github { query: GithubQuery },
}

/// GitHub queries offered: review requests addressed to me, or my open pull requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GithubQuery {
    ReviewRequested,
    Mine,
}

/// State of a live folder (inside the folder itself).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveFolder {
    pub source: LiveSource,
    /// Last successful update (ms).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refreshed_at: Option<i64>,
    /// Last failure, translated message; absent if the last update succeeded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Update in progress (never read back at startup).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refreshing: Option<bool>,
    /// Addresses already opened: the others carry a "new" dot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seen: Option<Vec<String>>,
}

/// Routing rule: new tabs whose host matches open in `space_id`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct SpaceRoute {
    pub id: String,
    /// Domain (`github.com` also covers its subdomains) or pattern with `*` (`*.notion.site`).
    pub pattern: String,
    pub space_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SpaceProfile {
    #[default]
    Shared,
    Isolated,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Space {
    pub id: String,
    pub name: String,
    /// An emoji.
    pub icon: String,
    /// HSL hue 0-359 that colors the sidebar; with a gradient, it is its first color (accent).
    #[serde(serialize_with = "super::f64_js")]
    pub hue: f64,
    /// Background gradient of the space. Missing = solid color (`hue`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme: Option<SpaceTheme>,
    pub pinned: Vec<Item>,
    pub today: Vec<Item>,
    /// Tab that has focus in this space (can be a favorite or a tab of a split view).
    pub active_tab_id: Option<String>,
    /// `isolated`: cookies, storage and accounts specific to the space. Missing = `shared`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<SpaceProfile>,
    /// Day tab groups, by id (see `Item.group`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub groups: Option<IndexMap<String, super::tabs::TabGroup>>,
    /// Space bookmarks (bar below the address bar), in order; folders can nest.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bookmarks: Option<Vec<Bookmark>>,
}

impl Space {
    pub fn is_isolated(&self) -> bool {
        self.profile == Some(SpaceProfile::Isolated)
    }
}

/// Bookmark: a link, or a folder of bookmarks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Bookmark {
    Link {
        id: String,
        #[serde(default)]
        title: String,
        url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        favicon: Option<String>,
    },
    Folder {
        id: String,
        #[serde(default)]
        name: String,
        #[serde(default)]
        items: Vec<Bookmark>,
    },
}

impl Bookmark {
    pub fn id(&self) -> &str {
        match self {
            Bookmark::Link { id, .. } | Bookmark::Folder { id, .. } => id,
        }
    }
}

/// Where to drop a bookmark in the active space: at the root of the bar or in a folder, at position `index`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BookmarkTarget {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder_id: Option<String>,
    pub index: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct SpaceDraft {
    pub name: String,
    pub icon: String,
    #[serde(serialize_with = "super::f64_js")]
    pub hue: f64,
    /// Gradient; None = solid color.
    pub theme: Option<SpaceTheme>,
}

/// Background of a gradient space: 2 or 3 hues (the first is also `Space.hue`) and an optional grain.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct SpaceTheme {
    /// HSL hues 0-359, in gradient order (2 or 3).
    #[serde(serialize_with = "super::vec_f64_js")]
    pub hues: Vec<f64>,
    /// Texture grain, from 0 (smooth) to 1.
    pub grain: f64,
}

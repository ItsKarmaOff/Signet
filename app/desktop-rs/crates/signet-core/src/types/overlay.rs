//! Ports `app/desktop/src/shared/types/overlay.ts`.

use super::permissions::SiteInfo;
use super::tabs::{IconTarget, Rect};
use crate::searchengines::KeywordInfo;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PaletteMode {
    NewTab,
    EditUrl,
    Split,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", rename_all_fields = "camelCase")]
pub enum PaletteResult {
    Url {
        id: String,
        title: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        subtitle: Option<String>,
        url: String,
    },
    Search {
        id: String,
        title: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        subtitle: Option<String>,
        url: String,
    },
    Suggestion {
        id: String,
        title: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        subtitle: Option<String>,
        url: String,
    },
    /// "Press Tab to search ...": offered when the typed text is exactly a keyword. Never executed.
    Keyword {
        id: String,
        title: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        subtitle: Option<String>,
        keyword: KeywordInfo,
    },
    History {
        id: String,
        title: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        subtitle: Option<String>,
        url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        favicon: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        inline: Option<bool>,
    },
    Archived {
        id: String,
        title: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        subtitle: Option<String>,
        url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        favicon: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        inline: Option<bool>,
    },
    /// `inline`: set on the top row only, when the page was typed or visited enough to complete the input.
    Tab {
        id: String,
        title: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        subtitle: Option<String>,
        tab_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        url: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        favicon: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        inline: Option<bool>,
    },
    Action {
        id: String,
        title: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        subtitle: Option<String>,
        action: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        shortcut: Option<String>,
        /// Preview (data URL) shown in place of the icon.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        image: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AutofillKind {
    Address,
    Card,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoginRef {
    pub id: String,
    pub username: String,
}

/// What the main window's overlay must show.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "lowercase", rename_all_fields = "camelCase")]
pub enum OverlayPayload {
    Palette {
        palette_mode: PaletteMode,
        initial: String,
        hue: f64,
        keywords: Vec<KeywordInfo>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        anchor: Option<Rect>,
    },
    Peek {
        tab_id: String,
        url: String,
        title: String,
        card: Rect,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        origin: Option<Point>,
    },
    /// Site panel, anchored under the toolbar's padlock.
    Site { info: SiteInfo, anchor: Rect },
    /// Drop zones over the page while a tab is dragged from the sidebar.
    Dropzones { area: Rect, split_card: Option<Rect> },
    /// Dropdown menu or form of the tab row, anchored under `anchor`.
    Popover { popover: PopoverRequest, anchor: Rect, hue: f64 },
    /// Prompt to save (or update) a password, at the top right of `card`.
    Password {
        token: String,
        origin: String,
        username: String,
        update: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kind: Option<AutofillKind>,
        card: Rect,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        anchor: Option<Rect>,
    },
    /// Saved accounts of the site (without passwords), under the key button.
    Passwords { origin: String, logins: Vec<LoginRef>, anchor: Rect },
    /// Screenshot tool placed over the tab's web view.
    Capture { tab_id: String, rect: Rect, hue: f64 },
    /// Zoom bubble at the top right of `card`.
    Zoom { tab_id: String, percent: u32, default_percent: u32, card: Rect, compact: bool },
    /// Install the site as an app, at the top right of `card`.
    Webapp {
        token: String,
        name: String,
        url: String,
        icon: Option<String>,
        maskable: bool,
        hue: f64,
        card: Rect,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum RenameTarget {
    Tab { tab_id: String },
    Folder { folder_id: String },
    Group { group_id: String },
    Bookmark { id: String },
}

/// What a tab-row popover shows (`horizontal` layout).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", rename_all_fields = "camelCase")]
pub enum PopoverRequest {
    Spaces,
    SpaceForm { space_id: Option<String> },
    /// Live folder form: creation (`folder_id` None) or source change.
    LiveFolder { folder_id: Option<String> },
    Folder { folder_id: String },
    Downloads,
    Icon { target: IconTarget },
    Rename { target: RenameTarget },
    /// Contents of a bookmarks folder (or, with `folder_id` None, of the bar starting at `from`).
    Bookmarks {
        folder_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        from: Option<usize>,
        tab_id: Option<String>,
    },
}

/// "Hold Cmd+Q to quit" bubble.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct QuitHudState {
    pub progress: f64,
    pub holding: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wait: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repeating: Option<bool>,
}

//! Ports `packages/shared/src/types/tabs.ts` and `app/desktop/src/shared/types/tabs.ts`.

use super::devtools::{DevOverrides, DevResponsive, DevToolsDock};
use super::spaces::LiveFolder;
use serde::{Deserialize, Serialize};

fn yes() -> bool {
    true
}

/// A sidebar item: a single tab, a split view, or a folder (folders only exist among the pinned tabs).
///
/// Tab group (day tabs only): a run of at least two consecutive items carrying the same `group`; its title, color and
/// collapsed state are in `Space.groups`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Item {
    Tab(TabItem),
    Split(SplitItem),
    Folder(FolderItem),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct TabItem {
    pub tab_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct SplitItem {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// 2 to 4 tabs.
    pub tab_ids: Vec<String>,
    /// Disposition; missing = `columns`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layout: Option<SplitLayout>,
    /// Parts (sum 1). `columns` / `rows`: one part per tab. `grid`: [top row, bottom row]. Missing = equal parts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sizes: Option<Vec<f64>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FolderItem {
    pub id: String,
    pub name: String,
    /// Emoji chosen in place of the folder icon.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default = "yes")]
    pub open: bool,
    pub items: Vec<Item>,
    /// Hue of the folder card; missing = neutral hue.
    #[serde(serialize_with = "super::opt_f64_js", skip_serializing_if = "Option::is_none")]
    pub hue: Option<f64>,
    /// Live folder: its tabs come from a source (feed, GitHub) and update on their own.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub live: Option<LiveFolder>,
}

impl Default for FolderItem {
    fn default() -> Self {
        Self { id: String::new(), name: String::new(), icon: None, open: true, items: Vec::new(), hue: None, live: None }
    }
}

/// Item that directly carries tabs (not a folder).
pub type Folder = FolderItem;

impl Item {
    pub fn tab(tab_id: impl Into<String>) -> Item {
        Item::Tab(TabItem { tab_id: tab_id.into(), group: None })
    }
    pub fn is_folder(&self) -> bool {
        matches!(self, Item::Folder(_))
    }
    /// Group label of a tab or split view (None for a folder).
    pub fn group(&self) -> Option<&str> {
        match self {
            Item::Tab(t) => t.group.as_deref(),
            Item::Split(s) => s.group.as_deref(),
            Item::Folder(_) => None,
        }
    }
    pub fn set_group(&mut self, group: Option<String>) {
        match self {
            Item::Tab(t) => t.group = group,
            Item::Split(s) => s.group = group,
            Item::Folder(_) => {}
        }
    }
    pub fn as_folder(&self) -> Option<&FolderItem> {
        if let Item::Folder(f) = self { Some(f) } else { None }
    }
    pub fn as_folder_mut(&mut self) -> Option<&mut FolderItem> {
        if let Item::Folder(f) = self { Some(f) } else { None }
    }
    pub fn as_split(&self) -> Option<&SplitItem> {
        if let Item::Split(s) = self { Some(s) } else { None }
    }
    pub fn as_split_mut(&mut self) -> Option<&mut SplitItem> {
        if let Item::Split(s) = self { Some(s) } else { None }
    }
}

/// Where an item lives in the sidebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Zone {
    Favorites,
    Pinned,
    Today,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReaderState {
    Available,
    On,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TranslationState {
    Working,
    On,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PasswordKey {
    Pending,
    Saved,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct TabInfo {
    pub id: String,
    pub url: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub favicon: Option<String>,
    /// "Home" URL of pinned tabs and favorites. Absent for day tabs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pinned_url: Option<String>,
    pub loading: bool,
    pub can_go_back: bool,
    pub can_go_forward: bool,
    /// True if the tab has a live web view (otherwise it is unloaded).
    pub loaded: bool,
    pub audible: bool,
    pub muted: bool,
    /// Timestamp (ms) of the last activation, used for automatic archiving.
    pub last_active: i64,
    /// Title chosen by the user; takes precedence over `title` when displayed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_title: Option<String>,
    /// Emoji chosen by the user; takes precedence over the favicon when displayed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_icon: Option<String>,
    /// Color of the top of the page (`#rrggbb`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme_color: Option<String>,
    /// Reader mode: `available` if the page looks like an article, `on` if it is shown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reader: Option<ReaderState>,
    /// Language declared by the page when it is not among the preferred languages: translation is offered.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub foreign_lang: Option<String>,
    /// Page translation: in progress, or shown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub translation: Option<TranslationState>,
    /// Key button in the address bar.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password_key: Option<PasswordKey>,
    /// Zoom of the page in percent. Missing = internal page, PDF or unknown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zoom: Option<u32>,
    /// Item of a live folder not yet opened ("new" dot).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub live_unread: Option<bool>,
    /// Developer mode, local page: errors and warnings written to the console since loading.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub console_errors: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub console_warnings: Option<u32>,
    /// Development settings active on the tab.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dev_overrides: Option<DevOverrides>,
    /// Developer tools open, and their location.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub devtools: Option<DevToolsDock>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SplitLayout {
    #[default]
    Columns,
    Rows,
    Grid,
}

/// Header of a day tab group.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TabGroup {
    pub name: String,
    /// Emoji chosen in place of the first tab's icon.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// Hue of the card (0-359).
    #[serde(serialize_with = "super::f64_js")]
    pub hue: f64,
    /// Collapsed: only the header remains.
    pub open: bool,
}

impl Default for TabGroup {
    fn default() -> Self {
        Self { name: String::new(), icon: None, hue: 0.0, open: true }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Rect {
    #[serde(serialize_with = "super::f64_js")]
    pub x: f64,
    #[serde(serialize_with = "super::f64_js")]
    pub y: f64,
    #[serde(serialize_with = "super::f64_js")]
    pub width: f64,
    #[serde(serialize_with = "super::f64_js")]
    pub height: f64,
}

impl Rect {
    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self { x, y, width, height }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Orientation {
    Vertical,
    Horizontal,
}

/// Resizable divider between two panes of a split view (in the gap between the views).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Divider {
    pub split_id: String,
    /// Index of the boundary in `sizes`.
    pub index: usize,
    pub orientation: Orientation,
    pub rect: Rect,
}

/// Media playing in a tab other than the displayed one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct MediaInfo {
    pub tab_id: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artist: Option<String>,
    /// Cover art URL (page metadata), otherwise absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artwork: Option<String>,
    pub site: String,
    pub playing: bool,
    pub muted: bool,
    /// True if the page declares previous / next tracks.
    pub can_skip: bool,
}

/// What an icon is being chosen for (an emoji).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum IconTarget {
    Tab { tab_id: String },
    Folder { folder_id: String },
    Group { group_id: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaneDevtools {
    pub dock: PaneDock,
    pub card: Rect,
    pub rect: Rect,
    pub divider: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PaneDock {
    Right,
    Bottom,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaneResponsive {
    #[serde(flatten)]
    pub responsive: DevResponsive,
    pub bar: Rect,
    pub area: Rect,
    pub scale: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pane {
    pub tab_id: String,
    /// Area of the web view.
    pub rect: Rect,
    /// Whole card: pane toolbar (top) + web view.
    pub card: Rect,
    pub focused: bool,
    /// Devtools docked next to the page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub devtools: Option<PaneDevtools>,
    /// Responsive mode: device bar, area around the page, display scale.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub responsive: Option<PaneResponsive>,
}

/// State of the find-in-page bar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct FindState {
    /// Reset (new id) on each opening, so the UI starts from a clean field.
    pub session: u64,
    pub query: String,
    /// Rank of the active match (1-based), 0 if none.
    pub active: u32,
    pub matches: u32,
    #[serde(serialize_with = "super::f64_js")]
    pub hue: f64,
    /// True during the exit animation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub closing: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PreviewFrom {
    Right,
    Left,
    Below,
}

/// Content of a tab's hover preview.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TabPreviewPayload {
    pub tab_id: String,
    pub title: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub favicon: Option<String>,
    /// Thumbnail (JPEG as a data URL), if the tab was displayed recently.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumb: Option<String>,
    pub audible: bool,
    /// Unloaded (sleeping) tab.
    pub asleep: bool,
    /// True when the card appears (enter animation); false when it moves from one tab to another.
    pub fresh: bool,
    pub from: PreviewFrom,
}

/// What the capture tool takes: an area (web view coordinates), the visible part or the whole page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum CaptureTarget {
    Region { rect: Rect },
    Visible,
    Full,
}

/// Target of a drag and drop in the sidebar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DropTarget {
    pub zone: Zone,
    /// Target space (ignored for favorites).
    pub space_id: String,
    /// Target folder (`pinned` zone only); absent = zone root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder_id: Option<String>,
    /// Target group (`today` zone only): the dropped item joins it if it touches this group.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
    /// Insertion position in the target list (root or the folder's `items`).
    pub index: usize,
}

/// What is being dragged: a tab, a whole split view, a whole folder or a whole group (by its header).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum DragSource {
    Tab { tab_id: String },
    Split { id: String },
    Folder { id: String },
    Group { id: String },
}

/// Edge or center where a tab is dropped on the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DropZone {
    Left,
    Right,
    Top,
    Bottom,
    Center,
}

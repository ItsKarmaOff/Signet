//! Ports `packages/shared/src/types/settings.ts` and `app/desktop/src/shared/types/settings.ts`.

use super::devtools::{DevAlias, DevResponsive, DevToolsPrefs};
use super::passwords::PasswordManager;
use super::spaces::SpaceRoute;
use super::data::ImportKind;
use super::extensions::ExtensionInfo;
use super::mods::Boost;
use crate::i18n::UiLanguage;
use crate::searchengines::{CustomEngine, KeywordOverrides};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum TintStrength {
    Subtle,
    #[default]
    Normal,
    Strong,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Density {
    Compact,
    #[default]
    Normal,
    Roomy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum TextSize {
    Small,
    #[default]
    Normal,
    Large,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum NewTabPosition {
    #[default]
    Top,
    Bottom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum TabLayout {
    #[default]
    Vertical,
    Horizontal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SidebarSide {
    #[default]
    Left,
    Right,
}

/// Secure DNS: `automatic` upgrades the system's DNS provider to DNS over HTTPS when it supports it, `secure` only
/// uses the chosen server, `off` never uses DNS over HTTPS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SecureDnsMode {
    Off,
    #[default]
    Automatic,
    Secure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ReaderFont {
    Sans,
    #[default]
    Serif,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ReaderTheme {
    #[default]
    Auto,
    Light,
    Sepia,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ReaderWidth {
    Narrow,
    #[default]
    Normal,
    Wide,
}

/// Reader mode presentation, set from its bar and remembered for all pages.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReaderPrefs {
    pub font: ReaderFont,
    /// Text size in px (14 to 28).
    pub size: u32,
    pub theme: ReaderTheme,
    pub width: ReaderWidth,
}

pub const DEFAULT_READER: ReaderPrefs = ReaderPrefs { font: ReaderFont::Serif, size: 19, theme: ReaderTheme::Auto, width: ReaderWidth::Normal };

impl Default for ReaderPrefs {
    fn default() -> Self {
        DEFAULT_READER
    }
}

/// Appearance settings applied to all interface views.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Appearance {
    pub tint: TintStrength,
    pub density: Density,
    pub text: TextSize,
    pub glass: bool,
}

/// User settings. Every optional field has the meaning of its "Missing = ..." doc.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Hours of inactivity before a day tab is archived.
    #[serde(serialize_with = "super::f64_js")]
    pub archive_after_hours: f64,
    /// Open in Peek the links that leave the domain of a pinned or favorite tab.
    pub peek_external_links: bool,
    /// Block ads and trackers.
    pub adblock: bool,
    /// Id of the app icon variant (see `APP_ICONS`).
    pub app_icon: String,
    /// Compact mode: sidebar and toolbar hidden, revealed when hovering the edges.
    pub compact_mode: bool,
    /// Automatic picture-in-picture when leaving a tab whose video is playing.
    pub auto_pip: bool,
    /// Routing rules sending new tabs to a space.
    pub routes: Vec<SpaceRoute>,
    /// Preferred languages announced to sites, in order of preference (BCP 47 codes).
    pub languages: Vec<String>,
    /// Interface language; missing = `system`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ui_language: Option<UiLanguage>,
    /// Engine for searches typed in the command bar. Missing = `google`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search_engine: Option<String>,
    /// Engines added by the user (keyword search).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search_engines: Option<Vec<CustomEngine>>,
    /// Changed keywords of built-in engines and presets, and presets turned off.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search_keywords: Option<KeywordOverrides>,
    /// Downloads folder (absolute path). Missing = the system's Downloads folder.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub download_dir: Option<String>,
    /// Ask where to save each download. Missing = no.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ask_download_path: Option<bool>,
    /// First-launch introduction finished or skipped. Missing on an existing state = already seen.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub onboarded: Option<bool>,
    /// Two-finger swipe on a page to go to the previous or next page. Missing = enabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub swipe_navigation: Option<bool>,
    /// Minutes out of view before a tab is put to sleep; 0 = never. Missing = 30.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tab_sleep_minutes: Option<u32>,
    /// Position of new tabs among the day tabs. Missing = top.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_tab_position: Option<NewTabPosition>,
    /// Tab layout: sidebar (`vertical`, default) or a row at the top of the window (`horizontal`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tab_layout: Option<TabLayout>,
    /// Order of extension buttons in the bar (identifiers).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extension_order: Option<Vec<String>>,
    /// Pinned extensions: only these have a button in the toolbar. Missing = none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pinned_extensions: Option<Vec<String>>,
    /// Ask for confirmation before quitting. Missing = enabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confirm_quit: Option<bool>,
    /// Bookmarks bar below the address bar. Missing = shown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub show_bookmarks_bar: Option<bool>,
    /// Theme of the interface and pages. Missing = `system`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme: Option<Theme>,
    /// Amount of the space tint on backgrounds. Missing = `normal`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tint: Option<TintStrength>,
    /// Side of the sidebar (vertical layout). Missing = `left`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sidebar_side: Option<SidebarSide>,
    /// Height of the sidebar's tab rows. Missing = `normal`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub density: Option<Density>,
    /// Interface text size. Missing = `normal`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_size: Option<TextSize>,
    /// Translucent (blur) effect on backgrounds. Missing = enabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transparency: Option<bool>,
    /// Spell checking in page fields. Missing = enabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spellcheck: Option<bool>,
    /// Reader mode presentation. Missing = `DEFAULT_READER`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reader: Option<ReaderPrefs>,
    /// Password manager. Missing = `signet`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password_manager: Option<PasswordManager>,
    /// Offer to save typed passwords. Missing = enabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offer_save_passwords: Option<bool>,
    /// Origins for which saving is never offered.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub never_save_passwords: Option<Vec<String>>,
    /// Save and fill addresses (autofill). Missing = enabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub autofill_addresses: Option<bool>,
    /// Save and fill payment cards (autofill). Missing = enabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub autofill_cards: Option<bool>,
    /// Origins for which saving an address or a card is never offered.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub never_save_autofill: Option<Vec<String>>,
    /// Modified shortcuts: chosen accelerator, or None (null) for none. Missing key = default.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shortcuts: Option<IndexMap<String, Option<String>>>,
    /// Tab preview on hover. Missing = enabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tab_preview: Option<bool>,
    /// Developer mode: port and console error counter on local pages. Missing = off.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dev_mode: Option<bool>,
    /// Developer mode: local pages kept out of history. Missing = recorded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dev_keep_out_of_history: Option<bool>,
    /// Local names: `mysite` typed in the command bar opens its local target.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dev_aliases: Option<Vec<DevAlias>>,
    /// Folders of unpacked extensions, reloaded at each launch.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unpacked_extensions: Option<Vec<String>>,
    /// JSON responses shown as a tree (JSON viewer). Missing = enabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub json_viewer: Option<bool>,
    /// Look for new versions automatically. Missing = enabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub check_updates: Option<bool>,
    /// Last responsive-mode device, restored on next open.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub responsive: Option<DevResponsive>,
    /// Location and sizes of the developer tools.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub devtools: Option<DevToolsPrefs>,
    /// Graphics acceleration (GPU) when available. Missing = enabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hardware_acceleration: Option<bool>,
    /// Secure DNS (DNS over HTTPS). Missing = `automatic`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secure_dns: Option<SecureDnsMode>,
    /// DNS over HTTPS server template, used in `secure` mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secure_dns_server: Option<String>,
    /// HTTPS-only mode. Missing = off.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub https_only: Option<bool>,
    /// Global Privacy Control. Missing = off.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub global_privacy_control: Option<bool>,
    /// Default font size of web pages in px (9, 12, 16, 20 or 24). Missing = 16.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_size: Option<u32>,
    /// Minimum font size of web pages in px (0 = none, then 6 to 24). Missing = 0.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minimum_font_size: Option<u32>,
    /// Default zoom of pages in percent (25 to 500). Missing = 100.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_zoom: Option<u32>,
}

impl Default for Settings {
    /// The values of a fresh install (`initialState().settings`), with default languages left empty.
    fn default() -> Self {
        Settings {
            archive_after_hours: 12.0,
            peek_external_links: true,
            adblock: true,
            app_icon: "aurore".into(),
            compact_mode: false,
            auto_pip: true,
            routes: Vec::new(),
            languages: Vec::new(),
            ui_language: None,
            search_engine: None,
            search_engines: None,
            search_keywords: None,
            download_dir: None,
            ask_download_path: None,
            onboarded: Some(false),
            swipe_navigation: None,
            tab_sleep_minutes: None,
            new_tab_position: None,
            tab_layout: None,
            extension_order: None,
            pinned_extensions: None,
            confirm_quit: None,
            show_bookmarks_bar: None,
            theme: None,
            tint: None,
            sidebar_side: None,
            density: None,
            text_size: None,
            transparency: None,
            spellcheck: None,
            reader: None,
            password_manager: None,
            offer_save_passwords: None,
            never_save_passwords: None,
            autofill_addresses: None,
            autofill_cards: None,
            never_save_autofill: None,
            shortcuts: None,
            tab_preview: None,
            dev_mode: None,
            dev_keep_out_of_history: None,
            dev_aliases: None,
            unpacked_extensions: None,
            json_viewer: None,
            check_updates: None,
            responsive: None,
            devtools: None,
            hardware_acceleration: None,
            secure_dns: None,
            secure_dns_server: None,
            https_only: None,
            global_privacy_control: None,
            font_size: None,
            minimum_font_size: None,
            default_zoom: None,
        }
    }
}

/// App icon variants (`resources/icons/<id>.png`; names come from i18n `appIcon.<id>`).
pub const APP_ICONS: [&str; 8] = ["aurore", "ocean", "corail", "menthe", "nuit", "papier", "graphite", "or"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BrowsingDataKind {
    History,
    Archive,
    Downloads,
    Cache,
    Cookies,
    Permissions,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AppIconInfo {
    pub id: String,
    pub name: String,
    /// 128 px preview as a data URL.
    pub preview: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ImportSourceInfo {
    pub id: String,
    pub title: String,
    /// Present on disk.
    pub available: bool,
    /// Data found.
    pub kinds: Vec<ImportKind>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct DataCounts {
    pub history: u32,
    pub archive: u32,
    pub downloads: u32,
    pub permissions: u32,
}

/// Everything the Settings window shows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct SettingsInfo {
    pub settings: Settings,
    pub version: String,
    /// False in development: some settings (default browser) only work when packaged.
    pub packaged: bool,
    pub default_browser: bool,
    pub app_icons: Vec<AppIconInfo>,
    pub import_sources: Vec<ImportSourceInfo>,
    pub boosts: Vec<Boost>,
    pub extensions: Vec<ExtensionInfo>,
    /// Entry counts, for the "Clear data" labels.
    pub counts: DataCounts,
    #[serde(serialize_with = "super::f64_js")]
    pub hue: f64,
    /// Folder where downloads go.
    pub download_dir: String,
    /// Graphics acceleration of the running engine.
    pub hardware_acceleration_active: bool,
    /// False when no system proxy settings screen is known.
    pub proxy_settings: bool,
}

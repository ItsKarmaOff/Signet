//! Ports `app/desktop/src/shared/types/windows.ts` and `devtools.ts`.

use serde::{Deserialize, Serialize};

/// State of the floating mini-player (custom picture-in-picture).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct PipState {
    pub tab_id: String,
    pub site: String,
    pub title: String,
    pub playing: bool,
    pub muted: bool,
    /// Seconds.
    pub current_time: f64,
    /// Seconds; 0 or infinity for a live stream.
    pub duration: f64,
    pub hue: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PipCommand {
    Toggle,
    Back,
    Forward,
    Seek,
    Mute,
    Return,
    Close,
}

/// Tab of a private window, as its bar shows it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct PrivateTab {
    pub id: String,
    pub url: String,
    pub title: String,
    pub favicon: Option<String>,
    pub loading: bool,
    pub audible: bool,
}

/// State of a private window's bar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct PrivateState {
    pub tabs: Vec<PrivateTab>,
    pub active_id: Option<String>,
    /// Sidebar width, None for tabs at the top.
    pub sidebar_width: Option<f64>,
    /// Address of the displayed tab ('' for a new empty tab).
    pub url: String,
    pub loading: bool,
    pub can_go_back: bool,
    pub can_go_forward: bool,
}

/// State of a Little Signet window's bar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct LittleState {
    pub tab_id: String,
    pub url: String,
    pub title: String,
    pub favicon: Option<String>,
    pub theme_color: Option<String>,
    pub loading: bool,
    pub can_go_back: bool,
    pub can_go_forward: bool,
    pub hue: f64,
}

/// Window of an app space reference.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct WebAppSpace {
    pub name: String,
    pub icon: String,
}

/// Site installed as an app, as stored in `webapps.json` (`WebApp` of `main/features/webapps.ts`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebApp {
    pub id: String,
    pub name: String,
    /// Address opened at launch.
    pub start_url: String,
    /// Prefix of addresses that stay "in the app"; beyond it, the bar shows the page's domain.
    pub scope: String,
    /// Isolated space whose cookies the app shares; `None` = common profile.
    pub space_id: Option<String>,
    /// Site color (manifest or `meta theme-color`), before the page provides its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bounds: Option<super::tabs::Rect>,
    /// Launcher name in the apps folder (without extension).
    pub bundle: String,
    pub installed_at: i64,
}

/// Site installed as an app (Settings > Apps).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct WebAppInfo {
    pub id: String,
    pub name: String,
    pub start_url: String,
    /// 64 px icon (PNG as a data URL).
    pub icon: String,
    /// True if its launcher is in the apps folder.
    pub launcher: bool,
    /// Space whose cookies and logins the app shares; None = common profile.
    pub space: Option<WebAppSpace>,
    /// Number of open windows.
    pub windows: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct WebAppsInfo {
    pub apps: Vec<WebAppInfo>,
    /// Launchers folder, for display.
    pub folder: String,
}

/// State of an app window's bar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct WebAppState {
    pub app_id: String,
    pub name: String,
    pub icon: String,
    pub url: String,
    pub title: String,
    pub theme_color: Option<String>,
    pub loading: bool,
    pub can_go_back: bool,
    /// False when the page has left the app's site: the bar then shows its domain.
    pub in_scope: bool,
    pub hue: f64,
}

/// Progress of a passkey request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PasskeyStatus {
    Waiting,
    Off,
    NoAdapter,
    Denied,
    Error,
    Unsupported,
    Connecting,
    Confirm,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PasskeyType {
    Get,
    Create,
}

/// What the `#passkey` window shows: a passkey request waiting for a phone.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PasskeyPromptState {
    #[serde(rename = "type")]
    pub kind: PasskeyType,
    /// Relying party id: the site the passkey is for.
    pub rp_id: String,
    /// QR code (SVG data URL).
    pub qr: String,
    pub status: PasskeyStatus,
}

/// State of the header of the detached devtools window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct DevToolsWindowState {
    pub tab_id: String,
    /// Host of the inspected page.
    pub site: String,
    /// False if the tab is not in the main window: no docked devtools.
    pub dockable: bool,
}

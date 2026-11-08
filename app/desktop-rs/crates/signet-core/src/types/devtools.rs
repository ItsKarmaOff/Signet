//! Ports `packages/shared/src/types/devtools.ts`.

use super::tabs::Rect;
use serde::{Deserialize, Serialize};

/// Local name (Settings > Development): `mysite`, `mysite.test` or `mysite.localhost`, never a real domain.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct DevAlias {
    pub name: String,
    /// Local address (origin, and optional path), without trailing `/`.
    pub target: String,
}

/// Simulated connection (Chromium presets).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DevNetwork {
    #[serde(rename = "fast4g")]
    Fast4g,
    #[serde(rename = "slow4g")]
    Slow4g,
    #[serde(rename = "3g")]
    ThreeG,
    #[serde(rename = "offline")]
    Offline,
}

/// Simulated user agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DevUserAgent {
    SafariMac,
    SafariIphone,
    SafariIpad,
    ChromeAndroid,
    FirefoxMac,
}

/// Device simulated by responsive mode; `Custom` = hand-picked size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DevDevice {
    #[serde(rename = "iphone-se")]
    IphoneSe,
    #[serde(rename = "iphone-16")]
    Iphone16,
    #[serde(rename = "iphone-16-pro-max")]
    Iphone16ProMax,
    #[serde(rename = "pixel-9")]
    Pixel9,
    #[serde(rename = "galaxy-s24")]
    GalaxyS24,
    #[serde(rename = "ipad-mini")]
    IpadMini,
    #[serde(rename = "ipad-pro-11")]
    IpadPro11,
    #[serde(rename = "laptop")]
    Laptop,
    #[serde(rename = "desktop")]
    Desktop,
    #[serde(rename = "custom")]
    Custom,
}

/// Responsive mode of a tab: device and size of the simulated screen (CSS px, in the displayed orientation).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DevResponsive {
    pub device: DevDevice,
    pub width: u32,
    pub height: u32,
    /// `custom` only: device whose size was left (density, touch and agent inherited).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<DevDevice>,
    /// `custom` only: simulates a mobile device (touch, viewport, agent); missing = same as `base`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mobile: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ColorScheme {
    Light,
    Dark,
}

/// Development settings of a tab (Development menu), while it is open. Missing field = normal behavior.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct DevOverrides {
    /// Cache disabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub no_cache: Option<bool>,
    /// JavaScript disabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub no_js: Option<bool>,
    /// Theme announced to the page (`prefers-color-scheme`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheme: Option<ColorScheme>,
    /// `prefers-reduced-motion: reduce`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reduced_motion: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network: Option<DevNetwork>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<DevUserAgent>,
    /// Responsive mode (the page takes the device size, centered in the panel).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub responsive: Option<DevResponsive>,
}

/// Location of a tab's developer tools: card on the right or bottom of the page, or a separate window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum DevToolsDock {
    #[default]
    Right,
    Bottom,
    Window,
}

/// Last location and sizes of the tools, restored on next open.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct DevToolsPrefs {
    pub dock: DevToolsDock,
    /// Width of the tools on the right (px).
    #[serde(serialize_with = "super::f64_js")]
    pub width: f64,
    /// Height of the tools at the bottom (px).
    #[serde(serialize_with = "super::f64_js")]
    pub height: f64,
    /// Position and size of the detached tools window.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bounds: Option<Rect>,
}

//! Ports `app/desktop/src/shared/types/link.ts`, plus the small parts of `packages/shared/src/link` it uses
//! (`LinkDevice`, `LinkPlatform`, `LinkStatus`) so that core does not depend on `signet-link`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum LinkPlatform {
    #[default]
    Macos,
    Windows,
    Linux,
    Ios,
    Android,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct LinkDevice {
    pub id: String,
    pub name: String,
    pub platform: LinkPlatform,
}

/// Displayed state of a link.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum LinkStatus {
    Connected,
    #[default]
    Connecting,
    PausedHere,
    PausedThere,
    Offline,
    Invalid,
}

/// A paired phone, as Settings and menus see it (never a key or token).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct LinkDeviceInfo {
    pub link_id: String,
    pub device: LinkDevice,
    /// Switch for this device, on this computer.
    pub enabled: bool,
    pub status: LinkStatus,
    pub created_at: i64,
    /// Last control connection (ms since 1970), or None.
    pub last_seen: Option<i64>,
}

/// Pairing QR code shown in Settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct LinkPairing {
    /// SVG image of the QR code, as a data URL.
    pub qr: String,
    /// Expiry time (ms since 1970).
    pub expires_at: i64,
}

/// Everything the UI knows about the link between devices.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct LinkState {
    /// Master switch ("Sync").
    pub general: bool,
    /// Name of this computer, as phones see it.
    pub name: String,
    pub devices: Vec<LinkDeviceInfo>,
    /// Number of phones whose link is active on both sides (toolbar indicator).
    pub active: u32,
    /// Current QR code, or None.
    pub pairing: Option<LinkPairing>,
    /// True if pairing failed because it could not listen on the network.
    pub listen_error: bool,
}

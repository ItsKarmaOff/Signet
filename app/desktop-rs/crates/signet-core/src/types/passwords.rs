//! Ports `packages/shared/src/types/passwords.ts` and `app/desktop/src/shared/types/passwords.ts`.

use serde::{Deserialize, Serialize};

/// Password manager: Signet's password vault, an extension, or none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PasswordManager {
    #[default]
    #[serde(rename = "signet")]
    Signet,
    #[serde(rename = "bitwarden")]
    Bitwarden,
    #[serde(rename = "1password")]
    OnePassword,
    #[serde(rename = "proton")]
    Proton,
    #[serde(rename = "dashlane")]
    Dashlane,
    #[serde(rename = "lastpass")]
    LastPass,
    #[serde(rename = "off")]
    Off,
}

/// A manager provided by a store extension (Chrome Web Store id).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PasswordExtension {
    pub id: PasswordManager,
    pub name: &'static str,
    pub extension_id: &'static str,
}

pub const PASSWORD_EXTENSIONS: [PasswordExtension; 5] = [
    PasswordExtension { id: PasswordManager::Bitwarden, name: "Bitwarden", extension_id: "nngceckbapebfimnlniiiahkandclblb" },
    PasswordExtension { id: PasswordManager::OnePassword, name: "1Password", extension_id: "aeblfdkhhhdcdjpifhhbdiojplfjncoa" },
    PasswordExtension { id: PasswordManager::Proton, name: "Proton Pass", extension_id: "ghmbeldphafepmbegfdlkpapadhbakde" },
    PasswordExtension { id: PasswordManager::Dashlane, name: "Dashlane", extension_id: "fdjamakpfbbddfjaooikfcpapjohcfmg" },
    PasswordExtension { id: PasswordManager::LastPass, name: "LastPass", extension_id: "hdokiejnpimakedhajhdlcegeplioahd" },
];

/// Saved login, as Settings sees it: never the password.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct SavedLogin {
    pub id: String,
    /// Exact origin (`https://example.com`).
    pub origin: String,
    pub username: String,
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PasswordImportError {
    Format,
    Read,
    Encryption,
}

/// Result of a CSV password import: added, already saved, and the cause of a failure.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct PasswordImportResult {
    pub added: u32,
    pub existing: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<PasswordImportError>,
}

/// State of the Settings Passwords section (`passwords:list`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct PasswordsInfo {
    pub logins: Vec<SavedLogin>,
    /// False if the system cannot encrypt (nothing can be saved).
    pub encryption: bool,
    /// True if revealing a password requires Touch ID.
    #[serde(rename = "touchId")]
    pub touch_id: bool,
}

//! Ports `app/desktop/src/shared/types/autofill.ts`.

use serde::{Deserialize, Serialize};

/// Card networks recognised from the number prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum CardNetwork {
    Visa,
    Mastercard,
    Amex,
    Discover,
    Diners,
    Jcb,
    Unionpay,
    Maestro,
    #[default]
    Unknown,
}

/// Saved address, as Settings and the page menu see it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct AutofillAddress {
    pub id: String,
    pub given_name: String,
    pub family_name: String,
    pub organization: String,
    /// Street, one line per row.
    pub street: String,
    pub city: String,
    pub postal_code: String,
    /// Region or state (code or name as typed).
    pub region: String,
    /// ISO 3166-1 alpha-2 code, or empty.
    pub country: String,
    pub phone: String,
    pub email: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub use_count: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<i64>,
}

/// Fields of an address a form or Settings can set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct AddressInput {
    pub given_name: String,
    pub family_name: String,
    pub organization: String,
    pub street: String,
    pub city: String,
    pub postal_code: String,
    pub region: String,
    pub country: String,
    pub phone: String,
    pub email: String,
}

/// Saved card, as Settings sees it: never the number nor the CVC.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct AutofillCard {
    pub id: String,
    pub holder: String,
    /// Last four digits, kept in clear for display.
    pub last4: String,
    pub network: CardNetwork,
    pub exp_month: u32,
    pub exp_year: u32,
    pub nickname: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub use_count: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<i64>,
}

/// Card as entered in Settings; `number` None = unchanged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct CardInput {
    pub holder: String,
    pub number: Option<String>,
    pub exp_month: u32,
    pub exp_year: u32,
    pub nickname: String,
}

/// State of the Settings Autofill section (`autofill:list`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AutofillInfo {
    pub addresses: Vec<AutofillAddress>,
    pub cards: Vec<AutofillCard>,
    /// False if the system cannot encrypt (no card can be saved).
    pub encryption: bool,
}

/// Result of a save from Settings: error message, or None.
pub type AutofillSaveResult = Option<String>;

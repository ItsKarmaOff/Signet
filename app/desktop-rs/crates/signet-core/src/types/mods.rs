//! Ports `app/desktop/src/shared/types/mods.ts`.

use serde::{Deserialize, Serialize};

/// Customization of a site (CSS and hidden elements), applied to all its pages.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Boost {
    /// Registrable domain, as returned by `site_of()` (e.g. `youtube.com`).
    pub site: String,
    pub enabled: bool,
    pub css: String,
    /// CSS selectors of elements hidden with the "Hide" tool.
    pub zapped: Vec<String>,
    pub updated_at: i64,
}

/// State of a Boost's editing window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct BoostEditorState {
    pub site: String,
    /// Existing Boost, or an empty draft if the site has none yet.
    pub boost: Boost,
    /// True while the "Hide" tool is waiting for a click in the page.
    pub zapping: bool,
    /// False if no tab of this site is open any more.
    pub has_tab: bool,
    #[serde(serialize_with = "super::f64_js")]
    pub hue: f64,
}

/// UI mod: a CSS sheet applied to all the app's views (never to web pages).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct InterfaceMod {
    pub id: String,
    pub name: String,
    pub css: String,
    pub enabled: bool,
}

/// State of the Settings Mods section.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ModsInfo {
    pub mods: Vec<InterfaceMod>,
    /// All mods turned off (setting, View menu).
    pub disabled: bool,
    /// Launched in safe mode (`--safe-mode`): no mod is applied during this session.
    #[serde(rename = "safeMode")]
    pub safe_mode: bool,
}

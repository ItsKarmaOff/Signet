//! signet-ui: GPUI theme, icons, assets and reusable widgets. See `README.md`.

pub mod app_state;
pub mod assets;
pub mod color;
pub(crate) mod data_url;
pub mod icon;
pub(crate) mod lang_names;
pub mod motion;
pub mod theme;
pub mod widgets;
pub mod screens;

pub use assets::SignetAssets;
pub use icon::IconName;
pub use theme::{SignetTheme, ThemeExt, ThemeMode, ThemeSettings, watch_system_appearance};

/// Initializes gpui-component and installs the Signet theme. Call once at startup, inside `Application::run`.
pub fn init(cx: &mut gpui::App, settings: ThemeSettings) {
    gpui_component::init(cx);
    SignetTheme::install(settings, cx);
}

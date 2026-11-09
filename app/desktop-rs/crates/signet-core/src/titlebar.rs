//! What a double-click on the title area does, read from the system settings of each platform.
//!
//! Only the pure parsing lives here; reading the settings is the job of the app (`NSUserDefaults` on macOS,
//! `gsettings` on Linux).

/// Action of a double-click on a window's title area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitlebarAction {
    /// Maximize or restore the window.
    Zoom,
    /// Fill the screen (macOS "Fill").
    Fill,
    /// Minimize the window.
    Minimize,
    /// Show the window menu (GNOME "menu").
    Menu,
    /// Do nothing.
    Nothing,
}

/// macOS: `AppleActionOnDoubleClick` (`None`, `Minimize`, `Maximize`, `Fill`) and the legacy
/// `AppleMiniaturizeOnDoubleClick` boolean, used only when the first key is absent.
pub fn mac_action(action: Option<&str>, legacy_minimize: bool) -> TitlebarAction {
    match action {
        Some("None") => TitlebarAction::Nothing,
        Some("Minimize") => TitlebarAction::Minimize,
        Some("Maximize") => TitlebarAction::Zoom,
        Some("Fill") => TitlebarAction::Fill,
        Some(_) => TitlebarAction::Zoom,
        None if legacy_minimize => TitlebarAction::Minimize,
        None => TitlebarAction::Zoom,
    }
}

/// GNOME: output of `gsettings get org.gnome.desktop.wm.preferences action-double-click-titlebar`
/// (for example `'toggle-maximize'\n`).
pub fn gnome_action(raw: Option<&str>) -> TitlebarAction {
    let Some(raw) = raw else { return TitlebarAction::Zoom };
    let value = raw.trim().trim_matches(|c| c == '\'' || c == '"').trim();
    match value {
        "toggle-maximize" | "toggle-maximize-horizontally" | "toggle-maximize-vertically" => TitlebarAction::Zoom,
        "minimize" => TitlebarAction::Minimize,
        "menu" => TitlebarAction::Menu,
        "none" | "lower" | "toggle-shade" => TitlebarAction::Nothing,
        _ => TitlebarAction::Zoom,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use TitlebarAction::*;

    #[test]
    fn mac_values() {
        assert_eq!(mac_action(Some("None"), false), Nothing);
        assert_eq!(mac_action(Some("Minimize"), false), Minimize);
        assert_eq!(mac_action(Some("Maximize"), false), Zoom);
        assert_eq!(mac_action(Some("Fill"), false), Fill);
    }

    #[test]
    fn mac_unknown_is_zoom_and_ignores_legacy() {
        assert_eq!(mac_action(Some("Whatever"), false), Zoom);
        assert_eq!(mac_action(Some(""), true), Zoom);
        assert_eq!(mac_action(Some("Maximize"), true), Zoom);
    }

    #[test]
    fn mac_absent_key() {
        assert_eq!(mac_action(None, false), Zoom);
        assert_eq!(mac_action(None, true), Minimize);
    }

    #[test]
    fn gnome_values() {
        for v in ["toggle-maximize", "toggle-maximize-horizontally", "toggle-maximize-vertically"] {
            assert_eq!(gnome_action(Some(&format!("'{v}'\n"))), Zoom, "{v}");
        }
        assert_eq!(gnome_action(Some("'minimize'\n")), Minimize);
        assert_eq!(gnome_action(Some("'menu'\n")), Menu);
        for v in ["none", "lower", "toggle-shade"] {
            assert_eq!(gnome_action(Some(&format!("'{v}'\n"))), Nothing, "{v}");
        }
    }

    #[test]
    fn gnome_quotes_and_spaces() {
        assert_eq!(gnome_action(Some("  \"minimize\"  ")), Minimize);
        assert_eq!(gnome_action(Some("minimize")), Minimize);
    }

    #[test]
    fn gnome_absent_or_garbage() {
        assert_eq!(gnome_action(None), Zoom);
        assert_eq!(gnome_action(Some("")), Zoom);
        assert_eq!(gnome_action(Some("'banana'")), Zoom);
        assert_eq!(gnome_action(Some("No schema")), Zoom);
    }
}

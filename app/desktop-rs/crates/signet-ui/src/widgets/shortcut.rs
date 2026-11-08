use crate::theme::{ThemeExt, space};
use gpui::{App, IntoElement, ParentElement, RenderOnce, SharedString, Styled, Window, div, prelude::FluentBuilder};

/// Turns an Electron-style accelerator (`CommandOrControl+Shift+T`) into display text.
/// macOS: symbols without separators (`⇧⌘T`). Elsewhere: `Ctrl+Shift+T`.
pub fn format_accelerator(accel: &str, mac: bool) -> String {
    let parts: Vec<&str> = accel.split('+').filter(|p| !p.is_empty()).collect();
    let (key, mods) = match parts.split_last() {
        Some((k, m)) => (*k, m.to_vec()),
        None => return String::new(),
    };
    let key_text = match key {
        "Plus" => "+".to_string(),
        "Space" => if mac { "Space".into() } else { "Space".into() },
        "Escape" | "Esc" => if mac { "⎋".into() } else { "Esc".into() },
        "Return" | "Enter" => if mac { "↩".into() } else { "Enter".into() },
        "Backspace" => if mac { "⌫".into() } else { "Backspace".into() },
        "Delete" => if mac { "⌦".into() } else { "Del".into() },
        "Tab" => if mac { "⇥".into() } else { "Tab".into() },
        "Up" => "↑".into(),
        "Down" => "↓".into(),
        "Left" => "←".into(),
        "Right" => "→".into(),
        k if k.chars().count() == 1 => k.to_uppercase(),
        k => k.to_string(),
    };
    let sym = |m: &str| -> &'static str {
        match (m, mac) {
            ("CommandOrControl" | "CmdOrCtrl", true) | ("Command" | "Cmd" | "Meta" | "Super", true) => "⌘",
            ("CommandOrControl" | "CmdOrCtrl" | "Control" | "Ctrl", false) => "Ctrl",
            ("Control" | "Ctrl", true) => "⌃",
            ("Alt" | "Option", true) => "⌥",
            ("Alt" | "Option", false) => "Alt",
            ("Shift", true) => "⇧",
            ("Shift", false) => "Shift",
            (_, true) => "",
            (_, false) => "Meta",
        }
    };
    if mac {
        // macOS order: control, option, shift, command.
        let order = ["⌃", "⌥", "⇧", "⌘"];
        let have: Vec<&str> = mods.iter().map(|m| sym(m)).collect();
        let mut out: String = order.iter().filter(|o| have.contains(o)).copied().collect();
        out.push_str(&key_text);
        out
    } else {
        let mut v: Vec<String> = mods.iter().map(|m| sym(m).to_string()).collect();
        v.push(key_text);
        v.join("+")
    }
}

/// Keyboard shortcut label (`kbd`): caption size, secondary color.
#[derive(IntoElement)]
pub struct ShortcutLabel {
    text: SharedString,
    boxed: bool,
}

impl ShortcutLabel {
    /// From display text (see [`format_accelerator`]).
    pub fn new(text: impl Into<SharedString>) -> Self {
        Self { text: text.into(), boxed: false }
    }
    pub fn from_accelerator(accel: &str, mac: bool) -> Self {
        Self::new(format_accelerator(accel, mac))
    }
    /// Draws it as a keycap (recorder, settings table).
    pub fn boxed(mut self, boxed: bool) -> Self {
        self.boxed = boxed;
        self
    }
}

impl RenderOnce for ShortcutLabel {
    fn render(self, _w: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.signet();
        div()
            .flex_none()
            .whitespace_nowrap()
            .text_size(t.type_scale.cap)
            .text_color(t.text_2)
            .when(self.boxed, |d| d.px(space::S1).rounded(crate::theme::radius::ICON).bg(t.surface_2))
            .child(self.text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mac_symbols() {
        assert_eq!(format_accelerator("CommandOrControl+Shift+T", true), "⇧⌘T");
        assert_eq!(format_accelerator("Alt+Command+Left", true), "⌥⌘←");
        assert_eq!(format_accelerator("Control+Tab", true), "⌃⇥");
    }

    #[test]
    fn other_platforms() {
        assert_eq!(format_accelerator("CommandOrControl+Shift+T", false), "Ctrl+Shift+T");
        assert_eq!(format_accelerator("Alt+Left", false), "Alt+←");
        assert_eq!(format_accelerator("F5", false), "F5");
        assert_eq!(format_accelerator("", false), "");
    }
}

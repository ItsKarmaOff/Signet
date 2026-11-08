//! Commands of the menu bar and the shortcuts (port of the click handlers of `app/menu.ts`), and the key table the CEF
//! key filter checks while a page has focus.

use std::collections::HashMap;

use signet_core::shortcuts::{parse_accel, shortcut_accels, ShortcutOverrides, SHORTCUTS};
use signet_core::types::PaletteMode;
use signet_web::KeyEvent;

use super::Core;
use crate::app::platform::IS_MAC;

/// Canonical text of a key combination.
pub fn key_string(cmd: bool, ctrl: bool, alt: bool, shift: bool, key: &str) -> String {
    format!("{}{}{}{}{}", if cmd { "cmd+" } else { "" }, if ctrl { "ctrl+" } else { "" }, if alt { "alt+" } else { "" }, if shift { "shift+" } else { "" }, key.to_lowercase())
}

/// Name of the key of a CEF key event, in the vocabulary of accelerators.
pub fn event_key(e: &KeyEvent) -> Option<String> {
    let named = match e.windows_key_code {
        9 => Some("tab"),
        13 => Some("return"),
        32 => Some("space"),
        8 => Some("backspace"),
        46 => Some("delete"),
        37 => Some("left"),
        38 => Some("up"),
        39 => Some("right"),
        40 => Some("down"),
        33 => Some("pageup"),
        34 => Some("pagedown"),
        36 => Some("home"),
        35 => Some("end"),
        122 => Some("f11"),
        _ => None,
    };
    if let Some(n) = named {
        return Some(n.to_string());
    }
    e.character.map(|c| c.to_lowercase().to_string())
}

/// All app shortcuts, as key strings mapped to command ids.
pub fn build_key_table(overrides: Option<&ShortcutOverrides>) -> HashMap<String, String> {
    let mut table = HashMap::new();
    for def in SHORTCUTS {
        for accel in shortcut_accels(def.id, overrides, IS_MAC) {
            if let Some(p) = parse_accel(&accel, IS_MAC) {
                let key = p.key.to_lowercase();
                let keys: Vec<String> = if key == "plus" { vec!["+".into(), "=".into()] } else { vec![key] };
                for k in keys {
                    table.insert(key_string(p.cmd, p.ctrl, p.alt, p.shift, &k), def.id.to_string());
                }
            }
        }
    }
    table.insert(key_string(IS_MAC, !IS_MAC, false, false, ","), "settings".into());
    for n in 1..=9 {
        table.insert(key_string(IS_MAC, !IS_MAC, false, false, &n.to_string()), format!("goToTab:{n}"));
        table.insert(key_string(false, IS_MAC, !IS_MAC, false, &n.to_string()), format!("goToSpace:{n}"));
    }
    // Ctrl+Q elsewhere is the hold-to-quit gauge, fed by the key observer (`quitkeys.rs`).
    if IS_MAC {
        table.insert(key_string(true, false, false, false, "q"), "quit".into());
    }
    table
}

impl Core {
    /// Rebuilds the shared key table after a shortcut change.
    pub fn refresh_key_table(&mut self) {
        let table = build_key_table(self.state().settings.shortcuts.as_ref());
        if let Ok(mut t) = self.key_table.lock() {
            *t = table;
        }
    }

    /// Runs a command by id (menu item, shortcut, palette).
    pub fn run_command(&mut self, id: &str) {
        if self.recording_shortcuts && id != "quit" {
            return;
        }
        if self.aux_command(id) {
            return;
        }
        let active = self.active_tab_id();
        match id {
            "newTab" => self.open_palette(PaletteMode::NewTab, None),
            "newLittle" => self.open_little_window("https://www.google.com"),
            "newPrivate" => self.open_private_window(),
            "newNote" => self.create_note("note"),
            "newEasel" => self.create_note("easel"),
            "reopenTab" => self.reopen_closed(),
            "closeTab" => {
                if let Some(t) = active {
                    self.close_tab(&t);
                }
            }
            "closeWindow" => self.send(crate::windows::Cmd::CloseMain),
            "bookmark" => self.toggle_bookmark(None),
            "pinTab" => {
                if let Some(t) = active {
                    self.pin_toggle(&t);
                }
            }
            "nextTab" => self.cycle_tab(1),
            "prevTab" => self.cycle_tab(-1),
            "openAddress" => self.emit(signet_core::api::UiEvent::EditAddressRequest),
            "back" => self.go_back(None),
            "forward" => self.go_forward(None),
            "reload" => self.reload_or_stop_menu(false),
            "forceReload" => self.reload_tab(None, true),
            "copyUrl" => self.copy_url(None),
            "zoomIn" => self.zoom_active(1),
            "zoomOut" => self.zoom_active(-1),
            "actualSize" => self.zoom_active(0),
            "nextSpace" => self.step_space(1),
            "prevSpace" => self.step_space(-1),
            "toggleSidebar" => self.toggle_sidebar(),
            "compactMode" => self.toggle_compact(),
            "tabLayout" => self.toggle_tab_layout(),
            "bookmarksBar" => self.toggle_bookmarks_bar(),
            "openSplit" => self.open_palette(PaletteMode::Split, None),
            "find" => self.show_find(),
            "findNext" => self.find_step(true),
            "findPrev" => self.find_step(false),
            "print" => {
                if let Some(v) = self.target_view(None) {
                    v.print();
                }
            }
            "viewSource" => {
                if let Some(v) = self.target_view(None) {
                    v.view_source();
                }
            }
            "devTools" | "jsConsole" => self.toggle_dev_tools(active.as_deref(), id == "jsConsole"),
            "mods" => {
                let off = !self.mods_all_disabled();
                self.set_mods_disabled(off);
            }
            "library" => self.open_library_window(None),
            "settings" => self.open_settings_window(None),
            "quit" => self.request_quit(),
            "fullScreen" => self.send(crate::windows::Cmd::ToggleFullscreen),
            "capture" | "reader" | "translate" | "savePage" | "exportPdf" | "installApp" | "customizeSite" | "responsive" => {
                self.phase3_command(id, active.as_deref());
            }
            other => {
                if let Some(n) = other.strip_prefix("goToTab:").and_then(|n| n.parse().ok()) {
                    self.goto_number(n);
                } else if let Some(n) = other.strip_prefix("goToSpace:").and_then(|n| n.parse().ok()) {
                    self.goto_space(n);
                } else if other.starts_with("dev:") {
                    if let Some(tab) = active {
                        self.run_develop_item(&tab, other);
                    }
                } else if let Some(rest) = other.strip_prefix("ext:") {
                    self.ext_run_command(rest);
                } else {
                    self.log(format!("unknown command {other}"));
                }
            }
        }
    }

    fn reload_or_stop_menu(&mut self, _force: bool) {
        self.reload_tab(None, false);
    }

    fn zoom_active(&mut self, delta: i32) {
        if let Some(t) = self.active_tab_id() {
            self.zoom_page(&t, delta);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_table_has_the_main_shortcuts() {
        let t = build_key_table(None);
        let (cmd, ctrl) = (IS_MAC, !IS_MAC);
        assert_eq!(t.get(&key_string(cmd, ctrl, false, false, "t")).map(String::as_str), Some("newTab"));
        assert_eq!(t.get(&key_string(cmd, ctrl, false, false, "w")).map(String::as_str), Some("closeTab"));
        assert_eq!(t.get(&key_string(cmd, ctrl, false, true, "t")).map(String::as_str), Some("reopenTab"));
        assert_eq!(t.get(&key_string(cmd, ctrl, false, false, "3")).map(String::as_str), Some("goToTab:3"));
    }

    #[test]
    fn overrides_replace_defaults() {
        let mut o = ShortcutOverrides::new();
        o.insert("newTab".into(), Some("Ctrl+Alt+Shift+K".into()));
        let t = build_key_table(Some(&o));
        let (cmd, ctrl) = (IS_MAC, !IS_MAC);
        assert!(t.get(&key_string(cmd, ctrl, false, false, "t")).is_none());
        assert_eq!(t.get(&key_string(false, true, true, true, "k")).map(String::as_str), Some("newTab"));
    }

    #[test]
    fn named_keys() {
        let e = KeyEvent { pressed: true, windows_key_code: 9, native_key_code: 0, character: None, cmd: false, ctrl: true, alt: false, shift: false, is_repeat: false };
        assert_eq!(event_key(&e).as_deref(), Some("tab"));
    }
}

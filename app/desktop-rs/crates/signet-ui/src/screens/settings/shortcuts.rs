//! Settings > Shortcuts (port of `ShortcutSettings.tsx`): each command is changed by clicking its shortcut then
//! typing the new one. Conflicts can be taken over (SPEC v19).

use gpui::{
    SharedString, Context, Entity, FocusHandle, Focusable, InteractiveElement, IntoElement, KeyDownEvent, MouseButton, ParentElement, Render, Styled, Window, div,
    prelude::FluentBuilder, px,
};
use serde_json::json;
use signet_core::api::SignetApi;
use signet_core::i18n::{IS_MAC, platform_keys};
use signet_core::shortcuts::{
    Conflict, FIXED_SHORTCUTS, SHORTCUT_GROUPS, SHORTCUTS, ShortcutOverrides, accel_key, accel_label, default_accel, shortcut_accels, shortcut_conflict, shortcut_def,
    take_over, usable_accel,
};
use std::rc::Rc;

use super::kit::{self, Btn};
use super::window::{InfoStore, info_of, watch};
use crate::app_state::{api, tr, tr_args};
use crate::icon::IconName;
use crate::theme::{ThemeExt, radius, space};
use crate::widgets::InlineConfirm;

/// Electron key of a GPUI key name; None for keys that cannot be a shortcut.
pub fn electron_key(key: &str) -> Option<String> {
    let lower = key.to_lowercase();
    let named = match lower.as_str() {
        "space" => Some("Space"),
        "tab" => Some("Tab"),
        "enter" => Some("Return"),
        "up" => Some("Up"),
        "down" => Some("Down"),
        "left" => Some("Left"),
        "right" => Some("Right"),
        "home" => Some("Home"),
        "end" => Some("End"),
        "pageup" => Some("PageUp"),
        "pagedown" => Some("PageDown"),
        "delete" => Some("Delete"),
        "insert" => Some("Insert"),
        _ => None,
    };
    if let Some(n) = named {
        return Some(n.to_string());
    }
    if let Some(n) = lower.strip_prefix('f').and_then(|n| n.parse::<u32>().ok())
        && (1..=24).contains(&n)
    {
        return Some(format!("F{n}"));
    }
    let mut chars = key.chars();
    let c = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    if c.is_ascii_alphanumeric() {
        return Some(c.to_ascii_uppercase().to_string());
    }
    if "-=[]\\;',./`".contains(c) {
        return Some(c.to_string());
    }
    None
}

/// Electron accelerator of a key press (Cmd becomes CmdOrCtrl on Mac, Ctrl becomes CmdOrCtrl elsewhere).
pub fn accel_of(control: bool, alt: bool, shift: bool, platform: bool, key: &str, mac: bool) -> String {
    let mods: Vec<&str> = if mac {
        [(control, "Ctrl"), (alt, "Alt"), (shift, "Shift"), (platform, "CmdOrCtrl")]
    } else {
        [(alt, "Alt"), (shift, "Shift"), (control, "CmdOrCtrl"), (platform, "Super")]
    }
    .into_iter()
    .filter(|(on, _)| *on)
    .map(|(_, name)| name)
    .collect();
    mods.into_iter().chain([key]).collect::<Vec<_>>().join("+")
}

fn without_key(o: &ShortcutOverrides, id: &str) -> ShortcutOverrides {
    o.iter().filter(|(k, _)| k.as_str() != id).map(|(k, v)| (k.clone(), v.clone())).collect()
}

#[derive(Clone)]
enum Notice {
    Conflict { id: String, accel: String, other: String, reset: bool },
    Reserved { id: String, label: String },
    Modifier { id: String },
    Unsupported { id: String },
}

impl Notice {
    fn id(&self) -> &str {
        match self {
            Notice::Conflict { id, .. } | Notice::Reserved { id, .. } | Notice::Modifier { id } | Notice::Unsupported { id } => id,
        }
    }
}

pub struct ShortcutSettings {
    store: Entity<InfoStore>,
    handle: Rc<dyn SignetApi>,
    recording: Option<String>,
    notice: Option<Notice>,
    confirm_reset: bool,
    focus: FocusHandle,
}

impl ShortcutSettings {
    pub fn new(store: Entity<InfoStore>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        watch(&store, cx);
        let focus = cx.focus_handle();
        // Window left or focus lost: input stops (menu shortcuts come back).
        cx.observe_window_activation(window, |this, window, cx| {
            if !window.is_window_active() {
                this.stop(cx);
            }
        })
        .detach();
        cx.on_blur(&focus, window, |this, _, cx| this.stop(cx)).detach();
        Self { store, handle: api(cx), recording: None, notice: None, confirm_reset: false, focus }
    }

    fn overrides(&self, cx: &gpui::App) -> ShortcutOverrides {
        info_of(&self.store, cx).settings.shortcuts.clone().unwrap_or_default()
    }

    fn stop(&mut self, cx: &mut Context<Self>) {
        if self.recording.take().is_some() {
            self.handle.set_shortcut_recording(false);
            cx.notify();
        }
    }

    fn start(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.notice = None;
        self.recording = Some(id.to_string());
        self.handle.set_shortcut_recording(true);
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn save(&mut self, next: ShortcutOverrides, cx: &mut Context<Self>) {
        self.notice = None;
        kit::update(cx, "shortcuts", json!(next));
        cx.notify();
    }

    /// Input: the next key becomes the shortcut (or cancels, or removes the shortcut).
    fn on_key(&mut self, ev: &KeyDownEvent, cx: &mut Context<Self>) {
        let Some(recording) = self.recording.clone() else { return };
        cx.stop_propagation();
        let k = &ev.keystroke;
        if ev.is_held || matches!(k.key.as_str(), "shift" | "control" | "alt" | "platform" | "function" | "capslock") {
            return;
        }
        let m = &k.modifiers;
        let plain = !m.platform && !m.control && !m.alt && !m.shift;
        let overrides = self.overrides(cx);
        if k.key == "escape" && plain {
            return self.stop(cx);
        }
        if (k.key == "backspace" || k.key == "delete") && plain {
            self.stop(cx);
            let mut next = overrides;
            next.insert(recording, None);
            return self.save(next, cx);
        }
        let key = electron_key(&k.key);
        self.stop(cx);
        let Some(key) = key else {
            self.notice = Some(Notice::Unsupported { id: recording });
            return cx.notify();
        };
        let accel = accel_of(m.control, m.alt, m.shift, m.platform, &key, IS_MAC);
        if !usable_accel(&accel, IS_MAC) {
            self.notice = Some(Notice::Modifier { id: recording });
            return cx.notify();
        }
        let current = shortcut_accels(&recording, Some(&overrides), IS_MAC);
        if current.len() == 1 && accel_key(&current[0], IS_MAC) == accel_key(&accel, IS_MAC) {
            return;
        }
        // The original primary re-typed: nothing to change (do not lose the extra shortcuts).
        let main = shortcut_def(&recording).and_then(|d| default_accel(d, IS_MAC));
        if !overrides.contains_key(&recording) && main.is_some_and(|m| accel_key(m, IS_MAC) == accel_key(&accel, IS_MAC)) {
            return;
        }
        match shortcut_conflict(&accel, &recording, Some(&overrides), IS_MAC) {
            Some(Conflict::Reserved(label)) => self.notice = Some(Notice::Reserved { id: recording, label: label.to_string() }),
            Some(Conflict::Command(other)) => self.notice = Some(Notice::Conflict { id: recording, accel, other, reset: false }),
            None => {
                let mut next = overrides;
                next.insert(recording, Some(accel));
                return self.save(next, cx);
            }
        }
        cx.notify();
    }

    fn reset(&mut self, id: &str, cx: &mut Context<Self>) {
        let mut next = self.overrides(cx);
        next.shift_remove(id);
        let original = shortcut_def(id).and_then(|d| default_accel(d, IS_MAC));
        // The original may be taken by another command: same warning as on input.
        if let Some(original) = original
            && let Some(Conflict::Command(other)) = shortcut_conflict(original, id, Some(&next), IS_MAC)
        {
            self.notice = Some(Notice::Conflict { id: id.to_string(), accel: original.to_string(), other, reset: true });
            return cx.notify();
        }
        self.save(next, cx);
    }
}

impl Focusable for ShortcutSettings {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Drop for ShortcutSettings {
    // Leaving the section mid-input: give the shortcuts back.
    fn drop(&mut self) {
        if self.recording.is_some() {
            self.handle.set_shortcut_recording(false);
        }
    }
}

/// `.st-keys` chip.
fn chip(cx: &gpui::App) -> gpui::Div {
    let t = cx.signet();
    div()
        .flex_none()
        .px(space::S2)
        .py(space::HALF)
        .rounded(px(6.0))
        .text_size(t.type_scale.sub)
        .line_height(px(17.0))
        .text_color(t.text)
        .bg(t.ctl)
        .border(px(0.5))
        .border_color(t.ctl_ring)
        .whitespace_nowrap()
}

impl Render for ShortcutSettings {
    fn render(&mut self, _w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.signet().clone();
        let info = info_of(&self.store, cx);
        let overrides = self.overrides(cx);
        let label = |key: &str| tr(key);
        let name_of = |id: &str| label(shortcut_def(id).map(|d| d.label).unwrap_or(id));
        let modified = !overrides.is_empty();
        let ext_with_commands: Vec<_> = info.extensions.iter().filter(|e| e.commands.as_ref().is_some_and(|c| !c.is_empty())).collect();

        let reset_ctl = if self.confirm_reset && modified {
            InlineConfirm::new("sc-reset-confirm", tr_args("settings.shortcuts.confirmReset", &[("count", &overrides.len().to_string())]), tr("settings.shortcuts.resetAll"), tr("common.cancel"))
                .on_cancel(kit::entity_cb(cx, |this, cx| {
                    this.confirm_reset = false;
                    cx.notify();
                }))
                .on_confirm(kit::entity_cb(cx, |this, cx| {
                    this.confirm_reset = false;
                    this.save(ShortcutOverrides::new(), cx);
                }))
                .into_any_element()
        } else {
            Btn::new("sc-reset-all", tr("settings.shortcuts.resetAll"))
                .icon(IconName::RotateCcw)
                .disabled(!modified)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.confirm_reset = true;
                    cx.notify();
                }))
                .into_any_element()
        };

        let mut col = kit::body()
            .id("shortcuts")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _, cx| this.on_key(ev, cx)))
            // Click elsewhere: input stops. The chips stop the propagation of their own clicks.
            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.stop(cx)))
            .child(div().flex().items_center().gap(space::S3).mb(space::S2).child(div().flex_1().min_w_0().child(kit::note(cx, tr("settings.shortcuts.intro")))).child(div().flex_none().child(reset_ctl)));

        for group in SHORTCUT_GROUPS {
            let mut rows: Vec<gpui::AnyElement> = Vec::new();
            for def in SHORTCUTS.iter().filter(|s| s.group == group) {
                let accels = shortcut_accels(def.id, Some(&overrides), IS_MAC);
                let changed = overrides.contains_key(def.id);
                let original = default_accel(def, IS_MAC);
                let recording = self.recording.as_deref() == Some(def.id);
                let note = self.notice.clone().filter(|n| n.id() == def.id);
                let id = def.id.to_string();
                let text = if recording {
                    tr("settings.shortcuts.recording")
                } else if accels.is_empty() {
                    tr("settings.shortcuts.none")
                } else {
                    accels.iter().map(|a| accel_label(a, IS_MAC)).collect::<Vec<_>>().join(" \u{b7} ")
                };
                let mut keys = chip(cx)
                    .id(SharedString::from(format!("sc-chip-{}", def.id)))
                    .min_w(px(64.0))
                    .text_center()
                    .cursor_pointer()
                    .when(changed, |d| d.text_color(t.accent))
                    .when(accels.is_empty() && !recording, |d| d.text_color(t.text_3))
                    .when(recording, |d| d.text_color(t.accent).border(px(1.5)).border_color(t.accent))
                    .hover(|s| s.border_color(t.accent.opacity(0.5)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener({
                            let id = id.clone();
                            move |this, _, window, cx| {
                                cx.stop_propagation();
                                if this.recording.as_deref() == Some(id.as_str()) {
                                    this.stop(cx)
                                } else {
                                    this.start(&id, window, cx)
                                }
                            }
                        }),
                    )
                    .child(text);
                let _ = &mut keys;
                let mut row = kit::row(cx).child(kit::label(cx, label(def.label), None));
                if changed {
                    let tip = match original {
                        Some(o) => tr_args("settings.shortcuts.reset", &[("keys", &accel_label(o, IS_MAC))]),
                        None => tr("settings.shortcuts.resetNone"),
                    };
                    let rid = id.clone();
                    row = row.child(kit::icon_btn(SharedString::from(format!("sc-reset-{}", def.id)), IconName::RotateCcw, tip, cx.listener(move |this, _, _, cx| this.reset(&rid, cx))).small());
                }
                row = row.child(keys);
                let mut block = div().flex().flex_col().child(row);
                if recording {
                    block = block.child(div().px(space::S4).pb(space::S2).child(kit::hint_text(cx, tr("settings.shortcuts.recordingHint"))));
                }
                if let Some(n) = note {
                    let line = match n {
                        Notice::Conflict { accel, other, reset, .. } => {
                            let other_name = name_of(&other);
                            let (def_id, owner) = (id.clone(), other.clone());
                            div()
                                .flex()
                                .items_center()
                                .gap(space::S3)
                                .child(
                                    div()
                                        .flex_1()
                                        .child(kit::err(cx, tr_args("settings.shortcuts.conflict", &[("name", &other_name)])))
                                        .child(kit::hint_text(cx, tr_args("settings.shortcuts.takeOverHint", &[("name", &other_name)]))),
                                )
                                .child(Btn::new(SharedString::from(format!("sc-cancel-{}", def.id)), tr("common.cancel")).on_click(cx.listener(|this, _, _, cx| {
                                    this.notice = None;
                                    cx.notify();
                                })))
                                .child(Btn::new(SharedString::from(format!("sc-take-{}", def.id)), tr("settings.shortcuts.takeOver")).accent().on_click(cx.listener(move |this, _, _, cx| {
                                    let current = this.overrides(cx);
                                    let next = take_over(&current, &def_id, &accel, &owner, IS_MAC);
                                    this.save(if reset { without_key(&next, &def_id) } else { next }, cx);
                                })))
                                .into_any_element()
                        }
                        Notice::Reserved { label: l, .. } => kit::err(cx, tr_args("settings.shortcuts.reservedBy", &[("name", &tr(&l))])).into_any_element(),
                        Notice::Modifier { .. } => {
                            kit::err(cx, tr(if IS_MAC { "settings.shortcuts.needModifier" } else { "settings.shortcuts.needModifierOther" })).into_any_element()
                        }
                        Notice::Unsupported { .. } => kit::err(cx, tr("settings.shortcuts.unsupported")).into_any_element(),
                    };
                    block = block.child(div().px(space::S4).pb(space::S2).child(line));
                }
                rows.push(block.into_any_element());
            }
            for f in FIXED_SHORTCUTS.iter().filter(|f| f.group == group) {
                let text = match (IS_MAC, f.other) {
                    (false, Some(o)) => o.to_string(),
                    _ => platform_keys(f.keys, IS_MAC),
                };
                rows.push(
                    kit::row(cx)
                        .child(kit::label(cx, label(f.label), None))
                        .child(chip(cx).opacity(0.6).child(text))
                        .into_any_element(),
                );
            }
            col = col.child(
                kit::section_col()
                    .child(kit::sub(cx, tr(&format!("settings.shortcuts.group.{}", group.id()))))
                    .child(kit::group(cx, rows)),
            );
        }

        if !ext_with_commands.is_empty() {
            let rows = ext_with_commands
                .iter()
                .flat_map(|ext| {
                    ext.commands.iter().flatten().map(move |c| {
                        let title = if !c.description.is_empty() {
                            c.description.clone()
                        } else if c.name.starts_with("_execute_") {
                            tr("settings.shortcuts.activateExtension")
                        } else {
                            c.name.clone()
                        };
                        let (shortcut, none) = if c.shortcut.is_empty() { (tr("settings.shortcuts.notSet"), true) } else { (c.shortcut.clone(), false) };
                        (title, ext.name.clone(), shortcut, none)
                    })
                })
                .map(|(title, ext_name, shortcut, none)| {
                    kit::row(cx)
                        .child(kit::label(cx, title, Some(ext_name.into())))
                        .child(chip(cx).opacity(0.6).when(none, |d| d.text_color(t.text_3)).child(shortcut))
                        .into_any_element()
                })
                .collect();
            col = col.child(
                kit::section_col()
                    .child(kit::sub(cx, tr("settings.shortcuts.extensions")))
                    .child(kit::note(cx, tr("settings.shortcuts.extensionsHint")))
                    .child(kit::group(cx, rows)),
            );
        }
        let _ = radius::ROW;
        col
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use signet_core::shortcuts::clean_overrides;

    #[test]
    fn keys_map_to_electron_names() {
        assert_eq!(electron_key("a").as_deref(), Some("A"));
        assert_eq!(electron_key("7").as_deref(), Some("7"));
        assert_eq!(electron_key(",").as_deref(), Some(","));
        assert_eq!(electron_key("enter").as_deref(), Some("Return"));
        assert_eq!(electron_key("pagedown").as_deref(), Some("PageDown"));
        assert_eq!(electron_key("f12").as_deref(), Some("F12"));
        assert_eq!(electron_key("f25"), None);
        assert_eq!(electron_key("escape"), None);
        assert_eq!(electron_key("\u{e9}"), None);
    }

    #[test]
    fn accelerators_follow_the_platform() {
        assert_eq!(accel_of(false, false, true, true, "T", true), "Shift+CmdOrCtrl+T");
        assert_eq!(accel_of(true, true, false, false, "L", true), "Ctrl+Alt+L");
        assert_eq!(accel_of(true, false, true, false, "T", false), "Shift+CmdOrCtrl+T");
        assert_eq!(accel_of(false, false, false, true, "T", false), "Super+T");
    }

    #[test]
    fn typed_accelerators_need_a_modifier() {
        let accel = accel_of(false, false, true, false, "T", true);
        assert!(!usable_accel(&accel, true));
        assert!(usable_accel(&accel_of(false, false, false, true, "T", true), true));
        assert!(usable_accel("F5", true));
    }

    #[test]
    fn take_over_moves_the_shortcut() {
        let mut o = ShortcutOverrides::new();
        o.insert("newTab".into(), Some("CmdOrCtrl+J".into()));
        let next = take_over(&o, "closeTab", "CmdOrCtrl+J", "newTab", true);
        assert_eq!(next.get("closeTab"), Some(&Some("CmdOrCtrl+J".to_string())));
        assert_eq!(next.get("newTab"), Some(&None));
        assert!(clean_overrides(&next, true).is_some());
    }

    #[test]
    fn removing_a_key_keeps_the_rest() {
        let mut o = ShortcutOverrides::new();
        o.insert("a".into(), None);
        o.insert("b".into(), Some("X".into()));
        let out = without_key(&o, "a");
        assert_eq!(out.len(), 1);
        assert!(out.contains_key("b"));
    }
}

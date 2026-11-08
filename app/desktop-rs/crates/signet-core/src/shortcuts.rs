//! Ports `app/desktop/src/shared/shortcuts.ts`: single list of commands, default bindings per platform and the
//! customization rules. Accelerators are written the Electron way (`CmdOrCtrl+Shift+T`). Settings
//! (`Settings::shortcuts`) only keep the overrides: `Some(accel)` = chosen shortcut, `None` = no shortcut, missing
//! key = default shortcut.

use indexmap::IndexMap;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::OnceLock;

pub const IS_WIN: bool = cfg!(target_os = "windows");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShortcutGroup {
    Tabs,
    Navigation,
    Spaces,
    Sidebar,
    Split,
    Search,
    Page,
    Windows,
}

impl ShortcutGroup {
    pub fn id(self) -> &'static str {
        match self {
            ShortcutGroup::Tabs => "tabs",
            ShortcutGroup::Navigation => "navigation",
            ShortcutGroup::Spaces => "spaces",
            ShortcutGroup::Sidebar => "sidebar",
            ShortcutGroup::Split => "split",
            ShortcutGroup::Search => "search",
            ShortcutGroup::Page => "page",
            ShortcutGroup::Windows => "windows",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ShortcutDef {
    pub id: &'static str,
    pub group: ShortcutGroup,
    /// Translation key of the label (`ui` namespace).
    pub label: &'static str,
    /// Default shortcut (None: none, to be chosen). `mac` / `other`: when it differs by system.
    pub accel: Option<&'static str>,
    pub mac: Option<&'static str>,
    pub other: Option<&'static str>,
    /// Default extra shortcuts (hidden menu entries), lost as soon as another shortcut is chosen.
    pub alt: &'static [&'static str],
    /// Extra shortcuts on Mac only.
    pub alt_mac: &'static [&'static str],
}

const fn def(id: &'static str, group: ShortcutGroup, label: &'static str, accel: Option<&'static str>) -> ShortcutDef {
    ShortcutDef { id, group, label, accel, mac: None, other: None, alt: &[], alt_mac: &[] }
}

use ShortcutGroup::*;

/// Editable commands, in Settings order.
pub static SHORTCUTS: &[ShortcutDef] = &[
    def("newTab", Tabs, "settings.shortcuts.item.newTab", Some("CmdOrCtrl+T")),
    ShortcutDef { mac: Some("Ctrl+Cmd+N"), other: Some("Ctrl+Alt+Shift+N"), ..def("newNote", Tabs, "settings.shortcuts.item.newNote", None) },
    def("newEasel", Tabs, "settings.shortcuts.item.newEasel", None),
    def("reopenTab", Tabs, "settings.shortcuts.item.reopenTab", Some("Shift+CmdOrCtrl+T")),
    def("closeTab", Tabs, "settings.shortcuts.item.closeTab", Some("CmdOrCtrl+W")),
    def("closeWindow", Tabs, "settings.shortcuts.item.closeWindow", Some("Shift+CmdOrCtrl+W")),
    def("bookmark", Tabs, "settings.shortcuts.item.bookmark", Some("CmdOrCtrl+D")),
    def("pinTab", Tabs, "settings.shortcuts.item.pinTab", Some("Alt+CmdOrCtrl+P")),
    ShortcutDef { alt: &["Shift+CmdOrCtrl+]"], ..def("nextTab", Tabs, "settings.shortcuts.item.nextTab", Some("Ctrl+Tab")) },
    ShortcutDef { alt: &["Shift+CmdOrCtrl+["], ..def("prevTab", Tabs, "settings.shortcuts.item.prevTab", Some("Ctrl+Shift+Tab")) },
    def("openAddress", Navigation, "settings.shortcuts.item.openAddress", Some("CmdOrCtrl+L")),
    def("back", Navigation, "settings.shortcuts.item.back", Some("CmdOrCtrl+[")),
    def("forward", Navigation, "settings.shortcuts.item.forward", Some("CmdOrCtrl+]")),
    def("reload", Navigation, "settings.shortcuts.item.reload", Some("CmdOrCtrl+R")),
    def("forceReload", Navigation, "settings.shortcuts.item.forceReload", Some("Shift+CmdOrCtrl+R")),
    def("copyUrl", Navigation, "settings.shortcuts.item.copyUrl", Some("Shift+CmdOrCtrl+C")),
    ShortcutDef { alt: &["CmdOrCtrl+="], ..def("zoomIn", Navigation, "settings.shortcuts.item.zoomIn", Some("CmdOrCtrl+Plus")) },
    def("zoomOut", Navigation, "settings.shortcuts.item.zoomOut", Some("CmdOrCtrl+-")),
    def("actualSize", Navigation, "settings.shortcuts.item.actualSize", Some("CmdOrCtrl+0")),
    def("nextSpace", Spaces, "settings.shortcuts.item.nextSpace", Some("Alt+CmdOrCtrl+Right")),
    def("prevSpace", Spaces, "settings.shortcuts.item.prevSpace", Some("Alt+CmdOrCtrl+Left")),
    def("toggleSidebar", Sidebar, "settings.shortcuts.item.toggleSidebar", Some("CmdOrCtrl+S")),
    def("compactMode", Sidebar, "settings.shortcuts.item.compactMode", Some("Alt+CmdOrCtrl+S")),
    // Cmd+Shift+S as well on Mac; elsewhere, Ctrl+Shift+S is already the main shortcut.
    ShortcutDef { alt_mac: &["Cmd+Shift+S"], ..def("tabLayout", Sidebar, "settings.shortcuts.item.tabLayout", Some("Ctrl+Shift+S")) },
    def("bookmarksBar", Sidebar, "settings.shortcuts.item.bookmarksBar", Some("Shift+CmdOrCtrl+B")),
    // Ctrl+Shift+= on Mac; elsewhere Ctrl+Shift+= is zoom in (Ctrl+Plus), hence Alt+Shift+=.
    ShortcutDef { mac: Some("Ctrl+Shift+="), other: Some("Alt+Shift+="), ..def("openSplit", Split, "settings.shortcuts.item.openSplit", None) },
    def("find", Search, "settings.shortcuts.item.find", Some("CmdOrCtrl+F")),
    def("findNext", Search, "settings.shortcuts.item.findNext", Some("CmdOrCtrl+G")),
    def("findPrev", Search, "settings.shortcuts.item.findPrev", Some("Shift+CmdOrCtrl+G")),
    def("print", Page, "settings.shortcuts.item.print", Some("CmdOrCtrl+P")),
    def("capture", Page, "settings.shortcuts.item.capture", Some("Shift+CmdOrCtrl+2")),
    def("reader", Page, "settings.shortcuts.item.reader", Some("Alt+CmdOrCtrl+R")),
    def("translate", Page, "settings.shortcuts.item.translate", None),
    def("savePage", Page, "settings.shortcuts.item.savePage", None),
    def("exportPdf", Page, "settings.shortcuts.item.exportPdf", None),
    def("viewSource", Page, "settings.shortcuts.item.viewSource", Some("Alt+CmdOrCtrl+U")),
    def("newLittle", Windows, "settings.shortcuts.item.newLittle", Some("Alt+CmdOrCtrl+N")),
    def("newPrivate", Windows, "settings.shortcuts.item.newPrivate", Some("Shift+CmdOrCtrl+N")),
    def("installApp", Windows, "settings.shortcuts.item.installApp", None),
    def("customizeSite", Windows, "settings.shortcuts.item.customizeSite", Some("Alt+CmdOrCtrl+B")),
    def("devTools", Windows, "settings.shortcuts.item.devTools", Some("Alt+CmdOrCtrl+I")),
    def("jsConsole", Windows, "settings.shortcuts.item.jsConsole", Some("Alt+CmdOrCtrl+J")),
    def("responsive", Windows, "settings.shortcuts.item.responsive", Some("Shift+CmdOrCtrl+M")),
    ShortcutDef { mac: Some("Cmd+Y"), other: Some("Ctrl+H"), ..def("library", Windows, "settings.shortcuts.item.library", None) },
];

pub static SHORTCUT_GROUPS: [ShortcutGroup; 8] = [Tabs, Navigation, Spaces, Sidebar, Split, Search, Page, Windows];

#[derive(Debug, Clone, Copy)]
pub struct FixedShortcut {
    pub group: ShortcutGroup,
    pub label: &'static str,
    /// Mac-style text.
    pub keys: &'static str,
    pub other: Option<&'static str>,
}

/// Non-editable shortcuts, shown read-only in their group (Mac-style text; `other` elsewhere).
pub static FIXED_SHORTCUTS: [FixedShortcut; 5] = [
    FixedShortcut { group: Tabs, label: "settings.shortcuts.item.goToTab", keys: "\u{2318}1 \u{2026} \u{2318}9", other: None },
    FixedShortcut { group: Spaces, label: "settings.shortcuts.item.goToSpace", keys: "\u{2303}1 \u{2026} \u{2303}9", other: Some("Alt+1 \u{2026} Alt+9") },
    FixedShortcut { group: Windows, label: "settings.shortcuts.item.settings", keys: "\u{2318},", other: None },
    FixedShortcut { group: Windows, label: "settings.shortcuts.item.fullScreen", keys: "\u{2303}\u{2318}F", other: Some("F11") },
    FixedShortcut { group: Windows, label: "settings.shortcuts.item.quit", keys: "\u{2318}Q", other: None },
];

#[derive(Debug, Clone, PartialEq)]
pub struct Reserved {
    pub accel: String,
    pub label: &'static str,
}

/// Reserved shortcuts (Edit menu, Quit, Settings, Cmd+1..9, Ctrl+1..9, system): refused for a command, with the name of
/// their owner. Electron accelerators, per system.
pub fn reserved_shortcuts(mac: bool, win: bool) -> Vec<Reserved> {
    let r = |a: &str, l: &'static str| Reserved { accel: a.to_string(), label: l };
    let mut list = vec![
        r("CmdOrCtrl+Q", "settings.shortcuts.item.quit"),
        r("CmdOrCtrl+,", "settings.shortcuts.item.settings"),
        r("CmdOrCtrl+Z", "settings.shortcuts.reserved.undo"),
        r(if win { "Ctrl+Y" } else { "Shift+CmdOrCtrl+Z" }, "settings.shortcuts.reserved.redo"),
        r("CmdOrCtrl+X", "settings.shortcuts.reserved.cut"),
        r("CmdOrCtrl+C", "settings.shortcuts.reserved.copy"),
        r("CmdOrCtrl+V", "settings.shortcuts.reserved.paste"),
        r("Alt+Shift+CmdOrCtrl+V", "settings.shortcuts.reserved.pasteMatchStyle"),
        r("CmdOrCtrl+A", "settings.shortcuts.reserved.selectAll"),
        r(if mac { "Ctrl+Cmd+F" } else { "F11" }, "settings.shortcuts.item.fullScreen"),
    ];
    if mac {
        list.push(r("Cmd+H", "settings.shortcuts.reserved.hide"));
        list.push(r("Alt+Cmd+H", "settings.shortcuts.reserved.hideOthers"));
        list.push(r("Cmd+M", "settings.shortcuts.reserved.minimize"));
    }
    for i in 1..=9 {
        list.push(r(&format!("CmdOrCtrl+{i}"), "settings.shortcuts.item.goToTab"));
        list.push(r(&if mac { format!("Ctrl+{i}") } else { format!("Alt+{i}") }, "settings.shortcuts.item.goToSpace"));
    }
    list
}

/// Overrides: chosen accelerator, or None for "no shortcut". Missing key = default.
pub type ShortcutOverrides = IndexMap<String, Option<String>>;

pub fn shortcut_def(id: &str) -> Option<&'static ShortcutDef> {
    static BY_ID: OnceLock<HashMap<&'static str, &'static ShortcutDef>> = OnceLock::new();
    BY_ID.get_or_init(|| SHORTCUTS.iter().map(|s| (s.id, s)).collect()).get(id).copied()
}

/// Default shortcut of a command on this system.
pub fn default_accel(def: &ShortcutDef, mac: bool) -> Option<&'static str> {
    (if mac { def.mac } else { def.other }).or(def.accel)
}

/// Active shortcuts of a command: the main one first, then the default extras. Empty if none.
pub fn shortcut_accels(id: &str, overrides: Option<&ShortcutOverrides>, mac: bool) -> Vec<String> {
    let Some(def) = shortcut_def(id) else { return Vec::new() };
    if let Some(o) = overrides
        && let Some(custom) = o.get(id)
    {
        return custom.iter().cloned().collect();
    }
    match default_accel(def, mac) {
        Some(main) => {
            let mut v = vec![main.to_string()];
            v.extend(def.alt.iter().map(|s| s.to_string()));
            if mac {
                v.extend(def.alt_mac.iter().map(|s| s.to_string()));
            }
            v
        }
        None => Vec::new(),
    }
}

/// Main shortcut of a command (for the menu and the command bar), or None.
pub fn shortcut_accel(id: &str, overrides: Option<&ShortcutOverrides>, mac: bool) -> Option<String> {
    shortcut_accels(id, overrides, mac).into_iter().next()
}

// ----------------------- Reading and writing accelerators -----------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Modifier {
    Cmd,
    Ctrl,
    Alt,
    Shift,
    CmdOrCtrl,
}

fn modifier_alias(s: &str) -> Option<Modifier> {
    Some(match s.to_lowercase().as_str() {
        "command" | "cmd" | "super" | "meta" => Modifier::Cmd,
        "control" | "ctrl" => Modifier::Ctrl,
        "alt" | "option" => Modifier::Alt,
        "shift" => Modifier::Shift,
        "commandorcontrol" | "cmdorctrl" => Modifier::CmdOrCtrl,
        _ => return None,
    })
}

const NAMED_KEYS: [&str; 17] =
    ["Plus", "Space", "Tab", "Backspace", "Delete", "Insert", "Return", "Enter", "Up", "Down", "Left", "Right", "Home", "End", "PageUp", "PageDown", "Escape"];

/// Valid key for Electron, written canonically (`a` becomes `A`), or None.
fn canonical_key(key: &str) -> Option<String> {
    let mut chars = key.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        if c.is_ascii_alphanumeric() {
            return Some(c.to_ascii_uppercase().to_string());
        }
        if "-=[]\\;',./`".contains(c) {
            return Some(c.to_string());
        }
    }
    let up = key.to_uppercase();
    if let Some(n) = up.strip_prefix('F').and_then(|d| d.parse::<u32>().ok())
        && (1..=24).contains(&n)
        && up[1..].bytes().all(|b| b.is_ascii_digit())
    {
        return Some(up);
    }
    if key.eq_ignore_ascii_case("esc") {
        return Some("Escape".into());
    }
    NAMED_KEYS.iter().find(|k| k.eq_ignore_ascii_case(key)).map(|k| if *k == "Enter" { "Return".to_string() } else { k.to_string() })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedAccel {
    pub cmd: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub key: String,
}

/// Reads an Electron accelerator (CmdOrCtrl resolved for the system); None if invalid.
pub fn parse_accel(accel: &str, mac: bool) -> Option<ParsedAccel> {
    if accel.chars().count() > 60 {
        return None;
    }
    // "Plus" is written in full: "+" is only a separator.
    let mut parts: Vec<&str> = accel.split('+').collect();
    if parts.iter().any(|p| p.is_empty()) {
        return None;
    }
    let key = canonical_key(parts.pop()?)?;
    let mut out = ParsedAccel { cmd: false, ctrl: false, alt: false, shift: false, key };
    for p in parts {
        match modifier_alias(p)? {
            Modifier::CmdOrCtrl => {
                if mac {
                    out.cmd = true
                } else {
                    out.ctrl = true
                }
            }
            Modifier::Cmd => out.cmd = true,
            Modifier::Ctrl => out.ctrl = true,
            Modifier::Alt => out.alt = true,
            Modifier::Shift => out.shift = true,
        }
    }
    Some(out)
}

/// Comparable form of an accelerator on this system (`shift+cmd+T`), or None if invalid.
pub fn accel_key(accel: &str, mac: bool) -> Option<String> {
    let p = parse_accel(accel, mac)?;
    // US layout (Electron's): Plus = Shift+=.
    let (mut shift, mut key) = (p.shift, p.key.clone());
    if key == "=" && shift {
        key = "Plus".into();
    }
    if key == "Plus" {
        shift = false;
    }
    let mut v: Vec<&str> = Vec::new();
    if p.ctrl {
        v.push("ctrl");
    }
    if p.alt {
        v.push("alt");
    }
    if shift {
        v.push("shift");
    }
    if p.cmd {
        v.push("cmd");
    }
    v.push(&key);
    Some(v.join("+"))
}

/// True if the accelerator can serve as a shortcut: at least Cmd, Ctrl or Alt (Shift alone is not enough), except F keys.
pub fn usable_accel(accel: &str, mac: bool) -> bool {
    let Some(p) = parse_accel(accel, mac) else { return false };
    p.cmd || p.ctrl || p.alt || (p.key.starts_with('F') && p.key[1..].bytes().all(|b| b.is_ascii_digit()) && p.key.len() > 1)
}

fn mac_glyph(key: &str) -> Option<&'static str> {
    Some(match key {
        "Plus" => "+",
        "Space" => "\u{2423}",
        "Tab" => "\u{21e5}",
        "Backspace" => "\u{232b}",
        "Delete" => "\u{2326}",
        "Return" => "\u{21a9}",
        "Up" => "\u{2191}",
        "Down" => "\u{2193}",
        "Left" => "\u{2190}",
        "Right" => "\u{2192}",
        "Home" => "\u{2196}",
        "End" => "\u{2198}",
        "PageUp" => "\u{21de}",
        "PageDown" => "\u{21df}",
        "Escape" => "\u{238b}",
        _ => return None,
    })
}

fn other_name(key: &str) -> Option<&'static str> {
    Some(match key {
        "Plus" => "+",
        "Up" => "\u{2191}",
        "Down" => "\u{2193}",
        "Left" => "\u{2190}",
        "Right" => "\u{2192}",
        "Return" => "Enter",
        "Escape" => "Esc",
        _ => return None,
    })
}

/// Text of an accelerator: symbols on Mac, `Ctrl+Alt+Shift+T` elsewhere.
pub fn accel_label(accel: &str, mac: bool) -> String {
    let Some(p) = parse_accel(accel, mac) else { return accel.to_string() };
    if mac {
        return format!(
            "{}{}{}{}{}",
            if p.ctrl { "\u{2303}" } else { "" },
            if p.alt { "\u{2325}" } else { "" },
            if p.shift { "\u{21e7}" } else { "" },
            if p.cmd { "\u{2318}" } else { "" },
            mac_glyph(&p.key).unwrap_or(&p.key)
        );
    }
    let mut v: Vec<&str> = Vec::new();
    if p.ctrl {
        v.push("Ctrl");
    }
    if p.alt {
        v.push("Alt");
    }
    if p.shift {
        v.push("Shift");
    }
    if p.cmd {
        v.push("Win");
    }
    v.push(other_name(&p.key).unwrap_or(&p.key));
    v.join("+")
}

/// Text of a command's current shortcut (command bar, tooltips), or None if it has none.
pub fn shortcut_label(id: &str, overrides: Option<&ShortcutOverrides>, mac: bool) -> Option<String> {
    shortcut_accel(id, overrides, mac).map(|a| accel_label(&a, mac))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Conflict {
    /// Another command.
    Command(String),
    /// A reserved shortcut (label key of its owner).
    Reserved(&'static str),
}

/// Who already uses this shortcut? Another command, a reserved shortcut, or None. `except`: the command being edited.
pub fn shortcut_conflict(accel: &str, except: &str, overrides: Option<&ShortcutOverrides>, mac: bool) -> Option<Conflict> {
    let key = accel_key(accel, mac)?;
    for r in reserved_shortcuts(mac, IS_WIN) {
        if accel_key(&r.accel, mac).as_deref() == Some(key.as_str()) {
            return Some(Conflict::Reserved(r.label));
        }
    }
    for d in SHORTCUTS {
        if d.id == except {
            continue;
        }
        if shortcut_accels(d.id, overrides, mac).iter().any(|a| accel_key(a, mac).as_deref() == Some(key.as_str())) {
            return Some(Conflict::Command(d.id.to_string()));
        }
    }
    None
}

fn has_secondary(def: &ShortcutDef, mac: bool) -> bool {
    !def.alt.is_empty() || (mac && !def.alt_mac.is_empty())
}

/// Overrides after giving `accel` to `id` and removing that shortcut from `other`. If `other` only held it as an
/// extra shortcut, it keeps its main one (override equal to the default = "main only").
pub fn take_over(overrides: &ShortcutOverrides, id: &str, accel: &str, other: &str, mac: bool) -> ShortcutOverrides {
    let key = accel_key(accel, mac);
    let accels = shortcut_accels(other, Some(overrides), mac);
    let main = accels.first();
    let only_secondary = main.is_some_and(|m| accel_key(m, mac) != key);
    let mut out = overrides.clone();
    out.insert(id.to_string(), Some(accel.to_string()));
    out.insert(other.to_string(), if only_secondary { main.cloned() } else { None });
    out
}

/// Cleans overrides coming from the UI: known commands, valid and usable shortcuts, no conflict among themselves or
/// with a reserved shortcut; an override equal to the default shortcut is removed. None if the set is inconsistent.
pub fn clean_overrides(value: &ShortcutOverrides, mac: bool) -> Option<ShortcutOverrides> {
    let mut out = ShortcutOverrides::new();
    for (id, accel) in value {
        let Some(def) = shortcut_def(id) else { continue };
        let Some(accel) = accel else {
            if default_accel(def, mac).is_some() {
                out.insert(id.clone(), None);
            }
            continue;
        };
        if !usable_accel(accel, mac) {
            return None;
        }
        let main = default_accel(def, mac);
        // Equal to the default: removed, unless it has extra shortcuts ("main only", see take_over).
        if main.is_some_and(|m| accel_key(m, mac) == accel_key(accel, mac)) && !has_secondary(def, mac) {
            continue;
        }
        out.insert(id.clone(), Some(accel.clone()));
    }
    // No active shortcut may be used twice.
    let mut seen: HashMap<String, &'static str> = HashMap::new();
    let reserved: Vec<Option<String>> = reserved_shortcuts(mac, IS_WIN).iter().map(|r| accel_key(&r.accel, mac)).collect();
    for def in SHORTCUTS {
        for a in shortcut_accels(def.id, Some(&out), mac) {
            let key = accel_key(&a, mac)?;
            if reserved.contains(&Some(key.clone())) && out.contains_key(def.id) {
                return None;
            }
            if let Some(other) = seen.get(&key)
                && *other != def.id
                && (out.contains_key(def.id) || out.contains_key(*other))
            {
                return None;
            }
            seen.insert(key, def.id);
        }
    }
    Some(out)
}

/// Same as `clean_overrides` on untyped JSON (settings written by hand or coming from IPC): None if not an object or
/// if a value is neither a string nor null.
pub fn clean_overrides_value(value: &Value, mac: bool) -> Option<ShortcutOverrides> {
    let obj = value.as_object()?;
    let mut typed = ShortcutOverrides::new();
    for (k, v) in obj {
        match v {
            Value::Null => typed.insert(k.clone(), None),
            Value::String(s) => typed.insert(k.clone(), Some(s.clone())),
            _ => {
                // A known command with a wrong type rejects the whole set; unknown ones are ignored.
                if shortcut_def(k).is_some() {
                    return None;
                }
                None
            }
        };
    }
    clean_overrides(&typed, mac)
}

/// Every shortcut Signet itself uses right now (user overrides, extra shortcuts and reserved ones included), as
/// comparable keys (see `accel_key`) mapped to an owner label. Extension commands never take these.
pub fn signet_accel_keys(overrides: Option<&ShortcutOverrides>, mac: bool) -> IndexMap<String, String> {
    let mut out = IndexMap::new();
    for r in reserved_shortcuts(mac, IS_WIN) {
        if let Some(key) = accel_key(&r.accel, mac) {
            out.insert(key, r.label.to_string());
        }
    }
    for def in SHORTCUTS {
        for accel in shortcut_accels(def.id, overrides, mac) {
            if let Some(key) = accel_key(&accel, mac) {
                out.insert(key, def.id.to_string());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ov(pairs: &[(&str, Option<&str>)]) -> ShortcutOverrides {
        pairs.iter().map(|(k, v)| (k.to_string(), v.map(String::from))).collect()
    }

    #[test]
    fn table_is_consistent() {
        let ids: std::collections::HashSet<_> = SHORTCUTS.iter().map(|s| s.id).collect();
        assert_eq!(ids.len(), SHORTCUTS.len());
        for s in SHORTCUTS {
            assert!(SHORTCUT_GROUPS.contains(&s.group));
            assert!(s.label.starts_with("settings.shortcuts.item."));
        }
        for f in &FIXED_SHORTCUTS {
            assert!(SHORTCUT_GROUPS.contains(&f.group));
        }
        for s in SHORTCUTS {
            for mac in [true, false] {
                let all = [s.accel, s.mac, s.other].into_iter().flatten().chain(s.alt.iter().copied()).chain(s.alt_mac.iter().copied());
                for a in all {
                    assert!(parse_accel(a, mac).is_some(), "{}: {a}", s.id);
                }
            }
        }
    }

    #[test]
    fn no_duplicate_defaults_and_no_reserved() {
        for mac in [true, false] {
            let mut seen: HashMap<String, &str> = HashMap::new();
            let reserved: Vec<_> = reserved_shortcuts(mac, !mac).iter().map(|r| accel_key(&r.accel, mac)).collect();
            for s in SHORTCUTS {
                for a in shortcut_accels(s.id, None, mac) {
                    let key = accel_key(&a, mac).unwrap();
                    assert!(!seen.contains_key(&key), "{a} shared by {} and {:?} (mac={mac})", s.id, seen.get(&key));
                    seen.insert(key.clone(), s.id);
                    assert!(!reserved.contains(&Some(key)), "{}: {a} (mac={mac})", s.id);
                }
            }
        }
        assert_eq!(shortcut_def("newTab").unwrap().accel, Some("CmdOrCtrl+T"));
        assert!(shortcut_def("nope").is_none());
    }

    #[test]
    fn parse_resolves_and_aliases() {
        let p = |a, m| parse_accel(a, m).unwrap();
        assert_eq!(p("CmdOrCtrl+T", true), ParsedAccel { cmd: true, ctrl: false, alt: false, shift: false, key: "T".into() });
        assert_eq!(p("CmdOrCtrl+T", false), ParsedAccel { cmd: false, ctrl: true, alt: false, shift: false, key: "T".into() });
        assert_eq!(p("command+option+shift+a", true), ParsedAccel { cmd: true, ctrl: false, alt: true, shift: true, key: "A".into() });
        let q = p("Control+Super+F5", false);
        assert!(q.ctrl && q.cmd && q.key == "F5");
        assert_eq!(p("Ctrl+enter", true).key, "Return");
        assert_eq!(p("Ctrl+esc", true).key, "Escape");
        assert_eq!(p("Ctrl+pageup", true).key, "PageUp");
        assert_eq!(p("Ctrl+F24", true).key, "F24");
        assert_eq!(p("CmdOrCtrl+,", true).key, ",");
        let s = p("Ctrl+Shift+=", true);
        assert!(s.shift && s.key == "=");
    }

    #[test]
    fn parse_rejects() {
        for bad in ["", "+", "Ctrl+", "+T", "Ctrl++", "Foo+T", "Ctrl+F25", "Ctrl+F0", "Ctrl+AB", "Ctrl+\u{e9}", "Ctrl+T+"] {
            assert!(parse_accel(bad, true).is_none(), "{bad}");
        }
        assert!(parse_accel(&format!("Ctrl+{}", "a".repeat(60)), true).is_none());
    }

    #[test]
    fn keys_compare() {
        assert_eq!(accel_key("Shift+CmdOrCtrl+T", true).unwrap(), "shift+cmd+T");
        assert_eq!(accel_key("Cmd+Shift+t", true).unwrap(), "shift+cmd+T");
        assert_eq!(accel_key("CmdOrCtrl+T", false).unwrap(), "ctrl+T");
        assert_eq!(accel_key("Ctrl+Shift+=", true), accel_key("Ctrl+Plus", true));
        assert_eq!(accel_key("Ctrl+Shift+Plus", true), accel_key("Ctrl+Plus", true));
        assert!(accel_key("???", true).is_none());
    }

    #[test]
    fn usable() {
        assert!(!usable_accel("Shift+T", true));
        assert!(!usable_accel("T", true));
        assert!(usable_accel("Alt+T", true));
        assert!(usable_accel("CmdOrCtrl+T", false));
        assert!(usable_accel("F11", false));
        assert!(usable_accel("Shift+F5", true));
        assert!(!usable_accel("Ctrl+", true));
    }

    #[test]
    fn labels() {
        assert_eq!(accel_label("Ctrl+Alt+Shift+Cmd+T", true), "\u{2303}\u{2325}\u{21e7}\u{2318}T");
        assert_eq!(accel_label("CmdOrCtrl+Left", true), "\u{2318}\u{2190}");
        assert_eq!(accel_label("CmdOrCtrl+Backspace", true), "\u{2318}\u{232b}");
        assert_eq!(accel_label("Shift+CmdOrCtrl+T", false), "Ctrl+Shift+T");
        assert_eq!(accel_label("Alt+Return", false), "Alt+Enter");
        assert_eq!(accel_label("Ctrl+Escape", false), "Ctrl+Esc");
        assert_eq!(accel_label("Cmd+T", false), "Win+T");
        assert_eq!(accel_label("n\u{2019}importe quoi", true), "n\u{2019}importe quoi");
    }

    #[test]
    fn active_shortcuts() {
        let lib = shortcut_def("library").unwrap();
        assert_eq!(default_accel(lib, true), Some("Cmd+Y"));
        assert_eq!(default_accel(lib, false), Some("Ctrl+H"));
        assert_eq!(default_accel(shortcut_def("newTab").unwrap(), true), Some("CmdOrCtrl+T"));
        assert!(shortcut_accels("nope", None, true).is_empty());
        assert!(shortcut_accels("newEasel", None, true).is_empty());
        assert_eq!(shortcut_accel("newEasel", None, true), None);
        assert!(shortcut_accels("nextTab", None, false).len() > 1);
        assert!(shortcut_accels("tabLayout", None, true).len() > shortcut_accels("tabLayout", None, false).len());
        assert_eq!(shortcut_accels("newTab", Some(&ov(&[("newTab", Some("Alt+X"))])), true), ["Alt+X"]);
        assert!(shortcut_accels("newTab", Some(&ov(&[("newTab", None)])), true).is_empty());
        assert_eq!(shortcut_label("newTab", None, true).as_deref(), Some("\u{2318}T"));
        assert_eq!(shortcut_label("newTab", None, false).as_deref(), Some("Ctrl+T"));
        assert_eq!(shortcut_label("newTab", Some(&ov(&[("newTab", None)])), true), None);
        assert_eq!(shortcut_label("nope", None, true), None);
    }

    #[test]
    fn reserved() {
        let labels = |mac| reserved_shortcuts(mac, false).iter().map(|r| r.label).collect::<Vec<_>>();
        assert!(labels(true).contains(&"settings.shortcuts.reserved.hide"));
        assert!(!labels(false).contains(&"settings.shortcuts.reserved.hide"));
        let find = |mac, win, suffix: &str| reserved_shortcuts(mac, win).into_iter().find(|r| r.label.ends_with(suffix)).unwrap().accel;
        assert_eq!(find(false, true, "redo"), "Ctrl+Y");
        assert_eq!(find(false, false, "redo"), "Shift+CmdOrCtrl+Z");
        assert_eq!(find(true, false, "fullScreen"), "Ctrl+Cmd+F");
        assert_eq!(find(false, false, "fullScreen"), "F11");
        let list = reserved_shortcuts(true, false);
        assert_eq!(list.iter().filter(|r| r.label.ends_with("goToTab")).count(), 9);
        assert_eq!(list.iter().filter(|r| r.label.ends_with("goToSpace")).count(), 9);
        assert!(reserved_shortcuts(false, false).iter().find(|r| r.accel == "Alt+3").unwrap().label.ends_with("goToSpace"));
    }

    #[test]
    fn conflicts() {
        assert_eq!(shortcut_conflict("CmdOrCtrl+C", "newEasel", None, true), Some(Conflict::Reserved("settings.shortcuts.reserved.copy")));
        assert_eq!(shortcut_conflict("CmdOrCtrl+T", "closeTab", None, true), Some(Conflict::Command("newTab".into())));
        assert_eq!(shortcut_conflict("CmdOrCtrl+T", "newTab", None, true), None);
        assert_eq!(shortcut_conflict("CmdOrCtrl+T", "closeTab", Some(&ov(&[("newTab", None)])), true), None);
        assert_eq!(
            shortcut_conflict("Alt+CmdOrCtrl+9", "closeTab", Some(&ov(&[("newEasel", Some("Alt+CmdOrCtrl+9"))])), true),
            Some(Conflict::Command("newEasel".into()))
        );
        assert_eq!(shortcut_conflict("cmd+t", "closeTab", None, true), Some(Conflict::Command("newTab".into())));
        assert_eq!(shortcut_conflict("nope", "newTab", None, true), None);
    }

    #[test]
    fn take_over_rules() {
        assert_eq!(take_over(&ShortcutOverrides::new(), "newEasel", "CmdOrCtrl+T", "newTab", true), ov(&[("newEasel", Some("CmdOrCtrl+T")), ("newTab", None)]));
        let def = SHORTCUTS.iter().find(|s| !s.alt.is_empty()).unwrap();
        let extra = def.alt[0];
        let main = default_accel(def, false).unwrap();
        let out = take_over(&ShortcutOverrides::new(), "newEasel", extra, def.id, false);
        assert_eq!(out[def.id].as_deref(), Some(main));
        assert_eq!(out["newEasel"].as_deref(), Some(extra));
    }

    #[test]
    fn clean_rules() {
        for v in [json!(null), json!(3), json!("x"), json!([])] {
            assert!(clean_overrides_value(&v, true).is_none());
        }
        assert_eq!(clean_overrides_value(&json!({"nope": "CmdOrCtrl+K"}), true).unwrap(), ShortcutOverrides::new());
        assert_eq!(clean_overrides_value(&json!({"newEasel": "Alt+CmdOrCtrl+9"}), true).unwrap(), ov(&[("newEasel", Some("Alt+CmdOrCtrl+9"))]));
        assert_eq!(clean_overrides_value(&json!({"newTab": "cmd+t"}), true).unwrap(), ShortcutOverrides::new());
        assert_eq!(clean_overrides_value(&json!({"newEasel": null}), true).unwrap(), ShortcutOverrides::new());
        assert_eq!(clean_overrides_value(&json!({"newTab": null}), true).unwrap(), ov(&[("newTab", None)]));
        assert!(clean_overrides_value(&json!({"newEasel": "Shift+T"}), true).is_none());
        assert!(clean_overrides_value(&json!({"newEasel": 42}), true).is_none());
        assert!(clean_overrides_value(&json!({"newEasel": "Ctrl+"}), true).is_none());
        assert!(clean_overrides_value(&json!({"newEasel": "CmdOrCtrl+C"}), true).is_none());
        assert!(clean_overrides_value(&json!({"newEasel": "Alt+CmdOrCtrl+9", "savePage": "Alt+CmdOrCtrl+9"}), true).is_none());
        assert!(clean_overrides_value(&json!({"newEasel": "CmdOrCtrl+T"}), true).is_none());
        assert_eq!(
            clean_overrides_value(&json!({"newTab": "CmdOrCtrl+W", "closeTab": "CmdOrCtrl+T"}), true).unwrap(),
            ov(&[("newTab", Some("CmdOrCtrl+W")), ("closeTab", Some("CmdOrCtrl+T"))])
        );
    }

    #[test]
    fn signet_keys_include_reserved_and_overrides() {
        let m = signet_accel_keys(None, true);
        assert_eq!(m.get("cmd+T").map(String::as_str), Some("newTab"));
        assert!(m.contains_key("cmd+Q"));
    }
}

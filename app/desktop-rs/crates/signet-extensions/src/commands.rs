//! `chrome.commands`: manifest `commands`, Chrome shortcut syntax, clashes, localisation, and the per-extension manager.
//! Port of `shared/extension-commands.ts` and `apis/commands.ts`. The platform is a parameter.

use crate::common::Emitter;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandPlatform {
    Darwin,
    Win32,
    Linux,
}

impl CommandPlatform {
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            CommandPlatform::Darwin
        } else if cfg!(target_os = "windows") {
            CommandPlatform::Win32
        } else {
            CommandPlatform::Linux
        }
    }
}

/// Most suggested shortcuts Chrome honors per extension; the others are ignored.
pub const MAX_SUGGESTED_KEYS: usize = 4;

/// Commands that click the extension's toolbar button instead of raising `commands.onCommand`.
pub const EXECUTE_COMMANDS: [&str; 3] = ["_execute_action", "_execute_browser_action", "_execute_page_action"];

pub const COMMAND_EVENT: &str = "commands.onCommand";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedKey {
    /// Command key (macOS only).
    pub cmd: bool,
    /// Real Control key.
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    /// Accelerator key name (`Y`, `F5`, `,`, `Up`, `MediaPlayPause`...).
    pub key: String,
    /// Name of the key in Chrome's syntax (for display).
    pub chrome_key: String,
}

fn is_media(k: &str) -> bool {
    matches!(k, "MediaNextTrack" | "MediaPlayPause" | "MediaPrevTrack" | "MediaStop")
}

fn accelerator_key(chrome: &str) -> Option<String> {
    let b = chrome.as_bytes();
    if b.len() == 1 && (b[0].is_ascii_uppercase() || b[0].is_ascii_digit()) {
        return Some(chrome.to_string());
    }
    if let Some(n) = chrome.strip_prefix('F').and_then(|n| n.parse::<u32>().ok()) {
        if (1..=12).contains(&n) && chrome == format!("F{n}") {
            return Some(chrome.to_string());
        }
    }
    let named = match chrome {
        "Comma" => ",",
        "Period" => ".",
        "Home" | "End" | "PageUp" | "PageDown" | "Space" | "Insert" | "Delete" | "Up" | "Down" | "Left" | "Right" => chrome,
        "UpArrow" => "Up",
        "DownArrow" => "Down",
        "LeftArrow" => "Left",
        "RightArrow" => "Right",
        "MediaNextTrack" => "MediaNextTrack",
        "MediaPlayPause" => "MediaPlayPause",
        "MediaPrevTrack" => "MediaPreviousTrack",
        "MediaStop" => "MediaStop",
        _ => return None,
    };
    Some(named.to_string())
}

/// Reads a Chrome shortcut (`Ctrl+Shift+Y`) for a platform. None when Chrome would refuse it: unknown token, no
/// Ctrl/Alt/Command/MacCtrl (media keys excepted), a modifier that does not exist on this system, or `Search`.
pub fn parse_suggested_key(text: &str, platform: CommandPlatform) -> Option<ParsedKey> {
    if text.is_empty() || text.len() > 60 {
        return None;
    }
    let mac = platform == CommandPlatform::Darwin;
    let mut parts: Vec<&str> = text.split('+').collect();
    let chrome_key = parts.pop()?.to_string();
    let key = accelerator_key(&chrome_key)?;
    let mut out = ParsedKey { cmd: false, ctrl: false, alt: false, shift: false, key, chrome_key: chrome_key.clone() };
    let mut seen: Vec<&str> = Vec::new();
    for p in parts {
        if seen.contains(&p) {
            return None;
        }
        seen.push(p);
        match p {
            "Ctrl" => {
                if mac {
                    out.cmd = true
                } else {
                    out.ctrl = true
                }
            }
            "Alt" => out.alt = true,
            "Shift" => out.shift = true,
            "Command" if mac => out.cmd = true,
            "MacCtrl" if mac => out.ctrl = true,
            _ => return None,
        }
    }
    if is_media(&chrome_key) {
        return Some(out);
    }
    if !out.cmd && !out.ctrl && !out.alt {
        return None;
    }
    Some(out)
}

/// The suggested key of a command for this platform: the platform's own entry, otherwise `default`.
pub fn suggested_key_for(suggested: &Value, platform: CommandPlatform) -> Option<String> {
    match suggested {
        Value::String(s) => Some(s.clone()),
        Value::Object(o) => {
            let own = match platform {
                CommandPlatform::Darwin => "mac",
                CommandPlatform::Win32 => "windows",
                CommandPlatform::Linux => "linux",
            };
            o.get(own).or_else(|| o.get("default")).and_then(|v| v.as_str()).map(String::from)
        }
        _ => None,
    }
}

/// Accelerator string (`Cmd+Shift+Y`).
pub fn to_accelerator(p: &ParsedKey) -> String {
    let mut v: Vec<&str> = Vec::new();
    if p.cmd {
        v.push("Cmd");
    }
    if p.ctrl {
        v.push("Ctrl");
    }
    if p.alt {
        v.push("Alt");
    }
    if p.shift {
        v.push("Shift");
    }
    v.push(&p.key);
    v.join("+")
}

/// Comparable form, same shape as `accelKey` in shortcuts (so it can be matched with Signet's own shortcuts).
pub fn identity_of(p: &ParsedKey) -> String {
    let mut v: Vec<&str> = Vec::new();
    if p.ctrl {
        v.push("ctrl");
    }
    if p.alt {
        v.push("alt");
    }
    if p.shift {
        v.push("shift");
    }
    if p.cmd {
        v.push("cmd");
    }
    v.push(&p.key);
    v.join("+")
}

/// Chrome's display format: `⇧⌘Y` on macOS, `Ctrl+Shift+Y` elsewhere.
pub fn display_shortcut(p: &ParsedKey, platform: CommandPlatform) -> String {
    if platform == CommandPlatform::Darwin {
        let arrow = match p.key.as_str() {
            "Up" => Some("↑"),
            "Down" => Some("↓"),
            "Left" => Some("←"),
            "Right" => Some("→"),
            _ => None,
        };
        return format!(
            "{}{}{}{}{}",
            if p.ctrl { "⌃" } else { "" },
            if p.alt { "⌥" } else { "" },
            if p.shift { "⇧" } else { "" },
            if p.cmd { "⌘" } else { "" },
            arrow.unwrap_or(&p.chrome_key)
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
    v.push(&p.chrome_key);
    v.join("+")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestCommand {
    pub name: String,
    /// As written in the manifest (maybe `__MSG_key__`).
    pub description: String,
    /// The shortcut this platform asks for, or None (none, invalid, or beyond the limit of four).
    pub key: Option<ParsedKey>,
}

/// Commands of a manifest, in manifest order. Only the first four valid shortcuts are kept.
pub fn read_commands(manifest: &Value, platform: CommandPlatform) -> Vec<ManifestCommand> {
    let Some(Value::Object(commands)) = manifest.get("commands") else { return vec![] };
    let mut out = Vec::new();
    let mut kept = 0;
    for (name, raw) in commands {
        if name.is_empty() {
            continue;
        }
        let spec = raw.as_object();
        let suggested = spec.and_then(|s| s.get("suggested_key")).and_then(|s| suggested_key_for(s, platform));
        let mut key = suggested.and_then(|s| parse_suggested_key(&s, platform));
        if key.is_some() && kept >= MAX_SUGGESTED_KEYS {
            key = None;
        }
        if key.is_some() {
            kept += 1;
        }
        let description = spec.and_then(|s| s.get("description")).and_then(|d| d.as_str()).unwrap_or("").to_string();
        out.push(ManifestCommand { name: name.clone(), description, key });
    }
    out
}

#[derive(Debug, Clone)]
pub struct ExtensionCommandSet {
    pub id: String,
    pub name: String,
    pub commands: Vec<ManifestCommand>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedCommand {
    pub extension_id: String,
    pub name: String,
    pub description: String,
    /// Accelerator when the command owns its shortcut, otherwise None.
    pub accel: Option<String>,
    /// Display text of the shortcut, empty when none or lost.
    pub shortcut: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clash {
    pub extension_id: String,
    pub command: String,
    pub accel: String,
    /// Who keeps the shortcut: `Signet` or an extension (`<id> <command>`).
    pub owner: String,
}

/// Decides who owns each shortcut: Signet's own shortcuts first (`signet`: identity key to owner label), then the
/// extension that came first in `sets`. A loser stays listed, without a shortcut.
pub fn resolve_commands(
    sets: &[ExtensionCommandSet],
    signet: &HashMap<String, String>,
    platform: CommandPlatform,
) -> (Vec<ResolvedCommand>, Vec<Clash>) {
    let mut taken: HashMap<String, String> = HashMap::new();
    let mut commands = Vec::new();
    let mut clashes = Vec::new();
    for set in sets {
        for c in &set.commands {
            let mk = |accel: Option<String>, shortcut: String| ResolvedCommand {
                extension_id: set.id.clone(),
                name: c.name.clone(),
                description: c.description.clone(),
                accel,
                shortcut,
            };
            let Some(key) = &c.key else {
                commands.push(mk(None, String::new()));
                continue;
            };
            let id = identity_of(key);
            let owner = if signet.contains_key(&id) { Some("Signet".to_string()) } else { taken.get(&id).cloned() };
            if let Some(owner) = owner {
                clashes.push(Clash { extension_id: set.id.clone(), command: c.name.clone(), accel: to_accelerator(key), owner });
                commands.push(mk(None, String::new()));
                continue;
            }
            taken.insert(id, format!("{} {}", set.id, c.name));
            commands.push(mk(Some(to_accelerator(key)), display_shortcut(key, platform)));
        }
    }
    (commands, clashes)
}

// ---- Localisation ----

/// Locales to try for the app locale (`fr-CA`): `fr_CA`, `fr`, then the manifest's `default_locale`.
pub fn locale_candidates(app_locale: &str, default_locale: Option<&str>) -> Vec<String> {
    let full = app_locale.replace('-', "_");
    let base = full.split('_').next().unwrap_or("").to_string();
    let mut out: Vec<String> = Vec::new();
    for l in [Some(full.clone()), Some(base), default_locale.map(String::from)].into_iter().flatten() {
        if !l.is_empty() && !out.contains(&l) {
            out.push(l);
        }
    }
    out
}

/// Resolves `__MSG_key__` like Chrome (key lookup ignores case, `$NAME$` placeholders and `$$` are expanded). Other text
/// is returned as is. An unknown key gives an empty string, as Chrome does. `load` returns the `messages.json` of a locale.
pub fn localize_text(text: &str, candidates: &[String], load: &dyn Fn(&str) -> Option<Value>) -> String {
    let Some(key) = text.strip_prefix("__MSG_").and_then(|r| r.strip_suffix("__")) else { return text.to_string() };
    if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '@') {
        return text.to_string();
    }
    let key = key.to_lowercase();
    for locale in candidates {
        let Some(Value::Object(messages)) = load(locale) else { continue };
        let Some(entry) = messages.iter().find(|(k, _)| k.to_lowercase() == key).map(|(_, v)| v) else { continue };
        let Some(message) = entry.get("message").and_then(|m| m.as_str()) else { continue };
        let placeholders: HashMap<String, String> = entry
            .get("placeholders")
            .and_then(|p| p.as_object())
            .map(|o| {
                o.iter()
                    .map(|(k, v)| (k.to_lowercase(), v.get("content").and_then(|c| c.as_str()).unwrap_or("").to_string()))
                    .collect()
            })
            .unwrap_or_default();
        return expand_placeholders(message, &placeholders);
    }
    String::new()
}

fn expand_placeholders(message: &str, placeholders: &HashMap<String, String>) -> String {
    let chars: Vec<char> = message.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '$' {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        if chars.get(i + 1) == Some(&'$') {
            out.push('$');
            i += 2;
            continue;
        }
        let mut j = i + 1;
        while j < chars.len() && (chars[j].is_ascii_alphanumeric() || chars[j] == '_' || chars[j] == '@') {
            j += 1;
        }
        if j > i + 1 && chars.get(j) == Some(&'$') {
            let name: String = chars[i + 1..j].iter().collect::<String>().to_lowercase();
            match placeholders.get(&name) {
                Some(v) => out.push_str(v),
                None => out.extend(&chars[i..=j]),
            }
            i = j + 1;
        } else {
            out.push('$');
            i += 1;
        }
    }
    out
}

// ---- Manager ----

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandInfo {
    pub name: String,
    pub description: String,
    pub shortcut: String,
}

impl CommandInfo {
    pub fn to_json(&self) -> Value {
        json!({ "name": self.name, "description": self.description, "shortcut": self.shortcut })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandBinding {
    pub accel: String,
    pub extension_id: String,
    pub name: String,
}

/// What the commands need from the rest of the app.
pub struct CommandsDeps {
    pub platform: CommandPlatform,
    pub emitter: Arc<dyn Emitter>,
    /// Signet's own shortcuts as they are now (identity key to owner label), user overrides included.
    pub signet_keys: Box<dyn Fn() -> HashMap<String, String> + Send + Sync>,
    /// Text of an extension's manifest, `__MSG_key__` resolved in its locale.
    pub localize: Box<dyn Fn(&str, &str) -> String + Send + Sync>,
    /// The tab in Chrome's `tabs.Tab` shape for the target the shortcut was pressed on, if there is one.
    pub active_tab: Box<dyn Fn(Option<&str>) -> Option<Value> + Send + Sync>,
}

/// What a command run asks the app to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandRun {
    /// Click the extension's toolbar button (open its popup or raise `action.onClicked`).
    RunAction { extension_id: String, target: Option<String> },
    /// `commands.onCommand` was emitted.
    Emitted,
}

/// The commands of every loaded extension and who owns each shortcut. Ownership is worked out each time from the
/// registered extensions and Signet's current shortcuts, so a change in either takes effect without bookkeeping.
pub struct ExtensionCommands {
    d: CommandsDeps,
    /// Insertion order is registration order: the first extension to register keeps a contested shortcut.
    sets: Vec<ExtensionCommandSet>,
    logged: Vec<String>,
    log_lines: Vec<String>,
}

impl ExtensionCommands {
    pub fn new(d: CommandsDeps) -> Self {
        ExtensionCommands { d, sets: Vec::new(), logged: Vec::new(), log_lines: Vec::new() }
    }

    /// An extension was loaded (or reloaded: it keeps its place in line).
    pub fn register(&mut self, id: &str, name: &str, manifest: &Value) {
        let set = ExtensionCommandSet { id: id.into(), name: name.into(), commands: read_commands(manifest, self.d.platform) };
        match self.sets.iter_mut().find(|s| s.id == id) {
            Some(existing) => *existing = set,
            None => self.sets.push(set),
        }
    }

    pub fn unregister(&mut self, id: &str) {
        self.sets.retain(|s| s.id != id);
    }

    /// Clash lines produced since the last call (development logging); each line appears once.
    pub fn take_log(&mut self) -> Vec<String> {
        std::mem::take(&mut self.log_lines)
    }

    fn resolve(&mut self) -> Vec<ResolvedCommand> {
        let (commands, clashes) = resolve_commands(&self.sets, &(self.d.signet_keys)(), self.d.platform);
        for c in clashes {
            let line = format!("[extension-commands] {} {} {} taken by {}", c.extension_id, c.command, c.accel, c.owner);
            if !self.logged.contains(&line) {
                self.logged.push(line.clone());
                self.log_lines.push(line);
            }
        }
        commands
    }

    fn info(&self, c: &ResolvedCommand) -> CommandInfo {
        CommandInfo { name: c.name.clone(), description: (self.d.localize)(&c.extension_id, &c.description), shortcut: c.shortcut.clone() }
    }

    /// `commands.getAll` for an extension.
    pub fn list_for(&mut self, id: &str) -> Vec<CommandInfo> {
        self.resolve().iter().filter(|c| c.extension_id == id).map(|c| self.info(c)).collect()
    }

    /// Commands of every extension that has any (Settings), in registration order.
    pub fn list_all(&mut self) -> Vec<(String, Vec<CommandInfo>)> {
        let mut out: Vec<(String, Vec<CommandInfo>)> = Vec::new();
        for c in self.resolve() {
            let info = self.info(&c);
            match out.iter_mut().find(|(id, _)| *id == c.extension_id) {
                Some((_, list)) => list.push(info),
                None => out.push((c.extension_id.clone(), vec![info])),
            }
        }
        out
    }

    /// Shortcuts to register in the application menu.
    pub fn bindings(&mut self) -> Vec<CommandBinding> {
        self.resolve()
            .into_iter()
            .filter_map(|c| c.accel.map(|accel| CommandBinding { accel, extension_id: c.extension_id, name: c.name }))
            .collect()
    }

    /// Changes whenever the menu has to be rebuilt.
    pub fn key(&mut self) -> String {
        self.bindings().iter().map(|b| format!("{}={}/{}", b.accel, b.extension_id, b.name)).collect::<Vec<_>>().join(",")
    }

    /// A shortcut was pressed; `target` is handed back to the hooks (the tab it was pressed on).
    pub fn run(&mut self, extension_id: &str, name: &str, target: Option<&str>) -> CommandRun {
        if EXECUTE_COMMANDS.contains(&name) {
            return CommandRun::RunAction { extension_id: extension_id.into(), target: target.map(String::from) };
        }
        let tab = (self.d.active_tab)(target);
        let args = match tab {
            Some(t) => vec![json!(name), t],
            None => vec![json!(name)],
        };
        // Wakes a stopped service worker, like alarms; open extension pages receive it too.
        self.d.emitter.emit(extension_id, COMMAND_EVENT, args, true, true);
        CommandRun::Emitted
    }

    /// Router handler for `commands.getAll`.
    pub fn get_all(&mut self, extension_id: &str) -> Value {
        Value::Array(self.list_for(extension_id).iter().map(CommandInfo::to_json).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::Outbox;
    use std::sync::Mutex;

    use CommandPlatform::*;

    fn acc(text: &str, p: CommandPlatform) -> Option<String> {
        parse_suggested_key(text, p).map(|k| to_accelerator(&k))
    }

    #[test]
    fn ctrl_maps_to_command_on_macos() {
        assert_eq!(acc("Ctrl+Shift+Y", Darwin).as_deref(), Some("Cmd+Shift+Y"));
        assert_eq!(acc("Ctrl+Shift+Y", Win32).as_deref(), Some("Ctrl+Shift+Y"));
        assert_eq!(acc("Ctrl+Shift+Y", Linux).as_deref(), Some("Ctrl+Shift+Y"));
    }

    #[test]
    fn macctrl_and_command() {
        assert_eq!(acc("MacCtrl+Shift+Y", Darwin).as_deref(), Some("Ctrl+Shift+Y"));
        assert_eq!(acc("Command+Shift+Y", Darwin).as_deref(), Some("Cmd+Shift+Y"));
        assert_eq!(acc("MacCtrl+Shift+Y", Win32), None);
        assert_eq!(acc("Command+Y", Linux), None);
    }

    #[test]
    fn supported_keys() {
        for k in ["A", "Z", "0", "9", "F1", "F12"] {
            assert_eq!(acc(&format!("Alt+{k}"), Win32), Some(format!("Alt+{k}")));
        }
        assert_eq!(acc("Alt+Comma", Win32).as_deref(), Some("Alt+,"));
        assert_eq!(acc("Alt+Period", Win32).as_deref(), Some("Alt+."));
        for k in ["Home", "End", "PageUp", "PageDown", "Space", "Insert", "Delete", "Up", "Down", "Left", "Right"] {
            assert_eq!(acc(&format!("Alt+{k}"), Linux), Some(format!("Alt+{k}")));
        }
        assert_eq!(acc("Alt+UpArrow", Linux).as_deref(), Some("Alt+Up"));
    }

    #[test]
    fn media_keys_need_no_modifier() {
        assert_eq!(acc("MediaPlayPause", Win32).as_deref(), Some("MediaPlayPause"));
        assert_eq!(acc("MediaPrevTrack", Darwin).as_deref(), Some("MediaPreviousTrack"));
        assert_eq!(acc("MediaNextTrack", Linux).as_deref(), Some("MediaNextTrack"));
        assert_eq!(acc("MediaStop", Linux).as_deref(), Some("MediaStop"));
    }

    #[test]
    fn rejects_what_chrome_rejects() {
        for bad in ["Shift+Y", "Y", "F5", "Ctrl+F13", "Ctrl+y", "Ctrl+Ctrl+Y", "Ctrl+Win+Y", ""] {
            assert_eq!(acc(bad, Win32), None, "{bad}");
        }
        assert_eq!(acc("Search+Y", Linux), None);
    }

    #[test]
    fn suggested_key_for_platform() {
        let spec = json!({ "default": "Ctrl+Shift+Y", "mac": "Command+Shift+U", "windows": "Alt+W", "linux": "Alt+L", "chromeos": "Alt+C" });
        assert_eq!(suggested_key_for(&spec, Darwin).as_deref(), Some("Command+Shift+U"));
        assert_eq!(suggested_key_for(&spec, Win32).as_deref(), Some("Alt+W"));
        assert_eq!(suggested_key_for(&spec, Linux).as_deref(), Some("Alt+L"));
        assert_eq!(suggested_key_for(&json!({ "default": "Ctrl+Y" }), Linux).as_deref(), Some("Ctrl+Y"));
        assert_eq!(suggested_key_for(&json!({ "mac": "Ctrl+Y" }), Linux), None);
        assert_eq!(suggested_key_for(&json!("Ctrl+Y"), Linux).as_deref(), Some("Ctrl+Y"));
        assert_eq!(suggested_key_for(&Value::Null, Linux), None);
    }

    #[test]
    fn display_uses_symbols_on_macos_and_words_elsewhere() {
        let p = parse_suggested_key("Ctrl+Shift+Y", Darwin).unwrap();
        assert_eq!(display_shortcut(&p, Darwin), "⇧⌘Y");
        assert_eq!(display_shortcut(&parse_suggested_key("MacCtrl+Alt+Up", Darwin).unwrap(), Darwin), "⌃⌥↑");
        assert_eq!(display_shortcut(&parse_suggested_key("Ctrl+Shift+Y", Win32).unwrap(), Win32), "Ctrl+Shift+Y");
        assert_eq!(display_shortcut(&parse_suggested_key("Alt+Ctrl+Shift+Up", Linux).unwrap(), Linux), "Ctrl+Alt+Shift+Up");
    }

    #[test]
    fn read_commands_lists_in_manifest_order() {
        let manifest = json!({
            "commands": {
                "_execute_action": { "suggested_key": { "default": "Alt+Shift+D" } },
                "zap": { "description": "Zap" },
                "toggle": { "description": "Toggle", "suggested_key": { "default": "Ctrl+Shift+L", "mac": "Command+Shift+L" } }
            }
        });
        let mac = read_commands(&manifest, Darwin);
        assert_eq!(mac.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["_execute_action", "zap", "toggle"]);
        assert_eq!(mac.iter().map(|c| c.key.as_ref().map(to_accelerator)).collect::<Vec<_>>(), [Some("Alt+Shift+D".to_string()), None, Some("Cmd+Shift+L".to_string())]);
        let win = read_commands(&manifest, Win32);
        let k = win[2].key.as_ref().unwrap();
        assert!(k.ctrl && k.key == "L");
    }

    #[test]
    fn read_commands_keeps_four_keys_and_tolerates_junk() {
        let mut commands = serde_json::Map::new();
        for k in ["A", "B", "C", "D", "E", "F"] {
            commands.insert(format!("c{k}"), json!({ "description": k, "suggested_key": { "default": format!("Alt+{k}") } }));
        }
        let keys: Vec<bool> = read_commands(&json!({ "commands": commands }), Linux).iter().map(|c| c.key.is_some()).collect();
        assert_eq!(keys, [true, true, true, true, false, false]);
        let c = json!({ "commands": { "bad": { "suggested_key": { "default": "Shift+X" } }, "a": { "suggested_key": { "default": "Alt+A" } } } });
        assert_eq!(read_commands(&c, Linux).iter().map(|c| c.key.is_some()).collect::<Vec<_>>(), [false, true]);
        assert!(read_commands(&Value::Null, Linux).is_empty());
        assert!(read_commands(&json!({ "commands": [] }), Linux).is_empty());
        assert_eq!(read_commands(&json!({ "commands": { "x": 3 } }), Linux), vec![ManifestCommand { name: "x".into(), description: String::new(), key: None }]);
    }

    fn set(id: &str, cmds: &[(&str, Option<&str>)]) -> ExtensionCommandSet {
        ExtensionCommandSet {
            id: id.into(),
            name: id.into(),
            commands: cmds
                .iter()
                .map(|(n, k)| ManifestCommand { name: n.to_string(), description: n.to_string(), key: k.and_then(|k| parse_suggested_key(k, Win32)) })
                .collect(),
        }
    }

    #[test]
    fn resolve_gives_the_shortcut_to_the_first_extension() {
        let (commands, clashes) = resolve_commands(
            &[set("one", &[("go", Some("Ctrl+Shift+Y"))]), set("two", &[("go", Some("Ctrl+Shift+Y")), ("other", Some("Alt+Q"))])],
            &HashMap::new(),
            Win32,
        );
        let rows: Vec<_> = commands.iter().map(|c| (c.extension_id.as_str(), c.name.as_str(), c.shortcut.as_str())).collect();
        assert_eq!(rows, [("one", "go", "Ctrl+Shift+Y"), ("two", "go", ""), ("two", "other", "Alt+Q")]);
        assert!(commands[1].accel.is_none());
        assert_eq!(clashes, vec![Clash { extension_id: "two".into(), command: "go".into(), accel: "Ctrl+Shift+Y".into(), owner: "one go".into() }]);
    }

    #[test]
    fn signet_shortcuts_win() {
        let signet: HashMap<String, String> =
            [("alt+shift+Q", "newTab"), ("ctrl+W", "closeTab"), ("ctrl+C", "copy"), ("ctrl+shift+T", "reopen")].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        let (commands, clashes) = resolve_commands(
            &[set("one", &[("a", Some("Alt+Shift+Q")), ("b", Some("Ctrl+W")), ("c", Some("Ctrl+C")), ("d", Some("Ctrl+Shift+T")), ("e", Some("Alt+Shift+Z"))])],
            &signet,
            Win32,
        );
        assert_eq!(commands.iter().map(|c| c.shortcut.as_str()).collect::<Vec<_>>(), ["", "", "", "", "Alt+Shift+Z"]);
        assert_eq!(clashes.iter().map(|c| c.command.as_str()).collect::<Vec<_>>(), ["a", "b", "c", "d"]);
        assert!(clashes.iter().all(|c| c.owner == "Signet"));
    }

    fn messages(locale: &str) -> Option<Value> {
        match locale {
            "fr" => Some(json!({ "Toggle": { "message": "Basculer $NAME$", "placeholders": { "name": { "content": "le mode" } } }, "price": { "message": "5$$" } })),
            "en" => Some(json!({ "toggle": { "message": "Toggle" }, "only_en": { "message": "English only" } })),
            _ => None,
        }
    }

    #[test]
    fn localization() {
        assert_eq!(locale_candidates("fr-CA", Some("en")), ["fr_CA", "fr", "en"]);
        assert_eq!(locale_candidates("en-US", Some("en")), ["en_US", "en"]);
        assert_eq!(locale_candidates("fr", None), ["fr"]);
        let c = vec!["fr".to_string(), "en".to_string()];
        assert_eq!(localize_text("__MSG_toggle__", &c, &messages), "Basculer le mode");
        assert_eq!(localize_text("__MSG_price__", &c, &messages), "5$");
        assert_eq!(localize_text("__MSG_only_en__", &c, &messages), "English only");
        assert_eq!(localize_text("__MSG_missing__", &c, &messages), "");
        assert_eq!(localize_text("Plain text", &["en".to_string()], &messages), "Plain text");
        assert_eq!(localize_text("", &["en".to_string()], &messages), "");
    }

    // ---- manager ----

    struct Setup {
        commands: ExtensionCommands,
        out: Arc<Outbox>,
        overrides: Arc<Mutex<HashMap<String, String>>>,
    }

    fn setup(platform: CommandPlatform) -> Setup {
        let out = Arc::new(Outbox::new());
        let overrides = Arc::new(Mutex::new(HashMap::new()));
        let o2 = overrides.clone();
        let commands = ExtensionCommands::new(CommandsDeps {
            platform,
            emitter: out.clone(),
            signet_keys: Box::new(move || o2.lock().unwrap().clone()),
            localize: Box::new(|_, text| if text == "__MSG_d__" { "Localized".into() } else { text.into() }),
            active_tab: Box::new(|t| t.map(|_| json!({ "id": 7 }))),
        });
        Setup { commands, out, overrides }
    }

    fn manifest(commands: Value) -> Value {
        json!({ "commands": commands })
    }

    #[test]
    fn get_all_lists_name_localized_description_and_shortcut() {
        let mut s = setup(Darwin);
        s.commands.register(
            "a",
            "A",
            &manifest(json!({
                "_execute_action": { "suggested_key": { "default": "Alt+Shift+D" } },
                "zap": { "description": "__MSG_d__" },
                "go": { "description": "Go", "suggested_key": { "default": "Ctrl+Shift+Y" } }
            })),
        );
        let rows: Vec<_> = s.commands.list_for("a").iter().map(|c| (c.name.clone(), c.description.clone(), c.shortcut.clone())).collect();
        assert_eq!(
            rows,
            [("_execute_action".into(), "".into(), "⌥⇧D".into()), ("zap".into(), "Localized".into(), "".into()), ("go".into(), "Go".into(), "⇧⌘Y".into())]
        );
        assert!(s.commands.list_for("unknown").is_empty());
    }

    #[test]
    fn a_loser_stays_listed_and_the_clash_is_logged_once() {
        let mut s = setup(Win32);
        let m = manifest(json!({ "go": { "description": "Go", "suggested_key": { "default": "Ctrl+Shift+Y" } } }));
        s.commands.register("a", "A", &m);
        s.commands.register("b", "B", &m);
        assert_eq!(s.commands.list_for("b")[0].shortcut, "");
        s.commands.list_for("b");
        s.commands.bindings();
        assert_eq!(s.commands.take_log(), ["[extension-commands] b go Ctrl+Shift+Y taken by a go"]);
        assert_eq!(s.commands.bindings(), vec![CommandBinding { accel: "Ctrl+Shift+Y".into(), extension_id: "a".into(), name: "go".into() }]);
    }

    #[test]
    fn re_evaluates_when_extensions_unload_or_signet_shortcuts_change() {
        let mut s = setup(Win32);
        let m = manifest(json!({ "go": { "description": "Go", "suggested_key": { "default": "Alt+Shift+Q" } } }));
        s.commands.register("a", "A", &m);
        s.commands.register("b", "B", &m);
        assert_eq!(s.commands.bindings().iter().map(|b| b.extension_id.clone()).collect::<Vec<_>>(), ["a"]);
        let before = s.commands.key();
        s.commands.unregister("a");
        assert_eq!(s.commands.bindings().iter().map(|b| b.extension_id.clone()).collect::<Vec<_>>(), ["b"]);
        assert_ne!(s.commands.key(), before);
        s.overrides.lock().unwrap().insert("alt+shift+Q".into(), "newTab".into());
        assert!(s.commands.bindings().is_empty());
        assert_eq!(s.commands.list_for("b")[0].shortcut, "");
    }

    #[test]
    fn a_reloaded_extension_keeps_its_place_in_line() {
        let mut s = setup(Win32);
        let m = manifest(json!({ "go": { "description": "Go", "suggested_key": { "default": "Alt+Shift+Q" } } }));
        s.commands.register("a", "A", &m);
        s.commands.register("b", "B", &m);
        s.commands.register("a", "A", &m);
        assert_eq!(s.commands.bindings().iter().map(|b| b.extension_id.clone()).collect::<Vec<_>>(), ["a"]);
    }

    #[test]
    fn list_all_only_has_extensions_with_commands() {
        let mut s = setup(Win32);
        s.commands.register("a", "A", &manifest(json!({ "one": { "description": "One" } })));
        s.commands.register("b", "B", &json!({}));
        assert_eq!(s.commands.list_all().iter().map(|(id, _)| id.clone()).collect::<Vec<_>>(), ["a"]);
    }

    #[test]
    fn platform_shortcuts_are_resolved_per_platform() {
        let m = manifest(json!({ "go": { "description": "Go", "suggested_key": { "default": "Ctrl+Shift+Y", "mac": "MacCtrl+Shift+Y" } } }));
        let mut win = setup(Win32);
        let mut mac = setup(Darwin);
        win.commands.register("a", "A", &m);
        mac.commands.register("a", "A", &m);
        assert_eq!(win.commands.bindings()[0].accel, "Ctrl+Shift+Y");
        assert_eq!(mac.commands.bindings()[0].accel, "Ctrl+Shift+Y");
        assert_eq!(mac.commands.list_for("a")[0].shortcut, "⌃⇧Y");
        let k = parse_suggested_key("Ctrl+Shift+Y", Darwin).unwrap();
        assert!(k.cmd && !k.ctrl);
    }

    #[test]
    fn execute_commands_click_the_button() {
        let mut s = setup(Win32);
        for name in EXECUTE_COMMANDS {
            assert_eq!(s.commands.run("a", name, Some("t")), CommandRun::RunAction { extension_id: "a".into(), target: Some("t".into()) });
        }
        assert!(s.out.drain().is_empty());
    }

    #[test]
    fn other_commands_wake_the_worker_with_name_and_tab() {
        let mut s = setup(Win32);
        s.commands.run("a", "toggle", Some("t"));
        let ev = s.out.drain();
        assert_eq!((ev[0].extension_id.as_str(), ev[0].name.as_str(), ev[0].wake), ("a", COMMAND_EVENT, true));
        assert_eq!(ev[0].args, vec![json!("toggle"), json!({ "id": 7 })]);
        s.commands.run("a", "toggle", None);
        assert_eq!(s.out.drain()[0].args, vec![json!("toggle")]);
    }

    #[test]
    fn get_all_handler_answers_for_the_caller() {
        let mut s = setup(Win32);
        s.commands.register("a", "A", &manifest(json!({ "one": { "description": "One" } })));
        assert_eq!(s.commands.get_all("a"), json!([{ "name": "one", "description": "One", "shortcut": "" }]));
    }
}

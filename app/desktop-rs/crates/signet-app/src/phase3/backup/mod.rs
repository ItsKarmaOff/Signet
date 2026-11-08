//! Backup and restore (port of the app side of `main/features/backup.ts`; the format and the rules are in
//! `signet_features::backup`). Here: native dialogs, authentication before exporting passwords, the pending
//! backup token, the `Hooks` implementation, safety copies and the restart after a restore.

pub(crate) mod dialog;

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use indexmap::IndexMap;
use serde_json::{json, Value};
use signet_core::api::BackupOpen;
use signet_core::core::store::{clean_draft, clean_settings, CLEAN_SETTING_KEYS};
use signet_core::ids::new_id;
use signet_core::types::{BackupInfo, BackupMode, BackupSection, BackupSummary, SafetyCopy, Settings, SpaceDraft, SpaceTheme};
use signet_features::backup::{self as fb, Hooks, Mode, Paths};
use signet_features::osauth::{AuthGate, SystemAuth};
use signet_features::passwords::{Cipher, KeychainCipher, Passwords};
use signet_features::secrets::KeyringStore;

use crate::browser::Core;
use dialog::Level;

const EXT: &str = "signetbackup";

/// State owned by this feature (a field of `Phase3`).
pub struct State {
    /// Backup read and awaiting import (the file never goes back through the interface).
    pending: Option<(String, Value)>,
    /// Authentication before exporting passwords (60 s grace).
    gate: Arc<AuthGate>,
    /// Extension ids left by a restore, to install once (see `take_pending_extensions`).
    pending_extensions: Vec<String>,
}

impl Default for State {
    fn default() -> Self {
        State { pending: None, gate: Arc::new(AuthGate::new(Box::new(SystemAuth))), pending_extensions: Vec::new() }
    }
}

// ------------------------------------------------------------------ helpers (pure)

/// Translation callback of signet-features.
pub(crate) fn translate(key: &str, args: &[(&str, String)]) -> String {
    let a: Vec<(&str, &str)> = args.iter().map(|(k, v)| (*k, v.as_str())).collect();
    signet_core::i18n::t_args(key, &a)
}

fn section_name(s: BackupSection) -> String {
    serde_json::to_value(s).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default()
}

/// Counts of a backup (`fb::counts`, `fb::info`) as the typed map of the interface, in Settings order.
fn typed_counts(map: &serde_json::Map<String, Value>) -> IndexMap<BackupSection, i64> {
    signet_core::types::BACKUP_SECTIONS
        .iter()
        .filter_map(|s| map.get(&section_name(*s)).and_then(Value::as_f64).map(|n| (*s, n as i64)))
        .collect()
}

/// `.signetbackup` file name: `<name> YYYY-MM-DD` (UTC date).
fn default_file_name(base: &str, now_ms: i64) -> String {
    let (y, m, d) = civil_from_days(now_ms.div_euclid(86_400_000));
    format!("{base} {y:04}-{m:02}-{d:02}.{EXT}")
}

/// Proleptic Gregorian date of a day count since 1970-01-01 (Howard Hinnant's algorithm).
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 with padding (the encoding of `passwords.json`).
fn base64(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (u32::from(c[0]) << 16) | (u32::from(*c.get(1).unwrap_or(&0)) << 8) | u32::from(*c.get(2).unwrap_or(&0));
        out.push(B64[(n >> 18) as usize & 63] as char);
        out.push(B64[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { B64[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { B64[n as usize & 63] as char } else { '=' });
    }
    out
}

/// Settings of a backup, validated like `cleanSettings` + `applyCleaned`: only the keys signet-core validates are
/// replaced by their cleaned value (or dropped), every other key is kept as the backup has it.
fn clean_settings_value(incoming: Value, current: &Value) -> Value {
    let Value::Object(mut inc) = incoming else { return incoming };
    let base: Settings = serde_json::from_value(current.clone()).unwrap_or_default();
    let cleaned = clean_settings(&inc, &base);
    if let Ok(Value::Object(c)) = serde_json::to_value(&cleaned) {
        for k in CLEAN_SETTING_KEYS {
            match c.get(k) {
                Some(v) => {
                    inc.insert(k.to_string(), v.clone());
                }
                None => {
                    inc.remove(k);
                }
            }
        }
    }
    Value::Object(inc)
}

/// Space theme of a backup validated like the space form (gradient of 2 or 3 hues), `None` = solid color.
fn clean_theme_value(theme: &Value, name: &str, icon: &str, hue: i64) -> Option<Value> {
    let draft = SpaceDraft { name: name.into(), icon: icon.into(), hue: hue as f64, theme: serde_json::from_value::<SpaceTheme>(theme.clone()).ok() };
    clean_draft(&draft).theme.and_then(|t| serde_json::to_value(t).ok())
}

/// Program and arguments that start the app again about a second after this process ended.
pub(crate) fn relaunch_argv(exe: &Path, args: &[String], windows: bool) -> Vec<String> {
    let exe_s = exe.to_string_lossy().into_owned();
    if windows {
        let quoted: Vec<String> = std::iter::once(&exe_s).chain(args.iter()).map(|a| format!("\"{a}\"")).collect();
        return vec!["cmd".into(), "/C".into(), format!("ping -n 2 127.0.0.1 >NUL & start \"\" {}", quoted.join(" "))];
    }
    // Packaged macOS app: reopen the bundle (`<name>.app/Contents/MacOS/<exe>`).
    if let Some(i) = exe_s.find(".app/Contents/MacOS/") {
        let bundle = format!("{}.app", &exe_s[..i]);
        return vec!["sh".into(), "-c".into(), "sleep 1; exec open -n \"$0\"".into(), bundle];
    }
    let mut v = vec!["sh".into(), "-c".into(), "sleep 1; exec \"$0\" \"$@\"".into(), exe_s];
    v.extend(args.iter().cloned());
    v
}

// ------------------------------------------------------------------ hooks

/// What `signet_features::backup` needs from the running app.
struct AppHooks {
    data_dir: PathBuf,
    extensions: Vec<(String, String)>,
    /// Created on first use: it reads (or creates) the vault key in the OS keychain.
    cipher: OnceLock<Arc<KeychainCipher>>,
}

impl AppHooks {
    fn new(core: &Core) -> Self {
        AppHooks { data_dir: core.data_dir.clone(), extensions: installed_extensions(core), cipher: OnceLock::new() }
    }

    fn cipher(&self) -> Arc<KeychainCipher> {
        self.cipher.get_or_init(|| Arc::new(KeychainCipher::new(&KeyringStore))).clone()
    }
}

/// Installed Chrome extensions as `(id, name)`.
fn installed_extensions(core: &Core) -> Vec<(String, String)> {
    core.installed_extension_list()
}

impl Hooks for AppHooks {
    // The caller flushes (`Core::flush_for_backup`): a hook only has `&self`.
    fn flush_all(&self) {}

    fn extensions(&self) -> Vec<(String, String)> {
        self.extensions.clone()
    }

    fn plain_logins(&self) -> Vec<Value> {
        let vault = Passwords::new(&self.data_dir, self.cipher());
        vault
            .list()
            .iter()
            .filter_map(|l| {
                let password = vault.reveal(&l.id)?;
                Some(json!({ "origin": l.origin, "username": l.username, "password": password, "createdAt": l.created_at, "updatedAt": l.updated_at }))
            })
            .collect()
    }

    fn encrypt_password(&self, plain: &str) -> Option<String> {
        self.cipher().encrypt(plain).map(|b| base64(&b))
    }

    fn encryption_available(&self) -> bool {
        self.cipher().available()
    }

    fn clean_settings(&self, incoming: Value, current: &Value) -> Value {
        clean_settings_value(incoming, current)
    }

    fn clean_theme(&self, theme: &Value, name: &str, icon: &str, hue: i64) -> Option<Value> {
        clean_theme_value(theme, name, icon, hue)
    }

    fn app_version(&self) -> String {
        env!("CARGO_PKG_VERSION").to_string()
    }
}

// ------------------------------------------------------------------ API

impl Core {
    fn backup_paths(&self) -> Paths {
        Paths::new(&self.data_dir)
    }

    /// Writes the in-memory stores to disk, to read up-to-date files (`flushAll`). The vault, mods and notes write
    /// each change right away.
    fn flush_for_backup(&mut self) {
        self.flush();
    }

    fn backup_summary(&mut self, file: Value) -> BackupSummary {
        let token = new_id();
        let summary = BackupSummary {
            token: token.clone(),
            created_at: file.get("createdAt").and_then(Value::as_f64).map(|n| n as i64).unwrap_or(0),
            app_version: file.get("appVersion").and_then(Value::as_str).unwrap_or("").chars().take(40).collect(),
            counts: typed_counts(&fb::counts(&file)),
            system_sealed: file.get("keychain").is_some_and(Value::is_array).then_some(true),
        };
        self.phase3.backup.pending = Some((token, file));
        summary
    }

    pub fn get_backup_info(&mut self) -> BackupInfo {
        self.flush_for_backup();
        let paths = self.backup_paths();
        let hooks = AppHooks::new(self);
        let counts = fb::info(&paths, &hooks).map(|m| typed_counts(&m)).unwrap_or_default();
        let safety = fb::safety_list(&paths).into_iter().map(|(name, created_at)| SafetyCopy { name, created_at }).collect();
        BackupInfo { counts, safety }
    }

    /// Exports the ticked parts. Returns an error message, or `None` (done or cancelled).
    pub fn export_backup(&mut self, sections: &[BackupSection], password: Option<&str>) -> Option<String> {
        let names: Vec<String> = sections.iter().map(|s| section_name(*s)).collect();
        let t = signet_core::i18n::t;
        if names.is_empty() {
            return Some(t("backup.error.nothing"));
        }
        if names.iter().any(|s| s == "passwords") {
            if password.is_none_or(|p| p.chars().count() < 6) {
                return Some(t("backup.error.passwordShort"));
            }
            let gate = self.phase3.backup.gate.clone();
            let fallback = |reason: &str| dialog::confirm(Level::Info, reason, "", &t("dialog.ok"), &t("dialog.cancel"), false);
            if !gate.authenticate(&t("backup.touchIdExport"), &fallback) {
                return Some(t("backup.error.auth"));
            }
        }
        let documents = dirs::document_dir().or_else(dirs::home_dir).unwrap_or_else(std::env::temp_dir);
        let proposed = documents.join(default_file_name(&t("backup.fileName"), signet_core::ids::now_ms()));
        let dest = dialog::save_file(&t("backup.exportTitle"), &proposed, &t("backup.fileType"), EXT)?;
        self.flush_for_backup();
        let paths = self.backup_paths();
        let hooks = AppHooks::new(self);
        fb::export_to(&paths, &dest, &names, password, &hooks).map(t)
    }

    /// Picks a file and reads it: the summary of its parts, an error, or cancelled.
    pub fn open_backup(&mut self) -> BackupOpen {
        let t = signet_core::i18n::t;
        let Some(path) = dialog::open_file(&t("backup.fileType"), &t("backup.fileType"), EXT) else { return BackupOpen::Cancelled };
        match fb::parse_file(&path) {
            Some(file) => BackupOpen::Summary { summary: self.backup_summary(file) },
            None => BackupOpen::Error { message: t("backup.error.invalid") },
        }
    }

    /// Safety copy chosen from the list (by its file name).
    pub fn open_safety_backup(&mut self, name: &str) -> Result<BackupSummary, String> {
        let paths = self.backup_paths();
        match fb::open_safety(&paths, name) {
            Some(file) => Ok(self.backup_summary(file)),
            None => Err(signet_core::i18n::t("backup.error.invalid")),
        }
    }

    /// Imports the ticked parts of the backup read (`token`), then restarts the app. Returns an error message
    /// (wrong password...), or `None` if the user gave up.
    pub fn apply_backup(&mut self, token: &str, sections: &[BackupSection], mode: BackupMode, password: Option<&str>) -> Option<String> {
        let t = signet_core::i18n::t;
        let file = match &self.phase3.backup.pending {
            Some((tok, file)) if tok == token => file.clone(),
            _ => return Some(t("backup.error.invalid")),
        };
        let names: Vec<String> = sections.iter().map(|s| section_name(*s)).collect();
        let hooks = AppHooks::new(self);
        let prepared = match fb::prepare(&file, &names, password, &hooks) {
            Ok(p) => p,
            Err(key) => return Some(t(key)),
        };
        let replace = mode == BackupMode::Replace;
        let ok = dialog::confirm(
            if replace { Level::Warning } else { Level::Info },
            &t(if replace { "backup.confirm.replace" } else { "backup.confirm.add" }),
            &t("backup.confirm.detail"),
            &t("backup.confirm.ok"),
            &t("dialog.cancel"),
            // Replacing erases data: the default button is "Cancel".
            replace,
        );
        if !ok {
            return None;
        }
        // Safety copy of the current state, import on the files, then restart.
        self.flush_for_backup();
        let paths = self.backup_paths();
        let fb_mode = if replace { Mode::Replace } else { Mode::Add };
        if let Some(key) = fb::import(&paths, &file, &prepared, fb_mode, &hooks, &translate) {
            return Some(t(key));
        }
        self.phase3.backup.pending = None;
        self.restart_after_restore()
    }

    /// Starts the app again and ends this process at once: a shutdown flush would write the old state over the
    /// restored files (Electron used `app.exit(0)` for the same reason).
    fn restart_after_restore(&mut self) -> ! {
        self.quitting = true;
        self.views.destroy_all();
        crate::app::single_instance::release(&self.data_dir);
        let argv = std::env::current_exe().map(|exe| relaunch_argv(&exe, &std::env::args().skip(1).collect::<Vec<_>>(), cfg!(windows)));
        if let Ok(argv) = argv
            && let Some((program, args)) = argv.split_first()
        {
            let _ = std::process::Command::new(program).args(args).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn();
        }
        std::process::exit(0)
    }

    pub fn reveal_safety_backups(&mut self) {
        let dir = self.backup_paths().safety();
        let _ = std::fs::create_dir_all(&dir);
        crate::app::platform::open_external(&dir.to_string_lossy());
    }

    /// Extension ids left by a restore (Chrome Web Store ids), handed out once: the extensions module installs
    /// the ones that are missing.
    pub fn take_backup_extensions(&mut self) -> Vec<String> {
        std::mem::take(&mut self.phase3.backup.pending_extensions)
    }
}

/// Called once after the browser core exists and the web layer is initialized.
pub(super) fn startup(core: &mut Core) {
    let paths = core.backup_paths();
    core.phase3.backup.pending_extensions = fb::take_pending_extensions(&paths);
}

/// Called for every CEF event, before the core handles it.
pub(super) fn web_event(_core: &mut Core, _ev: &signet_web::WebEvent) {}

/// Called on every core tick (about once per second or at the next store deadline).
pub(super) fn tick(_core: &mut Core) {}

/// Called before quitting (flush files, stop threads).
pub(super) fn quit(_core: &mut Core) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_name_uses_the_date() {
        assert_eq!(default_file_name("Signet backup", 0), "Signet backup 1970-01-01.signetbackup");
        // 2026-11-06T12:00:00Z
        assert_eq!(default_file_name("B", 1_793_966_400_000), "B 2026-11-06.signetbackup");
        // Leap day
        assert_eq!(default_file_name("B", 1_709_164_800_000), "B 2024-02-29.signetbackup");
    }

    #[test]
    fn base64_matches_the_standard() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn counts_follow_settings_order() {
        let mut m = serde_json::Map::new();
        m.insert("passwords".into(), json!(-1));
        m.insert("settings".into(), json!(1));
        m.insert("bookmarks".into(), json!(4));
        m.insert("unknown".into(), json!(9));
        let c = typed_counts(&m);
        let keys: Vec<BackupSection> = c.keys().copied().collect();
        assert_eq!(keys, vec![BackupSection::Settings, BackupSection::Bookmarks, BackupSection::Passwords]);
        assert_eq!(c[&BackupSection::Passwords], -1);
    }

    #[test]
    fn section_names_are_the_file_keys() {
        for s in signet_core::types::BACKUP_SECTIONS {
            assert!(fb::BACKUP_SECTIONS.contains(&section_name(s).as_str()));
        }
    }

    #[test]
    fn settings_are_cleaned_and_unknown_keys_kept() {
        let incoming = json!({ "tabSleepMinutes": 15, "fontSize": "huge", "archiveAfterHours": -4, "someFutureKey": true });
        let current = json!({ "archiveAfterHours": 24 });
        let out = clean_settings_value(incoming, &current);
        assert_eq!(out["someFutureKey"], json!(true));
        assert_eq!(out["tabSleepMinutes"], json!(15));
        assert_eq!(out["archiveAfterHours"].as_f64(), Some(24.0));
        assert!(out.get("fontSize").is_none());
    }

    #[test]
    fn theme_needs_two_hues() {
        assert!(clean_theme_value(&json!({ "hues": [10], "grain": 0.2 }), "n", "i", 10).is_none());
        let t = clean_theme_value(&json!({ "hues": [10, 200, 300, 40], "grain": 3 }), "n", "i", 10).unwrap();
        assert_eq!(t["hues"].as_array().map(Vec::len), Some(3));
        assert_eq!(t["grain"].as_f64(), Some(1.0));
    }

    #[test]
    fn relaunch_commands() {
        let unix = relaunch_argv(Path::new("/usr/bin/signet"), &["a".into()], false);
        assert_eq!(unix, ["sh", "-c", "sleep 1; exec \"$0\" \"$@\"", "/usr/bin/signet", "a"]);
        let mac = relaunch_argv(Path::new("/Applications/Signet GPUI.app/Contents/MacOS/signet"), &[], false);
        assert_eq!(mac[3], "/Applications/Signet GPUI.app");
        assert!(mac[2].contains("open -n"));
        let win = relaunch_argv(Path::new("C:\\Signet\\signet.exe"), &[], true);
        assert_eq!(win[0], "cmd");
        assert!(win[2].contains("start \"\" \"C:\\Signet\\signet.exe\""));
    }
}

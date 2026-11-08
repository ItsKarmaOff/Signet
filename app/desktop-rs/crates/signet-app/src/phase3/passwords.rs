//! PHASE3: passwords. Port of `main/features/passwords/passwords.ts` (main-process side), `main/app/osauth.ts`,
//! `main/ipc/passwords.ts` and the passwords parts of `main/browser/overlay.ts`, on top of
//! `signet_features::{passwords, osauth}`.
//!
//! - The vault (`Passwords`) is built on first use, with an AES-256-GCM key kept in the OS keychain (`LazyCipher`, so
//!   the keychain is not touched before something needs it).
//! - Work that must wait for the user (system authentication, dialogs) runs on a worker thread when a page waits for
//!   the answer; its completion is queued in `Jobs` and runs on the main thread at the next CEF event or tick.
//!   The Settings API is synchronous (`SignetApi`), so reveal, copy, export, import and card edits block until the
//!   user answers.

mod logic;
mod native;

pub(super) use logic::{enum_name, NEXT_OFFER_MS};

use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

use serde_json::{json, Value};
use signet_core::api::PasswordAnswer;
use signet_core::ids::now_ms;
use signet_core::i18n::{t, t_args};
use signet_core::types::{AutofillKind as UiKind, LoginRef, OverlayPayload, PasswordImportResult, PasswordManager, PasswordsInfo, Rect};
use signet_features::autofill::CardInput as CardNumberInput;
use signet_features::importers::chromium::{decrypt as chromium_decrypt, KeyNames, KeyProvider, Keys, SystemKeys};
use signet_features::osauth::{AuthGate, SystemAuth};
use signet_features::passwords::{self as pw, import_electron_vault, Answer, Cipher, KeyAction, KeyState, KeychainCipher, Passwords, PromptSettings, ELECTRON_KEYS};
use signet_features::secrets::KeyringStore;
use signet_scripts::messages::{Account, Credentials};
use signet_scripts::{Accepted, Reply};

use crate::browser::Core;

// --------------------------------------------------------------------------------------------- shared plumbing

/// Work finished on a worker thread, to run on the main thread with the core.
pub(super) type Job = Box<dyn FnOnce(&mut Core) + Send>;

/// Queue between worker threads and the main thread (drained at every CEF event and tick).
#[derive(Clone, Default)]
pub(super) struct Jobs(Arc<Mutex<Vec<Job>>>);

impl Jobs {
    pub(super) fn push(&self, job: Job) {
        if let Ok(mut q) = self.0.lock() {
            q.push(job);
        }
    }

    fn take(&self) -> Vec<Job> {
        self.0.lock().map(|mut q| std::mem::take(&mut *q)).unwrap_or_default()
    }
}

/// Keychain cipher opened on first use: the keychain (and its possible prompt) is not touched at startup.
#[derive(Default)]
pub(super) struct LazyCipher(OnceLock<KeychainCipher>);

impl LazyCipher {
    fn get(&self) -> &KeychainCipher {
        self.0.get_or_init(|| KeychainCipher::new(&KeyringStore))
    }
}

impl Cipher for LazyCipher {
    fn available(&self) -> bool {
        self.get().available()
    }
    fn encrypt(&self, plain: &str) -> Option<Vec<u8>> {
        self.get().encrypt(plain)
    }
    fn decrypt(&self, blob: &[u8]) -> Option<String> {
        self.get().decrypt(blob)
    }
}

/// System authentication with the one-minute grace; where the system cannot ask, a plain confirmation.
/// Blocks until the user answers: call it from a worker thread when something waits for it.
pub(super) fn authenticate(gate: &AuthGate, reason: &str) -> bool {
    gate.authenticate(reason, &|r| native::confirm(&t("passwords.confirm.title"), r, &t("passwords.confirm.allow"), &t("passwords.confirm.cancel")))
}

/// Prompt card shown in the overlay (a password or an autofill offer).
pub(super) struct Shown {
    pub token: String,
    pub tab: String,
    pub at: i64,
    /// Opened by the key button (not after a login): stays until answered.
    pub anchored: bool,
}

/// State owned by this feature (a field of `Phase3`).
pub struct State {
    pub(super) cipher: Arc<LazyCipher>,
    pub(super) gate: Arc<AuthGate>,
    pub(super) jobs: Jobs,
    vault: Option<Passwords>,
    pub(super) shown: Option<Shown>,
    /// A system authentication is running for a page request.
    pub(super) auth_busy: bool,
    last_manager: Option<String>,
    keys_synced: bool,
}

impl Default for State {
    fn default() -> Self {
        State {
            cipher: Arc::new(LazyCipher::default()),
            gate: Arc::new(AuthGate::new(Box::new(SystemAuth))),
            jobs: Jobs::default(),
            vault: None,
            shown: None,
            auth_busy: false,
            last_manager: None,
            keys_synced: false,
        }
    }
}

impl Core {
    pub(super) fn pw_vault(&mut self) -> &mut Passwords {
        let cipher = self.phase3.passwords.cipher.clone();
        let dir = self.data_dir.clone();
        self.phase3.passwords.vault.get_or_insert_with(|| Passwords::new(&dir, cipher))
    }

    /// The password manager chosen in Settings (`signet` by default).
    pub(super) fn manager_id(&self) -> String {
        logic::manager_name(self.state().settings.password_manager)
    }

    pub(super) fn open_tab_ids(&self) -> HashSet<String> {
        self.state().tabs.keys().cloned().collect()
    }

    // ----------------------------------------------------------------------------------- overlay prompt

    /// Opens the prompt card (save or update a login, or an autofill offer) over the tab's card. False when the
    /// overlay is busy or the tab is not displayed: the prompt then waits in the vault (key button).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn open_prompt(&mut self, tab: &str, token: &str, origin: &str, username: &str, update: bool, kind: Option<UiKind>, anchor: Option<Rect>) -> bool {
        if self.win.is_none() || self.ov.payload.is_some() || self.ov.leaving {
            return false;
        }
        let Some(card) = self.panes.iter().find(|p| p.tab_id == tab).map(|p| p.card) else { return false };
        let anchored = anchor.is_some();
        self.show_prompt_payload(OverlayPayload::Password { token: token.into(), origin: origin.into(), username: username.into(), update, kind, card, anchor });
        if anchored {
            self.request_focus();
        }
        self.phase3.passwords.shown = Some(Shown { token: token.into(), tab: tab.into(), at: now_ms(), anchored });
        true
    }

    /// Capture harness (`SIGNET_PW_PROMPT=1`): shows the "save password" card on the active tab.
    pub fn debug_password_prompt(&mut self, update: bool) {
        let Some(tab) = self.active_tab_id() else { return };
        self.open_prompt(&tab, "debug", "https://www.wikipedia.org", "christophe@example.com", update, None, None);
    }

    /// Same as the private `show_overlay` of `browser/overlay.rs`.
    fn show_prompt_payload(&mut self, payload: OverlayPayload) {
        self.ov.payload = Some(payload);
        self.ov.leaving = false;
        self.changed_view();
        self.emit_overlay();
    }

    /// Puts the prompt card away (answered, forgotten or navigated away).
    pub(super) fn close_prompt(&mut self) {
        self.phase3.passwords.shown = None;
        if matches!(self.ov.payload, Some(OverlayPayload::Password { .. } | OverlayPayload::Passwords { .. })) {
            self.close_overlay(false);
        }
    }

    // ----------------------------------------------------------------------------------- key button

    /// Updates `passwordKey` of a tab: pending prompt, accounts saved for its site, or nothing.
    fn refresh_key_with(&mut self, tab: &str, open: &HashSet<String>) {
        let Some(url) = self.state().tabs.get(tab).map(|t| t.url.clone()) else { return };
        let manager = self.manager_id();
        let state = self.pw_vault().key_state(tab, &url, &manager, false, &|id| open.contains(id));
        self.set_key(tab, state);
    }

    fn set_key(&mut self, tab: &str, state: Option<KeyState>) {
        let value = match state {
            Some(KeyState::Pending) => json!("pending"),
            Some(KeyState::Saved) => json!("saved"),
            None => Value::Null,
        };
        self.patch_one(tab, "passwordKey", value, false, false);
    }

    pub(super) fn refresh_key(&mut self, tab: &str) {
        let open = self.open_tab_ids();
        self.refresh_key_with(tab, &open);
    }

    /// All tabs (accounts added or removed, manager changed).
    pub(super) fn refresh_all_keys(&mut self) {
        let open = self.open_tab_ids();
        for id in open.clone() {
            self.refresh_key_with(&id, &open);
        }
    }

    /// Runs what worker threads queued.
    pub(super) fn run_jobs(&mut self) {
        for job in self.phase3.passwords.jobs.take() {
            job(self);
        }
    }
}

// --------------------------------------------------------------------------------------------- Settings API

impl Core {
    pub fn list_passwords(&mut self) -> PasswordsInfo {
        let v = self.pw_vault();
        PasswordsInfo { logins: v.list(), encryption: v.encryption(), touch_id: cfg!(target_os = "macos") }
    }

    pub fn reveal_password(&mut self, id: &str) -> Option<String> {
        let origin = self.pw_vault().list().into_iter().find(|l| l.id == id)?.origin;
        let gate = self.phase3.passwords.gate.clone();
        if !authenticate(&gate, &t_args("passwords.touchIdReveal", &[("site", &origin)])) {
            return None;
        }
        self.pw_vault().reveal(id)
    }

    pub fn copy_password(&mut self, id: &str) -> bool {
        let Some(password) = self.reveal_password(id) else { return false };
        crate::app::platform::copy_text(&password);
        // The clipboard is cleared after 90 s if it still holds the password.
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(pw::CLIPBOARD_TTL_SECS));
            if native::read_clipboard().is_some_and(|c| logic::clipboard_holds(&c, &password)) {
                crate::app::platform::copy_text("");
            }
        });
        true
    }

    pub fn save_password(&mut self, id: Option<&str>, origin: &str, username: &str, password: Option<&str>) -> Option<String> {
        let err = self.pw_vault().upsert(id, origin, username, password);
        if err.is_none() {
            self.refresh_all_keys();
        }
        err.map(t)
    }

    pub fn delete_password(&mut self, id: &str) {
        self.pw_vault().remove(id);
        self.refresh_all_keys();
    }

    pub fn import_passwords(&mut self) -> Option<PasswordImportResult> {
        if !self.pw_vault().encryption() {
            return Some(PasswordImportResult { error: Some(logic::import_error("encryption")), ..Default::default() });
        }
        let path = native::choose_csv(&t("settings.passwords.import"))?;
        let Ok(text) = fs::read_to_string(&path) else {
            return Some(PasswordImportResult { error: Some(logic::import_error("read")), ..Default::default() });
        };
        let res = self.pw_vault().import_csv(&text);
        if res.added > 0 {
            self.refresh_all_keys();
        }
        Some(PasswordImportResult { added: res.added as u32, existing: res.existing as u32, error: res.error.map(logic::import_error) })
    }

    pub fn export_passwords(&mut self) -> bool {
        let gate = self.phase3.passwords.gate.clone();
        if !authenticate(&gate, &t("passwords.touchIdExport")) {
            return false;
        }
        let default = dirs::document_dir().or_else(dirs::home_dir).unwrap_or_default().join(t("passwords.exportFile"));
        let Some(path) = native::save_csv(&t("passwords.touchIdExport"), &default) else { return false };
        write_private(&path, self.pw_vault().export_csv().as_bytes()).is_ok()
    }

    /// Installs the store extension of a password manager (Bitwarden...), unless already there.
    pub fn install_password_extension(&mut self, manager: PasswordManager) {
        let Some(entry) = signet_core::types::PASSWORD_EXTENSIONS.iter().find(|e| e.id == manager) else { return };
        self.install_store_extension(entry.extension_id, entry.name);
    }

    pub fn answer_password_prompt(&mut self, token: &str, answer: PasswordAnswer) {
        // The same prompt serves the autofill offers: the token tells which module owns it.
        if self.autofill_answer(token, answer) {
            return;
        }
        let a = match answer {
            PasswordAnswer::Save => Answer::Save,
            PasswordAnswer::Never => Answer::Never,
            PasswordAnswer::Dismiss => Answer::Dismiss,
        };
        let eff = self.pw_vault().answer(token, a);
        if let Some(origin) = eff.never_origin {
            let s = &mut self.store.data.state.settings;
            s.never_save_passwords = Some(logic::with_origin(s.never_save_passwords.take(), &origin));
            self.changed();
        }
        if eff.saved {
            self.refresh_all_keys();
        } else if let Some(tab) = eff.tab_id {
            self.refresh_key(&tab);
        }
        if self.phase3.passwords.shown.as_ref().is_some_and(|s| s.token == token) {
            self.close_prompt();
        }
    }

    /// Click on the key button: reopens the pending prompt, otherwise the site's accounts, under `anchor`.
    pub fn open_password_key(&mut self, tab_id: &str, anchor: Rect) {
        let Some(url) = self.state().tabs.get(tab_id).map(|t| t.url.clone()) else { return };
        // A prompt card of ours that is already up gives way to the one the user asks for.
        if self.phase3.passwords.shown.is_some() {
            self.close_prompt();
        }
        let manager = self.manager_id();
        let open = self.open_tab_ids();
        match self.pw_vault().open_key(tab_id, &url, &manager, false, &|id| open.contains(id)) {
            KeyAction::None => {}
            KeyAction::Prompt(req) => {
                self.open_prompt(tab_id, &req.token, &req.origin, &req.username, req.update, None, Some(anchor));
            }
            KeyAction::List { origin, logins } => {
                if self.win.is_none() || self.ov.payload.is_some() || self.ov.leaving || !self.panes.iter().any(|p| p.tab_id == tab_id) {
                    return;
                }
                let logins = logins.into_iter().map(|(id, username)| LoginRef { id, username }).collect();
                self.show_prompt_payload(OverlayPayload::Passwords { origin, logins, anchor });
                self.request_focus();
            }
        }
    }
}

/// Writes a file readable by its owner only (the CSV holds every password in clear).
fn write_private(path: &Path, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(path)?;
    f.write_all(data)
}

// --------------------------------------------------------------------------------------------- Electron vault

/// Key provider that hands out keys read once (the OS prompts only once for passwords and cards).
struct Fixed(Keys);

impl KeyProvider for Fixed {
    fn keys(&self, _profile: &Path, _names: &KeyNames) -> Keys {
        self.0.clone()
    }
}

/// First run after the Electron files were copied into the data folder: their logins and cards are encrypted with
/// Chromium's key, which this app cannot use. They are decrypted once (the OS asks the user for the keychain) and
/// stored again with this app's key. The copies are kept next to the originals (`*.electron-backup.json`).
fn migrate_electron(core: &mut Core) {
    let data = core.data_dir.clone();
    // Tests: no keychain access, and no "imported" marker either.
    if signet_features::secrets::no_keychain() {
        return;
    }
    let marker = data.join("electron-vault-imported");
    if marker.exists() {
        return;
    }
    let (pw_file, af_file) = (data.join("passwords.json"), data.join("autofill.json"));
    let foreign_logins = logic::foreign_login_ids(&fs::read_to_string(&pw_file).unwrap_or_default());
    let foreign_cards = logic::foreign_cards(&fs::read_to_string(&af_file).unwrap_or_default());
    if foreign_logins.is_empty() && foreign_cards.is_empty() {
        let _ = fs::write(&marker, "");
        return;
    }
    let Some(electron_dir) = dirs::config_dir().map(|d| d.join("Signet")) else { return };
    let keys = SystemKeys.keys(&electron_dir.join("Default"), &ELECTRON_KEYS);
    if keys.v10.is_none() && keys.v11.is_none() && keys.gcm.is_none() {
        core.log("passwords: Electron key not available, import postponed to the next launch");
        return;
    }
    let mut imported = (0usize, 0usize);
    if !foreign_logins.is_empty() {
        let backup = data.join("passwords.electron-backup.json");
        if !backup.exists() {
            let _ = fs::copy(&pw_file, &backup);
        }
        let logins = import_electron_vault(&pw_file, &electron_dir, &Fixed(keys.clone()));
        let vault = core.pw_vault();
        if !logins.is_empty() && vault.encryption() {
            for id in &foreign_logins {
                vault.remove(id);
            }
            imported.0 = vault.add_many(&logins).0;
        }
    }
    if !foreign_cards.is_empty() {
        let backup = data.join("autofill.electron-backup.json");
        if !backup.exists() {
            let _ = fs::copy(&af_file, &backup);
        }
        let vault = core.af_vault();
        for c in &foreign_cards {
            let Some(number) = chromium_decrypt(&c.blob, &keys, cfg!(target_os = "windows")) else { continue };
            let input = CardNumberInput { holder: c.holder.clone(), number: Some(number), exp_month: c.exp_month, exp_year: c.exp_year, nickname: c.nickname.clone() };
            if vault.upsert_card(None, &input, &mut || true).is_none() {
                vault.remove_card(&c.id);
                imported.1 += 1;
            }
        }
    }
    core.log(format!("passwords: imported {} logins and {} cards from the Electron app", imported.0, imported.1));
    let _ = fs::write(&marker, "");
}

// --------------------------------------------------------------------------------------------- hooks

/// Called once after the browser core exists and the web layer is initialized.
pub(super) fn startup(core: &mut Core) {
    migrate_electron(core);
}

/// Called for every CEF event, before the core handles it.
pub(super) fn web_event(core: &mut Core, ev: &signet_web::WebEvent) {
    core.run_jobs();
    // Navigation of a tab: a prompt about another site is forgotten, the key button follows the new page.
    if let signet_web::WebEvent::AddressChanged { url, is_main_frame: true, .. } = ev
        && let Some(tab) = core.views.tab_of(ev.view()).map(str::to_owned)
    {
        let gone = core.pw_vault().on_navigate(&tab, url);
        let af_gone = core.autofill_navigated(&tab, url);
        let shown_gone = core.phase3.passwords.shown.as_ref().is_some_and(|s| gone.contains(&s.token) || af_gone.contains(&s.token));
        if shown_gone {
            core.close_prompt();
        }
        let manager = core.manager_id();
        let open = core.open_tab_ids();
        let state = core.pw_vault().key_state(&tab, url, &manager, false, &|id| open.contains(id));
        core.set_key(&tab, state);
    }
}

/// Called on every core tick (about once per second or at the next store deadline).
pub(super) fn tick(core: &mut Core) {
    core.run_jobs();
    // Manager changed in Settings (or first tick): every key button follows.
    let manager = core.manager_id();
    if !core.phase3.passwords.keys_synced || core.phase3.passwords.last_manager.as_deref() != Some(manager.as_str()) {
        core.phase3.passwords.keys_synced = true;
        core.phase3.passwords.last_manager = Some(manager);
        core.refresh_all_keys();
    }
    let now = now_ms();
    let mut dismiss = None;
    if let Some(s) = &core.phase3.passwords.shown {
        if !matches!(core.ov.payload, Some(OverlayPayload::Password { .. } | OverlayPayload::Passwords { .. })) {
            // The overlay went away by itself (Escape, another overlay).
            core.phase3.passwords.shown = None;
        } else if logic::prompt_expired(s.anchored, s.at, now) {
            dismiss = Some(s.token.clone());
        }
    }
    // The prompt tucks itself away after a while: same as "Not now".
    if let Some(token) = dismiss {
        core.answer_password_prompt(&token, PasswordAnswer::Dismiss);
    }
}

/// Called before quitting (flush files, stop threads).
pub(super) fn quit(core: &mut Core) {
    if let Some(v) = &core.phase3.passwords.vault {
        v.flush();
    }
}

// --------------------------------------------------------------------------------------------- page channels

impl Core {
    /// Page channels `pw:accounts`, `pw:fill`, `pw:submit`, `page:submit` (accepted by `signet_scripts::validate`). Answer requests with `self.page_reply(ctx, ..)`.
    pub fn page_passwords(&mut self, ctx: &super::pages::PageCtx, accepted: signet_scripts::Accepted) {
        let manager = self.manager_id();
        match accepted {
            Accepted::PwAccounts { origin } => {
                let list: Vec<Account> = self.pw_vault().accounts(&origin, &manager).into_iter().map(|(id, username)| Account { id, username }).collect();
                self.reply_with(ctx, &list);
            }
            Accepted::PwFill { origin, id } => {
                let fill = self.pw_vault().fill(&origin, &id, &manager, ctx.sender.is_private).map(|(username, password)| Credentials { username, password });
                self.reply_with(ctx, &fill);
            }
            Accepted::PwSubmit { origin, username, password } => self.pw_submitted(ctx, &origin, &username, &password, &manager),
            // Form submission seen by the swipe script: it only matters to Peek, which owns it.
            Accepted::PageSubmit => {}
            _ => {}
        }
    }

    fn reply_with<T: serde::Serialize>(&mut self, ctx: &super::pages::PageCtx, data: &T) {
        if let Some(rid) = ctx.rid.as_deref() {
            self.page_reply(ctx, Reply::ok(rid, data));
        }
    }

    fn pw_submitted(&mut self, ctx: &super::pages::PageCtx, origin: &str, username: &str, password: &str, manager: &str) {
        let s = &self.state().settings;
        let settings = PromptSettings { offer_save: s.offer_save_passwords != Some(false), never_save: s.never_save_passwords.clone().unwrap_or_default() };
        let open = self.open_tab_ids();
        let tab = ctx.tab.clone();
        let req = self.pw_vault().submitted(&tab, origin, username, password, ctx.sender.is_private, manager, &settings, &|id| open.contains(id));
        let Some(req) = req else { return };
        self.refresh_key_with(&tab, &open);
        // Background tab or busy overlay: the prompt waits, available from the key button.
        self.open_prompt(&tab, &req.token, &req.origin, &req.username, req.update, None, None);
    }
}

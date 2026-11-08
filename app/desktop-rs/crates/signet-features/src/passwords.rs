//! Signet password vault (port of `main/features/passwords/passwords.ts`): encrypted logins, CSV import and export,
//! prompts after a login, key button state. UI, dialogs and page IPC belong to the app.
//!
//! Encryption: the Rust app uses AES-256-GCM with a random key kept in the OS keychain (service
//! "Signet GPUI Safe Storage"). `passwords.json` keeps the Electron format (`{ version: 1, logins: [...] }`,
//! passwords base64). Entries of the Electron vault (Chromium os_crypt `v10`/`v11`) can be read with
//! `import_electron_vault`; the user's keychain prompts at that moment.
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use rand::RngCore;
use serde::{Deserialize, Serialize};

use crate::importers::chromium::{decrypt as chromium_decrypt, KeyNames, KeyProvider, SystemKeys};
use crate::secrets::SecretStore;
use crate::util::{clip, now_ms, write_atomic};

pub const KEY_SERVICE: &str = "Signet GPUI Safe Storage";
pub const KEY_ACCOUNT: &str = "vault-key";
pub const MAX_FIELD: usize = 1000;
/// A prompt with no answer (or postponed) stays available from the key button for 30 minutes.
pub const PENDING_TTL_MS: i64 = 30 * 60_000;
/// Username entered alone (two-step login): kept 5 minutes for the password page.
pub const IDENTIFIER_TTL_MS: i64 = 5 * 60_000;
/// A copied password is removed from the clipboard after 90 s (if still there).
pub const CLIPBOARD_TTL_SECS: u64 = 90;
const PROMPT_RATE_MS: i64 = 3000;

/// Managers: `signet`, `off`, or an extension id.
pub const PASSWORD_EXTENSIONS: &[(&str, &str, &str)] = &[
    ("bitwarden", "Bitwarden", "nngceckbapebfimnlniiiahkandclblb"),
    ("1password", "1Password", "aeblfdkhhhdcdjpifhhbdiojplfjncoa"),
    ("proton", "Proton Pass", "ghmbeldphafepmbegfdlkpapadhbakde"),
    ("dashlane", "Dashlane", "fdjamakpfbbddfjaooikfcpapjohcfmg"),
    ("lastpass", "LastPass", "hdokiejnpimakedhajhdlcegeplioahd"),
];

pub fn is_manager(v: &str) -> bool {
    v == "signet" || v == "off" || PASSWORD_EXTENSIONS.iter().any(|(id, _, _)| *id == v)
}

/// http(s) origin of an input (`example.com`, `https://example.com/login`...), or `None`.
pub fn normalize_origin(input: &str) -> Option<String> {
    let text = input.trim();
    if text.is_empty() {
        return None;
    }
    let has_scheme = text.find("://").is_some_and(|i| {
        let s = &text[..i];
        s.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) && s.chars().all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c))
    });
    let u = url::Url::parse(&if has_scheme { text.to_string() } else { format!("https://{text}") }).ok()?;
    ((u.scheme() == "https" || u.scheme() == "http") && u.host_str().is_some_and(|h| !h.is_empty())).then(|| u.origin().ascii_serialization())
}

/// Reads a CSV (RFC 4180: quotes, doubled quotes, line breaks inside a field).
pub fn parse_csv(text: &str) -> Vec<Vec<String>> {
    let src: Vec<char> = text.trim_start_matches('\u{feff}').chars().collect();
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut i = 0;
    while i < src.len() {
        let c = src[i];
        if quoted {
            if c == '"' && src.get(i + 1) == Some(&'"') {
                field.push('"');
                i += 1;
            } else if c == '"' {
                quoted = false;
            } else {
                field.push(c);
            }
        } else if c == '"' {
            quoted = true;
        } else if c == ',' {
            row.push(std::mem::take(&mut field));
        } else if c == '\n' || c == '\r' {
            if c == '\r' && src.get(i + 1) == Some(&'\n') {
                i += 1;
            }
            row.push(std::mem::take(&mut field));
            rows.push(std::mem::take(&mut row));
        } else {
            field.push(c);
        }
        i += 1;
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows.into_iter().filter(|r| r.iter().any(|f| !f.trim().is_empty())).collect()
}

fn csv_field(s: &str) -> String {
    if s.contains(['"', ',', '\r', '\n']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_string() }
}

const URL_COLUMNS: &[&str] = &["url", "login_uri", "website", "origin", "login url", "web site", "hostname"];
const USER_COLUMNS: &[&str] = &["username", "login_username", "login", "user", "user name", "email"];
const PASSWORD_COLUMNS: &[&str] = &["password", "login_password"];

/// Encryption of one password.
pub trait Cipher: Send + Sync {
    fn available(&self) -> bool;
    fn encrypt(&self, plain: &str) -> Option<Vec<u8>>;
    fn decrypt(&self, blob: &[u8]) -> Option<String>;
}

const GCM_PREFIX: &[u8] = b"sg1";

/// AES-256-GCM with a random key stored in the OS keychain.
pub struct KeychainCipher {
    key: Option<[u8; 32]>,
}

impl KeychainCipher {
    pub fn new(store: &dyn SecretStore) -> Self {
        Self::with_service(store, KEY_SERVICE, KEY_ACCOUNT)
    }

    pub fn with_service(store: &dyn SecretStore, service: &str, account: &str) -> Self {
        let key = match store.get(service, account) {
            Ok(Some(k)) => k.try_into().ok(),
            Ok(None) => {
                let mut k = [0u8; 32];
                rand::rngs::OsRng.fill_bytes(&mut k);
                store.set(service, account, &k).ok().map(|_| k)
            }
            Err(_) => None,
        };
        KeychainCipher { key }
    }
}

impl Cipher for KeychainCipher {
    fn available(&self) -> bool {
        self.key.is_some()
    }
    fn encrypt(&self, plain: &str) -> Option<Vec<u8>> {
        let c = Aes256Gcm::new_from_slice(&self.key?).ok()?;
        let mut nonce = [0u8; 12];
        rand::rngs::OsRng.fill_bytes(&mut nonce);
        let ct = c.encrypt(Nonce::from_slice(&nonce), plain.as_bytes()).ok()?;
        let mut out = GCM_PREFIX.to_vec();
        out.extend(nonce);
        out.extend(ct);
        Some(out)
    }
    fn decrypt(&self, blob: &[u8]) -> Option<String> {
        if blob.len() < GCM_PREFIX.len() + 12 + 16 || &blob[..3] != GCM_PREFIX {
            return None;
        }
        let c = Aes256Gcm::new_from_slice(&self.key?).ok()?;
        let plain = c.decrypt(Nonce::from_slice(&blob[3..15]), &blob[15..]).ok()?;
        String::from_utf8(plain).ok()
    }
}

/// Login as written on disk (same JSON as Electron): the password is encrypted, base64.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StoredLogin {
    pub id: String,
    pub origin: String,
    #[serde(default)]
    pub username: String,
    pub password: String,
    #[serde(default)]
    pub created_at: i64,
    #[serde(default)]
    pub updated_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<i64>,
}

pub use signet_core::types::SavedLogin;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct PasswordImportResult {
    pub added: usize,
    pub existing: usize,
    /// `format`, `read` or `encryption`.
    pub error: Option<&'static str>,
}

#[derive(Debug, Clone)]
pub struct NewLogin {
    pub origin: String,
    pub username: String,
    pub password: String,
    pub created_at: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    Save,
    Never,
    Dismiss,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyState {
    Pending,
    Saved,
}

/// Entered login awaiting an answer (memory only).
#[derive(Debug, Clone)]
struct Pending {
    tab_id: String,
    origin: String,
    username: String,
    password: String,
    existing_id: Option<String>,
    at: i64,
}

/// Prompt to show.
#[derive(Debug, Clone, PartialEq)]
pub struct PromptRequest {
    pub token: String,
    pub tab_id: String,
    pub origin: String,
    pub username: String,
    pub update: bool,
}

/// Settings the prompt rules depend on.
#[derive(Debug, Clone, Default)]
pub struct PromptSettings {
    pub offer_save: bool,
    pub never_save: Vec<String>,
}

#[derive(Debug, Default, PartialEq)]
pub struct AnswerEffect {
    /// Origin to add to `neverSavePasswords`.
    pub never_origin: Option<String>,
    pub saved: bool,
    pub tab_id: Option<String>,
}

#[derive(Deserialize)]
struct FileShape {
    #[serde(default)]
    logins: Vec<serde_json::Value>,
}

pub struct Passwords {
    file: PathBuf,
    cipher: Arc<dyn Cipher>,
    entries: Vec<StoredLogin>,
    /// Unreadable file: the existing file is not overwritten during the session.
    blocked: bool,
    last_prompt: HashMap<String, i64>,
    pending: HashMap<String, Pending>,
    identifiers: HashMap<String, (String, String, i64)>,
}

fn site_of(origin: &str) -> String {
    // Registrable-domain approximation until signet-core's `site` is wired: last two labels.
    let host = url::Url::parse(origin).ok().and_then(|u| u.host_str().map(str::to_lowercase)).unwrap_or_default();
    let parts: Vec<&str> = host.split('.').collect();
    if parts.len() <= 2 || host.parse::<std::net::IpAddr>().is_ok() { host } else { parts[parts.len() - 2..].join(".") }
}

impl Passwords {
    pub fn new(data_dir: &Path, cipher: Arc<dyn Cipher>) -> Self {
        let file = data_dir.join("passwords.json");
        let mut p = Passwords { file, cipher, entries: vec![], blocked: false, last_prompt: HashMap::new(), pending: HashMap::new(), identifiers: HashMap::new() };
        p.load();
        p
    }

    fn load(&mut self) {
        match fs::read_to_string(&self.file) {
            Ok(raw) => match serde_json::from_str::<FileShape>(&raw) {
                Ok(f) => self.entries = f.logins.into_iter().filter_map(|v| serde_json::from_value::<StoredLogin>(v).ok()).collect(),
                Err(_) => {
                    // Corrupted: keep a separate copy before the next save overwrites it.
                    if fs::copy(&self.file, format!("{}.corrupt-{}", self.file.display(), now_ms())).is_err() {
                        self.blocked = true;
                    }
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => self.blocked = true,
        }
    }

    /// Writes the file now (atomic). Does nothing if the existing file was unreadable.
    pub fn flush(&self) {
        if self.blocked {
            return;
        }
        let body = serde_json::json!({ "version": 1, "logins": self.entries });
        let _ = write_atomic(&self.file, body.to_string().as_bytes());
    }

    fn save(&self) {
        self.flush();
    }

    pub fn encryption(&self) -> bool {
        self.cipher.available()
    }

    fn encrypt(&self, p: &str) -> Option<String> {
        self.cipher.encrypt(p).map(|b| base64::engine::general_purpose::STANDARD.encode(b))
    }

    fn decrypt(&self, e: &StoredLogin) -> Option<String> {
        self.cipher.decrypt(&base64::engine::general_purpose::STANDARD.decode(&e.password).ok()?)
    }

    fn find(&self, id: &str) -> Option<usize> {
        self.entries.iter().position(|e| e.id == id)
    }

    // ------------------------------------------------------------- settings

    pub fn list(&self) -> Vec<SavedLogin> {
        let host = |o: &str| o.split_once("://").map(|x| x.1).unwrap_or(o).trim_start_matches("www.").to_string();
        let mut v: Vec<SavedLogin> = self
            .entries
            .iter()
            .map(|e| SavedLogin { id: e.id.clone(), origin: e.origin.clone(), username: e.username.clone(), created_at: e.created_at, updated_at: e.updated_at, last_used_at: e.last_used_at })
            .collect();
        v.sort_by(|a, b| host(&a.origin).cmp(&host(&b.origin)).then(a.username.cmp(&b.username)));
        v
    }

    /// Password of a login. The app authenticates the user first (`osauth::AuthGate`).
    pub fn reveal(&self, id: &str) -> Option<String> {
        self.decrypt(&self.entries[self.find(id)?])
    }

    /// Adds or edits a login; returns an error key (`passwords.error.*`) or `None`.
    pub fn upsert(&mut self, id: Option<&str>, origin: &str, username: &str, password: Option<&str>) -> Option<&'static str> {
        if !self.encryption() {
            return Some("passwords.error.encryption");
        }
        let Some(origin) = normalize_origin(origin) else { return Some("passwords.error.site") };
        let username = clip(username.trim(), MAX_FIELD);
        let existing = match id {
            Some(i) => match self.find(i) {
                Some(ix) => Some(ix),
                None => return Some("passwords.error.missing"),
            },
            None => None,
        };
        if password.is_some_and(|p| p.chars().count() > MAX_FIELD) {
            return Some("passwords.error.password");
        }
        if existing.is_none() && password.is_none_or(str::is_empty) {
            return Some("passwords.error.password");
        }
        if self.entries.iter().enumerate().any(|(i, e)| Some(i) != existing && e.origin == origin && e.username == username) {
            return Some("passwords.error.duplicate");
        }
        let now = now_ms();
        let enc = match password.filter(|p| !p.is_empty()) {
            Some(p) => match self.encrypt(p) {
                Some(e) => Some(e),
                None => return Some("passwords.error.encryption"),
            },
            None => None,
        };
        match existing {
            Some(ix) => {
                let e = &mut self.entries[ix];
                e.origin = origin;
                e.username = username;
                e.updated_at = now;
                if let Some(enc) = enc {
                    e.password = enc;
                }
            }
            None => self.entries.push(StoredLogin { id: uuid::Uuid::new_v4().to_string(), origin, username, password: enc.unwrap_or_default(), created_at: now, updated_at: now, last_used_at: None }),
        }
        self.save();
        None
    }

    pub fn remove(&mut self, id: &str) {
        let before = self.entries.len();
        self.entries.retain(|e| e.id != id);
        if self.entries.len() != before {
            self.save();
        }
    }

    /// Adds logins coming from elsewhere; those already present (same site, same username) are kept as is.
    /// `existing` counts those saved before the call (duplicates inside the list do not count).
    pub fn add_many(&mut self, list: &[NewLogin]) -> (usize, usize) {
        if !self.encryption() {
            return (0, 0);
        }
        let before: std::collections::HashSet<String> = self.entries.iter().map(|e| format!("{}\n{}", e.origin, e.username)).collect();
        let mut counted = std::collections::HashSet::new();
        let (mut added, mut existing) = (0, 0);
        let now = now_ms();
        for l in list {
            let Some(origin) = normalize_origin(&l.origin) else { continue };
            let username = clip(l.username.trim(), MAX_FIELD);
            if l.password.is_empty() || l.password.chars().count() > MAX_FIELD {
                continue;
            }
            let key = format!("{origin}\n{username}");
            if before.contains(&key) {
                if counted.insert(key) {
                    existing += 1;
                }
                continue;
            }
            if self.entries.iter().any(|e| e.origin == origin && e.username == username) {
                continue;
            }
            let created_at = l.created_at.filter(|c| *c > 0 && *c <= now).unwrap_or(now);
            let Some(password) = self.encrypt(&l.password) else { continue };
            self.entries.push(StoredLogin { id: uuid::Uuid::new_v4().to_string(), origin, username, password, created_at, updated_at: now, last_used_at: None });
            added += 1;
        }
        if added > 0 {
            self.save();
        }
        (added, existing)
    }

    /// Imports a CSV exported from another browser or manager.
    pub fn import_csv(&mut self, text: &str) -> PasswordImportResult {
        if !self.encryption() {
            return PasswordImportResult { error: Some("encryption"), ..Default::default() };
        }
        let mut rows = parse_csv(text);
        if rows.is_empty() {
            return PasswordImportResult { error: Some("format"), ..Default::default() };
        }
        let header: Vec<String> = rows.remove(0).iter().map(|h| h.trim().to_lowercase()).collect();
        let col = |names: &[&str]| names.iter().find_map(|n| header.iter().position(|h| h == n));
        let (Some(ui), Some(pi)) = (col(URL_COLUMNS), col(PASSWORD_COLUMNS)) else {
            return PasswordImportResult { error: Some("format"), ..Default::default() };
        };
        let user: Vec<usize> = USER_COLUMNS.iter().filter_map(|n| header.iter().position(|h| h == n)).collect();
        let list: Vec<NewLogin> = rows
            .iter()
            .map(|row| NewLogin {
                origin: row.get(ui).cloned().unwrap_or_default(),
                // Proton Pass separates username and email: the first non-empty one wins.
                username: user.iter().map(|i| row.get(*i).map(|s| s.trim().to_string()).unwrap_or_default()).find(|s| !s.is_empty()).unwrap_or_default(),
                password: row.get(pi).cloned().unwrap_or_default(),
                created_at: None,
            })
            .collect();
        let (added, existing) = self.add_many(&list);
        PasswordImportResult { added, existing, error: None }
    }

    /// CSV export in Chrome's format (`name,url,username,password`). The app authenticates the user first.
    pub fn export_csv(&self) -> String {
        let mut lines = vec!["name,url,username,password".to_string()];
        for e in &self.entries {
            let Some(pw) = self.decrypt(e) else { continue };
            let name = url::Url::parse(&e.origin).ok().and_then(|u| u.host_str().map(String::from)).unwrap_or_default();
            lines.push([name.as_str(), &e.origin, &e.username, &pw].iter().map(|s| csv_field(s)).collect::<Vec<_>>().join(","));
        }
        format!("{}\n", lines.join("\n"))
    }

    // ------------------------------------------------------------- page side

    /// Accounts offered for an origin (the origin comes from the sending frame, never from the page).
    pub fn accounts(&self, origin: &str, manager: &str) -> Vec<(String, String)> {
        if manager != "signet" {
            return vec![];
        }
        self.entries.iter().filter(|e| e.origin == origin).map(|e| (e.id.clone(), e.username.clone())).collect()
    }

    /// Credentials to fill. Records the use unless the tab is private.
    pub fn fill(&mut self, origin: &str, id: &str, manager: &str, private: bool) -> Option<(String, String)> {
        let ix = self.find(id)?;
        if self.entries[ix].origin != origin || manager != "signet" {
            return None;
        }
        let password = self.decrypt(&self.entries[ix])?;
        let user = self.entries[ix].username.clone();
        if !private {
            self.entries[ix].last_used_at = Some(now_ms());
            self.save();
        }
        Some((user, password))
    }

    fn prune_pending(&mut self, open_tabs: &dyn Fn(&str) -> bool) {
        let now = now_ms();
        self.pending.retain(|_, p| now - p.at <= PENDING_TTL_MS && open_tabs(&p.tab_id));
    }

    /// A login was submitted in a page. Returns the prompt to show, if any.
    #[allow(clippy::too_many_arguments)]
    pub fn submitted(&mut self, tab_id: &str, origin: &str, username: &str, password: &str, private: bool, manager: &str, settings: &PromptSettings, tab_open: &dyn Fn(&str) -> bool) -> Option<PromptRequest> {
        if manager != "signet" || password.chars().count() > MAX_FIELD {
            return None;
        }
        let now = now_ms();
        let mut username = clip(username.trim(), MAX_FIELD);
        if password.is_empty() {
            if !username.is_empty() && !private {
                self.identifiers.insert(tab_id.into(), (origin.into(), username, now));
            }
            return None;
        }
        if username.is_empty()
            && let Some((o, u, at)) = self.identifiers.get(tab_id)
                && o == origin && now - at < IDENTIFIER_TTL_MS {
                    username = u.clone();
                }
        let same = self.entries.iter().position(|e| e.origin == origin && e.username == username);
        if let Some(ix) = same
            && self.decrypt(&self.entries[ix]).as_deref() == Some(password) {
                if !private {
                    self.entries[ix].last_used_at = Some(now);
                    self.save();
                }
                return None;
            }
        if !settings.offer_save || settings.never_save.iter().any(|o| o == origin) || !self.encryption() || private {
            return None;
        }
        self.prune_pending(tab_open);
        let rate_key = format!("{tab_id}\n{origin}");
        if now - self.last_prompt.get(&rate_key).copied().unwrap_or(0) < PROMPT_RATE_MS {
            return None;
        }
        self.last_prompt.insert(rate_key, now);
        if self.last_prompt.len() > 200 {
            self.last_prompt.retain(|_, at| now - *at <= PROMPT_RATE_MS);
        }
        // Form submission and button click often arrive together: a single prompt.
        if self.pending.values().any(|p| p.origin == origin && p.username == username && p.password == password) {
            return None;
        }
        self.pending.retain(|_, p| p.tab_id != tab_id);
        let token = uuid::Uuid::new_v4().to_string();
        let existing_id = same.map(|ix| self.entries[ix].id.clone());
        self.pending.insert(token.clone(), Pending { tab_id: tab_id.into(), origin: origin.into(), username: username.clone(), password: password.into(), existing_id: existing_id.clone(), at: now });
        Some(PromptRequest { token, tab_id: tab_id.into(), origin: origin.into(), username, update: existing_id.is_some() })
    }

    /// Answer to a prompt. "Not now" keeps the prompt available from the key button.
    pub fn answer(&mut self, token: &str, answer: Answer) -> AnswerEffect {
        let Some(p) = self.pending.get(token).cloned() else { return AnswerEffect::default() };
        if answer != Answer::Dismiss {
            self.pending.remove(token);
        }
        let mut eff = AnswerEffect { tab_id: Some(p.tab_id.clone()), ..Default::default() };
        match answer {
            Answer::Save => {
                let now = now_ms();
                let ix = match &p.existing_id {
                    Some(id) => self.find(id),
                    None => self.entries.iter().position(|e| e.origin == p.origin && e.username == p.username),
                };
                let Some(enc) = self.encrypt(&p.password) else { return eff };
                match ix {
                    Some(ix) => {
                        let e = &mut self.entries[ix];
                        e.password = enc;
                        e.updated_at = now;
                        e.last_used_at = Some(now);
                    }
                    None => self.entries.push(StoredLogin { id: uuid::Uuid::new_v4().to_string(), origin: p.origin, username: p.username, password: enc, created_at: now, updated_at: now, last_used_at: Some(now) }),
                }
                self.save();
                eff.saved = true;
            }
            Answer::Never => eff.never_origin = Some(p.origin),
            Answer::Dismiss => {}
        }
        eff
    }

    // ------------------------------------------------------------- key button

    fn pending_for(&mut self, tab_id: &str, origin: &str, tab_open: &dyn Fn(&str) -> bool) -> Option<(String, PromptRequest)> {
        self.prune_pending(tab_open);
        let site = site_of(origin);
        self.pending.iter().find(|(_, p)| p.tab_id == tab_id && site_of(&p.origin) == site).map(|(t, p)| {
            (t.clone(), PromptRequest { token: t.clone(), tab_id: p.tab_id.clone(), origin: p.origin.clone(), username: p.username.clone(), update: p.existing_id.is_some() })
        })
    }

    /// Key button state of a tab: pending prompt, accounts saved for its origin, or nothing.
    pub fn key_state(&mut self, tab_id: &str, tab_url: &str, manager: &str, private: bool, tab_open: &dyn Fn(&str) -> bool) -> Option<KeyState> {
        if manager != "signet" || private {
            return None;
        }
        let origin = normalize_web_origin(tab_url)?;
        if self.pending_for(tab_id, &origin, tab_open).is_some() {
            return Some(KeyState::Pending);
        }
        self.entries.iter().any(|e| e.origin == origin).then_some(KeyState::Saved)
    }

    /// Navigation: a prompt from another site is forgotten. Returns the tokens dropped (their card must go away).
    pub fn on_navigate(&mut self, tab_id: &str, url: &str) -> Vec<String> {
        let site = site_of(url);
        let gone: Vec<String> = self.pending.iter().filter(|(_, p)| p.tab_id == tab_id && site_of(&p.origin) != site).map(|(t, _)| t.clone()).collect();
        for t in &gone {
            self.pending.remove(t);
        }
        gone
    }

    /// Click on the key button: the pending prompt to reopen, otherwise the account list of the site.
    pub fn open_key(&mut self, tab_id: &str, tab_url: &str, manager: &str, private: bool, tab_open: &dyn Fn(&str) -> bool) -> KeyAction {
        let Some(origin) = normalize_web_origin(tab_url) else { return KeyAction::None };
        if manager != "signet" || private {
            return KeyAction::None;
        }
        if let Some((_, req)) = self.pending_for(tab_id, &origin, tab_open) {
            return KeyAction::Prompt(req);
        }
        let logins: Vec<(String, String)> = self.accounts(&origin, manager);
        if logins.is_empty() { KeyAction::None } else { KeyAction::List { origin, logins } }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum KeyAction {
    None,
    Prompt(PromptRequest),
    List { origin: String, logins: Vec<(String, String)> },
}

fn normalize_web_origin(url: &str) -> Option<String> {
    let u = url::Url::parse(url).ok()?;
    matches!(u.scheme(), "http" | "https").then(|| u.origin().ascii_serialization())
}

// ---------------------------------------------------------------- Electron vault import

/// Key names of the Electron app's `safeStorage` (Chromium os_crypt).
pub const ELECTRON_KEYS: KeyNames = KeyNames { mac: "Signet Safe Storage", linux: &["signet", "Signet"] };

/// Reads the passwords of the Electron app's vault (`<electron data>/passwords.json`) and decrypts them.
/// `data_dir` is the Electron user data folder (used on Windows to find `Local State`). The user's keychain prompts.
pub fn import_electron_vault(file: &Path, data_dir: &Path, provider: &dyn KeyProvider) -> Vec<NewLogin> {
    let Ok(raw) = fs::read_to_string(file) else { return vec![] };
    let Ok(shape) = serde_json::from_str::<FileShape>(&raw) else { return vec![] };
    let entries: Vec<StoredLogin> = shape.logins.into_iter().filter_map(|v| serde_json::from_value(v).ok()).collect();
    if entries.is_empty() {
        return vec![];
    }
    // `profile.parent()` must be the data dir for the Windows `Local State` lookup.
    let keys = provider.keys(&data_dir.join("Default"), &ELECTRON_KEYS);
    entries
        .into_iter()
        .filter_map(|e| {
            let blob = base64::engine::general_purpose::STANDARD.decode(&e.password).ok()?;
            let password = chromium_decrypt(&blob, &keys, cfg!(target_os = "windows"))?;
            Some(NewLogin { origin: e.origin, username: e.username, password, created_at: Some(e.created_at) })
        })
        .collect()
}

/// Same, with the real system keys.
pub fn import_electron_vault_system(file: &Path, data_dir: &Path) -> Vec<NewLogin> {
    import_electron_vault(file, data_dir, &SystemKeys)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::importers::chromium::{derive_cbc_key, Keys};
    use crate::secrets::MemoryStore;
    use cbc::cipher::{block_padding::Pkcs7, BlockEncryptMut, KeyIvInit};

    /// Reversible fake like the TS test's fake safeStorage.
    struct Fake(bool);
    impl Cipher for Fake {
        fn available(&self) -> bool { self.0 }
        fn encrypt(&self, p: &str) -> Option<Vec<u8>> { Some(format!("enc:{p}").into_bytes()) }
        fn decrypt(&self, b: &[u8]) -> Option<String> { String::from_utf8(b.to_vec()).ok().map(|s| s.trim_start_matches("enc:").to_string()) }
    }

    fn make(dir: &Path) -> Passwords { Passwords::new(dir, Arc::new(Fake(true))) }
    fn nl(o: &str, u: &str, p: &str, c: Option<i64>) -> NewLogin { NewLogin { origin: o.into(), username: u.into(), password: p.into(), created_at: c } }
    fn rows(v: &[&[&str]]) -> Vec<Vec<String>> { v.iter().map(|r| r.iter().map(|s| s.to_string()).collect()).collect() }

    #[test]
    fn csv_parsing() {
        assert_eq!(parse_csv("a,b,c\n1,2,3\r\n4,5,6\r7,8,9"), rows(&[&["a", "b", "c"], &["1", "2", "3"], &["4", "5", "6"], &["7", "8", "9"]]));
        assert_eq!(parse_csv("name,pw\n\"a,b\",\"x\"\"y\"\n\"ligne1\nligne2\",z"), rows(&[&["name", "pw"], &["a,b", "x\"y"], &["ligne1\nligne2", "z"]]));
        assert_eq!(parse_csv("\u{feff}a,b\n\n,\n1,\n   ,  \n"), rows(&[&["a", "b"], &["1", ""]]));
        assert_eq!(parse_csv("a,b"), rows(&[&["a", "b"]]));
        assert!(parse_csv("").is_empty());
        assert_eq!(parse_csv("a,\"b,c\nd"), rows(&[&["a", "b,c\nd"]]));
    }

    #[test]
    fn origins() {
        assert_eq!(normalize_origin("example.com").as_deref(), Some("https://example.com"));
        assert_eq!(normalize_origin(" https://Example.com/login?x=1 ").as_deref(), Some("https://example.com"));
        assert_eq!(normalize_origin("http://localhost:3000/a").as_deref(), Some("http://localhost:3000"));
        for bad in ["", "ftp://example.com", "javascript:alert(1)", "http://"] { assert_eq!(normalize_origin(bad), None, "{bad}"); }
    }

    #[test]
    fn managers() {
        assert!(is_manager("signet") && is_manager("off") && is_manager("bitwarden"));
        assert!(!is_manager("nope"));
    }

    #[test]
    fn add_many_rules() {
        let d = tempfile::tempdir().unwrap();
        let mut p = make(d.path());
        let r = p.add_many(&[
            nl("example.com", " me ", "pw1", None),
            nl("https://example.com/other", "me", "pw2", None),
            nl("ftp://x.com", "a", "p", None),
            nl("x.com", "a", "", None),
            nl("y.com", "a", &"x".repeat(1001), None),
            nl("z.com", "", "pw3", None),
        ]);
        assert_eq!(r, (2, 0));
        let l = p.list();
        assert_eq!(l.iter().map(|x| (x.origin.as_str(), x.username.as_str())).collect::<Vec<_>>(), vec![("https://example.com", "me"), ("https://z.com", "")]);
        assert!(!serde_json::to_string(&l).unwrap().contains("pw"));
    }

    #[test]
    fn existing_counted_once_and_kept() {
        let d = tempfile::tempdir().unwrap();
        let mut p = make(d.path());
        p.add_many(&[nl("a.com", "u", "old", None)]);
        let r = p.add_many(&[nl("a.com", "u", "new", None), nl("a.com", "u", "new2", None), nl("b.com", "u", "p", None)]);
        assert_eq!(r, (1, 1));
        let id = p.list().into_iter().find(|l| l.origin == "https://a.com").unwrap().id;
        assert_eq!(p.reveal(&id).as_deref(), Some("old"));
    }

    #[test]
    fn no_encryption_no_add() {
        let d = tempfile::tempdir().unwrap();
        let mut p = Passwords::new(d.path(), Arc::new(Fake(false)));
        assert_eq!(p.add_many(&[nl("a.com", "u", "p", None)]), (0, 0));
        assert_eq!(p.upsert(None, "a.com", "u", Some("p")), Some("passwords.error.encryption"));
    }

    #[test]
    fn created_at_clamped() {
        let d = tempfile::tempdir().unwrap();
        let mut p = make(d.path());
        let now = now_ms();
        p.add_many(&[nl("a.com", "1", "p", Some(1000)), nl("b.com", "1", "p", Some(now + 1_000_000_000)), nl("c.com", "1", "p", Some(0))]);
        let by: HashMap<String, i64> = p.list().into_iter().map(|l| (l.origin, l.created_at)).collect();
        assert_eq!(by["https://a.com"], 1000);
        assert!(by["https://b.com"] <= now_ms() && by["https://b.com"] >= now);
        assert!(by["https://c.com"] >= now);
    }

    #[test]
    fn file_format_and_reload() {
        let d = tempfile::tempdir().unwrap();
        let mut p = make(d.path());
        p.add_many(&[nl("a.com", "u", "secret", None)]);
        let raw = fs::read_to_string(d.path().join("passwords.json")).unwrap();
        assert!(!raw.contains("\"secret\""));
        let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(v["version"], 1);
        assert_eq!(v["logins"][0]["origin"], "https://a.com");
        assert!(v["logins"][0].get("createdAt").is_some());
        assert_eq!(make(d.path()).list().len(), 1);
    }

    #[test]
    fn corrupted_file_is_copied_not_lost() {
        let d = tempfile::tempdir().unwrap();
        fs::write(d.path().join("passwords.json"), "{not json").unwrap();
        let p = make(d.path());
        assert!(p.list().is_empty());
        assert!(fs::read_dir(d.path()).unwrap().flatten().any(|e| e.file_name().to_string_lossy().contains("corrupt")));
    }

    #[test]
    fn upsert_rules() {
        let d = tempfile::tempdir().unwrap();
        let mut p = make(d.path());
        assert_eq!(p.upsert(None, "nope://", "u", Some("p")), Some("passwords.error.site"));
        assert_eq!(p.upsert(None, "a.com", "u", None), Some("passwords.error.password"));
        assert_eq!(p.upsert(None, "a.com", "u", Some("pw")), None);
        assert_eq!(p.upsert(None, "a.com", "u", Some("pw2")), Some("passwords.error.duplicate"));
        assert_eq!(p.upsert(Some("ghost"), "a.com", "u", Some("pw")), Some("passwords.error.missing"));
        let id = p.list()[0].id.clone();
        assert_eq!(p.upsert(Some(&id), "a.com", "u2", None), None);
        assert_eq!(p.reveal(&id).as_deref(), Some("pw"));
        assert_eq!(p.list()[0].username, "u2");
        p.remove(&id);
        assert!(p.list().is_empty());
    }

    #[test]
    fn csv_import_export() {
        let d = tempfile::tempdir().unwrap();
        let mut p = make(d.path());
        let csv = "name,url,username,password\nA,https://a.com/login,me,\"p,w\"\nB,b.com,,x\n";
        assert_eq!(p.import_csv(csv), PasswordImportResult { added: 2, existing: 0, error: None });
        assert_eq!(p.import_csv("foo,bar\n1,2").error, Some("format"));
        assert_eq!(p.import_csv("").error, Some("format"));
        let out = p.export_csv();
        assert!(out.starts_with("name,url,username,password\n"));
        assert!(out.contains("a.com,https://a.com,me,\"p,w\""));
        // Proton style: username empty, email used
        let mut q = make(tempfile::tempdir().unwrap().path());
        assert_eq!(q.import_csv("url,username,email,password\nc.com,,mail@x.com,pw\n").added, 1);
        assert_eq!(q.list()[0].username, "mail@x.com");
    }

    fn settings() -> PromptSettings { PromptSettings { offer_save: true, never_save: vec![] } }
    fn open(_: &str) -> bool { true }

    #[test]
    fn prompt_flow() {
        let d = tempfile::tempdir().unwrap();
        let mut p = make(d.path());
        let req = p.submitted("t1", "https://a.com", "me", "pw", false, "signet", &settings(), &open).unwrap();
        assert!(!req.update);
        // Same entry again (form submit + click): no second prompt.
        assert!(p.submitted("t1", "https://a.com", "me", "pw", false, "signet", &settings(), &open).is_none());
        assert_eq!(p.key_state("t1", "https://a.com/x", "signet", false, &open), Some(KeyState::Pending));
        assert!(matches!(p.open_key("t1", "https://a.com/x", "signet", false, &open), KeyAction::Prompt(_)));
        let eff = p.answer(&req.token, Answer::Dismiss);
        assert!(!eff.saved);
        assert_eq!(p.key_state("t1", "https://a.com", "signet", false, &open), Some(KeyState::Pending));
        let eff = p.answer(&req.token, Answer::Save);
        assert!(eff.saved);
        assert_eq!(p.key_state("t1", "https://a.com", "signet", false, &open), Some(KeyState::Saved));
        assert!(matches!(p.open_key("t1", "https://a.com", "signet", false, &open), KeyAction::List { .. }));
        // Same password: nothing to ask. Different password: update prompt.
        assert!(p.submitted("t1", "https://a.com", "me", "pw", false, "signet", &settings(), &open).is_none());
        let again = p.submitted("t2", "https://a.com", "me", "other", false, "signet", &settings(), &open).unwrap();
        assert!(again.update);
        let eff = p.answer(&again.token, Answer::Save);
        assert!(eff.saved);
        let id = p.list()[0].id.clone();
        assert_eq!(p.reveal(&id).as_deref(), Some("other"));
    }

    #[test]
    fn prompt_suppressed_cases() {
        let d = tempfile::tempdir().unwrap();
        let mut p = make(d.path());
        assert!(p.submitted("t", "https://a.com", "u", "pw", true, "signet", &settings(), &open).is_none());
        assert!(p.submitted("t", "https://a.com", "u", "pw", false, "off", &settings(), &open).is_none());
        let never = PromptSettings { offer_save: true, never_save: vec!["https://a.com".into()] };
        assert!(p.submitted("t", "https://a.com", "u", "pw", false, "signet", &never, &open).is_none());
        assert!(p.submitted("t", "https://a.com", "u", "pw", false, "signet", &PromptSettings::default(), &open).is_none());
        let req = p.submitted("t", "https://a.com", "u", "pw", false, "signet", &settings(), &open).unwrap();
        // Rate limit: another prompt within 3 s on the same tab and origin is ignored.
        assert!(p.submitted("t", "https://a.com", "u", "pw3", false, "signet", &settings(), &open).is_none());
        let eff = p.answer(&req.token, Answer::Never);
        assert_eq!(eff.never_origin.as_deref(), Some("https://a.com"));
        assert!(p.list().is_empty());
    }

    #[test]
    fn two_step_login_remembers_username() {
        let d = tempfile::tempdir().unwrap();
        let mut p = make(d.path());
        assert!(p.submitted("t", "https://a.com", "bob", "", false, "signet", &settings(), &open).is_none());
        let req = p.submitted("t", "https://a.com", "", "pw", false, "signet", &settings(), &open).unwrap();
        assert_eq!(req.username, "bob");
    }

    #[test]
    fn navigation_drops_foreign_prompts() {
        let d = tempfile::tempdir().unwrap();
        let mut p = make(d.path());
        let req = p.submitted("t", "https://a.com", "u", "pw", false, "signet", &settings(), &open).unwrap();
        assert!(p.on_navigate("t", "https://login.a.com/x").is_empty());
        assert_eq!(p.on_navigate("t", "https://other.org"), vec![req.token]);
        assert_eq!(p.key_state("t", "https://a.com", "signet", false, &open), None);
        let closed = |_: &str| false;
        p.submitted("t", "https://a.com", "u", "pw", false, "signet", &settings(), &open);
        assert_eq!(p.key_state("t", "https://a.com", "signet", false, &closed), None);
    }

    #[test]
    fn fill_checks_origin_and_manager() {
        let d = tempfile::tempdir().unwrap();
        let mut p = make(d.path());
        p.add_many(&[nl("a.com", "me", "pw", None)]);
        let id = p.list()[0].id.clone();
        assert_eq!(p.accounts("https://a.com", "signet").len(), 1);
        assert!(p.accounts("https://a.com", "off").is_empty());
        assert!(p.fill("https://evil.com", &id, "signet", false).is_none());
        assert_eq!(p.fill("https://a.com", &id, "signet", true), Some(("me".into(), "pw".into())));
        assert!(p.list()[0].last_used_at.is_none());
        p.fill("https://a.com", &id, "signet", false);
        assert!(p.list()[0].last_used_at.is_some());
    }

    #[test]
    fn keychain_cipher_roundtrip_and_key_persists() {
        let store = MemoryStore::default();
        let c = KeychainCipher::new(&store);
        assert!(c.available());
        let blob = c.encrypt("hello").unwrap();
        assert_eq!(c.decrypt(&blob).as_deref(), Some("hello"));
        assert_eq!(KeychainCipher::new(&store).decrypt(&blob).as_deref(), Some("hello"));
        assert_eq!(KeychainCipher::new(&MemoryStore::default()).decrypt(&blob), None);
        assert_eq!(c.decrypt(b"v10garbage"), None);
    }

    struct FixedKeys(Keys);
    impl KeyProvider for FixedKeys {
        fn keys(&self, _: &Path, names: &KeyNames) -> Keys {
            assert_eq!(names.mac, "Signet Safe Storage");
            self.0.clone()
        }
    }

    #[test]
    fn electron_vault_import() {
        if cfg!(target_os = "windows") { return; }
        let d = tempfile::tempdir().unwrap();
        let key = derive_cbc_key(b"keychain-secret", 1003);
        let enc = |pw: &str| {
            let mut out = b"v10".to_vec();
            out.extend(cbc::Encryptor::<aes::Aes128>::new((&key).into(), (&[0x20u8; 16]).into()).encrypt_padded_vec_mut::<Pkcs7>(pw.as_bytes()));
            base64::engine::general_purpose::STANDARD.encode(out)
        };
        let vault = serde_json::json!({"version":1,"logins":[
            {"id":"1","origin":"https://a.com","username":"me","password":enc("pw1"),"createdAt":1234,"updatedAt":1234},
            {"id":"2","origin":"https://b.com","username":"x","password":"bm90LWEtdmFsaWQtYmxvYg==","createdAt":1,"updatedAt":1}
        ]});
        let file = d.path().join("passwords.json");
        fs::write(&file, vault.to_string()).unwrap();
        let out = import_electron_vault(&file, d.path(), &FixedKeys(Keys { v10: Some(key), ..Default::default() }));
        assert_eq!(out.len(), 1);
        assert_eq!((out[0].origin.as_str(), out[0].username.as_str(), out[0].password.as_str(), out[0].created_at), ("https://a.com", "me", "pw1", Some(1234)));
        let mut p = make(tempfile::tempdir().unwrap().path());
        assert_eq!(p.add_many(&out), (1, 0));
    }
}

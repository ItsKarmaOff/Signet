//! Profile of a Chromium browser (Chrome, Chromium, Brave, Edge, Arc): history, extensions and passwords
//! (port of `importers/chromium.ts`). SQLite databases are copied before reading (the open browser locks them).
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use cbc::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;

use crate::util::{Error, Result};

/// Name of the browser in the system keychain.
#[derive(Debug, Clone)]
pub struct KeyNames {
    /// macOS: Keychain service ("Brave Safe Storage").
    pub mac: &'static str,
    /// Linux: value of the `application` attribute in the Secret Service keyring, in the order tried.
    pub linux: &'static [&'static str],
}

pub const HISTORY_LIMIT: usize = 5000;
/// Microseconds between 1601-01-01 (Chromium's clock) and 1970-01-01.
const EPOCH_DELTA_US: i64 = 11_644_473_600_000_000;

pub fn chrome_time(us: i64) -> i64 {
    if us > EPOCH_DELTA_US { (us - EPOCH_DELTA_US) / 1000 } else { 0 }
}

#[derive(Debug, PartialEq, Eq)]
pub struct ProfileHas {
    pub history: bool,
    pub passwords: bool,
    pub extensions: bool,
}

/// Presence of each data file. Never reads nor lists a folder (macOS protects other browsers' data).
pub fn profile_has(profile: &Path) -> ProfileHas {
    ProfileHas { history: profile.join("History").exists(), passwords: profile.join("Login Data").exists(), extensions: profile.join("Extensions").exists() }
}

/// Copies a database (and its WAL journal) to a temporary folder and opens the copy.
fn with_copy<T>(file: &Path, f: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> Result<T> {
    let dir = tempfile::Builder::new().prefix("signet-import-").tempdir()?;
    let copy = dir.path().join("db");
    fs::copy(file, &copy)?;
    let wal = PathBuf::from(format!("{}-wal", file.display()));
    if wal.exists() {
        let _ = fs::copy(&wal, dir.path().join("db-wal"));
    }
    let conn = Connection::open_with_flags(&copy, OpenFlags::SQLITE_OPEN_READ_WRITE).map_err(|e| Error::Msg(e.to_string()))?;
    f(&conn).map_err(|e| Error::Msg(e.to_string()))
}

#[derive(Debug, Clone, PartialEq)]
pub struct HistoryRow {
    pub url: String,
    pub title: String,
    pub visits: i64,
    pub typed: i64,
    pub last_visit: i64,
}

/// Visited pages, most recent first (hidden pages excluded).
pub fn read_history(profile: &Path) -> Result<Vec<HistoryRow>> {
    with_copy(&profile.join("History"), |c| {
        let mut st = c.prepare(&format!(
            "SELECT url, title, visit_count, typed_count, last_visit_time FROM urls WHERE hidden = 0 AND url LIKE 'http%' ORDER BY last_visit_time DESC LIMIT {HISTORY_LIMIT}"
        ))?;
        let rows = st.query_map([], |r| {
            Ok(HistoryRow {
                url: r.get::<_, String>(0)?,
                title: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                visits: r.get::<_, Option<i64>>(2)?.filter(|v| *v != 0).unwrap_or(1),
                typed: r.get::<_, Option<i64>>(3)?.unwrap_or(0),
                last_visit: chrome_time(r.get::<_, Option<i64>>(4)?.unwrap_or(0)),
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).filter(|r| crate::importers::types::is_web_url(&r.url) && r.last_visit > 0).collect::<Vec<_>>())
    })
}

fn extension_settings(profile: &Path) -> serde_json::Map<String, Value> {
    let mut out = serde_json::Map::new();
    for name in ["Preferences", "Secure Preferences"] {
        let Ok(raw) = fs::read_to_string(profile.join(name)) else { continue };
        let Ok(json) = serde_json::from_str::<Value>(&raw) else { continue };
        if let Some(s) = json.get("extensions").and_then(|e| e.get("settings")).and_then(|s| s.as_object()) {
            for (k, v) in s {
                out.insert(k.clone(), v.clone());
            }
        }
    }
    out
}

fn disabled(s: &serde_json::Map<String, Value>) -> bool {
    s.get("state").and_then(|v| v.as_i64()) == Some(0)
        || match s.get("disable_reasons") {
            Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0) != 0.0,
            Some(Value::Array(a)) => !a.is_empty(),
            _ => false,
        }
}

/// Extensions installed from the store and enabled. Permission errors are returned, not taken for "none".
pub fn extension_ids(profile: &Path) -> Result<Vec<String>> {
    let rd = match fs::read_dir(profile.join("Extensions")) {
        Ok(r) => r,
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => return Err(Error::Io(e)),
        Err(_) => return Ok(vec![]),
    };
    let settings = extension_settings(profile);
    let mut ids: Vec<String> = rd
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|d| d.len() == 32 && d.bytes().all(|b| (b'a'..=b'p').contains(&b)))
        .filter(|id| match settings.get(id).and_then(|s| s.as_object()) {
            None => true,
            Some(s) => s.get("was_installed_by_default").and_then(|v| v.as_bool()) != Some(true) && s.get("from_webstore").and_then(|v| v.as_bool()) != Some(false) && !disabled(s),
        })
        .collect();
    ids.sort();
    Ok(ids)
}

// ---------------------------------------------------------------- passwords

const SALT: &[u8] = b"saltysalt";

/// Decryption keys; `None` = key not found (browser never opened, keychain locked...).
#[derive(Default, Clone)]
pub struct Keys {
    pub v10: Option<[u8; 16]>,
    pub v11: Option<[u8; 16]>,
    pub gcm: Option<[u8; 32]>,
}

pub fn derive_cbc_key(secret: &[u8], rounds: u32) -> [u8; 16] {
    let mut k = [0u8; 16];
    pbkdf2::pbkdf2_hmac::<sha1::Sha1>(secret, SALT, rounds, &mut k);
    k
}

/// Source of the browser's keys. The real one asks the system (macOS prompts the user).
pub trait KeyProvider {
    fn keys(&self, profile: &Path, names: &KeyNames) -> Keys;
}

fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string()).filter(|s| !s.is_empty())
}

pub struct SystemKeys;

impl KeyProvider for SystemKeys {
    fn keys(&self, profile: &Path, names: &KeyNames) -> Keys {
        if crate::secrets::no_keychain() {
            return Keys::default();
        }
        #[cfg(target_os = "macos")]
        {
            let _ = profile;
            let secret = run("security", &["find-generic-password", "-w", "-s", names.mac]);
            Keys { v10: secret.map(|s| derive_cbc_key(s.as_bytes(), 1003)), ..Default::default() }
        }
        #[cfg(target_os = "windows")]
        {
            let _ = names;
            return windows_keys(profile);
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = profile;
            let secret = names.linux.iter().find_map(|app| run("secret-tool", &["lookup", "application", app]));
            Keys { v10: Some(derive_cbc_key(b"peanuts", 1)), v11: secret.map(|s| derive_cbc_key(s.as_bytes(), 1)), gcm: None }
        }
    }
}

#[cfg(target_os = "windows")]
fn windows_keys(profile: &Path) -> Keys {
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD;
    let go = || -> Option<[u8; 32]> {
        let state: Value = serde_json::from_str(&fs::read_to_string(profile.parent()?.join("Local State")).ok()?).ok()?;
        let wrapped = b64.decode(state.get("os_crypt")?.get("encrypted_key")?.as_str()?).ok()?;
        if wrapped.get(..5)? != b"DPAPI" {
            return None;
        }
        let blob = b64.encode(&wrapped[5..]);
        let script = format!("Add-Type -AssemblyName System.Security;[Convert]::ToBase64String([Security.Cryptography.ProtectedData]::Unprotect([Convert]::FromBase64String('{blob}'), $null, 'CurrentUser'))");
        let out = run("powershell.exe", &["-NoProfile", "-NonInteractive", "-Command", &script])?;
        b64.decode(out).ok()?.try_into().ok()
    };
    Keys { gcm: go(), ..Default::default() }
}

fn decrypt_cbc(key: &[u8; 16], blob: &[u8]) -> Option<String> {
    let iv = [0x20u8; 16];
    let data = blob.get(3..)?;
    let plain = cbc::Decryptor::<aes::Aes128>::new(key.into(), (&iv).into()).decrypt_padded_vec_mut::<Pkcs7>(data).ok()?;
    String::from_utf8(plain).ok()
}

/// Decrypts a `password_value` blob; `None` when unreadable.
pub fn decrypt(blob: &[u8], keys: &Keys, windows: bool) -> Option<String> {
    if blob.is_empty() {
        return None;
    }
    let tag = blob.get(..3)?;
    if windows {
        // v10: AES-256-GCM (12-byte nonce, 16-byte tag). v20 (app-bound) is unreadable outside the browser.
        if tag != b"v10" || blob.len() < 15 + 16 {
            return None;
        }
        let cipher = Aes256Gcm::new_from_slice(&keys.gcm?).ok()?;
        let plain = cipher.decrypt(Nonce::from_slice(&blob[3..15]), &blob[15..]).ok()?;
        return String::from_utf8(plain).ok();
    }
    match tag {
        b"v10" => decrypt_cbc(&keys.v10?, blob),
        b"v11" => decrypt_cbc(&keys.v11?, blob),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChromiumLogin {
    pub origin: String,
    pub username: String,
    pub password: String,
    pub created_at: i64,
}

/// Saved credentials, decrypted. `failed` counts those that could not be.
pub fn read_passwords(profile: &Path, names: &KeyNames, provider: &dyn KeyProvider) -> Result<(Vec<ChromiumLogin>, usize)> {
    struct Row { origin: String, realm: String, user: String, pass: Vec<u8>, created: i64 }
    let rows = with_copy(&profile.join("Login Data"), |c| {
        let mut st = c.prepare("SELECT origin_url, signon_realm, username_value, password_value, date_created FROM logins WHERE blacklisted_by_user = 0 ORDER BY date_created DESC")?;
        let r = st.query_map([], |r| {
            Ok(Row {
                origin: r.get::<_, Option<String>>(0)?.unwrap_or_default(),
                realm: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                user: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                pass: r.get::<_, Option<Vec<u8>>>(3)?.unwrap_or_default(),
                created: r.get::<_, Option<i64>>(4)?.unwrap_or(0),
            })
        })?;
        Ok(r.filter_map(|r| r.ok()).collect::<Vec<_>>())
    })?;
    let web: Vec<Row> = rows.into_iter().filter(|r| crate::importers::types::is_web_url(if r.realm.is_empty() { &r.origin } else { &r.realm })).collect();
    if web.is_empty() {
        return Ok((vec![], 0));
    }
    let keys = provider.keys(profile, names);
    let mut logins = Vec::new();
    let mut failed = 0;
    for r in web {
        match decrypt(&r.pass, &keys, cfg!(target_os = "windows")).filter(|p| !p.is_empty()) {
            Some(password) => logins.push(ChromiumLogin { origin: if r.origin.is_empty() { r.realm } else { r.origin }, username: r.user, password, created_at: chrome_time(r.created) }),
            None => failed += 1,
        }
    }
    Ok((logins, failed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cbc::cipher::BlockEncryptMut;

    fn enc_cbc(key: &[u8; 16], plain: &str, prefix: &[u8]) -> Vec<u8> {
        let iv = [0x20u8; 16];
        let mut out = prefix.to_vec();
        out.extend(cbc::Encryptor::<aes::Aes128>::new(key.into(), (&iv).into()).encrypt_padded_vec_mut::<Pkcs7>(plain.as_bytes()));
        out
    }

    struct Fixed(Keys);
    impl KeyProvider for Fixed {
        fn keys(&self, _: &Path, _: &KeyNames) -> Keys { self.0.clone() }
    }

    const NAMES: KeyNames = KeyNames { mac: "Test Safe Storage", linux: &["test"] };

    #[test]
    fn time_conversion() {
        assert_eq!(chrome_time(0), 0);
        assert_eq!(chrome_time(EPOCH_DELTA_US), 0);
        assert_eq!(chrome_time(EPOCH_DELTA_US + 5_000), 5);
    }

    #[test]
    fn extension_ids_rules() {
        let d = tempfile::tempdir().unwrap();
        assert!(extension_ids(d.path()).unwrap().is_empty());
        fs::create_dir(d.path().join("Extensions")).unwrap();
        assert!(extension_ids(d.path()).unwrap().is_empty());
        let id = |c: char| c.to_string().repeat(32);
        for c in ['a', 'b', 'c', 'd', 'e'] { fs::create_dir(d.path().join("Extensions").join(id(c))).unwrap(); }
        fs::create_dir(d.path().join("Extensions").join("not-an-id")).unwrap();
        let prefs = serde_json::json!({"extensions":{"settings":{
            id('b'): {"was_installed_by_default": true},
            id('c'): {"from_webstore": false},
            id('d'): {"state": 0},
            id('e'): {"disable_reasons": [1]},
        }}});
        fs::write(d.path().join("Secure Preferences"), prefs.to_string()).unwrap();
        assert_eq!(extension_ids(d.path()).unwrap(), vec![id('a')]);
        assert_eq!(profile_has(d.path()), ProfileHas { history: false, passwords: false, extensions: true });
    }

    #[cfg(unix)]
    #[test]
    fn extensions_permission_denied_is_an_error() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let ext = d.path().join("Extensions");
        fs::create_dir_all(ext.join("a".repeat(32))).unwrap();
        fs::set_permissions(&ext, fs::Permissions::from_mode(0o000)).unwrap();
        let r = extension_ids(d.path());
        fs::set_permissions(&ext, fs::Permissions::from_mode(0o755)).unwrap();
        if unsafe_is_root() { return; }
        assert!(r.is_err());
    }
    #[cfg(unix)]
    fn unsafe_is_root() -> bool { std::env::var("USER").map(|u| u == "root").unwrap_or(false) }

    #[test]
    fn history_from_sqlite_copy() {
        let d = tempfile::tempdir().unwrap();
        let c = Connection::open(d.path().join("History")).unwrap();
        c.execute_batch("CREATE TABLE urls (url TEXT, title TEXT, visit_count INT, typed_count INT, last_visit_time INT, hidden INT);").unwrap();
        let t = EPOCH_DELTA_US + 1_700_000_000_000_000;
        c.execute("INSERT INTO urls VALUES ('https://a.com','A',3,1,?1,0)", [t]).unwrap();
        c.execute("INSERT INTO urls VALUES ('https://hidden.com','H',1,0,?1,1)", [t]).unwrap();
        c.execute("INSERT INTO urls VALUES ('chrome://x','X',1,0,?1,0)", [t]).unwrap();
        c.execute("INSERT INTO urls VALUES ('https://old.com','O',1,0,0,0)", []).unwrap();
        drop(c);
        let rows = read_history(d.path()).unwrap();
        assert_eq!(rows, vec![HistoryRow { url: "https://a.com".into(), title: "A".into(), visits: 3, typed: 1, last_visit: 1_700_000_000_000 }]);
    }

    #[test]
    fn passwords_decrypt_v10_and_count_failures() {
        let d = tempfile::tempdir().unwrap();
        let key = derive_cbc_key(b"secret", 1003);
        let c = Connection::open(d.path().join("Login Data")).unwrap();
        c.execute_batch("CREATE TABLE logins (origin_url TEXT, signon_realm TEXT, username_value TEXT, password_value BLOB, date_created INT, blacklisted_by_user INT);").unwrap();
        let t = EPOCH_DELTA_US + 1_000_000;
        let ins = |origin: &str, realm: &str, user: &str, blob: Vec<u8>, bl: i32| {
            c.execute("INSERT INTO logins VALUES (?1,?2,?3,?4,?5,?6)", rusqlite::params![origin, realm, user, blob, t, bl]).unwrap();
        };
        ins("https://a.com/login", "https://a.com/", "bob", enc_cbc(&key, "hunter2", b"v10"), 0);
        ins("https://b.com/", "https://b.com/", "amy", b"v20junk".to_vec(), 0);
        ins("https://c.com/", "https://c.com/", "x", enc_cbc(&key, "no", b"v10"), 1);
        ins("android://abc", "android://abc", "x", enc_cbc(&key, "no", b"v10"), 0);
        drop(c);
        let (logins, failed) = read_passwords(d.path(), &NAMES, &Fixed(Keys { v10: Some(key), ..Default::default() })).unwrap();
        if cfg!(target_os = "windows") { return; }
        assert_eq!(failed, 1);
        assert_eq!(logins, vec![ChromiumLogin { origin: "https://a.com/login".into(), username: "bob".into(), password: "hunter2".into(), created_at: 1000 }]);
        let (none, f) = read_passwords(d.path(), &NAMES, &Fixed(Keys::default())).unwrap();
        assert!(none.is_empty() && f == 2);
    }

    #[test]
    fn windows_gcm_roundtrip() {
        use aes_gcm::aead::Aead;
        let key = [7u8; 32];
        let nonce = [1u8; 12];
        let ct = Aes256Gcm::new_from_slice(&key).unwrap().encrypt(Nonce::from_slice(&nonce), b"pw".as_ref()).unwrap();
        let mut blob = b"v10".to_vec();
        blob.extend(nonce);
        blob.extend(ct);
        let keys = Keys { gcm: Some(key), ..Default::default() };
        assert_eq!(decrypt(&blob, &keys, true).as_deref(), Some("pw"));
        blob[0..3].copy_from_slice(b"v20");
        assert_eq!(decrypt(&blob, &keys, true), None);
    }
}

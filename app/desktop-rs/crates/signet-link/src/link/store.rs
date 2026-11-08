//! Links with phones: `link.json` in the app folder (same format as the Electron app).
//! Each link's private key is sealed with a `Vault` (system keychain); of the token only the SHA-256 is kept.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::certs::CertPair;
use super::protocol::LinkDevice;
use crate::b64;
use crate::rand_bytes;

/// Secret encryption. The app implements it with the system keychain.
pub trait Vault: Send + Sync {
    fn available(&self) -> bool;
    fn encrypt(&self, text: &str) -> String;
    fn decrypt(&self, data: &str) -> Option<String>;
}

/// No keychain: keys are stored as `raw:` text.
pub struct NoVault;
impl Vault for NoVault {
    fn available(&self) -> bool {
        false
    }
    fn encrypt(&self, text: &str) -> String {
        text.to_string()
    }
    fn decrypt(&self, data: &str) -> Option<String> {
        Some(data.to_string())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct StoredLink {
    pub link_id: String,
    pub peer: LinkDevice,
    /// This device's switch, on this computer.
    pub enabled: bool,
    pub cert_pem: String,
    pub key_pem: String,
    /// SHA-256 of the token, hexadecimal.
    pub token_hash: String,
    pub created_at: u64,
    pub last_seen: Option<u64>,
    pub peer_paused: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileLink {
    link_id: String,
    peer: LinkDevice,
    #[serde(default = "yes")]
    enabled: bool,
    cert_pem: String,
    key: String,
    token_hash: String,
    #[serde(default)]
    created_at: u64,
    #[serde(default)]
    last_seen: Option<u64>,
    #[serde(default)]
    peer_paused: bool,
}

fn yes() -> bool {
    true
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileDefaultCert {
    cert_pem: String,
    key: String,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct FileData {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    desktop_id: Option<String>,
    #[serde(default)]
    general: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    default_cert: Option<FileDefaultCert>,
    #[serde(default)]
    links: Vec<serde_json::Value>,
}

pub fn hash_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

/// Constant-time comparison of the received token and the stored SHA-256.
pub fn token_matches(token: &str, token_hash: &str) -> bool {
    let a = hash_token(token);
    constant_time_eq(a.as_bytes(), token_hash.as_bytes())
}

pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

pub fn new_token() -> String {
    b64::encode(&rand_bytes::<32>())
}

pub struct LinkStore {
    pub desktop_id: String,
    /// Master switch.
    pub general: bool,
    pub default_cert: Option<CertPair>,
    pub links: Vec<StoredLink>,
    /// Unreadable file: the existing file is never overwritten during the session.
    blocked: bool,
    file: PathBuf,
    vault: Box<dyn Vault>,
}

impl LinkStore {
    pub fn new(file: impl Into<PathBuf>, vault: Box<dyn Vault>) -> Self {
        let mut s = Self {
            desktop_id: hex::encode(rand_bytes::<16>()),
            general: true,
            default_cert: None,
            links: vec![],
            blocked: false,
            file: file.into(),
            vault,
        };
        s.load();
        s
    }

    fn seal(&self, text: &str) -> String {
        if self.vault.available() { format!("enc:{}", self.vault.encrypt(text)) } else { format!("raw:{text}") }
    }

    fn open(&self, data: &str) -> Option<String> {
        if let Some(r) = data.strip_prefix("raw:") {
            return Some(r.to_string());
        }
        self.vault.decrypt(data.strip_prefix("enc:")?)
    }

    fn load(&mut self) {
        let text = match fs::read_to_string(&self.file) {
            Ok(t) => t,
            Err(e) => {
                if e.kind() != std::io::ErrorKind::NotFound {
                    self.blocked = true;
                }
                return;
            }
        };
        let Ok(raw) = serde_json::from_str::<FileData>(&text) else {
            self.blocked = true;
            return;
        };
        if let Some(id) = raw.desktop_id.filter(|s| !s.is_empty()) {
            self.desktop_id = id;
        }
        if let Some(g) = raw.general {
            self.general = g;
        }
        if let Some(dc) = raw.default_cert {
            if let Some(key) = self.open(&dc.key) {
                self.default_cert = Some(CertPair { cert_pem: dc.cert_pem, key_pem: key });
            }
        }
        for l in raw.links {
            let Ok(l) = serde_json::from_value::<FileLink>(l) else { continue };
            let Some(key_pem) = self.open(&l.key) else { continue };
            self.links.push(StoredLink {
                link_id: l.link_id,
                peer: l.peer,
                enabled: l.enabled,
                cert_pem: l.cert_pem,
                key_pem,
                token_hash: l.token_hash,
                created_at: l.created_at,
                last_seen: l.last_seen,
                peer_paused: l.peer_paused,
            });
        }
    }

    fn serialize(&self) -> FileData {
        FileData {
            version: 1,
            desktop_id: Some(self.desktop_id.clone()),
            general: Some(self.general),
            default_cert: self.default_cert.as_ref().map(|c| FileDefaultCert { cert_pem: c.cert_pem.clone(), key: self.seal(&c.key_pem) }),
            links: self
                .links
                .iter()
                .filter_map(|l| {
                    serde_json::to_value(FileLink {
                        link_id: l.link_id.clone(),
                        peer: l.peer.clone(),
                        enabled: l.enabled,
                        cert_pem: l.cert_pem.clone(),
                        key: self.seal(&l.key_pem),
                        token_hash: l.token_hash.clone(),
                        created_at: l.created_at,
                        last_seen: l.last_seen,
                        peer_paused: l.peer_paused,
                    })
                    .ok()
                })
                .collect(),
        }
    }

    /// Atomic write (temporary file then rename). Does nothing if the existing file was unreadable.
    pub fn save(&self) {
        if self.blocked {
            return;
        }
        let Ok(json) = serde_json::to_string(&self.serialize()) else { return };
        let _ = write_atomic(&self.file, json.as_bytes());
    }

    pub fn flush(&self) {
        self.save();
    }

    pub fn get(&self, link_id: &str) -> Option<&StoredLink> {
        self.links.iter().find(|l| l.link_id == link_id)
    }

    pub fn get_mut(&mut self, link_id: &str) -> Option<&mut StoredLink> {
        self.links.iter_mut().find(|l| l.link_id == link_id)
    }

    pub fn add(&mut self, link: StoredLink) {
        self.links.push(link);
        self.save();
    }

    pub fn remove(&mut self, link_id: &str) -> bool {
        let Some(i) = self.links.iter().position(|l| l.link_id == link_id) else { return false };
        self.links.remove(i);
        self.save();
        true
    }
}

fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::link::protocol::LinkPlatform;

    struct TestVault(bool, bool);
    impl Vault for TestVault {
        fn available(&self) -> bool {
            self.0
        }
        fn encrypt(&self, s: &str) -> String {
            b64::encode(s.as_bytes())
        }
        fn decrypt(&self, s: &str) -> Option<String> {
            if self.1 { b64::decode_text(s) } else { None }
        }
    }
    fn vault(available: bool) -> Box<dyn Vault> {
        Box::new(TestVault(available, true))
    }

    fn link(id: &str) -> StoredLink {
        StoredLink {
            link_id: id.into(),
            peer: LinkDevice { id: "phone-1".into(), name: "iPhone".into(), platform: LinkPlatform::Ios },
            enabled: true,
            cert_pem: "CERT".into(),
            key_pem: "SECRET-KEY".into(),
            token_hash: hash_token(&"x".repeat(43)),
            created_at: 1,
            last_seen: None,
            peer_paused: false,
        }
    }

    fn file() -> (tempfile::TempDir, PathBuf) {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("link.json");
        (d, f)
    }

    #[test]
    fn token_is_32_bytes_and_compared_by_hash() {
        let t = new_token();
        assert_eq!(t.len(), 43);
        assert!(t.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'));
        assert!(token_matches(&t, &hash_token(&t)));
        assert!(!token_matches(&new_token(), &hash_token(&t)));
        assert!(!token_matches(&t, "zz"));
    }

    #[test]
    fn first_launch_defaults() {
        let (_d, f) = file();
        let s = LinkStore::new(&f, vault(true));
        assert_eq!(s.desktop_id.len(), 32);
        assert!(s.general);
        assert!(s.links.is_empty());
    }

    #[test]
    fn keeps_id_links_and_switches_with_the_key_encrypted() {
        let (_d, f) = file();
        let mut s = LinkStore::new(&f, vault(true));
        s.add(link("0123456789abcdef"));
        s.general = false;
        s.get_mut("0123456789abcdef").unwrap().enabled = false;
        s.get_mut("0123456789abcdef").unwrap().peer_paused = true;
        s.flush();
        assert!(!fs::read_to_string(&f).unwrap().contains("SECRET-KEY"));
        let again = LinkStore::new(&f, vault(true));
        assert_eq!(again.desktop_id, s.desktop_id);
        assert!(!again.general);
        assert_eq!(again.links.len(), 1);
        let l = &again.links[0];
        assert_eq!(l.key_pem, "SECRET-KEY");
        assert!(!l.enabled && l.peer_paused);
        assert_eq!(l.token_hash, link("a").token_hash);
    }

    #[test]
    fn without_a_keychain_the_key_is_kept_in_plain_text() {
        let (_d, f) = file();
        let mut s = LinkStore::new(&f, vault(false));
        s.add(link("0123456789abcdef"));
        s.flush();
        assert_eq!(LinkStore::new(&f, vault(false)).links[0].key_pem, "SECRET-KEY");
    }

    #[test]
    fn a_link_whose_key_cannot_be_decrypted_is_ignored() {
        let (_d, f) = file();
        let mut s = LinkStore::new(&f, vault(true));
        s.add(link("0123456789abcdef"));
        s.flush();
        assert!(LinkStore::new(&f, Box::new(TestVault(true, false))).links.is_empty());
    }

    #[test]
    fn deleting_a_link_removes_it_from_the_file() {
        let (_d, f) = file();
        let mut s = LinkStore::new(&f, vault(true));
        s.add(link("0123456789abcdef"));
        assert!(s.remove("0123456789abcdef"));
        assert!(!s.remove("0123456789abcdef"));
        s.flush();
        assert!(LinkStore::new(&f, vault(true)).links.is_empty());
    }

    #[test]
    fn unreadable_file_is_not_overwritten() {
        let (_d, f) = file();
        fs::write(&f, "{ pas du json").unwrap();
        let mut s = LinkStore::new(&f, vault(true));
        s.add(link("0123456789abcdef"));
        s.flush();
        assert_eq!(fs::read_to_string(&f).unwrap(), "{ pas du json");
    }

    #[test]
    fn reads_a_file_written_by_the_electron_app() {
        let (_d, f) = file();
        fs::write(
            &f,
            r#"{"version":1,"desktopId":"abc","general":true,"links":[{"linkId":"0123456789abcdef","peer":{"id":"p","name":"N","platform":"ios"},"enabled":true,"certPem":"C","tokenHash":"h","createdAt":5,"lastSeen":null,"peerPaused":false,"key":"raw:K"}]}"#,
        )
        .unwrap();
        let s = LinkStore::new(&f, vault(true));
        assert_eq!(s.desktop_id, "abc");
        assert_eq!(s.links[0].key_pem, "K");
    }
}

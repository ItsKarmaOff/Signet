//! OS keychain access behind a trait (Keychain, Credential Manager, Secret Service) with an in-memory double for tests.
use std::collections::HashMap;
use std::sync::Mutex;

pub trait SecretStore: Send + Sync {
    fn get(&self, service: &str, account: &str) -> Result<Option<Vec<u8>>, String>;
    fn set(&self, service: &str, account: &str, secret: &[u8]) -> Result<(), String>;
    fn delete(&self, service: &str, account: &str) -> Result<(), String>;
}

/// `SIGNET_NO_KEYCHAIN=1` (tests): the OS keychain is never touched, secrets live in memory for the process.
pub fn no_keychain() -> bool {
    std::env::var_os("SIGNET_NO_KEYCHAIN").is_some_and(|v| !v.is_empty() && v != "0")
}

fn memory() -> &'static MemoryStore {
    static M: std::sync::OnceLock<MemoryStore> = std::sync::OnceLock::new();
    M.get_or_init(MemoryStore::default)
}

/// `keyring` backed store. Binary secrets are kept as base64 text.
pub struct KeyringStore;

impl SecretStore for KeyringStore {
    fn get(&self, service: &str, account: &str) -> Result<Option<Vec<u8>>, String> {
        if no_keychain() {
            return memory().get(service, account);
        }
        use base64::Engine;
        let e = keyring::Entry::new(service, account).map_err(|e| e.to_string())?;
        match e.get_password() {
            Ok(p) => base64::engine::general_purpose::STANDARD.decode(p.trim()).map(Some).map_err(|e| e.to_string()),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }
    fn set(&self, service: &str, account: &str, secret: &[u8]) -> Result<(), String> {
        if no_keychain() {
            return memory().set(service, account, secret);
        }
        use base64::Engine;
        let e = keyring::Entry::new(service, account).map_err(|e| e.to_string())?;
        e.set_password(&base64::engine::general_purpose::STANDARD.encode(secret)).map_err(|e| e.to_string())
    }
    fn delete(&self, service: &str, account: &str) -> Result<(), String> {
        if no_keychain() {
            return memory().delete(service, account);
        }
        let e = keyring::Entry::new(service, account).map_err(|e| e.to_string())?;
        match e.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

#[derive(Default)]
pub struct MemoryStore(Mutex<HashMap<(String, String), Vec<u8>>>);

impl SecretStore for MemoryStore {
    fn get(&self, s: &str, a: &str) -> Result<Option<Vec<u8>>, String> {
        Ok(self.0.lock().map_err(|e| e.to_string())?.get(&(s.into(), a.into())).cloned())
    }
    fn set(&self, s: &str, a: &str, v: &[u8]) -> Result<(), String> {
        self.0.lock().map_err(|e| e.to_string())?.insert((s.into(), a.into()), v.to_vec());
        Ok(())
    }
    fn delete(&self, s: &str, a: &str) -> Result<(), String> {
        self.0.lock().map_err(|e| e.to_string())?.remove(&(s.into(), a.into()));
        Ok(())
    }
}

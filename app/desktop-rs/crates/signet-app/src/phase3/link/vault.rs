//! `signet_link::Vault` over the OS keychain (same store as the passwords: `signet_features::secrets`).
//!
//! The link store seals each private key with `encrypt`. Here the key text goes to the keychain under a random id and
//! the sealed value is `kc:<id>`. If the keychain refuses a write, the text itself is kept (`plain:`), which is what
//! the store does without a vault, so a key is never lost.
//! Known gap: the entry of a forgotten link stays in the keychain (a few hundred bytes).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use signet_features::secrets::SecretStore;
use signet_link::Vault;

const SERVICE: &str = "Signet GPUI link";
const PROBE: &str = "probe";

pub struct KeychainVault {
    store: Arc<dyn SecretStore>,
    /// Text to id, so the same key sealed again (every save of the file) reuses its entry.
    ids: Mutex<HashMap<String, String>>,
}

impl KeychainVault {
    pub fn new(store: Arc<dyn SecretStore>) -> Self {
        Self { store, ids: Mutex::new(HashMap::new()) }
    }
}

impl Vault for KeychainVault {
    fn available(&self) -> bool {
        self.store.get(SERVICE, PROBE).is_ok()
    }

    fn encrypt(&self, text: &str) -> String {
        let mut ids = match self.ids.lock() {
            Ok(g) => g,
            Err(_) => return format!("plain:{text}"),
        };
        if let Some(id) = ids.get(text) {
            return format!("kc:{id}");
        }
        let id = signet_core::ids::new_id();
        match self.store.set(SERVICE, &id, text.as_bytes()) {
            Ok(()) => {
                ids.insert(text.to_string(), id.clone());
                format!("kc:{id}")
            }
            Err(_) => format!("plain:{text}"),
        }
    }

    fn decrypt(&self, data: &str) -> Option<String> {
        if let Some(text) = data.strip_prefix("plain:") {
            return Some(text.to_string());
        }
        let id = data.strip_prefix("kc:")?;
        let bytes = self.store.get(SERVICE, id).ok()??;
        let text = String::from_utf8(bytes).ok()?;
        if let Ok(mut ids) = self.ids.lock() {
            ids.insert(text.clone(), id.to_string());
        }
        Some(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use signet_features::secrets::MemoryStore;

    #[test]
    fn round_trip_and_reuse_of_the_entry() {
        let v = KeychainVault::new(Arc::new(MemoryStore::default()));
        assert!(v.available());
        let a = v.encrypt("PRIVATE KEY");
        assert!(a.starts_with("kc:") && !a.contains("PRIVATE"));
        assert_eq!(v.encrypt("PRIVATE KEY"), a);
        assert_ne!(v.encrypt("OTHER"), a);
        assert_eq!(v.decrypt(&a).as_deref(), Some("PRIVATE KEY"));
    }

    #[test]
    fn unknown_or_foreign_data_is_refused() {
        let v = KeychainVault::new(Arc::new(MemoryStore::default()));
        assert_eq!(v.decrypt("kc:missing"), None);
        assert_eq!(v.decrypt("garbage"), None);
        assert_eq!(v.decrypt("plain:abc").as_deref(), Some("abc"));
    }

    #[test]
    fn a_second_session_reads_the_same_entries() {
        let store: Arc<dyn SecretStore> = Arc::new(MemoryStore::default());
        let sealed = KeychainVault::new(store.clone()).encrypt("K");
        let again = KeychainVault::new(store);
        assert_eq!(again.decrypt(&sealed).as_deref(), Some("K"));
        assert_eq!(again.encrypt("K"), sealed);
    }
}

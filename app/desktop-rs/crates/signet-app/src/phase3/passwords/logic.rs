//! Pure rules of the passwords and autofill wiring (no Core, no I/O): easy to test.

use base64::Engine;
use serde_json::Value;
use signet_core::types::PasswordImportError;

/// A prompt shown without the user asking (after a login) tucks itself away after this delay (`PW_PROMPT_MS`).
pub const PROMPT_MS: i64 = 30_000;
/// Delay before the next autofill offer of a tab is shown once the overlay has left (`setTimeout(.., 350)`).
pub const NEXT_OFFER_MS: i64 = 350;
/// Marker of the vaults written by this app (`KeychainCipher`).
const OWN_MARKER: &[u8] = b"sg1";

/// True for a base64 blob that this app did not write (so, one of the Electron app: Chromium `v10`/`v11`).
pub fn is_foreign_blob(b64: &str) -> bool {
    match base64::engine::general_purpose::STANDARD.decode(b64.trim()) {
        Ok(bytes) => !bytes.starts_with(OWN_MARKER),
        Err(_) => false,
    }
}

/// Ids of the logins of a `passwords.json` whose password comes from the Electron app.
pub fn foreign_login_ids(raw: &str) -> Vec<String> {
    entries_of(raw, "logins")
        .into_iter()
        .filter(|e| e.get("password").and_then(Value::as_str).is_some_and(is_foreign_blob))
        .filter_map(|e| e.get("id").and_then(Value::as_str).map(str::to_owned))
        .collect()
}

/// A card of an `autofill.json` written by the Electron app.
#[derive(Debug, Clone, PartialEq)]
pub struct ForeignCard {
    pub id: String,
    pub blob: Vec<u8>,
    pub holder: String,
    pub nickname: String,
    pub exp_month: i64,
    pub exp_year: i64,
}

/// Cards of an `autofill.json` whose number comes from the Electron app.
pub fn foreign_cards(raw: &str) -> Vec<ForeignCard> {
    entries_of(raw, "cards")
        .into_iter()
        .filter_map(|c| {
            let number = c.get("number")?.as_str()?;
            if !is_foreign_blob(number) {
                return None;
            }
            let text = |k: &str| c.get(k).and_then(Value::as_str).unwrap_or_default().to_owned();
            Some(ForeignCard {
                id: c.get("id")?.as_str()?.to_owned(),
                blob: base64::engine::general_purpose::STANDARD.decode(number.trim()).ok()?,
                holder: text("holder"),
                nickname: text("nickname"),
                exp_month: c.get("expMonth").and_then(Value::as_i64).unwrap_or(0),
                exp_year: c.get("expYear").and_then(Value::as_i64).unwrap_or(0),
            })
        })
        .collect()
}

fn entries_of(raw: &str, field: &str) -> Vec<Value> {
    serde_json::from_str::<Value>(raw).ok().and_then(|v| v.get(field).and_then(Value::as_array).cloned()).unwrap_or_default()
}

/// The clipboard still holds the copied password (a trailing line break added by some tools is ignored).
pub fn clipboard_holds(clip: &str, password: &str) -> bool {
    !password.is_empty() && clip.trim_end_matches(['\r', '\n']) == password
}

/// An unsolicited prompt (no anchor) has been up long enough to tuck itself away.
pub fn prompt_expired(anchored: bool, shown_at: i64, now: i64) -> bool {
    !anchored && now - shown_at >= PROMPT_MS
}

/// Import error of the features crate to the one the interface knows.
pub fn import_error(key: &str) -> PasswordImportError {
    match key {
        "encryption" => PasswordImportError::Encryption,
        "read" => PasswordImportError::Read,
        _ => PasswordImportError::Format,
    }
}

/// Password manager setting as the string the vault compares with (`signet` by default).
pub fn manager_name(manager: Option<signet_core::types::PasswordManager>) -> String {
    serde_json::to_value(manager.unwrap_or_default()).ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_else(|| "signet".into())
}

/// Adds an origin to a "never" list once.
pub fn with_origin(list: Option<Vec<String>>, origin: &str) -> Vec<String> {
    let mut v = list.unwrap_or_default();
    if !v.iter().any(|o| o == origin) {
        v.push(origin.to_owned());
    }
    v
}

/// Lowercase JSON name of a serializable enum (`CardNetwork`, `AutofillKind`...).
pub fn enum_name<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_value(v).ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b64(bytes: &[u8]) -> String {
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    #[test]
    fn foreign_blobs_are_the_ones_without_our_marker() {
        assert!(is_foreign_blob(&b64(b"v10abcdef")));
        assert!(!is_foreign_blob(&b64(b"sg1abcdef")));
        assert!(!is_foreign_blob("not base64 !!"));
    }

    #[test]
    fn foreign_logins_are_listed_by_id() {
        let raw = format!(
            r#"{{"version":1,"logins":[{{"id":"a","password":"{}"}},{{"id":"b","password":"{}"}},{{"password":"{}"}}]}}"#,
            b64(b"v10xxxx"),
            b64(b"sg1xxxx"),
            b64(b"v11xxxx")
        );
        assert_eq!(foreign_login_ids(&raw), vec!["a".to_string()]);
        assert!(foreign_login_ids("garbage").is_empty());
    }

    #[test]
    fn foreign_cards_keep_what_is_needed_to_rebuild_them() {
        let raw = format!(
            r#"{{"cards":[{{"id":"c1","number":"{}","holder":"A B","nickname":"n","expMonth":4,"expYear":2030}},{{"id":"c2","number":"{}"}}]}}"#,
            b64(b"v10zzz"),
            b64(b"sg1zzz")
        );
        let cards = foreign_cards(&raw);
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].id, "c1");
        assert_eq!(cards[0].blob, b"v10zzz".to_vec());
        assert_eq!((cards[0].exp_month, cards[0].exp_year), (4, 2030));
        assert_eq!(cards[0].holder, "A B");
    }

    #[test]
    fn clipboard_check_ignores_trailing_breaks_and_empty_passwords() {
        assert!(clipboard_holds("secret\r\n", "secret"));
        assert!(clipboard_holds("secret", "secret"));
        assert!(!clipboard_holds("other", "secret"));
        assert!(!clipboard_holds("", ""));
    }

    #[test]
    fn unanchored_prompts_expire() {
        assert!(!prompt_expired(false, 1000, 1000 + PROMPT_MS - 1));
        assert!(prompt_expired(false, 1000, 1000 + PROMPT_MS));
        assert!(!prompt_expired(true, 0, i64::MAX / 2));
    }

    #[test]
    fn names_and_lists() {
        assert_eq!(import_error("encryption"), PasswordImportError::Encryption);
        assert_eq!(import_error("read"), PasswordImportError::Read);
        assert_eq!(import_error("format"), PasswordImportError::Format);
        assert_eq!(manager_name(None), "signet");
        assert_eq!(manager_name(Some(signet_core::types::PasswordManager::OnePassword)), "1password");
        assert_eq!(with_origin(None, "https://a.com"), vec!["https://a.com".to_string()]);
        assert_eq!(with_origin(Some(vec!["https://a.com".into()]), "https://a.com").len(), 1);
        assert_eq!(enum_name(&signet_core::types::CardNetwork::Visa), "visa");
    }
}

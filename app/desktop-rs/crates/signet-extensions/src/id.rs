//! Extension id computation, the way Chromium does it.
//! An id is the first 16 bytes of the SHA-256 of the public key (DER, SubjectPublicKeyInfo) or, for an unpacked
//! extension without a `key`, of the absolute path of its folder; each nibble becomes a letter `a` to `p`.

use base64::Engine;
use sha2::{Digest, Sha256};
use std::path::Path;

/// 32 letters `a-p` for the first 16 bytes of a digest.
fn encode_id(digest: &[u8]) -> String {
    digest.iter().take(16).flat_map(|b| [b >> 4, b & 0x0f]).map(|n| (b'a' + n) as char).collect()
}

pub fn id_from_bytes(bytes: &[u8]) -> String {
    encode_id(&Sha256::digest(bytes))
}

/// Id of a DER public key (what a CRX header and the manifest `key` field hold).
pub fn id_from_public_key(der: &[u8]) -> String {
    id_from_bytes(der)
}

/// Id from the manifest `key` field (base64 of the DER key).
pub fn id_from_manifest_key(key: &str) -> Option<String> {
    let cleaned: String = key.chars().filter(|c| !c.is_whitespace()).collect();
    let der = base64::engine::general_purpose::STANDARD.decode(cleaned).ok()?;
    if der.is_empty() {
        return None;
    }
    Some(id_from_public_key(&der))
}

/// Id Chromium derives from the folder of an unpacked extension (absolute, symlinks resolved when the folder exists).
/// POSIX hashes the path bytes, Windows the UTF-16LE code units.
pub fn id_from_path(path: &Path) -> String {
    let abs = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let s = abs.to_string_lossy();
        // canonicalize returns a verbatim path (\\?\C:\...): Chromium hashes the plain one.
        let plain = s.strip_prefix(r"\\?\").unwrap_or(&s);
        let bytes: Vec<u8> = std::ffi::OsStr::new(plain).encode_wide().flat_map(|u| u.to_le_bytes()).collect();
        id_from_bytes(&bytes)
    }
    #[cfg(not(windows))]
    {
        use std::os::unix::ffi::OsStrExt;
        id_from_bytes(abs.as_os_str().as_bytes())
    }
}

/// True for a well-formed extension id.
pub fn is_valid_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| (b'a'..=b'p').contains(&b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_is_32_letters_a_to_p() {
        let id = id_from_bytes(b"hello");
        assert!(is_valid_id(&id));
        // sha256("hello") = 2cf24dba5fb0a30e26e83b2ac5b9e29e...: nibbles 2,c,f,2,4,d,b,a become c,m,p,c,e,n,l,k
        assert_eq!(&id[..8], "cmpcenlk");
    }

    #[test]
    fn manifest_key_ignores_whitespace_and_rejects_garbage() {
        let der = [1u8, 2, 3, 4, 5, 6, 7, 8, 9];
        let b64 = base64::engine::general_purpose::STANDARD.encode(der);
        let spaced = format!("{}\n{}", &b64[..4], &b64[4..]);
        assert_eq!(id_from_manifest_key(&spaced), Some(id_from_public_key(&der)));
        assert_eq!(id_from_manifest_key("!!!"), None);
        assert_eq!(id_from_manifest_key(""), None);
    }

    #[test]
    fn path_ids_differ_per_folder_and_are_stable() {
        let a = id_from_path(Path::new("/definitely/not/here/a"));
        assert_eq!(a, id_from_path(Path::new("/definitely/not/here/a")));
        assert_ne!(a, id_from_path(Path::new("/definitely/not/here/b")));
        assert!(is_valid_id(&a));
    }

    #[test]
    fn validity_check() {
        assert!(!is_valid_id("abc"));
        assert!(!is_valid_id(&"q".repeat(32)));
        assert!(is_valid_id(&"a".repeat(32)));
    }
}

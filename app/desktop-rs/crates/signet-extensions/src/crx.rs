//! Chrome Web Store downloads and CRX files: URLs, CRX2/CRX3 parsing with signature and id verification, unzip, install
//! layout and update checks. New code (the Electron app delegates this to `electron-chrome-web-store`).
//!
//! No network inside: downloads go through the [`Fetcher`] trait the app implements (CEF network stack or an HTTP client).

use crate::id::id_from_public_key;
use base64::Engine;
use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
use rsa::pkcs1v15::{Signature as RsaSignature, VerifyingKey as RsaVerifyingKey};
use rsa::pkcs8::DecodePublicKey;
use rsa::signature::Verifier;
use rsa::RsaPublicKey;
use serde_json::Value;
use sha2::Sha256;
use std::fs;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum CrxError {
    #[error("not a CRX file")]
    BadMagic,
    #[error("unsupported CRX version {0}")]
    Version(u32),
    #[error("truncated CRX file")]
    Truncated,
    #[error("malformed CRX header: {0}")]
    Header(&'static str),
    #[error("CRX signature is invalid")]
    BadSignature,
    #[error("CRX id does not match the signing key")]
    IdMismatch,
    #[error("CRX is for extension {found}, expected {expected}")]
    UnexpectedId { expected: String, found: String },
    #[error("archive error: {0}")]
    Archive(String),
    #[error("manifest error: {0}")]
    Manifest(String),
    #[error("download failed: {0}")]
    Download(String),
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
}

/// Version string Chrome sends as `prodversion` (the Web Store refuses very old ones).
pub const DEFAULT_PRODVERSION: &str = "156.0.7632.0";

/// Download URL of an extension (the server answers with a redirect to the CRX).
pub fn crx_download_url(id: &str, prodversion: &str) -> String {
    let x = format!("id={id}&installsource=ondemand&uc");
    format!(
        "https://clients2.google.com/service/update2/crx?response=redirect&acceptformat=crx2,crx3&prodversion={prodversion}&x={}",
        utf8_percent_encode(&x, NON_ALPHANUMERIC)
    )
}

/// Update check URL for installed extensions `(id, version)`; the answer is the XML read by [`parse_update_response`].
pub fn update_check_url(installed: &[(String, String)], prodversion: &str) -> String {
    let mut url = format!("https://clients2.google.com/service/update2/crx?acceptformat=crx2,crx3&prodversion={prodversion}");
    for (id, version) in installed {
        let x = format!("id={id}&v={version}&uc");
        url.push_str("&x=");
        url.push_str(&utf8_percent_encode(&x, NON_ALPHANUMERIC).to_string());
    }
    url
}

/// An update announced by the store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateInfo {
    pub id: String,
    pub version: String,
    pub codebase: String,
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let needle = format!("{name}=\"");
    let start = tag.find(&needle)? + needle.len();
    let end = tag[start..].find('"')? + start;
    Some(tag[start..end].replace("&amp;", "&"))
}

/// Reads the `gupdate` XML: one entry per `<app>` whose `<updatecheck>` has `status="ok"` and a codebase.
pub fn parse_update_response(xml: &str) -> Vec<UpdateInfo> {
    let mut out = Vec::new();
    for app in xml.split("<app ").skip(1) {
        let app = app.split("</app>").next().unwrap_or(app);
        let Some(id) = attr(app, "appid") else { continue };
        let Some(check) = app.find("<updatecheck").map(|i| &app[i..]) else { continue };
        let tag = check.split('>').next().unwrap_or(check);
        if attr(tag, "status").as_deref().is_some_and(|s| s != "ok") {
            continue;
        }
        if let (Some(codebase), Some(version)) = (attr(tag, "codebase"), attr(tag, "version")) {
            out.push(UpdateInfo { id, version, codebase });
        }
    }
    out
}

/// Extension id in a Web Store address (`.../detail/<slug>/<id>`, old or new store), if it is one.
pub fn store_extension_id(url: &str) -> Option<String> {
    let u = url::Url::parse(url).ok()?;
    let host = u.host_str()?;
    let ok = (host == "chromewebstore.google.com") || (host == "chrome.google.com" && u.path().starts_with("/webstore"));
    if !ok {
        return None;
    }
    u.path_segments()?.rev().find(|s| crate::id::is_valid_id(s)).map(String::from)
}

/// True for the pages of the Web Store (where the install bridge is injected).
pub fn is_store_url(url: &str) -> bool {
    url::Url::parse(url).ok().is_some_and(|u| {
        u.scheme() == "https" && (u.host_str() == Some("chromewebstore.google.com") || (u.host_str() == Some("chrome.google.com") && u.path().starts_with("/webstore")))
    })
}

/// Downloads bytes. Implemented by the app; tests use a fake.
pub trait Fetcher {
    fn get(&self, url: &str) -> Result<Vec<u8>, String>;
}

// ---- protobuf (just enough for the CRX3 header) ----

fn read_varint(buf: &[u8], pos: &mut usize) -> Option<u64> {
    let mut v = 0u64;
    let mut shift = 0;
    loop {
        let b = *buf.get(*pos)?;
        *pos += 1;
        v |= u64::from(b & 0x7f) << shift;
        if b & 0x80 == 0 {
            return Some(v);
        }
        shift += 7;
        if shift > 63 {
            return None;
        }
    }
}

/// `(field number, payload)` of every length-delimited field; other wire types are skipped.
fn fields(buf: &[u8]) -> Result<Vec<(u64, &[u8])>, CrxError> {
    let mut out = Vec::new();
    let mut pos = 0;
    while pos < buf.len() {
        let key = read_varint(buf, &mut pos).ok_or(CrxError::Header("bad key"))?;
        let (num, wire) = (key >> 3, key & 7);
        match wire {
            0 => {
                read_varint(buf, &mut pos).ok_or(CrxError::Header("bad varint"))?;
            }
            1 => pos += 8,
            5 => pos += 4,
            2 => {
                let len = read_varint(buf, &mut pos).ok_or(CrxError::Header("bad length"))? as usize;
                let end = pos.checked_add(len).filter(|e| *e <= buf.len()).ok_or(CrxError::Header("length out of range"))?;
                out.push((num, &buf[pos..end]));
                pos = end;
            }
            _ => return Err(CrxError::Header("unknown wire type")),
        }
    }
    Ok(out)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Crx {
    pub format: u32,
    /// Extension id derived from the signing key.
    pub id: String,
    /// DER public key (SubjectPublicKeyInfo) that defines the id.
    pub public_key: Vec<u8>,
    /// Offset of the ZIP archive in the file.
    pub zip_offset: usize,
}

impl Crx {
    pub fn key_base64(&self) -> String {
        base64::engine::general_purpose::STANDARD.encode(&self.public_key)
    }
}

fn le32(buf: &[u8], at: usize) -> Result<u32, CrxError> {
    buf.get(at..at + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])).ok_or(CrxError::Truncated)
}

fn verify_rsa_sha256(pub_der: &[u8], msg: &[u8], sig: &[u8]) -> bool {
    let Ok(key) = RsaPublicKey::from_public_key_der(pub_der) else { return false };
    let Ok(sig) = RsaSignature::try_from(sig) else { return false };
    RsaVerifyingKey::<Sha256>::new(key).verify(msg, &sig).is_ok()
}

fn verify_rsa_sha1(pub_der: &[u8], msg: &[u8], sig: &[u8]) -> bool {
    let Ok(key) = RsaPublicKey::from_public_key_der(pub_der) else { return false };
    let Ok(sig) = RsaSignature::try_from(sig) else { return false };
    RsaVerifyingKey::<sha1::Sha1>::new(key).verify(msg, &sig).is_ok()
}

fn verify_ecdsa_sha256(pub_der: &[u8], msg: &[u8], sig: &[u8]) -> bool {
    use p256::ecdsa::{signature::Verifier as _, Signature, VerifyingKey};
    let Ok(key) = VerifyingKey::from_public_key_der(pub_der) else { return false };
    let Ok(sig) = Signature::from_der(sig) else { return false };
    key.verify(msg, &sig).is_ok()
}

/// Parses a CRX2 or CRX3 file and verifies its signatures and id. Does not touch the archive.
pub fn parse_crx(bytes: &[u8]) -> Result<Crx, CrxError> {
    if bytes.get(..4) != Some(b"Cr24") {
        return Err(CrxError::BadMagic);
    }
    match le32(bytes, 4)? {
        2 => {
            let (pub_len, sig_len) = (le32(bytes, 8)? as usize, le32(bytes, 12)? as usize);
            let key_end = 16usize.checked_add(pub_len).ok_or(CrxError::Truncated)?;
            let sig_end = key_end.checked_add(sig_len).ok_or(CrxError::Truncated)?;
            if sig_end > bytes.len() {
                return Err(CrxError::Truncated);
            }
            let (key, sig, archive) = (&bytes[16..key_end], &bytes[key_end..sig_end], &bytes[sig_end..]);
            if !verify_rsa_sha1(key, archive, sig) {
                return Err(CrxError::BadSignature);
            }
            Ok(Crx { format: 2, id: id_from_public_key(key), public_key: key.to_vec(), zip_offset: sig_end })
        }
        3 => {
            let header_len = le32(bytes, 8)? as usize;
            let zip_offset = 12usize.checked_add(header_len).filter(|e| *e <= bytes.len()).ok_or(CrxError::Truncated)?;
            let header = &bytes[12..zip_offset];
            let archive = &bytes[zip_offset..];
            let mut signed_header: &[u8] = &[];
            let mut proofs: Vec<(bool, &[u8])> = Vec::new(); // (is_rsa, proof)
            for (num, payload) in fields(header)? {
                match num {
                    2 => proofs.push((true, payload)),
                    3 => proofs.push((false, payload)),
                    10000 => signed_header = payload,
                    _ => {}
                }
            }
            if proofs.is_empty() {
                return Err(CrxError::Header("no signature"));
            }
            let crx_id = fields(signed_header)?.into_iter().find(|(n, _)| *n == 1).map(|(_, p)| p.to_vec()).ok_or(CrxError::Header("no crx_id"))?;
            let mut msg = b"CRX3 SignedData\0".to_vec();
            msg.extend_from_slice(&(signed_header.len() as u32).to_le_bytes());
            msg.extend_from_slice(signed_header);
            msg.extend_from_slice(archive);
            let mut matching: Option<Vec<u8>> = None;
            for (is_rsa, proof) in proofs {
                let parts = fields(proof)?;
                let key = parts.iter().find(|(n, _)| *n == 1).map(|(_, p)| *p).ok_or(CrxError::Header("proof without key"))?;
                let sig = parts.iter().find(|(n, _)| *n == 2).map(|(_, p)| *p).ok_or(CrxError::Header("proof without signature"))?;
                let ok = if is_rsa { verify_rsa_sha256(key, &msg, sig) } else { verify_ecdsa_sha256(key, &msg, sig) };
                if !ok {
                    return Err(CrxError::BadSignature);
                }
                use sha2::Digest;
                if Sha256::digest(key)[..16] == crx_id[..] {
                    matching = Some(key.to_vec());
                }
            }
            let key = matching.ok_or(CrxError::IdMismatch)?;
            Ok(Crx { format: 3, id: id_from_public_key(&key), public_key: key, zip_offset })
        }
        v => Err(CrxError::Version(v)),
    }
}

/// Largest total size of an extracted extension (zip bombs).
pub const MAX_UNPACKED_BYTES: u64 = 512 * 1024 * 1024;

/// Extracts a ZIP archive into `dest` (created). Entries that would leave `dest` are refused.
pub fn unzip(archive: &[u8], dest: &Path) -> Result<(), CrxError> {
    let mut zip = zip::ZipArchive::new(Cursor::new(archive)).map_err(|e| CrxError::Archive(e.to_string()))?;
    fs::create_dir_all(dest)?;
    let mut total = 0u64;
    for i in 0..zip.len() {
        let mut file = zip.by_index(i).map_err(|e| CrxError::Archive(e.to_string()))?;
        let Some(rel) = file.enclosed_name() else { return Err(CrxError::Archive(format!("unsafe path in archive: {:?}", file.name()))) };
        let out = dest.join(rel);
        if file.is_dir() {
            fs::create_dir_all(&out)?;
            continue;
        }
        total = total.saturating_add(file.size());
        if total > MAX_UNPACKED_BYTES {
            return Err(CrxError::Archive("archive is too large".into()));
        }
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut data = Vec::with_capacity(file.size().min(16 * 1024 * 1024) as usize);
        file.by_ref().take(MAX_UNPACKED_BYTES).read_to_end(&mut data)?;
        fs::write(&out, data)?;
    }
    Ok(())
}

/// Adds the signing key to the extracted manifest (as the store installer does) so that an unpacked load of the folder
/// gets the store id.
pub fn write_key_into_manifest(dir: &Path, key_base64: &str) -> Result<(), CrxError> {
    let path = dir.join("manifest.json");
    let text = fs::read_to_string(&path)?;
    let mut value: Value = serde_json::from_str(&crate::manifest::strip_json_comments(&text)).map_err(|e| CrxError::Manifest(e.to_string()))?;
    let obj = value.as_object_mut().ok_or_else(|| CrxError::Manifest("not an object".into()))?;
    if !obj.contains_key("key") {
        obj.insert("key".into(), Value::String(key_base64.into()));
        fs::write(&path, serde_json::to_vec_pretty(&value).map_err(|e| CrxError::Manifest(e.to_string()))?)?;
    }
    Ok(())
}

/// Folder of an installed store extension: `<extensions>/<id>/<version>_0` (the layout Electron's installer used).
pub fn install_dir(extensions_dir: &Path, id: &str, version: &str) -> PathBuf {
    extensions_dir.join(id).join(format!("{version}_0"))
}

#[derive(Debug, Clone, PartialEq)]
pub struct Installed {
    pub id: String,
    pub version: String,
    pub name: String,
    pub dir: PathBuf,
}

/// Verifies a CRX, extracts it to `<extensions>/<id>/<version>_0` (atomically, replacing a previous copy of the same
/// version) and returns where. `expected_id` guards against the store serving another extension.
pub fn install_crx(bytes: &[u8], extensions_dir: &Path, expected_id: Option<&str>) -> Result<Installed, CrxError> {
    let crx = parse_crx(bytes)?;
    if let Some(expected) = expected_id {
        if expected != crx.id {
            return Err(CrxError::UnexpectedId { expected: expected.into(), found: crx.id });
        }
    }
    fs::create_dir_all(extensions_dir)?;
    let staging = tempfile_dir(extensions_dir, &crx.id)?;
    let result = (|| -> Result<Installed, CrxError> {
        unzip(&bytes[crx.zip_offset..], &staging)?;
        write_key_into_manifest(&staging, &crx.key_base64())?;
        let text = fs::read_to_string(staging.join("manifest.json")).map_err(|_| CrxError::Manifest("the archive has no manifest.json".into()))?;
        let manifest = crate::manifest::Manifest::parse(&text).map_err(|e| CrxError::Manifest(e.0))?;
        let dest = install_dir(extensions_dir, &crx.id, &manifest.version);
        if dest.exists() {
            fs::remove_dir_all(&dest)?;
        }
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::rename(&staging, &dest)?;
        Ok(Installed { id: crx.id.clone(), version: manifest.version, name: manifest.name, dir: dest })
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    result
}

fn tempfile_dir(base: &Path, id: &str) -> Result<PathBuf, CrxError> {
    let dir = base.join(format!(".staging-{id}-{}", std::process::id()));
    if dir.exists() {
        fs::remove_dir_all(&dir)?;
    }
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Downloads, verifies and installs an extension from the Web Store by id.
pub fn install_from_store(fetcher: &dyn Fetcher, id: &str, prodversion: &str, extensions_dir: &Path) -> Result<Installed, CrxError> {
    if !crate::id::is_valid_id(id) {
        return Err(CrxError::Download(format!("invalid extension id {id}")));
    }
    let bytes = fetcher.get(&crx_download_url(id, prodversion)).map_err(CrxError::Download)?;
    install_crx(&bytes, extensions_dir, Some(id))
}

/// Asks the store which installed extensions have a newer version, then downloads and installs them. Returns what was updated.
pub fn update_from_store(fetcher: &dyn Fetcher, installed: &[(String, String)], prodversion: &str, extensions_dir: &Path) -> Vec<Result<Installed, CrxError>> {
    if installed.is_empty() {
        return vec![];
    }
    let xml = match fetcher.get(&update_check_url(installed, prodversion)) {
        Ok(b) => String::from_utf8_lossy(&b).into_owned(),
        Err(e) => return vec![Err(CrxError::Download(e))],
    };
    let mut out = Vec::new();
    for u in parse_update_response(&xml) {
        let current = installed.iter().find(|(id, _)| *id == u.id).map(|(_, v)| v.as_str());
        if current.is_none_or(|c| crate::manifest::compare_versions(&u.version, c) != std::cmp::Ordering::Greater) {
            continue;
        }
        out.push(fetcher.get(&u.codebase).map_err(CrxError::Download).and_then(|b| install_crx(&b, extensions_dir, Some(&u.id))));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    const RSA3: &[u8] = include_bytes!("../tests/fixtures/rsa3.crx");
    const ECDSA3: &[u8] = include_bytes!("../tests/fixtures/ecdsa3.crx");
    const RSA2: &[u8] = include_bytes!("../tests/fixtures/rsa2.crx");

    fn ids() -> HashMap<String, String> {
        serde_json::from_str(include_str!("../tests/fixtures/ids.json")).unwrap()
    }

    #[test]
    fn download_url_format() {
        let url = crx_download_url("mpedjdfgkmlmhabmbdmnledbjbnmdfgb", "156.0.7632.0");
        assert_eq!(
            url,
            "https://clients2.google.com/service/update2/crx?response=redirect&acceptformat=crx2,crx3&prodversion=156.0.7632.0&x=id%3Dmpedjdfgkmlmhabmbdmnledbjbnmdfgb%26installsource%3Dondemand%26uc"
        );
    }

    #[test]
    fn update_check_url_lists_every_extension() {
        let url = update_check_url(&[("a".repeat(32), "1.0".into()), ("b".repeat(32), "2.1".into())], "156.0.1.0");
        assert_eq!(url.matches("&x=").count(), 2);
        assert!(url.contains("id%3Daaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa%26v%3D1%2E0%26uc") || url.contains("%26v%3D1.0%26uc"));
    }

    #[test]
    fn parses_the_update_response() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?><gupdate xmlns="http://www.google.com/update2/response" protocol="2.0">
            <app appid="aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" status="ok"><updatecheck codebase="https://x.example/a.crx?a=1&amp;b=2" fp="1" version="2.0.0" status="ok"/></app>
            <app appid="bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb" status="ok"><updatecheck status="noupdate"/></app></gupdate>"#;
        let u = parse_update_response(xml);
        assert_eq!(u, vec![UpdateInfo { id: "a".repeat(32), version: "2.0.0".into(), codebase: "https://x.example/a.crx?a=1&b=2".into() }]);
    }

    #[test]
    fn finds_the_extension_id_in_store_urls() {
        let id = "mpedjdfgkmlmhabmbdmnledbjbnmdfgb";
        assert_eq!(store_extension_id(&format!("https://chromewebstore.google.com/detail/some-name/{id}")).as_deref(), Some(id));
        assert_eq!(store_extension_id(&format!("https://chromewebstore.google.com/detail/some-name/{id}?hl=fr")).as_deref(), Some(id));
        assert_eq!(store_extension_id(&format!("https://chrome.google.com/webstore/detail/some-name/{id}")).as_deref(), Some(id));
        assert_eq!(store_extension_id(&format!("https://evil.example/detail/x/{id}")), None);
        assert!(is_store_url("https://chromewebstore.google.com/"));
        assert!(!is_store_url("http://chromewebstore.google.com/"));
        assert!(!is_store_url("https://chrome.google.com/other"));
    }

    #[test]
    fn crx3_rsa_is_verified_and_the_id_comes_from_the_key() {
        let crx = parse_crx(RSA3).unwrap();
        assert_eq!(crx.format, 3);
        assert_eq!(crx.id, ids()["rsa3.crx"]);
        assert!(!crx.public_key.is_empty());
    }

    #[test]
    fn crx3_ecdsa_is_verified() {
        assert_eq!(parse_crx(ECDSA3).unwrap().id, ids()["ecdsa3.crx"]);
    }

    #[test]
    fn crx2_is_verified() {
        let crx = parse_crx(RSA2).unwrap();
        assert_eq!((crx.format, crx.id), (2, ids()["rsa2.crx"].clone()));
    }

    #[test]
    fn a_tampered_archive_fails_the_signature() {
        for original in [RSA3, ECDSA3, RSA2] {
            let mut bytes = original.to_vec();
            let last = bytes.len() - 5;
            bytes[last] ^= 0xff;
            assert!(matches!(parse_crx(&bytes), Err(CrxError::BadSignature)), "{:?}", parse_crx(&bytes));
        }
    }

    #[test]
    fn malformed_files_are_refused() {
        assert!(matches!(parse_crx(b"nope"), Err(CrxError::BadMagic)));
        assert!(matches!(parse_crx(b"Cr24\x09\0\0\0"), Err(CrxError::Version(9))));
        assert!(matches!(parse_crx(&RSA3[..20]), Err(CrxError::Truncated)));
        let mut bad_id = RSA3.to_vec();
        // Flip a byte inside the signed crx_id: signature check fails before the id check.
        bad_id[40] ^= 1;
        assert!(parse_crx(&bad_id).is_err());
    }

    #[test]
    fn install_extracts_to_the_id_version_layout_and_stores_the_key() {
        let dir = tempfile::tempdir().unwrap();
        let installed = install_crx(RSA3, dir.path(), Some(&ids()["rsa3.crx"])).unwrap();
        assert_eq!((installed.name.as_str(), installed.version.as_str()), ("Fixture", "1.0.0"));
        assert_eq!(installed.dir, dir.path().join(&installed.id).join("1.0.0_0"));
        assert!(installed.dir.join("popup.html").exists());
        let manifest: Value = serde_json::from_str(&fs::read_to_string(installed.dir.join("manifest.json")).unwrap()).unwrap();
        let key = manifest["key"].as_str().unwrap();
        assert_eq!(crate::id::id_from_manifest_key(key).unwrap(), installed.id);
        // Installing again replaces the same folder.
        install_crx(RSA3, dir.path(), None).unwrap();
        // No staging folder is left behind.
        assert!(fs::read_dir(dir.path()).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().starts_with(".staging")));
    }

    #[test]
    fn install_refuses_another_extension_than_expected() {
        let dir = tempfile::tempdir().unwrap();
        let err = install_crx(RSA3, dir.path(), Some(&"a".repeat(32))).unwrap_err();
        assert!(matches!(err, CrxError::UnexpectedId { .. }));
        assert!(fs::read_dir(dir.path()).unwrap().next().is_none());
    }

    #[test]
    fn unzip_refuses_paths_that_leave_the_folder() {
        use std::io::Write;
        let mut buf = Vec::new();
        {
            let mut z = zip::ZipWriter::new(Cursor::new(&mut buf));
            z.start_file("../evil.txt", zip::write::SimpleFileOptions::default()).unwrap();
            z.write_all(b"x").unwrap();
            z.finish().unwrap();
        }
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("out");
        assert!(matches!(unzip(&buf, &dest), Err(CrxError::Archive(_))));
        assert!(!dir.path().join("evil.txt").exists());
    }

    struct Fake {
        responses: HashMap<String, Vec<u8>>,
    }
    impl Fetcher for Fake {
        fn get(&self, url: &str) -> Result<Vec<u8>, String> {
            self.responses.get(url).cloned().ok_or_else(|| format!("no response for {url}"))
        }
    }

    #[test]
    fn installs_from_the_store_through_the_fetcher() {
        let id = ids()["rsa3.crx"].clone();
        let fake = Fake { responses: HashMap::from([(crx_download_url(&id, DEFAULT_PRODVERSION), RSA3.to_vec())]) };
        let dir = tempfile::tempdir().unwrap();
        let installed = install_from_store(&fake, &id, DEFAULT_PRODVERSION, dir.path()).unwrap();
        assert_eq!(installed.id, id);
        assert!(matches!(install_from_store(&fake, &"c".repeat(32), DEFAULT_PRODVERSION, dir.path()), Err(CrxError::Download(_))));
        assert!(matches!(install_from_store(&fake, "bad", DEFAULT_PRODVERSION, dir.path()), Err(CrxError::Download(_))));
    }

    #[test]
    fn updates_only_newer_versions() {
        let id = ids()["rsa3.crx"].clone();
        let installed = vec![(id.clone(), "0.9".to_string())];
        let xml = format!(r#"<gupdate><app appid="{id}"><updatecheck codebase="https://dl.example/x.crx" version="1.0.0" status="ok"/></app></gupdate>"#);
        let fake = Fake { responses: HashMap::from([(update_check_url(&installed, DEFAULT_PRODVERSION), xml.clone().into_bytes()), ("https://dl.example/x.crx".to_string(), RSA3.to_vec())]) };
        let dir = tempfile::tempdir().unwrap();
        let out = update_from_store(&fake, &installed, DEFAULT_PRODVERSION, dir.path());
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].as_ref().unwrap().version, "1.0.0");
        // Already up to date: nothing downloaded.
        let current = vec![(id.clone(), "1.0.0".to_string())];
        let fake2 = Fake { responses: HashMap::from([(update_check_url(&current, DEFAULT_PRODVERSION), xml.into_bytes())]) };
        assert!(update_from_store(&fake2, &current, DEFAULT_PRODVERSION, dir.path()).is_empty());
    }
}

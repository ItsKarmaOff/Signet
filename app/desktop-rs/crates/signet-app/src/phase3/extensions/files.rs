//! Files of the `chrome-extension://` scheme: served from the extension folders, with path checks and
//! `web_accessible_resources` for requests made by web pages. Runs on a CEF IO thread.

use std::collections::HashMap;
use std::path::PathBuf;

use signet_extensions::contentscripts::glob_matches;
use signet_extensions::manifest::{BackgroundEntry, Manifest};
use signet_extensions::urlmatch::{extension_file_path, url_matches};
use signet_web::{ExtensionRequest, ExtensionResponse};

use super::bridge::{background_page, extension_id_of, mime_of, BG_PAGE};

/// An extension the engine serves.
#[derive(Clone)]
pub struct Loaded {
    pub dir: PathBuf,
    pub manifest: Manifest,
    pub background: BackgroundEntry,
}

fn response(mime: &str, body: Vec<u8>, status: u16, headers: Vec<(String, String)>) -> ExtensionResponse {
    ExtensionResponse { mime: mime.into(), body, status, headers }
}

/// Is the referrer a page of the extension itself or absent (a view the app opened, the address bar)?
fn is_own_origin(referrer: &str, id: &str) -> bool {
    referrer.is_empty() || referrer.starts_with(&format!("chrome-extension://{id}"))
}

/// May a web page at `referrer` load `path` (without the leading slash)?
pub fn web_accessible(manifest: &Manifest, path: &str, referrer: &str) -> bool {
    manifest.web_accessible_resources().iter().any(|(resources, matches)| {
        resources.iter().any(|r| glob_matches(r.trim_start_matches('/'), path)) && (matches.is_empty() || matches.iter().any(|m| url_matches(m, referrer) || url_matches(m, &format!("{}/", referrer.trim_end_matches('/')))))
    })
}

/// `%xx` escapes of a URL path (invalid UTF-8 is refused).
fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

pub fn serve(loaded: &HashMap<String, Loaded>, req: &ExtensionRequest) -> Option<ExtensionResponse> {
    let id = extension_id_of(&req.url)?;
    let ext = loaded.get(&id)?;
    let url = url::Url::parse(&req.url).ok()?;
    let path = percent_decode(url.path())?;
    let rel = path.trim_start_matches('/');
    let own = is_own_origin(&req.referrer, &id);
    if !own && !web_accessible(&ext.manifest, rel, &req.referrer) {
        return Some(response("text/plain", b"Denied".to_vec(), 403, vec![]));
    }
    // Extension pages run under the alias scheme, so even their own fetches are cross-origin.
    let cors = vec![("Access-Control-Allow-Origin".to_string(), "*".to_string())];
    if rel.is_empty() {
        return None;
    }
    if rel == BG_PAGE {
        let html = background_page(&ext.background)?;
        return Some(response("text/html; charset=utf-8", html.into_bytes(), 200, cors));
    }
    let file = extension_file_path(&ext.dir, &id, &path)?;
    // `canonicalize` also refuses links that point outside the extension folder.
    let real = std::fs::canonicalize(&file).ok()?;
    let root = std::fs::canonicalize(&ext.dir).ok()?;
    if !real.starts_with(&root) || !real.is_file() {
        return None;
    }
    let body = std::fs::read(&real).ok()?;
    Some(response(mime_of(rel), body, 200, cors))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const ID: &str = "abcdefghijklmnopabcdefghijklmnop";

    fn setup(manifest: serde_json::Value) -> (tempfile::TempDir, HashMap<String, Loaded>) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("popup.html"), "<p>hi</p>").unwrap();
        std::fs::create_dir_all(dir.path().join("img")).unwrap();
        std::fs::write(dir.path().join("img/a.png"), [1, 2, 3]).unwrap();
        let m = Manifest::from_value(manifest).unwrap();
        let background = m.background_entry();
        let mut map = HashMap::new();
        map.insert(ID.to_string(), Loaded { dir: dir.path().to_path_buf(), manifest: m, background });
        (dir, map)
    }

    fn req(url: &str, referrer: &str) -> ExtensionRequest {
        ExtensionRequest { url: url.into(), referrer: referrer.into(), method: "GET".into() }
    }

    fn mv3() -> serde_json::Value {
        json!({ "manifest_version": 3, "name": "T", "version": "1.0", "background": { "service_worker": "sw.js" },
                "web_accessible_resources": [{ "resources": ["img/*"], "matches": ["https://example.com/*"] }] })
    }

    #[test]
    fn own_pages_read_any_file_of_the_folder() {
        let (_d, map) = setup(mv3());
        let r = serve(&map, &req(&format!("chrome-extension://{ID}/popup.html"), "")).unwrap();
        assert_eq!((r.status, r.body.as_slice(), r.mime.as_str()), (200, b"<p>hi</p>".as_slice(), "text/html; charset=utf-8"));
        assert!(serve(&map, &req(&format!("chrome-extension://{ID}/missing.js"), &format!("chrome-extension://{ID}/popup.html"))).is_none());
    }

    #[test]
    fn traversal_and_other_extensions_are_refused() {
        let (_d, map) = setup(mv3());
        assert!(serve(&map, &req(&format!("chrome-extension://{ID}/../../etc/passwd"), "")).is_none());
        assert!(serve(&map, &req(&format!("chrome-extension://{ID}/%2e%2e/%2e%2e/etc/passwd"), "")).is_none());
        assert!(serve(&map, &req("chrome-extension://zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz/popup.html", "")).is_none());
    }

    #[test]
    fn web_pages_only_reach_web_accessible_resources() {
        let (_d, map) = setup(mv3());
        let from_site = "https://example.com/page";
        assert_eq!(serve(&map, &req(&format!("chrome-extension://{ID}/popup.html"), from_site)).unwrap().status, 403);
        let ok = serve(&map, &req(&format!("chrome-extension://{ID}/img/a.png"), from_site)).unwrap();
        assert_eq!((ok.status, ok.mime.as_str()), (200, "image/png"));
        assert!(ok.headers.iter().any(|(k, _)| k == "Access-Control-Allow-Origin"));
        // The match list limits the sites too.
        assert_eq!(serve(&map, &req(&format!("chrome-extension://{ID}/img/a.png"), "https://other.org/")).unwrap().status, 403);
    }

    #[test]
    fn the_generated_background_page_is_served() {
        let (_d, map) = setup(mv3());
        let r = serve(&map, &req(&format!("chrome-extension://{ID}/{BG_PAGE}"), "")).unwrap();
        assert!(String::from_utf8(r.body).unwrap().contains("src=\"/sw.js\""));
    }

    #[cfg(unix)]
    #[test]
    fn links_leaving_the_folder_are_refused() {
        let (d, map) = setup(mv3());
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret.txt"), "x").unwrap();
        std::os::unix::fs::symlink(outside.path().join("secret.txt"), d.path().join("link.txt")).unwrap();
        assert!(serve(&map, &req(&format!("chrome-extension://{ID}/link.txt"), "")).is_none());
    }
}

//! Fallback favicons (port of the cache rules of `main/web/favicons.ts`): fetched when the interface cannot load
//! them itself, kept as data URLs with a bounded cache; failures are retried after a minute.
use std::collections::VecDeque;
use std::time::Duration;

use base64::Engine;

use crate::http::Http;
use crate::util::now_ms;

/// Maximum size of a favicon, and number of entries kept in memory.
pub const MAX_BYTES: usize = 512 * 1024;
pub const MAX_ENTRIES: usize = 400;
/// Total budget (characters of the data URLs) of the cache.
pub const MAX_TOTAL: usize = 32 * 1024 * 1024;
/// A failure is retried after this delay.
pub const RETRY_MS: i64 = 60_000;
pub const TIMEOUT: Duration = Duration::from_secs(10);

/// Image type from the declared content type, or from the first bytes (`.ico` icons are often served with no type).
pub fn image_type(declared: &str, bytes: &[u8]) -> Option<String> {
    let ty = declared.split(';').next().unwrap_or("").trim().to_lowercase();
    if ty.starts_with("image/") {
        return Some(ty);
    }
    let h = bytes;
    let t = if h.starts_with(&[0, 0, 1, 0]) { "image/x-icon" }
        else if h.starts_with(&[0x89, 0x50]) { "image/png" }
        else if h.starts_with(&[0xff, 0xd8]) { "image/jpeg" }
        else if h.starts_with(&[0x47, 0x49]) { "image/gif" }
        else if h.starts_with(&[0x52, 0x49]) { "image/webp" }
        else {
            let head = String::from_utf8_lossy(&bytes[..bytes.len().min(64)]);
            let head = head.trim_start().to_lowercase();
            if head.starts_with("<?xml") || head.starts_with("<svg") { "image/svg+xml" } else { return None }
        };
    Some(t.into())
}

/// Data URL for an image body, `None` if empty, too big or not an image.
pub fn to_data_url(content_type: &str, bytes: &[u8]) -> Option<String> {
    if bytes.is_empty() || bytes.len() > MAX_BYTES {
        return None;
    }
    let ty = image_type(content_type, bytes)?;
    Some(format!("data:{ty};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
}

/// Fetches a favicon as a data URL (the HTTP layer decides about cookies: pass the tab's when it has one).
pub fn fetch(http: &dyn Http, url: &str) -> Option<String> {
    let r = http.get(url, &[], TIMEOUT, MAX_BYTES + 1).ok()?;
    if !r.ok() {
        return None;
    }
    to_data_url("", &r.body)
}

pub fn is_fetchable(url: &str) -> bool {
    let l = url.to_ascii_lowercase();
    (l.starts_with("http://") || l.starts_with("https://")) && url.len() <= 4096
}

/// LRU cache of favicons. `Some(None)` is a remembered failure.
#[derive(Default)]
pub struct FaviconCache {
    order: VecDeque<String>,
    map: std::collections::HashMap<String, (Option<String>, i64)>,
    total: usize,
}

impl FaviconCache {
    /// Cached answer: `Some(data)` for a hit (or a recent failure), `None` when the icon must be fetched.
    pub fn get(&mut self, url: &str, now: i64) -> Option<Option<String>> {
        let hit = self.map.get(url)?.clone();
        if hit.0.is_none() && now - hit.1 >= RETRY_MS {
            return None;
        }
        // The most recently used entry goes to the end of the list.
        self.order.retain(|u| u != url);
        self.order.push_back(url.to_string());
        Some(hit.0)
    }

    /// Stores a result (not for icons only private tabs carry).
    pub fn put(&mut self, url: &str, data: Option<String>, now: i64) {
        if let Some((old, _)) = self.map.remove(url) {
            self.total -= old.map(|d| d.len()).unwrap_or(0);
            self.order.retain(|u| u != url);
        }
        self.total += data.as_ref().map(|d| d.len()).unwrap_or(0);
        self.map.insert(url.to_string(), (data, now));
        self.order.push_back(url.to_string());
        while self.order.len() > MAX_ENTRIES || self.total > MAX_TOTAL {
            let Some(oldest) = self.order.pop_front() else { break };
            if let Some((d, _)) = self.map.remove(&oldest) {
                self.total -= d.map(|d| d.len()).unwrap_or(0);
            }
        }
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Resolves an icon: cache, then `fetch`. `private_only`: icon carried by private tabs only, never cached.
    pub fn resolve(&mut self, http: &dyn Http, url: &str, private_only: bool) -> Option<String> {
        if !is_fetchable(url) {
            return None;
        }
        let now = now_ms();
        if let Some(hit) = self.get(url, now) {
            return hit;
        }
        let data = fetch(http, url);
        if !private_only {
            self.put(url, data.clone(), now);
        }
        data
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{resp, FakeHttp};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    fn type_sniffing() {
        assert_eq!(image_type("image/png; charset=x", b"").as_deref(), Some("image/png"));
        assert_eq!(image_type("application/octet-stream", &[0, 0, 1, 0, 9]).as_deref(), Some("image/x-icon"));
        assert_eq!(image_type("", &[0x89, 0x50, 0x4e]).as_deref(), Some("image/png"));
        assert_eq!(image_type("", &[0xff, 0xd8, 0xff]).as_deref(), Some("image/jpeg"));
        assert_eq!(image_type("", b"GIF89a").as_deref(), Some("image/gif"));
        assert_eq!(image_type("", b"RIFF....WEBP").as_deref(), Some("image/webp"));
        assert_eq!(image_type("", b"  <?xml version").as_deref(), Some("image/svg+xml"));
        assert_eq!(image_type("text/html", b"<html>"), None);
    }

    #[test]
    fn data_urls_are_bounded() {
        assert!(to_data_url("image/png", b"abc").unwrap().starts_with("data:image/png;base64,"));
        assert_eq!(to_data_url("image/png", b""), None);
        assert_eq!(to_data_url("image/png", &vec![1; MAX_BYTES + 1]), None);
        assert!(is_fetchable("https://a.com/f.ico") && !is_fetchable("data:image/png;base64,AA") && !is_fetchable(&format!("https://a.com/{}", "x".repeat(5000))));
    }

    #[test]
    fn cache_retry_and_eviction() {
        let mut c = FaviconCache::default();
        assert_eq!(c.get("u", 0), None);
        c.put("u", None, 1000);
        assert_eq!(c.get("u", 1000 + RETRY_MS - 1), Some(None));
        assert_eq!(c.get("u", 1000 + RETRY_MS), None);
        c.put("ok", Some("data:x".into()), 0);
        assert_eq!(c.get("ok", 10_000_000), Some(Some("data:x".into())));
        for i in 0..MAX_ENTRIES + 10 {
            c.put(&format!("k{i}"), Some("d".into()), 0);
        }
        assert_eq!(c.len(), MAX_ENTRIES);
        assert_eq!(c.get("k0", 0), None);
        assert!(c.get(&format!("k{}", MAX_ENTRIES + 9), 0).is_some());
    }

    #[test]
    fn resolve_caches_successes_only_for_non_private() {
        let n = Arc::new(AtomicUsize::new(0));
        let n2 = n.clone();
        let http = FakeHttp(Box::new(move |_| { n2.fetch_add(1, Ordering::SeqCst); Ok(crate::http::Response { status: 200, body: vec![0x89, 0x50, 1], url: String::new() }) }));
        let mut c = FaviconCache::default();
        assert!(c.resolve(&http, "https://a.com/f.png", false).is_some());
        assert!(c.resolve(&http, "https://a.com/f.png", false).is_some());
        assert_eq!(n.load(Ordering::SeqCst), 1);
        assert!(c.resolve(&http, "https://b.com/f.png", true).is_some());
        assert!(c.resolve(&http, "https://b.com/f.png", true).is_some());
        assert_eq!(n.load(Ordering::SeqCst), 3);
        assert_eq!(c.resolve(&http, "ftp://x", false), None);
        let bad = FakeHttp(Box::new(|_| resp(404, "")));
        assert_eq!(c.resolve(&bad, "https://c.com/f", false), None);
    }
}

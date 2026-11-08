//! Network hub helpers (port of the pure parts of `main/web/network.ts`): run order and Chrome match patterns.
use regex::Regex;
use url::Url;

/// Run order of blocking consumers (lower runs first).
pub mod order {
    pub const HTTPS_ONLY: i32 = 50;
    pub const DNR: i32 = 80;
    pub const ADBLOCK: i32 = 100;
    pub const HEADERS: i32 = 300;
    pub const DEVMODE: i32 = 900;
}

/// A handler that never answers must not hang the request.
pub const HANDLER_TIMEOUT_MS: u64 = 5_000;

enum Host {
    Any,
    Sub(String),
    Exact(String),
}

/// A compiled Chrome match pattern (`<all_urls>`, `*://*.example.com/path*`).
pub struct Pattern {
    all: bool,
    scheme: Option<Regex>,
    host: Host,
    port: Option<String>,
    path: Option<Regex>,
}

impl Pattern {
    /// `None` when the pattern is invalid.
    pub fn compile(pattern: &str) -> Option<Pattern> {
        if pattern == "<all_urls>" {
            return Some(Pattern { all: true, scheme: None, host: Host::Any, port: None, path: None });
        }
        let m = Regex::new(r"(?i)^(\*|[a-z][a-z0-9+.-]*)://([^/]*)(/.*)$").ok()?.captures(pattern)?;
        let (scheme, authority, path) = (&m[1], &m[2], &m[3]);
        let (host_s, port) = match Regex::new(r":(\d+|\*)$").ok()?.captures(authority) {
            Some(c) => (authority[..c.get(0)?.start()].to_lowercase(), Some(c[1].to_string())),
            None => (authority.to_lowercase(), None),
        };
        let path_re = format!("^{}$", path.split('*').map(regex::escape).collect::<Vec<_>>().join(".*"));
        let scheme_re = if scheme == "*" { "^https?$".to_string() } else { format!("^{}$", regex::escape(&scheme.to_lowercase())) };
        let host = if host_s == "*" {
            Host::Any
        } else if let Some(b) = host_s.strip_prefix("*.") {
            Host::Sub(b.to_string())
        } else {
            Host::Exact(host_s)
        };
        Some(Pattern { all: false, scheme: Some(Regex::new(&scheme_re).ok()?), host, port, path: Some(Regex::new(&format!("(?s){path_re}")).ok()?) })
    }

    pub fn matches(&self, raw: &str) -> bool {
        if self.all {
            let l = raw.to_ascii_lowercase();
            return ["http:", "https:", "ws:", "wss:", "ftp:", "file:"].iter().any(|p| l.starts_with(p));
        }
        let Ok(u) = Url::parse(raw) else { return false };
        let (Some(scheme), Some(path)) = (&self.scheme, &self.path) else { return false };
        if !scheme.is_match(u.scheme()) {
            return false;
        }
        let h = u.host_str().unwrap_or("").to_lowercase();
        match &self.host {
            Host::Any => {}
            Host::Sub(b) => {
                if h != *b && !h.ends_with(&format!(".{b}")) {
                    return false;
                }
            }
            Host::Exact(e) => {
                if h != *e {
                    return false;
                }
            }
        }
        if let Some(p) = &self.port
            && p != "*" && u.port().map(|x| x.to_string()).unwrap_or_default() != *p {
                return false;
            }
        let pq = format!("{}{}", u.path(), u.query().map(|q| format!("?{q}")).unwrap_or_default());
        path.is_match(&pq)
    }
}

/// URL patterns plus optional resource types, like `NetworkFilter` in the TS hub.
pub struct Filter {
    patterns: Vec<Pattern>,
    types: Option<Vec<String>>,
}

impl Filter {
    pub fn new(urls: &[&str], types: &[&str]) -> Filter {
        let mut patterns: Vec<Pattern> = urls.iter().filter_map(|u| Pattern::compile(u)).collect();
        if urls.is_empty() {
            patterns.extend(Pattern::compile("<all_urls>"));
        }
        Filter { patterns, types: if types.is_empty() { None } else { Some(types.iter().map(|s| s.to_string()).collect()) } }
    }

    pub fn matches(&self, url: &str, resource_type: Option<&str>) -> bool {
        let type_ok = match (&self.types, resource_type) {
            (None, _) => true,
            (Some(t), Some(r)) => t.iter().any(|x| x == r),
            (Some(_), None) => false,
        };
        type_ok && self.patterns.iter().any(|p| p.matches(url))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn m(p: &str, u: &str) -> bool { Pattern::compile(p).is_some_and(|p| p.matches(u)) }
    #[test]
    fn all_urls() {
        assert!(m("<all_urls>", "https://a.com/x"));
        assert!(m("<all_urls>", "file:///tmp/a"));
        assert!(!m("<all_urls>", "chrome://gpu"));
    }
    #[test]
    fn host_patterns() {
        assert!(m("*://*.example.com/*", "https://a.example.com/x?y"));
        assert!(m("*://*.example.com/*", "http://example.com/"));
        assert!(!m("*://*.example.com/*", "https://badexample.com/"));
        assert!(!m("*://example.com/*", "ftp://example.com/"));
        assert!(m("https://example.com/path*", "https://example.com/path/a?b=1"));
        assert!(!m("https://example.com/path*", "https://example.com/other"));
        assert!(m("http://localhost:3000/*", "http://localhost:3000/a"));
        assert!(!m("http://localhost:3000/*", "http://localhost:4000/a"));
        assert!(m("http://localhost:*/*", "http://localhost:4000/a"));
    }
    #[test]
    fn invalid_pattern() {
        assert!(Pattern::compile("nonsense").is_none());
    }
    #[test]
    fn filter_types() {
        let f = Filter::new(&["http://*/*", "https://*/*"], &["script"]);
        assert!(f.matches("https://a.com/", Some("script")));
        assert!(!f.matches("https://a.com/", Some("image")));
        assert!(!f.matches("https://a.com/", None));
        assert!(Filter::new(&[], &[]).matches("https://a.com/", None));
    }
}

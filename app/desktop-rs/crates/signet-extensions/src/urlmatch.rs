//! Chrome match patterns. Port of `apis/urlmatch.ts`, `patternCovers` of `apis/permissions.ts` and `compilePattern` of
//! `web/network.ts`.

use regex::Regex;
use std::path::{Component, Path, PathBuf};
use url::Url;

const ALL_URL_SCHEMES: [&str; 6] = ["http", "https", "ws", "wss", "ftp", "file"];

fn escape_re(s: &str) -> String {
    regex::escape(s)
}

/// `a*b` glob (only `*`) as an anchored regex.
fn glob_re(path: &str) -> Option<Regex> {
    let body = path.split('*').map(escape_re).collect::<Vec<_>>().join(".*");
    Regex::new(&format!("^{body}$")).ok()
}

struct Parsed {
    scheme: String,
    host: String,
    port: Option<String>,
    path: String,
}

fn parse(pattern: &str) -> Option<Parsed> {
    let sep = pattern.find("://")?;
    let scheme = &pattern[..sep];
    let valid_scheme = scheme == "*"
        || (scheme.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
            && scheme.chars().all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c)));
    if !valid_scheme {
        return None;
    }
    let rest = &pattern[sep + 3..];
    let slash = rest.find('/')?;
    let authority = &rest[..slash];
    let path = &rest[slash..];
    let (host, port) = match authority.rfind(':') {
        Some(i) if {
            let p = &authority[i + 1..];
            p == "*" || (!p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
        } =>
        {
            (&authority[..i], Some(authority[i + 1..].to_string()))
        }
        _ => (authority, None),
    };
    Some(Parsed { scheme: scheme.to_lowercase(), host: host.to_lowercase(), port, path: path.to_string() })
}

/// True if every URL matched by the match pattern `wanted` is also matched by `held`.
pub fn pattern_covers(held: &str, wanted: &str) -> bool {
    if held == wanted {
        return true;
    }
    let w = if wanted == "<all_urls>" { None } else { parse(wanted) };
    if held == "<all_urls>" {
        return wanted == "<all_urls>" || w.is_some_and(|w| w.scheme == "*" || ALL_URL_SCHEMES.contains(&w.scheme.as_str()));
    }
    let (Some(h), Some(w)) = (parse(held), w) else { return false };
    if h.scheme != "*" {
        if h.scheme != w.scheme {
            return false;
        }
    } else if w.scheme != "*" && w.scheme != "http" && w.scheme != "https" {
        return false;
    }
    if let Some(base) = h.host.strip_prefix("*.") {
        let host = w.host.strip_prefix("*.").unwrap_or(&w.host);
        if host != base && !host.ends_with(&format!(".{base}")) {
            return false;
        }
    } else if h.host != "*" && h.host != w.host {
        return false;
    }
    if let Some(hp) = &h.port {
        if hp != "*" && Some(hp) != w.port.as_ref() {
            return false;
        }
    }
    glob_re(&h.path).is_some_and(|re| re.is_match(&w.path))
}

/// A compiled Chrome match pattern (`<all_urls>`, `*://*.example.com/path*`).
#[derive(Debug, Clone)]
pub struct MatchPattern {
    all_urls: bool,
    scheme: String,
    host: String,
    port: Option<String>,
    path: Option<Regex>,
}

impl MatchPattern {
    /// None when the pattern is invalid.
    pub fn parse(pattern: &str) -> Option<MatchPattern> {
        if pattern == "<all_urls>" {
            return Some(MatchPattern { all_urls: true, scheme: String::new(), host: String::new(), port: None, path: None });
        }
        let p = parse(pattern)?;
        Some(MatchPattern { all_urls: false, scheme: p.scheme, host: p.host, port: p.port, path: Some(glob_re(&p.path)?) })
    }

    pub fn matches(&self, raw: &str) -> bool {
        if self.all_urls {
            let lower = raw.to_ascii_lowercase();
            return ["http:", "https:", "ws:", "wss:", "ftp:", "file:"].iter().any(|s| lower.starts_with(s));
        }
        let Ok(u) = Url::parse(raw) else { return false };
        let scheme = u.scheme();
        let scheme_ok = if self.scheme == "*" { scheme == "http" || scheme == "https" } else { scheme == self.scheme };
        if !scheme_ok {
            return false;
        }
        let h = u.host_str().unwrap_or("").to_lowercase();
        if self.host == "*" {
        } else if let Some(base) = self.host.strip_prefix("*.") {
            if h != base && !h.ends_with(&format!(".{base}")) {
                return false;
            }
        } else if h != self.host {
            return false;
        }
        if let Some(port) = &self.port {
            if port != "*" && u.port().map(|p| p.to_string()).unwrap_or_default() != *port {
                return false;
            }
        }
        let mut path_and_query = u.path().to_string();
        if let Some(q) = u.query() {
            path_and_query.push('?');
            path_and_query.push_str(q);
        }
        self.path.as_ref().is_some_and(|re| re.is_match(&path_and_query))
    }
}

/// True when `pattern` is a valid match pattern.
pub fn is_valid_pattern(pattern: &str) -> bool {
    MatchPattern::parse(pattern).is_some()
}

/// A URL as the narrowest match pattern that covers it (query and fragment do not take part in matching).
pub fn url_as_pattern(url: &str) -> Option<String> {
    let u = Url::parse(url).ok()?;
    match u.host_str() {
        Some(h) if !h.is_empty() => {
            let port = u.port().map(|p| format!(":{p}")).unwrap_or_default();
            Some(format!("{}://{}{}{}", u.scheme(), h, port, u.path()))
        }
        _ => {
            if u.scheme() == "file" {
                Some(format!("file://{}", u.path()))
            } else {
                None
            }
        }
    }
}

/// True if the match pattern covers the URL.
pub fn url_matches(pattern: &str, url: &str) -> bool {
    url_as_pattern(url).is_some_and(|wanted| pattern_covers(pattern, &wanted))
}

/// What an extension holds: manifest and granted permissions, host patterns included.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Access {
    pub permissions: Vec<String>,
    pub origins: Vec<String>,
}

/// Can the extension read or change the page at `url` (screenshot, CSS injection)? Chrome asks for a host permission or
/// `activeTab`. Signet does not track the user gesture that makes `activeTab` valid, so the permission alone counts.
/// Its own pages are always allowed.
pub fn can_access_url(access: &Access, extension_id: &str, url: &str) -> bool {
    if url.starts_with(&format!("chrome-extension://{extension_id}/")) {
        return true;
    }
    if access.permissions.iter().any(|p| p == "activeTab") || access.origins.iter().any(|o| o == "<all_urls>") {
        return true;
    }
    access.origins.iter().any(|p| url_matches(p, url))
}

fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// A file of the extension named by `reference`: a path relative to its folder, a rooted path, or a `chrome-extension://`
/// URL of this extension. None when it points elsewhere (another extension, outside the folder).
pub fn extension_file_path(root: &Path, extension_id: &str, reference: &str) -> Option<PathBuf> {
    let mut rel = reference.to_string();
    if reference.len() >= 19 && reference[..19].eq_ignore_ascii_case("chrome-extension://") {
        let u = Url::parse(reference).ok()?;
        if u.host_str() != Some(extension_id) {
            return None;
        }
        rel = percent_encoding::percent_decode_str(u.path()).decode_utf8().ok()?.into_owned();
    }
    let rel = rel.trim_start_matches(['/', '\\']);
    let abs = normalize(&root.join(rel));
    let root = normalize(root);
    if abs == root || !abs.starts_with(&root) {
        return None;
    }
    Some(abs)
}

/// What an extension's host patterns allow its pages to fetch across origins (CORS exemption, like Chrome gives
/// extensions with host permissions): `(scheme, domain, with_subdomains)` triples for the engine's origin whitelist.
/// An empty domain with subdomains means every host. Patterns for other schemes (`file`, `chrome`...) are ignored.
pub fn cors_targets(patterns: &[String]) -> Vec<(String, String, bool)> {
    let mut out: Vec<(String, String, bool)> = Vec::new();
    let mut push = |t: (String, String, bool)| {
        if !out.contains(&t) {
            out.push(t);
        }
    };
    for p in patterns {
        let p = p.trim();
        let (schemes, rest): (Vec<&str>, &str) = if p == "<all_urls>" {
            (vec!["http", "https"], "*/*")
        } else if let Some((scheme, rest)) = p.split_once("://") {
            match scheme {
                "*" => (vec!["http", "https"], rest),
                "http" | "https" | "ws" | "wss" => (vec![scheme], rest),
                _ => continue,
            }
        } else {
            continue;
        };
        let host = rest.split('/').next().unwrap_or("");
        let host = host.split(':').next().unwrap_or("");
        let (domain, sub) = match host {
            "" => continue,
            "*" => (String::new(), true),
            h => match h.strip_prefix("*.") {
                Some(base) if !base.is_empty() && !base.contains('*') => (base.to_string(), true),
                _ if h.contains('*') => continue,
                _ => (h.to_string(), false),
            },
        };
        for s in schemes {
            push((s.to_string(), domain.clone(), sub));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn acc(perms: &[&str], origins: &[&str]) -> Access {
        Access { permissions: perms.iter().map(|s| s.to_string()).collect(), origins: origins.iter().map(|s| s.to_string()).collect() }
    }

    #[test]
    fn url_matches_host_patterns_on_the_path_only() {
        assert!(url_matches("https://*.example.com/*", "https://www.example.com/a?b=1#c"));
        assert!(!url_matches("https://example.com/docs/*", "https://example.com/other"));
        assert!(url_matches("*://*/*", "http://localhost:8080/x"));
        assert!(url_matches("<all_urls>", "https://a.test/"));
        assert_eq!(url_as_pattern("not a url"), None);
    }

    #[test]
    fn can_access_url_cases() {
        assert!(can_access_url(&acc(&["activeTab"], &[]), ID, "https://a.test/"));
        assert!(can_access_url(&acc(&[], &["<all_urls>"]), ID, "https://a.test/"));
        assert!(can_access_url(&acc(&[], &["https://a.test/*"]), ID, "https://a.test/x"));
        assert!(!can_access_url(&acc(&[], &["https://a.test/*"]), ID, "https://b.test/x"));
        assert!(!can_access_url(&acc(&[], &[]), ID, "https://a.test/"));
        assert!(can_access_url(&acc(&[], &[]), ID, &format!("chrome-extension://{ID}/page.html")));
    }

    #[test]
    fn extension_file_path_resolves_and_refuses_escapes() {
        let root = std::env::temp_dir().join("ext").join("root");
        assert_eq!(extension_file_path(&root, ID, "css/a.css"), Some(root.join("css").join("a.css")));
        assert_eq!(extension_file_path(&root, ID, "/css/a.css"), Some(root.join("css").join("a.css")));
        assert_eq!(extension_file_path(&root, ID, &format!("chrome-extension://{ID}/i.png")), Some(root.join("i.png")));
        assert_eq!(extension_file_path(&root, ID, &format!("chrome-extension://{}/i.png", "b".repeat(32))), None);
        assert_eq!(extension_file_path(&root, ID, "../secret"), None);
        assert_eq!(extension_file_path(&root, ID, ""), None);
    }

    #[test]
    fn pattern_covers_matches_schemes_hosts_ports_and_paths() {
        assert!(pattern_covers("<all_urls>", "https://a.example/*"));
        assert!(!pattern_covers("<all_urls>", "chrome-extension://x/*"));
        assert!(pattern_covers("*://*/*", "https://a.example/x"));
        assert!(!pattern_covers("*://*/*", "<all_urls>"));
        assert!(pattern_covers("https://*.example.com/*", "https://a.b.example.com/*"));
        assert!(pattern_covers("https://*.example.com/*", "https://example.com/*"));
        assert!(!pattern_covers("https://*.example.com/*", "https://*.com/*"));
        assert!(!pattern_covers("https://*.example.com/*", "http://a.example.com/*"));
        assert!(!pattern_covers("https://a.example/*", "https://*.example/*"));
        assert!(pattern_covers("https://a.example/*", "https://a.example:8080/*"));
        assert!(!pattern_covers("https://a.example:8080/*", "https://a.example/*"));
        assert!(pattern_covers("https://a.example/app/*", "https://a.example/app/x"));
        assert!(!pattern_covers("https://a.example/app/*", "https://a.example/*"));
        assert!(!pattern_covers("*://a.example/*", "ftp://a.example/*"));
    }

    #[test]
    fn compile_pattern_matches_urls() {
        let t = |p: &str, u: &str| MatchPattern::parse(p).unwrap().matches(u);
        assert!(t("<all_urls>", "https://a.test/x"));
        assert!(!t("<all_urls>", "chrome://settings"));
        assert!(t("*://*.example.com/*", "https://sub.example.com/a?b=1"));
        assert!(t("*://*.example.com/*", "http://example.com/"));
        assert!(!t("*://*.example.com/*", "ftp://example.com/"));
        assert!(!t("https://example.com/a*", "https://example.com/b"));
        assert!(t("https://example.com:8080/*", "https://example.com:8080/x"));
        assert!(!t("https://example.com:8080/*", "https://example.com/x"));
        assert!(t("https://example.com:*/*", "https://example.com:99/x"));
        assert!(MatchPattern::parse("nonsense").is_none());
    }

    #[test]
    fn host_patterns_become_cors_whitelist_entries() {
        let v = |l: &[&str]| cors_targets(&l.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!(v(&["<all_urls>"]), [("http".into(), "".into(), true), ("https".into(), "".into(), true)]);
        assert_eq!(v(&["*://*/*"]), v(&["<all_urls>"]));
        assert_eq!(v(&["https://*.nordvpn.com/*"]), [("https".into(), "nordvpn.com".into(), true)]);
        assert_eq!(v(&["*://api.example.com/v1/*"]), [("http".into(), "api.example.com".into(), false), ("https".into(), "api.example.com".into(), false)]);
        assert_eq!(v(&["https://localhost:8080/*"]), [("https".into(), "localhost".into(), false)]);
        assert!(v(&["file:///*", "chrome://*/*", "https://a*b.com/*", "tabs", "https:///x"]).is_empty());
        assert_eq!(v(&["https://*.a.com/*", "https://*.a.com/x"]).len(), 1);
    }
}

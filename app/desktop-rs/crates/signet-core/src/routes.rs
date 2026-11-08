//! Ports `packages/shared/src/routes.ts`: routing rules sending new tabs to a space.

use crate::types::SpaceRoute;
use regex::Regex;

fn glob(s: &str) -> String {
    // `*` = any sequence of characters except `/`.
    regex::escape(s).replace(r"\*", "[^/]*")
}

/// Cleans a hand-typed pattern: no scheme, lowercase, no trailing `/`.
pub fn normalize_pattern(input: &str) -> String {
    let s = input.trim().to_lowercase();
    let s = match Regex::new(r"^[a-z][a-z0-9+.-]*://").ok().and_then(|re| re.find(&s).map(|m| m.end())) {
        Some(end) => s[end..].to_string(),
        None => s,
    };
    s.trim_end_matches('/').to_string()
}

/// Does the pattern (host, with an optional path) match this URL? Equal host or subdomain.
pub fn match_pattern(pattern: &str, url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(url) else { return false };
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return false;
    }
    let clean = normalize_pattern(pattern);
    if clean.is_empty() {
        return false;
    }
    let (host_pattern, path_pattern) = match clean.find('/') {
        Some(i) => (&clean[..i], &clean[i..]),
        None => (clean.as_str(), ""),
    };
    let host = parsed.host_str().unwrap_or("").to_lowercase();
    let Ok(host_re) = Regex::new(&format!("^(?:[^/]*\\.)?{}$", glob(host_pattern))) else { return false };
    if !host_re.is_match(&host) {
        return false;
    }
    if path_pattern.is_empty() {
        return true;
    }
    let Ok(path_re) = Regex::new(&format!("^{}(?:[/?#]|$)", glob(path_pattern))) else { return false };
    path_re.is_match(&parsed.path().to_lowercase())
}

/// First rule that matches this URL.
pub fn match_route<'a>(url: &str, routes: &'a [SpaceRoute]) -> Option<&'a SpaceRoute> {
    routes.iter().find(|r| match_pattern(&r.pattern, url))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes() {
        assert_eq!(normalize_pattern("  HTTPS://GitHub.com/Foo// "), "github.com/foo");
        assert_eq!(normalize_pattern("example.com"), "example.com");
    }

    #[test]
    fn host_and_subdomains() {
        assert!(match_pattern("github.com", "https://github.com/x"));
        assert!(match_pattern("github.com", "https://gist.github.com/x"));
        assert!(!match_pattern("github.com", "https://notgithub.com"));
        assert!(!match_pattern("github.com", "https://github.com.evil.org"));
        assert!(match_pattern("GitHub.com", "https://GITHUB.com/"));
    }

    #[test]
    fn wildcards() {
        assert!(match_pattern("*.example.com", "https://a.example.com"));
        assert!(match_pattern("docs.*", "https://docs.example.com"));
        assert!(match_pattern("example.*", "https://example.com"));
    }

    #[test]
    fn paths() {
        assert!(match_pattern("github.com/foo", "https://github.com/foo"));
        assert!(match_pattern("github.com/foo", "https://github.com/foo/bar"));
        assert!(!match_pattern("github.com/foo", "https://github.com/foobar"));
        assert!(!match_pattern("github.com/foo", "https://github.com/"));
        assert!(match_pattern("github.com/foo/*", "https://github.com/foo/bar"));
    }

    #[test]
    fn escapes_and_validity() {
        assert!(match_pattern("a+b.com", "https://a+b.com"));
        assert!(!match_pattern("a.com", "https://axcom.com"));
        assert!(!match_pattern("example.com", "ftp://example.com"));
        assert!(!match_pattern("example.com", "not a url"));
        assert!(!match_pattern("", "https://example.com"));
    }

    #[test]
    fn first_matching_route() {
        let routes = vec![
            SpaceRoute { id: "1".into(), pattern: "github.com".into(), space_id: "a".into() },
            SpaceRoute { id: "2".into(), pattern: "github.com/foo".into(), space_id: "b".into() },
        ];
        assert_eq!(match_route("https://github.com/foo", &routes).unwrap().pattern, "github.com");
        assert!(match_route("https://example.com", &routes).is_none());
    }
}

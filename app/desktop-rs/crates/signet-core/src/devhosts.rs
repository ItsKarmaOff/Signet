//! Ports `app/desktop/src/shared/devhosts.ts`: development hosts and local names (aliases).

use crate::types::DevAlias;
use regex::Regex;
use std::sync::OnceLock;

/// Host of a development server: the machine itself, a name reserved for tests, or a local network address.
pub fn is_local_host(hostname: &str) -> bool {
    let lower = hostname.to_lowercase();
    let h = lower.strip_prefix('[').unwrap_or(&lower);
    let h = h.strip_suffix(']').unwrap_or(h);
    if h == "localhost" || h.ends_with(".localhost") || h.ends_with(".local") || h.ends_with(".test") {
        return true;
    }
    if h == "::1" || h == "0.0.0.0" {
        return true;
    }
    static IP: OnceLock<Regex> = OnceLock::new();
    let ip = IP.get_or_init(|| Regex::new(r"^(\d{1,3})\.(\d{1,3})\.\d{1,3}\.\d{1,3}$").expect("static regex"));
    let Some(c) = ip.captures(h) else { return false };
    let (a, b): (u32, u32) = (c[1].parse().unwrap_or(0), c[2].parse().unwrap_or(0));
    a == 127 || a == 10 || (a == 192 && b == 168) || (a == 172 && (16..=31).contains(&b))
}

/// http(s) page served by a development server (see `is_local_host`).
pub fn is_local_dev_url(address: &str) -> bool {
    let lower = address.to_lowercase();
    if !(lower.starts_with("http:") || lower.starts_with("https:")) {
        return false;
    }
    url::Url::parse(address).ok().and_then(|u| u.host_str().map(is_local_host)).unwrap_or(false)
}

/// Valid name (lowercase, digits, hyphens; optional `.test` or `.localhost` suffix).
pub fn is_alias_name(name: &str) -> bool {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?(?:\.(?:test|localhost))?$").expect("static regex")).is_match(name)
}

/// Target address of a local name, from what is typed: `7353`, `localhost:7353`, `127.0.0.1:3000/app` or a full
/// address. Local addresses only; None otherwise. No query or fragment, no trailing `/`.
pub fn alias_target(input: &str) -> Option<String> {
    static PORT: OnceLock<Regex> = OnceLock::new();
    static SCHEME: OnceLock<Regex> = OnceLock::new();
    let mut text = input.trim().to_string();
    if PORT.get_or_init(|| Regex::new(r"^\d{2,5}$").expect("static regex")).is_match(&text) {
        text = format!("localhost:{text}");
    }
    if !SCHEME.get_or_init(|| Regex::new(r"(?i)^https?://").expect("static regex")).is_match(&text) {
        text = format!("http://{text}");
    }
    let url = url::Url::parse(&text).ok()?;
    if !is_local_dev_url(url.as_str()) {
        return None;
    }
    let origin = url.origin().ascii_serialization();
    Some(format!("{origin}{}", url.path()).trim_end_matches('/').to_string())
}

/// Address from an input that starts with a local name (`mysite`, `mysite/admin?x=1`, `http://mysite.test/...`).
pub fn resolve_alias(input: &str, aliases: &[DevAlias]) -> Option<String> {
    if aliases.is_empty() {
        return None;
    }
    static R: OnceLock<Regex> = OnceLock::new();
    let re = R.get_or_init(|| Regex::new(r"(?is)^(?:https?://)?([a-z0-9.-]+)([/?#].*)?$").expect("static regex"));
    let t = input.trim();
    let m = re.captures(t)?;
    let alias = aliases.iter().find(|a| a.name == m[1].to_lowercase())?;
    let rest = m.get(2).map_or("", |r| r.as_str());
    Some(if rest.is_empty() || rest.starts_with(['/', '?', '#']) { format!("{}{rest}", alias.target) } else { format!("{}/{rest}", alias.target) })
}

/// Local name matching an address (same origin, path under the target's).
pub fn alias_for<'a>(address: &str, aliases: &'a [DevAlias]) -> Option<&'a DevAlias> {
    let lower = address.to_lowercase();
    if aliases.is_empty() || !(lower.starts_with("http:") || lower.starts_with("https:")) {
        return None;
    }
    aliases.iter().find(|a| {
        address == a.target
            || ["/", "?", "#"].iter().any(|sep| address.starts_with(&format!("{}{sep}", a.target)))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alias(name: &str, target: &str) -> DevAlias {
        DevAlias { name: name.into(), target: target.into() }
    }

    #[test]
    fn local_hosts() {
        for h in ["localhost", "a.localhost", "x.local", "site.test", "127.0.0.1", "10.1.2.3", "192.168.0.9", "172.20.0.1", "::1", "[::1]", "0.0.0.0"] {
            assert!(is_local_host(h), "{h}");
        }
        for h in ["example.com", "8.8.8.8", "172.32.0.1", "192.169.0.1"] {
            assert!(!is_local_host(h), "{h}");
        }
        assert!(is_local_dev_url("http://localhost:3000/x"));
        assert!(!is_local_dev_url("https://example.com"));
        assert!(!is_local_dev_url("ftp://localhost"));
    }

    #[test]
    fn alias_names() {
        for n in ["app", "my-app", "a", "site.test", "x.localhost", "a1"] {
            assert!(is_alias_name(n), "{n}");
        }
        for n in ["", "-a", "a-", "A", "bad name", "a.com", "a.b.test"] {
            assert!(!is_alias_name(n), "{n}");
        }
    }

    #[test]
    fn alias_targets() {
        assert_eq!(alias_target("7353").unwrap(), "http://localhost:7353");
        assert_eq!(alias_target("localhost:7353").unwrap(), "http://localhost:7353");
        assert_eq!(alias_target("127.0.0.1:3000/app/").unwrap(), "http://127.0.0.1:3000/app");
        assert_eq!(alias_target("https://site.test/x?y=1#z").unwrap(), "https://site.test/x");
        assert_eq!(alias_target("https://evil.com"), None);
        assert_eq!(alias_target("localhost:1").unwrap(), "http://localhost:1");
    }

    #[test]
    fn alias_resolution() {
        let list = vec![alias("app", "http://localhost:3000")];
        assert_eq!(resolve_alias("app", &list).unwrap(), "http://localhost:3000");
        assert_eq!(resolve_alias("app/admin?x=1", &list).unwrap(), "http://localhost:3000/admin?x=1");
        assert_eq!(resolve_alias("http://APP/x", &list).unwrap(), "http://localhost:3000/x");
        assert_eq!(resolve_alias("other", &list), None);
        assert_eq!(resolve_alias("app", &[]), None);
        assert!(alias_for("http://localhost:3000/a", &list).is_some());
        assert!(alias_for("http://localhost:30001/a", &list).is_none());
        assert!(alias_for("http://localhost:3000", &list).is_some());
    }
}

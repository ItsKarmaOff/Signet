//! Ports `app/desktop/src/main/core/https-only.ts`: which http:// addresses are upgraded to https://.

use crate::urls::is_https_fallback_error;
use std::collections::{HashMap, HashSet};

fn unbracket(host: &str) -> &str {
    let h = host.strip_prefix('[').unwrap_or(host);
    h.strip_suffix(']').unwrap_or(h)
}

fn parse_ipv4(h: &str) -> Option<[u32; 4]> {
    let parts: Vec<&str> = h.split('.').collect();
    if parts.len() != 4 || parts.iter().any(|p| p.is_empty() || p.len() > 3 || !p.bytes().all(|b| b.is_ascii_digit())) {
        return None;
    }
    let n: Vec<u32> = parts.iter().filter_map(|p| p.parse().ok()).collect();
    <[u32; 4]>::try_from(n).ok()
}

fn is_private_ipv4(h: &str) -> bool {
    let Some([a, b, _, _]) = parse_ipv4(h) else { return false };
    a == 0 || a == 127 || a == 10 || (a == 172 && (16..=31).contains(&b)) || (a == 192 && b == 168) || (a == 169 && b == 254)
}

fn is_local_ipv6(h: &str) -> bool {
    if !h.contains(':') {
        return false;
    }
    if h == "::1" || h == "::" {
        return true;
    }
    // IPv4-mapped (`::ffff:7f00:1`, as URL normalizes it): the address inside decides.
    if let Some(rest) = h.strip_prefix("::ffff:")
        && let Some((hi, lo)) = rest.split_once(':')
        && let (Ok(hi), Ok(lo)) = (u32::from_str_radix(hi, 16), u32::from_str_radix(lo, 16))
        && hi <= 0xffff
        && lo <= 0xffff
    {
        return is_private_ipv4(&format!("{}.{}.{}.{}", hi >> 8, hi & 255, lo >> 8, lo & 255));
    }
    let first_str = h.split(':').next().unwrap_or("");
    let first = if first_str.is_empty() { 0 } else { u32::from_str_radix(first_str, 16).unwrap_or(u32::MAX) };
    if first == u32::MAX {
        return false;
    }
    (first & 0xfe00) == 0xfc00 || (first & 0xffc0) == 0xfe80 // unique local fc00::/7, link-local fe80::/10
}

/// Host that stays on http even in HTTPS-only mode: `localhost` and `*.localhost`, loopback, private and link-local
/// addresses, `.local`, and single-label names (an intranet).
pub fn is_exempt_host(hostname: &str) -> bool {
    let lower = hostname.to_lowercase();
    let h = unbracket(&lower);
    let h = h.strip_suffix('.').unwrap_or(h);
    if h.is_empty() {
        return true;
    }
    if h == "localhost" || h.ends_with(".localhost") || h.ends_with(".local") {
        return true;
    }
    if is_private_ipv4(h) || is_local_ipv6(h) {
        return true;
    }
    !h.contains('.') && !h.contains(':') // single label
}

#[derive(Debug, Default)]
pub struct UpgradeOptions<'a> {
    /// Hosts the user allowed through the warning (per session), lowercase.
    pub allowed_hosts: Option<&'a HashSet<String>>,
    /// Targets of the local names (dev aliases): their hosts stay on http.
    pub alias_targets: &'a [String],
}

/// The https:// address to open instead of an http:// one, or None to leave it as is (not http, exempt host, allowed host).
/// Same host, path, query and fragment; an explicit port is kept.
pub fn https_upgrade(url: &str, opts: &UpgradeOptions) -> Option<String> {
    let mut u = url::Url::parse(url).ok()?;
    if u.scheme() != "http" {
        return None;
    }
    let host = u.host_str()?.to_lowercase();
    if is_exempt_host(&host) || opts.allowed_hosts.is_some_and(|a| a.contains(&host)) {
        return None;
    }
    for target in opts.alias_targets {
        if url::Url::parse(target).ok().and_then(|t| t.host_str().map(str::to_lowercase)).as_deref() == Some(host.as_str()) {
            return None;
        }
    }
    u.set_scheme("https").ok()?;
    Some(u.to_string())
}

/// Load failure that says the site has no working secure connection (rather than no site at all, like a DNS error).
pub fn is_insecure_site_error(code: i32) -> bool {
    code == -103 || is_https_fallback_error(code)
}

/// True if `failed_url` is the https:// version (same host) of the http:// address that was upgraded.
pub fn is_upgrade_of(http_url: &str, failed_url: &str) -> bool {
    match (url::Url::parse(http_url), url::Url::parse(failed_url)) {
        (Ok(a), Ok(b)) => a.scheme() == "http" && b.scheme() == "https" && a.host_str().map(str::to_lowercase) == b.host_str().map(str::to_lowercase),
        _ => false,
    }
}

/// Prefix of the title the warning page sets to ask for "Continue to site" (followed by the token).
pub const CONTINUE_TITLE: &str = "signet-https-continue:";
/// Upgrades of the same host within this delay (ms), beyond which the site is considered to redirect back to http.
const LOOP_WINDOW_MS: i64 = 10_000;
const LOOP_LIMIT: u32 = 3;

#[derive(Debug, Clone)]
struct Attempt {
    http_url: String,
    at: i64,
    count: u32,
}

/// A tab showing the "no secure connection" warning: the token proves a click comes from that page.
#[derive(Debug, Clone)]
struct Warning {
    http_url: String,
    token: String,
}

fn is_same_host(a: &str, b: &str) -> bool {
    match (url::Url::parse(a), url::Url::parse(b)) {
        (Ok(a), Ok(b)) => a.host_str().map(str::to_lowercase) == b.host_str().map(str::to_lowercase),
        _ => false,
    }
}

/// HTTPS-only mode state (port of `main/browser/https-only.ts` without the Electron `Session` and request hub).
/// The app calls `set_config` when the setting or the dev aliases change, and `upgrade_request` from its main-frame
/// request hook (only pages, like Chrome: frames and subresources are left to Chromium). Sessions are identified by a
/// caller-chosen key (a private window has its own).
#[derive(Debug, Default)]
pub struct HttpsOnly {
    enabled: bool,
    alias_targets: Vec<String>,
    /// Hosts the user allowed through the warning, per session: in memory only.
    allowed: HashMap<String, HashSet<String>>,
    attempts: HashMap<String, Attempt>,
    warnings: HashMap<String, Warning>,
}

impl HttpsOnly {
    pub fn new() -> Self {
        Self::default()
    }

    /// Mirrors `settings.httpsOnly === true` and the targets of `settings.devAliases`.
    pub fn set_config(&mut self, enabled: bool, alias_targets: Vec<String>) {
        self.enabled = enabled;
        self.alias_targets = alias_targets;
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// The https:// address to load instead of `url` (noted for the tab), or `None` to load `url` itself.
    pub fn resolve_for(&mut self, tab_id: &str, session: &str, url: &str, now: i64) -> Option<String> {
        if !self.enabled {
            return None;
        }
        let opts = UpgradeOptions { allowed_hosts: self.allowed.get(session), alias_targets: &self.alias_targets };
        let https = https_upgrade(url, &opts)?;
        let prev = self.attempts.get(tab_id);
        let again = prev.is_some_and(|p| now - p.at < LOOP_WINDOW_MS && is_same_host(&p.http_url, url));
        let count = if again { prev.map_or(0, |p| p.count) + 1 } else { 1 };
        // A site that sends back to http each time we go to https: stop upgrading, the page loads over http.
        if count > LOOP_LIMIT {
            return None;
        }
        self.attempts.insert(tab_id.to_string(), Attempt { http_url: url.to_string(), at: now, count });
        Some(https)
    }

    /// Same as `resolve_for`, `url` itself when it is not upgraded.
    pub fn resolve(&mut self, tab_id: &str, session: &str, url: &str, now: i64) -> String {
        self.resolve_for(tab_id, session, url, now).unwrap_or_else(|| url.to_string())
    }

    /// Request hook for a main-frame `http://` request: the redirect target, or `None` to let it through.
    /// `tab_id` is `None` when the request does not belong to a tab.
    pub fn upgrade_request(&mut self, tab_id: Option<&str>, session: &str, url: &str, now: i64) -> Option<String> {
        self.resolve_for(tab_id?, session, url, now)
    }

    /// The tab's current navigation is a typed address promoted to https:// (its http:// version is the fallback).
    pub fn note_typed(&mut self, tab_id: &str, https_url: &str, now: i64) {
        if !self.enabled {
            return;
        }
        let http_url = match https_url.get(..6) {
            Some(p) if p.eq_ignore_ascii_case("https:") => format!("http:{}", &https_url[6..]),
            _ => https_url.to_string(),
        };
        self.attempts.insert(tab_id.to_string(), Attempt { http_url, at: now, count: 1 });
    }

    /// A main-frame load failed: the http:// address to offer on the warning page if it is the upgraded load that has
    /// no working secure connection, otherwise `None` (normal error page).
    pub fn insecure_failure(&self, tab_id: &str, failed_url: &str, code: i32) -> Option<String> {
        let attempt = self.attempts.get(tab_id)?;
        if !self.enabled || !is_insecure_site_error(code) {
            return None;
        }
        // The failed address is the https:// one (the redirect is followed internally); the http:// one is accepted just in case.
        let http_same = failed_url.get(..5).is_some_and(|p| p.eq_ignore_ascii_case("http:")) && is_same_host(&attempt.http_url, failed_url);
        if !is_upgrade_of(&attempt.http_url, failed_url) && !http_same {
            return None;
        }
        Some(attempt.http_url.clone())
    }

    /// Records the warning shown in the tab and returns the token its page must send back.
    pub fn warn(&mut self, tab_id: &str, http_url: &str) -> String {
        let token = uuid::Uuid::new_v4().to_string();
        self.warnings.insert(tab_id.to_string(), Warning { http_url: http_url.to_string(), token: token.clone() });
        token
    }

    /// The warning page asked to continue (its title carries the token): the http:// address to load, after allowing its host.
    pub fn continue_from(&mut self, tab_id: &str, session: &str, title: &str) -> Option<String> {
        let w = self.warnings.get(tab_id)?;
        if title.strip_prefix(CONTINUE_TITLE) != Some(w.token.as_str()) {
            return None;
        }
        let w = self.warnings.remove(tab_id)?;
        self.allow(session, &w.http_url);
        self.attempts.remove(tab_id);
        Some(w.http_url)
    }

    /// Allows the host of this address for the session (until the app quits).
    pub fn allow(&mut self, session: &str, url: &str) {
        if let Some(host) = url::Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_lowercase)) {
            self.allowed.entry(session.to_string()).or_default().insert(host);
        }
    }

    /// A private window closed: its allowed hosts are forgotten.
    pub fn forget_session(&mut self, session: &str) {
        self.allowed.remove(session);
    }

    /// A page committed: once the upgraded https:// page is there, its upgrade attempt is over (a redirect back to http keeps counting).
    pub fn on_committed(&mut self, tab_id: &str, url: &str) {
        if url.get(..6).is_some_and(|p| p.eq_ignore_ascii_case("https:"))
            && self.attempts.get(tab_id).is_some_and(|a| is_upgrade_of(&a.http_url, url))
        {
            self.attempts.remove(tab_id);
        }
    }

    pub fn forget(&mut self, tab_id: &str) {
        self.attempts.remove(tab_id);
        self.warnings.remove(tab_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn up(u: &str) -> Option<String> {
        https_upgrade(u, &UpgradeOptions::default())
    }

    #[test]
    fn upgrades_keeping_parts() {
        assert_eq!(up("http://example.com/a/b?x=1&y=2#frag").unwrap(), "https://example.com/a/b?x=1&y=2#frag");
        assert_eq!(up("http://example.com").unwrap(), "https://example.com/");
        assert_eq!(up("http://example.com:8080/x").unwrap(), "https://example.com:8080/x");
        assert_eq!(up("http://example.com:80/x").unwrap(), "https://example.com/x");
    }

    #[test]
    fn leaves_others_alone() {
        for u in ["https://example.com", "file:///a.html", "signet://settings", "not a url"] {
            assert_eq!(up(u), None, "{u}");
        }
        for u in [
            "http://localhost:3000",
            "http://app.localhost",
            "http://127.0.0.1:8080",
            "http://192.168.1.2",
            "http://nas/",
            "http://printer.local/",
            "http://[::1]:3000/",
        ] {
            assert_eq!(up(u), None, "{u}");
        }
    }

    #[test]
    fn allowed_hosts_and_aliases() {
        let allowed: HashSet<String> = ["old.example.com".to_string()].into();
        let o = UpgradeOptions { allowed_hosts: Some(&allowed), alias_targets: &[] };
        assert_eq!(https_upgrade("http://OLD.example.com/x", &o), None);
        assert_eq!(https_upgrade("http://other.example.com/x", &o).unwrap(), "https://other.example.com/x");
        let targets = vec!["http://site.test:7000".to_string()];
        let o = UpgradeOptions { allowed_hosts: None, alias_targets: &targets };
        assert_eq!(https_upgrade("http://site.test:7000/x", &o), None);
        assert_eq!(up("http://site.test:7000/x").unwrap(), "https://site.test:7000/x");
    }

    #[test]
    fn exempt_ipv4() {
        for h in ["127.0.0.1", "127.8.8.8", "10.0.0.5", "172.16.0.1", "172.31.255.1", "192.168.0.1", "169.254.10.1", "0.0.0.0"] {
            assert!(is_exempt_host(h), "{h}");
        }
        for h in ["172.15.0.1", "172.32.0.1", "8.8.8.8", "192.169.0.1", "169.253.0.1", "11.0.0.1"] {
            assert!(!is_exempt_host(h), "{h}");
        }
    }

    #[test]
    fn exempt_ipv6_and_names() {
        for h in ["[::1]", "::1", "[fd12:3456::1]", "[fc00::1]", "[fe80::1]", "[febf::1]", "[::ffff:7f00:1]", "[::ffff:c0a8:101]"] {
            assert!(is_exempt_host(h), "{h}");
        }
        for h in ["[2001:db8::1]", "[fec0::1]", "[::ffff:808:808]"] {
            assert!(!is_exempt_host(h), "{h}");
        }
        for h in ["localhost", "a.localhost", "nas.local", "intranet", "LOCALHOST"] {
            assert!(is_exempt_host(h), "{h}");
        }
        for h in ["example.com", "a.b.example.com", "local.example.com", "notlocalhost.com"] {
            assert!(!is_exempt_host(h), "{h}");
        }
    }

    #[test]
    fn insecure_errors() {
        for c in [-102, -107, -113, -200, -201, -205, -118, -7, -100, -101, -324, -103] {
            assert!(is_insecure_site_error(c), "{c}");
        }
        for c in [-105, -137, -106, -3, -300] {
            assert!(!is_insecure_site_error(c), "{c}");
        }
    }

    #[test]
    fn upgrade_of() {
        assert!(is_upgrade_of("http://a.com/x", "https://a.com/x"));
        assert!(is_upgrade_of("http://a.com/x", "https://A.com:8443/y"));
        assert!(!is_upgrade_of("http://a.com/x", "https://b.com/x"));
        assert!(!is_upgrade_of("http://a.com/x", "http://a.com/x"));
    }

    fn make(on: bool) -> HttpsOnly {
        let mut h = HttpsOnly::new();
        h.set_config(on, vec!["http://site.test:7000".to_string()]);
        h
    }

    #[test]
    fn resolve_off_and_exempt() {
        assert_eq!(make(false).resolve("t", "s", "http://example.com/", 0), "http://example.com/");
        let mut h = make(true);
        assert_eq!(h.resolve("t", "s", "http://example.com/a?b#c", 0), "https://example.com/a?b#c");
        assert_eq!(h.resolve("t", "s", "http://localhost:3000/", 0), "http://localhost:3000/");
        assert_eq!(h.resolve("t", "s", "http://site.test:7000/x", 0), "http://site.test:7000/x");
        assert_eq!(h.resolve("t", "s", "https://example.com/", 0), "https://example.com/");
    }

    #[test]
    fn loop_detection() {
        let mut h = make(true);
        let r: Vec<String> = (0..4).map(|i| h.resolve("t", "s", "http://loop.example.com/", i)).collect();
        assert!(r[..3].iter().all(|u| u == "https://loop.example.com/"));
        assert_eq!(r[3], "http://loop.example.com/");
        // outside the window the count restarts
        assert_eq!(h.resolve("t", "s", "http://loop.example.com/", 60_000), "https://loop.example.com/");
    }

    #[test]
    fn warning_flow() {
        let mut h = make(true);
        h.resolve("t", "s", "http://example.com/a", 0);
        assert_eq!(h.insecure_failure("t", "https://example.com/a", -102).as_deref(), Some("http://example.com/a"));
        assert_eq!(h.insecure_failure("t", "https://example.com/a", -200).as_deref(), Some("http://example.com/a"));
        assert_eq!(h.insecure_failure("t", "https://example.com/a", -105), None);
        assert_eq!(h.insecure_failure("t", "https://other.com/a", -102), None);
        assert_eq!(h.insecure_failure("other-tab", "https://example.com/a", -102), None);
        assert_eq!(h.insecure_failure("t", "http://example.com/a", -102).as_deref(), Some("http://example.com/a"));
    }

    #[test]
    fn typed_address_and_off() {
        let mut h = make(true);
        h.note_typed("t", "https://example.com/", 0);
        assert_eq!(h.insecure_failure("t", "https://example.com/", -107).as_deref(), Some("http://example.com/"));
        let mut off = make(false);
        off.note_typed("t", "https://example.com/", 0);
        assert_eq!(off.insecure_failure("t", "https://example.com/", -102), None);
    }

    #[test]
    fn continue_needs_token_and_is_per_session() {
        let mut h = make(true);
        h.resolve("t", "a", "http://example.com/a", 0);
        let token = h.warn("t", "http://example.com/a");
        assert_eq!(h.continue_from("t", "a", &format!("{CONTINUE_TITLE}forged")), None);
        assert_eq!(h.continue_from("other", "a", &format!("{CONTINUE_TITLE}{token}")), None);
        assert_eq!(h.continue_from("t", "a", &format!("{CONTINUE_TITLE}{token}")).as_deref(), Some("http://example.com/a"));
        assert_eq!(h.continue_from("t", "a", &format!("{CONTINUE_TITLE}{token}")), None);
        assert_eq!(h.resolve("t", "a", "http://example.com/a", 0), "http://example.com/a");
        assert_eq!(h.resolve("t", "a", "http://example.com/other", 0), "http://example.com/other");
        assert_eq!(h.resolve("t", "b", "http://example.com/a", 0), "https://example.com/a");
        h.forget_session("a");
        assert_eq!(h.resolve("t", "a", "http://example.com/a", 0), "https://example.com/a");
    }

    #[test]
    fn request_hook() {
        let mut h = make(true);
        assert_eq!(h.upgrade_request(Some("t"), "s", "http://example.com/submit?a=1", 0).as_deref(), Some("https://example.com/submit?a=1"));
        assert_eq!(h.upgrade_request(None, "s", "http://example.com/", 0), None);
        assert_eq!(h.upgrade_request(Some("t"), "s", "http://localhost/", 0), None);
        h.allow("s", "http://example.com/");
        assert_eq!(h.upgrade_request(Some("t"), "s", "http://example.com/a", 0), None);
        assert_eq!(h.upgrade_request(Some("t"), "s", "http://other.example.com/a", 0).as_deref(), Some("https://other.example.com/a"));
    }

    #[test]
    fn committed_ends_attempt() {
        let mut h = make(true);
        h.resolve("t", "s", "http://example.com/a", 0);
        h.on_committed("t", "https://example.com/a");
        assert_eq!(h.insecure_failure("t", "https://example.com/a", -102), None);
        h.resolve("t", "s", "http://example.com/a", 0);
        h.forget("t");
        assert_eq!(h.insecure_failure("t", "https://example.com/a", -102), None);
    }
}

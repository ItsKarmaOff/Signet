//! Ports `packages/shared/src/site.ts`: registrable domain of an address.

use regex::Regex;
use std::sync::OnceLock;

/// Registrable domain according to the Public Suffix List (private suffixes included): `www.bbc.co.uk` gives
/// `bbc.co.uk` (with the port for localhost and IP addresses). Empty string if the address is unreadable.
pub fn site_of(url: &str) -> String {
    let Ok(u) = url::Url::parse(url) else { return String::new() };
    let Some(host) = u.host_str() else { return String::new() };
    let host = host.to_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host).to_string();
    let port = u.port().map(|p| p.to_string()).unwrap_or_default();
    static IPV4: OnceLock<Regex> = OnceLock::new();
    let ipv4 = IPV4.get_or_init(|| Regex::new(r"^\d+(\.\d+){3}$").expect("static regex"));
    // Distinct ports = distinct applications (localhost:3000 and localhost:5173 do not share the same Boost).
    if ipv4.is_match(&host) || host.contains(':') || host == "localhost" {
        return if port.is_empty() { host } else { format!("{host}:{port}") };
    }
    // None for single-label hosts or `.lan`/`.local`: keep the host as is.
    match psl::domain_str(&host) {
        Some(d) if psl::suffix(host.as_bytes()).is_some_and(|s| s.is_known()) => d.to_string(),
        _ => host,
    }
}

#[cfg(test)]
mod tests {
    use super::site_of;

    #[test]
    fn registrable_domain() {
        assert_eq!(site_of("https://www.example.com/a?b=1"), "example.com");
        assert_eq!(site_of("https://sub.deep.example.com"), "example.com");
        assert_eq!(site_of("https://www.bbc.co.uk/news"), "bbc.co.uk");
    }

    #[test]
    fn lowercases_and_ignores_port_of_public_domains() {
        assert_eq!(site_of("https://WWW.Example.COM:8443/"), "example.com");
    }

    #[test]
    fn private_suffixes() {
        assert_eq!(site_of("https://alice.github.io/repo"), "alice.github.io");
        assert_eq!(site_of("https://bob.github.io/"), "bob.github.io");
    }

    #[test]
    fn keeps_port_for_localhost_and_ip() {
        assert_eq!(site_of("http://localhost:3000/x"), "localhost:3000");
        assert_eq!(site_of("http://localhost:5173"), "localhost:5173");
        assert_eq!(site_of("http://localhost/"), "localhost");
        assert_eq!(site_of("http://192.168.1.5:8080/"), "192.168.1.5:8080");
        assert_eq!(site_of("http://[::1]:80/"), "[::1]");
    }

    #[test]
    fn host_as_is_without_public_domain() {
        assert_eq!(site_of("http://intranet/"), "intranet");
        assert_eq!(site_of("http://nas.local/"), "nas.local");
    }

    #[test]
    fn unreadable() {
        assert_eq!(site_of("pas une url"), "");
        assert_eq!(site_of(""), "");
    }
}

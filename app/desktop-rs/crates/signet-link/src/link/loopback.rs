//! "localhost" addresses: what the phone sends to the computer instead of looking it up on itself.

use url::Url;

/// The machine itself: `localhost`, `*.localhost`, 127.0.0.0/8, `::1`, `0.0.0.0`.
pub fn is_loopback_host(hostname: &str) -> bool {
    let h = hostname.to_lowercase();
    let h = h.trim_start_matches('[').trim_end_matches(']');
    if h == "localhost" || h.ends_with(".localhost") {
        return true;
    }
    if h == "::1" || h == "0:0:0:0:0:0:0:1" || h == "0.0.0.0" {
        return true;
    }
    let parts: Vec<&str> = h.split('.').collect();
    parts.len() == 4
        && parts[0] == "127"
        && parts.iter().all(|p| !p.is_empty() && p.len() <= 3 && p.bytes().all(|b| b.is_ascii_digit()))
}

fn scheme_ok(url: &str) -> bool {
    let l = url.to_ascii_lowercase();
    ["http:", "https:", "ws:", "wss:"].iter().any(|s| l.starts_with(s))
}

/// http(s) or ws(s) address pointing at the machine itself.
pub fn is_loopback_url(url: &str) -> bool {
    if !scheme_ok(url) {
        return false;
    }
    match Url::parse(url) {
        Ok(u) => u.host_str().map(is_loopback_host).unwrap_or(false),
        Err(_) => false,
    }
}

/// `127.0.0.1`, `[::1]` and `0.0.0.0` become `localhost`; the rest is unchanged.
pub fn to_link_localhost(url: &str) -> String {
    if !is_loopback_url(url) {
        return url.to_string();
    }
    let Ok(u) = Url::parse(url) else { return url.to_string() };
    let h = u.host_str().unwrap_or("").to_lowercase();
    if h == "localhost" || h.ends_with(".localhost") {
        return url.to_string();
    }
    let mut out = format!("{}://localhost", u.scheme());
    if let Some(p) = u.port() {
        out.push_str(&format!(":{p}"));
    }
    out.push_str(u.path());
    if let Some(q) = u.query() {
        out.push('?');
        out.push_str(q);
    }
    if let Some(f) = u.fragment() {
        out.push('#');
        out.push_str(f);
    }
    out
}

/// Port of a local address (80 / 443 by default); None if the address is not local.
pub fn loopback_port(url: &str) -> Option<u16> {
    if !is_loopback_url(url) {
        return None;
    }
    let u = Url::parse(url).ok()?;
    u.port_or_known_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_the_machine_itself() {
        for h in ["localhost", "app.localhost", "127.0.0.1", "127.1.2.3", "[::1]", "0.0.0.0"] {
            assert!(is_loopback_host(h), "{h}");
        }
        for h in ["192.168.1.2", "example.com", "localhost.example.com"] {
            assert!(!is_loopback_host(h), "{h}");
        }
        assert!(is_loopback_url("ws://localhost:5173/"));
        assert!(!is_loopback_url("file:///tmp"));
    }

    #[test]
    fn maps_local_ips_to_localhost() {
        assert_eq!(to_link_localhost("http://127.0.0.1:5173/a?b=1#c"), "http://localhost:5173/a?b=1#c");
        assert_eq!(to_link_localhost("http://[::1]:3000/"), "http://localhost:3000/");
        assert_eq!(to_link_localhost("http://localhost:3000/x"), "http://localhost:3000/x");
        assert_eq!(to_link_localhost("https://example.com/"), "https://example.com/");
    }

    #[test]
    fn gives_the_port() {
        assert_eq!(loopback_port("http://localhost:5173/"), Some(5173));
        assert_eq!(loopback_port("http://localhost/"), Some(80));
        assert_eq!(loopback_port("https://localhost/"), Some(443));
        assert_eq!(loopback_port("https://example.com/"), None);
    }
}

//! Secure DNS servers offered in Settings > Privacy (port of `app/desktop/src/shared/dns.ts`).

/// DNS over HTTPS providers offered (same servers as Chrome); any other https address can be typed.
pub struct DnsProvider {
    pub id: &'static str,
    pub name: &'static str,
    pub template: &'static str,
}

pub const DNS_PROVIDERS: [DnsProvider; 5] = [
    DnsProvider { id: "cloudflare", name: "Cloudflare (1.1.1.1)", template: "https://chrome.cloudflare-dns.com/dns-query" },
    DnsProvider { id: "google", name: "Google (Public DNS)", template: "https://dns.google/dns-query{?dns}" },
    DnsProvider { id: "quad9", name: "Quad9 (9.9.9.9)", template: "https://dns.quad9.net/dns-query" },
    DnsProvider { id: "nextdns", name: "NextDNS", template: "https://chromium.dns.nextdns.io" },
    DnsProvider { id: "cleanbrowsing", name: "CleanBrowsing (Family Filter)", template: "https://doh.cleanbrowsing.org/doh/family-filter{?dns}" },
];

/// Server used by `secure` mode when none was chosen.
pub const DEFAULT_DNS_SERVER: &str = DNS_PROVIDERS[0].template;

/// True for a usable DNS over HTTPS server template: an https address, without credentials.
pub fn is_doh_template(value: &str) -> bool {
    if value.len() > 2048 {
        return false;
    }
    let address = value.strip_suffix("{?dns}").unwrap_or(value);
    match url::Url::parse(address) {
        Ok(u) => u.scheme() == "https" && u.host_str().is_some_and(|h| !h.is_empty()) && u.username().is_empty() && u.password().is_none(),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn providers_are_valid() {
        for p in &DNS_PROVIDERS {
            assert!(is_doh_template(p.template), "{}", p.id);
        }
        assert_eq!(DEFAULT_DNS_SERVER, "https://chrome.cloudflare-dns.com/dns-query");
    }

    #[test]
    fn rejects_unsafe_or_malformed() {
        assert!(!is_doh_template("http://dns.example/dns-query"));
        assert!(!is_doh_template("https://user:pw@dns.example/dns-query"));
        assert!(!is_doh_template("not a url"));
        assert!(!is_doh_template(""));
        assert!(!is_doh_template(&format!("https://a.example/{}", "x".repeat(2100))));
        assert!(is_doh_template("https://dns.example/dns-query{?dns}"));
    }
}

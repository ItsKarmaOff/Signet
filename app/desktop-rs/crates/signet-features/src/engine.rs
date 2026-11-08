//! Engine settings (port of `main/features/engine.ts`): secure DNS validation and system proxy settings screens.
//! Applying them to CEF (command line switches, host resolver) belongs to the app.
use std::path::Path;

/// DNS over HTTPS providers offered (same servers as Chrome).
pub const DNS_PROVIDERS: [(&str, &str, &str); 5] = [
    ("cloudflare", "Cloudflare (1.1.1.1)", "https://chrome.cloudflare-dns.com/dns-query"),
    ("google", "Google (Public DNS)", "https://dns.google/dns-query{?dns}"),
    ("quad9", "Quad9 (9.9.9.9)", "https://dns.quad9.net/dns-query"),
    ("nextdns", "NextDNS", "https://chromium.dns.nextdns.io"),
    ("cleanbrowsing", "CleanBrowsing (Family Filter)", "https://doh.cleanbrowsing.org/doh/family-filter{?dns}"),
];
/// Server used by `secure` mode when none was chosen.
pub const DEFAULT_DNS_SERVER: &str = DNS_PROVIDERS[0].2;

pub fn is_dns_mode(v: &str) -> bool {
    matches!(v, "off" | "automatic" | "secure")
}

/// True for a usable DNS over HTTPS server template: an https address, without credentials.
pub fn is_doh_template(value: &str) -> bool {
    if value.len() > 2048 {
        return false;
    }
    let trimmed = value.strip_suffix("{?dns}").unwrap_or(value);
    url::Url::parse(trimmed).is_ok_and(|u| u.scheme() == "https" && u.host_str().is_some_and(|h| !h.is_empty()) && u.username().is_empty() && u.password().is_none())
}

/// Resolver configuration for a `secureDns` setting: `(mode, servers)`, or `None` to leave the engine's default.
pub fn secure_dns(mode: Option<&str>, server: Option<&str>) -> Option<(String, Vec<String>)> {
    let mode = mode.filter(|m| is_dns_mode(m))?;
    let server = server.filter(|s| is_doh_template(s)).unwrap_or(DEFAULT_DNS_SERVER);
    Some((mode.to_string(), if mode == "secure" { vec![server.to_string()] } else { vec![] }))
}

/// Linux: system proxy settings screens, by desktop, in the order tried.
pub const LINUX_PROXY_TOOLS: [(&str, &[&str]); 4] = [("gnome-control-center", &["network"]), ("systemsettings", &["kcm_proxy"]), ("kcmshell6", &["kcm_proxy"]), ("kcmshell5", &["proxy"])];

/// First known proxy tool found in the given `PATH` value.
pub fn proxy_tool(path_var: &str) -> Option<(&'static str, &'static [&'static str])> {
    LINUX_PROXY_TOOLS.iter().copied().find(|(cmd, _)| std::env::split_paths(path_var).any(|dir| is_executable(&dir.join(cmd))))
}

fn is_executable(p: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        p.metadata().is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        p.is_file()
    }
}

/// How to open the system proxy settings.
#[derive(Debug, Clone, PartialEq)]
pub enum ProxyOpen {
    Url(&'static str),
    Command(&'static str, &'static [&'static str]),
    Unavailable,
}

pub fn open_proxy_settings(os: crate::importers::sources::Os, path_var: &str) -> ProxyOpen {
    use crate::importers::sources::Os;
    match os {
        Os::Mac => ProxyOpen::Url("x-apple.systempreferences:com.apple.Network-Settings.extension"),
        Os::Windows => ProxyOpen::Url("ms-settings:network-proxy"),
        Os::Linux => proxy_tool(path_var).map(|(c, a)| ProxyOpen::Command(c, a)).unwrap_or(ProxyOpen::Unavailable),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::importers::sources::Os;

    #[test]
    fn doh_templates() {
        assert!(is_doh_template("https://dns.google/dns-query{?dns}"));
        assert!(is_doh_template(DEFAULT_DNS_SERVER));
        for bad in ["http://dns.google/dns-query", "https://u:p@dns.example/q", "not a url", "", "https://"] { assert!(!is_doh_template(bad), "{bad}"); }
        assert!(!is_doh_template(&format!("https://a.com/{}", "x".repeat(2100))));
        assert!(DNS_PROVIDERS.iter().all(|p| is_doh_template(p.2)));
    }

    #[test]
    fn secure_dns_config() {
        assert_eq!(secure_dns(None, None), None);
        assert_eq!(secure_dns(Some("bogus"), None), None);
        assert_eq!(secure_dns(Some("off"), Some("https://x.com/q")), Some(("off".into(), vec![])));
        assert_eq!(secure_dns(Some("secure"), None), Some(("secure".into(), vec![DEFAULT_DNS_SERVER.to_string()])));
        assert_eq!(secure_dns(Some("secure"), Some("https://x.com/q")), Some(("secure".into(), vec!["https://x.com/q".to_string()])));
        assert_eq!(secure_dns(Some("secure"), Some("http://x.com/q")).unwrap().1, vec![DEFAULT_DNS_SERVER.to_string()]);
        assert!(is_dns_mode("automatic") && !is_dns_mode("on"));
    }

    #[cfg(unix)]
    #[test]
    fn linux_tool_lookup() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        assert_eq!(proxy_tool(d.path().to_str().unwrap()), None);
        let kcm = d.path().join("kcmshell5");
        std::fs::write(&kcm, "#!/bin/sh").unwrap();
        assert_eq!(proxy_tool(d.path().to_str().unwrap()), None);
        std::fs::set_permissions(&kcm, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(proxy_tool(d.path().to_str().unwrap()), Some(("kcmshell5", &["proxy"][..])));
        let gnome = d.path().join("gnome-control-center");
        std::fs::write(&gnome, "").unwrap();
        std::fs::set_permissions(&gnome, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(open_proxy_settings(Os::Linux, d.path().to_str().unwrap()), ProxyOpen::Command("gnome-control-center", &["network"]));
        assert_eq!(open_proxy_settings(Os::Linux, ""), ProxyOpen::Unavailable);
        assert!(matches!(open_proxy_settings(Os::Mac, ""), ProxyOpen::Url(u) if u.starts_with("x-apple")));
        assert_eq!(open_proxy_settings(Os::Windows, ""), ProxyOpen::Url("ms-settings:network-proxy"));
    }
}

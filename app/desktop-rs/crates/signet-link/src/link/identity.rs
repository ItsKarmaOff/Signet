//! This computer as phones see it: platform, name, LAN addresses.

use std::net::IpAddr;
use std::process::Command;

use super::protocol::LinkPlatform;

pub fn desktop_platform() -> LinkPlatform {
    if cfg!(target_os = "macos") {
        LinkPlatform::Macos
    } else if cfg!(target_os = "windows") {
        LinkPlatform::Windows
    } else {
        LinkPlatform::Linux
    }
}

/// `MacBook-de-Christophe.local` becomes `MacBook-de-Christophe`; no control characters, 64 characters at most.
pub fn clean_host_name(raw: &str) -> String {
    let filtered: String = raw.chars().filter(|c| (*c as u32) > 31 && *c as u32 != 127).collect();
    let mut name = filtered.trim().to_string();
    let lower = name.to_lowercase();
    for suffix in [".local", ".lan", ".home", ".localdomain"] {
        if lower.ends_with(suffix) {
            name.truncate(name.len() - suffix.len());
            break;
        }
    }
    let name: String = name.chars().take(64).collect();
    let name = name.trim().to_string();
    if name.is_empty() { "Signet".into() } else { name }
}

fn os_hostname() -> String {
    Command::new("hostname")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("COMPUTERNAME").ok())
        .or_else(|| std::env::var("HOSTNAME").ok())
        .unwrap_or_default()
}

/// Computer name: on Mac the one from System Settings (`scutil`), elsewhere the hostname.
pub fn computer_name() -> String {
    if cfg!(target_os = "macos") {
        if let Ok(o) = Command::new("/usr/sbin/scutil").args(["--get", "ComputerName"]).output() {
            let out = String::from_utf8_lossy(&o.stdout).to_string();
            if o.status.success() && !out.trim().is_empty() {
                return clean_host_name(&out);
            }
        }
    }
    clean_host_name(&os_hostname())
}

/// One interface with its addresses, as `lan_addresses` reads them.
#[derive(Debug, Clone)]
pub struct Iface {
    pub name: String,
    pub addrs: Vec<(IpAddr, bool)>, // (address, internal)
}

const VIRTUAL_PREFIXES: &[&str] = &[
    "lo", "utun", "awdl", "llw", "anpi", "bridge", "vmnet", "vboxnet", "docker", "br-", "veth", "virbr", "lxc", "lxd", "cni", "flannel", "tun",
    "tap", "zt", "tailscale", "wg", "ham", "vethernet", "virtualbox", "vmware", "hyper-v", "bluetooth",
];
const PHYSICAL_PREFIXES: &[&str] = &["en", "eth", "wlan", "wl", "wi-fi", "wifi", "ethernet", "eno", "ens", "enp", "wlp"];

fn is_virtual(name: &str) -> bool {
    let n = name.to_lowercase();
    if VIRTUAL_PREFIXES.iter().any(|p| n.starts_with(p)) {
        return true;
    }
    // `ap` followed by a digit (access point interfaces).
    n.strip_prefix("ap").and_then(|r| r.chars().next()).is_some_and(|c| c.is_ascii_digit())
}

fn is_physical(name: &str) -> bool {
    let n = name.to_lowercase();
    PHYSICAL_PREFIXES.iter().any(|p| n.starts_with(p))
}

/// Addresses the phone can reach, for the QR code: IPv4 of real interfaces first, no virtual networks or link-local,
/// three at most. IPv6 only if there is no IPv4 at all.
pub fn lan_addresses_of(interfaces: &[Iface]) -> Vec<String> {
    let mut v4: Vec<(String, bool)> = vec![];
    let mut v6: Vec<String> = vec![];
    for iface in interfaces {
        if is_virtual(&iface.name) {
            continue;
        }
        for (addr, internal) in &iface.addrs {
            if *internal {
                continue;
            }
            match addr {
                IpAddr::V4(a) => {
                    if !(a.octets()[0] == 169 && a.octets()[1] == 254) {
                        v4.push((a.to_string(), is_physical(&iface.name)));
                    }
                }
                IpAddr::V6(a) => {
                    if !a.to_string().to_lowercase().starts_with("fe80") {
                        v6.push(a.to_string());
                    }
                }
            }
        }
    }
    let ordered: Vec<String> = v4.iter().filter(|a| a.1).chain(v4.iter().filter(|a| !a.1)).map(|a| a.0.clone()).collect();
    if ordered.is_empty() { v6.into_iter().take(2).collect() } else { ordered.into_iter().take(3).collect() }
}

/// Addresses of this computer on the local network.
pub fn lan_addresses() -> Vec<String> {
    let mut by_name: Vec<Iface> = vec![];
    for i in if_addrs::get_if_addrs().unwrap_or_default() {
        let entry = (i.ip(), i.is_loopback());
        match by_name.iter_mut().find(|x| x.name == i.name) {
            Some(x) => x.addrs.push(entry),
            None => by_name.push(Iface { name: i.name.clone(), addrs: vec![entry] }),
        }
    }
    lan_addresses_of(&by_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn iface(name: &str, addrs: &[(&str, bool)]) -> Iface {
        Iface { name: name.into(), addrs: addrs.iter().map(|(a, i)| (a.parse().unwrap(), *i)).collect() }
    }

    #[test]
    fn keeps_wifi_and_ethernet_without_virtual_or_link_local() {
        let got = lan_addresses_of(&[
            iface("lo0", &[("127.0.0.1", true)]),
            iface("bridge100", &[("192.168.139.3", false)]),
            iface("bridge101", &[("192.168.97.0", false)]),
            iface("utun3", &[("10.8.0.2", false)]),
            iface("en7", &[("169.254.10.4", false)]),
            iface("en0", &[("192.168.219.102", false), ("fd07::1", false), ("fe80::1", false)]),
        ]);
        assert_eq!(got, vec!["192.168.219.102"]);
    }

    #[test]
    fn puts_real_interfaces_first_three_at_most() {
        let got = lan_addresses_of(&[
            iface("foo0", &[("10.0.0.9", false)]),
            iface("eth0", &[("10.0.0.1", false)]),
            iface("wlan0", &[("10.0.0.2", false)]),
            iface("enp3s0", &[("10.0.0.3", false)]),
        ]);
        assert_eq!(got, vec!["10.0.0.1", "10.0.0.2", "10.0.0.3"]);
    }

    #[test]
    fn ipv6_only_without_ipv4() {
        let got = lan_addresses_of(&[iface("en0", &[("fe80::1", false), ("fd07::1", false), ("2a01::1", false), ("2a01::2", false)])]);
        assert_eq!(got, vec!["fd07::1", "2a01::1"]);
    }

    #[test]
    fn computer_name_is_cleaned() {
        assert_eq!(clean_host_name("MacBook-de-Christophe.local"), "MacBook-de-Christophe");
        assert_eq!(clean_host_name("  PC\u{0} de\nLéa "), "PC deLéa");
        assert_eq!(clean_host_name(""), "Signet");
        assert_eq!(clean_host_name(&"x".repeat(100)).chars().count(), 64);
    }
}

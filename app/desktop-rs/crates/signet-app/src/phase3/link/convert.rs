//! Pure conversions between `signet-link` and the types of the interface.

use signet_core::types::{LinkDeviceInfo, LinkPairing, LinkState, PasskeyStatus};
use signet_link::{LinkState as LibState, PasskeyStatus as LibStatus, TabLike};

const STANDARD: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 with padding (data URLs).
pub fn base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                out.push(STANDARD[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// `data:` URL of a QR code (SVG with a quiet zone of 2 modules, like the Electron app).
pub fn qr_data_url(m: &signet_link::QrMatrix) -> String {
    format!("data:image/svg+xml;base64,{}", base64(m.to_svg(2).as_bytes()))
}

/// State shown by Settings > Devices and the toolbar.
pub fn link_state(s: &LibState) -> LinkState {
    let devices = s
        .devices
        .iter()
        .map(|d| LinkDeviceInfo {
            link_id: d.link_id.clone(),
            // Same JSON on both sides (`id`, `name`, `platform` in lowercase).
            device: serde_json::to_value(&d.device).ok().and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default(),
            enabled: d.enabled,
            status: serde_json::to_value(d.status).ok().and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default(),
            created_at: d.created_at as i64,
            last_seen: d.last_seen.map(|t| t as i64),
        })
        .collect();
    LinkState {
        general: s.general,
        name: s.name.clone(),
        devices,
        active: s.active as u32,
        pairing: s.pairing.as_ref().and_then(|p| p.qr.as_ref().map(|m| LinkPairing { qr: qr_data_url(m), expires_at: p.expires_at as i64 })),
        listen_error: s.listen_error,
    }
}

pub fn passkey_status(s: LibStatus) -> PasskeyStatus {
    match s {
        LibStatus::Waiting => PasskeyStatus::Waiting,
        LibStatus::Off => PasskeyStatus::Off,
        LibStatus::NoAdapter => PasskeyStatus::NoAdapter,
        LibStatus::Denied => PasskeyStatus::Denied,
        LibStatus::Error => PasskeyStatus::Error,
        LibStatus::Unsupported => PasskeyStatus::Unsupported,
        LibStatus::Connecting => PasskeyStatus::Connecting,
        LibStatus::Confirm => PasskeyStatus::Confirm,
        LibStatus::Failed => PasskeyStatus::Failed,
    }
}

/// Tabs offered to phones: non-empty addresses, sorted so that an unchanged set compares equal.
pub fn tab_list<'a>(tabs: impl Iterator<Item = (&'a str, &'a str)>) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = tabs.filter(|(u, _)| !u.is_empty()).map(|(u, t)| (u.to_string(), t.to_string())).collect();
    v.sort();
    v
}

pub fn to_tab_likes(v: &[(String, String)]) -> Vec<TabLike> {
    v.iter().map(|(url, title)| TabLike { url: url.clone(), title: title.clone() }).collect()
}

/// A phone may only make the computer open web addresses.
pub fn phone_url_ok(url: &str) -> bool {
    let l = url.trim().to_ascii_lowercase();
    l.starts_with("http://") || l.starts_with("https://")
}

#[cfg(test)]
mod tests {
    use super::*;
    use signet_link::link::manager::{DeviceInfo, PairingInfo};
    use signet_link::link::protocol::{LinkDevice, LinkPlatform};
    use signet_link::{LinkStatus, QrMatrix};

    #[test]
    fn base64_matches_the_standard() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn state_conversion_keeps_devices_and_pairing() {
        let lib = LibState {
            general: true,
            name: "Mac".into(),
            devices: vec![DeviceInfo {
                link_id: "l1".into(),
                device: LinkDevice { id: "d".into(), name: "Phone".into(), platform: LinkPlatform::Ios },
                enabled: true,
                status: LinkStatus::PausedThere,
                created_at: 5,
                last_seen: Some(9),
            }],
            active: 0,
            pairing: Some(PairingInfo { url: "signet://link?d=x".into(), expires_at: 77, qr: QrMatrix::new("signet://link?d=x") }),
            listen_error: false,
        };
        let s = link_state(&lib);
        assert_eq!(s.devices[0].device.name, "Phone");
        assert_eq!(s.devices[0].device.platform, signet_core::types::LinkPlatform::Ios);
        assert_eq!(s.devices[0].status, signet_core::types::LinkStatus::PausedThere);
        assert_eq!(s.devices[0].last_seen, Some(9));
        let p = s.pairing.expect("pairing");
        assert_eq!(p.expires_at, 77);
        assert!(p.qr.starts_with("data:image/svg+xml;base64,"));
    }

    #[test]
    fn statuses_map_one_to_one() {
        assert_eq!(passkey_status(LibStatus::NoAdapter), PasskeyStatus::NoAdapter);
        assert_eq!(passkey_status(LibStatus::Confirm), PasskeyStatus::Confirm);
        assert_eq!(passkey_status(LibStatus::Failed), PasskeyStatus::Failed);
    }

    #[test]
    fn tab_lists_are_stable_and_skip_empty_addresses() {
        let a = tab_list([("http://b/", "B"), ("", "x"), ("http://a/", "A")].into_iter());
        let b = tab_list([("http://a/", "A"), ("http://b/", "B")].into_iter());
        assert_eq!(a, b);
        assert_eq!(a.len(), 2);
        assert_eq!(to_tab_likes(&a)[0].url, "http://a/");
    }

    #[test]
    fn phone_urls_are_web_only() {
        assert!(phone_url_ok("https://example.com"));
        assert!(phone_url_ok(" HTTP://localhost:3000"));
        assert!(!phone_url_ok("file:///etc/passwd"));
        assert!(!phone_url_ok("javascript:alert(1)"));
        assert!(!phone_url_ok("signet://settings"));
    }
}

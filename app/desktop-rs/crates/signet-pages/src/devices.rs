//! Responsive design mode device list (port of app/desktop/src/shared/devices.ts, data only).
//! The bar and the resize handles of the mode are interface chrome (GPUI); the emulated page is the tab itself.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceKind {
    Phone,
    Tablet,
    Computer,
    Custom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UserAgentKind {
    SafariIphone,
    SafariIpad,
    ChromeAndroid,
}

/// Screen size in CSS px (portrait), density, mobile (touch, viewport), user agent.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DeviceSpec {
    pub id: &'static str,
    /// The device's own name; `None` = translated label (`develop.device.<id>`).
    pub name: Option<&'static str>,
    pub kind: DeviceKind,
    pub width: u32,
    pub height: u32,
    pub dpr: f64,
    pub mobile: bool,
    pub user_agent: Option<UserAgentKind>,
}

#[allow(clippy::too_many_arguments)]
const fn dev(id: &'static str, name: Option<&'static str>, kind: DeviceKind, w: u32, h: u32, dpr: f64, mobile: bool, ua: Option<UserAgentKind>) -> DeviceSpec {
    DeviceSpec { id, name, kind, width: w, height: h, dpr, mobile, user_agent: ua }
}

use DeviceKind::*;
use UserAgentKind::*;

pub const DEVICES: &[DeviceSpec] = &[
    dev("iphone-se", Some("iPhone SE"), Phone, 375, 667, 2.0, true, Some(SafariIphone)),
    dev("iphone-16", Some("iPhone 16"), Phone, 393, 852, 3.0, true, Some(SafariIphone)),
    dev("iphone-16-pro-max", Some("iPhone 16 Pro Max"), Phone, 440, 956, 3.0, true, Some(SafariIphone)),
    dev("pixel-9", Some("Pixel 9"), Phone, 412, 923, 2.625, true, Some(ChromeAndroid)),
    dev("galaxy-s24", Some("Galaxy S24"), Phone, 360, 780, 3.0, true, Some(ChromeAndroid)),
    dev("ipad-mini", Some("iPad mini"), Tablet, 744, 1133, 2.0, true, Some(SafariIpad)),
    dev("ipad-pro-11", Some("iPad Pro 11\""), Tablet, 834, 1210, 2.0, true, Some(SafariIpad)),
    dev("laptop", None, Computer, 1280, 800, 2.0, false, None),
    dev("desktop", None, Computer, 1920, 1080, 1.0, false, None),
    dev("custom", None, Custom, 800, 600, 2.0, false, None),
];

pub const DEFAULT_DEVICE: &str = "iphone-16";
/// Bounds of the size chosen by hand (CSS px).
pub const MIN_DEVICE: u32 = 200;
pub const MAX_DEVICE: u32 = 3840;

/// The device with this id, or the default one.
pub fn device_spec(id: Option<&str>) -> &'static DeviceSpec {
    let find = |i: &str| DEVICES.iter().find(|d| d.id == i);
    id.and_then(find).or_else(|| find(DEFAULT_DEVICE)).unwrap_or(&DEVICES[1])
}

/// Size typed by hand, rounded and kept within bounds.
pub fn clamp_size(n: f64) -> u32 {
    n.clamp(MIN_DEVICE as f64, MAX_DEVICE as f64).round() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_matches_spec() {
        assert_eq!(DEVICES.len(), 10);
        assert!(DEVICES.iter().filter(|d| d.mobile).all(|d| d.user_agent.is_some()));
        assert_eq!(device_spec(Some("pixel-9")).dpr, 2.625);
    }

    #[test]
    fn unknown_device_falls_back_to_default() {
        assert_eq!(device_spec(Some("nope")).id, DEFAULT_DEVICE);
        assert_eq!(device_spec(None).id, DEFAULT_DEVICE);
    }

    #[test]
    fn size_is_clamped() {
        assert_eq!(clamp_size(10.0), 200);
        assert_eq!(clamp_size(5000.0), 3840);
        assert_eq!(clamp_size(375.4), 375);
    }
}

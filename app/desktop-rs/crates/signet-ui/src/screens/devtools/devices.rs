//! Responsive mode logic of `shared/devices.ts` that the bar needs (`asCustom`, the mobile part of `responsiveProfile`),
//! on top of the device table of `signet_pages::devices`.

use signet_core::types::{DevDevice, DevResponsive};
use signet_pages::devices::{DeviceKind, DeviceSpec, device_spec};

/// Id of a device as the table and the wire format spell it.
pub fn device_id(d: DevDevice) -> &'static str {
    match d {
        DevDevice::IphoneSe => "iphone-se",
        DevDevice::Iphone16 => "iphone-16",
        DevDevice::Iphone16ProMax => "iphone-16-pro-max",
        DevDevice::Pixel9 => "pixel-9",
        DevDevice::GalaxyS24 => "galaxy-s24",
        DevDevice::IpadMini => "ipad-mini",
        DevDevice::IpadPro11 => "ipad-pro-11",
        DevDevice::Laptop => "laptop",
        DevDevice::Desktop => "desktop",
        DevDevice::Custom => "custom",
    }
}

/// Device of an id of the table.
pub fn device_of(id: &str) -> DevDevice {
    match id {
        "iphone-se" => DevDevice::IphoneSe,
        "iphone-16" => DevDevice::Iphone16,
        "iphone-16-pro-max" => DevDevice::Iphone16ProMax,
        "pixel-9" => DevDevice::Pixel9,
        "galaxy-s24" => DevDevice::GalaxyS24,
        "ipad-mini" => DevDevice::IpadMini,
        "ipad-pro-11" => DevDevice::IpadPro11,
        "laptop" => DevDevice::Laptop,
        "desktop" => DevDevice::Desktop,
        "custom" => DevDevice::Custom,
        _ => DevDevice::Iphone16,
    }
}

pub fn spec(d: DevDevice) -> &'static DeviceSpec {
    device_spec(Some(device_id(d)))
}

/// Whether the emulated profile is a mobile one (`responsiveProfile(r).mobile`).
pub fn is_mobile(r: &DevResponsive) -> bool {
    let own = spec(r.device);
    if r.device != DevDevice::Custom {
        return own.mobile;
    }
    let base = match r.base {
        Some(b) if b != DevDevice::Custom => spec(b),
        _ => own,
    };
    r.mobile.unwrap_or(base.mobile)
}

/// Switches to free size while keeping the current device's profile.
pub fn as_custom(r: &DevResponsive, width: Option<u32>, height: Option<u32>) -> DevResponsive {
    let mut out = DevResponsive { device: DevDevice::Custom, width: width.unwrap_or(r.width), height: height.unwrap_or(r.height), base: None, mobile: None };
    if r.device == DevDevice::Custom {
        out.base = r.base;
        out.mobile = r.mobile;
    } else {
        let s = spec(r.device);
        out.base = Some(r.device);
        out.mobile = Some(s.mobile);
    }
    out
}

/// Label of a device in the list: its own name, or the translated one (`develop.device.<id>`).
pub fn label(d: &DeviceSpec) -> String {
    d.name.map(str::to_string).unwrap_or_else(|| crate::app_state::tr(&format!("develop.device.{}", d.id)))
}

/// Devices of a group, in list order.
pub fn group(kind: DeviceKind) -> impl Iterator<Item = &'static DeviceSpec> {
    signet_pages::devices::DEVICES.iter().filter(move |d| d.kind == kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(device: DevDevice, width: u32, height: u32) -> DevResponsive {
        DevResponsive { device, width, height, base: None, mobile: None }
    }

    #[test]
    fn ids_round_trip() {
        for d in signet_pages::devices::DEVICES {
            assert_eq!(device_id(device_of(d.id)), d.id);
        }
    }

    #[test]
    fn mobile_follows_the_device_or_the_base() {
        assert!(is_mobile(&r(DevDevice::IphoneSe, 375, 667)));
        assert!(!is_mobile(&r(DevDevice::Desktop, 1, 1)));
        assert!(is_mobile(&DevResponsive { base: Some(DevDevice::Pixel9), ..r(DevDevice::Custom, 500, 500) }));
        assert!(!is_mobile(&DevResponsive { base: Some(DevDevice::Desktop), ..r(DevDevice::Custom, 500, 500) }));
        assert!(!is_mobile(&r(DevDevice::Custom, 500, 500)));
        assert!(is_mobile(&DevResponsive { base: Some(DevDevice::Laptop), mobile: Some(true), ..r(DevDevice::Custom, 500, 500) }));
        assert!(!is_mobile(&DevResponsive { base: Some(DevDevice::Iphone16), mobile: Some(false), ..r(DevDevice::Custom, 500, 500) }));
    }

    #[test]
    fn as_custom_keeps_the_profile_of_the_device_being_left() {
        assert_eq!(as_custom(&r(DevDevice::Pixel9, 412, 923), Some(500), None), DevResponsive { device: DevDevice::Custom, width: 500, height: 923, base: Some(DevDevice::Pixel9), mobile: Some(true) });
        let free = DevResponsive { base: Some(DevDevice::Laptop), mobile: Some(false), ..r(DevDevice::Custom, 1, 2) };
        assert_eq!(as_custom(&free, None, Some(9)), DevResponsive { device: DevDevice::Custom, width: 1, height: 9, base: Some(DevDevice::Laptop), mobile: Some(false) });
        assert_eq!(as_custom(&r(DevDevice::Custom, 1, 2), None, None), r(DevDevice::Custom, 1, 2));
    }
}

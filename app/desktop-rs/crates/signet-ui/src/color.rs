//! Color math ported from `packages/shared/src/mobile/color.ts`: HSL, OKLab/OKLCH, mixing, space palettes.

use gpui::{Hsla, Rgba};

pub type Rgb = [f32; 3];

fn clamp01(n: f32) -> f32 {
    n.clamp(0.0, 1.0)
}

fn to_linear(c: f32) -> f32 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}
fn to_gamma(c: f32) -> f32 {
    if c <= 0.0031308 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

/// Hue wrapped into [0, 360[.
pub fn normalize_hue(h: f32) -> f32 {
    ((h % 360.0) + 360.0) % 360.0
}

/// HSL (h degrees, s and l in 0..1) to sRGB (0..1).
pub fn hsl_to_rgb(h: f32, s: f32, l: f32) -> Rgb {
    let a = s * l.min(1.0 - l);
    let f = |n: f32| {
        let k = (n + normalize_hue(h) / 30.0) % 12.0;
        l - a * (-1.0f32).max((k - 3.0).min(9.0 - k).min(1.0))
    };
    [f(0.0), f(8.0), f(4.0)]
}

fn rgb_to_oklab([r, g, b]: Rgb) -> [f32; 3] {
    let (lr, lg, lb) = (to_linear(r), to_linear(g), to_linear(b));
    let l = (0.4122214708 * lr + 0.5363325363 * lg + 0.0514459929 * lb).cbrt();
    let m = (0.2119034982 * lr + 0.6806995451 * lg + 0.1073969566 * lb).cbrt();
    let s = (0.0883024619 * lr + 0.2817188376 * lg + 0.6299787005 * lb).cbrt();
    [
        0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s,
        1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s,
        0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s,
    ]
}

fn oklab_to_linear([ll, a, b]: [f32; 3]) -> Rgb {
    let l = (ll + 0.3963377774 * a + 0.2158037573 * b).powi(3);
    let m = (ll - 0.1055613458 * a - 0.0638541728 * b).powi(3);
    let s = (ll - 0.0894841775 * a - 1.291485548 * b).powi(3);
    [
        4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
        -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
        -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s,
    ]
}

fn in_gamut(rgb: Rgb) -> bool {
    rgb.iter().all(|c| *c >= -0.0005 && *c <= 1.0005)
}

/// OKLCH (L 0..1, C, h degrees) to sRGB; out of gamut, chroma is reduced until it fits.
pub fn oklch_to_rgb(l: f32, c: f32, h: f32) -> Rgb {
    let rad = normalize_hue(h).to_radians();
    let at = |c: f32| oklab_to_linear([l, c * rad.cos(), c * rad.sin()]);
    let mut lin = at(c);
    if !in_gamut(lin) {
        let (mut lo, mut hi) = (0.0f32, c);
        for _ in 0..24 {
            let mid = (lo + hi) / 2.0;
            if in_gamut(at(mid)) { lo = mid } else { hi = mid }
        }
        lin = at(lo);
    }
    [clamp01(to_gamma(clamp01(lin[0]))), clamp01(to_gamma(clamp01(lin[1]))), clamp01(to_gamma(clamp01(lin[2])))]
}

/// OKLCH hue (degrees) of an sRGB color: the `h` of `oklch(from <color> ... h)` in CSS.
pub fn oklch_hue(rgb: Rgb) -> f32 {
    let [_, a, b] = rgb_to_oklab(rgb);
    normalize_hue(b.atan2(a).to_degrees())
}

/// OKLCH hue of `hsl(hue s l)`: the starting color of the accent and the background tint.
pub fn base_hue(hue: f32, s: f32, l: f32) -> f32 {
    oklch_hue(hsl_to_rgb(hue, s, l))
}

/// Mix of `a` toward `b` (`t` 0..1) in OKLab, like `color-mix(in oklch)` for small amounts. Alpha is mixed linearly.
pub fn mix(a: Rgba, b: Rgba, t: f32) -> Rgba {
    let k = clamp01(t);
    let la = rgb_to_oklab([a.r, a.g, a.b]);
    let lb = rgb_to_oklab([b.r, b.g, b.b]);
    let lab = [la[0] + (lb[0] - la[0]) * k, la[1] + (lb[1] - la[1]) * k, la[2] + (lb[2] - la[2]) * k];
    let c = lab[1].hypot(lab[2]);
    let [r, g, bl] = oklch_to_rgb(lab[0], c, lab[2].atan2(lab[1]).to_degrees());
    Rgba { r, g, b: bl, a: a.a + (b.a - a.a) * k }
}

/// `rgb(r g b / a)` with 0..255 channels.
pub fn rgba8(r: u8, g: u8, b: u8, a: f32) -> Rgba {
    Rgba { r: r as f32 / 255.0, g: g as f32 / 255.0, b: b as f32 / 255.0, a }
}

pub fn from_rgb(c: Rgb) -> Rgba {
    Rgba { r: c[0], g: c[1], b: c[2], a: 1.0 }
}

pub fn with_alpha(c: Rgba, a: f32) -> Rgba {
    Rgba { a, ..c }
}

pub fn hsla(c: Rgba) -> Hsla {
    c.into()
}

/// Parses `#rrggbb` or `#rrggbbaa`.
pub fn from_hex(hex: &str) -> Option<Rgba> {
    let h = hex.trim().trim_start_matches('#');
    if h.len() != 6 && h.len() != 8 {
        return None;
    }
    let n = u32::from_str_radix(h, 16).ok()?;
    let (rgb, a) = if h.len() == 8 { (n >> 8, (n & 255) as f32 / 255.0) } else { (n, 1.0) };
    Some(rgba8((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, a))
}

/// WCAG relative luminance (0..1), to pick readable text on a color.
pub fn luminance(c: Rgba) -> f32 {
    0.2126 * to_linear(c.r) + 0.7152 * to_linear(c.g) + 0.0722 * to_linear(c.b)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scheme {
    Light,
    Dark,
}

impl Scheme {
    pub fn is_dark(self) -> bool {
        self == Scheme::Dark
    }
}

/// Strength of the space tint in backgrounds (Appearance setting): 0.5, 1 or 2.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TintStrength {
    Subtle,
    #[default]
    Normal,
    Strong,
}

impl TintStrength {
    pub fn k(self) -> f32 {
        match self {
            TintStrength::Subtle => 0.5,
            TintStrength::Normal => 1.0,
            TintStrength::Strong => 2.0,
        }
    }
}

/// Gradient strength relative to the solid hue (same constant as desktop).
pub const GRADIENT_STRENGTH: f32 = 2.6;

/// Hue boost around 295 degrees (blue, purple), where cool gray backgrounds absorb the tint (`--tint-boost`).
pub fn tint_boost(hue: f32) -> f32 {
    1.0 + 0.7 * (hue - 295.0).to_radians().cos().max(0.0)
}

/// Accent of a space: fixed lightness so white on top stays readable for every hue.
pub fn accent_color(hue: f32, scheme: Scheme) -> Rgba {
    from_rgb(match scheme {
        Scheme::Dark => oklch_to_rgb(0.72, 0.14, base_hue(hue, 0.75, 0.68)),
        Scheme::Light => oklch_to_rgb(0.55, 0.16, base_hue(hue, 0.7, 0.55)),
    })
}

/// Soft tint of the space, used to color backgrounds (`--tint-color`).
pub fn tint_color(hue: f32, scheme: Scheme) -> Rgba {
    let boost = tint_boost(hue);
    let bh = base_hue(hue, 0.7, 0.55);
    from_rgb(match scheme {
        Scheme::Dark => oklch_to_rgb(0.7, 0.12 * boost, bh),
        Scheme::Light => oklch_to_rgb(0.62, 0.13 * boost, bh),
    })
}

/// Swatch / preview color of a hue (`hsl(h 70% 62%)`).
pub fn swatch_color(hue: f32) -> Rgba {
    from_rgb(hsl_to_rgb(hue, 0.7, 0.62))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_roundtrip() {
        let c = from_hex("#ff8000").unwrap();
        assert!((c.r - 1.0).abs() < 1e-3 && (c.g - 0.502).abs() < 1e-2 && c.b == 0.0);
        assert!(from_hex("zzz").is_none());
    }

    #[test]
    fn hsl_primaries() {
        let r = hsl_to_rgb(0.0, 1.0, 0.5);
        assert!((r[0] - 1.0).abs() < 1e-4 && r[1].abs() < 1e-4);
        let g = hsl_to_rgb(120.0, 1.0, 0.5);
        assert!((g[1] - 1.0).abs() < 1e-4);
    }

    #[test]
    fn oklch_lightness_is_fixed_for_accent() {
        // The accent keeps the same OKLab L whatever the hue (white text stays legible).
        for hue in [10.0, 90.0, 180.0, 250.0, 320.0] {
            let a = accent_color(hue, Scheme::Light);
            let lab = rgb_to_oklab([a.r, a.g, a.b]);
            assert!((lab[0] - 0.55).abs() < 0.02, "hue {hue}: L = {}", lab[0]);
        }
    }

    #[test]
    fn tint_boost_peaks_near_295() {
        assert!((tint_boost(295.0) - 1.7).abs() < 1e-4);
        assert!((tint_boost(115.0) - 1.0).abs() < 1e-4);
    }

    #[test]
    fn mix_endpoints() {
        let a = from_hex("#000000").unwrap();
        let b = from_hex("#ffffff").unwrap();
        assert!(mix(a, b, 0.0).r < 0.01);
        assert!(mix(a, b, 1.0).r > 0.99);
    }

    #[test]
    fn luminance_order() {
        assert!(luminance(from_hex("#ffffff").unwrap()) > luminance(from_hex("#202020").unwrap()));
    }
}

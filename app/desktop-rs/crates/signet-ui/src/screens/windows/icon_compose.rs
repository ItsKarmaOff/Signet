//! App icon composition of `WebAppInstall.tsx` (`compose`, `opaqueCorners`, `autoFit`), without a canvas: the image on a
//! rounded plate of the macOS icon grid (1024 px square, 824 px plate, 185 px corners, light shadow), or the name's initial
//! on the space's tint.
//!
//! Differences with the canvas version: SVG images cannot be decoded here (the caller falls back to the initial), and the
//! initial is drawn with a small built-in stroke font (A-Z, 0-9, accented Latin letters folded to their base letter, `?`
//! otherwise) because no font rasterizer is available to this crate.

use image::{DynamicImage, RgbaImage, imageops};

/// Icon size and plate of the macOS icon grid, in px (the reference scale).
pub const SIZE: u32 = 1024;
const PLATE: f32 = 824.0;
const RADIUS: f32 = 185.0;

/// How the image is framed: `Fill` the whole rounded plate, `Fit` placed at 60 % on a white plate, `Raw` as is over the
/// whole square (an icon already drawn for the Mac).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fit {
    Fill,
    Fit,
    Raw,
}

impl Fit {
    pub const ALL: [Fit; 3] = [Fit::Fill, Fit::Fit, Fit::Raw];

    /// Catalog key suffix: `webapp.fit.<key>`.
    pub fn key(self) -> &'static str {
        match self {
            Fit::Fill => "fill",
            Fit::Fit => "fit",
            Fit::Raw => "raw",
        }
    }
}

/// Image chosen by the user: 15 MB at most (it is downscaled to 1024 px anyway).
pub const MAX_CUSTOM_BYTES: usize = 15_000_000;

/// Decodes PNG, JPEG, GIF, WebP, BMP or ICO bytes; `None` if unreadable.
pub fn load_image(bytes: &[u8]) -> Option<RgbaImage> {
    let img = image::load_from_memory(bytes).ok()?.to_rgba8();
    (img.width() > 0 && img.height() > 0).then_some(img)
}

/// True if the image's corners are opaque (full square, to stretch over the whole plate rather than center in it).
pub fn opaque_corners(img: &RgbaImage) -> bool {
    let small = imageops::resize(img, 32, 32, imageops::FilterType::Triangle);
    [(1, 1), (30, 1), (1, 30), (30, 30)].iter().all(|&(x, y)| small.get_pixel(x, y)[3] > 240)
}

/// Suggested framing: plate filled for a maskable icon or a crisp opaque square, otherwise placed on a white background.
pub fn auto_fit(img: &RgbaImage, maskable: bool) -> Fit {
    let natural = img.width().min(img.height());
    if maskable || (natural >= 120 && opaque_corners(img)) { Fit::Fill } else { Fit::Fit }
}

/// Coverage (0..1) of a rounded square of half-size `half` centered at `c`, at pixel center `(x, y)`.
fn rounded_square_coverage(x: f32, y: f32, c: f32, half: f32, radius: f32) -> f32 {
    let (qx, qy) = ((x - c).abs() - (half - radius), (y - c).abs() - (half - radius));
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    let inside = qx.max(qy).min(0.0);
    let sdf = outside + inside - radius;
    (0.5 - sdf).clamp(0.0, 1.0)
}

/// `rgba` over `dst` (straight alpha).
fn blend(dst: &mut image::Rgba<u8>, rgb: [f32; 3], alpha: f32) {
    let a = alpha.clamp(0.0, 1.0);
    if a <= 0.0 {
        return;
    }
    let da = f32::from(dst[3]) / 255.0;
    let out_a = a + da * (1.0 - a);
    for i in 0..3 {
        let d = f32::from(dst[i]) / 255.0;
        let v = (rgb[i] * a + d * da * (1.0 - a)) / out_a.max(1e-6);
        dst[i] = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    }
    dst[3] = (out_a.clamp(0.0, 1.0) * 255.0).round() as u8;
}

/// `hsl(h 50% 48%)` as linear-free sRGB floats.
pub fn hsl_rgb(h: f32, s: f32, l: f32) -> [f32; 3] {
    let h = h.rem_euclid(360.0) / 60.0;
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    [r + m, g + m, b + m]
}

/// Draws `src` scaled by `ratio` centered on `(cx, cy)`, only where `mask(x, y)` allows (coverage 0..1).
fn draw_scaled(dst: &mut RgbaImage, src: &RgbaImage, ratio: f32, cx: f32, cy: f32, mask: impl Fn(u32, u32) -> f32) {
    let w = (src.width() as f32 * ratio).round().max(1.0) as u32;
    let h = (src.height() as f32 * ratio).round().max(1.0) as u32;
    let scaled = imageops::resize(src, w, h, imageops::FilterType::Lanczos3);
    let (ox, oy) = ((cx - w as f32 / 2.0).round() as i64, (cy - h as f32 / 2.0).round() as i64);
    for (sx, sy, p) in scaled.enumerate_pixels() {
        let (x, y) = (ox + i64::from(sx), oy + i64::from(sy));
        if x < 0 || y < 0 || x >= i64::from(dst.width()) || y >= i64::from(dst.height()) {
            continue;
        }
        let cov = mask(x as u32, y as u32);
        if cov > 0.0 {
            blend(dst.get_pixel_mut(x as u32, y as u32), [f32::from(p[0]) / 255.0, f32::from(p[1]) / 255.0, f32::from(p[2]) / 255.0], f32::from(p[3]) / 255.0 * cov);
        }
    }
}

// ----------------------------------------------------------------------------- initial (stroke font)

type Stroke = &'static [(f32, f32)];

/// Glyph on a 4 x 6 grid (y down), as polylines.
fn glyph(c: char) -> &'static [Stroke] {
    const O: Stroke = &[(1.0, 0.0), (3.0, 0.0), (4.0, 1.0), (4.0, 5.0), (3.0, 6.0), (1.0, 6.0), (0.0, 5.0), (0.0, 1.0), (1.0, 0.0)];
    const P: Stroke = &[(0.0, 6.0), (0.0, 0.0), (2.6, 0.0), (4.0, 1.0), (4.0, 2.4), (2.6, 3.6), (0.0, 3.6)];
    match c {
        'A' => &[&[(0.0, 6.0), (2.0, 0.0), (4.0, 6.0)], &[(0.9, 4.0), (3.1, 4.0)]],
        'B' => &[&[(0.0, 0.0), (0.0, 6.0), (2.6, 6.0), (3.6, 5.2), (3.6, 3.8), (2.6, 3.0), (0.0, 3.0)], &[(0.0, 0.0), (2.4, 0.0), (3.4, 0.8), (3.4, 2.2), (2.4, 3.0)]],
        'C' => &[&[(4.0, 1.0), (3.0, 0.0), (1.0, 0.0), (0.0, 1.0), (0.0, 5.0), (1.0, 6.0), (3.0, 6.0), (4.0, 5.0)]],
        'D' => &[&[(0.0, 0.0), (0.0, 6.0), (2.5, 6.0), (4.0, 4.5), (4.0, 1.5), (2.5, 0.0), (0.0, 0.0)]],
        'E' => &[&[(4.0, 0.0), (0.0, 0.0), (0.0, 6.0), (4.0, 6.0)], &[(0.0, 3.0), (3.0, 3.0)]],
        'F' => &[&[(4.0, 0.0), (0.0, 0.0), (0.0, 6.0)], &[(0.0, 3.0), (3.0, 3.0)]],
        'G' => &[&[(4.0, 1.0), (3.0, 0.0), (1.0, 0.0), (0.0, 1.0), (0.0, 5.0), (1.0, 6.0), (3.0, 6.0), (4.0, 5.0), (4.0, 3.2), (2.2, 3.2)]],
        'H' => &[&[(0.0, 0.0), (0.0, 6.0)], &[(4.0, 0.0), (4.0, 6.0)], &[(0.0, 3.0), (4.0, 3.0)]],
        'I' => &[&[(1.0, 0.0), (3.0, 0.0)], &[(2.0, 0.0), (2.0, 6.0)], &[(1.0, 6.0), (3.0, 6.0)]],
        'J' => &[&[(4.0, 0.0), (4.0, 5.0), (3.0, 6.0), (1.0, 6.0), (0.0, 5.0)]],
        'K' => &[&[(0.0, 0.0), (0.0, 6.0)], &[(4.0, 0.0), (0.0, 3.5)], &[(1.4, 2.6), (4.0, 6.0)]],
        'L' => &[&[(0.0, 0.0), (0.0, 6.0), (4.0, 6.0)]],
        'M' => &[&[(0.0, 6.0), (0.0, 0.0), (2.0, 3.5), (4.0, 0.0), (4.0, 6.0)]],
        'N' => &[&[(0.0, 6.0), (0.0, 0.0), (4.0, 6.0), (4.0, 0.0)]],
        'O' | '0' => &[O],
        'P' => &[P],
        'Q' => &[O, &[(2.6, 4.4), (4.0, 6.2)]],
        'R' => &[P, &[(2.0, 3.6), (4.0, 6.0)]],
        'S' => &[&[(4.0, 1.0), (3.0, 0.0), (1.0, 0.0), (0.0, 1.0), (0.0, 2.2), (1.0, 3.0), (3.0, 3.0), (4.0, 3.8), (4.0, 5.0), (3.0, 6.0), (1.0, 6.0), (0.0, 5.0)]],
        'T' => &[&[(0.0, 0.0), (4.0, 0.0)], &[(2.0, 0.0), (2.0, 6.0)]],
        'U' => &[&[(0.0, 0.0), (0.0, 5.0), (1.0, 6.0), (3.0, 6.0), (4.0, 5.0), (4.0, 0.0)]],
        'V' => &[&[(0.0, 0.0), (2.0, 6.0), (4.0, 0.0)]],
        'W' => &[&[(0.0, 0.0), (1.0, 6.0), (2.0, 2.5), (3.0, 6.0), (4.0, 0.0)]],
        'X' => &[&[(0.0, 0.0), (4.0, 6.0)], &[(4.0, 0.0), (0.0, 6.0)]],
        'Y' => &[&[(0.0, 0.0), (2.0, 3.0)], &[(4.0, 0.0), (2.0, 3.0), (2.0, 6.0)]],
        'Z' => &[&[(0.0, 0.0), (4.0, 0.0), (0.0, 6.0), (4.0, 6.0)]],
        '1' => &[&[(0.8, 1.2), (2.2, 0.0), (2.2, 6.0)], &[(0.8, 6.0), (3.6, 6.0)]],
        '2' => &[&[(0.0, 1.0), (1.0, 0.0), (3.0, 0.0), (4.0, 1.0), (4.0, 2.4), (0.0, 6.0), (4.0, 6.0)]],
        '3' => &[&[(0.0, 1.0), (1.0, 0.0), (3.0, 0.0), (4.0, 1.0), (4.0, 2.2), (3.0, 3.0), (1.6, 3.0)], &[(3.0, 3.0), (4.0, 3.8), (4.0, 5.0), (3.0, 6.0), (1.0, 6.0), (0.0, 5.0)]],
        '4' => &[&[(3.0, 6.0), (3.0, 0.0), (0.0, 4.4), (4.0, 4.4)]],
        '5' => &[&[(4.0, 0.0), (0.4, 0.0), (0.0, 3.0), (2.6, 2.6), (4.0, 3.6), (4.0, 5.0), (3.0, 6.0), (1.0, 6.0), (0.0, 5.0)]],
        '6' => &[&[(3.6, 0.6), (2.0, 0.0), (1.0, 0.0), (0.0, 1.0), (0.0, 5.0), (1.0, 6.0), (3.0, 6.0), (4.0, 5.0), (4.0, 3.8), (3.0, 3.0), (1.0, 3.0), (0.0, 3.8)]],
        '7' => &[&[(0.0, 0.0), (4.0, 0.0), (1.6, 6.0)]],
        '8' => &[&[(1.0, 0.0), (3.0, 0.0), (4.0, 1.0), (4.0, 2.2), (3.0, 3.0), (1.0, 3.0), (0.0, 2.2), (0.0, 1.0), (1.0, 0.0)], &[(1.0, 3.0), (0.0, 3.8), (0.0, 5.0), (1.0, 6.0), (3.0, 6.0), (4.0, 5.0), (4.0, 3.8), (3.0, 3.0)]],
        '9' => &[&[(0.4, 5.4), (2.0, 6.0), (3.0, 6.0), (4.0, 5.0), (4.0, 1.0), (3.0, 0.0), (1.0, 0.0), (0.0, 1.0), (0.0, 2.2), (1.0, 3.0), (3.0, 3.0), (4.0, 2.2)]],
        _ => &[&[(0.0, 1.0), (1.0, 0.0), (3.0, 0.0), (4.0, 1.0), (4.0, 2.2), (2.0, 3.4), (2.0, 4.4)], &[(2.0, 5.6), (2.0, 5.7)]],
    }
}

/// Uppercase base letter of the first character (accents folded), the glyph set's alphabet only.
pub fn initial_of(name: &str) -> char {
    let c = name.trim().chars().next().unwrap_or('?');
    let folded = match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'À' | 'Á' | 'Â' | 'Ã' | 'Ä' | 'Å' => 'A',
        'ç' | 'Ç' => 'C',
        'è' | 'é' | 'ê' | 'ë' | 'È' | 'É' | 'Ê' | 'Ë' => 'E',
        'ì' | 'í' | 'î' | 'ï' | 'Ì' | 'Í' | 'Î' | 'Ï' => 'I',
        'ñ' | 'Ñ' => 'N',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'Ò' | 'Ó' | 'Ô' | 'Õ' | 'Ö' => 'O',
        'ù' | 'ú' | 'û' | 'ü' | 'Ù' | 'Ú' | 'Û' | 'Ü' => 'U',
        'ý' | 'ÿ' | 'Ý' => 'Y',
        c => c.to_ascii_uppercase(),
    };
    if folded.is_ascii_alphanumeric() { folded } else { '?' }
}

fn dist_to_segment(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (abx, aby) = (b.0 - a.0, b.1 - a.1);
    let len2 = abx * abx + aby * aby;
    let t = if len2 <= 0.0 { 0.0 } else { (((p.0 - a.0) * abx + (p.1 - a.1) * aby) / len2).clamp(0.0, 1.0) };
    let (qx, qy) = (a.0 + t * abx, a.1 + t * aby);
    ((p.0 - qx).powi(2) + (p.1 - qy).powi(2)).sqrt()
}

/// White initial centered in the plate, `k` = icon size / 1024.
fn draw_initial(img: &mut RgbaImage, name: &str, k: f32) {
    let strokes = glyph(initial_of(name));
    let unit = 52.0 * k;
    let half = 22.0 * k;
    let (cx, cy) = (img.width() as f32 / 2.0, img.width() as f32 / 2.0 + 16.0 * k);
    let to_px = |(x, y): (f32, f32)| (cx + (x - 2.0) * unit, cy + (y - 3.0) * unit);
    let (min_x, max_x) = (cx - 2.0 * unit - half - 2.0, cx + 2.0 * unit + half + 2.0);
    let (min_y, max_y) = (cy - 3.0 * unit - half - 2.0, cy + 3.0 * unit + half + 2.0);
    for y in min_y.floor().max(0.0) as u32..(max_y.ceil() as u32).min(img.height()) {
        for x in min_x.floor().max(0.0) as u32..(max_x.ceil() as u32).min(img.width()) {
            let p = (x as f32 + 0.5, y as f32 + 0.5);
            let mut d = f32::MAX;
            for s in strokes {
                for w in s.windows(2) {
                    d = d.min(dist_to_segment(p, to_px(w[0]), to_px(w[1])));
                }
            }
            let cov = (half + 0.5 - d).clamp(0.0, 1.0);
            if cov > 0.0 {
                blend(img.get_pixel_mut(x, y), [1.0, 1.0, 1.0], cov);
            }
        }
    }
}

/// App icon of `size` px (the reference design is 1024): `img` framed by `fit`, or, without an image, the initial of
/// `name` on the tint of `hue`.
pub fn compose(img: Option<&RgbaImage>, fit: Fit, name: &str, hue: f32, size: u32) -> RgbaImage {
    let k = size as f32 / SIZE as f32;
    let mut out = RgbaImage::new(size, size);
    let half_size = size as f32 / 2.0;
    if let (Some(src), Fit::Raw) = (img, fit) {
        // As is: contained in the whole square, no plate or shadow.
        let ratio = (size as f32 / src.width() as f32).min(size as f32 / src.height() as f32);
        draw_scaled(&mut out, src, ratio, half_size, half_size, |_, _| 1.0);
        return out;
    }
    let (plate, radius) = (PLATE * k, RADIUS * k);
    let half = plate / 2.0;
    let coverage = |x: u32, y: u32| rounded_square_coverage(x as f32 + 0.5, y as f32 + 0.5, half_size, half, radius);

    // Drop shadow of the plate, like system icons (offset 12, blur 28 => sigma 14).
    let mut shadow = RgbaImage::new(size, size);
    let offset = (12.0 * k).round() as u32;
    for y in 0..size {
        for x in 0..size {
            let cov = rounded_square_coverage(x as f32 + 0.5, y as f32 + 0.5 - offset as f32, half_size, half, radius);
            if cov > 0.0 {
                shadow.put_pixel(x, y, image::Rgba([0, 0, 0, (cov * 0.3 * 255.0).round() as u8]));
            }
        }
    }
    let shadow = imageops::blur(&shadow, (14.0 * k).max(0.5));
    imageops::overlay(&mut out, &shadow, 0, 0);

    // Plate: white behind an image, the space tint behind the initial.
    let base = if img.is_some() { [1.0, 1.0, 1.0] } else { hsl_rgb(hue, 0.5, 0.48) };
    for y in 0..size {
        for x in 0..size {
            let cov = coverage(x, y);
            if cov > 0.0 {
                blend(out.get_pixel_mut(x, y), base, cov);
            }
        }
    }
    match (img, fit) {
        (Some(src), Fit::Fill) => {
            // Covers the plate while keeping proportions.
            let ratio = (plate / src.width() as f32).max(plate / src.height() as f32);
            draw_scaled(&mut out, src, ratio, half_size, half_size, coverage);
        }
        (Some(src), _) => {
            let box_ = plate * 0.6;
            let ratio = (box_ / src.width() as f32).min(box_ / src.height() as f32);
            draw_scaled(&mut out, src, ratio, half_size, half_size, coverage);
            // Subtle hairline: a white plate stays visible on a light background.
            let line = (3.0 * k).max(1.0);
            for y in 0..size {
                for x in 0..size {
                    let edge = rounded_square_coverage(x as f32 + 0.5, y as f32 + 0.5, half_size, half, radius);
                    let inner = rounded_square_coverage(x as f32 + 0.5, y as f32 + 0.5, half_size, half - line, (radius - line).max(0.0));
                    let ring = (edge - inner).clamp(0.0, 1.0);
                    if ring > 0.0 {
                        blend(out.get_pixel_mut(x, y), [0.0, 0.0, 0.0], ring * 0.08);
                    }
                }
            }
        }
        (None, _) => {
            // Light-to-dark shade over the plate, then the initial.
            let top = half_size - half;
            for y in 0..size {
                for x in 0..size {
                    let cov = coverage(x, y);
                    if cov > 0.0 {
                        let f = ((y as f32 - top) / plate).clamp(0.0, 1.0);
                        let (white, black) = (0.18 * (1.0 - f), 0.12 * f);
                        blend(out.get_pixel_mut(x, y), [1.0, 1.0, 1.0], white * cov);
                        blend(out.get_pixel_mut(x, y), [0.0, 0.0, 0.0], black * cov);
                    }
                }
            }
            draw_initial(&mut out, name, k);
        }
    }
    out
}

/// PNG bytes of an image.
pub fn encode_png(img: &RgbaImage) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    DynamicImage::ImageRgba8(img.clone()).write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png).ok()?;
    Some(bytes)
}

/// `data:image/png;base64,...` of an image, empty if the encoding fails.
pub fn to_png_data_url(img: &RgbaImage) -> String {
    match encode_png(img) {
        Some(bytes) => format!("data:image/png;base64,{}", super::data_url::base64_encode(&bytes)),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, rgba: [u8; 4]) -> RgbaImage {
        RgbaImage::from_pixel(w, h, image::Rgba(rgba))
    }

    #[test]
    fn opaque_squares_fill_and_transparent_corners_fit() {
        let square = solid(256, 256, [200, 30, 30, 255]);
        assert!(opaque_corners(&square));
        assert_eq!(auto_fit(&square, false), Fit::Fill);
        let mut round = solid(256, 256, [200, 30, 30, 255]);
        for y in 0..40 {
            for x in 0..40 {
                round.put_pixel(x, y, image::Rgba([0, 0, 0, 0]));
            }
        }
        assert!(!opaque_corners(&round));
        assert_eq!(auto_fit(&round, false), Fit::Fit);
        assert_eq!(auto_fit(&round, true), Fit::Fill);
        // small icons are never stretched, even if opaque
        assert_eq!(auto_fit(&solid(64, 64, [1, 2, 3, 255]), false), Fit::Fit);
    }

    #[test]
    fn plate_is_rounded_and_centered() {
        let icon = compose(Some(&solid(200, 200, [255, 0, 0, 255])), Fit::Fill, "x", 200.0, 128);
        assert_eq!(icon.dimensions(), (128, 128));
        // outside the plate (corner): transparent; center: the image's red
        assert_eq!(icon.get_pixel(1, 1)[3], 0);
        let c = icon.get_pixel(64, 64);
        assert!(c[0] > 240 && c[1] < 15 && c[3] == 255);
        // the plate's corner is rounded: the pixel just inside the plate's bounding square is still empty
        let o = ((128.0 - 824.0 * 128.0 / 1024.0) / 2.0) as u32 + 1;
        assert!(icon.get_pixel(o, o)[3] < 255);
    }

    #[test]
    fn fit_leaves_a_white_margin() {
        let icon = compose(Some(&solid(200, 200, [0, 0, 255, 255])), Fit::Fit, "x", 0.0, 128);
        let margin = icon.get_pixel(64, 20);
        assert!(margin[0] > 240 && margin[1] > 240 && margin[2] > 240);
        let c = icon.get_pixel(64, 64);
        assert!(c[2] > 240 && c[0] < 15);
    }

    #[test]
    fn raw_covers_the_whole_square_without_a_plate() {
        let icon = compose(Some(&solid(100, 100, [0, 255, 0, 255])), Fit::Raw, "x", 0.0, 128);
        assert_eq!(icon.get_pixel(1, 1)[1], 255);
        assert_eq!(icon.get_pixel(126, 126)[3], 255);
    }

    #[test]
    fn without_an_image_the_initial_is_drawn_on_the_tint() {
        let with_letter = compose(None, Fit::Fit, "Docs", 120.0, 128);
        let blank = compose(None, Fit::Fit, "", 120.0, 128);
        assert_ne!(with_letter, blank);
        // the tint is green-ish at hue 120
        let p = with_letter.get_pixel(20, 64);
        assert!(p[1] > p[0] && p[1] > p[2]);
        // the letter's strokes are white somewhere near the center
        let whites = (40..90).flat_map(|y| (40..90).map(move |x| (x, y))).filter(|&(x, y)| with_letter.get_pixel(x, y)[0] > 230).count();
        assert!(whites > 50);
    }

    #[test]
    fn initials_fold_accents_and_unknown_characters() {
        assert_eq!(initial_of("  école"), 'E');
        assert_eq!(initial_of("docs"), 'D');
        assert_eq!(initial_of("7up"), '7');
        assert_eq!(initial_of("日本"), '?');
        assert_eq!(initial_of(""), '?');
    }

    #[test]
    fn png_round_trips_through_a_data_url() {
        let icon = compose(None, Fit::Fit, "A", 10.0, 64);
        let url = to_png_data_url(&icon);
        let (mime, bytes) = super::super::data_url::decode_data_url(&url).unwrap();
        assert_eq!(mime, "image/png");
        assert_eq!(load_image(&bytes).unwrap().dimensions(), (64, 64));
    }

    #[test]
    fn hsl_matches_css() {
        let [r, g, b] = hsl_rgb(0.0, 1.0, 0.5);
        assert!((r - 1.0).abs() < 1e-5 && g.abs() < 1e-5 && b.abs() < 1e-5);
        let [r, g, b] = hsl_rgb(240.0, 1.0, 0.5);
        assert!(r.abs() < 1e-5 && g.abs() < 1e-5 && (b - 1.0).abs() < 1e-5);
    }
}

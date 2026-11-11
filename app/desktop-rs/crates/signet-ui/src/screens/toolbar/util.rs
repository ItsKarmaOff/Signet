//! Helpers of the toolbar (port of the parts of `renderer/src/lib/util.ts` the bars use).

use crate::color::{from_hex, luminance};
use crate::theme::SignetTheme;
use gpui::{Hsla, Rgba, Window};
use std::time::Instant;
use signet_core::types::DevDevice;

/// Height of the pane bar (same value as `PANE_BAR` in main).
pub const PANE_BAR: f32 = 40.0;
/// Height of the bookmarks bar, under the pane bar (same value as `BOOKMARK_BAR` in main).
pub const BOOKMARK_BAR: f32 = 32.0;

/// True on macOS: shortcuts are written with the Command key there.
pub const IS_MAC: bool = cfg!(target_os = "macos");

/// Colors of a pane bar: the site color and the content colors that read well on it (`--bar-*`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BarColors {
    pub bg: Hsla,
    pub fg: Hsla,
    pub fg2: Hsla,
    pub hover: Hsla,
}

/// True if light content reads better than dark on a background of relative luminance `l` (WCAG contrast ratios).
pub fn light_on(l: f32) -> bool {
    1.05 / (l + 0.05) > (l + 0.05) / 0.05
}

fn white(a: f32) -> Hsla {
    Rgba { r: 1.0, g: 1.0, b: 1.0, a }.into()
}

fn black(a: f32) -> Hsla {
    Rgba { r: 0.0, g: 0.0, b: 0.0, a }.into()
}

/// The site's theme colour when it is a usable `#rrggbb` / `#rrggbbaa`.
pub fn site_color(color: Option<&str>) -> Option<Rgba> {
    color.and_then(|c| (c.len() == 7 || c.len() == 9).then(|| from_hex(c)).flatten())
}

/// Fill of the compact mode's floating bar inside its card. Over a blur (`card_alpha`: the card's translucent fill alpha)
/// the theme bar draws nothing (the card's frosted `bg` shows) and a site colour keeps its hue at the card's alpha;
/// without blur, the bar's own colour as before.
pub fn floating_bar_fill(bar_bg: Hsla, site_colored: bool, card_alpha: Option<f32>) -> Option<Hsla> {
    match card_alpha {
        None => Some(bar_bg),
        Some(_) if !site_colored => None,
        Some(a) => Some(Hsla { a, ..bar_bg }),
    }
}

/// Bar background = site color; light or dark content depending on contrast. No readable color: theme tokens.
pub fn bar_colors(color: Option<&str>, t: &SignetTheme) -> BarColors {
    let themed = BarColors { bg: t.surface, fg: t.text, fg2: t.text_2, hover: t.surface_2 };
    let Some(c) = site_color(color) else {
        return themed;
    };
    // `--bar-fg-2-alpha`: 0.5, raised in increased contrast.
    let alpha = if t.settings.high_contrast { 0.75 } else { 0.5 };
    let bg: Hsla = Rgba { a: 1.0, ..c }.into();
    if light_on(luminance(c)) {
        BarColors { bg, fg: white(0.92), fg2: white(alpha + 0.05), hover: white(0.1) }
    } else {
        BarColors { bg, fg: black(0.85), fg2: black(alpha), hover: black(0.1) }
    }
}

/// Cross-fade of the bar colors when the site color changes (`transition: background var(--t-theme)`, 240 ms).
/// Call [`BarFade::step`] on every render with the target colors: it returns the colors to draw.
#[derive(Default)]
pub struct BarFade {
    from: Option<BarColors>,
    to: Option<BarColors>,
    start: Option<Instant>,
}

fn blend(a: Hsla, b: Hsla, k: f32) -> Hsla {
    crate::color::mix(Rgba::from(a), Rgba::from(b), k).into()
}

impl BarFade {
    fn at(&self) -> Option<(BarColors, bool)> {
        let (from, to, start) = (self.from?, self.to?, self.start?);
        let k = (start.elapsed().as_secs_f32() / crate::theme::duration::THEME.as_secs_f32()).min(1.0);
        if k >= 1.0 {
            return Some((to, true));
        }
        let e = crate::motion::ease_out(k);
        Some((BarColors { bg: blend(from.bg, to.bg, e), fg: blend(from.fg, to.fg, e), fg2: blend(from.fg2, to.fg2, e), hover: blend(from.hover, to.hover, e) }, false))
    }

    /// Colors to draw now. The first target is shown as is; a new one fades in from what is displayed.
    pub fn step(&mut self, target: BarColors, reduced: bool, window: &mut Window) -> BarColors {
        if self.to != Some(target) {
            self.from = if reduced { None } else { self.at().map(|c| c.0).or(self.to) };
            self.to = Some(target);
            self.start = self.from.is_some().then(Instant::now);
        }
        match self.at() {
            Some((c, done)) => {
                if !done {
                    window.request_animation_frame();
                }
                c
            }
            None => target,
        }
    }
}

/// Splits an address into (host, port) for http-like URLs. Port is empty when absent.
pub fn host_port(url: &str) -> Option<(String, String)> {
    let (scheme, rest) = url.split_once("://")?;
    if scheme.is_empty() || !scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')) {
        return None;
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let authority = authority.rsplit('@').next().unwrap_or("");
    if authority.is_empty() {
        return None;
    }
    let (host, port) = if let Some(end) = authority.strip_prefix('[').and_then(|a| a.find(']')) {
        let host = &authority[..end + 2];
        let port = authority[end + 2..].strip_prefix(':').unwrap_or("");
        (host, port)
    } else {
        match authority.rsplit_once(':') {
            Some((h, p)) if p.chars().all(|c| c.is_ascii_digit()) => (h, p),
            _ => (authority, ""),
        }
    };
    Some((host.to_lowercase(), port.to_string()))
}

/// Domain displayed for a URL (without `www.`).
pub fn domain_of(url: &str) -> String {
    if url.len() >= 7 && url[..7].eq_ignore_ascii_case("signet:") {
        return url.trim_end_matches('/').to_string(); // internal address: shown in full
    }
    match host_port(url) {
        Some((host, _)) if !host.is_empty() => host.strip_prefix("www.").unwrap_or(&host).to_string(),
        _ => url.to_string(),
    }
}

pub fn is_secure(url: &str) -> bool {
    url.starts_with("https:")
}

/// True for `http:`, `https:`, `file:` and `signet:` addresses (the star button is shown).
pub fn is_bookmarkable(url: &str) -> bool {
    let l = url.to_ascii_lowercase();
    ["http:", "https:", "file:", "signet:"].iter().any(|p| l.starts_with(p))
}

pub fn is_http(url: &str) -> bool {
    let l = url.to_ascii_lowercase();
    l.starts_with("http:") || l.starts_with("https:")
}

/// Address without its trailing `/`.
pub fn norm(url: &str) -> &str {
    url.strip_suffix('/').unwrap_or(url)
}

/// Name of a language in the interface language; the code as is if unknown.
pub fn language_name(code: &str, locale: &str) -> String {
    crate::lang_names::base_language_name(code, locale.to_lowercase().starts_with("fr")).unwrap_or(code).to_string()
}

/// Count shown on a badge: 99+ beyond.
pub fn count99(n: u32) -> String {
    if n > 99 { "99+".into() } else { n.to_string() }
}

/// Device's own name (responsive mode); None = translated label (`develop.device.<id>`).
pub fn device_name(d: DevDevice) -> Option<&'static str> {
    Some(match d {
        DevDevice::IphoneSe => "iPhone SE",
        DevDevice::Iphone16 => "iPhone 16",
        DevDevice::Iphone16ProMax => "iPhone 16 Pro Max",
        DevDevice::Pixel9 => "Pixel 9",
        DevDevice::GalaxyS24 => "Galaxy S24",
        DevDevice::IpadMini => "iPad mini",
        DevDevice::IpadPro11 => "iPad Pro 11\"",
        DevDevice::Laptop | DevDevice::Desktop | DevDevice::Custom => return None,
    })
}

/// Catalog key suffix of a device without its own name.
pub fn device_key(d: DevDevice) -> &'static str {
    match d {
        DevDevice::Desktop => "desktop",
        DevDevice::Custom => "custom",
        _ => "laptop",
    }
}

/// Colors of an extension badge, like Chrome: the background the extension chose (a CSS color or Chrome's
/// `[r, g, b, a]` array) and black or white text, whichever contrasts better with it.
pub fn badge_colors(color: Option<&serde_json::Value>, text_color: Option<&serde_json::Value>) -> Option<(Hsla, Hsla)> {
    let rgb = color.and_then(rgb_of)?;
    let c = Rgba { r: rgb[0] as f32 / 255.0, g: rgb[1] as f32 / 255.0, b: rgb[2] as f32 / 255.0, a: 1.0 };
    let auto = if light_on(luminance(c)) { white(1.0) } else { black(0.85) };
    Some((c.into(), text_color.and_then(css_color).unwrap_or(auto)))
}

fn rgb_of(v: &serde_json::Value) -> Option<[u8; 3]> {
    use serde_json::Value;
    match v {
        Value::Array(a) if a.len() >= 3 && a.iter().take(3).all(|c| c.is_number()) => {
            let ch = |i: usize| a[i].as_f64().unwrap_or(0.0).round().clamp(0.0, 255.0) as u8;
            Some([ch(0), ch(1), ch(2)])
        }
        Value::String(s) => {
            let s = s.trim();
            if let Some(hex) = s.strip_prefix('#') {
                let full: String = match hex.len() {
                    3 => hex.chars().flat_map(|c| [c, c]).collect(),
                    6 => hex.to_string(),
                    _ => return None,
                };
                let n = u32::from_str_radix(&full, 16).ok()?;
                return Some([(n >> 16) as u8, (n >> 8) as u8, n as u8]);
            }
            let inner = s.strip_prefix("rgb")?.trim_start_matches('a').trim_start_matches('(').split(')').next()?;
            let mut nums = inner.split(|c: char| c == ',' || c.is_whitespace() || c == '/').filter(|p| !p.is_empty()).map(|p| p.parse::<f64>());
            let mut next = || nums.next()?.ok().map(|n| n.round().clamp(0.0, 255.0) as u8);
            Some([next()?, next()?, next()?])
        }
        _ => None,
    }
}

/// A color given to `setBadgeTextColor`: `[r, g, b, a]` (a: 0 to 255) or a hex / rgb() color.
pub fn css_color(v: &serde_json::Value) -> Option<Hsla> {
    let rgb = rgb_of(v)?;
    let alpha = match v {
        serde_json::Value::Array(a) => a.get(3).and_then(|x| x.as_f64()).map_or(1.0, |x| (x / 255.0).clamp(0.0, 1.0)),
        _ => 1.0,
    };
    Some(Rgba { r: rgb[0] as f32 / 255.0, g: rgb[1] as f32 / 255.0, b: rgb[2] as f32 / 255.0, a: alpha as f32 }.into())
}

/// Badge text for a blocked-request count, as Chrome writes it: the number, 999+ beyond.
pub fn count_text(n: Option<u32>) -> String {
    match n {
        Some(n) if n >= 1 => if n > 999 { "999+".into() } else { n.to_string() },
        _ => String::new(),
    }
}

/// Open in a new tab: Command-click on Mac, Ctrl-click elsewhere, or middle click.
pub fn wants_new_tab(modifiers: gpui::Modifiers, middle: bool) -> bool {
    (if IS_MAC { modifiers.platform } else { modifiers.control }) || middle
}

/// A rectangle in the api's coordinates from GPUI bounds.
/// A rectangle measured in a floating window, moved by the window's origin in the main window.
pub fn shifted(r: signet_core::types::Rect, origin: (f64, f64)) -> signet_core::types::Rect {
    signet_core::types::Rect::new(r.x + origin.0, r.y + origin.1, r.width, r.height)
}

pub fn rect_of(b: gpui::Bounds<gpui::Pixels>) -> signet_core::types::Rect {
    signet_core::types::Rect::new(f32::from(b.origin.x) as f64, f32::from(b.origin.y) as f64, f32::from(b.size.width) as f64, f32::from(b.size.height) as f64)
}

#[cfg(test)]
mod tests {
    #[test]
    fn floating_bar_fill_follows_the_blur() {
        use super::floating_bar_fill;
        let bg = gpui::hsla(0.3, 0.5, 0.4, 1.0);
        assert_eq!(floating_bar_fill(bg, false, None), Some(bg));
        assert_eq!(floating_bar_fill(bg, true, None), Some(bg));
        assert_eq!(floating_bar_fill(bg, false, Some(0.6)), None);
        let c = floating_bar_fill(bg, true, Some(0.6)).unwrap();
        assert_eq!((c.h, c.s, c.l, c.a), (0.3, 0.5, 0.4, 0.6));
    }

    use super::*;
    use serde_json::json;

    #[test]
    fn domain_strips_www_and_keeps_hostname() {
        assert_eq!(domain_of("https://www.example.com/a?b"), "example.com");
        assert_eq!(domain_of("http://sub.example.com:8080/"), "sub.example.com");
    }

    #[test]
    fn domain_shows_internal_addresses_in_full() {
        assert_eq!(domain_of("signet://settings/"), "signet://settings");
        assert_eq!(domain_of("SIGNET://note/abc"), "SIGNET://note/abc");
    }

    #[test]
    fn domain_returns_input_when_not_an_address() {
        assert_eq!(domain_of("pas une url"), "pas une url");
        assert_eq!(domain_of(""), "");
        assert_eq!(domain_of("file:///tmp/x.html"), "file:///tmp/x.html");
    }

    #[test]
    fn host_and_port() {
        assert_eq!(host_port("http://localhost:3000/x"), Some(("localhost".into(), "3000".into())));
        assert_eq!(host_port("https://user:pw@a.com/"), Some(("a.com".into(), String::new())));
        assert_eq!(host_port("http://[::1]:80/"), Some(("[::1]".into(), "80".into())));
        assert_eq!(host_port("nothing"), None);
    }

    #[test]
    fn secure_is_https_only() {
        assert!(is_secure("https://a.com"));
        assert!(!is_secure("http://a.com"));
        assert!(!is_secure("signet://x"));
    }

    #[test]
    fn bar_text_follows_contrast() {
        assert!(light_on(0.0));
        assert!(!light_on(1.0));
        assert!(light_on(luminance(from_hex("#101010").unwrap())));
        assert!(!light_on(luminance(from_hex("#f5f5f5").unwrap())));
    }

    #[test]
    fn language_names() {
        assert_eq!(language_name("de", "en"), "German");
        assert_eq!(language_name("de", "fr"), "allemand");
        assert_eq!(language_name("pt-BR", "en"), "Portuguese");
        assert_eq!(language_name("!!", "en"), "!!");
    }

    #[test]
    fn badge_colors_pick_readable_text() {
        let (bg, fg) = badge_colors(Some(&json!([255, 255, 255, 255])), None).unwrap();
        assert!(bg.l > 0.9);
        assert_eq!(fg, black(0.85));
        let (_, fg) = badge_colors(Some(&json!("#101010")), None).unwrap();
        assert_eq!(fg, white(1.0));
        let (_, fg) = badge_colors(Some(&json!("#fff")), Some(&json!([255, 0, 0, 255]))).unwrap();
        assert!(fg.s > 0.9);
        assert!(badge_colors(Some(&json!("red")), None).is_none());
        assert!(badge_colors(None, None).is_none());
    }

    #[test]
    fn css_colors() {
        assert!(css_color(&json!([1, 2, 3, 0])).unwrap().a < 0.01);
        assert!(css_color(&json!("rgb(10 20 30)")).is_some());
        assert!(css_color(&json!("nope")).is_none());
    }

    #[test]
    fn counts() {
        assert_eq!(count_text(None), "");
        assert_eq!(count_text(Some(0)), "");
        assert_eq!(count_text(Some(12)), "12");
        assert_eq!(count_text(Some(1500)), "999+");
        assert_eq!(count99(100), "99+");
    }

    #[test]
    fn bookmarkable_addresses() {
        assert!(is_bookmarkable("HTTPS://a.com"));
        assert!(is_bookmarkable("signet://notes"));
        assert!(!is_bookmarkable("about:blank"));
        assert_eq!(norm("https://a.com/"), "https://a.com");
    }
}

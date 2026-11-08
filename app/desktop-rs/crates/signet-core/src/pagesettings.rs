//! Ports `app/desktop/src/shared/pagesettings.ts`: web page settings (choice lists and mappings).

use indexmap::IndexMap;
use serde_json::Value;
use std::collections::HashSet;

/// Default font size choices in px (Chrome's five levels). 16 is the default.
pub const FONT_SIZES: [u32; 5] = [9, 12, 16, 20, 24];
pub const DEFAULT_FONT_SIZE: u32 = 16;
/// Minimum font size choices in px; 0 = none (the default).
pub const MIN_FONT_SIZES: [u32; 10] = [0, 6, 8, 10, 12, 14, 16, 18, 20, 24];
/// Default page zoom choices in percent. 100 is the default.
pub const ZOOM_PERCENTS: [u32; 17] = [25, 33, 50, 67, 75, 80, 90, 100, 110, 125, 150, 175, 200, 250, 300, 400, 500];
pub const DEFAULT_ZOOM: u32 = 100;

pub fn is_font_size(v: u32) -> bool {
    FONT_SIZES.contains(&v)
}
pub fn is_min_font_size(v: u32) -> bool {
    MIN_FONT_SIZES.contains(&v)
}
pub fn is_zoom_percent(v: u32) -> bool {
    ZOOM_PERCENTS.contains(&v)
}

#[derive(Debug, Clone, PartialEq)]
pub struct FontPreferences {
    pub default_font_size: u32,
    pub default_monospace_font_size: u32,
    pub minimum_font_size: u32,
}

/// Font preferences from the settings. Like Chrome, the monospace size stays 3 px under the default one at 16 px.
pub fn font_preferences(font_size: Option<u32>, minimum_font_size: Option<u32>) -> FontPreferences {
    let size = font_size.filter(|v| is_font_size(*v)).unwrap_or(DEFAULT_FONT_SIZE);
    FontPreferences {
        default_font_size: size,
        default_monospace_font_size: ((f64::from(size) * 13.0 / 16.0) + 0.5).floor() as u32,
        minimum_font_size: minimum_font_size.filter(|v| is_min_font_size(*v)).unwrap_or(0),
    }
}

/// Host whose zoom Chromium shares between tabs: the host name of an http(s) page, None for anything else.
pub fn zoom_host(address: &str) -> Option<String> {
    let u = url::Url::parse(address).ok()?;
    if u.scheme() != "http" && u.scheme() != "https" {
        return None;
    }
    if u.path().to_lowercase().ends_with(".pdf") {
        return None; // the PDF viewer has its own zoom
    }
    u.host_str().map(str::to_lowercase).filter(|h| !h.is_empty())
}

/// Most hosts whose zoom is remembered (the oldest are forgotten first).
pub const SITE_ZOOM_MAX: usize = 500;
const ZOOM_LEVEL_MIN: f64 = -8.0;
const ZOOM_LEVEL_MAX: f64 = 9.0;

/// Zoom levels the user chose per host, oldest first.
pub type SiteZoom = IndexMap<String, f64>;
/// Zoom memory of each isolated space (key: space id).
pub type SiteZoomBySpace = IndexMap<String, SiteZoom>;
const SITE_ZOOM_SPACES_MAX: usize = 200;

/// Zoom memory read from disk: valid hosts and levels only, at most `SITE_ZOOM_MAX` (the last ones).
pub fn clean_site_zoom(raw: &Value) -> SiteZoom {
    let Some(obj) = raw.as_object() else { return SiteZoom::new() };
    let entries: Vec<(String, f64)> = obj
        .iter()
        .filter_map(|(k, v)| {
            let n = v.as_f64()?;
            (!k.is_empty() && k.chars().count() <= 253 && n.is_finite() && (ZOOM_LEVEL_MIN..=ZOOM_LEVEL_MAX).contains(&n)).then(|| (k.clone(), n))
        })
        .collect();
    let skip = entries.len().saturating_sub(SITE_ZOOM_MAX);
    entries.into_iter().skip(skip).collect()
}

/// Per-space zoom memory read from disk: cleaned maps only, empty ones dropped.
pub fn clean_site_zoom_by_space(raw: &Value) -> SiteZoomBySpace {
    let Some(obj) = raw.as_object() else { return SiteZoomBySpace::new() };
    let all: Vec<(String, SiteZoom)> = obj
        .iter()
        .filter(|(id, _)| !id.is_empty() && id.chars().count() <= 100)
        .map(|(id, m)| (id.clone(), clean_site_zoom(m)))
        .filter(|(_, m)| !m.is_empty())
        .collect();
    let skip = all.len().saturating_sub(SITE_ZOOM_SPACES_MAX);
    all.into_iter().skip(skip).collect()
}

/// Remembers a host's zoom level as the most recent one, within the bound.
pub fn remember_site_zoom(map: &mut SiteZoom, host: &str, level: f64) {
    map.shift_remove(host);
    map.insert(host.to_string(), level);
    while map.len() > SITE_ZOOM_MAX {
        map.shift_remove_index(0);
    }
}

/// Zoom factor to apply to a page that just committed, or None to leave it alone.
pub fn default_zoom_factor(default_percent: Option<u32>, address: &str, user_hosts: &HashSet<String>, current_factor: f64) -> Option<f64> {
    let pct = default_percent.filter(|p| is_zoom_percent(*p))?;
    let host = zoom_host(address)?;
    if user_hosts.contains(&host) {
        return None;
    }
    let target = f64::from(pct) / 100.0;
    if (current_factor - target).abs() < 0.005 { None } else { Some(target) }
}

/// Page zoom in percent for a zoom factor.
pub fn zoom_percent_of(factor: f64) -> u32 {
    (factor * 100.0 + 0.5).floor() as u32
}

/// Zoom percent to report for a page: None when it does not follow page zoom (internal page, PDF).
pub fn reported_zoom(address: &str, factor: f64) -> Option<u32> {
    zoom_host(address).map(|_| zoom_percent_of(factor))
}

/// The default zoom in percent (100 without a valid choice).
pub fn default_zoom_percent(setting: Option<u32>) -> u32 {
    setting.filter(|p| is_zoom_percent(*p)).unwrap_or(DEFAULT_ZOOM)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoomIndicator {
    In,
    Out,
}

/// Toolbar zoom indicator: shown when the page zoom differs from the default.
pub fn zoom_indicator(percent: Option<u32>, default_percent: Option<u32>) -> Option<ZoomIndicator> {
    let p = percent?;
    let def = default_zoom_percent(default_percent);
    if p == def { None } else if p > def { Some(ZoomIndicator::In) } else { Some(ZoomIndicator::Out) }
}

/// "110%" in English, "110 %" (narrow no-break space) in French.
pub fn format_zoom_percent(percent: u32, locale: &str) -> String {
    if locale.to_lowercase().starts_with("fr") { format!("{percent}\u{202f}%") } else { format!("{percent}%") }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn choice_lists() {
        assert!(is_font_size(16) && !is_font_size(15));
        assert!(is_min_font_size(0) && is_min_font_size(12) && !is_min_font_size(7));
        assert!(is_zoom_percent(125) && !is_zoom_percent(101));
    }

    #[test]
    fn fonts() {
        assert_eq!(font_preferences(None, None), FontPreferences { default_font_size: 16, default_monospace_font_size: 13, minimum_font_size: 0 });
        assert_eq!(font_preferences(Some(24), Some(12)), FontPreferences { default_font_size: 24, default_monospace_font_size: 20, minimum_font_size: 12 });
        assert_eq!(font_preferences(Some(15), Some(7)).default_font_size, 16);
    }

    #[test]
    fn hosts_and_zoom() {
        assert_eq!(zoom_host("https://Example.com/a").as_deref(), Some("example.com"));
        assert_eq!(zoom_host("signet://settings"), None);
        assert_eq!(zoom_host("https://a.com/doc.PDF"), None);
        assert_eq!(reported_zoom("https://a.com", 1.1), Some(110));
        assert_eq!(reported_zoom("file:///a", 1.1), None);
        let none = HashSet::new();
        assert_eq!(default_zoom_factor(Some(125), "https://a.com", &none, 1.0), Some(1.25));
        assert_eq!(default_zoom_factor(Some(125), "https://a.com", &none, 1.25), None);
        assert_eq!(default_zoom_factor(None, "https://a.com", &none, 1.0), None);
        let users: HashSet<String> = ["a.com".to_string()].into();
        assert_eq!(default_zoom_factor(Some(125), "https://a.com", &users, 1.0), None);
        assert_eq!(zoom_indicator(Some(110), None), Some(ZoomIndicator::In));
        assert_eq!(zoom_indicator(Some(90), None), Some(ZoomIndicator::Out));
        assert_eq!(zoom_indicator(Some(100), None), None);
        assert_eq!(zoom_indicator(None, None), None);
        assert_eq!(format_zoom_percent(110, "fr"), "110\u{202f}%");
        assert_eq!(format_zoom_percent(110, "en"), "110%");
    }

    #[test]
    fn memory() {
        let z = clean_site_zoom(&json!({"a.com": 2, "b.com": "big", "c.com": 99}));
        assert_eq!(z.keys().collect::<Vec<_>>(), ["a.com"]);
        assert!(clean_site_zoom(&Value::Null).is_empty());
        let by = clean_site_zoom_by_space(&json!({"s1": {"a.com": 2, "b.com": "big"}, "s2": {}, "s3": 5}));
        assert_eq!(by.keys().collect::<Vec<_>>(), ["s1"]);
        let mut m = SiteZoom::new();
        for i in 0..=SITE_ZOOM_MAX {
            remember_site_zoom(&mut m, &format!("h{i}.com"), 1.0);
        }
        assert_eq!(m.len(), SITE_ZOOM_MAX);
        assert!(!m.contains_key("h0.com"));
        remember_site_zoom(&mut m, "h1.com", 2.0);
        assert_eq!(m.last().unwrap().0, "h1.com");
    }
}

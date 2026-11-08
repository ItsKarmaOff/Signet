//! Toolbar button state of extensions: enable/disable, badge text colour, `setIcon` image data. Port of `apis/action.ts`.
//! Enabled state and badge text colour are set globally or per tab (a tab's value wins). Kept in memory, like Chrome:
//! extensions set them again at startup. Badge text, background colour, title and popup per tab are in [`ActionModel`].

use crate::common::{ApiError, ApiResult};
use base64::Engine;
use serde_json::{json, Map, Value};
use std::collections::HashMap;

/// Chrome's default badge text color.
pub fn default_text_color() -> Value {
    json!([255, 255, 255, 255])
}

#[derive(Debug, Clone, Default, PartialEq)]
struct Slot {
    enabled: Option<bool>,
    text_color: Option<Value>,
}

/// Is a `tabId` argument a tab? (`None` means the whole action.)
pub fn tab_of(details: &Value) -> Option<i64> {
    let id = match details {
        Value::Object(o) => o.get("tabId"),
        other => Some(other),
    };
    id.and_then(|v| v.as_i64()).filter(|n| *n >= 0)
}

/// Colors `setBadgeTextColor` takes: `[r, g, b, a]` or a CSS color string.
pub fn parse_color(color: &Value) -> ApiResult<Value> {
    if let Some(s) = color.as_str().map(str::trim).filter(|s| !s.is_empty()) {
        return Ok(json!(s));
    }
    if let Some(a) = color.as_array().filter(|a| a.len() == 4 && a.iter().all(|c| c.as_f64().is_some_and(|c| (0.0..=255.0).contains(&c)))) {
        return Ok(Value::Array(a.iter().map(|c| json!(c.as_f64().unwrap_or(0.0).round() as i64)).collect()));
    }
    Err(ApiError::new("Invalid value for argument 1. Expected a color."))
}

#[derive(Default)]
pub struct ActionStates {
    global: HashMap<String, Slot>,
    tabs: HashMap<String, HashMap<i64, Slot>>,
    changed: bool,
}

impl ActionStates {
    pub fn new() -> Self {
        Self::default()
    }

    /// True once after something changed: push `snapshot()` to the toolbar.
    pub fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }

    fn slot(&mut self, ext: &str, tab: Option<i64>) -> &mut Slot {
        match tab {
            None => self.global.entry(ext.to_string()).or_default(),
            Some(t) => self.tabs.entry(ext.to_string()).or_default().entry(t).or_default(),
        }
    }

    pub fn set_enabled(&mut self, ext: &str, tab: Option<i64>, enabled: bool) {
        self.slot(ext, tab).enabled = Some(enabled);
        self.changed = true;
    }

    pub fn is_enabled(&self, ext: &str, tab: Option<i64>) -> bool {
        self.tabs.get(ext).and_then(|t| t.get(&tab.unwrap_or(-1))).and_then(|s| s.enabled).or_else(|| self.global.get(ext).and_then(|s| s.enabled)).unwrap_or(true)
    }

    pub fn set_text_color(&mut self, ext: &str, tab: Option<i64>, color: Value) {
        self.slot(ext, tab).text_color = Some(color);
        self.changed = true;
    }

    pub fn text_color(&self, ext: &str, tab: Option<i64>) -> Value {
        self.tabs
            .get(ext)
            .and_then(|t| t.get(&tab.unwrap_or(-1)))
            .and_then(|s| s.text_color.clone())
            .or_else(|| self.global.get(ext).and_then(|s| s.text_color.clone()))
            .unwrap_or_else(default_text_color)
    }

    pub fn forget_tab(&mut self, tab: i64) {
        let mut any = false;
        for per in self.tabs.values_mut() {
            any |= per.remove(&tab).is_some();
        }
        self.changed |= any;
    }

    pub fn forget_extension(&mut self, ext: &str) {
        let a = self.global.remove(ext).is_some();
        let b = self.tabs.remove(ext).is_some();
        self.changed |= a || b;
    }

    /// Only what differs from the defaults, for the toolbar: `{ extId: { enabled?, textColor?, tabs?: { tabId: {...} } } }`.
    pub fn snapshot(&self) -> Value {
        let state = |s: &Slot| {
            let mut m = Map::new();
            if let Some(e) = s.enabled {
                m.insert("enabled".into(), json!(e));
            }
            if let Some(c) = &s.text_color {
                m.insert("textColor".into(), c.clone());
            }
            m
        };
        let mut out: Map<String, Value> = Map::new();
        for (id, s) in &self.global {
            if s.enabled.is_some() || s.text_color.is_some() {
                out.insert(id.clone(), Value::Object(state(s)));
            }
        }
        for (id, per) in &self.tabs {
            for (tab, s) in per {
                if s.enabled.is_none() && s.text_color.is_none() {
                    continue;
                }
                let entry = out.entry(id.clone()).or_insert_with(|| json!({}));
                let tabs = entry.as_object_mut().expect("object").entry("tabs").or_insert_with(|| json!({}));
                tabs[tab.to_string()] = Value::Object(state(s));
            }
        }
        Value::Object(out)
    }

    pub fn call(&mut self, ext: &str, method: &str, args: &[Value]) -> ApiResult<Value> {
        let a = args.first().cloned().unwrap_or(Value::Null);
        match method {
            "action.enable" => {
                self.set_enabled(ext, tab_of(&a), true);
                Ok(Value::Null)
            }
            "action.disable" => {
                self.set_enabled(ext, tab_of(&a), false);
                Ok(Value::Null)
            }
            "action.isEnabled" => Ok(json!(self.is_enabled(ext, tab_of(&a)))),
            "action.setBadgeTextColor" => {
                let color = parse_color(a.get("color").unwrap_or(&Value::Null))?;
                self.set_text_color(ext, tab_of(&a), color);
                Ok(Value::Null)
            }
            "action.getBadgeTextColor" => Ok(self.text_color(ext, tab_of(&a))),
            _ => Err(ApiError::new(format!("Unknown Extension API - {method}"))),
        }
    }
}

/// Badge text, badge background, title and popup, global and per tab (`action.setBadgeText`...). In Electron the library
/// kept these; the new host has to, so the toolbar can draw the button.
#[derive(Default)]
pub struct ActionModel {
    global: HashMap<String, Map<String, Value>>,
    tabs: HashMap<String, HashMap<i64, Map<String, Value>>>,
}

const MODEL_KEYS: [(&str, &str, &str); 4] = [
    ("setBadgeText", "getBadgeText", "text"),
    ("setBadgeBackgroundColor", "getBadgeBackgroundColor", "color"),
    ("setTitle", "getTitle", "title"),
    ("setPopup", "getPopup", "popup"),
];

impl ActionModel {
    pub fn new() -> Self {
        Self::default()
    }

    fn set(&mut self, ext: &str, tab: Option<i64>, key: &str, value: Value) {
        let map = match tab {
            None => self.global.entry(ext.to_string()).or_default(),
            Some(t) => self.tabs.entry(ext.to_string()).or_default().entry(t).or_default(),
        };
        map.insert(key.to_string(), value);
    }

    /// Value of a property for a tab: the tab's own, else the global one.
    pub fn get(&self, ext: &str, tab: Option<i64>, key: &str) -> Option<Value> {
        tab.and_then(|t| self.tabs.get(ext)?.get(&t)?.get(key).cloned()).or_else(|| self.global.get(ext)?.get(key).cloned())
    }

    pub fn forget_tab(&mut self, tab: i64) {
        for per in self.tabs.values_mut() {
            per.remove(&tab);
        }
    }

    pub fn forget_extension(&mut self, ext: &str) {
        self.global.remove(ext);
        self.tabs.remove(ext);
    }

    /// `action.set*` / `get*` (and the `browserAction` aliases). Returns Some(value) for the getters.
    pub fn call(&mut self, ext: &str, method: &str, args: &[Value]) -> ApiResult<Option<Value>> {
        let name = method.strip_prefix("action.").or_else(|| method.strip_prefix("browserAction.")).ok_or_else(|| ApiError::new(format!("Unknown Extension API - {method}")))?;
        let details = args.first().cloned().unwrap_or(Value::Null);
        for (setter, getter, key) in MODEL_KEYS {
            if name == setter {
                let value = details.get(key).cloned().unwrap_or(Value::Null);
                if key == "color" {
                    parse_color(&value)?;
                }
                self.set(ext, tab_of(&details), key, value);
                return Ok(None);
            }
            if name == getter {
                let default = match key {
                    "text" => json!(""),
                    "color" => json!([217, 48, 37, 255]),
                    _ => Value::Null,
                };
                return Ok(Some(self.get(ext, tab_of(&details), key).unwrap_or(default)));
            }
        }
        Err(ApiError::new(format!("Unknown Extension API - {method}")))
    }
}

/// Image data as it crosses from the page: `ImageData` itself does not travel, its parts do.
const MAX_SIDE: i64 = 512;

/// Validates an image and returns `(width, height, rgba)`.
pub fn read_image(image: &Value) -> ApiResult<(u32, u32, Vec<u8>)> {
    let bad = || ApiError::new("Invalid image data.");
    let w = image.get("width").and_then(|w| w.as_i64()).ok_or_else(bad)?;
    let h = image.get("height").and_then(|w| w.as_i64()).ok_or_else(bad)?;
    if w < 1 || h < 1 || w > MAX_SIDE || h > MAX_SIDE {
        return Err(bad());
    }
    let data: Vec<u8> = match image.get("data") {
        Some(Value::Array(a)) => a.iter().map(|n| n.as_u64().unwrap_or(0).min(255) as u8).collect(),
        // Typed arrays can also arrive as `{ "0": r, "1": g, ... }`.
        Some(Value::Object(o)) => (0..o.len()).map(|i| o.get(&i.to_string()).and_then(|n| n.as_u64()).unwrap_or(0).min(255) as u8).collect(),
        _ => return Err(bad()),
    };
    if data.len() != (w * h * 4) as usize {
        return Err(bad());
    }
    Ok((w as u32, h as u32, data))
}

/// RGBA pixels to BGRA, the order some native image constructors expect.
pub fn rgba_to_bgra(rgba: &[u8]) -> Vec<u8> {
    rgba.chunks_exact(4).flat_map(|p| [p[2], p[1], p[0], p[3]]).collect()
}

fn png_data_url(width: u32, height: u32, rgba: &[u8]) -> ApiResult<String> {
    let mut buf = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut buf, width, height);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().map_err(|_| ApiError::new("Invalid image data."))?;
        w.write_image_data(rgba).map_err(|_| ApiError::new("Invalid image data."))?;
    }
    Ok(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(buf)))
}

/// `imageData` of `setIcon`: one image or a dictionary of sizes to images, as PNG data URLs keyed by width.
pub fn png_icons(image_data: &Value) -> ApiResult<Vec<(String, String)>> {
    let bad = || ApiError::new("Invalid image data.");
    let obj = image_data.as_object().ok_or_else(bad)?;
    let mut out = Vec::new();
    if obj.contains_key("width") && obj.contains_key("data") {
        let (w, h, px) = read_image(image_data)?;
        out.push((w.to_string(), png_data_url(w, h, &px)?));
    } else {
        if obj.is_empty() {
            return Err(bad());
        }
        for image in obj.values() {
            let (w, h, px) = read_image(image)?;
            out.push((w.to_string(), png_data_url(w, h, &px)?));
        }
    }
    Ok(out)
}

/// What `action.setIcon` with `imageData` asks the toolbar to draw.
#[derive(Debug, Clone, PartialEq)]
pub struct IconUpdate {
    pub extension_id: String,
    pub tab_id: Option<i64>,
    /// Width to PNG data URL.
    pub images: Vec<(String, String)>,
}

pub fn set_icon(ext: &str, details: &Value) -> ApiResult<IconUpdate> {
    Ok(IconUpdate { extension_id: ext.into(), tab_id: tab_of(details), images: png_icons(details.get("imageData").unwrap_or(&Value::Null))? })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(w: usize, h: usize, fill: [u8; 4]) -> Value {
        json!({ "width": w, "height": h, "data": (0..w * h).flat_map(|_| fill).collect::<Vec<u8>>() })
    }

    #[test]
    fn enabled_by_default_global_disable_and_tab_override() {
        let mut s = ActionStates::new();
        assert!(s.is_enabled("e", Some(1)));
        s.set_enabled("e", None, false);
        assert!(!s.is_enabled("e", None));
        assert!(!s.is_enabled("e", Some(1)));
        s.set_enabled("e", Some(1), true);
        assert!(s.is_enabled("e", Some(1)));
        assert!(!s.is_enabled("e", Some(2)));
        assert!(s.is_enabled("other", Some(1)));
    }

    #[test]
    fn a_per_tab_disable_goes_away_with_the_tab() {
        let mut s = ActionStates::new();
        s.set_enabled("e", Some(3), false);
        assert!(!s.is_enabled("e", Some(3)));
        assert!(s.is_enabled("e", Some(4)));
        s.forget_tab(3);
        assert!(s.is_enabled("e", Some(3)));
    }

    #[test]
    fn badge_text_color_default_white_tab_over_global() {
        let mut s = ActionStates::new();
        assert_eq!(s.text_color("e", Some(1)), json!([255, 255, 255, 255]));
        s.set_text_color("e", None, json!("#000"));
        s.set_text_color("e", Some(2), json!([1, 2, 3, 255]));
        assert_eq!(s.text_color("e", Some(1)), json!("#000"));
        assert_eq!(s.text_color("e", Some(2)), json!([1, 2, 3, 255]));
    }

    #[test]
    fn snapshot_holds_only_what_differs_and_reports_changes() {
        let mut s = ActionStates::new();
        assert_eq!(s.snapshot(), json!({}));
        s.set_enabled("e", None, false);
        s.set_enabled("e", Some(5), true);
        s.set_text_color("f", Some(5), json!("red"));
        assert_eq!(s.snapshot(), json!({ "e": { "enabled": false, "tabs": { "5": { "enabled": true } } }, "f": { "tabs": { "5": { "textColor": "red" } } } }));
        assert!(s.take_changed());
        assert!(!s.take_changed());
        s.forget_extension("e");
        assert_eq!(s.snapshot(), json!({ "f": { "tabs": { "5": { "textColor": "red" } } } }));
    }

    #[test]
    fn call_reads_the_tab_from_details_or_a_bare_number_and_validates_colors() {
        let mut s = ActionStates::new();
        s.call("e", "action.disable", &[json!(7)]).unwrap();
        assert_eq!(s.call("e", "action.isEnabled", &[json!(7)]).unwrap(), json!(false));
        assert_eq!(s.call("e", "action.isEnabled", &[Value::Null]).unwrap(), json!(true));
        s.call("e", "action.setBadgeTextColor", &[json!({ "color": [0, 0, 0, 255], "tabId": 7 })]).unwrap();
        assert_eq!(s.call("e", "action.getBadgeTextColor", &[json!({ "tabId": 7 })]).unwrap(), json!([0, 0, 0, 255]));
        assert!(s.call("e", "action.setBadgeTextColor", &[json!({ "color": 12 })]).is_err());
        assert_eq!(parse_color(&json!(" #fff ")).unwrap(), json!("#fff"));
    }

    #[test]
    fn model_stores_badge_text_title_popup_per_tab() {
        let mut m = ActionModel::new();
        assert_eq!(m.call("e", "action.getBadgeText", &[json!({})]).unwrap(), Some(json!("")));
        m.call("e", "action.setBadgeText", &[json!({ "text": "9" })]).unwrap();
        m.call("e", "action.setBadgeText", &[json!({ "text": "3", "tabId": 2 })]).unwrap();
        assert_eq!(m.call("e", "action.getBadgeText", &[json!({ "tabId": 2 })]).unwrap(), Some(json!("3")));
        assert_eq!(m.call("e", "action.getBadgeText", &[json!({ "tabId": 4 })]).unwrap(), Some(json!("9")));
        m.call("e", "browserAction.setPopup", &[json!({ "popup": "p.html" })]).unwrap();
        assert_eq!(m.get("e", Some(1), "popup"), Some(json!("p.html")));
        assert!(m.call("e", "action.setBadgeBackgroundColor", &[json!({ "color": 5 })]).is_err());
        m.forget_tab(2);
        assert_eq!(m.call("e", "action.getBadgeText", &[json!({ "tabId": 2 })]).unwrap(), Some(json!("9")));
    }

    #[test]
    fn image_data_validation_and_bgra_order() {
        let (w, h, px) = read_image(&image(2, 1, [10, 20, 30, 40])).unwrap();
        assert_eq!((w, h), (2, 1));
        assert_eq!(rgba_to_bgra(&px), [30, 20, 10, 40, 30, 20, 10, 40]);
        assert!(read_image(&json!({ "width": 2, "height": 2, "data": [0, 0, 0, 0] })).is_err());
        assert!(read_image(&json!({ "width": 0, "height": 1, "data": [] })).is_err());
        assert!(read_image(&Value::Null).is_err());
        assert!(read_image(&json!({ "width": 9999, "height": 1, "data": [] })).is_err());
    }

    #[test]
    fn one_image_or_a_dictionary_gives_png_data_urls_keyed_by_width() {
        let one = png_icons(&image(1, 1, [0, 0, 0, 0])).unwrap();
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].0, "1");
        assert!(one[0].1.starts_with("data:image/png;base64,iVBORw0KGgo"));
        let many = png_icons(&json!({ "16": image(2, 2, [0, 0, 0, 0]), "32": image(4, 4, [0, 0, 0, 0]) })).unwrap();
        assert_eq!(many.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(), ["2", "4"]);
        assert!(png_icons(&json!({})).is_err());
        assert!(png_icons(&json!("x")).is_err());
    }

    #[test]
    fn set_icon_hands_the_icons_and_the_tab_to_the_button() {
        let u = set_icon("e", &json!({ "tabId": 3, "imageData": image(1, 1, [1, 2, 3, 4]) })).unwrap();
        assert_eq!((u.extension_id.as_str(), u.tab_id, u.images.len()), ("e", Some(3), 1));
    }
}

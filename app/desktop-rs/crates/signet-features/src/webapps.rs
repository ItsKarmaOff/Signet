//! Web apps, sites installed as apps (port of the data and rules of `main/features/webapps.ts`): registry in
//! `webapps.json`, scope matching, manifest and icon choice, launcher data per platform. Windows, overlays and
//! the actual file writing of launchers (icns, lsregister) belong to the app.
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine;
use regex::Regex;
use serde_json::Value;
use url::Url;

use crate::http::Http;
use crate::util::{read_json, write_json};

/// Isolated world of the script that reads the page's manifest and icons.
pub const WORLD: i32 = 1005;
/// Height of an app window's title bar.
pub const WEBAPP_BAR: u32 = 38;
pub const CASCADE: i32 = 24;
/// Prefix of launcher identifiers: used to recognize our own before deleting them. (Rust app bundle id.)
pub const MAIN_BUNDLE_ID: &str = "dev.signet.app.gpui";
pub const BUNDLE_PREFIX: &str = "dev.signet.app.gpui.webapp.";
/// Launch argument (development: the launcher starts the app, which hands over to the open instance).
pub const LAUNCH_ARG: &str = "--signet-webapp=";
pub const MAX_ICON_BYTES: usize = 5_000_000;
pub const MAX_MANIFEST_BYTES: usize = 512_000;
pub const MAX_NAME: usize = 60;

/// Reads the manifest, declared icons and app name from the page (isolated world). Evaluated as an expression.
pub const PAGE_SCRIPT: &str = include_str!("assets/webapp-page.js");

fn is_http(url: &str) -> bool {
    let l = url.to_ascii_lowercase();
    l.starts_with("http://") || l.starts_with("https://")
}

fn s(v: Option<&Value>, max: usize) -> String {
    v.and_then(|x| x.as_str()).map(|t| t.chars().take(max).collect()).unwrap_or_default()
}

/// Simple CSS color (#rgb, #rrggbb, rgb(...), hsl(...)); the rest is ignored.
pub fn clean_color(v: Option<&Value>) -> Option<String> {
    let c = s(v, 64);
    let c = c.trim();
    let hex = Regex::new(r"(?i)^#[0-9a-f]{3,8}$").ok()?;
    let func = Regex::new(r"(?i)^(rgb|hsl)a?\([\d\s.,%/+-]+\)$").ok()?;
    (hex.is_match(c) || func.is_match(c)).then(|| c.to_string())
}

/// Name: control characters removed, spaces tightened, capped at 60 characters.
pub fn clean_app_name(v: &Value) -> String {
    let raw: String = v.as_str().map(|t| t.chars().take(400).collect()).unwrap_or_default();
    let collapsed = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let no_ctl: String = collapsed.chars().filter(|c| !c.is_control()).collect();
    no_ctl.trim().chars().take(MAX_NAME).collect()
}

fn clean_name_str(t: &str) -> String {
    clean_app_name(&Value::String(t.into()))
}

/// Absolute address resolved against `base`, or `None`.
pub fn resolve(href: Option<&Value>, base: &str) -> Option<String> {
    let h = s(href, 2048);
    Url::parse(base).ok()?.join(&h).ok().map(|u| u.to_string())
}

pub fn origin_of(url: &str) -> String {
    Url::parse(url).map(|u| u.origin().ascii_serialization()).unwrap_or_default()
}

/// Address without fragment, normalized (host's trailing slash included).
pub fn normalize(url: &str) -> String {
    match Url::parse(url) {
        Ok(mut u) => {
            u.set_fragment(None);
            u.to_string()
        }
        Err(_) => url.to_string(),
    }
}

/// Launcher file name: without `/`, `:`, `\` or leading dot.
pub fn file_name(name: &str) -> String {
    let n: String = name.chars().map(|c| if matches!(c, '/' | ':' | '\\') { '-' } else { c }).collect();
    let n = n.trim_start_matches('.').trim().to_string();
    if n.is_empty() { "App".into() } else { n }
}

/// Useful side of a declared icon: `sizes` (the largest), `any` or SVG = vector.
pub fn declared_size(sizes: &str, url: &str, ty: &str) -> u32 {
    let any = Regex::new(r"(?i)\bany\b").expect("static regex");
    let svg_url = Regex::new(r"(?i)\.svg([?#]|$)").expect("static regex");
    if any.is_match(sizes) || ty.to_lowercase().contains("svg") || svg_url.is_match(url) {
        return 1024;
    }
    let dims = Regex::new(r"(?i)(\d+)\s*x\s*(\d+)").expect("static regex");
    dims.captures_iter(sizes).map(|m| m[1].parse::<u32>().unwrap_or(0).min(m[2].parse::<u32>().unwrap_or(0))).max().unwrap_or(0)
}

/// Size rank: above 512, the smallest (sharp enough, lighter); below, the largest.
pub fn rank(size: u32) -> i64 {
    if size >= 512 { 100_000 - size as i64 } else { size as i64 }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub url: String,
    pub size: u32,
    pub maskable: bool,
}

pub use signet_core::types::{Rect as Bounds, WebApp};

/// App read from disk, validated; `None` if unusable.
pub fn clean_app(raw: &Value) -> Option<WebApp> {
    let o = raw.as_object()?;
    let id = o.get("id")?.as_str().filter(|i| Regex::new(r"(?i)^[0-9a-f-]{36}$").map(|r| r.is_match(i)).unwrap_or(false))?;
    let start_url = s(o.get("startUrl"), 2048);
    let scope = s(o.get("scope"), 2048);
    let name = clean_app_name(o.get("name").unwrap_or(&Value::Null));
    if !is_http(&start_url) || !is_http(&scope) || name.is_empty() {
        return None;
    }
    let bounds = o.get("bounds").and_then(|b| serde_json::from_value::<Bounds>(b.clone()).ok()).filter(|b| [b.x, b.y, b.width, b.height].iter().all(|n| n.is_finite()) && b.width >= 200.0 && b.height >= 150.0);
    Some(WebApp {
        id: id.to_lowercase(),
        bundle: o.get("bundle").and_then(|b| b.as_str()).filter(|b| !b.is_empty()).map(file_name).unwrap_or_else(|| file_name(&name)),
        name,
        start_url,
        scope,
        space_id: o.get("spaceId").and_then(|x| x.as_str()).map(String::from),
        theme_color: clean_color(o.get("themeColor")),
        bounds,
        installed_at: o.get("installedAt").and_then(|n| n.as_f64()).filter(|n| n.is_finite()).map(|n| n as i64).unwrap_or_else(crate::util::now_ms),
    })
}

pub fn in_scope(a: &WebApp, url: &str) -> bool {
    normalize(url).starts_with(&a.scope)
}

/// Launch address (`signet://webapp/<id>`) or `--signet-webapp=<id>` argument to the app id.
pub fn parse_launch(arg: &str) -> Option<String> {
    let re = Regex::new(r"(?i)^signet://webapp/([0-9a-f-]{36})/?$").ok()?;
    let id = re.captures(arg).map(|c| c[1].to_string()).or_else(|| arg.strip_prefix(LAUNCH_ARG).map(String::from))?;
    Some(id.to_lowercase())
}

/// The registry (`webapps.json`, `{ version: 1, apps: [...] }`).
pub struct WebApps {
    file: PathBuf,
    pub apps: Vec<WebApp>,
}

impl WebApps {
    pub fn load(data_dir: &Path) -> Self {
        let file = data_dir.join("webapps.json");
        let apps = read_json::<Value>(&file).and_then(|r| r.get("apps").and_then(|a| a.as_array().map(|a| a.iter().filter_map(clean_app).collect()))).unwrap_or_default();
        WebApps { file, apps }
    }

    pub fn save(&self) {
        let _ = write_json(&self.file, &serde_json::json!({ "version": 1, "apps": self.apps }));
    }

    pub fn find(&self, id: &str) -> Option<&WebApp> {
        self.apps.iter().find(|a| a.id == id)
    }

    /// Installed apps, for the command bar.
    pub fn names(&self) -> Vec<(String, String)> {
        self.apps.iter().map(|a| (a.id.clone(), a.name.clone())).collect()
    }

    /// Installed app whose site contains this address (most specific scope), for "Open in app".
    pub fn app_for(&self, url: &str) -> Option<&WebApp> {
        if !is_http(url) {
            return None;
        }
        self.apps.iter().filter(|a| in_scope(a, url)).max_by_key(|a| a.scope.len())
    }

    /// Free launcher name: the app's, or followed by "2", "3"... if another app (or file) already has it.
    /// `exists` tells whether a launcher path exists and `own` whether it belongs to this app.
    pub fn free_bundle(&self, name: &str, id: &str, exists: &dyn Fn(&str) -> bool, own: &dyn Fn(&str) -> bool) -> String {
        let base = file_name(name);
        for n in 1.. {
            let candidate = if n == 1 { base.clone() } else { format!("{base} {n}") };
            let taken = self.apps.iter().any(|a| a.id != id && a.bundle == candidate);
            if !taken && (!exists(&candidate) || own(&candidate)) {
                return candidate;
            }
        }
        unreachable!()
    }

    /// Renames an app; returns its new launcher name through `free_bundle`. `None` if nothing changes.
    pub fn rename(&mut self, id: &str, name: &str, exists: &dyn Fn(&str) -> bool, own: &dyn Fn(&str) -> bool) -> Option<(String, String)> {
        let clean = clean_name_str(name);
        let (old_bundle, current) = { let a = self.find(id)?; (a.bundle.clone(), a.name.clone()) };
        if clean.is_empty() || clean == current {
            return None;
        }
        let new_bundle = self.free_bundle(&clean, id, exists, own);
        let a = self.apps.iter_mut().find(|a| a.id == id)?;
        a.name = clean;
        a.bundle = new_bundle.clone();
        self.save();
        Some((old_bundle, new_bundle))
    }

    pub fn remove(&mut self, id: &str) -> Option<WebApp> {
        let ix = self.apps.iter().position(|a| a.id == id)?;
        let a = self.apps.remove(ix);
        self.save();
        Some(a)
    }

    /// Adds an app from an accepted proposal.
    pub fn install(&mut self, p: &Proposal, name: &str, exists: &dyn Fn(&str) -> bool, own: &dyn Fn(&str) -> bool) -> WebApp {
        let id = uuid::Uuid::new_v4().to_string();
        let name = { let n = clean_name_str(name); if n.is_empty() { p.name.clone() } else { n } };
        let bundle = self.free_bundle(&name, &id, exists, own);
        let a = WebApp { id, name, start_url: p.start_url.clone(), scope: p.scope.clone(), space_id: p.space_id.clone(), theme_color: p.theme_color.clone(), bounds: None, bundle, installed_at: crate::util::now_ms() };
        self.apps.push(a.clone());
        self.save();
        a
    }

    pub fn remember_bounds(&mut self, id: &str, b: Bounds) {
        if let Some(a) = self.apps.iter_mut().find(|a| a.id == id) {
            a.bounds = Some(b);
            self.save();
        }
    }
}

// ---------------------------------------------------------------- inspecting a page

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PageIcon {
    pub href: String,
    pub sizes: String,
    pub rel: String,
    pub ty: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PageInfo {
    pub manifest: Option<String>,
    pub icons: Vec<PageIcon>,
    pub app_name: String,
    pub theme_color: String,
    pub title: String,
}

/// Result of the page script, bounded.
pub fn clean_page(raw: &Value) -> Option<PageInfo> {
    let o = raw.as_object()?;
    let icons = o.get("icons").and_then(|i| i.as_array()).map(|a| a.iter().take(40).map(|i| PageIcon { href: s(i.get("href"), 2048), sizes: s(i.get("sizes"), 200), rel: s(i.get("rel"), 100), ty: s(i.get("type"), 100) }).collect()).unwrap_or_default();
    Some(PageInfo { manifest: Some(s(o.get("manifest"), 2048)).filter(|m| !m.is_empty()), icons, app_name: s(o.get("appName"), 200), theme_color: s(o.get("themeColor"), 64), title: s(o.get("title"), 400) })
}

/// What the page inspection found, while waiting for the user's answer.
#[derive(Debug, Clone, PartialEq)]
pub struct Proposal {
    pub name: String,
    pub start_url: String,
    pub scope: String,
    pub space_id: Option<String>,
    pub theme_color: Option<String>,
    /// Icon as a data URL.
    pub icon: Option<String>,
    pub maskable: bool,
}

pub struct TabFacts<'a> {
    pub page_url: &'a str,
    pub title: &'a str,
    pub favicon: Option<&'a str>,
    /// Isolated space id when the tab lives in one.
    pub space_id: Option<&'a str>,
}

/// Reads at most `max` bytes of an address, in 5 s at most.
pub fn fetch_limited(http: &dyn Http, url: &str, max: usize) -> Option<(Vec<u8>, String)> {
    if !is_http(url) {
        return None;
    }
    let r = http.get(url, &[], Duration::from_secs(5), max + 1).ok()?;
    if !r.ok() || r.body.is_empty() || r.body.len() > max {
        return None;
    }
    Some((r.body, String::new()))
}

fn ext_type(ext: &str) -> Option<&'static str> {
    Some(match ext { "svg" => "image/svg+xml", "png" => "image/png", "ico" => "image/x-icon", "jpg" | "jpeg" => "image/jpeg", "webp" => "image/webp", "gif" => "image/gif", _ => return None })
}

/// Icon as a data URL (the overlay canvas must not be "tainted" by another origin). `type_of` gives the response
/// content type when the HTTP layer knows it; the extension is the fallback.
pub fn icon_data(http: &dyn Http, url: &str) -> Option<String> {
    let data_re = Regex::new(r"(?i)^data:image/[a-z0-9.+-]+[;,]").ok()?;
    if data_re.is_match(url) {
        return ((url.len() as f64) < MAX_ICON_BYTES as f64 * 1.4).then(|| url.to_string());
    }
    let (body, _) = fetch_limited(http, url, MAX_ICON_BYTES)?;
    let path = Url::parse(url).ok()?.path().to_lowercase();
    let ext = path.rsplit_once('.').map(|(_, e)| e.to_string()).unwrap_or_default();
    let ty = ext_type(&ext).or_else(|| sniff(&body))?;
    Some(format!("data:{ty};base64,{}", base64::engine::general_purpose::STANDARD.encode(body)))
}

fn sniff(b: &[u8]) -> Option<&'static str> {
    if b.starts_with(&[0, 0, 1, 0]) { Some("image/x-icon") }
    else if b.starts_with(&[0x89, 0x50]) { Some("image/png") }
    else if b.starts_with(&[0xff, 0xd8]) { Some("image/jpeg") }
    else if b.starts_with(b"GI") { Some("image/gif") }
    else if b.starts_with(b"RI") { Some("image/webp") }
    else if String::from_utf8_lossy(&b[..b.len().min(64)]).trim_start().starts_with("<svg") || String::from_utf8_lossy(&b[..b.len().min(64)]).trim_start().starts_with("<?xml") { Some("image/svg+xml") }
    else { None }
}

/// Builds the install proposal from what the page and its manifest declare.
pub fn inspect(http: &dyn Http, tab: &TabFacts, page: Option<&PageInfo>) -> Proposal {
    let origin = origin_of(tab.page_url);
    let manifest_url = page.and_then(|p| p.manifest.as_ref()).filter(|m| is_http(m)).cloned();
    let manifest: Option<Value> = manifest_url.as_deref().and_then(|u| fetch_limited(http, u, MAX_MANIFEST_BYTES)).and_then(|(b, _)| serde_json::from_slice::<Value>(&b).ok()).filter(|v| v.is_object());
    let mval = |k: &str| manifest.as_ref().and_then(|m| m.get(k));

    let declared_start = match (&manifest, &manifest_url) { (Some(_), Some(mu)) => resolve(mval("start_url"), mu), _ => None };
    let start_url = match declared_start { Some(d) if origin_of(&d) == origin => normalize(&d), _ => normalize(tab.page_url) };
    let declared_scope = match (&manifest, &manifest_url) { (Some(_), Some(mu)) if mval("scope").is_some_and(|v| !v.is_null() && v.as_str() != Some("")) => resolve(mval("scope"), mu), _ => None };
    let scope = match declared_scope { Some(d) if origin_of(&d) == origin && start_url.starts_with(&normalize(&d)) => normalize(&d), _ => format!("{origin}/") };

    let host = Url::parse(tab.page_url).ok().and_then(|u| u.host_str().map(|h| h.trim_start_matches("www.").to_string())).unwrap_or_default();
    let name = [clean_app_name(mval("name").unwrap_or(&Value::Null)), clean_app_name(mval("short_name").unwrap_or(&Value::Null)), clean_name_str(page.map(|p| p.app_name.as_str()).unwrap_or("")), clean_name_str(page.map(|p| p.title.as_str()).unwrap_or("")), clean_name_str(tab.title), host]
        .into_iter().find(|n| !n.is_empty()).unwrap_or_else(|| "App".into());

    // Candidate icons: maskable (they fill the square), then the others, from sharpest to least sharp.
    let mut masks: Vec<Candidate> = Vec::new();
    let mut others: Vec<Candidate> = Vec::new();
    if let (Some(m), Some(mu)) = (&manifest, &manifest_url) {
        for raw in m.get("icons").and_then(|i| i.as_array()).into_iter().flatten().take(40) {
            let Some(url) = resolve(raw.get("src"), mu) else { continue };
            let size = declared_size(&s(raw.get("sizes"), 200), &url, &s(raw.get("type"), 100));
            let purposes: Vec<String> = s(raw.get("purpose"), 100).to_lowercase().split_whitespace().map(String::from).collect();
            if purposes.iter().any(|p| p == "maskable") { masks.push(Candidate { url: url.clone(), size, maskable: true }); }
            if purposes.is_empty() || purposes.iter().any(|p| p == "any") { others.push(Candidate { url, size, maskable: false }); }
        }
    }
    for l in page.map(|p| p.icons.as_slice()).unwrap_or(&[]) {
        if l.href.is_empty() { continue; }
        let apple = l.rel.contains("apple-touch-icon");
        let size = Some(declared_size(&l.sizes, &l.href, &l.ty)).filter(|s| *s > 0).unwrap_or(if apple { 180 } else { 32 });
        others.push(Candidate { url: l.href.clone(), size, maskable: apple });
    }
    if let Some(f) = tab.favicon.filter(|f| !f.is_empty()) { others.push(Candidate { url: f.into(), size: 16, maskable: false }); }
    if !origin.is_empty() && others.is_empty() { others.push(Candidate { url: format!("{origin}/favicon.ico"), size: 16, maskable: false }); }
    let order = candidate_order(masks, others);
    let (mut icon, mut maskable) = (None, false);
    let mut tried: HashSet<String> = HashSet::new();
    for c in order {
        if tried.contains(&c.url) || tried.len() >= 4 { continue; }
        tried.insert(c.url.clone());
        if let Some(i) = icon_data(http, &c.url) { icon = Some(i); maskable = c.maskable; break; }
    }
    Proposal {
        name, start_url, scope, space_id: tab.space_id.map(String::from),
        theme_color: clean_color(mval("theme_color")).or_else(|| clean_color(Some(&Value::String(page.map(|p| p.theme_color.clone()).unwrap_or_default())))),
        icon, maskable,
    }
}

/// Order in which icon candidates are tried.
pub fn candidate_order(masks: Vec<Candidate>, others: Vec<Candidate>) -> Vec<Candidate> {
    let mut big: Vec<Candidate> = masks.iter().filter(|c| c.size >= 192).cloned().collect();
    big.sort_by_key(|c| std::cmp::Reverse(rank(c.size)));
    let mut rest: Vec<Candidate> = others.into_iter().chain(masks.into_iter().filter(|c| c.size < 192)).collect();
    rest.sort_by(|x, y| rank(y.size).cmp(&rank(x.size)).then((y.maskable as u8).cmp(&(x.maskable as u8))));
    big.into_iter().chain(rest).collect()
}

// ---------------------------------------------------------------- launchers

fn xml(t: &str) -> String {
    t.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

/// Shell-quoted string.
pub fn shq(t: &str) -> String {
    format!("'{}'", t.replace('\'', "'\\''"))
}

/// macOS `Info.plist` of a launcher.
pub fn plist(a: &WebApp) -> String {
    let name = xml(&a.name);
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n  <key>CFBundleDevelopmentRegion</key><string>en</string>\n  <key>CFBundleDisplayName</key><string>{name}</string>\n  <key>CFBundleName</key><string>{name}</string>\n  <key>CFBundleExecutable</key><string>launcher</string>\n  <key>CFBundleIconFile</key><string>app</string>\n  <key>CFBundleIdentifier</key><string>{BUNDLE_PREFIX}{}</string>\n  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>\n  <key>CFBundlePackageType</key><string>APPL</string>\n  <key>CFBundleShortVersionString</key><string>1.0</string>\n  <key>CFBundleVersion</key><string>1</string>\n  <key>LSMinimumSystemVersion</key><string>11.0</string>\n  <key>LSUIElement</key><true/>\n</dict>\n</plist>\n",
        a.id
    )
}

/// macOS launcher script: hands the app's address back to Signet and exits immediately.
/// `dev_exe`: `Some((executable, app path))` in development.
pub fn mac_script(a: &WebApp, dev_exe: Option<(&str, &str)>) -> String {
    let head = format!("#!/bin/sh\n# Signet app launcher ({}).\n", a.name.replace(['\r', '\n'], " "));
    match dev_exe {
        None => format!("{head}exec /usr/bin/open -b {MAIN_BUNDLE_ID} {}\n", shq(&format!("signet://webapp/{}", a.id))),
        Some((exe, path)) => {
            let args = [exe.to_string(), path.to_string(), format!("{LAUNCH_ARG}{}", a.id)];
            format!("{head}{} >/dev/null 2>&1 &\n", args.iter().map(|x| shq(x)).collect::<Vec<_>>().join(" "))
        }
    }
}

/// True if a launcher's `Info.plist` content is the one of app `id` (an app that is not ours is never deleted).
pub fn plist_belongs_to(plist_text: &str, id: &str) -> bool {
    plist_text.contains(&format!("<string>{BUNDLE_PREFIX}{id}</string>"))
}

/// Linux launcher (`~/.local/share/applications/<bundle>.desktop`).
pub fn desktop_entry(a: &WebApp, exe: &str, icon_path: Option<&str>) -> String {
    let esc = |t: &str| t.replace('\\', "\\\\").replace('\n', " ");
    let exec = format!("{} {}", shq(exe), shq(&format!("signet://webapp/{}", a.id)));
    let mut out = format!("[Desktop Entry]\nType=Application\nVersion=1.0\nName={}\nExec={}\nTerminal=false\nCategories=Network;WebBrowser;\nStartupWMClass={BUNDLE_PREFIX}{}\nX-Signet-WebApp={}\n", esc(&a.name), exec.replace('%', "%%"), a.id, a.id);
    if let Some(i) = icon_path { out.push_str(&format!("Icon={i}\n")); }
    out
}

/// Windows launcher: a `.cmd`-less approach, an Internet shortcut that hands the address back to the app.
pub fn windows_url_shortcut(a: &WebApp, icon_path: Option<&str>) -> String {
    let mut out = format!("[InternetShortcut]\r\nURL=signet://webapp/{}\r\n", a.id);
    if let Some(i) = icon_path { out.push_str(&format!("IconFile={i}\r\nIconIndex=0\r\n")); }
    out
}

/// Where the launchers go (`SIGNET_WEBAPPS_DIR` overrides it).
pub fn launchers_folder(home: &Path, env_override: Option<&str>) -> PathBuf {
    match env_override.filter(|e| !e.is_empty()) {
        Some(p) => PathBuf::from(p),
        None => home.join("Applications").join("Signet Apps"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{resp, FakeHttp};
    use serde_json::json;

    const ID: &str = "123e4567-e89b-12d3-a456-426614174000";

    fn app(scope: &str, id: &str) -> WebApp {
        WebApp { id: id.into(), name: "App".into(), start_url: format!("{scope}start"), scope: scope.into(), space_id: None, theme_color: None, bounds: None, bundle: "App".into(), installed_at: 1 }
    }

    #[test]
    fn clean_name() {
        assert_eq!(clean_app_name(&json!("  Mon\u{0}  App  Test ")), "Mon App Test");
        assert_eq!(clean_app_name(&json!("App\nTest\tX")), "App Test X");
        assert_eq!(clean_app_name(&json!("x".repeat(100))).len(), 60);
        assert_eq!(clean_app_name(&Value::Null), "");
        assert_eq!(clean_app_name(&json!(42)), "");
    }

    #[test]
    fn colors_and_urls() {
        assert_eq!(clean_color(Some(&json!("#fff"))).as_deref(), Some("#fff"));
        assert_eq!(clean_color(Some(&json!("rgb(1, 2, 3)"))).as_deref(), Some("rgb(1, 2, 3)"));
        assert_eq!(clean_color(Some(&json!("url(x)"))), None);
        assert_eq!(normalize("https://a.com/p#frag"), "https://a.com/p");
        assert_eq!(origin_of("https://a.com:8080/x"), "https://a.com:8080");
        assert_eq!(file_name("a/b:c"), "a-b-c");
        assert_eq!(file_name("..."), "App");
        assert_eq!(resolve(Some(&json!("/m.json")), "https://a.com/x/y").as_deref(), Some("https://a.com/m.json"));
        assert_eq!(parse_launch(&format!("signet://webapp/{}", ID.to_uppercase())).as_deref(), Some(ID));
        assert_eq!(parse_launch(&format!("--signet-webapp={ID}")).as_deref(), Some(ID));
        assert_eq!(parse_launch("signet://settings"), None);
    }

    #[test]
    fn icon_sizes() {
        assert_eq!(declared_size("48x48 192x192", "x.png", "image/png"), 192);
        assert_eq!(declared_size("any", "x.png", ""), 1024);
        assert_eq!(declared_size("", "x.svg?v=1", ""), 1024);
        assert_eq!(declared_size("512x256", "x", ""), 256);
        assert!(rank(1024) < rank(512) && rank(192) > rank(64));
    }

    #[test]
    fn clean_app_rules() {
        assert!(clean_app(&json!({"id": ID, "name": "A", "startUrl": "https://a.com/", "scope": "https://a.com/"})).is_some());
        assert!(clean_app(&json!({"id": "bad", "name": "A", "startUrl": "https://a.com/", "scope": "https://a.com/"})).is_none());
        assert!(clean_app(&json!({"id": ID, "name": "A", "startUrl": "ftp://a.com/", "scope": "https://a.com/"})).is_none());
        assert!(clean_app(&json!({"id": ID, "name": " ", "startUrl": "https://a.com/", "scope": "https://a.com/"})).is_none());
        let a = clean_app(&json!({"id": ID.to_uppercase(), "name": "A", "startUrl": "https://a.com/", "scope": "https://a.com/", "bounds": {"x":1,"y":1,"width":100,"height":100}, "bundle": "", "themeColor": "nope"})).unwrap();
        assert_eq!((a.id.as_str(), a.bundle.as_str(), a.bounds, a.theme_color), (ID, "A", None, None));
    }

    #[test]
    fn registry_scope_and_persistence() {
        let d = tempfile::tempdir().unwrap();
        let mut r = WebApps::load(d.path());
        r.apps.push(app("https://a.com/", "1"));
        r.apps.push(app("https://a.com/docs/", "2"));
        assert_eq!(r.app_for("https://a.com/docs/x#h").map(|a| a.id.as_str()), Some("2"));
        assert_eq!(r.app_for("https://a.com/other").map(|a| a.id.as_str()), Some("1"));
        assert!(r.app_for("https://b.com/").is_none() && r.app_for("javascript:1").is_none());
        let none = |_: &str| false;
        let p = Proposal { name: "Docs".into(), start_url: "https://d.com/".into(), scope: "https://d.com/".into(), space_id: Some("sp".into()), theme_color: None, icon: None, maskable: false };
        let a = r.install(&p, "  ", &none, &none);
        assert_eq!((a.name.as_str(), a.bundle.as_str()), ("Docs", "Docs"));
        let b = r.install(&p, "Docs", &none, &none);
        assert_eq!(b.bundle, "Docs 2");
        assert_eq!(r.rename(&b.id, "Docs", &none, &none), None);
        let (old, new) = r.rename(&b.id, "Other/Name", &none, &none).unwrap();
        assert_eq!((old.as_str(), new.as_str()), ("Docs 2", "Other-Name"));
        let again = WebApps::load(d.path());
        assert_eq!(again.apps.len(), 2); // the two hand-pushed test apps have invalid ids and are dropped
        assert!(again.find(&b.id).is_some());
        let exists = |c: &str| c == "Taken";
        assert_eq!(r.free_bundle("Taken", "x", &exists, &|_| false), "Taken 2");
        assert_eq!(r.free_bundle("Taken", "x", &exists, &|_| true), "Taken");
        assert!(r.remove(&a.id).is_some() && r.remove(&a.id).is_none());
    }

    #[test]
    fn inspect_uses_manifest_and_orders_icons() {
        let http = FakeHttp(Box::new(|u| match u {
            "https://a.com/manifest.json" => resp(200, r##"{"name":"  Big  App ","short_name":"B","start_url":"/app?x=1","scope":"/","theme_color":"#123456","icons":[
                {"src":"/small.png","sizes":"48x48"},{"src":"/mask.png","sizes":"512x512","purpose":"maskable"},{"src":"/big.png","sizes":"1024x1024","purpose":"any"}]}"##),
            "https://a.com/mask.png" => resp(200, "\u{89}PNG fake"),
            _ => resp(404, ""),
        }));
        let page = PageInfo { manifest: Some("https://a.com/manifest.json".into()), title: "Title".into(), ..Default::default() };
        let tab = TabFacts { page_url: "https://a.com/some/page", title: "Tab", favicon: None, space_id: Some("iso") };
        let p = inspect(&http, &tab, Some(&page));
        assert_eq!(p.name, "Big App");
        assert_eq!((p.start_url.as_str(), p.scope.as_str()), ("https://a.com/app?x=1", "https://a.com/"));
        assert_eq!(p.theme_color.as_deref(), Some("#123456"));
        assert_eq!(p.space_id.as_deref(), Some("iso"));
        // The maskable 512 icon comes first, and it loads: used as is.
        assert!(p.maskable);
        assert!(p.icon.as_deref().unwrap().starts_with("data:image/png;base64,"));
    }

    #[test]
    fn inspect_falls_back_without_manifest() {
        let http = FakeHttp(Box::new(|u| if u == "https://www.shop.com/favicon.ico" { resp(200, "\0\0\u{1}\0data") } else { resp(404, "") }));
        let tab = TabFacts { page_url: "https://www.shop.com/p#frag", title: "", favicon: None, space_id: None };
        let p = inspect(&http, &tab, None);
        assert_eq!((p.name.as_str(), p.start_url.as_str(), p.scope.as_str()), ("shop.com", "https://www.shop.com/p", "https://www.shop.com/"));
        assert!(p.icon.as_deref().unwrap().starts_with("data:image/x-icon"));
        // A manifest start_url on another origin is ignored.
        let http2 = FakeHttp(Box::new(|u| if u.ends_with("m.json") { resp(200, r#"{"start_url":"https://evil.com/","scope":"https://evil.com/"}"#) } else { resp(404, "") }));
        let page = PageInfo { manifest: Some("https://shop.com/m.json".into()), ..Default::default() };
        let tab2 = TabFacts { page_url: "https://shop.com/", title: "S", favicon: None, space_id: None };
        let q = inspect(&http2, &tab2, Some(&page));
        assert_eq!((q.start_url.as_str(), q.scope.as_str()), ("https://shop.com/", "https://shop.com/"));
        assert!(q.icon.is_none());
    }

    #[test]
    fn icon_data_rules() {
        let http = FakeHttp(Box::new(|u| if u.contains("big") { Ok(crate::http::Response { status: 200, body: vec![1; MAX_ICON_BYTES + 1], url: u.into() }) } else { resp(200, "<svg xmlns='x'/>") }));
        assert_eq!(icon_data(&http, "data:image/png;base64,AAA").as_deref(), Some("data:image/png;base64,AAA"));
        assert!(icon_data(&http, "https://a.com/big.png").is_none());
        assert!(icon_data(&http, "https://a.com/x.svg").unwrap().starts_with("data:image/svg+xml"));
        assert!(icon_data(&http, "ftp://a.com/x.png").is_none());
        assert!(icon_data(&http, "https://a.com/noext").unwrap().starts_with("data:image/svg+xml"));
    }

    #[test]
    fn launcher_files() {
        let a = WebApp { name: "My <App> & \"co\"".into(), ..app("https://a.com/", ID) };
        let p = plist(&a);
        assert!(p.contains("My &lt;App&gt; &amp; &quot;co&quot;") && p.contains(&format!("{BUNDLE_PREFIX}{ID}")));
        assert!(plist_belongs_to(&p, ID) && !plist_belongs_to(&p, "other"));
        assert!(mac_script(&a, None).contains(&format!("/usr/bin/open -b {MAIN_BUNDLE_ID} 'signet://webapp/{ID}'")));
        let dev = mac_script(&a, Some(("/bin/app", "it's")));
        assert!(dev.contains("'/bin/app' 'it'\\''s' '--signet-webapp=") && dev.trim_end().ends_with("&"));
        let d = desktop_entry(&a, "/usr/bin/signet", Some("/i.png"));
        assert!(d.contains("Name=My <App> & \"co\"") && d.contains("Icon=/i.png") && d.contains(&format!("signet://webapp/{ID}")));
        assert!(windows_url_shortcut(&a, None).contains(&format!("URL=signet://webapp/{ID}")));
        assert_eq!(launchers_folder(Path::new("/h"), None), PathBuf::from("/h/Applications/Signet Apps"));
        assert_eq!(launchers_folder(Path::new("/h"), Some("/x")), PathBuf::from("/x"));
        assert!(PAGE_SCRIPT.contains("apple-touch-icon") && PAGE_SCRIPT.contains(r"(^|\s)(icon"));
    }
}

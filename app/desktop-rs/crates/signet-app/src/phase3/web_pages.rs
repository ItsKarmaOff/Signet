//! Pages the web layer shows itself: favicons (`signet_features::favicons`, port of `web/favicons.ts`), error pages
//! (`signet_features::errorpage`, port of `web/errorpage.ts`), the JSON viewer trigger (`signet_pages::json_viewer`) and
//! the settings the `signet://` handler (`crate::scheme`) needs.
//!
//! - Favicons: the tab keeps the announced icon URL (the interface loads it itself, like Electron). The first announced
//!   URL is also downloaded in the background into a bounded data URL cache, served by `Core::fallback_favicon` for the
//!   places that cannot load a URL. The fetch is a plain HTTP request without the profile's cookies.
//! - Error pages: a main-frame `LoadError` is remembered; when the engine's error page has loaded (`LoadEnd`) the clean
//!   page is injected over it (the script is idempotent, the tick repeats it a few times in case the engine's page was
//!   not ready yet). The memory is dropped when another page starts loading in the view, not on any stray `LoadEnd`.
//!   The HTTPS-only warning (module `net`) takes precedence.
//! - JSON viewer: a main-frame response with a JSON content type marks the view; the viewer script runs at `LoadEnd`.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use signet_core::urls::{http_fallback, is_https_fallback_error};
use signet_features::errorpage;
use signet_features::favicons::{self, FaviconCache};
use signet_features::http::UreqHttp;
use signet_pages::json_viewer;
use signet_web::{ViewId, WebEvent};

use crate::browser::Core;

/// Translation function in the shape `signet_features` expects.
pub(super) fn translate(key: &str, args: &[(&str, String)]) -> String {
    let a: Vec<(&str, &str)> = args.iter().map(|(k, v)| (*k, v.as_str())).collect();
    signet_core::i18n::t_args(key, &a)
}

/// Favicon cache shared with the download threads.
#[derive(Default)]
struct Icons {
    cache: Mutex<FaviconCache>,
    fetching: Mutex<HashSet<String>>,
}

/// State owned by this feature (a field of `Phase3`).
#[derive(Default)]
pub struct State {
    icons: Arc<Icons>,
    /// Last main-frame load failure per view: url and engine error code.
    failed: HashMap<ViewId, (String, i32)>,
    /// Views whose error page was injected: when it was last tried and how many times (the tick checks it stuck).
    injected: HashMap<ViewId, (std::time::Instant, u8)>,
    /// Views whose main-frame response is JSON.
    json: HashSet<ViewId>,
    last_languages: Vec<String>,
    /// Page colors sampled from the top of the displayed tabs (port of `main/browser/sitecolor.ts`).
    site: SiteColor,
}

/// Samples taken per navigation before the tab is only probed slowly, and the slow period.
const SITE_MAX_SAMPLES: u8 = 12;
const SITE_STABLE_AFTER: u8 = 4;
const SITE_SLOW_MS: u128 = 5000;

#[derive(Default)]
struct SiteSample {
    color: Option<String>,
    same: u8,
    n: u8,
    at: Option<std::time::Instant>,
}

#[derive(Default)]
struct SiteColor {
    tabs: HashMap<String, SiteSample>,
    /// Tabs with a sample in flight and since when (a page that never answers must not block the next sample).
    inflight: HashMap<String, std::time::Instant>,
    done: Arc<Mutex<Vec<(String, Option<String>)>>>,
}

/// Page script of the sampler: background colors found under 7 points of a row near the top, most frequent first.
/// (A DevTools screenshot disturbs the view, so the pixels are inferred from the styles.)
const SITE_CHANNEL: &str = "site:color";

const SITE_JS: &str = r#"(()=>{try{const w=innerWidth,y=Math.min(6,innerHeight-1),out={};const op=b=>{const m=/^rgba?\(([^)]+)\)/.exec(b);if(!m)return false;const p=m[1].split(/[ ,\/]+/);return (p.length>3?parseFloat(p[3]):1)>=0.95};const hb=getComputedStyle(document.documentElement).backgroundColor,bb=document.body?getComputedStyle(document.body).backgroundColor:'';const canvas=op(hb)?hb:op(bb)?bb:'rgb(255, 255, 255)';
for(let i=0;i<7;i++){const x=Math.max(0,Math.min(w-1,Math.round((i+0.5)*w/7)));let c=canvas;
for(const e of document.elementsFromPoint(x,y)){const b=getComputedStyle(e).backgroundColor;const m=/^rgba?\(([^)]+)\)/.exec(b);
if(m){const p=m[1].split(/[ ,\/]+/);const a=p.length>3?parseFloat(p[3]):1;if(a>=0.95){c=b;break}}}
out[c]=(out[c]||0)+1}
return JSON.stringify(out)}catch(e){return ''}})()"#;

fn parse_css_rgb(c: &str) -> Option<String> {
    let inner = c.split('(').nth(1)?.split(')').next()?;
    let v: Vec<u8> = inner.split([' ', ',', '/']).filter(|p| !p.is_empty()).take(3).filter_map(|p| p.parse::<f32>().ok()).map(|f| f.round().clamp(0.0, 255.0) as u8).collect();
    (v.len() == 3).then(|| format!("#{:02x}{:02x}{:02x}", v[0], v[1], v[2]))
}

/// Samples through the page's own `__signetPost` bridge, not DevTools: an attached DevTools agent makes Chromium reset
/// the page view to its default size about 250 ms after each navigation, which blanked the page for a frame.
fn sample_site(core: &mut Core, tab: &str) {
    let Some(view) = core.views.view_of(tab) else { return };
    core.phase3.web_pages.site.inflight.insert(tab.to_string(), std::time::Instant::now());
    view.execute_js(&format!("try{{__signetPost('{SITE_CHANNEL}',{SITE_JS})}}catch(e){{}}"));
}

/// Most frequent opaque color of the sampler's answer, as `#rrggbb`.
fn site_color(payload: &str) -> Option<String> {
    let counts: HashMap<String, u32> = serde_json::from_str(payload).ok()?;
    let best = counts.into_iter().max_by_key(|(_, n)| *n)?.0;
    parse_css_rgb(&best)
}

/// Applies finished samples and starts the next ones for the displayed tabs.
fn site_tick(core: &mut Core) {
    core.phase3.web_pages.site.inflight.retain(|_, at| at.elapsed().as_millis() < 2000);
    let finished: Vec<_> = core.phase3.web_pages.site.done.lock().map(|mut d| std::mem::take(&mut *d)).unwrap_or_default();
    for (tab, color) in finished {
        let site = &mut core.phase3.web_pages.site;
        site.inflight.remove(&tab);
        let e = site.tabs.entry(tab.clone()).or_default();
        e.same = if e.color == color { e.same.saturating_add(1) } else { 0 };
        e.n = e.n.saturating_add(1);
        e.at = Some(std::time::Instant::now());
        e.color = color.clone();
        if let Some(c) = color {
            let mut f = serde_json::Map::new();
            f.insert("themeColor".into(), serde_json::Value::String(c));
            core.patch_tab(&tab, f, false, false);
        }
    }
    if !core.win.as_ref().is_some_and(|w| w.active) {
        return;
    }
    let shown = core.shown_tabs.clone();
    let alive: HashSet<String> = core.state().tabs.keys().cloned().collect();
    core.phase3.web_pages.site.tabs.retain(|id, _| alive.contains(id));
    for id in shown {
        let site = &core.phase3.web_pages.site;
        if site.inflight.contains_key(&id) {
            continue;
        }
        let due = match site.tabs.get(&id) {
            None => true,
            Some(e) => (e.same < SITE_STABLE_AFTER && e.n < SITE_MAX_SAMPLES) || e.at.is_none_or(|t| t.elapsed().as_millis() >= SITE_SLOW_MS),
        };
        if due {
            sample_site(core, &id);
        }
    }
}

/// Downloads an icon in the background into the cache (once at a time per URL).
fn prefetch(icons: &Arc<Icons>, url: &str) {
    if !favicons::is_fetchable(url) {
        return;
    }
    let now = signet_core::ids::now_ms();
    if icons.cache.lock().is_ok_and(|mut c| c.get(url, now).is_some()) {
        return;
    }
    if !icons.fetching.lock().is_ok_and(|mut f| f.insert(url.to_string())) {
        return;
    }
    let icons = icons.clone();
    let url = url.to_string();
    let spawned = std::thread::Builder::new().name("favicon".into()).spawn({
        let (icons, url) = (icons.clone(), url.clone());
        move || {
            let data = favicons::fetch(&UreqHttp, &url);
            if let Ok(mut c) = icons.cache.lock() {
                c.put(&url, data, signet_core::ids::now_ms());
            }
            if let Ok(mut f) = icons.fetching.lock() {
                f.remove(&url);
            }
        }
    });
    if spawned.is_err()
        && let Ok(mut f) = icons.fetching.lock()
    {
        f.remove(&url);
    }
}

/// Cached icon as a data URL; a miss starts the download and answers `None` (ask again later).
fn lookup(icons: &Arc<Icons>, url: &str) -> Option<String> {
    if !favicons::is_fetchable(url) {
        return None;
    }
    let hit = icons.cache.lock().ok().and_then(|mut c| c.get(url, signet_core::ids::now_ms()));
    match hit {
        Some(data) => data,
        None => {
            prefetch(icons, url);
            None
        }
    }
}

/// True if the main-frame response is a JSON document the viewer should take over.
fn json_response(url: &str, status: i32, mime: &str) -> bool {
    (200..300).contains(&status) && json_viewer::applies_to(url) && json_viewer::is_json_content_type(mime)
}

/// True if the core will retry this failed load over http by itself (typed input promoted to https://): no error page.
fn core_retries(pending: Option<&String>, url: &str, code: i32) -> bool {
    pending.is_some_and(|p| is_https_fallback_error(code) && http_fallback(p, url).is_some())
}

/// True if a `LoadEnd` is the one of the engine's error page for this failure.
fn is_error_page_load(load_url: &str, failed_url: &str) -> bool {
    load_url.starts_with("chrome-error:") || load_url == failed_url
}

impl Core {
    /// Favicon as a data URL for places that cannot load a URL (`SignetApi::fallback_favicon`). Never blocks: a miss
    /// starts the download and returns `None`.
    pub fn fallback_favicon(&mut self, url: &str) -> Option<String> {
        lookup(&self.phase3.web_pages.icons, url)
    }
}

/// Called once after the browser core exists and the web layer is initialized.
pub(super) fn startup(core: &mut Core) {
    crate::scheme::set_site_languages(core.state().settings.languages.clone());
    core.phase3.web_pages.last_languages = core.state().settings.languages.clone();
}

/// Called for every CEF event, before the core handles it.
pub(super) fn web_event(core: &mut Core, ev: &WebEvent) {
    let view = ev.view();
    match ev {
        WebEvent::PageMessage { channel, payload, is_main_frame: true, .. } if channel == SITE_CHANNEL => {
            let Some(tab) = core.views.tab_of(view).map(str::to_string) else { return };
            if core.phase3.web_pages.site.inflight.contains_key(&tab)
                && let Ok(mut d) = core.phase3.web_pages.site.done.lock()
            {
                d.push((tab, site_color(payload)));
            }
        }
        WebEvent::FaviconUrls { urls, .. } => {
            if let Some(first) = urls.first() {
                prefetch(&core.phase3.web_pages.icons, first);
            }
        }
        WebEvent::MainFrameResponse { url, status, mime_type, .. } => {
            if json_response(url, *status, mime_type) {
                core.phase3.web_pages.json.insert(view);
            } else {
                core.phase3.web_pages.json.remove(&view);
            }
        }
        WebEvent::LoadError { is_main_frame: true, code, url, .. } if *code != -3 => {
            let tab = core.views.tab_of(view).map(str::to_string);
            let retry = core_retries(tab.as_ref().and_then(|t| core.upgraded.get(t)), url, *code);
            if !retry && !core.phase3.net.has_warning(view) {
                core.phase3.web_pages.failed.insert(view, (url.clone(), *code));
                core.phase3.web_pages.injected.remove(&view);
            }
        }
        WebEvent::LoadStart { is_main_frame: true, url, .. } => {
            // A different page starts loading in the view: the failure no longer applies (a reload of the failed URL keeps it).
            if core.phase3.web_pages.failed.get(&view).is_some_and(|(failed_url, _)| !is_error_page_load(url, failed_url)) {
                core.phase3.web_pages.failed.remove(&view);
                core.phase3.web_pages.injected.remove(&view);
            }
        }
        WebEvent::LoadEnd { is_main_frame: true, url, .. } => {
            if let Some(t) = core.views.tab_of(view).map(str::to_string) {
                core.phase3.web_pages.site.tabs.remove(&t);
            }
            let Some(v) = core.views.tab_of(view).and_then(|t| core.views.view_of(t)) else { return };
            if let Some((failed_url, code)) = core.phase3.web_pages.failed.get(&view).cloned() {
                if is_error_page_load(url, &failed_url) {
                    let text = errorpage::load_error_text(&translate, &failed_url, code);
                    v.execute_js(&errorpage::injection_script(&text, errorpage::ERROR_CSS));
                    core.phase3.web_pages.injected.insert(view, (std::time::Instant::now(), 1));
                    return;
                }
                // Any other page that ends loading is not the failure (`LoadStart` clears the memory for new pages).
            }
            if core.phase3.web_pages.json.remove(&view) && core.state().settings.json_viewer != Some(false) && json_viewer::applies_to(url) {
                // The script answers `ok`, `already`, `not-json`...: kept in the self-test trace.
                let trace = core.trace.clone();
                let params = serde_json::json!({ "expression": json_viewer::script(signet_core::i18n::current_locale().id(), false), "returnByValue": true }).to_string();
                v.execute_devtools_method("Runtime.evaluate", &params, move |r| {
                    if let Some(t) = trace.borrow_mut().as_mut() {
                        t.push(format!("json-viewer {}", r.unwrap_or_else(|e| e)));
                    }
                });
            }
        }
        WebEvent::Closed { .. } => {
            core.phase3.web_pages.failed.remove(&view);
            core.phase3.web_pages.injected.remove(&view);
            core.phase3.web_pages.json.remove(&view);
        }
        _ => {}
    }
}

/// Injection attempts per failed load (the first one is made at `LoadEnd`), and the delay between two.
const INJECT_TRIES: u8 = 3;
const INJECT_EVERY_MS: u128 = 900;

/// Repeats the (idempotent) error page script a few times: the engine's page may not have been ready at `LoadEnd`.
fn reinject_error_pages(core: &mut Core) {
    let due: Vec<ViewId> = core.phase3.web_pages.injected.iter().filter(|(_, (at, n))| *n < INJECT_TRIES && at.elapsed().as_millis() >= INJECT_EVERY_MS).map(|(v, _)| *v).collect();
    for view in due {
        let Some((url, code)) = core.phase3.web_pages.failed.get(&view).cloned() else {
            core.phase3.web_pages.injected.remove(&view);
            continue;
        };
        if core.phase3.net.has_warning(view) {
            continue;
        }
        if let Some(v) = core.views.tab_of(view).and_then(|t| core.views.view_of(t)) {
            let text = errorpage::load_error_text(&translate, &url, code);
            v.execute_js(&errorpage::injection_script(&text, errorpage::ERROR_CSS));
        }
        if let Some(e) = core.phase3.web_pages.injected.get_mut(&view) {
            *e = (std::time::Instant::now(), e.1 + 1);
        }
    }
}

/// Called on every core tick (about once per second or at the next store deadline).
pub(super) fn tick(core: &mut Core) {
    site_tick(core);
    reinject_error_pages(core);
    // The `signet://version` page lists the site languages.
    if core.phase3.web_pages.last_languages != core.state().settings.languages {
        let langs = core.state().settings.languages.clone();
        crate::scheme::set_site_languages(langs.clone());
        core.phase3.web_pages.last_languages = langs;
    }
}

/// Called before quitting (flush files, stop threads).
pub(super) fn quit(core: &mut Core) {
    core.phase3.web_pages.failed.clear();
    core.phase3.web_pages.injected.clear();
    core.phase3.web_pages.json.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_trigger() {
        assert!(json_response("https://api.example.com/x", 200, "application/json; charset=utf-8"));
        assert!(json_response("https://api.example.com/x", 200, "application/vnd.api+json"));
        assert!(!json_response("https://api.example.com/x", 404, "application/json"));
        assert!(!json_response("https://example.com/", 200, "text/html"));
        assert!(!json_response("signet://note/1", 200, "application/json"));
    }

    #[test]
    fn error_page_matching() {
        assert!(is_error_page_load("chrome-error://chromewebdata/", "https://a.com/"));
        assert!(is_error_page_load("https://a.com/", "https://a.com/"));
        assert!(!is_error_page_load("https://b.com/", "https://a.com/"));
    }

    #[test]
    fn core_http_fallback_skips_the_error_page() {
        let pending = "https://example.com/".to_string();
        assert!(core_retries(Some(&pending), "https://example.com/", -102));
        assert!(!core_retries(Some(&pending), "https://example.com/", -105), "DNS failure is not retried");
        assert!(!core_retries(None, "https://example.com/", -102));
    }

    #[test]
    fn icon_cache_serves_data_urls_only_for_fetchable_addresses() {
        let icons = Arc::new(Icons::default());
        assert_eq!(lookup(&icons, "data:image/png;base64,AAAA"), None);
        assert_eq!(lookup(&icons, "file:///icon.png"), None);
        icons.cache.lock().unwrap().put("https://a.com/f.ico", Some("data:image/png;base64,AA".into()), signet_core::ids::now_ms());
        assert_eq!(lookup(&icons, "https://a.com/f.ico").as_deref(), Some("data:image/png;base64,AA"));
    }

    #[test]
    fn translate_falls_back_to_the_key() {
        assert!(!translate("pageError.title", &[]).is_empty());
    }
}

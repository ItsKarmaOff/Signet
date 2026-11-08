//! Developer tools, Develop menu, developer mode and responsive design mode.
//! Port of `main/devtools/devtools.ts`, `develop.ts` and `devmode.ts`.
//!
//! - The tools are an ordinary view loading the bundled DevTools frontend from the loopback remote debugging port
//!   (`CefBrowserHost::ShowDevTools` aborts the process with the Alloy style). The page's own target id comes from
//!   CDP `Target.getTargetInfo`; only that address may load in the tools view (see `web_event`).
//! - Docked tools: the view is created and moved by [`Core::pane_extras`], which the layout calls for every pane
//!   (the tools take their share of the card, the page keeps the rest). The detached mode puts the view in the
//!   `#devtools` window (`Win::DevTools`) under its 36 px header.
//! - Develop settings (cache, JavaScript, theme, motion, connection, user agent) and responsive mode are applied
//!   through the DevTools protocol (`execute_devtools_method`) and last as long as the tab's view.
//! - Developer mode counts console errors and warnings of local pages (`consoleErrors`, `consoleWarnings`).

use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use signet_core::core::store::SAVE_DELAY;
use signet_core::devhosts::is_local_dev_url;
use signet_core::split::GAP;
use signet_core::types::{
    ColorScheme, DevDevice, DevNetwork, DevOverrides, DevResponsive, DevToolsDock, DevToolsPrefs, DevToolsWindowState, DevUserAgent, PaneDevtools, PaneDock,
    PaneResponsive, Rect,
};
use signet_pages::devices::{DEFAULT_DEVICE, DEVICES, UserAgentKind, clamp_size, device_spec};
use raw_window_handle::RawWindowHandle;
use signet_web::{Bounds, ConsoleLevel, ProfileKind, ViewId, ViewOptions, WebEvent, WebView};

use crate::browser::Core;
use crate::windows::Entry;

/// Height of the tools card header (`DEVTOOLS_BAR` of the interface).
pub const DEVTOOLS_BAR: f64 = 36.0;
const MIN_TOOLS_W: f64 = 320.0;
const MIN_TOOLS_H: f64 = 160.0;
const MIN_PAGE_W: f64 = 320.0;
const MIN_PAGE_H: f64 = 200.0;
const DEFAULT_WIDTH: f64 = 480.0;
const DEFAULT_HEIGHT: f64 = 320.0;
/// Responsive mode: device bar above the page, margin around it (resize handles).
pub const RESPONSIVE_BAR: f64 = 40.0;
const RESPONSIVE_PAD: f64 = 16.0;
/// The frontend needs this long after its load before its models answer (inspect).
const FRONTEND_READY: Duration = Duration::from_millis(1500);
/// A target lookup without answer after this delay is asked again.
const RESOLVE_RETRY: Duration = Duration::from_secs(3);
/// Chromium major version for plausible user agents (CEF 156).
const CHROME: u32 = 156;

// ------------------------------------------------------------------------------------------------ pure logic

/// Tools size bounded by the available room: each side keeps its minimum, otherwise half and half.
pub fn clamp_tools(size: f64, total: f64, min_tools: f64, min_page: f64) -> f64 {
    let room = total - GAP;
    if room < min_tools + min_page {
        return (room / 2.0).round();
    }
    size.max(min_tools).min(room - min_page).round()
}

/// A card split between the page and the tools.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Carved {
    pub page: Rect,
    pub tools: Rect,
    /// Area of the tools view (the card minus its header).
    pub rect: Rect,
    pub divider: Rect,
}

/// Splits a pane card: the page, and the tools on the right or at the bottom, separated by the split view gap.
pub fn carve_card(dock: PaneDock, size: f64, card: Rect) -> Carved {
    let (page, tools, divider) = match dock {
        PaneDock::Right => {
            let w = clamp_tools(size, card.width, MIN_TOOLS_W, MIN_PAGE_W);
            let page = Rect::new(card.x, card.y, card.width - w - GAP, card.height);
            (page, Rect::new(page.x + page.width + GAP, card.y, w, card.height), Rect::new(page.x + page.width, card.y, GAP, card.height))
        }
        PaneDock::Bottom => {
            let h = clamp_tools(size, card.height, MIN_TOOLS_H, MIN_PAGE_H);
            let page = Rect::new(card.x, card.y, card.width, card.height - h - GAP);
            (page, Rect::new(card.x, page.y + page.height + GAP, card.width, h), Rect::new(card.x, page.y + page.height, card.width, GAP))
        }
    };
    let rect = Rect::new(tools.x, tools.y + DEVTOOLS_BAR, tools.width, (tools.height - DEVTOOLS_BAR).max(0.0));
    Carved { page, tools, rect, divider }
}

/// Emulated profile of a responsive device: density, mobile, agent.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Profile {
    pub dpr: f64,
    pub mobile: bool,
    pub user_agent: Option<DevUserAgent>,
}

/// Id of a device in the table and the wire format.
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

fn device_of(id: &str) -> DevDevice {
    DEVICES.iter().find(|d| d.id == id).map(|d| match d.id {
        "iphone-se" => DevDevice::IphoneSe,
        "iphone-16" => DevDevice::Iphone16,
        "iphone-16-pro-max" => DevDevice::Iphone16ProMax,
        "pixel-9" => DevDevice::Pixel9,
        "galaxy-s24" => DevDevice::GalaxyS24,
        "ipad-mini" => DevDevice::IpadMini,
        "ipad-pro-11" => DevDevice::IpadPro11,
        "laptop" => DevDevice::Laptop,
        "desktop" => DevDevice::Desktop,
        _ => DevDevice::Custom,
    })
    .unwrap_or(DevDevice::Iphone16)
}

fn agent_of(k: UserAgentKind) -> DevUserAgent {
    match k {
        UserAgentKind::SafariIphone => DevUserAgent::SafariIphone,
        UserAgentKind::SafariIpad => DevUserAgent::SafariIpad,
        UserAgentKind::ChromeAndroid => DevUserAgent::ChromeAndroid,
    }
}

/// Profile of a responsive mode: the device's, or inherited from `base` for a free size (`responsiveProfile`).
pub fn responsive_profile(r: &DevResponsive) -> Profile {
    let spec = device_spec(Some(device_id(r.device)));
    if r.device != DevDevice::Custom {
        return Profile { dpr: spec.dpr, mobile: spec.mobile, user_agent: spec.user_agent.map(agent_of) };
    }
    let base = match r.base {
        Some(b) if b != DevDevice::Custom => device_spec(Some(device_id(b))),
        _ => spec,
    };
    let mobile = r.mobile.unwrap_or(base.mobile);
    if !mobile {
        return Profile { dpr: base.dpr, mobile: false, user_agent: None };
    }
    let m = if base.mobile { base } else { device_spec(Some(DEFAULT_DEVICE)) };
    Profile { dpr: m.dpr, mobile: true, user_agent: m.user_agent.map(agent_of) }
}

/// Text sent to sites and `navigator.platform` of an offered user agent.
pub fn user_agent_string(id: DevUserAgent) -> (String, &'static str) {
    match id {
        DevUserAgent::SafariMac => ("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Safari/605.1.15".into(), "MacIntel"),
        DevUserAgent::SafariIphone => {
            ("Mozilla/5.0 (iPhone; CPU iPhone OS 18_6 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Mobile/15E148 Safari/604.1".into(), "iPhone")
        }
        DevUserAgent::SafariIpad => ("Mozilla/5.0 (iPad; CPU OS 18_6 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Mobile/15E148 Safari/604.1".into(), "iPad"),
        DevUserAgent::ChromeAndroid => (format!("Mozilla/5.0 (Linux; Android 10; K) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{CHROME}.0.0.0 Mobile Safari/537.36"), "Linux armv81"),
        DevUserAgent::FirefoxMac => (format!("Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:{0}.0) Gecko/20100101 Firefox/{0}.0", CHROME + 3), "MacIntel"),
    }
}

const UA_IDS: [DevUserAgent; 5] = [DevUserAgent::SafariMac, DevUserAgent::SafariIphone, DevUserAgent::SafariIpad, DevUserAgent::ChromeAndroid, DevUserAgent::FirefoxMac];
const NETWORK_IDS: [DevNetwork; 4] = [DevNetwork::Fast4g, DevNetwork::Slow4g, DevNetwork::ThreeG, DevNetwork::Offline];

pub fn ua_id(u: DevUserAgent) -> &'static str {
    match u {
        DevUserAgent::SafariMac => "safari-mac",
        DevUserAgent::SafariIphone => "safari-iphone",
        DevUserAgent::SafariIpad => "safari-ipad",
        DevUserAgent::ChromeAndroid => "chrome-android",
        DevUserAgent::FirefoxMac => "firefox-mac",
    }
}

pub fn network_id(n: DevNetwork) -> &'static str {
    match n {
        DevNetwork::Fast4g => "fast4g",
        DevNetwork::Slow4g => "slow4g",
        DevNetwork::ThreeG => "3g",
        DevNetwork::Offline => "offline",
    }
}

/// Simulated connection with Chromium's preset values: (offline, latency ms, download B/s, upload B/s).
pub fn network_conditions(n: DevNetwork) -> (bool, f64, f64, f64) {
    match n {
        DevNetwork::Fast4g => (false, 165.0, 9_000_000.0 / 8.0 * 0.9, 1_500_000.0 / 8.0 * 0.9),
        DevNetwork::Slow4g => (false, 562.5, 1_600_000.0 / 8.0 * 0.9, 750_000.0 / 8.0 * 0.9),
        DevNetwork::ThreeG => (false, 2000.0, 500_000.0 / 8.0 * 0.8, 500_000.0 / 8.0 * 0.8),
        DevNetwork::Offline => (true, 0.0, 0.0, 0.0),
    }
}

/// Active development settings without default values (empty = none).
pub fn clean_overrides(o: &DevOverrides) -> DevOverrides {
    let mut out = DevOverrides::default();
    if o.no_cache == Some(true) {
        out.no_cache = Some(true);
    }
    if o.no_js == Some(true) {
        out.no_js = Some(true);
    }
    out.scheme = o.scheme;
    if o.reduced_motion == Some(true) {
        out.reduced_motion = Some(true);
    }
    out.network = o.network;
    out.user_agent = o.user_agent;
    if let Some(r) = &o.responsive {
        let mut c = DevResponsive { device: r.device, width: clamp_size(f64::from(r.width)), height: clamp_size(f64::from(r.height)), base: None, mobile: None };
        if r.device == DevDevice::Custom {
            c.base = r.base.filter(|b| *b != DevDevice::Custom);
            c.mobile = r.mobile;
        }
        out.responsive = Some(c);
    }
    out
}

fn is_empty(o: &DevOverrides) -> bool {
    *o == DevOverrides::default()
}

fn is_mobile(o: &DevOverrides) -> bool {
    o.responsive.as_ref().is_some_and(|r| responsive_profile(r).mobile)
}

/// User agent in effect: the chosen one, otherwise the responsive device's.
pub fn effective_user_agent(o: &DevOverrides) -> Option<DevUserAgent> {
    o.user_agent.or_else(|| o.responsive.as_ref().and_then(|r| responsive_profile(r).user_agent))
}

/// Emulation sent for a device, a size and a scale (to resend it only on change).
pub fn metrics_key(r: &DevResponsive, scale: f64) -> String {
    let p = responsive_profile(r);
    format!("{}:{}x{}:{}{}@{:.4}", device_id(r.device), r.width, r.height, p.dpr, if p.mobile { "m" } else { "" }, scale)
}

/// Placement of a responsive page in the pane area: (frame of the page view, device bar, display scale).
pub fn responsive_frame(width: u32, height: u32, rect: Rect, frozen: Option<f64>) -> (Rect, Rect, f64) {
    let bar = Rect::new(rect.x, rect.y, rect.width, RESPONSIVE_BAR);
    let room_w = (rect.width - RESPONSIVE_PAD * 2.0).max(1.0);
    let room_h = (rect.height - RESPONSIVE_BAR - RESPONSIVE_PAD).max(1.0);
    let (w, h) = (f64::from(width), f64::from(height));
    let scale = frozen.unwrap_or_else(|| 1f64.min(room_w / w).min(room_h / h));
    // Frozen scale: the page grows on screen up to the area edges (beyond that, it is cropped).
    let fw = (w * scale).round().min(if frozen.is_some() { room_w } else { f64::INFINITY }).max(1.0);
    let fh = (h * scale).round().min(if frozen.is_some() { room_h } else { f64::INFINITY }).max(1.0);
    let frame = Rect::new(rect.x + ((rect.width - fw) / 2.0).round(), rect.y + RESPONSIVE_BAR, fw, fh);
    (frame, bar, scale)
}

/// One DevTools protocol command; `fallback` is tried if the first is refused.
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub method: &'static str,
    pub params: Value,
    pub fallback: Option<(&'static str, Value)>,
}

fn step(method: &'static str, params: Value) -> Step {
    Step { method, params, fallback: None }
}

/// Commands that apply a tab's settings. `network_on`: the protocol Network domain is already enabled; `leave_responsive`:
/// responsive mode was on and is off now. Returns the commands and whether Network stays enabled.
pub fn override_steps(o: &DevOverrides, network_on: bool, languages: &str, leave_responsive: bool) -> (Vec<Step>, bool) {
    let mut steps = Vec::new();
    let needs_net = o.no_cache == Some(true) || o.network.is_some();
    let mut net = network_on;
    if needs_net && !net {
        steps.push(step("Network.enable", json!({"maxTotalBufferSize": 1_000_000, "maxResourceBufferSize": 500_000})));
        net = true;
    }
    if needs_net || net {
        let n = o.network.map(network_conditions);
        steps.push(step("Network.setCacheDisabled", json!({"cacheDisabled": o.no_cache == Some(true)})));
        let (offline, latency, down, up) = n.unwrap_or((false, 0.0, -1.0, -1.0));
        let cond = json!({"latency": latency, "downloadThroughput": down, "uploadThroughput": up});
        let by_rule = if n.is_some() {
            let mut c = cond.clone();
            c["urlPattern"] = json!("");
            json!({"offline": offline, "matchedNetworkConditions": [c]})
        } else {
            json!({"offline": false, "matchedNetworkConditions": []})
        };
        let mut old = cond;
        old["offline"] = json!(offline);
        steps.push(Step { method: "Network.emulateNetworkConditionsByRule", params: by_rule, fallback: Some(("Network.emulateNetworkConditions", old)) });
    }
    if !needs_net && net {
        steps.push(step("Network.disable", json!({})));
        net = false;
    }
    steps.push(step("Emulation.setScriptExecutionDisabled", json!({"value": o.no_js == Some(true)})));
    let scheme = match o.scheme {
        Some(ColorScheme::Light) => "light",
        Some(ColorScheme::Dark) => "dark",
        None => "",
    };
    steps.push(step(
        "Emulation.setEmulatedMedia",
        json!({"media": "", "features": [
            {"name": "prefers-color-scheme", "value": scheme},
            {"name": "prefers-reduced-motion", "value": if o.reduced_motion == Some(true) { "reduce" } else { "" }},
        ]}),
    ));
    // The chosen agent as is; otherwise the default one (an empty string resets it), always with the languages.
    match effective_user_agent(o) {
        Some(id) => {
            let (ua, platform) = user_agent_string(id);
            steps.push(step("Emulation.setUserAgentOverride", json!({"userAgent": ua, "acceptLanguage": languages, "platform": platform})));
        }
        None => steps.push(step("Emulation.setUserAgentOverride", json!({"userAgent": "", "acceptLanguage": languages}))),
    }
    if leave_responsive {
        steps.push(step("Emulation.clearDeviceMetricsOverride", json!({})));
        steps.push(step("Emulation.setTouchEmulationEnabled", json!({"enabled": false})));
        steps.push(step("Emulation.setEmitTouchEventsForMouse", json!({"enabled": false})));
    }
    (steps, net)
}

/// Commands that emulate a device at a display scale (size, density, orientation, touch).
pub fn emulate_steps(r: &DevResponsive, scale: f64) -> Vec<Step> {
    let p = responsive_profile(r);
    let mut m = json!({
        "width": r.width, "height": r.height, "deviceScaleFactor": p.dpr, "mobile": p.mobile, "scale": scale,
        "screenWidth": r.width, "screenHeight": r.height,
    });
    if p.mobile {
        m["screenOrientation"] = if r.width > r.height { json!({"type": "landscapePrimary", "angle": 90}) } else { json!({"type": "portraitPrimary", "angle": 0}) };
    }
    vec![
        step("Emulation.setDeviceMetricsOverride", m),
        step("Emulation.setTouchEmulationEnabled", if p.mobile { json!({"enabled": true, "maxTouchPoints": 5}) } else { json!({"enabled": false}) }),
        step("Emulation.setEmitTouchEventsForMouse", json!({"enabled": p.mobile, "configuration": "mobile"})),
    ]
}

/// Applies a Develop menu or palette item (`dev:<what>[:<value>]`) to a tab's settings; false for an unknown id.
pub fn apply_item(id: &str, o: &mut DevOverrides) -> bool {
    let Some(rest) = id.strip_prefix("dev:") else { return false };
    let (what, value) = rest.split_once(':').unwrap_or((rest, ""));
    match what {
        "noCache" => o.no_cache = Some(o.no_cache != Some(true)),
        "noJs" => o.no_js = Some(o.no_js != Some(true)),
        "motion" => o.reduced_motion = Some(o.reduced_motion != Some(true)),
        "scheme" => {
            o.scheme = match value {
                "light" => Some(ColorScheme::Light),
                "dark" => Some(ColorScheme::Dark),
                _ => None,
            }
        }
        "net" => o.network = NETWORK_IDS.iter().copied().find(|n| network_id(*n) == value),
        "ua" => o.user_agent = UA_IDS.iter().copied().find(|u| ua_id(*u) == value),
        "reset" => *o = DevOverrides::default(),
        _ => return false,
    }
    true
}

/// A reload is needed for JavaScript, the user agent and the simulated touch device to take effect.
pub fn needs_reload(before: &DevOverrides, next: &DevOverrides) -> bool {
    before.no_js != next.no_js || effective_user_agent(before) != effective_user_agent(next) || is_mobile(before) != is_mobile(next)
}

/// Only the size of responsive mode changed (handle drag): the rest is already applied.
pub fn size_only(before: &DevOverrides, next: &DevOverrides) -> bool {
    match (&before.responsive, &next.responsive) {
        (Some(a), Some(b)) => {
            let rest = |o: &DevOverrides| DevOverrides { responsive: None, ..o.clone() };
            a.device == b.device && is_mobile(before) == is_mobile(next) && rest(before) == rest(next)
        }
        _ => false,
    }
}

fn dock_to_pane(d: DevToolsDock) -> Option<PaneDock> {
    match d {
        DevToolsDock::Right => Some(PaneDock::Right),
        DevToolsDock::Bottom => Some(PaneDock::Bottom),
        DevToolsDock::Window => None,
    }
}

fn web(r: Rect) -> Bounds {
    Bounds::new(r.x as f32, r.y as f32, r.width.max(1.0) as f32, r.height.max(1.0) as f32)
}

/// Sends the commands in order; `done` runs after the last one answered.
fn send_steps(v: WebView, steps: Vec<Step>, done: Option<Box<dyn FnOnce()>>) {
    let n = steps.len();
    let mut done = done;
    for (i, s) in steps.into_iter().enumerate() {
        let last = i + 1 == n;
        let cb = if last { done.take() } else { None };
        let fallback = s.fallback.clone();
        v.execute_devtools_method(s.method, &s.params.to_string(), move |r| {
            if r.is_err()
                && let Some((m, p)) = fallback
            {
                v.execute_devtools_method(m, &p.to_string(), move |_| {
                    if let Some(f) = cb {
                        f()
                    }
                });
                return;
            }
            if let Some(f) = cb {
                f()
            }
        });
    }
    if n == 0
        && let Some(f) = done
    {
        f()
    }
}

// ------------------------------------------------------------------------------------------------ state

/// Developer tools of one tab.
struct Tools {
    dock: DevToolsDock,
    width: f64,
    height: f64,
    /// Docked tools view.
    view: Option<ViewId>,
    /// Tools window (detached mode) or docked view created for the current dock.
    opened: bool,
    /// Card at the last layout (to bound the size during a drag).
    card: Option<Rect>,
    focus: bool,
    visible: Option<bool>,
    loaded: bool,
    /// Panel the frontend opens on (`panel=` of its address), such as `console`.
    panel: Option<String>,
    /// Element to inspect once the frontend is ready.
    inspect: Option<(i32, i32)>,
    since: Instant,
    /// Frontend address (known once the page's target id was read).
    url: Option<String>,
    /// Target lookup in flight, since.
    resolving: Option<Instant>,
    /// When the frontend finished loading.
    loaded_at: Option<Instant>,
}

/// The `#devtools` window: native handle and content size, once created.
#[derive(Clone, Copy)]
struct ToolsWindow {
    raw: RawWindowHandle,
    size: (f64, f64),
}

/// State owned by this feature (a field of `Phase3`).
#[derive(Default)]
pub struct State {
    tools: HashMap<String, Tools>,
    /// Develop settings per tab, with the view they were applied to (a new view starts without settings).
    overrides: HashMap<String, (ViewId, DevOverrides)>,
    network_on: HashMap<String, bool>,
    metrics: HashMap<String, String>,
    scales: HashMap<String, f64>,
    frozen: HashMap<String, f64>,
    counts: HashMap<String, (u32, u32)>,
    window: Option<ToolsWindow>,
}

impl State {
    /// Developer tools exist for this tab.
    pub fn is_open(&self, tab: &str) -> bool {
        self.tools.contains_key(tab)
    }

    /// Frontend view of a tab's tools (self-test).
    pub fn view_of(&self, tab: &str) -> Option<ViewId> {
        self.tools.get(tab).and_then(|t| t.view)
    }

    /// Frontend address of a tab's tools (self-test).
    pub fn url_of(&self, tab: &str) -> Option<String> {
        self.tools.get(tab).and_then(|t| t.url.clone())
    }
}

/// What the layout needs for a pane (see [`Core::pane_extras`]).
pub struct PaneExtras {
    /// Card of the page (the pane card minus the docked tools).
    pub card: Rect,
    /// Final rectangle of the page view.
    pub rect: Rect,
    pub devtools: Option<PaneDevtools>,
    pub responsive: Option<PaneResponsive>,
}

impl Core {
    // -------------------------------------------------------------------- queries

    pub fn dev_overrides(&self, tab: &str) -> DevOverrides {
        self.phase3.devtools.overrides.get(tab).map(|(_, o)| o.clone()).unwrap_or_default()
    }

    /// Developer tools open on this tab (keeps the tab awake).
    pub fn devtools_open(&self, tab: &str) -> bool {
        self.phase3.devtools.is_open(tab)
    }

    fn dev_mode(&self) -> bool {
        self.state().settings.dev_mode == Some(true)
    }

    /// Whether the address is a local page that developer mode must keep out of the history (option of the mode).
    pub fn keep_out_of_history(&self, url: &str) -> bool {
        let s = &self.state().settings;
        s.dev_mode == Some(true) && s.dev_keep_out_of_history == Some(true) && is_local_dev_url(url)
    }

    /// Built-in tools and responsive mode only exist in the main window (not Little Signet, private, app, Peek).
    fn can_dock(&self, tab: &str) -> bool {
        !self.store.data.detached.contains(tab) && self.ov.peek_tab.as_deref() != Some(tab)
    }

    fn prefs(&self) -> DevToolsPrefs {
        let mut p = self.state().settings.devtools.clone().unwrap_or_default();
        if p.width <= 0.0 {
            p.width = DEFAULT_WIDTH;
        }
        if p.height <= 0.0 {
            p.height = DEFAULT_HEIGHT;
        }
        p
    }

    fn save_prefs(&mut self, f: impl FnOnce(&mut DevToolsPrefs)) {
        let mut p = self.prefs();
        f(&mut p);
        self.store.data.state.settings.devtools = Some(p);
        self.store.schedule(SAVE_DELAY);
    }

    fn host_of(&self, tab: &str) -> String {
        let url = self.state().tabs.get(tab).map(|t| t.url.clone()).unwrap_or_default();
        url::Url::parse(&url).ok().and_then(|u| u.host_str().map(str::to_string)).filter(|h| !h.is_empty()).unwrap_or_else(|| self.state().tabs.get(tab).map(|t| t.title.clone()).unwrap_or_default())
    }

    /// Window position (below the pane bar, towards the right) where the menus of a tab's buttons open.
    fn menu_point(&self, tab: &str) -> (f64, f64) {
        match self.panes.iter().find(|p| p.tab_id == tab) {
            Some(p) => ((p.rect.x + p.rect.width - 200.0).max(p.rect.x), p.rect.y),
            None => (200.0, 80.0),
        }
    }

    // -------------------------------------------------------------------- developer tools

    pub fn toggle_dev_tools(&mut self, tab: Option<&str>, console: bool) {
        let Some(tab) = tab else { return };
        if console {
            return self.show_console(tab);
        }
        let Some(dock) = self.phase3.devtools.tools.get(tab).map(|t| t.dock) else {
            self.open_dev_tools(tab, None);
            return;
        };
        // Built-in tools while the tab moved to another window: shown there instead of closing where nobody sees them.
        if dock != DevToolsDock::Window && !self.can_dock(tab) {
            return self.set_dock(tab, DevToolsDock::Window, true);
        }
        self.drop_tools(tab, true);
    }

    /// Creates the entry for the tab's tools; docked views are created by the next layout.
    fn open_dev_tools(&mut self, tab: &str, dock: Option<DevToolsDock>) -> bool {
        if self.phase3.devtools.tools.contains_key(tab) {
            return true;
        }
        if !WebView::devtools_available() {
            self.log("devtools: remote debugging is off (SIGNET_NO_DEVTOOLS is set), the tools cannot open");
            return false;
        }
        if self.views.view_of(tab).is_none() {
            return false;
        }
        let prefs = self.prefs();
        let want = dock.unwrap_or(prefs.dock);
        let dock = if want != DevToolsDock::Window && !self.can_dock(tab) { DevToolsDock::Window } else { want };
        self.phase3.devtools.tools.insert(
            tab.to_string(),
            Tools {
                dock,
                width: prefs.width,
                height: prefs.height,
                view: None,
                opened: false,
                card: None,
                focus: true,
                visible: None,
                loaded: false,
                panel: None,
                inspect: None,
                since: Instant::now(),
                url: None,
                resolving: None,
                loaded_at: None,
            },
        );
        self.patch_one(tab, "devtools", serde_json::to_value(dock).unwrap_or(Value::Null), false, false);
        self.pump_tools(tab);
        true
    }

    /// Asks for the page's target id (once at a time); the answer comes back through [`Core::tools_url_ready`].
    fn resolve_tools(&mut self, tab: &str) {
        let Some(page) = self.views.view_of(tab) else { return };
        let tx = self.cmds.clone();
        let Some(t) = self.phase3.devtools.tools.get_mut(tab) else { return };
        if t.url.is_some() || t.resolving.is_some_and(|since| since.elapsed() < RESOLVE_RETRY) {
            return;
        }
        t.resolving = Some(Instant::now());
        let name = tab.to_string();
        page.devtools_frontend_url(move |r| {
            let _ = tx.unbounded_send(crate::windows::Cmd::Run(Box::new(move |c| c.tools_url_ready(&name, r))));
        });
    }

    fn tools_url_ready(&mut self, tab: &str, r: Result<String, String>) {
        let Some(t) = self.phase3.devtools.tools.get_mut(tab) else { return };
        t.resolving = None;
        match r {
            Ok(url) => {
                t.url = Some(url.clone());
                self.note(format!("devtools-url {url}"));
                self.pump_tools(tab);
                self.changed_view();
            }
            Err(e) => {
                self.log(format!("devtools: cannot find the page target: {e}"));
                self.drop_tools(tab, true);
            }
        }
    }

    /// Creates what the tools need: the target lookup, then the window (detached mode) or a layout for the docked view.
    fn pump_tools(&mut self, tab: &str) {
        self.resolve_tools(tab);
        let Some(t) = self.phase3.devtools.tools.get_mut(tab) else { return };
        if t.dock == DevToolsDock::Window {
            if !t.opened {
                t.opened = true;
                t.since = Instant::now();
                self.send(crate::windows::Cmd::OpenWin(crate::windows::Win::Devtools, None));
            } else {
                self.create_window_view(tab);
            }
        } else if t.view.is_none() {
            self.changed_view();
        }
    }

    /// Frontend view in the `#devtools` window, once the address and the window are there.
    fn create_window_view(&mut self, tab: &str) {
        let Some(w) = self.phase3.devtools.window else { return };
        let Some(t) = self.phase3.devtools.tools.get_mut(tab) else { return };
        let (Some(url), None) = (t.url.clone(), t.view) else { return };
        if t.dock != DevToolsDock::Window {
            return;
        }
        if let Some(v) = new_frontend(w.raw, &t.panel.as_deref().map_or(url.clone(), |p| with_panel(&url, p)), window_bounds(w.size)) {
            t.view = Some(v.id());
            t.loaded = false;
            t.loaded_at = None;
            t.visible = Some(true);
            t.since = Instant::now();
            v.focus();
        }
    }

    /// The `#devtools` window exists (`size`: its content, in points).
    pub fn devtools_window_created(&mut self, raw: Option<RawWindowHandle>, size: (f64, f64)) {
        match raw {
            Some(raw) => self.phase3.devtools.window = Some(ToolsWindow { raw, size }),
            None => return,
        }
        let tabs: Vec<String> = self.phase3.devtools.tools.iter().filter(|(_, t)| t.dock == DevToolsDock::Window).map(|(k, _)| k.clone()).collect();
        for tab in tabs {
            self.create_window_view(&tab);
        }
    }

    pub fn devtools_window_resized(&mut self, size: (f64, f64)) {
        let Some(w) = self.phase3.devtools.window.as_mut() else { return };
        if w.size == size {
            return;
        }
        w.size = size;
        for t in self.phase3.devtools.tools.values().filter(|t| t.dock == DevToolsDock::Window) {
            if let Some(v) = t.view.and_then(WebView::from_id) {
                v.set_bounds(window_bounds(size));
            }
        }
    }

    /// The user closed the `#devtools` window.
    pub fn devtools_window_closed(&mut self) {
        self.phase3.devtools.window = None;
        let tabs: Vec<String> = self.phase3.devtools.tools.iter().filter(|(_, t)| t.dock == DevToolsDock::Window).map(|(k, _)| k.clone()).collect();
        for tab in tabs {
            self.drop_tools(&tab, false);
        }
    }

    /// Closes the tools of a tab and forgets them (`close`: also the `#devtools` window).
    fn drop_tools(&mut self, tab: &str, close: bool) {
        let Some(t) = self.phase3.devtools.tools.remove(tab) else { return };
        if let Some(v) = t.view.and_then(WebView::from_id) {
            v.close();
        }
        if close && t.dock == DevToolsDock::Window && self.phase3.devtools.window.take().is_some() {
            self.send(crate::windows::Cmd::CloseWin(crate::windows::Win::Devtools));
        }
        self.patch_one(tab, "devtools", Value::Null, false, false);
        if t.dock != DevToolsDock::Window {
            self.request_focus();
        }
        self.changed_view();
    }

    pub fn close_dev_tools(&mut self, tab_id: &str) {
        self.drop_tools(tab_id, true);
    }

    pub fn set_dev_tools_dock(&mut self, tab_id: &str, dock: DevToolsDock) {
        self.set_dock(tab_id, dock, false);
    }

    /// `forced`: window imposed (tab outside the main window), not remembered as a preference.
    fn set_dock(&mut self, tab: &str, dock: DevToolsDock, forced: bool) {
        let next = if dock != DevToolsDock::Window && !self.can_dock(tab) { DevToolsDock::Window } else { dock };
        if !forced {
            self.save_prefs(|p| p.dock = next);
        }
        let Some(cur) = self.phase3.devtools.tools.get(tab).map(|t| t.dock) else {
            self.open_dev_tools(tab, Some(next));
            return;
        };
        if cur == next {
            return;
        }
        // The view is recreated in the new place (same address: the target is the same).
        let leaving_window = cur == DevToolsDock::Window;
        if let Some(t) = self.phase3.devtools.tools.get_mut(tab) {
            if let Some(v) = t.view.take().and_then(WebView::from_id) {
                v.close();
            }
            t.dock = next;
            t.opened = false;
            t.loaded = false;
            t.loaded_at = None;
            t.visible = None;
            t.focus = true;
            t.since = Instant::now();
        }
        if leaving_window && self.phase3.devtools.window.take().is_some() {
            self.send(crate::windows::Cmd::CloseWin(crate::windows::Win::Devtools));
        }
        self.patch_one(tab, "devtools", serde_json::to_value(next).unwrap_or(Value::Null), false, false);
        self.pump_tools(tab);
        self.changed_view();
    }

    /// Size of the built-in tools (px, width on the right or height at the bottom); 0 = original size.
    pub fn set_dev_tools_size(&mut self, tab_id: &str, size: f64, done: bool) {
        let Some(t) = self.phase3.devtools.tools.get_mut(tab_id) else { return };
        if t.dock == DevToolsDock::Window || !size.is_finite() {
            return;
        }
        let right = t.dock == DevToolsDock::Right;
        let mut next = if size > 0.0 { size } else if right { DEFAULT_WIDTH } else { DEFAULT_HEIGHT };
        if let Some(card) = t.card {
            next = if right { clamp_tools(next, card.width, MIN_TOOLS_W, MIN_PAGE_W) } else { clamp_tools(next, card.height, MIN_TOOLS_H, MIN_PAGE_H) };
        }
        if right {
            t.width = next;
        } else {
            t.height = next;
        }
        if done || size <= 0.0 {
            self.save_prefs(|p| if right { p.width = next } else { p.height = next });
        }
        self.changed_view();
    }

    /// Inspects the element under the pointer: opens the tools if needed, then selects the element (the frontend's
    /// inspect mode, then a pointer click at the point on the page).
    pub fn inspect_at(&mut self, tab: &str, x: i32, y: i32) {
        if self.views.view_of(tab).is_none() {
            return;
        }
        if let Some(dock) = self.phase3.devtools.tools.get(tab).map(|t| t.dock)
            && dock != DevToolsDock::Window
            && !self.can_dock(tab)
        {
            self.set_dock(tab, DevToolsDock::Window, true);
        }
        if !self.phase3.devtools.tools.contains_key(tab) && !self.open_dev_tools(tab, None) {
            return;
        }
        if let Some(t) = self.phase3.devtools.tools.get_mut(tab) {
            t.inspect = Some((x, y));
            t.focus = true;
        }
        self.changed_view();
    }

    /// Runs a pending `inspect_at` once the frontend can answer.
    fn run_inspect(&mut self, tab: &str) {
        let Some(t) = self.phase3.devtools.tools.get_mut(tab) else { return };
        let (Some((x, y)), Some(at), Some(front)) = (t.inspect, t.loaded_at, t.view.and_then(WebView::from_id)) else { return };
        if at.elapsed() < FRONTEND_READY {
            return;
        }
        t.inspect = None;
        // The frontend looks the node up itself and selects it (see `inspect_js`).
        let expr = inspect_js(x, y);
        let params = json!({ "expression": expr, "awaitPromise": true, "returnByValue": true }).to_string();
        front.execute_devtools_method("Runtime.evaluate", &params, |r| {
            if let Err(e) = r {
                eprintln!("[devtools] inspect failed: {e}");
            }
        });
    }

    /// Opens the tools (or shows them if open) on the console.
    pub fn show_console(&mut self, tab: &str) {
        if !self.phase3.devtools.tools.contains_key(tab) {
            if !self.open_dev_tools(tab, None) {
                return;
            }
        } else if let Some(dock) = self.phase3.devtools.tools.get(tab).map(|t| t.dock)
            && dock != DevToolsDock::Window
            && !self.can_dock(tab)
        {
            self.set_dock(tab, DevToolsDock::Window, true);
        }
        let Some(t) = self.phase3.devtools.tools.get_mut(tab) else { return };
        t.focus = true;
        t.panel = Some("console".into());
        // The panel is a parameter of the frontend's address: a view already showing it loads it again.
        if let (Some(v), Some(url)) = (t.view.and_then(WebView::from_id), t.url.as_deref()) {
            v.load_url(&with_panel(url, "console"));
            t.loaded_at = None;
        }
        self.changed_view();
    }

    /// Menu of the tools button: location and closing.
    pub fn show_dev_tools_menu(&mut self, tab_id: &str) {
        let Some(dock) = self.phase3.devtools.tools.get(tab_id).map(|t| t.dock) else { return };
        let dockable = self.can_dock(tab_id);
        let t = signet_core::i18n::t;
        let item = |d: DevToolsDock, id: &str, label: String| Entry::Item { label, id: id.into(), enabled: d == DevToolsDock::Window || dockable, checked: dock == d };
        let entries = vec![
            item(DevToolsDock::Right, "right", t("devtools.dockRight")),
            item(DevToolsDock::Bottom, "bottom", t("devtools.dockBottom")),
            item(DevToolsDock::Window, "window", t("devtools.dockWindow")),
            Entry::Sep,
            Entry::Item { label: t("devtools.close"), id: "close".into(), enabled: true, checked: false },
        ];
        let tab = tab_id.to_string();
        let at = self.menu_point(tab_id);
        self.popup(entries, at, move |c, id| match id {
            "right" => c.set_dock(&tab, DevToolsDock::Right, false),
            "bottom" => c.set_dock(&tab, DevToolsDock::Bottom, false),
            "window" => c.set_dock(&tab, DevToolsDock::Window, false),
            "close" => c.drop_tools(&tab, true),
            _ => {}
        });
    }

    /// State of the detached tools window header (the CEF window has none yet: any tab with detached tools).
    pub fn dev_tools_window_ready(&mut self) -> Option<DevToolsWindowState> {
        let tab = self.phase3.devtools.tools.iter().find(|(_, t)| t.dock == DevToolsDock::Window).map(|(k, _)| k.clone())?;
        Some(DevToolsWindowState { site: self.host_of(&tab), dockable: self.can_dock(&tab), tab_id: tab })
    }

    // -------------------------------------------------------------------- layout

    /// Called by the layout for every pane (replaces its `devtools: None, responsive: None`).
    ///
    /// `card` is the pane card from the split geometry, `bar` the height of the pane bar (and bookmarks bar) above the page.
    /// Docked tools take their share of the card (the view is created and moved here); responsive mode centers the
    /// page at the device size under its bar. The returned rectangles go in `Pane` and the page view is placed at `rect`.
    pub fn pane_extras(&mut self, tab_id: &str, card: Rect, bar: f64) -> PaneExtras {
        let (page_card, devtools) = self.carve_devtools(tab_id, card);
        let area = Rect::new(page_card.x, page_card.y + bar, page_card.width, (page_card.height - bar).max(0.0));
        match self.place_responsive(tab_id, area) {
            Some((frame, pr)) => PaneExtras { card: page_card, rect: frame, devtools, responsive: Some(pr) },
            None => PaneExtras { card: page_card, rect: area, devtools, responsive: None },
        }
    }

    fn carve_devtools(&mut self, tab: &str, card: Rect) -> (Rect, Option<PaneDevtools>) {
        let page = self.views.view_of(tab);
        let docked = self.can_dock(tab);
        let parent = self.views.parent;
        if self.phase3.devtools.tools.get(tab).is_some_and(|t| t.view.is_none() && t.url.is_none()) {
            self.resolve_tools(tab);
        }
        let Some(t) = self.phase3.devtools.tools.get_mut(tab) else { return (card, None) };
        let (Some(pane_dock), Some(_page)) = (dock_to_pane(t.dock), page) else { return (card, None) };
        if !docked {
            return (card, None);
        }
        t.card = Some(card);
        let size = if pane_dock == PaneDock::Right { t.width } else { t.height };
        let c = carve_card(pane_dock, size, card);
        match t.view.map(WebView::from_id) {
            Some(Some(v)) => v.set_bounds(web(c.rect)),
            Some(None) => {} // closed: the Closed event or the tick removes the entry
            None => {
                // Waits for the page's target id (the answer relayouts).
                if let (Some(url), Some(parent)) = (t.url.clone(), parent)
                    && let Some(v) = new_frontend(parent, &t.panel.as_deref().map_or(url.clone(), |p| with_panel(&url, p)), web(c.rect))
                {
                    t.view = Some(v.id());
                    t.opened = true;
                    t.loaded = false;
                    t.loaded_at = None;
                    t.visible = None;
                    t.since = Instant::now();
                }
            }
        }
        (c.page, Some(PaneDevtools { dock: pane_dock, card: c.tools, rect: c.rect, divider: c.divider }))
    }

    fn place_responsive(&mut self, tab: &str, area: Rect) -> Option<(Rect, PaneResponsive)> {
        let r = self.dev_overrides(tab).responsive?;
        let v = self.views.view_of(tab)?;
        let st = &mut self.phase3.devtools;
        let (frame, bar, scale) = responsive_frame(r.width, r.height, area, st.frozen.get(tab).copied());
        st.scales.insert(tab.to_string(), scale);
        let key = metrics_key(&r, scale);
        if st.metrics.get(tab) != Some(&key) {
            st.metrics.insert(tab.to_string(), key);
            send_steps(v, emulate_steps(&r, scale), None);
        }
        Some((frame, PaneResponsive { responsive: r, bar, area, scale }))
    }

    /// Called by the layout once `panes` is set: shows or hides the docked tools with their page and gives them the focus.
    pub fn devtools_after_layout(&mut self, shown: &[String], fullscreen: bool) {
        for (tab, t) in self.phase3.devtools.tools.iter_mut() {
            let Some(v) = t.view.and_then(WebView::from_id) else { continue };
            let vis = !fullscreen && shown.contains(tab) && t.dock != DevToolsDock::Window;
            if t.visible != Some(vis) {
                v.set_visible(vis);
                t.visible = Some(vis);
            }
            if vis && t.focus {
                t.focus = false;
                v.focus();
            }
        }
    }

    // -------------------------------------------------------------------- develop settings

    pub fn show_develop_menu(&mut self, tab_id: &str) {
        let entries = self.develop_menu_entries(tab_id, false);
        let tab = tab_id.to_string();
        let at = self.menu_point(tab_id);
        self.popup(entries, at, move |c, id| c.run_develop_item(&tab, id));
    }

    /// Items of the Develop menu for a tab. `full`: the menu bar's, with cache and site data commands (the address bar
    /// button's has the responsive mode instead). Ids are run by [`Core::run_develop_item`].
    pub fn develop_menu_entries(&self, tab: &str, full: bool) -> Vec<Entry> {
        let t = signet_core::i18n::t;
        let o = self.dev_overrides(tab);
        let enabled = self.views.view_of(tab).is_some();
        let item = |label: String, id: &str, checked: bool| Entry::Item { label, id: id.into(), enabled, checked };
        let mut e = Vec::new();
        if full {
            e.push(item(t("develop.emptyCache"), "dev:emptyCache", false));
            e.push(item(t("develop.clearSiteData"), "dev:clearSiteData", false));
            e.push(Entry::Sep);
        } else {
            e.push(item(t("develop.responsive"), "dev:responsive", o.responsive.is_some()));
            e.push(Entry::Sep);
        }
        e.push(item(t("develop.noCache"), "dev:noCache", o.no_cache == Some(true)));
        e.push(item(t("develop.noJs"), "dev:noJs", o.no_js == Some(true)));
        e.push(Entry::Sep);
        e.push(Entry::Sub {
            label: t("develop.scheme"),
            items: vec![
                item(t("develop.schemeSystem"), "dev:scheme:system", o.scheme.is_none()),
                item(t("develop.schemeLight"), "dev:scheme:light", o.scheme == Some(ColorScheme::Light)),
                item(t("develop.schemeDark"), "dev:scheme:dark", o.scheme == Some(ColorScheme::Dark)),
            ],
        });
        e.push(item(t("develop.reducedMotion"), "dev:motion", o.reduced_motion == Some(true)));
        let mut net = vec![item(t("develop.network.none"), "dev:net:none", o.network.is_none()), Entry::Sep];
        net.extend(NETWORK_IDS.iter().map(|n| item(t(&format!("develop.network.{}", network_id(*n))), &format!("dev:net:{}", network_id(*n)), o.network == Some(*n))));
        e.push(Entry::Sub { label: t("develop.networkTitle"), items: net });
        let mut ua = vec![item(t("develop.ua.default"), "dev:ua:default", o.user_agent.is_none()), Entry::Sep];
        ua.extend(UA_IDS.iter().map(|u| item(t(&format!("develop.ua.{}", ua_id(*u))), &format!("dev:ua:{}", ua_id(*u)), o.user_agent == Some(*u))));
        e.push(Entry::Sub { label: t("develop.uaTitle"), items: ua });
        e.push(Entry::Sep);
        e.push(Entry::Item { label: t("develop.reset"), id: "dev:reset".into(), enabled: enabled && !is_empty(&o), checked: false });
        e
    }

    /// Runs an id of [`Core::develop_menu_entries`] or of the palette actions on a tab.
    pub fn run_develop_item(&mut self, tab: &str, id: &str) {
        match id {
            "dev:responsive" => self.toggle_responsive(tab),
            "dev:emptyCache" => self.empty_cache_and_reload(tab),
            "dev:clearSiteData" => self.clear_tab_site_data(tab),
            _ => {
                let mut o = self.dev_overrides(tab);
                if apply_item(id, &mut o) {
                    self.set_overrides(tab, o);
                }
            }
        }
    }

    /// Command bar actions of developer mode for a tab: (id, title); run them with [`Core::run_develop_item`].
    pub fn develop_palette_actions(&self, tab: &str) -> Vec<(String, String)> {
        let t = signet_core::i18n::t;
        let ta = signet_core::i18n::t_args;
        let o = self.dev_overrides(tab);
        let mut a: Vec<(String, String)> = Vec::new();
        let mut add = |id: &str, title: String| a.push((id.to_string(), title));
        add("dev:responsive", t(if o.responsive.is_some() { "develop.palette.responsiveOff" } else { "develop.palette.responsiveOn" }));
        add("dev:emptyCache", t("develop.emptyCache"));
        add("dev:clearSiteData", t("develop.clearSiteData"));
        add("dev:noCache", t(if o.no_cache == Some(true) { "develop.palette.enableCache" } else { "develop.palette.disableCache" }));
        add("dev:noJs", t(if o.no_js == Some(true) { "develop.palette.enableJs" } else { "develop.palette.disableJs" }));
        if o.scheme != Some(ColorScheme::Light) {
            add("dev:scheme:light", t("develop.palette.forceLight"));
        }
        if o.scheme != Some(ColorScheme::Dark) {
            add("dev:scheme:dark", t("develop.palette.forceDark"));
        }
        if o.scheme.is_some() {
            add("dev:scheme:system", t("develop.palette.followSystem"));
        }
        add("dev:motion", t(if o.reduced_motion == Some(true) { "develop.palette.motionOn" } else { "develop.palette.motionOff" }));
        for n in NETWORK_IDS.iter().filter(|n| o.network != Some(**n)) {
            add(&format!("dev:net:{}", network_id(*n)), ta("develop.palette.network", &[("name", &t(&format!("develop.network.{}", network_id(*n))))]));
        }
        if o.network.is_some() {
            add("dev:net:none", t("develop.palette.networkNone"));
        }
        for u in UA_IDS.iter().filter(|u| o.user_agent != Some(**u)) {
            add(&format!("dev:ua:{}", ua_id(*u)), ta("develop.palette.ua", &[("name", &t(&format!("develop.ua.{}", ua_id(*u))))]));
        }
        if o.user_agent.is_some() {
            add("dev:ua:default", t("develop.palette.uaDefault"));
        }
        if !is_empty(&o) {
            add("dev:reset", t("develop.reset"));
        }
        a
    }

    /// Changes a tab's settings and applies them.
    fn set_overrides(&mut self, tab: &str, wanted: DevOverrides) {
        let Some(v) = self.views.view_of(tab) else { return };
        let mut wanted = wanted;
        // Responsive mode only exists in the main window.
        if wanted.responsive.is_some() && !self.can_dock(tab) {
            wanted.responsive = None;
        }
        let before = self.dev_overrides(tab);
        let next = clean_overrides(&wanted);
        if next == before {
            return;
        }
        let languages = signet_core::languages::accept_language(&self.state().settings.languages);
        let st = &mut self.phase3.devtools;
        if is_empty(&next) {
            st.overrides.remove(tab);
        } else {
            st.overrides.insert(tab.to_string(), (v.id(), next.clone()));
        }
        let reload = needs_reload(&before, &next);
        let mut steps = Vec::new();
        if !size_only(&before, &next) {
            let was_net = st.network_on.get(tab).copied().unwrap_or(false);
            let leave = before.responsive.is_some() && next.responsive.is_none();
            let (s, net) = override_steps(&next, was_net, &languages, leave);
            steps = s;
            st.network_on.insert(tab.to_string(), net);
            if leave {
                st.metrics.remove(tab);
            }
        }
        // The device is emulated before the reload (the layout fixes the scale if needed).
        if let Some(r) = &next.responsive
            && before.responsive.as_ref() != Some(r)
        {
            let scale = st.scales.get(tab).copied().unwrap_or(1.0);
            st.metrics.insert(tab.to_string(), metrics_key(r, scale));
            steps.extend(emulate_steps(r, scale));
        }
        send_steps(v, steps, if reload { Some(Box::new(move || v.reload())) } else { None });
        let value = if is_empty(&next) { Value::Null } else { serde_json::to_value(&next).unwrap_or(Value::Null) };
        self.patch_one(tab, "devOverrides", value, false, false);
        // `devOverrides` does not touch the layout, except responsive mode (page view size).
        if before.responsive != next.responsive {
            self.changed_view();
        }
    }

    /// Reapplies every tab's settings (after a language change, which resets the user agent).
    pub fn reapply_develop(&mut self) {
        let tabs: Vec<String> = self.phase3.devtools.overrides.keys().cloned().collect();
        let languages = signet_core::languages::accept_language(&self.state().settings.languages);
        for tab in tabs {
            let Some(v) = self.views.view_of(&tab) else { continue };
            let o = self.dev_overrides(&tab);
            let was = self.phase3.devtools.network_on.get(&tab).copied().unwrap_or(false);
            let (s, net) = override_steps(&o, was, &languages, false);
            self.phase3.devtools.network_on.insert(tab, net);
            send_steps(v, s, None);
        }
    }

    // -------------------------------------------------------------------- responsive mode

    /// Enters responsive mode with the last device used, or leaves it.
    pub fn toggle_responsive(&mut self, tab: &str) {
        if self.dev_overrides(tab).responsive.is_some() {
            return self.set_responsive(tab, None);
        }
        let last = self.state().settings.responsive.clone();
        let spec = device_spec(last.as_ref().map(|l| device_id(l.device)));
        self.set_responsive(tab, Some(last.unwrap_or(DevResponsive { device: device_of(spec.id), width: spec.width, height: spec.height, base: None, mobile: None })));
    }

    /// Device and size of responsive mode (remembered for next time), or None to leave it.
    pub fn set_responsive(&mut self, tab_id: &str, responsive: Option<DevResponsive>) {
        let mut o = self.dev_overrides(tab_id);
        o.responsive = responsive;
        self.set_overrides(tab_id, o);
        if let Some(kept) = self.dev_overrides(tab_id).responsive {
            self.store.data.state.settings.responsive = Some(kept);
            self.store.schedule(1500); // during a handle drag, one write every 1.5 s at most
        }
    }

    /// Start / end of a handle drag: the scale is frozen at the start, then the page is fitted to the area again.
    pub fn set_responsive_dragging(&mut self, tab_id: &str, dragging: bool) {
        if self.dev_overrides(tab_id).responsive.is_none() {
            return;
        }
        let st = &mut self.phase3.devtools;
        if dragging {
            let s = st.scales.get(tab_id).copied().unwrap_or(1.0);
            st.frozen.insert(tab_id.to_string(), s);
        } else if st.frozen.remove(tab_id).is_some() {
            self.changed_view();
        }
    }

    // -------------------------------------------------------------------- cache and site data

    /// Clears the cache of the tab's whole profile, then reloads without cache.
    pub fn empty_cache_and_reload(&mut self, tab: &str) {
        let Some(v) = self.views.view_of(tab) else { return };
        signet_web::clear_profile_cache(&self.profile_for_tab(tab));
        v.reload_ignore_cache();
    }

    /// Erases cookies and storage of the page's origin, then reloads it.
    pub fn clear_tab_site_data(&mut self, tab: &str) {
        let Some(v) = self.views.view_of(tab) else { return };
        let url = self.state().tabs.get(tab).map(|t| t.url.clone()).unwrap_or_default();
        if !(url.starts_with("http:") || url.starts_with("https:")) {
            return;
        }
        let Ok(u) = url::Url::parse(&url) else { return };
        let origin = u.origin().ascii_serialization();
        v.clear_site_data(&origin, move |_| v.reload_ignore_cache());
    }

    // -------------------------------------------------------------------- developer mode

    fn bump_console(&mut self, tab: &str, error: bool) {
        let c = self.phase3.devtools.counts.entry(tab.to_string()).or_default();
        if error {
            c.0 += 1;
        } else {
            c.1 += 1;
        }
        let (e, w) = *c;
        let nz = |n: u32| if n == 0 { Value::Null } else { json!(n) };
        let mut m = serde_json::Map::new();
        m.insert("consoleErrors".into(), nz(e));
        m.insert("consoleWarnings".into(), nz(w));
        self.patch_tab(tab, m, false, false);
    }

    fn reset_console(&mut self, tab: &str) {
        if self.phase3.devtools.counts.remove(tab).is_none() {
            return;
        }
        let mut m = serde_json::Map::new();
        m.insert("consoleErrors".into(), Value::Null);
        m.insert("consoleWarnings".into(), Value::Null);
        self.patch_tab(tab, m, false, false);
    }
}

/// Ordinary view showing the DevTools frontend. In-memory profile: no page scripts, nothing kept on disk.
fn new_frontend(parent: RawWindowHandle, url: &str, bounds: Bounds) -> Option<WebView> {
    if !signet_web::is_frontend_url(url) {
        return None;
    }
    let opts = ViewOptions { profile: ProfileKind::private("devtools"), url: url.to_string(), bounds, visible: true, transparent_background: false };
    match WebView::create(parent, opts) {
        Ok(v) => Some(v),
        Err(e) => {
            eprintln!("[devtools] cannot create the tools view: {e}");
            None
        }
    }
}

/// Tools view in the `#devtools` window: under the header.
fn window_bounds(size: (f64, f64)) -> Bounds {
    Bounds { x: 0.0, y: DEVTOOLS_BAR as f32, width: size.0.max(1.0) as f32, height: (size.1 - DEVTOOLS_BAR).max(1.0) as f32 }
}

/// Script run in the frontend: finds the node at a page point (`DOMModel.nodeForLocation`, the CDP
/// `DOM.getNodeForLocation`) and reveals it, which selects it in the Elements panel. The frontend's modules are
/// loaded by address, so the module instances are the ones the interface uses.
fn inspect_js(x: i32, y: i32) -> String {
    format!(
        "(async()=>{{try{{const S=await import('/devtools/core/sdk/sdk.js');const C=await import('/devtools/core/common/common.js');\
         const t=S.TargetManager.TargetManager.instance().primaryPageTarget();const m=t&&t.model(S.DOMModel.DOMModel);\
         if(!m)return 'no-dom-model';await m.requestDocument();const n=await m.nodeForLocation({x},{y},false);\
         if(!n)return 'no-node';await C.Revealer.reveal(n);return 'ok '+n.nodeName()}}catch(e){{return 'err '+e}}}})()"
    )
}

/// Frontend address opening on a panel.
fn with_panel(url: &str, panel: &str) -> String {
    format!("{url}&panel={panel}")
}

// ------------------------------------------------------------------------------------------------ hooks

/// Called once after the browser core exists and the web layer is initialized.
pub(super) fn startup(_core: &mut Core) {}

/// Called for every CEF event, before the core handles it.
pub(super) fn web_event(core: &mut Core, ev: &WebEvent) {
    match ev {
        WebEvent::Console { view, level, .. } => {
            if !core.dev_mode() || !matches!(level, ConsoleLevel::Error | ConsoleLevel::Warning) {
                return;
            }
            let Some(tab) = core.views.tab_of(*view).map(str::to_string) else { return };
            let local = core.state().tabs.get(&tab).is_some_and(|t| is_local_dev_url(&t.url));
            if local {
                core.bump_console(&tab, matches!(level, ConsoleLevel::Error));
            }
        }
        // New page (not an internal navigation of a single-page app): the counters restart from zero, like the console.
        WebEvent::LoadStart { view, is_main_frame: true, .. } | WebEvent::RenderProcessGone { view, .. } => {
            if let Some(tab) = core.views.tab_of(*view).map(str::to_string) {
                core.reset_console(&tab);
            }
        }
        WebEvent::LoadEnd { view, is_main_frame: true, url, .. } if core.phase3.devtools.tools.values().any(|t| t.view == Some(*view)) => {
            core.note(format!("devtools-loaded {url}"));
            let Some(t) = core.phase3.devtools.tools.values_mut().find(|t| t.view == Some(*view)) else { return };
            t.loaded = true;
            t.loaded_at = Some(Instant::now());
        }
        // Only the bundled frontend may show in a tools view: anything else is replaced by a blank page.
        WebEvent::AddressChanged { view, url, is_main_frame: true } if !signet_web::is_frontend_url(url) && core.phase3.devtools.tools.values().any(|t| t.view == Some(*view)) => {
            core.log(format!("devtools: refused to show {url} in the tools view"));
            if let Some(v) = WebView::from_id(*view) {
                v.load_url("about:blank");
            }
        }
        WebEvent::Closed { view } => {
            let tab = core.phase3.devtools.tools.iter().find(|(_, t)| t.view == Some(*view)).map(|(k, _)| k.clone());
            if let Some(tab) = tab {
                // Closed from outside (page closed, devtools closed by CEF): the entry goes.
                core.drop_tools(&tab, false);
            }
        }
        _ => {}
    }
}

/// Called on every core tick (about once per second or at the next store deadline).
pub(super) fn tick(core: &mut Core) {
    // Developer mode off: no more counters.
    if !core.dev_mode() {
        let tabs: Vec<String> = core.phase3.devtools.counts.keys().cloned().collect();
        for t in tabs {
            core.reset_console(&t);
        }
    }
    // A new view (tab slept or closed) starts without settings.
    let stale: Vec<String> =
        core.phase3.devtools.overrides.iter().filter(|(tab, (id, _))| core.views.view_of(tab).map(|v| v.id()) != Some(*id)).map(|(k, _)| k.clone()).collect();
    for tab in stale {
        let st = &mut core.phase3.devtools;
        st.overrides.remove(&tab);
        st.network_on.remove(&tab);
        st.metrics.remove(&tab);
        st.scales.remove(&tab);
        st.frozen.remove(&tab);
        core.patch_one(&tab, "devOverrides", Value::Null, false, false);
        core.changed_view();
    }
    // Tools: forget those of closed pages and closed windows, retry the pending ones.
    let tabs: Vec<String> = core.phase3.devtools.tools.keys().cloned().collect();
    for tab in tabs {
        let page = core.views.view_of(&tab);
        let Some(t) = core.phase3.devtools.tools.get_mut(&tab) else { continue };
        let Some(_page) = page else {
            core.drop_tools(&tab, false);
            continue;
        };
        if t.view.is_none() {
            core.pump_tools(&tab); // waiting for the target lookup or the window
        } else if t.view.and_then(WebView::from_id).is_none() {
            core.drop_tools(&tab, true);
        } else {
            core.run_inspect(&tab);
        }
    }
}

/// Called before quitting (flush files, stop threads).
pub(super) fn quit(core: &mut Core) {
    let tabs: Vec<String> = core.phase3.devtools.tools.keys().cloned().collect();
    for tab in tabs {
        core.drop_tools(&tab, true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resp(device: DevDevice, w: u32, h: u32) -> DevResponsive {
        DevResponsive { device, width: w, height: h, base: None, mobile: None }
    }

    #[test]
    fn frontend_address_takes_a_panel() {
        assert_eq!(with_panel("http://127.0.0.1:9/devtools/devtools_app.html?ws=127.0.0.1:9/devtools/page/X", "console"), "http://127.0.0.1:9/devtools/devtools_app.html?ws=127.0.0.1:9/devtools/page/X&panel=console");
        let js = inspect_js(30, 45);
        assert!(js.contains("nodeForLocation(30,45,false)"));
    }

    #[test]
    fn window_view_sits_under_the_header() {
        let b = window_bounds((1000.0, 700.0));
        assert_eq!((b.x, b.y, b.width, b.height), (0.0, 36.0, 1000.0, 664.0));
        assert_eq!(window_bounds((10.0, 10.0)).height, 1.0);
    }

    #[test]
    fn tools_size_is_bounded() {
        // Plenty of room: bounded by the minimums and by the page's.
        assert_eq!(clamp_tools(100.0, 1200.0, 320.0, 320.0), 320.0);
        assert_eq!(clamp_tools(480.0, 1200.0, 320.0, 320.0), 480.0);
        assert_eq!(clamp_tools(2000.0, 1200.0, 320.0, 320.0), 1200.0 - GAP - 320.0);
        // Too narrow for both minimums: half and half.
        assert_eq!(clamp_tools(480.0, 600.0, 320.0, 320.0), ((600.0 - GAP) / 2.0).round());
    }

    #[test]
    fn carves_right_and_bottom() {
        let card = Rect::new(10.0, 20.0, 1000.0, 600.0);
        let c = carve_card(PaneDock::Right, 480.0, card);
        assert_eq!(c.page, Rect::new(10.0, 20.0, 1000.0 - 480.0 - GAP, 600.0));
        assert_eq!(c.tools, Rect::new(10.0 + 1000.0 - 480.0, 20.0, 480.0, 600.0));
        assert_eq!(c.divider, Rect::new(c.page.x + c.page.width, 20.0, GAP, 600.0));
        assert_eq!(c.rect, Rect::new(c.tools.x, 20.0 + DEVTOOLS_BAR, 480.0, 600.0 - DEVTOOLS_BAR));
        let b = carve_card(PaneDock::Bottom, 320.0, card);
        assert_eq!(b.page.height, 600.0 - 320.0 - GAP);
        assert_eq!(b.tools, Rect::new(10.0, b.page.y + b.page.height + GAP, 1000.0, 320.0));
        assert_eq!(b.rect.height, 320.0 - DEVTOOLS_BAR);
        assert_eq!(b.divider.height, GAP);
    }

    #[test]
    fn profiles_follow_the_device() {
        let p = responsive_profile(&resp(DevDevice::Iphone16, 393, 852));
        assert_eq!((p.dpr, p.mobile, p.user_agent), (3.0, true, Some(DevUserAgent::SafariIphone)));
        let p = responsive_profile(&resp(DevDevice::Desktop, 1920, 1080));
        assert_eq!((p.dpr, p.mobile, p.user_agent), (1.0, false, None));
        // Free size: inherits the base device, mobile can be switched.
        let mut c = resp(DevDevice::Custom, 500, 500);
        c.base = Some(DevDevice::Pixel9);
        assert_eq!(responsive_profile(&c).user_agent, Some(DevUserAgent::ChromeAndroid));
        c.mobile = Some(false);
        let p = responsive_profile(&c);
        assert_eq!((p.dpr, p.mobile, p.user_agent), (2.625, false, None));
        // Mobile wanted without a mobile base: the default device's profile.
        let mut d = resp(DevDevice::Custom, 500, 500);
        d.base = Some(DevDevice::Laptop);
        d.mobile = Some(true);
        let p = responsive_profile(&d);
        assert_eq!((p.dpr, p.mobile, p.user_agent), (3.0, true, Some(DevUserAgent::SafariIphone)));
    }

    #[test]
    fn page_is_fitted_and_centered() {
        let area = Rect::new(0.0, 0.0, 1000.0, 800.0);
        // Fits: unscaled, centered under the bar.
        let (f, bar, s) = responsive_frame(400, 600, area, None);
        assert_eq!((s, f), (1.0, Rect::new(300.0, RESPONSIVE_BAR, 400.0, 600.0)));
        assert_eq!(bar, Rect::new(0.0, 0.0, 1000.0, RESPONSIVE_BAR));
        // Too tall: shrunk to the room left (800 - 40 - 16).
        let (f, _, s) = responsive_frame(400, 1500, area, None);
        assert!((s - 744.0 / 1500.0).abs() < 1e-9);
        assert_eq!(f.height, 744.0);
        // Frozen scale: the page keeps its scale and is cropped to the room.
        let (f, _, s) = responsive_frame(2000, 600, area, Some(1.0));
        assert_eq!((s, f.width, f.x), (1.0, 968.0, 16.0));
    }

    #[test]
    fn overrides_are_cleaned() {
        let o = DevOverrides { no_cache: Some(false), no_js: Some(true), reduced_motion: Some(false), responsive: Some(resp(DevDevice::Custom, 50, 9000)), ..Default::default() };
        let c = clean_overrides(&o);
        assert_eq!((c.no_cache, c.no_js, c.reduced_motion), (None, Some(true), None));
        let r = c.responsive.unwrap();
        assert_eq!((r.width, r.height), (200, 3840));
        assert!(is_empty(&clean_overrides(&DevOverrides { no_cache: Some(false), ..Default::default() })));
        // Base and mobile only mean something for a free size.
        let mut r = resp(DevDevice::Iphone16, 393, 852);
        r.base = Some(DevDevice::Pixel9);
        r.mobile = Some(false);
        let c = clean_overrides(&DevOverrides { responsive: Some(r), ..Default::default() });
        assert_eq!((c.responsive.as_ref().unwrap().base, c.responsive.unwrap().mobile), (None, None));
    }

    #[test]
    fn menu_items_toggle_settings() {
        let mut o = DevOverrides::default();
        assert!(apply_item("dev:noCache", &mut o));
        assert_eq!(o.no_cache, Some(true));
        assert!(apply_item("dev:noCache", &mut o));
        assert_eq!(o.no_cache, Some(false));
        assert!(apply_item("dev:scheme:dark", &mut o) && o.scheme == Some(ColorScheme::Dark));
        assert!(apply_item("dev:scheme:system", &mut o) && o.scheme.is_none());
        assert!(apply_item("dev:net:3g", &mut o) && o.network == Some(DevNetwork::ThreeG));
        assert!(apply_item("dev:net:none", &mut o) && o.network.is_none());
        assert!(apply_item("dev:ua:safari-ipad", &mut o) && o.user_agent == Some(DevUserAgent::SafariIpad));
        assert!(apply_item("dev:reset", &mut o) && is_empty(&clean_overrides(&o)));
        assert!(!apply_item("dev:nope", &mut o) && !apply_item("other", &mut o));
    }

    #[test]
    fn reload_rules() {
        let none = DevOverrides::default();
        let js = DevOverrides { no_js: Some(true), ..Default::default() };
        assert!(needs_reload(&none, &js));
        let ua = DevOverrides { user_agent: Some(DevUserAgent::FirefoxMac), ..Default::default() };
        assert!(needs_reload(&none, &ua));
        let dark = DevOverrides { scheme: Some(ColorScheme::Dark), ..Default::default() };
        assert!(!needs_reload(&none, &dark));
        let phone = DevOverrides { responsive: Some(resp(DevDevice::Iphone16, 393, 852)), ..Default::default() };
        let desktop = DevOverrides { responsive: Some(resp(DevDevice::Desktop, 1920, 1080)), ..Default::default() };
        assert!(needs_reload(&none, &phone) && needs_reload(&phone, &desktop));
        // The phone's agent is the same as the Safari iPhone one: choosing it explicitly changes nothing.
        let same = DevOverrides { user_agent: Some(DevUserAgent::SafariIphone), responsive: phone.responsive.clone(), ..Default::default() };
        assert!(!needs_reload(&phone, &same));
        let mut bigger = phone.clone();
        bigger.responsive = Some(resp(DevDevice::Iphone16, 500, 900));
        assert!(size_only(&phone, &bigger) && !size_only(&phone, &desktop) && !size_only(&none, &phone));
    }

    #[test]
    fn steps_for_cache_network_and_agent() {
        let o = DevOverrides { no_cache: Some(true), network: Some(DevNetwork::Offline), ..Default::default() };
        let (s, net) = override_steps(&o, false, "fr,en", false);
        assert!(net);
        let m: Vec<&str> = s.iter().map(|x| x.method).collect();
        assert_eq!(&m[..3], ["Network.enable", "Network.setCacheDisabled", "Network.emulateNetworkConditionsByRule"]);
        assert_eq!(s[1].params["cacheDisabled"], json!(true));
        assert_eq!(s[2].params["offline"], json!(true));
        assert_eq!(s[2].fallback.as_ref().unwrap().0, "Network.emulateNetworkConditions");
        assert!(m.contains(&"Emulation.setUserAgentOverride"));
        // Leaving the settings resets the cache and the connection, then disables the domain.
        let (s, net) = override_steps(&DevOverrides::default(), true, "en", false);
        assert!(!net);
        assert_eq!(s[0].params["cacheDisabled"], json!(false));
        assert_eq!(s[1].params["matchedNetworkConditions"], json!([]));
        assert_eq!(s[2].method, "Network.disable");
        // No network setting: Network is never touched.
        let (s, _) = override_steps(&DevOverrides { no_js: Some(true), ..Default::default() }, false, "en", false);
        assert!(s.iter().all(|x| !x.method.starts_with("Network.")));
        assert_eq!(s[0].params["value"], json!(true));
        // Default agent is an empty string with the languages.
        let ua = s.iter().find(|x| x.method == "Emulation.setUserAgentOverride").unwrap();
        assert_eq!((ua.params["userAgent"].as_str(), ua.params["acceptLanguage"].as_str()), (Some(""), Some("en")));
        // The device's agent when responsive mode is on.
        let phone = DevOverrides { responsive: Some(resp(DevDevice::Iphone16, 393, 852)), ..Default::default() };
        let (s, _) = override_steps(&phone, false, "en", false);
        let ua = s.iter().find(|x| x.method == "Emulation.setUserAgentOverride").unwrap();
        assert_eq!(ua.params["platform"], json!("iPhone"));
        let (s, _) = override_steps(&DevOverrides::default(), false, "en", true);
        assert_eq!(s.last().unwrap().method, "Emulation.setEmitTouchEventsForMouse");
    }

    #[test]
    fn emulation_matches_the_device() {
        let s = emulate_steps(&resp(DevDevice::Iphone16, 852, 393), 0.5);
        assert_eq!(s[0].params["deviceScaleFactor"], json!(3.0));
        assert_eq!(s[0].params["scale"], json!(0.5));
        assert_eq!(s[0].params["screenOrientation"]["type"], json!("landscapePrimary"));
        assert_eq!(s[1].params["enabled"], json!(true));
        let s = emulate_steps(&resp(DevDevice::Laptop, 1280, 800), 1.0);
        assert!(s[0].params.get("screenOrientation").is_none());
        assert_eq!(s[1].params["enabled"], json!(false));
        assert_eq!(metrics_key(&resp(DevDevice::Pixel9, 412, 923), 0.5), "pixel-9:412x923:2.625m@0.5000");
    }

    #[test]
    fn user_agents_and_connections() {
        for u in UA_IDS {
            let (ua, platform) = user_agent_string(u);
            assert!(ua.starts_with("Mozilla/5.0") && !platform.is_empty());
        }
        assert!(user_agent_string(DevUserAgent::ChromeAndroid).0.contains(&format!("Chrome/{CHROME}.")));
        assert_eq!(network_id(DevNetwork::ThreeG), "3g");
        assert_eq!(network_conditions(DevNetwork::Fast4g).1, 165.0);
        assert!(network_conditions(DevNetwork::Offline).0);
        assert_eq!(device_of("pixel-9"), DevDevice::Pixel9);
        assert_eq!(device_id(device_of("ipad-pro-11")), "ipad-pro-11");
    }
}

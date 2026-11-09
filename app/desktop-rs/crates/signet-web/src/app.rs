//! The CEF `App` shared by all processes: custom schemes, message pump scheduling (browser process)
//! and page scripts plus the `__signetPost` bridge (render process).

use std::collections::HashMap;
use std::sync::Mutex;

use cef::{sys, *};

/// Name of the process message carrying a page-to-app bridge call.
pub(crate) const BRIDGE_MESSAGE: &str = "signet.page";
/// Key of the JSON blob passed to renderers through `extra_info`.
pub(crate) const EXTRA_KEY: &str = "signet";
/// Global function injected in pages (non enumerable, read only).
pub const BRIDGE_FUNCTION: &str = "__signetPost";
/// Process message of the internal media reporter (audio state, media session).
pub(crate) const MEDIA_MESSAGE: &str = "signet.media";
/// Temporary global the media script grabs and deletes.
const INTERNAL_FUNCTION: &str = "__signetInternal";
const MEDIA_SCRIPT: &str = include_str!("media.js");

#[derive(Clone)]
struct RenderScript {
    name: String,
    source: String,
    main_only: bool,
}

#[derive(Clone)]
struct RenderBrowser {
    view: u64,
    profile: String,
    scripts: Vec<RenderScript>,
}

static RENDER_SCRIPTS: Mutex<Option<HashMap<i32, RenderBrowser>>> = Mutex::new(None);

/// Print script errors when `SIGNET_WEB_DEBUG` is set (page scripts must never break the page).
fn debug_eval(what: &str, ok: ::std::os::raw::c_int, exc: &Option<V8Exception>) {
    if ok == 0 && std::env::var_os("SIGNET_WEB_DEBUG").is_some() {
        let msg = exc.as_ref().map(|e| CefString::from(&e.message()).to_string()).unwrap_or_default();
        eprintln!("[signet-web] {what} script failed: {msg}");
    }
}

/// Renderer-side trace, enabled by `SIGNET_WEB_DEBUG`.
pub(crate) fn rlog(msg: &str) {
    if std::env::var_os("SIGNET_WEB_DEBUG").is_some() {
        eprintln!("[signet-web renderer {}] {msg}", std::process::id());
    }
}

fn parse_entry(raw: &str) -> Option<RenderBrowser> {
    let v = serde_json::from_str::<serde_json::Value>(raw).ok()?;
    let scripts: Vec<RenderScript> = v["scripts"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|s| RenderScript {
                    name: s["n"].as_str().unwrap_or("script").to_string(),
                    source: s["s"].as_str().unwrap_or_default().to_string(),
                    main_only: s["m"].as_bool().unwrap_or(false),
                })
                .collect()
        })
        .unwrap_or_default();
    Some(RenderBrowser {
        view: v["view"].as_u64().unwrap_or(0),
        profile: v["profile"].as_str().unwrap_or("p:default").to_string(),
        scripts,
    })
}

wrap_v8_handler! {
    struct BridgeHandler {
        message: &'static str,
    }

    impl V8Handler {
        fn execute(
            &self,
            _name: Option<&CefString>,
            _object: Option<&mut V8Value>,
            arguments: Option<&[Option<V8Value>]>,
            _retval: Option<&mut Option<V8Value>>,
            _exception: Option<&mut CefString>,
        ) -> ::std::os::raw::c_int {
            let Some(args) = arguments else { return 1 };
            let text = |i: usize| -> String {
                args.get(i)
                    .and_then(|v| v.as_ref())
                    .map(|v| CefString::from(&v.string_value()).to_string())
                    .unwrap_or_default()
            };
            let (channel, payload) = (text(0), text(1));
            let Some(ctx) = v8_context_get_current_context() else { return 1 };
            let Some(frame) = ctx.frame() else { return 1 };
            let Some(mut msg) = process_message_create(Some(&CefString::from(self.message))) else { return 1 };
            if let Some(list) = msg.argument_list() {
                list.set_string(0, Some(&CefString::from(channel.as_str())));
                list.set_string(1, Some(&CefString::from(payload.as_str())));
            }
            frame.send_process_message(ProcessId::from(sys::cef_process_id_t::PID_BROWSER), Some(&mut msg));
            1
        }
    }
}

wrap_render_process_handler! {
    struct SignetRenderHandler;

    impl RenderProcessHandler {
        fn on_browser_created(&self, browser: Option<&mut Browser>, extra_info: Option<&mut DictionaryValue>) {
            crate::startup::once("renderer.first_browser_created");
            let Some(browser) = browser else { return };
            let Some(extra) = extra_info else {
                rlog(&format!("on_browser_created id={} WITHOUT extra_info", browser.identifier()));
                return;
            };
            let raw = CefString::from(&extra.string(Some(&CefString::from(EXTRA_KEY)))).to_string();
            rlog(&format!("on_browser_created id={} extra_len={}", browser.identifier(), raw.len()));
            let Some(entry) = parse_entry(&raw) else { return };
            if let Ok(mut g) = RENDER_SCRIPTS.lock() {
                g.get_or_insert_with(HashMap::new).insert(browser.identifier(), entry);
            }
        }

        fn on_browser_destroyed(&self, browser: Option<&mut Browser>) {
            if let (Some(b), Ok(mut g)) = (browser, RENDER_SCRIPTS.lock()) {
                if let Some(m) = g.as_mut() {
                    m.remove(&b.identifier());
                }
            }
        }

        fn on_context_created(
            &self,
            browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            context: Option<&mut V8Context>,
        ) {
            let (Some(browser), Some(frame), Some(context)) = (browser, frame, context) else { return };
            rlog(&format!("on_context_created browser={} url={} main={}", browser.identifier(), CefString::from(&frame.url()), frame.is_main()));
            crate::startup::once_with("renderer.first_v8_context", || CefString::from(&frame.url()).to_string());
            // Only browsers created by the app carry the bridge (not DevTools).
            let known = RENDER_SCRIPTS.lock().ok().and_then(|g| g.as_ref().and_then(|m| m.get(&browser.identifier()).cloned()));
            let entry = match known {
                Some(e) => e,
                // A new renderer process after a cross-process navigation: ask the browser process.
                None => {
                    let Some(e) = crate::bundle::lookup(browser.identifier()).and_then(|raw| parse_entry(&raw)) else {
                        rlog("no entry and lookup failed");
                        return;
                    };
                    if let Ok(mut g) = RENDER_SCRIPTS.lock() {
                        g.get_or_insert_with(HashMap::new).insert(browser.identifier(), e.clone());
                    }
                    e
                }
            };
            if let Some(global) = context.global() {
                let mut handler = BridgeHandler::new(BRIDGE_MESSAGE);
                let name = CefString::from(BRIDGE_FUNCTION);
                if let Some(mut func) = v8_value_create_function(Some(&name), Some(&mut handler)) {
                    global.set_value_bykey(
                        Some(&name),
                        Some(&mut func),
                        V8Propertyattribute::from(sys::cef_v8_propertyattribute_t::V8_PROPERTY_ATTRIBUTE_DONTENUM),
                    );
                    let lock = CefString::from(format!(
                        "Object.defineProperty(window,'{BRIDGE_FUNCTION}',{{writable:false,configurable:false,enumerable:false}});"
                    ).as_str());
                    let mut ret = None;
                    let mut exc = None;
                    context.eval(Some(&lock), None, 0, Some(&mut ret), Some(&mut exc));
                }
                // Internal media reporter: its own channel, the script deletes the temporary global.
                let mut media = BridgeHandler::new(MEDIA_MESSAGE);
                let iname = CefString::from(INTERNAL_FUNCTION);
                if let Some(mut func) = v8_value_create_function(Some(&iname), Some(&mut media)) {
                    global.set_value_bykey(
                        Some(&iname),
                        Some(&mut func),
                        V8Propertyattribute::from(sys::cef_v8_propertyattribute_t::V8_PROPERTY_ATTRIBUTE_DONTENUM),
                    );
                    let code = CefString::from(MEDIA_SCRIPT);
                    let url = CefString::from("signet-script://media");
                    let mut ret = None;
                    let mut exc = None;
                    let ok = context.eval(Some(&code), Some(&url), 1, Some(&mut ret), Some(&mut exc));
                    debug_eval("media", ok, &exc);
                }
            }
            let is_main = frame.is_main() == 1;
            // The bundle of the app (signet-scripts): once per document, after `__signetPost`, before page scripts.
            let frame_url = CefString::from(&frame.url()).to_string();
            let frame_id = CefString::from(&frame.identifier()).to_string();
            let fetched = crate::bundle::fetch(entry.view, &entry.profile, &frame_url, is_main, &frame_id);
            rlog(&format!("bundle fetch url={frame_url} -> {}", fetched.as_ref().map(|b| b.len() as i64).unwrap_or(-1)));
            if let Some(bundle) = fetched {
                let code = CefString::from(bundle.as_str());
                let url = CefString::from("signet-script://bundle");
                let mut ret = None;
                let mut exc = None;
                let ok = context.eval(Some(&code), Some(&url), 1, Some(&mut ret), Some(&mut exc));
                debug_eval("bundle", ok, &exc);
            }
            let scripts = entry.scripts;
            for s in scripts {
                if s.main_only && !is_main {
                    continue;
                }
                let code = CefString::from(s.source.as_str());
                let url = CefString::from(format!("signet-script://{}", s.name).as_str());
                let mut ret = None;
                let mut exc = None;
                context.eval(Some(&code), Some(&url), 1, Some(&mut ret), Some(&mut exc));
            }
        }
    }
}

wrap_browser_process_handler! {
    struct SignetBrowserHandler;

    impl BrowserProcessHandler {
        fn on_context_initialized(&self) {
            crate::startup::mark("cef.on_context_initialized");
            crate::runtime::CONTEXT_READY.store(true, std::sync::atomic::Ordering::SeqCst);
        }

        fn on_before_child_process_launch(&self, command_line: Option<&mut CommandLine>) {
            if crate::startup::enabled() {
                let kind = command_line.as_ref().map(|c| CefString::from(&c.switch_value(Some(&CefString::from("type")))).to_string()).unwrap_or_default();
                crate::startup::mark_with(|| format!("cef.child_launch type={kind}"));
            }
            if let (Some(cl), Some(v)) = (command_line, crate::bundle::child_switch_value()) {
                cl.append_switch_with_value(Some(&CefString::from(crate::bundle::SWITCH)), Some(&CefString::from(v.as_str())));
            }
        }

        fn on_schedule_message_pump_work(&self, delay_ms: i64) {
            crate::runtime::wake(delay_ms);
        }
    }
}

wrap_app! {
    pub struct SignetApp {
        render: bool,
    }

    impl App {
        fn on_before_command_line_processing(
            &self,
            _process_type: Option<&CefString>,
            command_line: Option<&mut CommandLine>,
        ) {
            if let Some(cl) = command_line {
                // Do not touch the login keychain on macOS (no password prompt), use a mock one.
                #[cfg(target_os = "macos")]
                {
                    cl.append_switch(Some(&CefString::from("use-mock-keychain")));
                }
                // Experiments: `SIGNET_CEF_SWITCHES=name,name=value` adds Chromium switches (browser and children).
                if let Ok(list) = std::env::var("SIGNET_CEF_SWITCHES") {
                    for item in list.split(',').map(str::trim).filter(|i| !i.is_empty()) {
                        match item.split_once('=') {
                            Some((k, v)) => cl.append_switch_with_value(Some(&CefString::from(k)), Some(&CefString::from(v))),
                            None => cl.append_switch(Some(&CefString::from(item))),
                        }
                    }
                }
                // The DevTools frontend (served on the loopback debugging port) opens its WebSocket from that origin only.
                if let Some(port) = crate::runtime::devtools_port() {
                    cl.append_switch_with_value(Some(&CefString::from("remote-allow-origins")), Some(&CefString::from(format!("http://127.0.0.1:{port}").as_str())));
                }
                let _ = cl;
            }
        }

        fn on_register_custom_schemes(&self, registrar: Option<&mut SchemeRegistrar>) {
            if let Some(r) = registrar {
                crate::scheme::register_custom_schemes(r);
            }
        }

        fn browser_process_handler(&self) -> Option<BrowserProcessHandler> {
            Some(SignetBrowserHandler::new())
        }

        fn render_process_handler(&self) -> Option<RenderProcessHandler> {
            Some(SignetRenderHandler::new())
        }
    }
}

impl SignetApp {
    pub(crate) fn build(render: bool) -> App {
        SignetApp::new(render)
    }
}

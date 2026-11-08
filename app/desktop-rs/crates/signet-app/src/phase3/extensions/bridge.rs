//! Pure helpers of the extension bridge: message parsing, JavaScript to run in frames, the generated background page.

use serde_json::{json, Value};
use signet_extensions::common::CrxResult;
use signet_extensions::host::Injection;
use signet_extensions::manifest::{BackgroundEntry, RunAt};

/// Fragment marking the URL of a background page: its context is the `worker` kind.
pub const BG_MARKER: &str = "#__signet_bg";
/// Path of the generated background page.
pub const BG_PAGE: &str = "_generated_background_page.html";
/// Channel of the popup size reports.
pub const SIZE_CHANNEL: &str = "signet-crx-size";

/// A call posted by `bridge.js`.
#[derive(Debug, Clone, PartialEq)]
pub struct Msg {
    pub id: String,
    pub method: String,
    pub args: Value,
}

pub fn parse_message(payload: &str) -> Option<Msg> {
    let v: Value = serde_json::from_str(payload).ok()?;
    Some(Msg { id: v.get("id")?.as_str()?.to_string(), method: v.get("method")?.as_str()?.to_string(), args: v.get("args").cloned().unwrap_or_else(|| json!([])) })
}

/// Unwraps the envelope content scripts use (`__ext`, extension id, context id, method, args), see `content_wrapper`.
pub fn unwrap_content(msg: &Msg) -> Option<(String, String, Msg)> {
    if msg.method != "__ext" {
        return None;
    }
    let a = msg.args.as_array()?;
    Some((a.first()?.as_str()?.to_string(), a.get(1)?.as_str()?.to_string(), Msg { id: msg.id.clone(), method: a.get(2)?.as_str()?.to_string(), args: a.get(3).cloned().unwrap_or_else(|| json!([])) }))
}

/// The extension id of a `chrome-extension://<id>/...` URL.
pub fn extension_id_of(url: &str) -> Option<String> {
    let rest = url.strip_prefix("chrome-extension://")?;
    let id = rest.split(['/', '?', '#']).next()?;
    signet_extensions::id::is_valid_id(id).then(|| id.to_string())
}

/// A positive 31 bit number for a Signet tab id: the id extensions see (`tabs.Tab.id`).
pub fn tab_number(tab_id: &str) -> i64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in tab_id.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x100000001b3);
    }
    ((h ^ (h >> 31)) & 0x7fff_ffff) as i64 + 1
}

/// `webNavigation` style frame id: 0 for the main frame, a positive number for the others.
pub fn frame_number(frame_id: &str, is_main: bool) -> i64 {
    if is_main {
        return 0;
    }
    if let Ok(n) = frame_id.rsplit(['.', ':']).next().unwrap_or("").parse::<i64>() {
        return n.max(1);
    }
    let mut h: u64 = 5381;
    for b in frame_id.bytes() {
        h = h.wrapping_mul(33) ^ u64::from(b);
    }
    (h & 0x7fff_ffff) as i64 + 1
}

fn js_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

/// Delivers `json` on `channel` to `window.__signetCrxDispatch` of a frame (silent when the frame has no bridge).
pub fn dispatch(channel: &str, json: &str) -> String {
    format!("try{{window.__signetCrxDispatch({},{})}}catch(e){{}};", js_str(channel), js_str(json))
}

pub fn reply_js(msg_id: &str, result: &CrxResult) -> String {
    dispatch("signet-crx-reply", &json!({ "id": msg_id, "result": result.to_json() }).to_string())
}

/// An event for every listener of an extension page or background.
pub fn event_js(name: &str, args: &[Value]) -> String {
    dispatch("signet-crx-event", &json!({ "name": name, "args": args }).to_string())
}

/// An event for one context.
pub fn ctx_js(name: &str, args: &[Value]) -> String {
    dispatch("signet-crx-ctx", &json!({ "name": name, "args": args }).to_string())
}

/// An event for the content scripts of one extension in a frame.
pub fn hub_js(ext: &str, name: &str, args: &[Value]) -> String {
    format!("try{{__signetCrxHub.emit({},{},{})}}catch(e){{}};", js_str(ext), js_str(name), json!(args))
}

/// The page script of a content script scope. All extensions of a frame share `window.__signetCrx` (one bridge), whose
/// calls carry no extension id. Each scope therefore runs with a private bridge that wraps every call in an envelope
/// (`__ext`) naming the extension and the context id the app handed out; the app checks both against the frame the
/// message comes from. Events for content scripts arrive through `__signetCrxHub`.
pub fn content_wrapper(ext: &str, ctx_id: &str, script: &str) -> String {
    let (e, c) = (js_str(ext), js_str(ctx_id));
    format!(
        "{bridge}\n;(function(){{var g=globalThis,base=g.__signetCrx;if(!base)return;\
var hub=g.__signetCrxHub;if(!hub){{hub={{subs:{{}},emit:function(x,n,a){{var l=hub.subs[x]||[];for(var i=0;i<l.length;i++){{try{{l[i](n,a||[])}}catch(e){{}}}}}}}};\
Object.defineProperty(g,'__signetCrxHub',{{value:hub,enumerable:false,configurable:true}})}}\
var mine={{context:null,call:function(m,a){{return base.call('__ext',[{e},{c},m,a||[]])}},subscribe:function(cb){{(hub.subs[{e}]=hub.subs[{e}]||[]).push(cb)}}}};\
g.__signetCrx=mine;try{{\n{script}\n}}finally{{g.__signetCrx=base}}}})();",
        bridge = signet_extensions::assets::BRIDGE_JS
    )
}

/// The program of one content script group, started according to its `run_at`.
pub fn injection_js(inj: &Injection) -> String {
    let mut body = String::new();
    if !inj.css.is_empty() {
        body.push_str(&format!(
            "(function(css){{var add=function(){{var s=document.createElement('style');s.textContent=css;(document.head||document.documentElement).appendChild(s)}};\
if(document.documentElement)add();else{{var mo=new MutationObserver(function(){{if(document.documentElement){{mo.disconnect();add()}}}});mo.observe(document,{{childList:true}})}}}})({});",
            js_str(&inj.css)
        ));
    }
    body.push_str(&inj.js);
    let run = format!("var run=function(){{try{{\n{body}\n}}catch(e){{console.error('[signet] content script failed',e)}}}};");
    match inj.run_at {
        RunAt::DocumentStart => format!("(function(){{{run}run()}})();"),
        RunAt::DocumentEnd => format!("(function(){{{run}if(document.readyState==='loading')document.addEventListener('DOMContentLoaded',run,{{once:true}});else run()}})();"),
        RunAt::DocumentIdle => format!(
            "(function(){{{run}var go=function(){{setTimeout(run,0)}};if(document.readyState==='loading')document.addEventListener('DOMContentLoaded',go,{{once:true}});else go()}})();"
        ),
    }
}

/// Reports the size of the content of a popup (`signet-crx-size`), so the app can resize the popup view.
pub const POPUP_SIZE_SCRIPT: &str = "(function(){var last='';function send(){var d=document.documentElement,b=document.body;if(!d)return;\
var w=Math.ceil(Math.max(d.scrollWidth,b?b.scrollWidth:0)),h=Math.ceil(Math.max(d.scrollHeight,b?b.scrollHeight:0));\
var k=w+'x'+h;if(k!==last){last=k;try{__signetPost('signet-crx-size',JSON.stringify({w:w,h:h}))}catch(e){}}}\
addEventListener('load',send);setInterval(send,250)})();";

/// Hides the Web Store banners that ask to switch browser.
pub const STORE_CSS_SCRIPT: &str = "(function(){var css='[aria-label*=\"Switch to Chrome\" i],[data-test-id*=\"chrome-banner\"],.ZoYCUb,[href*=\"google.com/chrome\"][role=\"banner\"]{display:none!important}';\
var add=function(){var s=document.createElement('style');s.textContent=css;(document.head||document.documentElement).appendChild(s)};\
if(document.documentElement)add();else document.addEventListener('DOMContentLoaded',add,{once:true})})();";

const WORKER_POLYFILL: &str = "self.importScripts=function(){for(var i=0;i<arguments.length;i++){var u=new URL(arguments[i],location.href).href;var x=new XMLHttpRequest();x.open('GET',u,false);x.send();\
if(x.status>=400||(!x.status&&!x.responseText))throw new Error('importScripts failed: '+u);(0,eval)(x.responseText+'\\n//# sourceURL='+u)}};\
self.skipWaiting=function(){return Promise.resolve()};self.clients={claim:function(){return Promise.resolve()},matchAll:function(){return Promise.resolve([])}};";

fn attr(s: &str) -> String {
    s.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;")
}

/// `_generated_background_page.html`: a page that loads the service worker script, the background scripts of an MV2
/// extension, or (for a background page) redirects nothing: those load their own file.
pub fn background_page(entry: &BackgroundEntry) -> Option<String> {
    let scripts = match entry {
        BackgroundEntry::ServiceWorker { path, module } => format!("<script{} src=\"/{}\"></script>", if *module { " type=\"module\"" } else { "" }, attr(path.trim_start_matches('/'))),
        BackgroundEntry::Scripts(list) => list.iter().map(|s| format!("<script src=\"/{}\"></script>", attr(s.trim_start_matches('/')))).collect::<Vec<_>>().join(""),
        BackgroundEntry::Page(_) | BackgroundEntry::None => return None,
    };
    Some(format!("<!doctype html><html><head><meta charset=\"utf-8\"><title>Background</title><script>{WORKER_POLYFILL}</script></head><body>{scripts}</body></html>"))
}

/// The URL of the page that runs an extension's background, `None` when it has none.
pub fn background_url(id: &str, entry: &BackgroundEntry) -> Option<String> {
    let path = match entry {
        BackgroundEntry::ServiceWorker { .. } | BackgroundEntry::Scripts(_) => BG_PAGE.to_string(),
        BackgroundEntry::Page(p) => p.trim_start_matches('/').to_string(),
        BackgroundEntry::None => return None,
    };
    Some(format!("chrome-extension://{id}/{path}{BG_MARKER}"))
}

pub fn mime_of(path: &str) -> &'static str {
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "wasm" => "application/wasm",
        "txt" => "text/plain; charset=utf-8",
        "mp3" => "audio/mpeg",
        "ogg" => "audio/ogg",
        "wav" => "audio/wav",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "xml" => "text/xml; charset=utf-8",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "abcdefghijklmnopabcdefghijklmnop";

    #[test]
    fn messages_parse_and_unwrap() {
        let m = parse_message(r#"{"id":"c1","method":"storage.get","args":[1]}"#).unwrap();
        assert_eq!((m.id.as_str(), m.method.as_str()), ("c1", "storage.get"));
        assert!(parse_message("nope").is_none() && parse_message(r#"{"id":1}"#).is_none());
        let env = parse_message(&json!({ "id": "c2", "method": "__ext", "args": [ID, "ctx", "tabs.query", [{}]] }).to_string()).unwrap();
        let (ext, ctx, inner) = unwrap_content(&env).unwrap();
        assert_eq!((ext.as_str(), ctx.as_str(), inner.method.as_str(), inner.id.as_str()), (ID, "ctx", "tabs.query", "c2"));
        assert!(unwrap_content(&m).is_none());
    }

    #[test]
    fn extension_ids_come_from_the_url() {
        assert_eq!(extension_id_of(&format!("chrome-extension://{ID}/popup.html?x#y")).as_deref(), Some(ID));
        assert_eq!(extension_id_of(&format!("chrome-extension://{ID}")).as_deref(), Some(ID));
        assert!(extension_id_of("chrome-extension://short/x").is_none());
        assert!(extension_id_of("https://example.com/").is_none());
    }

    #[test]
    fn tab_and_frame_numbers_are_stable_and_positive() {
        assert_eq!(tab_number("t-1"), tab_number("t-1"));
        assert_ne!(tab_number("t-1"), tab_number("t-2"));
        assert!(tab_number("anything") > 0 && tab_number("anything") <= 0x8000_0000);
        assert_eq!(frame_number("whatever", true), 0);
        assert_eq!(frame_number("12.34", false), 34);
        assert!(frame_number("ABCDEF", false) > 0);
    }

    #[test]
    fn dispatch_code_is_valid_javascript_text() {
        let js = reply_js("c1", &CrxResult::ok(json!({ "a": "q\"uote" })));
        assert!(js.starts_with("try{window.__signetCrxDispatch(\"signet-crx-reply\","));
        assert!(js.contains("\\\"ok\\\":true"));
        assert!(hub_js(ID, "runtime.onMessage", &[json!(1)]).contains("__signetCrxHub.emit"));
    }

    #[test]
    fn content_wrapper_namespaces_the_bridge() {
        let w = content_wrapper(ID, "ctx-1", "globalThis.X=1;");
        assert!(w.contains("base.call('__ext',[\"abcdefghijklmnopabcdefghijklmnop\",\"ctx-1\",m,a||[]])"));
        assert!(w.contains("globalThis.X=1;") && w.contains("g.__signetCrx=base"));
    }

    #[test]
    fn run_at_decides_when_the_group_starts() {
        let mk = |run_at| Injection { extension_id: ID.into(), run_at, css: "a{b:c}".into(), js: "go();".into() };
        assert!(injection_js(&mk(RunAt::DocumentStart)).ends_with("run()})();"));
        assert!(injection_js(&mk(RunAt::DocumentEnd)).contains("DOMContentLoaded"));
        assert!(injection_js(&mk(RunAt::DocumentIdle)).contains("setTimeout(run,0)"));
        assert!(injection_js(&mk(RunAt::DocumentStart)).contains("a{b:c}"));
    }

    #[test]
    fn the_background_page_loads_the_worker_script() {
        let html = background_page(&BackgroundEntry::ServiceWorker { path: "sw.js".into(), module: true }).unwrap();
        assert!(html.contains("<script type=\"module\" src=\"/sw.js\">") && html.contains("importScripts"));
        let many = background_page(&BackgroundEntry::Scripts(vec!["a.js".into(), "b.js".into()])).unwrap();
        assert!(many.contains("src=\"/a.js\"") && many.contains("src=\"/b.js\""));
        assert!(background_page(&BackgroundEntry::Page("bg.html".into())).is_none());
        assert_eq!(background_url(ID, &BackgroundEntry::Page("/bg.html".into())).unwrap(), format!("chrome-extension://{ID}/bg.html{BG_MARKER}"));
        assert!(background_url(ID, &BackgroundEntry::None).is_none());
    }

    #[test]
    fn scripts_have_no_long_dashes() {
        for s in [POPUP_SIZE_SCRIPT, STORE_CSS_SCRIPT, WORKER_POLYFILL] {
            assert!(!s.contains('\u{2014}') && !s.contains('\u{2013}'));
        }
    }
}

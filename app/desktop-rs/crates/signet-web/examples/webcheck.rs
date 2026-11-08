//! Runtime check of the signet-web additions (hooks, CDP, media, dialogs, overlays, focus).
//! Must run from a bundle (see README: the helper apps sit in Contents/Frameworks). It serves a local
//! HTTP server, runs a scripted sequence, prints `[PASS]` / `[FAIL]` lines and quits by itself.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::rc::Rc;
use std::time::{Duration, Instant};

use futures::channel::oneshot;
use gpui::{App, Bounds, Context, SharedString, Window, WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions, div, prelude::*, px, rgb, size};
use raw_window_handle::{HasWindowHandle as _, RawWindowHandle};
use signet_web::*;

struct Root;
impl gpui::Render for Root {
    fn render(&mut self, _w: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().flex().size_full().bg(rgb(0x11111b)).child(div().w(px(240.)).h_full().bg(rgb(0x1e1e2e)).child(SharedString::from("sidebar")))
    }
}

struct Overlay;
impl gpui::Render for Overlay {
    fn render(&mut self, _w: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().bg(gpui::rgba(0xff000066)).child(SharedString::from("overlay"))
    }
}

#[derive(Default)]
struct Ctx {
    msgs: Vec<(String, bool, String, String, String, String)>,
    blocked: Vec<String>,
    responses: Vec<(String, i32, String)>,
    audio: Vec<bool>,
    sessions: Vec<MediaSessionInfo>,
    dialogs: VecDeque<JsDialogRequest>,
    auths: VecDeque<AuthRequest>,
    prekeys: Vec<(Option<char>, bool)>,
    loads: Vec<String>,
    closed: bool,
    focus_gained: u32,
    cdp_events: Vec<String>,
    title: String,
}

type Shared = Rc<RefCell<Ctx>>;

fn wav() -> Vec<u8> {
    let n = 8000u32;
    let mut v = Vec::new();
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + n).to_le_bytes());
    v.extend_from_slice(b"WAVEfmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&8000u32.to_le_bytes());
    v.extend_from_slice(&8000u32.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&8u16.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&n.to_le_bytes());
    v.extend((0..n).map(|i| 128u8.wrapping_add(((i % 40) as u8).saturating_sub(20))));
    v
}

fn serve() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = l.local_addr().map(|a| a.port()).unwrap_or(0);
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            std::thread::spawn(move || {
                let mut s = s;
                let mut buf = vec![0u8; 8192];
                let n = s.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let path = req.split_whitespace().nth(1).unwrap_or("/").to_string();
                let headers_json = {
                    let m: Vec<String> = req.lines().skip(1).take_while(|l| !l.is_empty()).filter_map(|l| l.split_once(": ")).map(|(k, v)| format!("{}:{}", k.to_lowercase(), v)).collect();
                    serde_json::to_string(&m).unwrap_or_default()
                };
                let (status, ctype, extra, body): (&str, &str, String, Vec<u8>) = match path.as_str() {
                    "/" => ("200 OK", "text/html", String::new(), ROOT.as_bytes().to_vec()),
                    "/frame" => ("200 OK", "text/html", String::new(), FRAME.as_bytes().to_vec()),
                    "/tone.wav" => ("200 OK", "audio/wav", String::new(), wav()),
                    "/headers" => ("200 OK", "application/json", String::new(), headers_json.into_bytes()),
                    "/old" => ("200 OK", "text/html", String::new(), b"<title>OLD</title>old".to_vec()),
                    "/new" => ("200 OK", "text/html", String::new(), b"<title>NEW</title>new".to_vec()),
                    "/ad.js" => ("200 OK", "text/javascript", String::new(), b"window.adLoaded=true".to_vec()),
                    "/beforeunload" => ("200 OK", "text/html", String::new(), b"<title>BU</title><button id=b>x</button><script>onbeforeunload=e=>{e.preventDefault();e.returnValue='x';return 'x'}</script>".to_vec()),
                    "/auth" => {
                        if req.contains("Authorization: Basic dTpw") {
                            ("200 OK", "text/html", String::new(), b"<title>AUTH OK</title>ok".to_vec())
                        } else {
                            ("401 Unauthorized", "text/html", "WWW-Authenticate: Basic realm=\"t\"\r\n".into(), b"no".to_vec())
                        }
                    }
                    _ => ("404 Not Found", "text/plain", String::new(), b"nf".to_vec()),
                };
                let head = format!("HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n{extra}\r\n", body.len());
                let _ = s.write_all(head.as_bytes());
                let _ = s.write_all(&body);
            });
        }
    });
    port
}

const ROOT: &str = r#"<!doctype html><title>root</title>
<script>window.__seen={ran:window.__bundleRan===true,dispatch:typeof window.__signetDispatch};</script>
<script src="/ad.js"></script>
<audio id="a" loop src="/tone.wav"></audio>
<iframe id="f" src="/frame"></iframe>
<script>
navigator.mediaSession.metadata=new MediaMetadata({title:'Song',artist:'Band',artwork:[{src:'/art.png',sizes:'96x96',type:'image/png'}]});
navigator.mediaSession.setActionHandler('nexttrack',()=>{window.__next=(window.__next||0)+1});
</script>
<p style="font:20px system-ui">hello world</p>"#;

const FRAME: &str = r#"<!doctype html><title>frame</title><script>window.__fseen={ran:window.__bundleRan===true};</script>iframe"#;

const BUNDLE: &str = r#"(function(){var post=window.__signetPost;
Object.defineProperty(window,'__signetDispatch',{value:function(c,j){window.__reply=c+':'+j},enumerable:false});
window.__bundleRan=true;
post('bundle:hello',JSON.stringify({href:location.href,main:window===window.top}));})();"#;

async fn sleep(cx: &mut gpui::AsyncApp, ms: u64) {
    cx.background_executor().timer(Duration::from_millis(ms)).await;
}

async fn until(cx: &mut gpui::AsyncApp, ms: u64, mut f: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + Duration::from_millis(ms);
    while Instant::now() < end {
        if f() {
            return true;
        }
        sleep(cx, 25).await;
    }
    f()
}

fn check(name: &str, ok: bool, detail: impl std::fmt::Display) {
    println!("[{}] {name}: {detail}", if ok { "PASS" } else { "FAIL" });
}

async fn cdp(view: WebView, method: &str, params: &str) -> Result<serde_json::Value, String> {
    let (tx, rx) = oneshot::channel();
    view.execute_devtools_method(method, params, move |r| {
        let _ = tx.send(r);
    });
    let r = rx.await.map_err(|_| "dropped".to_string())??;
    serde_json::from_str(&r).map_err(|e| e.to_string())
}

async fn eval(view: WebView, expr: &str) -> serde_json::Value {
    let p = serde_json::json!({"expression": expr, "returnByValue": true, "userGesture": true, "awaitPromise": true}).to_string();
    match cdp(view, "Runtime.evaluate", &p).await {
        Ok(v) => v["result"]["value"].clone(),
        Err(e) => serde_json::json!(format!("ERR {e}")),
    }
}

async fn run(cx: &mut gpui::AsyncApp, ctx: Shared, view: WebView, parent: RawWindowHandle, overlay: Option<RawWindowHandle>, port: u16) {
    let base = format!("http://127.0.0.1:{port}");
    // ---- 1. bundle, messages, frames
    view.load_url(&format!("{base}/"));
    let ok = until(cx, 8000, || ctx.borrow().loads.iter().any(|u| u.ends_with("/frame")) && ctx.borrow().msgs.len() >= 2).await;
    let msgs = ctx.borrow().msgs.clone();
    check("bundle runs in main frame and iframe", ok, format!("{} messages", msgs.len()));
    let main_m = msgs.iter().find(|m| m.1);
    let sub_m = msgs.iter().find(|m| !m.1);
    check("PageMessage main: frame id and top url", main_m.is_some_and(|m| !m.2.is_empty() && m.3 == format!("{base}/")), format!("{main_m:?}"));
    check("PageMessage iframe: own url, top url, frame id", sub_m.is_some_and(|m| m.2.ends_with("/frame") && m.3 == format!("{base}/") && !m.0.is_empty()), format!("{sub_m:?}"));
    let seen = eval(view, "JSON.stringify([window.__seen, window.frames[0].__fseen])").await;
    check("bundle ran before page scripts (main + iframe)", seen.as_str() == Some(r#"[{"ran":true,"dispatch":"function"},{"ran":true}]"#), &seen);
    if let Some(m) = sub_m {
        let n = view.execute_js_in(&FrameTarget::Id(m.0.clone()), "__signetDispatch('reply','{\"ok\":1}')");
        sleep(cx, 200).await;
        let r = eval(view, "window.frames[0].__reply").await;
        check("execute_js_in targets the iframe by id", n == 1 && r.as_str() == Some("reply:{\"ok\":1}"), &r);
        let n2 = view.execute_js_in(&FrameTarget::Url(format!("{base}/frame")), "window.__byurl=1");
        check("execute_js_in by url", n2 == 1, n2);
    }
    check("frames()", view.frames().len() == 2 && view.frames()[0].is_main, format!("{:?}", view.frames().iter().map(|f| f.url.clone()).collect::<Vec<_>>()));

    // ---- 2. request filter: block, redirect, headers
    let blocked = ctx.borrow().blocked.clone();
    let ad = eval(view, "String(window.adLoaded)").await;
    check("request filter blocks /ad.js", blocked.iter().any(|u| u.ends_with("/ad.js")) && ad.as_str() == Some("undefined"), format!("{blocked:?} adLoaded={ad}"));
    ctx.borrow_mut().loads.clear();
    view.load_url(&format!("{base}/old"));
    let ok = until(cx, 5000, || ctx.borrow().title == "NEW").await;
    check("request filter redirects /old to /new", ok, ctx.borrow().title.clone());
    ctx.borrow_mut().responses.clear();
    view.load_url(&format!("{base}/headers"));
    let ok = until(cx, 5000, || ctx.borrow().responses.iter().any(|r| r.0.ends_with("/headers"))).await;
    let body = eval(view, "document.body.innerText").await;
    check("AddHeaders (Sec-GPC) reaches the server", body.as_str().is_some_and(|b| b.contains("sec-gpc:1")), &body);
    check("MainFrameResponse event with mime", ok && ctx.borrow().responses.iter().any(|r| r.2.contains("application/json") && r.1 == 200), format!("{:?}", ctx.borrow().responses));

    // ---- 3. CDP: screenshot, emulation, UA, mhtml
    view.load_url(&format!("{base}/"));
    until(cx, 5000, || ctx.borrow().title == "root").await;
    sleep(cx, 500).await;
    let (tx, rx) = oneshot::channel();
    view.capture_screenshot(&ScreenshotOptions { clip: Some(signet_web::Bounds::new(0.0, 0.0, 200.0, 100.0)), ..Default::default() }, move |r| {
        let _ = tx.send(r);
    });
    let png = rx.await.unwrap_or(Err("dropped".into()));
    check("screenshot with clip is a PNG", png.as_ref().is_ok_and(|b| b.starts_with(&[0x89, b'P', b'N', b'G'])), png.as_ref().map(|b| format!("{} bytes", b.len())).unwrap_or_else(|e| e.clone()));
    view.set_device_emulation(Some(&DeviceEmulation { width: 375, height: 667, device_scale_factor: 2.0, mobile: false, user_agent: Some("SignetTestUA/1.0".into()), touch: true }));
    sleep(cx, 400).await;
    let w = eval(view, "innerWidth+'x'+innerHeight+' ua='+navigator.userAgent+' touch='+navigator.maxTouchPoints").await;
    check("device emulation (size, UA, touch)", w.as_str().is_some_and(|s| s.starts_with("375x667") && s.contains("SignetTestUA") && s.contains("touch=5")), &w);
    view.set_device_emulation(None);
    sleep(cx, 400).await;
    let w = eval(view, "innerWidth>400&&!navigator.userAgent.includes('SignetTestUA')").await;
    check("device emulation cleared", w.as_bool() == Some(true), &w);
    view.set_user_agent_override(Some("Other/2"));
    sleep(cx, 200).await;
    let ua = eval(view, "navigator.userAgent").await;
    check("set_user_agent_override", ua.as_str() == Some("Other/2"), &ua);
    view.set_user_agent_override(None);
    let path = std::env::temp_dir().join("signet-webcheck.mhtml");
    let (tx, rx) = oneshot::channel();
    view.save_page_mhtml(path.to_str().unwrap_or("x.mhtml"), move |r| {
        let _ = tx.send(r);
    });
    let r = rx.await.unwrap_or(Err("dropped".into()));
    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    check("save_page_mhtml", r.is_ok() && size > 200, format!("{r:?} {size} bytes"));
    let _ = std::fs::remove_file(&path);

    // ---- 4. media state
    let r = eval(view, "document.getElementById('a').play().then(()=>'playing')").await;
    let ok = until(cx, 5000, || ctx.borrow().audio.last() == Some(&true)).await;
    check("AudioPlayingChanged(true) when audio plays", ok, format!("play()={r} {:?}", ctx.borrow().audio));
    let diag = eval(view, "String(MediaSession.prototype.setActionHandler).slice(0,80)+' | '+location.href").await;
    println!("  media diag: {diag}");
    let s = ctx.borrow().sessions.last().cloned();
    check("media session info (title, artist, artwork, actions)", s.as_ref().is_some_and(|s| s.title == "Song" && s.artist == "Band" && s.artwork.first().is_some_and(|a| a.src.ends_with("/art.png")) && s.actions.contains(&"nexttrack".to_string())), format!("{s:?}"));
    view.media_control(MediaCommand::Next);
    sleep(cx, 200).await;
    let n = eval(view, "window.__next").await;
    check("media_control(Next) calls the page handler", n.as_i64() == Some(1), &n);
    view.media_control(MediaCommand::Pause);
    let ok = until(cx, 4000, || ctx.borrow().audio.last() == Some(&false)).await;
    check("AudioPlayingChanged(false) after pause", ok, format!("{:?}", ctx.borrow().audio));
    check("page audio not silenced (is_audio_muted false)", !view.is_audio_muted(), "");

    // ---- 5. history, zoom, css
    view.load_url(&format!("{base}/new"));
    until(cx, 5000, || ctx.borrow().title == "NEW").await;
    let entries = view.navigation_entries();
    check("navigation_entries", entries.len() >= 3 && entries.last().is_some_and(|e| e.current && e.url.ends_with("/new")), format!("{:?}", entries.iter().map(|e| (e.index, e.url.rsplit('/').next().unwrap_or("").to_string(), e.current)).collect::<Vec<_>>()));
    let target = entries.get(entries.len().saturating_sub(3)).cloned();
    if let Some(t) = &target {
        view.go_to_history_index(t.index);
        let ok = until(cx, 5000, || view.url() == t.url).await;
        check("go_to_history_index", ok, format!("{} -> {}", t.url, view.url()));
    }
    view.set_zoom_percent(150);
    sleep(cx, 200).await;
    check("zoom percent round trip", view.zoom_percent() == 150, view.zoom_percent());
    view.zoom_reset();
    let n = view.insert_css(&FrameTarget::Main, "k", "body{background:rgb(1,2,3)!important}");
    sleep(cx, 200).await;
    let bg = eval(view, "getComputedStyle(document.body).backgroundColor").await;
    check("insert_css", n == 1 && bg.as_str() == Some("rgb(1, 2, 3)"), &bg);
    view.remove_css(&FrameTarget::Main, "k");
    sleep(cx, 200).await;
    let bg = eval(view, "getComputedStyle(document.body).backgroundColor").await;
    check("remove_css", bg.as_str() != Some("rgb(1, 2, 3)"), &bg);

    // ---- 7. profile data
    let (tx, rx) = oneshot::channel();
    clear_profile_data(&ProfileKind::persistent("webcheck"), ClearData::default(), move || {
        let _ = tx.send(());
    });
    let ok = futures::future::select(Box::pin(rx), Box::pin(cx.background_executor().timer(Duration::from_secs(8)))).await;
    check("clear_profile_data completes", matches!(ok, futures::future::Either::Left((Ok(()), _))), "");
    check("set_spellcheck", set_spellcheck(&ProfileKind::persistent("webcheck"), true, &["en-US", "fr"]).is_ok(), format!("{:?}", set_spellcheck(&ProfileKind::persistent("webcheck"), true, &["en-US"])));

    // ---- 8. focus and keys (macOS synthesized events)
    #[cfg(target_os = "macos")]
    {
        view.focus();
        sleep(cx, 300).await;
        check("view.focus(): first responder is in CEF", signet_web::mac::describe_first_responder(parent).contains("in_cef=true"), signet_web::mac::describe_first_responder(parent));
        let _ = signet_web::mac::post_click(parent, 100.0, 300.0);
        sleep(cx, 400).await;
        check("click on a GPUI area takes the focus back from CEF", signet_web::mac::describe_first_responder(parent).contains("in_cef=false"), signet_web::mac::describe_first_responder(parent));
        let _ = signet_web::mac::post_click(parent, 700.0, 300.0);
        sleep(cx, 400).await;
        check("click on the page gives CEF the focus", signet_web::mac::describe_first_responder(parent).contains("in_cef=true"), format!("{} focus_gained={}", signet_web::mac::describe_first_responder(parent), ctx.borrow().focus_gained));
        ctx.borrow_mut().prekeys.clear();
        // Cmd+L, Cmd+W, Cmd+Q while the page has focus
        for (code, ch) in [(37u16, "l"), (13, "w"), (12, "q")] {
            let _ = signet_web::mac::post_key_event(parent, code, ch, 1 << 20, true);
            let _ = signet_web::mac::post_key_event(parent, code, ch, 1 << 20, false);
            sleep(cx, 300).await;
        }
        let keys = ctx.borrow().prekeys.clone();
        check("Cmd+L / Cmd+W / Cmd+Q reach the key filter while a page has focus", ['l', 'w', 'q'].iter().all(|c| keys.contains(&(Some(*c), true))), format!("{keys:?}"));
        // overlay
        if let Some(o) = overlay {
            let r = signet_web::overlay::attach_child_window(parent, o);
            let _ = signet_web::overlay::configure_overlay_window(o);
            check("attach_child_window", r.is_ok(), format!("{r:?}"));
            check("overlay window is a child of the main window", mac_children(parent) == 1, mac_children(parent));
            let _ = signet_web::overlay::set_click_through(o, true);
            check("click-through", mac_ignores(o), "");
            let _ = signet_web::overlay::set_click_through(o, false);
            let r = signet_web::overlay::detach_child_window(parent, o);
            check("detach_child_window", r.is_ok() && mac_children(parent) == 0, format!("{r:?}"));
            let _ = signet_web::overlay::attach_child_window(parent, o);
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (parent, overlay);
    dialogs(cx, &ctx, view, &base).await;
    println!("[done]");
}


async fn timeout<T>(cx: gpui::AsyncApp, ms: u64, f: impl std::future::Future<Output = T>) -> Option<T> {
    let t = cx.background_executor().timer(Duration::from_millis(ms));
    match futures::future::select(Box::pin(f), Box::pin(t)).await {
        futures::future::Either::Left((v, _)) => Some(v),
        futures::future::Either::Right(_) => None,
    }
}

async fn dialogs(cx: &mut gpui::AsyncApp, ctx: &Shared, view: WebView, base: &str) {
    // ---- 6. dialogs, auth, certificate mode
    set_js_dialog_mode(JsDialogMode::Events);
    let fut = eval(view, "confirm('sure?')");
    let mut cx2 = cx.clone();
    let tfut = timeout(cx.clone(), 6000, fut);
    let handle = async {
        let ok = until(&mut cx2, 4000, || !ctx.borrow().dialogs.is_empty()).await;
        let d = ctx.borrow_mut().dialogs.pop_front();
        (ok, d)
    };
    let (res, (ok, d)) = futures::future::join(tfut, handle).await.pipe_swap();
    let res = res.unwrap_or(serde_json::json!("TIMEOUT"));
    let _ = res;
    check("confirm() reaches the app as JsDialogRequest", ok && d.as_ref().is_some_and(|d| d.kind == JsDialogKind::Confirm && d.message == "sure?"), format!("{d:?}"));
    if let Some(d) = d {
        d.accept(None);
    }
    sleep(cx, 300).await;
    let fut = eval(view, "prompt('name?','bob')");
    let mut cx2 = cx.clone();
    let tfut = timeout(cx.clone(), 6000, fut);
    let handle = async {
        until(&mut cx2, 4000, || !ctx.borrow().dialogs.is_empty()).await;
        let d = ctx.borrow_mut().dialogs.pop_front();
        if let Some(d) = &d { println!("  prompt default = {:?}", d.default_prompt); }
        if let Some(d) = d { d.accept(Some("alice".into())); }
    };
    let (v, _) = futures::future::join(tfut, handle).await;
    let v = v.unwrap_or(serde_json::json!("TIMEOUT"));
    check("prompt() answered by the app", v.as_str() == Some("alice"), &v);

    view.load_url(&format!("{base}/auth"));
    let ok = until(cx, 6000, || !ctx.borrow().auths.is_empty()).await;
    let a = ctx.borrow_mut().auths.pop_front();
    check("AuthRequest event", ok && a.as_ref().is_some_and(|a| a.realm == "t" && a.host == "127.0.0.1" && a.scheme.eq_ignore_ascii_case("basic")), format!("{a:?}"));
    if let Some(a) = a { a.submit("u", "p"); }
    let ok = until(cx, 6000, || ctx.borrow().title == "AUTH OK").await;
    check("auth credentials accepted", ok, ctx.borrow().title.clone());

    // beforeunload on close
    view.load_url(&format!("{base}/beforeunload"));
    until(cx, 5000, || ctx.borrow().title == "BU").await;
    sleep(cx, 300).await;
    let _ = cdp(view, "Input.dispatchMouseEvent", r#"{"type":"mousePressed","x":20,"y":20,"button":"left","clickCount":1}"#).await;
    let _ = cdp(view, "Input.dispatchMouseEvent", r#"{"type":"mouseReleased","x":20,"y":20,"button":"left","clickCount":1}"#).await;
    view.request_close();
    let ok = until(cx, 5000, || !ctx.borrow().dialogs.is_empty()).await;
    let d = ctx.borrow_mut().dialogs.pop_front();
    check("beforeunload dialog on request_close", ok && d.as_ref().is_some_and(|d| d.kind == JsDialogKind::BeforeUnload), format!("{d:?}"));
    if let Some(d) = d { d.dismiss(); }
    sleep(cx, 500).await;
    check("dismissing keeps the view open", !ctx.borrow().closed, "");
    set_js_dialog_mode(JsDialogMode::Native);

}

trait PipeSwap<A, B> { fn pipe_swap(self) -> (A, B); }
impl<A, B> PipeSwap<A, B> for (A, B) { fn pipe_swap(self) -> (A, B) { self } }

#[cfg(target_os = "macos")]
fn mac_children(parent: RawWindowHandle) -> usize {
    if let RawWindowHandle::AppKit(h) = parent {
        unsafe {
            let view = &*(h.ns_view.as_ptr() as *mut objc2::runtime::AnyObject);
            let w: *mut objc2::runtime::AnyObject = objc2::msg_send![view, window];
            let kids: *mut objc2::runtime::AnyObject = objc2::msg_send![&*w, childWindows];
            if kids.is_null() { return 0; }
            let n: usize = objc2::msg_send![&*kids, count];
            return n;
        }
    }
    0
}

#[cfg(target_os = "macos")]
fn mac_ignores(h: RawWindowHandle) -> bool {
    if let RawWindowHandle::AppKit(h) = h {
        unsafe {
            let view = &*(h.ns_view.as_ptr() as *mut objc2::runtime::AnyObject);
            let w: *mut objc2::runtime::AnyObject = objc2::msg_send![view, window];
            let b: bool = objc2::msg_send![&*w, ignoresMouseEvents];
            return b;
        }
    }
    false
}

fn main() {
    let ctx: Shared = Rc::new(RefCell::new(Ctx::default()));
    let port = serve();
    gpui_platform::application().run(move |cx: &mut App| {
        let data_dir = std::env::temp_dir().join("signet-webcheck-data");
        let _ = std::fs::remove_dir_all(&data_dir);
        let mut config = Config::new(data_dir);
        config.log_severity = 3;
        if let Err(e) = init(config) {
            eprintln!("init failed: {e}");
            cx.quit();
            return;
        }
        set_frame_bundle(|f: &FrameInfo| f.url.starts_with("http://127.0.0.1").then(|| BUNDLE.to_string()));
        set_request_filter(|r: &RequestInfo| {
            if r.url.ends_with("/ad.js") {
                RequestDecision::Block
            } else if r.url.ends_with("/old") {
                RequestDecision::Redirect(r.url.replace("/old", "/new"))
            } else if r.url.ends_with("/headers") {
                RequestDecision::AddHeaders(vec![("Sec-GPC".into(), "1".into())])
            } else {
                RequestDecision::Allow
            }
        });
        set_key_filter(|e| e.pressed && e.cmd && matches!(e.character, Some('q') | Some('l') | Some('w')));
        let c2 = ctx.clone();
        signet_web::gpui_pump::install(cx, move |ev, _cx| {
            let mut c = c2.borrow_mut();
            match ev {
                WebEvent::PageMessage { frame_url, is_main_frame, channel, payload, frame_id, top_url, .. } => c.msgs.push((frame_id, is_main_frame, frame_url, top_url, channel, payload)),
                WebEvent::RequestBlocked { url, .. } => c.blocked.push(url),
                WebEvent::MainFrameResponse { url, status, mime_type, .. } => c.responses.push((url, status, mime_type)),
                WebEvent::AudioPlayingChanged { playing, .. } => c.audio.push(playing),
                WebEvent::MediaSession { info, .. } => c.sessions.push(info),
                WebEvent::JsDialogRequest { request, .. } => c.dialogs.push_back(request),
                WebEvent::AuthRequest { request, .. } => c.auths.push_back(request),
                WebEvent::PreKey { event, handled, .. } => c.prekeys.push((event.character, handled)),
                WebEvent::LoadEnd { url, .. } => c.loads.push(url),
                WebEvent::Closed { .. } => c.closed = true,
                WebEvent::FocusGained { .. } => c.focus_gained += 1,
                WebEvent::TitleChanged { title, .. } => c.title = title,
                WebEvent::DevToolsEvent { method, .. } => c.cdp_events.push(method),
                _ => {}
            }
        });

        let bounds = Bounds::centered(None, size(px(1000.), px(700.)), cx);
        let slot: Rc<RefCell<Option<(WebView, RawWindowHandle)>>> = Rc::new(RefCell::new(None));
        let s2 = slot.clone();
        let _ = cx.open_window(
            WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), ..Default::default() },
            move |window, cx| {
                let handle = window.window_handle().expect("handle").as_raw();
                let view = WebView::create(
                    handle,
                    ViewOptions { profile: ProfileKind::persistent("webcheck"), url: "about:blank".into(), bounds: signet_web::Bounds::new(240.0, 0.0, 760.0, 700.0), ..Default::default() },
                )
                .expect("view");
                *s2.borrow_mut() = Some((view, handle));
                cx.new(|_| Root)
            },
        );
        let ov: Rc<RefCell<Option<RawWindowHandle>>> = Rc::new(RefCell::new(None));
        let o2 = ov.clone();
        let _ = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(gpui::point(px(300.), px(200.)), size(px(200.), px(100.))))),
                titlebar: None,
                focus: false,
                kind: WindowKind::PopUp,
                window_background: WindowBackgroundAppearance::Transparent,
                ..Default::default()
            },
            move |window, cx| {
                *o2.borrow_mut() = window.window_handle().ok().map(|h| h.as_raw());
                cx.new(|_| Overlay)
            },
        );
        cx.activate(true);
        let ctx = ctx.clone();
        cx.spawn(async move |cx| {
            sleep(cx, 1500).await;
            let Some((view, handle)) = slot.borrow().clone() else { return };
            let overlay = *ov.borrow();
            let t = Instant::now();
            run(cx, ctx, view, handle, overlay, port).await;
            println!("[elapsed] {:?}", t.elapsed());
            let _ = cx.update(|cx| cx.quit());
        })
        .detach();
        // Safety net.
        cx.spawn(async move |cx| {
            sleep(cx, 85_000).await;
            println!("[timeout]");
            let _ = cx.update(|cx| cx.quit());
        })
        .detach();
    });
}

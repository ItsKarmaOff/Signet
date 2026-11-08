//! PHASE3: messages sent by page scripts (`window.__signetPost`): routed by channel to passwords, autofill, boosts,
//! reader, media, webauthn... Pages are untrusted: check `tab`, `frame_url` and the payload shape.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use signet_scripts::validate::{origin_of_url, refusal_reply};
use signet_scripts::{Accepted, FrameContext, PageEvent, Registry, Reply, RunAt, ScriptOptions, ScriptSettings, Sender};
use signet_web::{FrameTarget, ViewId, WebView};

use crate::browser::Core;

/// Quiet period after a swipe navigation (port of `COOLDOWN` in `browser/swipe.ts`).
const SWIPE_COOLDOWN: Duration = Duration::from_millis(120);
/// A pending request older than this is forgotten (the page timed out long before; passkeys wait 10 minutes).
const PENDING_MAX: Duration = Duration::from_secs(11 * 60);

/// Channels that modules read directly in their `web_event` hook: not routed here, not logged.
const HANDLED_ELSEWHERE: [&str; 4] = ["sg:reader", "sg:translate", "zap:result", "webapp:page"];

/// Who sent a page message (built by the browser process, never from the payload) and how to answer it.
#[derive(Debug, Clone)]
pub struct PageCtx {
    pub tab: String,
    pub view: signet_web::ViewId,
    pub frame_id: String,
    pub frame_url: String,
    pub is_main_frame: bool,
    /// Request id: `Some` for requests (answer exactly once with `page_reply`), `None` for sends.
    pub rid: Option<String>,
    pub sender: Sender,
}

/// Outcome of the checks on a raw page message.
#[derive(Debug, PartialEq)]
pub(crate) enum Routed {
    /// Valid: hand to the owner of the channel.
    Dispatch { rid: Option<String>, accepted: Accepted },
    /// Refused or malformed request: answer this at once.
    Answer(Reply),
    /// Refused or malformed send: nothing to do.
    Drop,
}

/// Builds the sender from what the browser process knows (never from the payload).
pub(crate) fn build_sender(is_tab: bool, is_private: bool, is_main_frame: bool, frame_url: &str, top_url: &str) -> Sender {
    Sender { is_tab, is_private, is_main_frame, frame_url: frame_url.to_owned(), origin: origin_of_url(frame_url), top_origin: origin_of_url(top_url) }
}

/// Parse then validate a message. A request that cannot be served always gets an answer when it carries an id.
pub(crate) fn route(sender: &Sender, channel: &str, payload: &str) -> Routed {
    match signet_scripts::messages::parse(channel, payload) {
        Err(_) => match raw_rid(payload) {
            Some(rid) => Routed::Answer(Reply::err(&rid, "invalid")),
            None => Routed::Drop,
        },
        Ok(inc) => match signet_scripts::validate(sender, &inc.message, &[]) {
            Ok(accepted) => Routed::Dispatch { rid: inc.rid, accepted },
            Err(refusal) => match inc.rid.as_deref().and_then(|rid| refusal_reply(rid, &inc.message, &refusal)) {
                Some(r) => Routed::Answer(r),
                None => Routed::Drop,
            },
        },
    }
}

fn raw_rid(payload: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(payload).ok()?;
    v.get("rid")?.as_str().filter(|r| !r.is_empty() && r.len() <= 64).map(str::to_owned)
}

/// Frames that must not get the page scripts: extension pages, DevTools and the interface itself.
pub(crate) fn skips_scripts(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    let app = crate::browser::layout::APP_BASE;
    lower.starts_with("chrome-extension:")
        || lower.starts_with("devtools:")
        || lower.starts_with("chrome-devtools:")
        || signet_web::is_frontend_url(url)
        || lower == app
        || [format!("{app}/"), format!("{app}?"), format!("{app}#")].iter().any(|p| lower.starts_with(p.as_str()))
}

/// Answer of `swipe:can` from the setting, the cooldown and the history state.
pub(crate) fn swipe_state(setting_on: bool, cooling: bool, back: bool, forward: bool) -> signet_scripts::messages::SwipeState {
    if !setting_on || cooling {
        return signet_scripts::messages::SwipeState::OFF;
    }
    signet_scripts::messages::SwipeState { enabled: true, back, forward }
}

fn is_notes_url(url: &str) -> bool {
    url.starts_with("signet://note/") || url.starts_with("signet://easel/")
}

/// State owned by this feature (a field of `Phase3`).
#[derive(Default)]
pub struct State {
    /// Script settings read by the bundle closure (CEF helper thread).
    settings: Arc<RwLock<ScriptSettings>>,
    /// Views of private windows (not in `Core::views`), registered by the windows module: view to tab id.
    private_views: HashMap<ViewId, String>,
    /// Requests waiting for their answer: (view, frame, rid) to the time they arrived.
    pending: HashMap<(ViewId, String, String), Instant>,
    swipe_last: HashMap<ViewId, Instant>,
    last_locale: Option<String>,
    last_appearance: Option<String>,
}

fn script_settings(core: &Core) -> ScriptSettings {
    ScriptSettings { global_privacy_control: core.state().settings.global_privacy_control == Some(true), webauthn_intercept: signet_scripts::scripts::webauthn_intercept_enabled() }
}

/// Called once after the browser core exists and the web layer is initialized.
pub(super) fn startup(core: &mut Core) {
    let shared = core.phase3.pages.settings.clone();
    if let Ok(mut w) = shared.write() {
        *w = script_settings(core);
    }
    core.phase3.pages.last_locale = Some(core.locale().id().to_owned());
    core.phase3.pages.last_appearance = serde_json::to_string(&core.appearance()).ok();
    let registry = Registry::new(ScriptOptions::default());
    super::hooks::add_bundle_part(0, move |f| {
        if skips_scripts(&f.url) {
            return None;
        }
        let settings = shared.read().map(|s| *s).unwrap_or_default();
        let ctx = FrameContext { url: f.url.clone(), is_main_frame: f.is_main_frame, is_private: f.is_private, settings };
        registry.bundle_for(&ctx, RunAt::DocumentStart)
    });
}

/// Called for every CEF event, before the core handles it.
pub(super) fn web_event(core: &mut Core, ev: &signet_web::WebEvent) {
    // Tabs are handled by the core (`on_page_message`); views of private windows only reach this hook.
    if let signet_web::WebEvent::PageMessage { view, frame_url, is_main_frame, channel, payload, frame_id, top_url } = ev
        && let Some(tab) = core.phase3.pages.private_views.get(view).cloned()
    {
        core.on_page_message(*view, &tab, frame_id, frame_url, top_url, *is_main_frame, channel, payload);
    }
}

/// Called on every core tick (about once per second or at the next store deadline).
pub(super) fn tick(core: &mut Core) {
    let now = script_settings(core);
    if let Ok(mut w) = core.phase3.pages.settings.write() {
        *w = now;
    }
    core.phase3.pages.pending.retain(|_, t| t.elapsed() < PENDING_MAX);
    core.phase3.pages.swipe_last.retain(|_, t| t.elapsed() < Duration::from_secs(60));

    let locale = core.locale().id().to_owned();
    let look = core.appearance();
    let look_json = serde_json::to_string(&look).ok();
    let mut events = Vec::new();
    if core.phase3.pages.last_locale.as_deref() != Some(locale.as_str()) {
        events.push(PageEvent::Locale(locale.clone()));
        core.phase3.pages.last_locale = Some(locale);
    }
    if core.phase3.pages.last_appearance != look_json {
        if let Ok(v) = serde_json::to_value(look) {
            events.push(PageEvent::Appearance(v));
        }
        core.phase3.pages.last_appearance = look_json;
    }
    if events.is_empty() {
        return;
    }
    for id in core.views.ids() {
        let Some(v) = core.views.view_of(&id) else { continue };
        if is_notes_url(&v.url()) {
            for e in &events {
                v.execute_js_in(&FrameTarget::Main, &e.to_js());
            }
        }
    }
}

/// Called before quitting (flush files, stop threads).
pub(super) fn quit(core: &mut Core) {
    core.phase3.pages.pending.clear();
}

impl Core {
    /// Registers a view of a private window as a private tab for page messages (call when it is created).
    pub fn pages_register_private_view(&mut self, view: ViewId, tab: &str) {
        self.phase3.pages.private_views.insert(view, tab.to_owned());
    }

    /// Forgets a private view (call when it is destroyed).
    pub fn pages_unregister_view(&mut self, view: ViewId) {
        self.phase3.pages.private_views.remove(&view);
        self.phase3.pages.pending.retain(|(v, _, _), _| *v != view);
        self.phase3.pages.swipe_last.remove(&view);
    }

    /// Entry point of every `window.__signetPost` message. `tab` is the tab (or private tab) id of `view`.
    #[allow(clippy::too_many_arguments)]
    pub fn on_page_message(&mut self, view: ViewId, tab: &str, frame_id: &str, frame_url: &str, top_url: &str, is_main_frame: bool, channel: &str, payload: &str) {
        // Private channels of other modules (reader, translation, zap, web apps) are read by their own `web_event`.
        if HANDLED_ELSEWHERE.contains(&channel) {
            return;
        }
        let private = self.phase3.pages.private_views.contains_key(&view);
        let sender = build_sender(true, private, is_main_frame, frame_url, top_url);
        let ctx = |rid: Option<String>| PageCtx { tab: tab.to_owned(), view, frame_id: frame_id.to_owned(), frame_url: frame_url.to_owned(), is_main_frame, rid, sender: sender.clone() };
        match route(&sender, channel, payload) {
            Routed::Dispatch { rid, accepted } => {
                if let Some(r) = &rid {
                    self.phase3.pages.pending.insert((view, frame_id.to_owned(), r.clone()), Instant::now());
                }
                self.page_dispatch(&ctx(rid), accepted);
            }
            Routed::Answer(reply) => self.send_reply(view, frame_id, &reply),
            Routed::Drop => self.log(format!("page message {channel} dropped from {frame_url}")),
        }
    }

    fn send_reply(&self, view: ViewId, frame_id: &str, reply: &Reply) {
        if let Some(v) = WebView::from_id(view) {
            v.execute_js_in(&FrameTarget::Id(frame_id.to_owned()), &reply.to_js());
        }
    }

    /// Answers a page request (`ctx.rid` must be `Some`) in the frame that sent it. A request is answered once: a
    /// second reply, or one for an unknown id, is ignored.
    pub fn page_reply(&mut self, ctx: &PageCtx, reply: Reply) {
        let Some(rid) = &ctx.rid else {
            self.log("page_reply on a send, ignored");
            return;
        };
        if self.phase3.pages.pending.remove(&(ctx.view, ctx.frame_id.clone(), rid.clone())).is_none() {
            self.log("page_reply for an unknown or answered request, ignored");
            return;
        }
        self.send_reply(ctx.view, &ctx.frame_id, &reply);
    }

    /// Router target: `Accepted` messages are dispatched to the owning feature (see each module's `page_*`).
    pub(super) fn page_dispatch(&mut self, ctx: &PageCtx, accepted: Accepted) {
        match accepted {
            Accepted::PwAccounts { .. } | Accepted::PwFill { .. } | Accepted::PwSubmit { .. } => self.page_passwords(ctx, accepted),
            // A form was submitted: the navigation that follows is not a link, so no Peek (`noteFormSubmit`).
            Accepted::PageSubmit => self.note_form_submit(&ctx.tab),
            Accepted::AfMenu { .. } | Accepted::AfFill { .. } | Accepted::AfManage { .. } | Accepted::AfSubmit { .. } => self.page_autofill(ctx, accepted),
            Accepted::WebauthnRequest(_) | Accepted::WebauthnAbort => self.page_webauthn(ctx, accepted),
            Accepted::Notes { .. } => self.page_notes(ctx, accepted),
            Accepted::SwipeCan | Accepted::SwipeNavigate(_) => self.page_swipe(ctx, accepted),
        }
    }

    fn swipe_info(&self, view: ViewId) -> signet_scripts::messages::SwipeState {
        let on = self.state().settings.swipe_navigation != Some(false);
        let cooling = self.phase3.pages.swipe_last.get(&view).is_some_and(|t| t.elapsed() < SWIPE_COOLDOWN);
        let (back, fwd) = WebView::from_id(view).map(|v| (v.can_go_back(), v.can_go_forward())).unwrap_or((false, false));
        swipe_state(on, cooling, back, fwd)
    }

    /// Swipe channels (`swipe:can`, `swipe:navigate`).
    pub fn page_swipe(&mut self, ctx: &PageCtx, accepted: Accepted) {
        use signet_scripts::messages::Direction;
        match accepted {
            Accepted::SwipeCan => {
                let info = self.swipe_info(ctx.view);
                self.page_reply(ctx, Reply::ok(ctx.rid.as_deref().unwrap_or_default(), &info));
            }
            Accepted::SwipeNavigate(dir) => {
                let info = self.swipe_info(ctx.view);
                let Some(v) = WebView::from_id(ctx.view) else { return };
                let (allowed, go): (bool, fn(&WebView)) = match dir {
                    Direction::Back => (info.enabled && info.back, WebView::back),
                    Direction::Forward => (info.enabled && info.forward, WebView::forward),
                };
                if allowed {
                    self.phase3.pages.swipe_last.insert(ctx.view, Instant::now());
                    go(&v);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(url: &str) -> Sender {
        build_sender(true, false, true, url, url)
    }

    #[test]
    fn sender_origins_come_from_urls() {
        let s = build_sender(true, true, false, "https://Sub.Example.com:8443/x", "https://example.com/");
        assert_eq!(s.origin.as_deref(), Some("https://sub.example.com:8443"));
        assert_eq!(s.top_origin.as_deref(), Some("https://example.com"));
        assert!(s.is_private && !s.is_main_frame && s.is_tab);
        assert_eq!(build_sender(true, false, true, "signet://note/1", "signet://note/1").origin, None);
    }

    #[test]
    fn swipe_request_dispatches() {
        let r = route(&tab("https://a.com/"), "swipe:can", r#"{"rid":"r1","data":null}"#);
        assert_eq!(r, Routed::Dispatch { rid: Some("r1".into()), accepted: Accepted::SwipeCan });
    }

    #[test]
    fn refused_request_is_answered_and_send_dropped() {
        let not_tab = build_sender(false, false, true, "https://a.com/", "https://a.com/");
        let r = route(&not_tab, "swipe:can", r#"{"rid":"r1","data":null}"#);
        assert_eq!(r, Routed::Answer(Reply::ok("r1", &signet_scripts::messages::SwipeState::OFF)));
        assert_eq!(route(&not_tab, "swipe:navigate", r#"{"data":"back"}"#), Routed::Drop);
    }

    #[test]
    fn malformed_gets_error_when_it_has_an_id() {
        assert_eq!(route(&tab("https://a.com/"), "pw:fill", r#"{"rid":"r2","data":{"id":5}}"#), Routed::Answer(Reply::err("r2", "invalid")));
        assert_eq!(route(&tab("https://a.com/"), "nope", "{}"), Routed::Drop);
        assert_eq!(route(&tab("https://a.com/"), "swipe:can", "not json"), Routed::Drop);
    }

    #[test]
    fn credentials_need_an_http_origin() {
        let r = route(&tab("signet://note/1"), "pw:accounts", r#"{"rid":"r3","data":null}"#);
        assert_eq!(r, Routed::Answer(Reply::ok("r3", &Vec::<serde_json::Value>::new())));
    }

    #[test]
    fn script_skip_list() {
        assert!(skips_scripts("chrome-extension://abc/popup.html"));
        assert!(skips_scripts("devtools://devtools/bundled/x.html"));
        assert!(skips_scripts("signet://app/index.html"));
        assert!(!skips_scripts("signet://note/abc"));
        assert!(!skips_scripts("https://example.com/"));
    }

    #[test]
    fn swipe_rules() {
        assert!(!swipe_state(false, false, true, true).enabled);
        assert!(!swipe_state(true, true, true, true).enabled);
        let s = swipe_state(true, false, true, false);
        assert!(s.enabled && s.back && !s.forward);
    }
}

//! CEF events and ticks: page messages become host calls, navigations and tab changes become extension events.

use std::collections::{HashMap, HashSet};

use serde_json::{json, Value};
use signet_core::ids::now_ms;
use signet_extensions::common::{ApiContext, ContextKind};
use signet_extensions::tabs::TabHost as _;
use signet_extensions::crx::is_store_url;
use signet_extensions::host::Call;
use signet_extensions::navigation::{is_fragment_change, FrameInfo as NavFrame};
use signet_web::{ViewId, WebEvent};

use super::bridge::{extension_id_of, frame_number, parse_message, tab_number, unwrap_content, Msg, SIZE_CHANNEL};
use super::ctx::Kind;
use super::shared::{Inbox, Shared};
use super::tabhost::{snapshot, HostAction};
use super::{ReplyTo, TabSig};
use crate::browser::Core;
use std::sync::Arc;

const MAX_PENDING_REPLIES: usize = 4000;

impl Core {
    pub(super) fn ext_web_event(&mut self, ev: &WebEvent) {
        let Some(shared) = self.ext_shared() else { return };
        match ev {
            WebEvent::PageMessage { view, frame_url, is_main_frame, channel, payload, frame_id, .. } => match channel.as_str() {
                "signet-crx" => self.ext_message(&shared, *view, frame_url, frame_id, *is_main_frame, payload),
                SIZE_CHANNEL => self.ext_popup_size(*view, payload),
                _ => {}
            },
            WebEvent::Created { .. } => self.ext_refresh_views(&shared),
            WebEvent::Closed { view } => self.ext_view_closed(&shared, *view),
            WebEvent::LoadStart { view, is_main_frame: true, url } => {
                if let Some(tab) = self.ext_tab_of(*view) {
                    self.ext_navigation_event("webNavigation.onBeforeNavigate", tab, url, false);
                }
            }
            WebEvent::AddressChanged { view, url, is_main_frame: true } => self.ext_address_changed(&shared, *view, url),
            WebEvent::LoadEnd { view, is_main_frame, url, .. } => {
                self.ext_offscreen_loaded(&shared, *view);
                if *is_main_frame && let Some(tab) = self.ext_tab_of(*view) {
                    self.ext_navigation_event("webNavigation.onDOMContentLoaded", tab, url, false);
                    self.ext_navigation_event("webNavigation.onCompleted", tab, url, false);
                }
            }
            WebEvent::LoadError { view, code, text, url, is_main_frame, .. } => {
                if let Some(ext) = self.phase3.extensions.rt.as_ref().and_then(|r| r.offscreen.get(view).cloned()) {
                    let fx = shared.host().offscreen_failed(&ext, text);
                    self.ext_apply(fx);
                } else if *is_main_frame && *code != -3 && let Some(tab) = self.ext_tab_of(*view) {
                    let fx = shared.host().load_failed(tab, i64::from(*code), text, url, Some(&NavFrame::main()));
                    self.ext_apply(fx);
                }
            }
            WebEvent::FocusGained { view } => {
                let popup = self.phase3.extensions.rt.as_ref().and_then(|r| r.popup.as_ref().map(|p| p.view.id()));
                if popup.is_some_and(|p| p != *view) {
                    self.ext_close_popup();
                }
            }
            WebEvent::PreKey { event, handled: true, .. } => self.ext_key(&shared, event),
            _ => {}
        }
        self.ext_after_contexts(&shared);
    }

    /// The extension's number for the tab shown by `view`.
    pub(super) fn ext_tab_of(&self, view: ViewId) -> Option<i64> {
        self.views.tab_of(view).map(tab_number)
    }

    pub(super) fn ext_refresh_views(&mut self, shared: &Arc<Shared>) {
        let map: HashMap<ViewId, i64> = self.views.ids().into_iter().filter_map(|id| self.views.view_of(&id).map(|v| (v.id(), tab_number(&id)))).collect();
        if let Ok(mut m) = shared.view_tab.write() {
            *m = map;
        }
    }

    fn ext_view_closed(&mut self, shared: &Arc<Shared>, view: ViewId) {
        let closed = shared.ctxs().remove_view(view);
        {
            let mut host = shared.host();
            for c in closed {
                host.context_closed(&c);
            }
        }
        let mut stopped = None;
        if let Some(rt) = self.phase3.extensions.rt.as_mut() {
            rt.replies.retain(|_, r| r.view != view);
            rt.last_url.remove(&view);
            if let Some(id) = rt.bg.iter().find(|(_, v)| v.id() == view).map(|(k, _)| k.clone()) {
                rt.bg.remove(&id);
                stopped = Some(id);
            }
            if rt.popup.as_ref().is_some_and(|p| p.view.id() == view) {
                rt.popup = None;
                shared.popup_view.store(0, std::sync::atomic::Ordering::Relaxed);
            }
            if let Some(ext) = rt.offscreen.remove(&view) {
                drop(ext);
                shared.host().offscreen_page_closed(view.0 as i64);
            }
        }
        if let Some(id) = stopped {
            shared.host().worker_stopped(&id, "");
            // A background that died is started again at the next event that needs it (StartWorker).
        }
    }

    fn ext_offscreen_loaded(&mut self, shared: &Arc<Shared>, view: ViewId) {
        let ext = self.phase3.extensions.rt.as_ref().and_then(|r| r.offscreen.get(&view).cloned());
        if let Some(ext) = ext {
            let fx = shared.host().offscreen_loaded(&ext, view.0 as i64);
            self.ext_apply(fx);
        }
    }

    fn ext_address_changed(&mut self, shared: &Arc<Shared>, view: ViewId, url: &str) {
        let Some(tab) = self.ext_tab_of(view) else { return };
        let previous = self.phase3.extensions.rt.as_mut().and_then(|r| r.last_url.insert(view, url.to_string()));
        if is_fragment_change(previous.as_deref(), url) {
            let fx = shared.host().navigated_in_page(tab, "main", true, url, Some(&NavFrame::main()));
            self.ext_apply(fx);
            self.ext_navigation_event("webNavigation.onHistoryStateUpdated", tab, url, true);
        } else {
            shared.host().frame_navigated(tab, "main", true, url);
            self.ext_navigation_event("webNavigation.onCommitted", tab, url, true);
        }
    }

    /// A `webNavigation` event of a main frame, for extensions with the `webNavigation` permission.
    fn ext_navigation_event(&mut self, name: &str, tab: i64, url: &str, committed: bool) {
        let mut details = json!({ "tabId": tab, "url": url, "frameId": 0, "parentFrameId": -1, "processId": -1, "timeStamp": now_ms() });
        if committed && name.ends_with("onCommitted") {
            details["transitionType"] = json!("link");
            details["transitionQualifiers"] = json!([]);
        }
        self.ext_broadcast(Some("webNavigation"), name, &|_| vec![details.clone()]);
    }

    /// Extension shortcut pressed while a page has the focus.
    fn ext_key(&mut self, shared: &Arc<Shared>, event: &signet_web::KeyEvent) {
        let Some(key) = crate::browser::commands::event_key(event) else { return };
        let ks = crate::browser::commands::key_string(event.cmd, event.ctrl, event.alt, event.shift, &key);
        let id = self.key_table.lock().ok().and_then(|t| t.get(&ks).cloned());
        let Some(rest) = id.as_deref().and_then(|i| i.strip_prefix("ext:")) else { return };
        self.ext_run_command(rest);
    }

    /// Runs an extension command given as `<extension id>:<command name>` (shortcut, menu).
    pub fn ext_run_command(&mut self, rest: &str) {
        let Some(shared) = self.ext_shared() else { return };
        let Some((ext, name)) = rest.split_once(':') else { return };
        let active = self.active_tab_id();
        let snap = snapshot(self, HashMap::new());
        let tab_json = snap.active.and_then(|n| snap.tab(n)).map(|t| t.to_chrome_tab(true));
        let fx = shared.host().run_command(ext, name, active.as_deref(), tab_json);
        self.ext_apply(fx);
    }

    // ------------------------------------------------------------------ calls

    fn ext_message(&mut self, shared: &Arc<Shared>, view: ViewId, frame_url: &str, frame_id: &str, is_main: bool, payload: &str) {
        let Some(msg) = parse_message(payload) else { return };
        let call_id = {
            let Some(rt) = self.phase3.extensions.rt.as_mut() else { return };
            if rt.replies.len() > MAX_PENDING_REPLIES {
                rt.replies.clear();
            }
            let id = rt.next_call;
            rt.next_call += 1;
            id
        };
        let reply_to = ReplyTo { view, frame: frame_id.to_string(), msg_id: msg.id.clone() };
        // Web Store pages: `chrome.webstorePrivate`.
        if is_store_url(frame_url) {
            self.ext_register_reply(call_id, reply_to);
            let args: Vec<Value> = msg.args.as_array().cloned().unwrap_or_default();
            let fx = shared.host().webstore_call(call_id, frame_url, &msg.method, &args);
            self.ext_apply(fx);
            return;
        }
        let Some((ext, context, mut inner)) = self.ext_sender(shared, view, frame_url, frame_id, &msg) else {
            self.log(format!("extensions: refused a message from {frame_url}"));
            return;
        };
        if inner.method == "runtime.__openContext" {
            self.ext_fill_open_context(&mut inner, view, frame_id, is_main);
        }
        self.ext_register_reply(call_id, ReplyTo { msg_id: inner.id.clone(), ..reply_to });
        let dirs = shared.loaded.read().map(|m| m.iter().map(|(k, v)| (k.clone(), v.dir.clone())).collect()).unwrap_or_default();
        let snap = snapshot(self, dirs);
        let call = Call { id: call_id, extension_id: ext, context, method: Value::String(inner.method), args: inner.args };
        let fx = shared.host().call(call, &snap);
        for a in snap.take_actions() {
            let HostAction::Activate(id) = a;
            self.activate_tab(&id);
        }
        self.ext_apply(fx);
    }

    fn ext_register_reply(&mut self, call_id: u64, to: ReplyTo) {
        if let Some(rt) = self.phase3.extensions.rt.as_mut() {
            rt.replies.insert(call_id, to);
        }
    }

    /// Who sent the message: the extension and the context come from the frame the browser saw, never from the payload.
    fn ext_sender(&self, shared: &Shared, view: ViewId, frame_url: &str, frame_id: &str, msg: &Msg) -> Option<(String, ApiContext, Msg)> {
        if let Some(ext) = extension_id_of(frame_url) {
            let ctxs = shared.ctxs();
            let c = ctxs.page_of(view, frame_id).filter(|c| c.ext == ext)?;
            let kind = if c.kind == Kind::Worker { ContextKind::Worker } else { ContextKind::Page };
            return Some((ext, ApiContext { kind, id: c.id.clone(), url: frame_url.to_string() }, msg.clone()));
        }
        // A content script in a web page: the envelope names the extension and the context id; both must match what
        // was registered for this very frame.
        let (ext, ctx_id, inner) = unwrap_content(msg)?;
        let ctxs = shared.ctxs();
        let c = ctxs.get(&ctx_id).filter(|c| c.kind == Kind::Content && c.ext == ext && c.view == view && c.frame == frame_id)?;
        Some((ext, ApiContext { kind: ContextKind::Page, id: c.id.clone(), url: frame_url.to_string() }, inner))
    }

    /// The page script could not know its tab (the view may not have been registered yet): fill it in.
    fn ext_fill_open_context(&self, msg: &mut Msg, view: ViewId, frame_id: &str, is_main: bool) {
        let Some(tab) = self.ext_tab_of(view) else { return };
        if let Some(info) = msg.args.get_mut(0).and_then(|a| a.as_object_mut()) {
            info.insert("tabId".into(), json!(tab));
            info.insert("frameId".into(), json!(frame_number(frame_id, is_main)));
        }
    }

    // ------------------------------------------------------------------ contexts

    /// Tells the host about contexts that appeared or went away, and feeds the workers that came up.
    pub(super) fn ext_after_contexts(&mut self, shared: &Arc<Shared>) {
        let (replaced, opened) = {
            let mut c = shared.ctxs();
            (c.take_replaced(), c.take_opened())
        };
        if replaced.is_empty() && opened.is_empty() {
            return;
        }
        let mut fx = Vec::new();
        {
            let mut host = shared.host();
            for id in replaced {
                fx.extend(host.context_closed(&id));
            }
            for c in opened.iter().filter(|c| c.kind == Kind::Worker) {
                fx.extend(host.worker_running(&c.ext, &c.id));
            }
        }
        for w in opened.iter().filter(|c| c.kind == Kind::Worker) {
            let tickets: Vec<u64> = match self.phase3.extensions.rt.as_mut() {
                Some(rt) => {
                    let (hit, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut rt.wake).into_iter().partition(|(e, _, _)| *e == w.ext);
                    rt.wake = rest;
                    hit.into_iter().map(|(_, t, _)| t).collect()
                }
                None => vec![],
            };
            for t in tickets {
                fx.extend(shared.host().wake_result(t, true));
            }
            let queued = self.phase3.extensions.rt.as_mut().and_then(|r| r.worker_events.remove(&w.ext)).unwrap_or_default();
            for (name, args) in queued {
                self.ext_run_in(w.view, &w.frame, &super::bridge::event_js(&name, &args));
            }
        }
        self.ext_apply(fx);
    }

    // ------------------------------------------------------------------ tick

    pub(super) fn ext_tick(&mut self) {
        let Some(shared) = self.ext_shared() else { return };
        self.ext_refresh_views(&shared);
        for item in shared.take_inbox() {
            let fx = match item {
                Inbox::Permission { ticket, allowed } => shared.host().permission_answer(ticket, allowed),
                Inbox::Install { ticket, allowed } => shared.host().install_decision(ticket, allowed),
                Inbox::StoreInstall { call_id, result } => shared.host().store_install_done(call_id, result),
                Inbox::RemoveConfirmed(id) => shared.host().remove_extension(&id),
                Inbox::Unpacked(Some(path)) => {
                    let r = shared.host().add_unpacked(&path);
                    match r {
                        Ok(fx) => fx,
                        Err(e) => {
                            let folder = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                            let title = signet_core::i18n::t("unpacked.pickTitle");
                            let text = format!("{}\n{e}", signet_core::i18n::t_args("unpacked.loadFailed", &[("folder", &folder)]));
                            std::thread::spawn(move || super::dialogs::alert(&title, &text));
                            vec![]
                        }
                    }
                }
                Inbox::Unpacked(None) => vec![],
            };
            self.ext_apply(fx);
        }
        self.ext_start_waiting(&shared);
        self.ext_after_contexts(&shared);
        self.ext_expire_wakes(&shared);
        self.ext_expire_relay();
        let fx = shared.host().tick();
        self.ext_apply(fx);
        self.ext_diff_tabs(&shared);
        self.ext_popup_tick();
    }

    fn ext_expire_wakes(&mut self, shared: &Arc<Shared>) {
        let now = now_ms();
        let late: Vec<u64> = match self.phase3.extensions.rt.as_mut() {
            Some(rt) => {
                let (late, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut rt.wake).into_iter().partition(|(_, _, deadline)| *deadline <= now);
                rt.wake = rest;
                late.into_iter().map(|(_, t, _)| t).collect()
            }
            None => vec![],
        };
        for t in late {
            let fx = shared.host().wake_result(t, false);
            self.ext_apply(fx);
        }
    }

    /// Finds what changed in the tabs since the last tick and tells the extensions.
    fn ext_diff_tabs(&mut self, shared: &Arc<Shared>) {
        let snap = snapshot(self, HashMap::new());
        let mut now: HashMap<String, TabSig> = HashMap::new();
        for t in &snap.tabs {
            if let Some(core_id) = snap.core_ids.get(&t.id) {
                now.insert(core_id.clone(), TabSig { url: t.url.clone(), title: t.title.clone(), favicon: t.fav_icon_url.clone(), loading: t.loading, audible: t.audible, muted: t.muted, pinned: t.pinned });
            }
        }
        let active = self.active_tab_id();
        let Some(rt) = self.phase3.extensions.rt.as_mut() else { return };
        let before = rt.tabs.replace(now.clone());
        let last_active = std::mem::replace(&mut rt.last_active, active.clone());
        let active_changed = last_active != active;
        let Some(before) = before else {
            for t in &snap.tabs {
                shared.host().tab_created(t.id, &t.url);
            }
            shared.notify();
            return;
        };
        let by_core = |core: &str| snap.tabs.iter().find(|t| snap.core_ids.get(&t.id).is_some_and(|c| c == core)).cloned();
        let removed: Vec<String> = before.keys().filter(|k| !now.contains_key(*k)).cloned().collect();
        for core in &removed {
            let number = tab_number(core);
            shared.host().tab_gone(number);
            self.ext_broadcast(None, "tabs.onRemoved", &|_| vec![json!(number), json!({ "windowId": 1, "isWindowClosing": false })]);
        }
        let mut seen: HashSet<&String> = HashSet::new();
        for (core, sig) in &now {
            seen.insert(core);
            let Some(tab) = by_core(core) else { continue };
            match before.get(core) {
                None => {
                    shared.host().tab_created(tab.id, &tab.url);
                    self.ext_broadcast(None, "tabs.onCreated", &|sensitive| vec![tab.to_chrome_tab(sensitive)]);
                }
                Some(prev) if prev != sig => {
                    let mut change = serde_json::Map::new();
                    if prev.loading != sig.loading {
                        change.insert("status".into(), json!(if sig.loading { "loading" } else { "complete" }));
                    }
                    if prev.audible != sig.audible {
                        change.insert("audible".into(), json!(sig.audible));
                    }
                    if prev.muted != sig.muted {
                        change.insert("mutedInfo".into(), json!({ "muted": sig.muted }));
                    }
                    if prev.pinned != sig.pinned {
                        change.insert("pinned".into(), json!(sig.pinned));
                    }
                    let sensitive_change = |c: &mut serde_json::Map<String, Value>| {
                        if prev.url != sig.url {
                            c.insert("url".into(), json!(sig.url));
                        }
                        if prev.title != sig.title {
                            c.insert("title".into(), json!(sig.title));
                        }
                        if prev.favicon != sig.favicon {
                            c.insert("favIconUrl".into(), json!(sig.favicon));
                        }
                    };
                    let mut full = change.clone();
                    sensitive_change(&mut full);
                    if full.is_empty() {
                        continue;
                    }
                    let (id, plain) = (tab.id, change);
                    self.ext_broadcast(None, "tabs.onUpdated", &|sensitive| vec![json!(id), if sensitive { Value::Object(full.clone()) } else { Value::Object(plain.clone()) }, tab.to_chrome_tab(sensitive)]);
                }
                Some(_) => {}
            }
        }
        if active_changed {
            if let Some(n) = snap.active {
                self.ext_broadcast(None, "tabs.onActivated", &|_| vec![json!({ "tabId": n, "windowId": 1 })]);
            }
            shared.notify();
        }
    }
}

//! Carries out the `Effect`s of the `ExtensionHost`.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{json, Value};
use signet_core::api::UiEvent;
use signet_core::ids::now_ms;
use signet_core::model::OpenOptions;
use signet_extensions::common::CrxResult;
use signet_extensions::host::Effect;
use signet_extensions::manifest::{BackgroundEntry, Manifest};
use signet_extensions::notifications::NotifEffect;
use signet_extensions::permissions::PermissionLine;
use signet_extensions::router::RouterAction;
use signet_extensions::settings::Apply;
use signet_web::{FrameTarget, ProfileKind, ViewId, WebView};

use super::bridge::{background_url, ctx_js, event_js, hub_js, reply_js};
use super::ctx::{Ctx, Kind};
use super::dialogs;
use super::files::Loaded;
use super::shared::{Inbox, Shared};
use super::tabhost::{snapshot, HostAction};
use crate::browser::Core;

fn tr(key: &str) -> String {
    signet_core::i18n::t(key)
}

fn tr_args(key: &str, args: &[(&str, &str)]) -> String {
    signet_core::i18n::t_args(key, args)
}

/// The lines of a permission dialog as text, one per line.
pub fn describe_lines(lines: &[PermissionLine], t: &dyn Fn(&str, &[(&str, &str)]) -> String) -> String {
    lines
        .iter()
        .map(|l| {
            let text = match l {
                PermissionLine::AllSites => t("dialog.extension.perm.allSites", &[]),
                PermissionLine::Hosts { hosts, more } => {
                    let list = hosts.join(", ");
                    if *more > 0 { t("dialog.extension.perm.hostsMore", &[("hosts", &list), ("count", &more.to_string())]) } else { t("dialog.extension.perm.hosts", &[("hosts", &list)]) }
                }
                PermissionLine::Known(k) => t(&format!("dialog.extension.perm.{k}"), &[]),
                PermissionLine::Unknown(n) => n.clone(),
            };
            format!("- {text}")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn describe(lines: &[PermissionLine]) -> String {
    describe_lines(lines, &|k, a| if a.is_empty() { tr(k) } else { tr_args(k, a) })
}

impl Core {
    /// Offers a proxy authentication challenge to the extensions that control the proxy. Returns the request when
    /// nobody takes it (the caller drops it, which cancels the challenge).
    pub fn ext_proxy_auth(&mut self, req: signet_web::AuthRequest) -> Option<signet_web::AuthRequest> {
        let Some(shared) = self.ext_shared() else { return Some(req) };
        let challenge = signet_extensions::auth::ProxyChallenge {
            url: req.origin.clone(),
            host: req.host.clone(),
            port: req.port.clamp(0, 65535) as u16,
            scheme: req.scheme.clone(),
            realm: req.realm.clone(),
        };
        let (handled, fx) = shared.host().proxy_auth_challenge(&challenge);
        use signet_extensions::auth::{AuthOutcome, Handled};
        let left = match handled {
            Handled::Skip => Some(req),
            Handled::Done(AuthOutcome::Credentials { username, password }) => {
                req.submit(username, password);
                None
            }
            Handled::Done(_) => {
                req.cancel();
                None
            }
            Handled::Pending(key) => {
                self.phase3.extensions.proxy_auth.insert(key, req);
                None
            }
        };
        self.ext_apply(fx);
        left
    }

    pub(super) fn ext_shared(&self) -> Option<Arc<Shared>> {
        self.phase3.extensions.rt.as_ref().map(|r| r.shared.clone())
    }

    /// Runs `js` in a frame of a view (nothing if the view is gone).
    pub(super) fn ext_run_in(&self, view: ViewId, frame: &str, js: &str) {
        if let Some(v) = WebView::from_id(view) {
            v.execute_js_in(&FrameTarget::Id(frame.to_string()), js);
        }
    }

    /// Carries out effects, and the effects they produce.
    pub(super) fn ext_apply(&mut self, fx: Vec<Effect>) {
        let mut queue: VecDeque<Effect> = fx.into();
        while let Some(e) = queue.pop_front() {
            let more = self.ext_effect(e);
            queue.extend(more);
        }
    }

    fn ext_effect(&mut self, e: Effect) -> Vec<Effect> {
        let Some(shared) = self.ext_shared() else { return vec![] };
        match e {
            Effect::Reply { call_id, result } => self.ext_reply(call_id, &result),
            Effect::Router(a) => return self.ext_router(a),
            Effect::ToContext { context_id, name, args } => {
                let ctx = shared.ctxs().get(&context_id).cloned();
                if let Some(c) = ctx {
                    let js = if c.kind == Kind::Content { hub_js(&c.ext, &name, &args) } else { ctx_js(&name, &args) };
                    self.ext_run_in(c.view, &c.frame, &js);
                }
            }
            Effect::LoadExtension { id, dir, manifest, background } => self.ext_load(&shared, id, dir, manifest, background),
            Effect::UnloadExtension { id } => self.ext_unload(&shared, &id),
            Effect::ReloadExtension { id, .. } => self.ext_close_background(&shared, &id),
            Effect::PermissionPrompt { ticket, name, lines, .. } => {
                let (title, detail) = (tr_args("dialog.extension.request.message", &[("name", &name)]), tr_args("dialog.extension.request.detail", &[("permissions", &describe(&lines))]));
                let (ok, cancel) = (tr("dialog.extension.request.allow"), tr("dialog.extension.request.deny"));
                let s = shared.clone();
                std::thread::spawn(move || {
                    let allowed = dialogs::confirm("Signet", &format!("{title}\n\n{detail}"), &ok, &cancel);
                    s.push(Inbox::Permission { ticket, allowed });
                });
            }
            Effect::InstallPrompt { ticket, name, lines, .. } => {
                let title = tr_args("dialog.extension.message", &[("name", &name)]);
                let detail = if lines.is_empty() { tr("dialog.extension.detailNone") } else { tr_args("dialog.extension.detail", &[("permissions", &describe(&lines))]) };
                let (ok, cancel) = (tr("dialog.extension.add"), tr("dialog.cancel"));
                let s = shared.clone();
                std::thread::spawn(move || {
                    let allowed = dialogs::confirm("Signet", &format!("{title}\n\n{detail}"), &ok, &cancel);
                    s.push(Inbox::Install { ticket, allowed });
                });
            }
            Effect::InstallFromStore { call_id, extension_id } => self.ext_store_install(&shared, call_id, extension_id),
            Effect::Notification(n) => {
                if let NotifEffect::Show { spec, .. } = n {
                    std::thread::spawn(move || dialogs::notify(&spec.title, &spec.body));
                }
            }
            Effect::ApplySetting(a) => self.ext_apply_setting(&a),
            Effect::SetActionIcon(_) => shared.notify(),
            Effect::ActionStates(v) => {
                if let Ok(states) = serde_json::from_value(v) {
                    if let Some(rt) = self.phase3.extensions.rt.as_mut() {
                        rt.states = states;
                    }
                    let s = self.phase3.extensions.rt.as_ref().map(|r| r.states.clone()).unwrap_or_default();
                    self.emit(UiEvent::ExtensionActionStates(s));
                }
                shared.notify();
            }
            Effect::ActionCounts(c) => {
                let counts = counts_to_ui(&c);
                self.emit(UiEvent::ExtensionActionCounts(counts));
                shared.notify();
            }
            Effect::NetworkHooks(events) => {
                shared.dnr_on.store(events.contains(&signet_extensions::dnr_engine::DnrEvent::OnBeforeRequest), std::sync::atomic::Ordering::Relaxed);
            }
            Effect::CommandsChanged => self.ext_rebuild_command_keys(&shared),
            Effect::OffscreenOpen { extension_id, url } => self.ext_offscreen_open(&shared, extension_id, url),
            Effect::OffscreenClose { page_id } => {
                let view = ViewId(page_id as u64);
                if let Some(v) = WebView::from_id(view) {
                    v.close();
                }
                if let Some(rt) = self.phase3.extensions.rt.as_mut() {
                    rt.offscreen.remove(&view);
                }
                shared.host().offscreen_page_closed(page_id);
            }
            Effect::TabAction { call_id, extension_id, method, args } => {
                let result = self.ext_tab_action(&shared, &extension_id, &method, &args);
                return shared.host().tab_action_done(call_id, result);
            }
            Effect::OpenOptions { extension_id } => self.ext_open_options(&shared, &extension_id),
            Effect::RunAction { extension_id, popup_url } => self.ext_run_action(&extension_id, popup_url),
            Effect::ExtensionsChanged => {
                self.ext_rebuild_command_keys(&shared);
                shared.notify();
                self.dirty.needs_full = true;
            }
            Effect::UnpackedPathsChanged(list) => {
                self.store.data.state.settings.unpacked_extensions = Some(list);
                self.changed();
            }
            Effect::PinnedChanged(list) => {
                self.store.data.state.settings.pinned_extensions = Some(list);
                self.changed();
                shared.notify();
            }
            Effect::OrderChanged(list) => {
                self.store.data.state.settings.extension_order = Some(list);
                self.changed();
                shared.notify();
            }
            Effect::ProxyAuth { key, outcome } => {
                if let Some(req) = self.phase3.extensions.proxy_auth.remove(&key) {
                    match outcome {
                        signet_extensions::auth::AuthOutcome::Credentials { username, password } => req.submit(username, password),
                        _ => req.cancel(),
                    }
                }
            }
            Effect::OpenUrl { url } => {
                self.open_tab(&url, OpenOptions { activate: true, after_tab_id: None, opener_id: None });
            }
            Effect::Log(s) => self.log(s),
        }
        vec![]
    }

    // ------------------------------------------------------------------ replies and events

    pub(super) fn ext_reply(&mut self, call_id: u64, result: &CrxResult) {
        let Some(to) = self.phase3.extensions.rt.as_mut().and_then(|r| r.replies.remove(&call_id)) else { return };
        self.ext_run_in(to.view, &to.frame, &reply_js(&to.msg_id, result));
    }

    fn ext_router(&mut self, a: RouterAction) -> Vec<Effect> {
        let Some(shared) = self.ext_shared() else { return vec![] };
        match a {
            RouterAction::DeliverRunning { extension_id, name, args } => {
                let running: Vec<Ctx> = shared.ctxs().running(&extension_id);
                let js = event_js(&name, &args);
                for c in running {
                    self.ext_run_in(c.view, &c.frame, &js);
                }
                vec![]
            }
            RouterAction::StartWorker { extension_id, ticket } => {
                if shared.ctxs().worker_of(&extension_id).is_some() {
                    return shared.host().wake_result(ticket, true);
                }
                if self.ext_start_background(&shared, &extension_id) {
                    if let Some(rt) = self.phase3.extensions.rt.as_mut() {
                        rt.wake.push((extension_id, ticket, now_ms() + 10_000));
                    }
                    vec![]
                } else {
                    shared.host().wake_result(ticket, false)
                }
            }
            RouterAction::DeliverToWorker { extension_id, events } => {
                let worker = shared.ctxs().worker_of(&extension_id);
                match worker {
                    Some(w) => {
                        for (name, args) in events {
                            self.ext_run_in(w.view, &w.frame, &event_js(&name, &args));
                        }
                    }
                    None => {
                        if let Some(rt) = self.phase3.extensions.rt.as_mut() {
                            rt.worker_events.entry(extension_id).or_default().extend(events);
                        }
                    }
                }
                vec![]
            }
        }
    }

    /// Sends an event to every running context of every extension that holds `permission` (or to all with `None`).
    pub(super) fn ext_broadcast(&mut self, permission: Option<&str>, name: &str, make_args: &dyn Fn(bool) -> Vec<Value>) {
        let Some(shared) = self.ext_shared() else { return };
        let ids: Vec<(String, bool, bool)> = shared
            .loaded
            .read()
            .map(|m| {
                m.iter()
                    .map(|(id, l)| {
                        let has = |p: &str| l.manifest.permissions.iter().any(|x| x == p);
                        (id.clone(), permission.is_none_or(has), has("tabs") || l.manifest.host_permissions.iter().any(|h| h == "<all_urls>" || h == "*://*/*"))
                    })
                    .collect()
            })
            .unwrap_or_default();
        for (id, allowed, sensitive) in ids {
            if !allowed {
                continue;
            }
            let js = event_js(name, &make_args(sensitive));
            for c in shared.ctxs().running(&id) {
                self.ext_run_in(c.view, &c.frame, &js);
            }
        }
    }

    // ------------------------------------------------------------------ load, unload, background

    fn ext_load(&mut self, shared: &Arc<Shared>, id: String, dir: PathBuf, manifest: Value, background: BackgroundEntry) {
        let Ok(m) = Manifest::from_value(manifest) else {
            self.log(format!("extensions: manifest of {id} is not valid"));
            return;
        };
        self.ext_close_background(shared, &id);
        if let Ok(mut l) = shared.loaded.write() {
            l.insert(id.clone(), Loaded { dir, manifest: m, background: background.clone() });
        }
        if background != BackgroundEntry::None
            && let Some(rt) = self.phase3.extensions.rt.as_mut()
            && !rt.waiting.contains(&id)
        {
            rt.waiting.push(id.clone());
        }
        self.ext_start_waiting(shared);
        shared.notify();
    }

    fn ext_unload(&mut self, shared: &Arc<Shared>, id: &str) {
        self.ext_close_background(shared, id);
        if let Ok(mut l) = shared.loaded.write() {
            l.remove(id);
        }
        if let Some(rt) = self.phase3.extensions.rt.as_mut() {
            rt.waiting.retain(|w| w != id);
            rt.worker_events.remove(id);
            rt.wake.retain(|(e, _, _)| e != id);
        }
        let closed = shared.ctxs().remove_ext(id);
        let mut host = shared.host();
        for c in closed {
            host.context_closed(&c);
        }
        host.worker_stopped(id, "");
        drop(host);
        shared.notify();
    }

    fn ext_close_background(&mut self, shared: &Arc<Shared>, id: &str) {
        let view = self.phase3.extensions.rt.as_mut().and_then(|r| r.bg.remove(id));
        if let Some(v) = view {
            let closed = shared.ctxs().remove_view(v.id());
            let mut host = shared.host();
            for c in closed {
                host.context_closed(&c);
            }
            host.worker_stopped(id, "");
            drop(host);
            v.close();
        }
    }

    /// Starts the backgrounds that waited for the main window.
    pub(super) fn ext_start_waiting(&mut self, shared: &Arc<Shared>) {
        let waiting: Vec<String> = self.phase3.extensions.rt.as_ref().map(|r| r.waiting.clone()).unwrap_or_default();
        for id in waiting {
            if self.ext_create_background(shared, &id) {
                if let Some(rt) = self.phase3.extensions.rt.as_mut() {
                    rt.waiting.retain(|w| *w != id);
                }
            }
        }
    }

    /// Makes sure a background is running or will run. False when the extension has none.
    pub(super) fn ext_start_background(&mut self, shared: &Arc<Shared>, id: &str) -> bool {
        let has = shared.loaded.read().ok().and_then(|m| m.get(id).map(|l| l.background != BackgroundEntry::None)).unwrap_or(false);
        if !has {
            return false;
        }
        let up = self.phase3.extensions.rt.as_ref().is_some_and(|r| r.bg.contains_key(id));
        if !up
            && let Some(rt) = self.phase3.extensions.rt.as_mut()
            && !rt.waiting.contains(&id.to_string())
        {
            rt.waiting.push(id.to_string());
        }
        self.ext_start_waiting(shared);
        true
    }

    fn ext_create_background(&mut self, shared: &Arc<Shared>, id: &str) -> bool {
        if self.phase3.extensions.rt.as_ref().is_some_and(|r| r.bg.contains_key(id)) {
            return true;
        }
        let Some(parent) = self.views.parent else { return false };
        let Some(url) = shared.loaded.read().ok().and_then(|m| m.get(id).and_then(|l| background_url(id, &l.background))) else { return true };
        match WebView::create_background(parent, ProfileKind::persistent("default"), &url) {
            Ok(v) => {
                self.log(format!("extensions: background of {id} started at {url}"));
                if let Some(rt) = self.phase3.extensions.rt.as_mut() {
                    rt.bg.insert(id.to_string(), v);
                }
                true
            }
            Err(e) => {
                self.log(format!("extensions: cannot start the background of {id}: {e}"));
                false
            }
        }
    }

    fn ext_offscreen_open(&mut self, shared: &Arc<Shared>, ext: String, url: String) {
        let Some(parent) = self.views.parent else {
            let fx = shared.host().offscreen_failed(&ext, "The window is not ready.");
            self.ext_apply(fx);
            return;
        };
        match WebView::create_background(parent, ProfileKind::persistent("default"), &url) {
            Ok(v) => {
                if let Some(rt) = self.phase3.extensions.rt.as_mut() {
                    rt.offscreen.insert(v.id(), ext);
                }
            }
            Err(e) => {
                let fx = shared.host().offscreen_failed(&ext, &e.to_string());
                self.ext_apply(fx);
            }
        }
    }

    // ------------------------------------------------------------------ settings, commands, notifications

    fn ext_apply_setting(&mut self, a: &Apply) {
        match a.key.as_str() {
            "proxy.settings" => {
                let pref = signet_extensions::proxyconfig::to_engine_proxy(&a.value).to_cef_preference();
                let n = signet_web::set_preference_json("proxy", &pref);
                self.log(format!("extensions: proxy preference set on {n} profile(s)"));
            }
            "privacy.network.webRTCIPHandlingPolicy" => {
                if let Some(policy) = a.value.as_str() {
                    signet_web::set_preference_json("webrtc.ip_handling_policy", &json!(policy));
                }
            }
            other => self.log(format!("extensions: setting {other} has no engine counterpart")),
        }
    }

    /// Installed extensions as `(id, name)`.
    pub fn installed_extension_list(&self) -> Vec<(String, String)> {
        let Some(shared) = self.ext_shared() else { return Vec::new() };
        shared.host().infos().into_iter().map(|i| (i.id, i.name)).collect()
    }

    /// Installs an extension of the Chrome Web Store in the background unless it is already there (password
    /// manager extensions, extensions found by an import or left by a backup restore).
    pub fn install_store_extension(&mut self, ext_id: &str, name: &str) {
        let Some(shared) = self.ext_shared() else { return };
        if self.installed_extension_list().iter().any(|(id, _)| id == ext_id) {
            return;
        }
        self.log(format!("extensions: installing {name} ({ext_id}) from the store"));
        // Call id 0 belongs to no page request: the answer is dropped.
        self.ext_store_install(&shared, 0, ext_id.to_string());
    }

    /// Installs the extensions an import or a restore asked for, once.
    pub(super) fn ext_install_requested(&mut self) {
        let mut ids = self.take_import_extensions();
        ids.extend(self.take_backup_extensions());
        for id in ids {
            self.install_store_extension(&id, &id);
        }
    }

    /// Shortcuts of the extensions' commands as `(accelerator, command id)` for the menu bar (`ext:<id>:<name>`).
    pub fn ext_command_accels(&self) -> Vec<(String, String)> {
        let Some(shared) = self.ext_shared() else { return Vec::new() };
        let bindings = shared.host().command_bindings();
        bindings.into_iter().map(|b| (b.accel, format!("ext:{}:{}", b.extension_id, b.name))).collect()
    }

    /// Extension shortcuts join the key table the key filter reads (while a page has focus).
    pub(super) fn ext_rebuild_command_keys(&mut self, shared: &Arc<Shared>) {
        let bindings = shared.host().command_bindings();
        self.send(crate::windows::Cmd::RebuildMenu);
        let Ok(mut table) = self.key_table.lock() else { return };
        if let Some(rt) = self.phase3.extensions.rt.as_mut() {
            for k in rt.command_keys.drain(..) {
                if table.get(&k).is_some_and(|v| v.starts_with("ext:")) {
                    table.remove(&k);
                }
            }
            for b in bindings {
                let Some(p) = signet_core::shortcuts::parse_accel(&b.accel, crate::app::platform::IS_MAC) else { continue };
                let key = crate::browser::commands::key_string(p.cmd, p.ctrl, p.alt, p.shift, &p.key);
                if table.contains_key(&key) {
                    continue;
                }
                table.insert(key.clone(), format!("ext:{}:{}", b.extension_id, b.name));
                rt.command_keys.push(key);
            }
        }
    }

    // ------------------------------------------------------------------ store

    fn ext_store_install(&mut self, shared: &Arc<Shared>, call_id: u64, id: String) {
        let s = shared.clone();
        std::thread::spawn(move || {
            let result = (|| {
                let fetcher = super::api::HttpFetcher;
                let url = signet_extensions::crx::crx_download_url(&id, signet_extensions::crx::DEFAULT_PRODVERSION);
                let bytes = signet_extensions::crx::Fetcher::get(&fetcher, &url)?;
                s.host().install_crx_bytes(&bytes, Some(&id)).map_err(|e| e.to_string())
            })();
            s.push(Inbox::StoreInstall { call_id, result });
        });
    }

    // ------------------------------------------------------------------ tabs

    /// `tabs.*` calls that need the browser.
    fn ext_tab_action(&mut self, shared: &Arc<Shared>, ext: &str, method: &str, args: &[Value]) -> Result<Value, String> {
        let dirs = shared.loaded.read().map(|m| m.iter().map(|(k, v)| (k.clone(), v.dir.clone())).collect()).unwrap_or_default();
        let snap = snapshot(self, dirs);
        let arg = |i: usize| args.get(i).cloned().unwrap_or(Value::Null);
        // The tab a call targets: the first argument when it is a number, else the active tab.
        let target = |a: &Value| -> Option<String> { a.as_i64().and_then(|n| snap.core_ids.get(&n).cloned()).or_else(|| snap.active.and_then(|n| snap.core_ids.get(&n).cloned())) };
        let tab_json = |core_id: &str, snap: &super::tabhost::SnapHost| snap.tabs.iter().find(|t| snap.core_ids.get(&t.id).is_some_and(|c| c == core_id)).map(|t| t.to_chrome_tab(true)).unwrap_or(Value::Null);
        match method {
            "tabs.create" => {
                let p = arg(0);
                let url = p.get("url").and_then(|u| u.as_str()).unwrap_or("about:blank").to_string();
                let url = if url.contains("://") || url.starts_with("about:") || url.starts_with("data:") { url } else { format!("chrome-extension://{ext}/{}", url.trim_start_matches('/')) };
                let activate = p.get("active").and_then(|a| a.as_bool()).unwrap_or(true);
                let id = self.open_tab(&url, OpenOptions { activate, after_tab_id: None, opener_id: None });
                if p.get("pinned").and_then(|a| a.as_bool()) == Some(true) {
                    self.pin_toggle(&id);
                }
                let after = snapshot(self, std::collections::HashMap::new());
                Ok(tab_json(&id, &after))
            }
            "tabs.update" => {
                let (first, props) = if arg(0).is_object() { (Value::Null, arg(0)) } else { (arg(0), arg(1)) };
                let id = target(&first).ok_or_else(|| "No tab with the given id.".to_string())?;
                if let Some(url) = props.get("url").and_then(|u| u.as_str()) {
                    self.load_url(&id, url, false);
                }
                if let Some(muted) = props.get("muted").and_then(|m| m.as_bool())
                    && self.state().tabs.get(&id).is_some_and(|t| t.muted != muted)
                {
                    self.toggle_mute(&id);
                }
                if let Some(pinned) = props.get("pinned").and_then(|m| m.as_bool())
                    && snap.tabs.iter().any(|t| snap.core_ids.get(&t.id) == Some(&id) && t.pinned != pinned)
                {
                    self.pin_toggle(&id);
                }
                if props.get("active").and_then(|a| a.as_bool()) == Some(true) {
                    self.activate_tab(&id);
                }
                let after = snapshot(self, std::collections::HashMap::new());
                Ok(tab_json(&id, &after))
            }
            "tabs.remove" => {
                let ids: Vec<Value> = match arg(0) {
                    Value::Array(a) => a,
                    v => vec![v],
                };
                for v in ids {
                    if let Some(id) = v.as_i64().and_then(|n| snap.core_ids.get(&n).cloned()) {
                        self.close_tab(&id);
                    }
                }
                Ok(Value::Null)
            }
            "tabs.reload" => {
                let id = target(&arg(0)).ok_or_else(|| "No tab with the given id.".to_string())?;
                let bypass = arg(1).get("bypassCache").and_then(|b| b.as_bool()).unwrap_or(false) || arg(0).get("bypassCache").and_then(|b| b.as_bool()).unwrap_or(false);
                self.reload_tab(Some(&id), bypass);
                Ok(Value::Null)
            }
            "tabs.duplicate" => {
                let id = target(&arg(0)).ok_or_else(|| "No tab with the given id.".to_string())?;
                self.duplicate_tab(&id);
                Ok(Value::Null)
            }
            "tabs.goBack" | "tabs.goForward" => {
                let id = target(&arg(0)).ok_or_else(|| "No tab with the given id.".to_string())?;
                if method == "tabs.goBack" { self.go_back(Some(&id)) } else { self.go_forward(Some(&id)) }
                Ok(Value::Null)
            }
            "tabs.executeScript" => {
                let (first, details) = if arg(0).is_object() { (Value::Null, arg(0)) } else { (arg(0), arg(1)) };
                let id = target(&first).ok_or_else(|| "No tab with the given id.".to_string())?;
                let code = match (details.get("code").and_then(|c| c.as_str()), details.get("file").and_then(|f| f.as_str())) {
                    (Some(c), _) => c.to_string(),
                    (None, Some(f)) => snap.read_extension_file_of(shared, ext, f)?,
                    _ => return Err("Either code or file must be specified.".into()),
                };
                let view = self.views.view_of(&id).ok_or_else(|| "The tab is not loaded.".to_string())?;
                view.execute_js(&code);
                Ok(json!([Value::Null]))
            }
            other => Err(format!("{other} is not supported in Signet yet.")),
        }
        .inspect(|_| {
            for a in snap.take_actions() {
                let HostAction::Activate(id) = a;
                self.activate_tab(&id);
            }
        })
    }

    fn ext_open_options(&mut self, shared: &Arc<Shared>, ext: &str) {
        let page = shared.loaded.read().ok().and_then(|m| m.get(ext).and_then(|l| l.manifest.options_page.clone()));
        if let Some(p) = page {
            self.open_tab(&format!("chrome-extension://{ext}/{}", p.trim_start_matches('/')), OpenOptions { activate: true, after_tab_id: None, opener_id: None });
        }
    }
}

impl super::tabhost::SnapHost {
    /// Text of a file of an extension (the snapshot only knows the folders it was given).
    pub fn read_extension_file_of(&self, shared: &Shared, ext: &str, file: &str) -> Result<String, String> {
        let dir = shared.loaded.read().ok().and_then(|m| m.get(ext).map(|l| l.dir.clone())).ok_or("Unknown extension.")?;
        let path = signet_extensions::urlmatch::extension_file_path(&dir, ext, file).ok_or("File outside the extension.")?;
        std::fs::read_to_string(path).map_err(|e| e.to_string())
    }
}

/// Counts in the shape of `SignetApi` (tab number, then extension id).
pub fn counts_to_ui(c: &signet_extensions::dnr_engine::ActionCounts) -> signet_core::types::ExtensionActionCounts {
    c.iter().map(|(tab, per)| (tab.to_string(), per.iter().map(|(e, n)| (e.clone(), *n)).collect())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_lines_become_text() {
        let t = |k: &str, a: &[(&str, &str)]| format!("{k}{}", a.iter().map(|(n, v)| format!("[{n}={v}]")).collect::<String>());
        let lines = vec![PermissionLine::AllSites, PermissionLine::Hosts { hosts: vec!["a.com".into(), "b.com".into()], more: 2 }, PermissionLine::Known("tabs"), PermissionLine::Unknown("odd".into())];
        let text = describe_lines(&lines, &t);
        assert_eq!(
            text,
            "- dialog.extension.perm.allSites\n- dialog.extension.perm.hostsMore[hosts=a.com, b.com][count=2]\n- dialog.extension.perm.tabs\n- odd"
        );
    }

    #[test]
    fn counts_use_text_ids() {
        let mut c = signet_extensions::dnr_engine::ActionCounts::new();
        c.entry(7).or_default().insert("ext".into(), 3);
        let ui = counts_to_ui(&c);
        assert_eq!(ui.get("7").and_then(|m| m.get("ext")), Some(&3));
    }
}

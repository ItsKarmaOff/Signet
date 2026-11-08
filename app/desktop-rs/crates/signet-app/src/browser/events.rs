//! CEF events to the browser state (port of the listeners of `browser/tabs.ts` `createTabView`).

use serde_json::json;
use signet_core::model::OpenOptions;
use signet_features::internal;
use signet_web::{Disposition, WebEvent};

use super::commands::{event_key, key_string};
use super::layout::DOC_BASE;
use super::Core;

impl Core {
    pub fn on_web_event(&mut self, ev: WebEvent) {
        if self.trace.borrow().is_some()
            && matches!(
                ev,
                WebEvent::LoadEnd { .. }
                    | WebEvent::LoadStart { .. }
                    | WebEvent::LoadError { .. }
                    | WebEvent::PageMessage { .. }
                    | WebEvent::RequestBlocked { .. }
                    | WebEvent::Console { .. }
                    | WebEvent::RenderProcessGone { .. }
                    | WebEvent::MainFrameResponse { .. }
                    | WebEvent::NewWindowRequested { .. }
                    | WebEvent::Created { .. }
            )
        {
            self.note(format!("{ev:?}"));
        }
        self.phase3_web_event(&ev);
        let ev = match ev {
            // Proxy challenges go to the extensions that control the proxy; anything else is cancelled.
            WebEvent::AuthRequest { request, .. } => {
                if request.is_proxy {
                    drop(self.ext_proxy_auth(request));
                }
                return;
            }
            ev => ev,
        };
        let view = ev.view();
        let Some(tab) = self.views.tab_of(view).map(str::to_string) else {
            // Not a tab (devtools, overlay helpers): only downloads and permissions are answered.
            match ev {
                WebEvent::DownloadStarting { request, .. } => self.on_download_starting(request),
                WebEvent::DownloadProgress { progress, .. } => self.on_download_progress(progress),
                _ => {}
            }
            return;
        };
        match ev {
            WebEvent::TitleChanged { title, .. } => {
                // The HTTPS-only "Continue" page carries its token in the title: never show it.
                if title.starts_with("signet-https-continue:") {
                    return;
                }
                let title = internal::display_url(DOC_BASE, &title);
                self.patch_one(&tab, "title", json!(title), true, true);
                if let Some(url) = self.state().tabs.get(&tab).map(|t| t.url.clone()) {
                    self.history.update(&url, Some(&title), None);
                }
                if self.peek_tab() == Some(tab.as_str()) {
                    self.peek_refresh();
                }
            }
            WebEvent::AddressChanged { url, is_main_frame, .. } => {
                if !is_main_frame {
                    return;
                }
                let shown = internal::display_url(DOC_BASE, &url);
                if shown.starts_with("data:") || shown == "about:blank" && self.state().tabs.get(&tab).is_some_and(|t| !t.url.is_empty()) {
                    return;
                }
                let (back, fwd) = self.views.view_of(&tab).map(|v| (v.can_go_back(), v.can_go_forward())).unwrap_or((false, false));
                let mut m = serde_json::Map::new();
                m.insert("url".into(), json!(shown));
                m.insert("canGoBack".into(), json!(back));
                m.insert("canGoForward".into(), json!(fwd));
                self.patch_tab(&tab, m, true, false);
                let typed = self.typed.remove(&tab).is_some();
                if let Some(t) = self.state().tabs.get(&tab).cloned()
                    && !shown.starts_with("signet://")
                    && !self.keep_out_of_history(&shown)
                {
                    self.history.record(&shown, &t.title, t.favicon.as_deref(), typed);
                }
                if self.peek_tab() == Some(tab.as_str()) {
                    self.peek_refresh();
                }
            }
            WebEvent::FaviconUrls { urls, .. } => {
                if let Some(first) = urls.first() {
                    self.patch_one(&tab, "favicon", json!(first), true, true);
                    if let Some(url) = self.state().tabs.get(&tab).map(|t| t.url.clone()) {
                        self.history.update(&url, None, Some(first));
                    }
                }
            }
            WebEvent::LoadingState { is_loading, can_go_back, can_go_forward, .. } => {
                let mut m = serde_json::Map::new();
                m.insert("loading".into(), json!(is_loading));
                m.insert("canGoBack".into(), json!(can_go_back));
                m.insert("canGoForward".into(), json!(can_go_forward));
                self.patch_tab(&tab, m, false, false);
            }
            WebEvent::LoadStart { is_main_frame: true, .. } => self.patch_one(&tab, "loading", json!(true), false, false),
            WebEvent::LoadEnd { is_main_frame: true, http_status, url, .. } => {
                self.upgraded.remove(&tab);
                self.log(format!("load end {tab} {http_status} {url}"));
                self.patch_one(&tab, "loading", json!(false), false, false);
            }
            WebEvent::LoadError { code, url, is_main_frame, text, .. } => {
                if !is_main_frame || code == -3 {
                    return;
                }
                self.log(format!("load error {tab} {code} {text} {url}"));
                if let Some(pending) = self.upgraded.remove(&tab)
                    && signet_core::urls::is_https_fallback_error(code)
                    && let Some(http) = signet_core::urls::http_fallback(&pending, &url)
                {
                    self.load_url(&tab, &http, false);
                    return;
                }
                self.patch_one(&tab, "loading", json!(false), false, false);
            }
            WebEvent::NewWindowRequested { url, disposition, .. } => self.on_new_window(&tab, &url, disposition),
            WebEvent::FullscreenChanged { fullscreen, .. } => {
                if fullscreen && self.shown_tabs.contains(&tab) {
                    self.fullscreen = Some(tab);
                } else if self.fullscreen.as_deref() == Some(tab.as_str()) {
                    self.fullscreen = None;
                }
                self.send(crate::windows::Cmd::PageFullscreen(self.fullscreen.is_some()));
                self.changed_view();
            }
            WebEvent::AudioPlayingChanged { playing, .. } => self.patch_one(&tab, "audible", json!(playing), false, false),
            WebEvent::RenderProcessGone { status, text, .. } => {
                self.log(format!("render process gone {tab} status={status} {text}"));
                self.patch_one(&tab, "loading", json!(false), false, false);
                let url = self.state().tabs.get(&tab).map(|t| t.url.clone()).unwrap_or_default();
                let lower = url.to_lowercase();
                if (lower.starts_with("http:") || lower.starts_with("https:") || lower.starts_with("file:"))
                    && let Some(v) = self.views.view_of(&tab)
                {
                    let tr = |k: &str, a: &[(&str, String)]| {
                        let args: Vec<(&str, &str)> = a.iter().map(|(k, v)| (*k, v.as_str())).collect();
                        signet_core::i18n::t_args(k, &args)
                    };
                    let doc = signet_features::errorpage::crash_document(&tr, &url);
                    v.load_url(&format!("data:text/html;charset=utf-8,{}", percent_encode(&doc)));
                }
            }
            WebEvent::Console { level, message, .. } => {
                if self.debug && matches!(level, signet_web::ConsoleLevel::Error) {
                    self.log(format!("console error {tab}: {message}"));
                }
            }
            WebEvent::PreKey { event, handled, .. } => {
                // Escape closes the Peek.
                if event.pressed && event.windows_key_code == 27 && self.peek_tab() == Some(tab.as_str()) {
                    self.close_overlay(false);
                    return;
                }
                if !handled {
                    return;
                }
                let Some(key) = event_key(&event) else { return };
                let ks = key_string(event.cmd, event.ctrl, event.alt, event.shift, &key);
                let id = self.key_table.lock().ok().and_then(|t| t.get(&ks).cloned());
                // Extension shortcuts already ran in the extensions' own web_event hook.
                if let Some(id) = id.filter(|i| !i.starts_with("ext:")) {
                    self.log(format!("page key {ks} -> {id}"));
                    self.run_command(&id);
                }
            }
            WebEvent::ContextMenu { info, .. } => self.show_page_context_menu(&tab, info),
            WebEvent::StatusText { url, .. } => {
                if self.status_url != url {
                    self.status_url = url;
                }
            }
            WebEvent::FindResult { count, active_ordinal, final_update, .. } => self.on_find_result(&tab, count, active_ordinal, final_update),
            WebEvent::Permission { request, .. } => self.on_permission_request(&tab, request),
            WebEvent::DownloadStarting { request, .. } => self.on_download_starting(request),
            WebEvent::DownloadProgress { progress, .. } => self.on_download_progress(progress),
            WebEvent::PageMessage { view, frame_url, channel, payload, is_main_frame, frame_id, top_url } => {
                self.on_page_message(view, &tab, &frame_id, &frame_url, &top_url, is_main_frame, &channel, &payload)
            }
            WebEvent::CertificateError { code, url, .. } => self.log(format!("certificate error {code} {url}")),
            WebEvent::FocusGained { .. } => {}
            _ => {}
        }
    }

    fn on_new_window(&mut self, opener: &str, url: &str, d: Disposition) {
        if url.is_empty() || url == "about:blank" {
            return;
        }
        if self.open_shortcut(url) {
            return;
        }
        match d {
            Disposition::CurrentTab => self.load_url(opener, url, false),
            Disposition::BackgroundTab => {
                self.open_tab(url, OpenOptions { activate: false, after_tab_id: None, opener_id: Some(opener.to_string()) });
            }
            _ => {
                if matches!(d, Disposition::NewWindow | Disposition::Popup) && self.peek_new_window(opener, url) {
                    return;
                }
                if self.store.data.detached.contains(opener) {
                    // A detached tab (Peek) navigates in place.
                    self.load_url(opener, url, false);
                    return;
                }
                self.open_tab(url, OpenOptions { activate: true, after_tab_id: None, opener_id: Some(opener.to_string()) });
                self.request_focus();
            }
        }
    }
}

fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

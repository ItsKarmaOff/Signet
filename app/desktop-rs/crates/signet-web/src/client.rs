//! Browser-process handlers: one `Client` per view, translating CEF callbacks into [`WebEvent`]s.
//! Everything here runs on the CEF UI thread, which is the main thread (external message pump).

use cef::{sys, *};

use crate::runtime::{push_event, with_state};
use crate::types::*;
use crate::types::KeyEvent as AppKeyEvent;
use crate::types::ResourceType;

fn s(c: Option<&CefString>) -> String {
    c.map(|c| c.to_string()).unwrap_or_default()
}

fn userfree(u: &CefStringUserfree) -> String {
    CefString::from(u).to_string()
}

fn opt(u: &CefStringUserfree) -> Option<String> {
    let v = userfree(u);
    if v.is_empty() { None } else { Some(v) }
}

fn disposition(d: &WindowOpenDisposition) -> Disposition {
    use sys::cef_window_open_disposition_t as W;
    match d.as_ref() {
        W::CEF_WOD_CURRENT_TAB => Disposition::CurrentTab,
        W::CEF_WOD_NEW_FOREGROUND_TAB => Disposition::ForegroundTab,
        W::CEF_WOD_NEW_BACKGROUND_TAB => Disposition::BackgroundTab,
        W::CEF_WOD_NEW_POPUP => Disposition::Popup,
        W::CEF_WOD_NEW_WINDOW => Disposition::NewWindow,
        _ => Disposition::Other,
    }
}

wrap_life_span_handler! {
    struct SignetLifeSpan { view: ViewId }

    impl LifeSpanHandler {
        fn on_before_popup(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _popup_id: ::std::os::raw::c_int,
            target_url: Option<&CefString>,
            target_frame_name: Option<&CefString>,
            target_disposition: WindowOpenDisposition,
            user_gesture: ::std::os::raw::c_int,
            _popup_features: Option<&PopupFeatures>,
            _window_info: Option<&mut WindowInfo>,
            _client: Option<&mut Option<Client>>,
            _settings: Option<&mut BrowserSettings>,
            _extra_info: Option<&mut Option<DictionaryValue>>,
            _no_javascript_access: Option<&mut ::std::os::raw::c_int>,
        ) -> ::std::os::raw::c_int {
            push_event(WebEvent::NewWindowRequested {
                view: self.view,
                url: s(target_url),
                disposition: disposition(&target_disposition),
                user_gesture: user_gesture != 0,
                target_name: s(target_frame_name),
            });
            1
        }

        fn on_after_created(&self, browser: Option<&mut Browser>) {
            if let Some(b) = browser {
                crate::view::attach_browser(self.view, b.clone());
            }
            push_event(WebEvent::Created { view: self.view });
        }

        fn do_close(&self, browser: Option<&mut Browser>) -> ::std::os::raw::c_int {
            if let Some(host) = browser.and_then(|b| b.host()) {
                crate::platform::destroy_native(&host);
            }
            // The window around the view belongs to the app: returning false makes CEF close that NSWindow.
            1
        }

        fn on_before_close(&self, browser: Option<&mut Browser>) {
            if let Some(b) = browser {
                crate::bundle::forget_browser(b.identifier());
            }
            with_state(|st| {
                st.views.remove(&self.view);
            });
            crate::cdp::detach(self.view);
            crate::media::reset(self.view);
            push_event(WebEvent::Closed { view: self.view });
        }
    }
}

wrap_display_handler! {
    struct SignetDisplay { view: ViewId }

    impl DisplayHandler {
        fn on_address_change(&self, _browser: Option<&mut Browser>, frame: Option<&mut Frame>, url: Option<&CefString>) {
            let is_main = frame.map(|f| f.is_main() == 1).unwrap_or(true);
            push_event(WebEvent::AddressChanged { view: self.view, url: s(url), is_main_frame: is_main });
        }

        fn on_title_change(&self, _browser: Option<&mut Browser>, title: Option<&CefString>) {
            push_event(WebEvent::TitleChanged { view: self.view, title: s(title) });
        }

        fn on_favicon_urlchange(&self, _browser: Option<&mut Browser>, icon_urls: Option<&mut CefStringList>) {
            // `CefStringList::into_iter` on a borrowed list yields nothing: read it through the C API.
            let urls: Vec<String> = icon_urls
                .map(|l| {
                    let list: *mut cef::sys::_cef_string_list_t = l.into();
                    if list.is_null() {
                        return Vec::new();
                    }
                    let count = unsafe { cef::sys::cef_string_list_size(list) };
                    (0..count)
                        .filter_map(|i| unsafe {
                            let mut value: cef::sys::cef_string_t = std::mem::zeroed();
                            if cef::sys::cef_string_list_value(list, i, &mut value) > 0 {
                                let text = CefString::from(std::ptr::from_ref(&value) as *const _).to_string();
                                cef::sys::cef_string_utf16_clear(&mut value);
                                Some(text)
                            } else {
                                None
                            }
                        })
                        .collect()
                })
                .unwrap_or_default();
            push_event(WebEvent::FaviconUrls { view: self.view, urls });
        }

        fn on_fullscreen_mode_change(&self, _browser: Option<&mut Browser>, fullscreen: ::std::os::raw::c_int) {
            push_event(WebEvent::FullscreenChanged { view: self.view, fullscreen: fullscreen != 0 });
        }

        fn on_status_message(&self, _browser: Option<&mut Browser>, value: Option<&CefString>) {
            push_event(WebEvent::StatusText { view: self.view, url: s(value) });
        }

        fn on_console_message(
            &self,
            _browser: Option<&mut Browser>,
            level: LogSeverity,
            message: Option<&CefString>,
            source: Option<&CefString>,
            line: ::std::os::raw::c_int,
        ) -> ::std::os::raw::c_int {
            use sys::cef_log_severity_t as L;
            let level = match level.as_ref() {
                L::LOGSEVERITY_VERBOSE => ConsoleLevel::Debug,
                L::LOGSEVERITY_INFO => ConsoleLevel::Info,
                L::LOGSEVERITY_WARNING => ConsoleLevel::Warning,
                _ => ConsoleLevel::Error,
            };
            push_event(WebEvent::Console { view: self.view, level, message: s(message), source: s(source), line });
            // Keep the default (stdout) logging off.
            1
        }

        fn on_loading_progress_change(&self, _browser: Option<&mut Browser>, progress: f64) {
            push_event(WebEvent::LoadProgress { view: self.view, progress });
        }
    }
}

wrap_load_handler! {
    struct SignetLoad { view: ViewId }

    impl LoadHandler {
        fn on_loading_state_change(
            &self,
            _browser: Option<&mut Browser>,
            is_loading: ::std::os::raw::c_int,
            can_go_back: ::std::os::raw::c_int,
            can_go_forward: ::std::os::raw::c_int,
        ) {
            crate::view::tlog(|| format!("loading_state view={} loading={}", self.view.0, is_loading));
            push_event(WebEvent::LoadingState {
                view: self.view,
                is_loading: is_loading != 0,
                can_go_back: can_go_back != 0,
                can_go_forward: can_go_forward != 0,
            });
        }

        fn on_load_start(&self, _browser: Option<&mut Browser>, frame: Option<&mut Frame>, _t: TransitionType) {
            crate::view::tlog(|| format!("load_start view={}", self.view.0));
            if let Some(f) = frame {
                if f.is_main() == 1 {
                    crate::media::reset(self.view);
                }
                push_event(WebEvent::LoadStart { view: self.view, is_main_frame: f.is_main() == 1, url: userfree(&f.url()) });
            }
        }

        fn on_load_end(&self, _browser: Option<&mut Browser>, frame: Option<&mut Frame>, http_status_code: ::std::os::raw::c_int) {
            if let Some(f) = frame {
                push_event(WebEvent::LoadEnd {
                    view: self.view,
                    is_main_frame: f.is_main() == 1,
                    http_status: http_status_code,
                    url: userfree(&f.url()),
                });
            }
        }

        fn on_load_error(
            &self,
            _browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            error_code: Errorcode,
            error_text: Option<&CefString>,
            failed_url: Option<&CefString>,
        ) {
            let is_main = frame.map(|f| f.is_main() == 1).unwrap_or(true);
            let code: i32 = error_code.as_ref().clone() as i32;
            if std::env::var_os("SIGNET_WEB_DEBUG").is_some() {
                eprintln!("[signet-web] load error {code} {} {}", s(error_text), s(failed_url));
            }
            push_event(WebEvent::LoadError { view: self.view, code, text: s(error_text), url: s(failed_url), is_main_frame: is_main });
        }
    }
}

/// Type of the native event argument of `on_pre_key_event`; it differs per platform in the CEF bindings.
#[cfg(target_os = "macos")]
type OsKeyEvent = *mut u8;
#[cfg(target_os = "windows")]
type OsKeyEvent<'a> = Option<&'a mut sys::MSG>;
#[cfg(target_os = "linux")]
type OsKeyEvent<'a> = Option<&'a mut sys::XEvent>;

wrap_keyboard_handler! {
    struct SignetKeyboard { view: ViewId }

    impl KeyboardHandler {
        fn on_pre_key_event(
            &self,
            _browser: Option<&mut Browser>,
            event: Option<&cef::KeyEvent>,
            _os_event: OsKeyEvent,
            _is_keyboard_shortcut: Option<&mut ::std::os::raw::c_int>,
        ) -> ::std::os::raw::c_int {
            let Some(e) = event else { return 0 };
            use sys::cef_key_event_type_t as K;
            let kind = e.type_.as_ref().clone();
            let pressed = matches!(kind, K::KEYEVENT_RAWKEYDOWN | K::KEYEVENT_KEYDOWN);
            let is_up = matches!(kind, K::KEYEVENT_KEYUP);
            let m = e.modifiers;
            let ch = char::from_u32(e.unmodified_character as u32).filter(|c| *c != '\0').or_else(|| char::from_u32(e.character as u32).filter(|c| *c != '\0'));
            let ev = AppKeyEvent {
                pressed,
                windows_key_code: e.windows_key_code,
                native_key_code: e.native_key_code,
                character: ch.map(|c| c.to_ascii_lowercase()),
                shift: m & (1 << 1) != 0,
                ctrl: m & (1 << 2) != 0,
                alt: m & (1 << 3) != 0,
                cmd: m & (1 << 7) != 0,
                is_repeat: m & (1 << 13) != 0,
            };
            let code = ev.windows_key_code;
            let handled = with_state(|st| {
                if is_up {
                    if let Some(f) = st.key_observer.as_ref() {
                        f(&ev, true);
                    }
                    if let Some(i) = st.swallowed_keys.iter().position(|k| *k == code) {
                        st.swallowed_keys.remove(i);
                        return true;
                    }
                    return false;
                }
                let observed = st.key_observer.as_ref().map(|f| f(&ev, false)).unwrap_or(false);
                let h = observed || st.key_filter.as_ref().map(|f| f(&ev)).unwrap_or(false);
                if h && pressed && !st.swallowed_keys.contains(&code) {
                    st.swallowed_keys.push(code);
                }
                h
            })
            .unwrap_or(false);
            if pressed {
                push_event(WebEvent::PreKey { view: self.view, event: ev, handled });
            }
            handled as i32
        }
    }
}

wrap_focus_handler! {
    struct SignetFocus { view: ViewId }

    impl FocusHandler {
        fn on_got_focus(&self, _browser: Option<&mut Browser>) {
            push_event(WebEvent::FocusGained { view: self.view });
        }
    }
}

wrap_context_menu_handler! {
    struct SignetContextMenu { view: ViewId }

    impl ContextMenuHandler {
        fn run_context_menu(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            params: Option<&mut ContextMenuParams>,
            _model: Option<&mut MenuModel>,
            callback: Option<&mut RunContextMenuCallback>,
        ) -> ::std::os::raw::c_int {
            let Some(p) = params else { return 0 };
            let ty = p.type_flags().as_ref().0;
            let edit = p.edit_state_flags().as_ref().0;
            // cef_context_menu_type_flags_t: LINK=2 MEDIA=4 SELECTION=8 EDITABLE=16
            // cef_context_menu_edit_state_flags_t: CAN_UNDO=1 CAN_REDO=2 CAN_CUT=4 CAN_COPY=8 CAN_PASTE=16 CAN_DELETE=32 CAN_SELECT_ALL=64
            let mut sugg = CefStringList::new();
            p.dictionary_suggestions(Some(&mut sugg));
            let image = if p.has_image_contents() != 0 { opt(&p.source_url()) } else { None };
            let media = if ty & 4 != 0 && image.is_none() { opt(&p.source_url()) } else { None };
            let info = ContextMenuInfo {
                x: p.xcoord(),
                y: p.ycoord(),
                link_url: if ty & 2 != 0 { opt(&p.unfiltered_link_url()).or_else(|| opt(&p.link_url())) } else { None },
                image_url: image,
                media_url: media,
                page_url: userfree(&p.page_url()),
                frame_url: userfree(&p.frame_url()),
                selection_text: userfree(&p.selection_text()),
                misspelled_word: opt(&p.misspelled_word()),
                suggestions: sugg.into_iter().collect(),
                editable: p.is_editable() != 0 || ty & 16 != 0,
                has_selection: ty & 8 != 0,
                can_undo: edit & 1 != 0,
                can_redo: edit & 2 != 0,
                can_cut: edit & 4 != 0,
                can_copy: edit & 8 != 0,
                can_paste: edit & 16 != 0,
                can_select_all: edit & 64 != 0,
            };
            push_event(WebEvent::ContextMenu { view: self.view, info });
            // The app draws its own menu: dismiss the native one.
            if let Some(cb) = callback {
                cb.cancel();
            }
            1
        }
    }
}

wrap_find_handler! {
    struct SignetFind { view: ViewId }

    impl FindHandler {
        fn on_find_result(
            &self,
            _browser: Option<&mut Browser>,
            identifier: ::std::os::raw::c_int,
            count: ::std::os::raw::c_int,
            _selection_rect: Option<&Rect>,
            active_match_ordinal: ::std::os::raw::c_int,
            final_update: ::std::os::raw::c_int,
        ) {
            push_event(WebEvent::FindResult {
                view: self.view,
                request_id: identifier,
                count,
                active_ordinal: active_match_ordinal,
                final_update: final_update != 0,
            });
        }
    }
}

fn map_permissions(bits: u32) -> Vec<Permission> {
    let table: &[(u32, Permission)] = &[
        (4, Permission::Camera),
        (4096, Permission::Microphone),
        (256, Permission::Geolocation),
        (32768, Permission::Notifications),
        (16, Permission::ClipboardRead),
        (8192, Permission::MidiSysex),
        (16384, Permission::MultipleDownloads),
        (1048576, Permission::StorageAccess),
        (32, Permission::StorageAccess),
        (2048, Permission::IdleDetection),
        (8388608, Permission::WindowManagement),
        (128, Permission::LocalFonts),
        (262144, Permission::ProtectedMedia),
    ];
    let mut out: Vec<Permission> = table.iter().filter(|(b, _)| bits & b != 0).map(|(_, p)| *p).collect();
    out.dedup();
    if out.is_empty() {
        out.push(Permission::Other(bits));
    }
    out
}

wrap_permission_handler! {
    struct SignetPermission { view: ViewId }

    impl PermissionHandler {
        fn on_request_media_access_permission(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            requesting_origin: Option<&CefString>,
            requested_permissions: u32,
            callback: Option<&mut MediaAccessCallback>,
        ) -> ::std::os::raw::c_int {
            let Some(cb) = callback.cloned() else { return 0 };
            let mut perms = Vec::new();
            if requested_permissions & 1 != 0 { perms.push(Permission::Microphone); }
            if requested_permissions & 2 != 0 { perms.push(Permission::Camera); }
            if requested_permissions & 12 != 0 { perms.push(Permission::ScreenCapture); }
            let responder: Box<dyn FnOnce(bool)> = Box::new(move |allow| {
                if allow { cb.cont(requested_permissions) } else { cb.cancel() }
            });
            push_event(WebEvent::Permission {
                view: self.view,
                request: PermissionRequest { origin: s(requesting_origin), permissions: perms, responder: Some(responder) },
            });
            1
        }

        fn on_show_permission_prompt(
            &self,
            _browser: Option<&mut Browser>,
            _prompt_id: u64,
            requesting_origin: Option<&CefString>,
            requested_permissions: u32,
            callback: Option<&mut PermissionPromptCallback>,
        ) -> ::std::os::raw::c_int {
            let Some(cb) = callback.cloned() else { return 0 };
            let responder: Box<dyn FnOnce(bool)> = Box::new(move |allow| {
                cb.cont(PermissionRequestResult::from(if allow {
                    sys::cef_permission_request_result_t::CEF_PERMISSION_RESULT_ACCEPT
                } else {
                    sys::cef_permission_request_result_t::CEF_PERMISSION_RESULT_DENY
                }));
            });
            push_event(WebEvent::Permission {
                view: self.view,
                request: PermissionRequest {
                    origin: s(requesting_origin),
                    permissions: map_permissions(requested_permissions),
                    responder: Some(responder),
                },
            });
            1
        }
    }
}

wrap_download_handler! {
    struct SignetDownload { view: ViewId }

    impl DownloadHandler {
        fn can_download(&self, _b: Option<&mut Browser>, _url: Option<&CefString>, _m: Option<&CefString>) -> ::std::os::raw::c_int {
            1
        }

        fn on_before_download(
            &self,
            _browser: Option<&mut Browser>,
            item: Option<&mut DownloadItem>,
            suggested_name: Option<&CefString>,
            callback: Option<&mut BeforeDownloadCallback>,
        ) -> ::std::os::raw::c_int {
            let (Some(item), Some(cb)) = (item, callback.cloned()) else { return 0 };
            let responder: Box<dyn FnOnce(Option<String>)> = Box::new(move |path| {
                if let Some(p) = path {
                    cb.cont(Some(&CefString::from(p.as_str())), 0);
                }
            });
            push_event(WebEvent::DownloadStarting {
                view: self.view,
                request: DownloadRequest {
                    id: item.id(),
                    url: userfree(&item.url()),
                    suggested_name: s(suggested_name),
                    mime_type: userfree(&item.mime_type()),
                    total_bytes: item.total_bytes(),
                    responder: Some(responder),
                },
            });
            1
        }

        fn on_download_updated(
            &self,
            _browser: Option<&mut Browser>,
            item: Option<&mut DownloadItem>,
            callback: Option<&mut DownloadItemCallback>,
        ) {
            let Some(item) = item else { return };
            let state = if item.is_complete() != 0 {
                DownloadState::Complete
            } else if item.is_canceled() != 0 {
                DownloadState::Canceled
            } else if item.is_interrupted() != 0 {
                DownloadState::Interrupted
            } else {
                DownloadState::InProgress
            };
            let id = item.id();
            with_state(|st| match (&state, callback.cloned()) {
                (DownloadState::InProgress, Some(cb)) => {
                    st.downloads.insert(id, cb);
                }
                _ => {
                    st.downloads.remove(&id);
                }
            });
            push_event(WebEvent::DownloadProgress {
                view: self.view,
                progress: DownloadProgress {
                    id,
                    url: userfree(&item.url()),
                    path: userfree(&item.full_path()),
                    state,
                    received_bytes: item.received_bytes(),
                    total_bytes: item.total_bytes(),
                    percent: item.percent_complete(),
                    speed_bps: item.current_speed(),
                    paused: item.is_paused() != 0,
                },
            });
        }
    }
}

wrap_request_handler! {
    struct SignetRequest { view: ViewId }

    impl RequestHandler {
        // A new render view (first one, or a cross-process navigation swap) starts with a default size and scale:
        // push the real ones, or the page is laid out and painted small for a moment, then jumps to the pane size.
        fn on_render_view_ready(&self, browser: Option<&mut Browser>) {
            crate::view::tlog(|| format!("on_render_view_ready view={}", self.view.0));
            if let Some(host) = browser.and_then(|b| b.host()) {
                host.notify_screen_info_changed();
                host.was_resized();
            }
        }

        fn on_open_urlfrom_tab(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            target_url: Option<&CefString>,
            target_disposition: WindowOpenDisposition,
            user_gesture: ::std::os::raw::c_int,
        ) -> ::std::os::raw::c_int {
            push_event(WebEvent::NewWindowRequested {
                view: self.view,
                url: s(target_url),
                disposition: disposition(&target_disposition),
                user_gesture: user_gesture != 0,
                target_name: String::new(),
            });
            1
        }

        fn resource_request_handler(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _request: Option<&mut Request>,
            _is_navigation: ::std::os::raw::c_int,
            _is_download: ::std::os::raw::c_int,
            request_initiator: Option<&CefString>,
            _disable_default_handling: Option<&mut ::std::os::raw::c_int>,
        ) -> Option<ResourceRequestHandler> {
            // Always installed (main frame response events); the filter is checked per request.
            Some(SignetResourceRequest::new(self.view, s(request_initiator)))
        }

        fn auth_credentials(
            &self,
            _browser: Option<&mut Browser>,
            origin_url: Option<&CefString>,
            is_proxy: ::std::os::raw::c_int,
            host: Option<&CefString>,
            port: ::std::os::raw::c_int,
            realm: Option<&CefString>,
            scheme: Option<&CefString>,
            callback: Option<&mut AuthCallback>,
        ) -> ::std::os::raw::c_int {
            if std::env::var_os("SIGNET_WEB_DEBUG").is_some() {
                eprintln!("[signet-web] auth_credentials host={} realm={} callback={}", s(host), s(realm), callback.is_some());
            }
            let Some(cb) = callback.cloned() else { return 0 };
            let responder: Box<dyn FnOnce(Option<(String, String)>)> = Box::new(move |creds| match creds {
                Some((u, p)) => cb.cont(Some(&CefString::from(u.as_str())), Some(&CefString::from(p.as_str()))),
                None => cb.cancel(),
            });
            // Called on the IO thread: the event joins the queue through the shared IO queue.
            crate::hooks::push_io_event(crate::hooks::IoEvent::Auth {
                view: self.view,
                request: crate::hooks::SendAuth(AuthRequest {
                    origin: s(origin_url),
                    host: s(host),
                    port,
                    realm: s(realm),
                    scheme: s(scheme),
                    is_proxy: is_proxy != 0,
                    responder: Some(responder),
                }),
            });
            1
        }

        fn on_certificate_error(
            &self,
            _browser: Option<&mut Browser>,
            cert_error: Errorcode,
            request_url: Option<&CefString>,
            ssl_info: Option<&mut Sslinfo>,
            callback: Option<&mut Callback>,
        ) -> ::std::os::raw::c_int {
            let code = cert_error.as_ref().clone() as i32;
            let url = s(request_url);
            let (mut subject, mut issuer, mut status) = (String::new(), String::new(), 0u32);
            if let Some(info) = ssl_info {
                status = info.cert_status().as_ref().clone() as u32;
                if let Some(cert) = info.x509_certificate() {
                    subject = cert.subject().map(|p| userfree(&p.display_name())).unwrap_or_default();
                    issuer = cert.issuer().map(|p| userfree(&p.display_name())).unwrap_or_default();
                }
            }
            let info = CertificateErrorInfo { view: self.view, code, url: url.clone(), cert_status: status, subject, issuer };
            let decision = with_state(|st| st.certificate_handler.as_ref().map(|h| h(&info))).flatten();
            match (decision, callback.cloned()) {
                (Some(CertificateDecision::Proceed), Some(cb)) => {
                    cb.cont();
                    return 1;
                }
                (Some(CertificateDecision::Ask), Some(cb)) => {
                    let responder: Box<dyn FnOnce(bool)> = Box::new(move |go| if go { cb.cont() } else { cb.cancel() });
                    push_event(WebEvent::CertificateErrorPrompt {
                        view: self.view,
                        request: CertificateErrorRequest { info, responder: Some(responder) },
                    });
                    return 1;
                }
                _ => {}
            }
            push_event(WebEvent::CertificateError { view: self.view, code, url });
            // 0: cancel the request, CEF shows its error page.
            0
        }

        fn on_render_process_terminated(
            &self,
            _browser: Option<&mut Browser>,
            status: TerminationStatus,
            error_code: ::std::os::raw::c_int,
            error_string: Option<&CefString>,
        ) {
            crate::media::reset(self.view);
            push_event(WebEvent::RenderProcessGone {
                view: self.view,
                status: status.as_ref().clone() as i32,
                error_code,
                text: s(error_string),
            });
        }
    }
}


wrap_resource_request_handler! {
    struct SignetResourceRequest { view: ViewId, initiator: String }

    impl ResourceRequestHandler {
        fn on_before_resource_load(
            &self,
            browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            request: Option<&mut Request>,
            _callback: Option<&mut Callback>,
        ) -> ReturnValue {
            use sys::cef_return_value_t as R;
            let go = ReturnValue::from(R::RV_CONTINUE);
            let Some(request) = request else { return go };
            let Some(filter) = crate::hooks::REQUEST_FILTER.read().ok().and_then(|g| g.clone()) else { return go };
            let url = userfree(&request.url());
            let rt = ResourceType::from_raw(request.resource_type().as_ref().clone() as i32);
            let _ = frame;
            let is_main = rt == ResourceType::MainFrame;
            let first_party = if is_main {
                url.clone()
            } else {
                browser
                    .and_then(|b| b.main_frame())
                    .map(|f| userfree(&f.url()))
                    .filter(|u| !u.is_empty())
                    .unwrap_or_else(|| userfree(&request.first_party_for_cookies()))
            };
            let info = RequestInfo {
                view: self.view,
                url: crate::extension::from_nav(&url),
                method: userfree(&request.method()),
                resource_type: rt,
                first_party_url: crate::extension::from_nav(&first_party),
                is_main_frame: is_main,
                initiator: crate::extension::from_nav(&self.initiator),
            };
            match filter(&info) {
                RequestDecision::Allow => go,
                RequestDecision::Block => {
                    crate::hooks::push_io_event(crate::hooks::IoEvent::RequestBlocked {
                        view: self.view,
                        url: info.url,
                        resource_type: rt,
                        is_main_frame: is_main,
                    });
                    ReturnValue::from(R::RV_CANCEL)
                }
                RequestDecision::Redirect(to) => {
                    request.set_url(Some(&CefString::from(to.as_str())));
                    go
                }
                RequestDecision::AddHeaders(headers) => {
                    for (k, v) in headers {
                        request.set_header_by_name(Some(&CefString::from(k.as_str())), Some(&CefString::from(v.as_str())), 1);
                    }
                    go
                }
            }
        }

        fn on_resource_response(
            &self,
            _browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            request: Option<&mut Request>,
            response: Option<&mut Response>,
        ) -> ::std::os::raw::c_int {
            let (Some(request), Some(response)) = (request, response) else { return 0 };
            let rt = ResourceType::from_raw(request.resource_type().as_ref().clone() as i32);
            let is_main = rt == ResourceType::MainFrame;
            let sub = rt == ResourceType::SubFrame;
            let _ = frame;
            if !is_main && !sub {
                return 0;
            }
            let url = userfree(&request.url());
            let mime = userfree(&response.mime_type());
            if is_main {
                crate::hooks::push_io_event(crate::hooks::IoEvent::MainFrameResponse {
                    view: self.view,
                    url: url.clone(),
                    status: response.status(),
                    mime_type: mime.clone(),
                });
            }
            let Some(hook) = crate::hooks::RESPONSE_HOOK.read().ok().and_then(|g| g.clone()) else { return 0 };
            let mut map = CefStringMultimap::new();
            response.header_map(Some(&mut map));
            let headers: Vec<(String, String)> =
                map.into_iter().flat_map(|(k, vs)| vs.into_iter().map(move |v| (k.clone(), v))).collect();
            let info = ResponseInfo { view: self.view, url, status: response.status(), mime_type: mime, headers, is_main_frame: is_main };
            match hook(&info) {
                ResponseDecision::Continue => 0,
                ResponseDecision::Redirect(to) => {
                    request.set_url(Some(&CefString::from(to.as_str())));
                    1
                }
            }
        }
    }
}

wrap_jsdialog_handler! {
    struct SignetJsDialog { view: ViewId }

    impl JsdialogHandler {
        fn on_jsdialog(
            &self,
            _browser: Option<&mut Browser>,
            origin_url: Option<&CefString>,
            dialog_type: JsdialogType,
            message_text: Option<&CefString>,
            default_prompt_text: Option<&CefString>,
            callback: Option<&mut JsdialogCallback>,
            _suppress_message: Option<&mut ::std::os::raw::c_int>,
        ) -> ::std::os::raw::c_int {
            if std::env::var_os("SIGNET_WEB_DEBUG").is_some() {
                eprintln!("[signet-web] on_jsdialog mode={:?}", crate::hooks::js_dialog_mode());
            }
            if crate::hooks::js_dialog_mode() != JsDialogMode::Events {
                return 0;
            }
            let Some(cb) = callback.cloned() else { return 0 };
            use sys::cef_jsdialog_type_t as T;
            let kind = match dialog_type.as_ref() {
                T::JSDIALOGTYPE_ALERT => JsDialogKind::Alert,
                T::JSDIALOGTYPE_CONFIRM => JsDialogKind::Confirm,
                _ => JsDialogKind::Prompt,
            };
            let responder: Box<dyn FnOnce(bool, String)> =
                Box::new(move |ok, text| cb.cont(ok as i32, Some(&CefString::from(text.as_str()))));
            push_event(WebEvent::JsDialogRequest {
                view: self.view,
                request: JsDialogRequest {
                    kind,
                    origin: s(origin_url),
                    message: s(message_text),
                    default_prompt: s(default_prompt_text),
                    is_reload: false,
                    responder: Some(responder),
                },
            });
            1
        }

        fn on_before_unload_dialog(
            &self,
            _browser: Option<&mut Browser>,
            message_text: Option<&CefString>,
            is_reload: ::std::os::raw::c_int,
            callback: Option<&mut JsdialogCallback>,
        ) -> ::std::os::raw::c_int {
            if crate::hooks::js_dialog_mode() != JsDialogMode::Events {
                return 0;
            }
            let Some(cb) = callback.cloned() else { return 0 };
            let responder: Box<dyn FnOnce(bool, String)> =
                Box::new(move |ok, _| cb.cont(ok as i32, Some(&CefString::from(""))));
            push_event(WebEvent::JsDialogRequest {
                view: self.view,
                request: JsDialogRequest {
                    kind: JsDialogKind::BeforeUnload,
                    origin: String::new(),
                    message: s(message_text),
                    default_prompt: String::new(),
                    is_reload: is_reload != 0,
                    responder: Some(responder),
                },
            });
            1
        }
    }
}

wrap_client! {
    pub struct SignetClient {
        view: ViewId,
        life: LifeSpanHandler,
        display: DisplayHandler,
        load: LoadHandler,
        keyboard: KeyboardHandler,
        focus: FocusHandler,
        menu: ContextMenuHandler,
        find: FindHandler,
        permission: PermissionHandler,
        download: DownloadHandler,
        request: RequestHandler,
        jsdialog: JsdialogHandler,
    }

    impl Client {
        fn life_span_handler(&self) -> Option<LifeSpanHandler> { Some(self.life.clone()) }
        fn display_handler(&self) -> Option<DisplayHandler> { Some(self.display.clone()) }
        fn load_handler(&self) -> Option<LoadHandler> { Some(self.load.clone()) }
        fn keyboard_handler(&self) -> Option<KeyboardHandler> { Some(self.keyboard.clone()) }
        fn focus_handler(&self) -> Option<FocusHandler> { Some(self.focus.clone()) }
        fn context_menu_handler(&self) -> Option<ContextMenuHandler> { Some(self.menu.clone()) }
        fn find_handler(&self) -> Option<FindHandler> { Some(self.find.clone()) }
        fn permission_handler(&self) -> Option<PermissionHandler> { Some(self.permission.clone()) }
        fn download_handler(&self) -> Option<DownloadHandler> { Some(self.download.clone()) }
        fn request_handler(&self) -> Option<RequestHandler> { Some(self.request.clone()) }
        fn jsdialog_handler(&self) -> Option<JsdialogHandler> { Some(self.jsdialog.clone()) }

        fn on_process_message_received(
            &self,
            browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            _source_process: ProcessId,
            message: Option<&mut ProcessMessage>,
        ) -> ::std::os::raw::c_int {
            let (Some(frame), Some(msg)) = (frame, message) else { return 0 };
            let name = userfree(&msg.name());
            let is_bridge = name == crate::app::BRIDGE_MESSAGE;
            let is_media = name == crate::app::MEDIA_MESSAGE;
            if !is_bridge && !is_media {
                return 0;
            }
            let Some(args) = msg.argument_list() else { return 1 };
            let (a0, a1) = (userfree(&args.string(0)), userfree(&args.string(1)));
            let frame_id = userfree(&frame.identifier());
            if is_media {
                crate::media::on_report(self.view, &frame_id, &a0, &a1);
                return 1;
            }
            let top_url = browser
                .and_then(|b| b.main_frame())
                .map(|f| userfree(&f.url()))
                .unwrap_or_default();
            push_event(WebEvent::PageMessage {
                view: self.view,
                frame_url: userfree(&frame.url()),
                is_main_frame: frame.is_main() == 1,
                channel: a0,
                payload: a1,
                frame_id,
                top_url,
            });
            1
        }
    }
}

/// Build the client of a view.
pub(crate) fn make_client(view: ViewId) -> Client {
    SignetClient::new(
        view,
        SignetLifeSpan::new(view),
        SignetDisplay::new(view),
        SignetLoad::new(view),
        SignetKeyboard::new(view),
        SignetFocus::new(view),
        SignetContextMenu::new(view),
        SignetFind::new(view),
        SignetPermission::new(view),
        SignetDownload::new(view),
        SignetRequest::new(view),
        SignetJsDialog::new(view),
    )
}

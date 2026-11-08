//! Passkeys from a phone (hybrid transport): page channels `webauthn:request` / `webauthn:abort`, the session
//! (`HybridManager`), and the prompt shown by the passkey window (`signet_ui::screens::windows::passkey::Passkey`).
//!
//! One session at a time. The page is answered exactly once: by the final answer of the session, or at once with
//! `NotAllowedError` when the user cancels, the page aborts, a new request replaces it, the tab navigates or goes away.

use serde_json::{Value, json};
use signet_core::api::UiEvent;
use signet_core::types::{PasskeyPromptState, PasskeyStatus, PasskeyType};
use signet_link::hybrid::session::not_allowed_answer;
use signet_link::{HybridEvent, HybridHandle, HybridManager, Precheck, WebAuthnAnswer, webauthn_precheck};
use signet_scripts::webauthn_request::{RequestKind, WebAuthnRequest};
use signet_scripts::{Accepted, Reply};

use super::convert;
use crate::phase3::pages::PageCtx;
use crate::browser::Core;

pub(super) struct Session {
    ctx: PageCtx,
    handle: HybridHandle,
    prompt: PasskeyPromptState,
}

#[derive(Default)]
pub(super) struct State {
    /// Created at the first request (it owns a runtime and the Bluetooth scan).
    hybrid: Option<HybridManager>,
    session: Option<Session>,
}

/// `{type, mediation?, publicKey}`: the page payload `webauthn_precheck` validates.
pub(super) fn request_payload(r: &WebAuthnRequest) -> Value {
    let mut p = json!({
        "type": match r.kind { RequestKind::Get => "get", RequestKind::Create => "create" },
        "publicKey": r.public_key,
    });
    if let Some(m) = &r.mediation {
        p["mediation"] = json!(m);
    }
    p
}

fn reply(core: &mut Core, ctx: &PageCtx, answer: &WebAuthnAnswer) {
    if let Some(rid) = ctx.rid.as_deref() {
        core.page_reply(ctx, Reply::ok(rid, &answer.to_json()));
    }
}

impl Core {
    /// Page channels `webauthn:request` and `webauthn:abort`.
    pub fn page_webauthn(&mut self, ctx: &PageCtx, accepted: Accepted) {
        match accepted {
            Accepted::WebauthnRequest(req) => self.passkey_request(ctx, &req),
            Accepted::WebauthnAbort => {
                let owns = self.phase3.link.passkey.session.as_ref().is_some_and(|s| s.ctx.view == ctx.view && s.ctx.frame_id == ctx.frame_id);
                if owns {
                    self.passkey_end(&not_allowed_answer());
                }
            }
            _ => {}
        }
    }

    fn passkey_request(&mut self, ctx: &PageCtx, req: &WebAuthnRequest) {
        if ctx.rid.is_none() {
            return;
        }
        let payload = request_payload(req);
        let request = match webauthn_precheck(ctx.sender.origin.as_deref(), ctx.sender.top_origin.as_deref(), ctx.sender.is_main_frame, &payload) {
            Precheck::Answer(a) => return reply(self, ctx, &a),
            Precheck::Start(r) => r,
        };
        // A new request replaces the running one, which is refused.
        self.passkey_end(&not_allowed_answer());
        let st = &mut self.phase3.link.passkey;
        let handle = st.hybrid.get_or_insert_with(HybridManager::new).start_hybrid(request);
        let Some(matrix) = handle.qr_matrix.as_ref() else {
            handle.cancel();
            return reply(self, ctx, &not_allowed_answer());
        };
        let prompt = PasskeyPromptState {
            kind: if handle.request_type == "create" { PasskeyType::Create } else { PasskeyType::Get },
            rp_id: handle.rp_id.clone(),
            qr: convert::qr_data_url(matrix),
            status: PasskeyStatus::Waiting,
        };
        st.session = Some(Session { ctx: ctx.clone(), handle, prompt });
        self.passkey_window(true);
    }

    /// What the passkey window shows (None: nothing to show).
    pub fn get_passkey_prompt(&mut self) -> Option<PasskeyPromptState> {
        self.phase3.link.passkey.session.as_ref().map(|s| s.prompt.clone())
    }

    /// Cancel button, Escape.
    pub fn cancel_passkey_prompt(&mut self) {
        self.passkey_end(&not_allowed_answer());
    }

    /// The passkey window was closed by the user (the window manager calls this).
    pub fn passkey_window_closed(&mut self) {
        self.passkey_end_with(&not_allowed_answer(), false);
    }

    fn passkey_end(&mut self, answer: &WebAuthnAnswer) {
        self.passkey_end_with(answer, true);
    }

    /// Ends the session (if any), closes the window and answers the page.
    fn passkey_end_with(&mut self, answer: &WebAuthnAnswer, close_window: bool) {
        let Some(s) = self.phase3.link.passkey.session.take() else { return };
        s.handle.cancel();
        if close_window {
            self.passkey_window(false);
        }
        reply(self, &s.ctx, answer);
    }

    /// Statuses and the final answer of the running session.
    pub(super) fn passkey_pump(&mut self) {
        let mut events = Vec::new();
        if let Some(s) = &self.phase3.link.passkey.session {
            while let Ok(ev) = s.handle.events.try_recv() {
                events.push(ev);
            }
        }
        for ev in events {
            match ev {
                HybridEvent::Status(st) => {
                    let st = convert::passkey_status(st);
                    if let Some(s) = self.phase3.link.passkey.session.as_mut() {
                        s.prompt.status = st;
                    }
                    self.emit(UiEvent::PasskeyStatus(st));
                }
                HybridEvent::AdvertSeen { tunnel_domain } => self.log(format!("passkey: phone found (tunnel {tunnel_domain})")),
                HybridEvent::Done(answer) => {
                    self.passkey_end(&answer);
                    return;
                }
            }
        }
    }

    /// The tab of the session navigates, crashes or closes: the request is over.
    pub(super) fn passkey_web_event(&mut self, ev: &signet_web::WebEvent) {
        use signet_web::WebEvent::*;
        let Some(s) = &self.phase3.link.passkey.session else { return };
        let gone = match ev {
            LoadStart { view, is_main_frame: true, .. } | Closed { view } | RenderProcessGone { view, .. } => *view == s.ctx.view,
            _ => false,
        };
        if gone {
            self.passkey_end(&not_allowed_answer());
        }
    }

    pub(super) fn passkey_shutdown(&mut self) {
        // No answer: the pages are going away with the application.
        if let Some(s) = self.phase3.link.passkey.session.take() {
            s.handle.cancel();
        }
        self.phase3.link.passkey.hybrid = None;
    }

    /// Opens (a fresh one: it reads the prompt when created) or closes the passkey window.
    fn passkey_window(&mut self, open: bool) {
        use crate::windows::{Cmd, Win};
        self.send(Cmd::CloseWin(Win::Passkey));
        if open {
            self.send(Cmd::OpenWin(Win::Passkey, None));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(kind: RequestKind, mediation: Option<&str>) -> WebAuthnRequest {
        WebAuthnRequest {
            kind,
            mediation: mediation.map(str::to_string),
            rp_id: "example.com".into(),
            origin: "https://example.com".into(),
            public_key: json!({ "challenge": "AQID", "rpId": "example.com" }).as_object().cloned().unwrap_or_default(),
        }
    }

    #[test]
    fn payload_has_the_page_shape() {
        let p = request_payload(&req(RequestKind::Create, None));
        assert_eq!(p["type"], "create");
        assert_eq!(p["publicKey"]["challenge"], "AQID");
        assert!(p.get("mediation").is_none());
        assert_eq!(request_payload(&req(RequestKind::Get, Some("conditional")))["mediation"], "conditional");
    }

    #[test]
    fn precheck_starts_a_session_for_a_secure_main_frame() {
        let p = request_payload(&req(RequestKind::Get, None));
        match webauthn_precheck(Some("https://example.com"), Some("https://example.com"), true, &p) {
            Precheck::Start(r) => assert_eq!(r.rp_id, "example.com"),
            Precheck::Answer(a) => panic!("{a:?}"),
        }
    }

    #[test]
    fn precheck_refuses_conditional_insecure_and_foreign_rp() {
        let cond = request_payload(&req(RequestKind::Get, Some("conditional")));
        assert!(matches!(webauthn_precheck(Some("https://example.com"), Some("https://example.com"), true, &cond), Precheck::Answer(_)));
        let p = request_payload(&req(RequestKind::Get, None));
        assert!(matches!(webauthn_precheck(Some("http://example.com"), Some("http://example.com"), true, &p), Precheck::Answer(_)));
        assert!(matches!(webauthn_precheck(Some("https://evil.test"), Some("https://evil.test"), true, &p), Precheck::Answer(_)));
        assert!(matches!(webauthn_precheck(None, None, true, &p), Precheck::Answer(_)));
    }

    #[test]
    fn refusal_answers_are_not_allowed_json() {
        let j = not_allowed_answer().to_json();
        assert_eq!(j["error"]["name"], "NotAllowedError");
    }
}

//! What the browser side must check before acting on a page message.
//!
//! Everything a page sends is untrusted, including the payload and the channel. The only trusted inputs are in
//! [`Sender`], which the app fills from what the browser process knows about the frame that posted the message
//! (never from the message). Rules ported from `ipc/page.ts`, `features/passwords/passwords.ts`,
//! `features/autofill/autofill.ts` + `rules.ts`, `features/notes/notes.ts` and `web/webauthn.ts`.
//!
//! Stricter than Electron in one way: every channel requires the main frame (the scripts only run there).

use serde_json::{Map, Value};
use url::Url;

use crate::messages::{
    AutofillKind, Direction, PageMessage, Reply, SwipeState, WebAuthnAnswer,
};
use crate::notes::{self, NoteRef, NotesPatch};
use crate::scripts::AppPage;
use crate::webauthn_request::{self, WebAuthnError, WebAuthnInput, WebAuthnRequest};

/// Max length of a username or password (UTF-16 units, like the JS) accepted from a page.
pub const MAX_FIELD: usize = 1000;
/// Max length of an item id.
pub const MAX_ID: usize = 200;

/// What the browser process knows about the frame that posted a message.
#[derive(Debug, Clone)]
pub struct Sender {
    /// The view belongs to a tab (not an interface view, not an extension page).
    pub is_tab: bool,
    /// The tab is in a private window.
    pub is_private: bool,
    pub is_main_frame: bool,
    /// Current address of the frame, as the browser sees it.
    pub frame_url: String,
    /// Origin of the frame (`https://host[:port]`), None if opaque. See [`origin_of_url`].
    pub origin: Option<String>,
    /// Origin of the top frame, None if opaque.
    pub top_origin: Option<String>,
}

impl Sender {
    /// Sender of a tab's main frame, deriving the origins from `url`.
    pub fn main_frame(url: &str, is_private: bool) -> Self {
        let origin = origin_of_url(url);
        Self { is_tab: true, is_private, is_main_frame: true, frame_url: url.to_owned(), top_origin: origin.clone(), origin }
    }
}

/// `scheme://host[:port]` of an http(s) address, lowercase host; None for any other scheme.
pub fn origin_of_url(url: &str) -> Option<String> {
    let u = Url::parse(url).ok()?;
    matches!(u.scheme(), "http" | "https").then(|| u.origin().ascii_serialization())
}

/// Why a message is refused. Nothing is done for it; requests get [`refusal_reply`].
#[derive(Debug, Clone, PartialEq)]
pub enum Refusal {
    NotATab,
    NotMainFrame,
    /// Not an http(s) origin of the shape `scheme://host[:port]`.
    BadOrigin,
    /// Cards are not filled on plain http (outside localhost).
    InsecureOrigin,
    /// A value is too long or malformed.
    BadPayload,
    /// Notes: the frame is not an app page opened on a document.
    NotADocumentPage,
    /// Notes: private windows have no notes.
    PrivateWindow,
    /// Notes: the save was refused (too large, wrong shape). The page gets `false`.
    SaveRefused,
    /// A passkey request that cannot be served; the page gets this error.
    WebAuthn(WebAuthnError),
}

/// A message that passed the checks, with the values to use (trimmed, bounded).
#[derive(Debug, Clone, PartialEq)]
pub enum Accepted {
    SwipeCan,
    SwipeNavigate(Direction),
    PageSubmit,
    PwAccounts { origin: String },
    PwFill { origin: String, id: String },
    /// `username` is trimmed and bounded, `password` is at most [`MAX_FIELD`] long (may be empty: username step).
    PwSubmit { origin: String, username: String, password: String },
    AfMenu { origin: String, kind: AutofillKind },
    AfFill { origin: String, kind: AutofillKind, id: String },
    AfManage { kind: AutofillKind },
    /// Plain object `{ address?, card? }`; the autofill feature cleans it (`cleanAddress`, `parseCardSubmission`).
    AfSubmit { origin: String, payload: Map<String, Value> },
    WebauthnRequest(WebAuthnRequest),
    WebauthnAbort,
    Notes { doc: NoteRef, op: NotesOp },
}

#[derive(Debug, Clone, PartialEq)]
pub enum NotesOp {
    Locale,
    Appearance,
    Load,
    Save(NotesPatch),
    Remove,
}

fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// `s.trim().slice(0, n)` of JS.
fn trim_slice(s: &str, n: usize) -> String {
    let mut count = 0;
    let mut out = String::new();
    for c in s.trim().chars() {
        count += c.len_utf16();
        if count > n {
            break;
        }
        out.push(c);
    }
    out
}

/// `^https?://[^/]+$` (case-insensitive) on the sender's origin: the only origins credentials and autofill are served to.
pub fn http_origin(origin: Option<&str>) -> Result<String, Refusal> {
    let o = origin.ok_or(Refusal::BadOrigin)?;
    let lower = o.to_ascii_lowercase();
    let rest = lower.strip_prefix("https://").or_else(|| lower.strip_prefix("http://")).ok_or(Refusal::BadOrigin)?;
    if rest.is_empty() || rest.contains('/') {
        return Err(Refusal::BadOrigin);
    }
    Ok(o.to_owned())
}

/// Hosts where http is as safe as https for a card: the developer's own machine (port of `isLocalHost`).
pub fn is_local_host(hostname: &str) -> bool {
    let h = hostname.to_ascii_lowercase();
    let h = h.trim_start_matches('[').trim_end_matches(']');
    h == "localhost" || h.ends_with(".localhost") || h == "127.0.0.1" || h == "::1"
}

/// Filling happens on http(s) pages only; a card also needs https (or localhost for development) (port of `canFill`).
pub fn can_fill(kind: AutofillKind, origin: &str) -> bool {
    let Ok(u) = Url::parse(origin) else { return false };
    match u.scheme() {
        "https" => true,
        "http" => kind == AutofillKind::Address || u.host_str().is_some_and(is_local_host),
        _ => false,
    }
}

fn tab(sender: &Sender) -> Result<(), Refusal> {
    if !sender.is_tab {
        return Err(Refusal::NotATab);
    }
    if !sender.is_main_frame {
        return Err(Refusal::NotMainFrame);
    }
    Ok(())
}

fn id_ok(id: &str) -> Result<String, Refusal> {
    if id.is_empty() || utf16_len(id) > MAX_ID { Err(Refusal::BadPayload) } else { Ok(id.to_owned()) }
}

/// Checks a parsed message against the sender. `pages` are the app pages that host notes
/// ([`crate::ScriptOptions::notes_pages`]).
pub fn validate(sender: &Sender, message: &PageMessage, pages: &[AppPage]) -> Result<Accepted, Refusal> {
    use PageMessage as M;
    match message {
        M::SwipeCan => tab(sender).map(|()| Accepted::SwipeCan),
        M::SwipeNavigate(d) => tab(sender).map(|()| Accepted::SwipeNavigate(*d)),
        M::PageSubmit => tab(sender).map(|()| Accepted::PageSubmit),

        M::PwAccounts => {
            tab(sender)?;
            Ok(Accepted::PwAccounts { origin: http_origin(sender.origin.as_deref())? })
        }
        M::PwFill { id } => {
            tab(sender)?;
            let origin = http_origin(sender.origin.as_deref())?;
            Ok(Accepted::PwFill { origin, id: id_ok(id)? })
        }
        M::PwSubmit { username, password } => {
            tab(sender)?;
            let origin = http_origin(sender.origin.as_deref())?;
            if utf16_len(password) > MAX_FIELD {
                return Err(Refusal::BadPayload);
            }
            Ok(Accepted::PwSubmit { origin, username: trim_slice(username, MAX_FIELD), password: password.clone() })
        }

        M::AfMenu { kind } => {
            tab(sender)?;
            let origin = http_origin(sender.origin.as_deref())?;
            fill_rule(*kind, &origin)?;
            Ok(Accepted::AfMenu { origin, kind: *kind })
        }
        M::AfFill { kind, id } => {
            tab(sender)?;
            let origin = http_origin(sender.origin.as_deref())?;
            fill_rule(*kind, &origin)?;
            Ok(Accepted::AfFill { origin, kind: *kind, id: id_ok(id)? })
        }
        M::AfManage { kind } => {
            tab(sender)?;
            http_origin(sender.origin.as_deref())?;
            Ok(Accepted::AfManage { kind: *kind })
        }
        M::AfSubmit(payload) => {
            tab(sender)?;
            let origin = http_origin(sender.origin.as_deref())?;
            match payload {
                Value::Object(map) => Ok(Accepted::AfSubmit { origin, payload: map.clone() }),
                _ => Err(Refusal::BadPayload),
            }
        }

        M::WebauthnRequest(payload) => {
            if !sender.is_tab {
                return Err(Refusal::WebAuthn(webauthn_request::not_allowed_error()));
            }
            let input = WebAuthnInput {
                origin: sender.origin.as_deref(),
                top_origin: sender.top_origin.as_deref(),
                is_main_frame: sender.is_main_frame,
                payload,
            };
            webauthn_request::validate_webauthn_request(&input).map(Accepted::WebauthnRequest).map_err(Refusal::WebAuthn)
        }
        M::WebauthnAbort => {
            if sender.is_tab { Ok(Accepted::WebauthnAbort) } else { Err(Refusal::NotATab) }
        }

        M::NotesLocale => notes_doc(sender, pages).map(|doc| Accepted::Notes { doc, op: NotesOp::Locale }),
        M::NotesAppearance => notes_doc(sender, pages).map(|doc| Accepted::Notes { doc, op: NotesOp::Appearance }),
        M::NotesLoad => notes_doc(sender, pages).map(|doc| Accepted::Notes { doc, op: NotesOp::Load }),
        M::NotesRemove => notes_doc(sender, pages).map(|doc| Accepted::Notes { doc, op: NotesOp::Remove }),
        M::NotesSave(patch) => {
            let doc = notes_doc(sender, pages)?;
            let clean = notes::clean_patch(doc.kind, patch).ok_or(Refusal::SaveRefused)?;
            Ok(Accepted::Notes { doc, op: NotesOp::Save(clean) })
        }
    }
}

fn fill_rule(kind: AutofillKind, origin: &str) -> Result<(), Refusal> {
    if can_fill(kind, origin) { Ok(()) } else { Err(Refusal::InsecureOrigin) }
}

/// The document a notes page may touch: its id comes from the frame's address, never from the message.
fn notes_doc(sender: &Sender, pages: &[AppPage]) -> Result<NoteRef, Refusal> {
    tab(sender)?;
    if sender.is_private {
        return Err(Refusal::PrivateWindow);
    }
    notes::note_of_app_url(&sender.frame_url, pages).ok_or(Refusal::NotADocumentPage)
}

/// The reply a refused request gets (what the Electron handlers return); None for messages that expect no reply.
pub fn refusal_reply(rid: &str, message: &PageMessage, refusal: &Refusal) -> Option<Reply> {
    use PageMessage as M;
    Some(match message {
        M::SwipeCan => Reply::ok(rid, &SwipeState::OFF),
        M::PwAccounts => Reply::ok(rid, &Vec::<Value>::new()),
        M::PwFill { .. } | M::AfMenu { .. } | M::AfFill { .. } => Reply::ok(rid, &Value::Null),
        M::WebauthnRequest(_) => {
            let error = match refusal {
                Refusal::WebAuthn(e) => e.clone(),
                _ => webauthn_request::not_allowed_error(),
            };
            Reply::ok(rid, &WebAuthnAnswer::Error(error))
        }
        M::NotesSave(_) if *refusal == Refusal::SaveRefused => Reply::ok(rid, &false),
        M::NotesLocale | M::NotesAppearance | M::NotesLoad | M::NotesSave(_) | M::NotesRemove => Reply::err(rid, "Call refused"),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const ID: &str = "0b9d0f3e-1c1a-4a53-9a6f-2f6d2b1c9e11";

    fn pages() -> Vec<AppPage> {
        vec![AppPage::new("signet:", "app", "/index.html")]
    }
    fn web(url: &str) -> Sender {
        Sender::main_frame(url, false)
    }
    fn run(s: &Sender, m: &PageMessage) -> Result<Accepted, Refusal> {
        validate(s, m, &pages())
    }
    fn notes_sender() -> Sender {
        Sender::main_frame(&format!("signet://app/index.html#note/{ID}"), false)
    }

    #[test]
    fn origins() {
        assert_eq!(origin_of_url("https://Example.com/a?b#c").as_deref(), Some("https://example.com"));
        assert_eq!(origin_of_url("http://example.com:8080/").as_deref(), Some("http://example.com:8080"));
        assert_eq!(origin_of_url("ftp://example.com/"), None);
        assert_eq!(origin_of_url("signet://app/index.html"), None);
        assert!(http_origin(Some("https://a.b")).is_ok());
        assert!(http_origin(Some("HTTP://a.b:81")).is_ok());
        for bad in [None, Some("null"), Some("https://"), Some("https://a/b"), Some("file:///x"), Some("chrome-extension://abc")] {
            assert_eq!(http_origin(bad), Err(Refusal::BadOrigin), "{bad:?}");
        }
    }

    #[test]
    fn every_channel_needs_a_tab_main_frame() {
        let mut s = web("https://example.com/");
        s.is_tab = false;
        assert_eq!(run(&s, &PageMessage::SwipeCan), Err(Refusal::NotATab));
        assert_eq!(run(&s, &PageMessage::PwAccounts), Err(Refusal::NotATab));
        assert_eq!(run(&s, &PageMessage::AfManage { kind: AutofillKind::Card }), Err(Refusal::NotATab));
        assert_eq!(run(&s, &PageMessage::WebauthnAbort), Err(Refusal::NotATab));
        assert_eq!(run(&notes_sender_with(|s| s.is_tab = false), &PageMessage::NotesLoad), Err(Refusal::NotATab));
        let mut f = web("https://example.com/");
        f.is_main_frame = false;
        assert_eq!(run(&f, &PageMessage::SwipeNavigate(Direction::Back)), Err(Refusal::NotMainFrame));
        assert_eq!(run(&f, &PageMessage::PageSubmit), Err(Refusal::NotMainFrame));
        assert_eq!(run(&f, &PageMessage::PwFill { id: "1".into() }), Err(Refusal::NotMainFrame));
        assert_eq!(run(&notes_sender_with(|s| s.is_main_frame = false), &PageMessage::NotesLoad), Err(Refusal::NotMainFrame));
    }

    fn notes_sender_with(f: impl FnOnce(&mut Sender)) -> Sender {
        let mut s = notes_sender();
        f(&mut s);
        s
    }

    #[test]
    fn swipe_accepts_a_tab() {
        let s = web("https://example.com/");
        assert_eq!(run(&s, &PageMessage::SwipeCan), Ok(Accepted::SwipeCan));
        assert_eq!(run(&s, &PageMessage::SwipeNavigate(Direction::Forward)), Ok(Accepted::SwipeNavigate(Direction::Forward)));
        assert_eq!(run(&s, &PageMessage::PageSubmit), Ok(Accepted::PageSubmit));
    }

    #[test]
    fn passwords_use_the_sender_origin() {
        let s = web("https://login.example.com:8443/path");
        assert_eq!(run(&s, &PageMessage::PwAccounts), Ok(Accepted::PwAccounts { origin: "https://login.example.com:8443".into() }));
        assert_eq!(
            run(&s, &PageMessage::PwFill { id: "abc".into() }),
            Ok(Accepted::PwFill { origin: "https://login.example.com:8443".into(), id: "abc".into() })
        );
        // Internal pages and extension pages are never served.
        assert_eq!(run(&web("signet://app/index.html"), &PageMessage::PwAccounts), Err(Refusal::BadOrigin));
        let mut opaque = web("https://example.com/");
        opaque.origin = None;
        assert_eq!(run(&opaque, &PageMessage::PwAccounts), Err(Refusal::BadOrigin));
    }

    #[test]
    fn password_submission_is_bounded() {
        let s = web("https://example.com/");
        let long_user = format!("  {}  ", "u".repeat(1500));
        let m = PageMessage::PwSubmit { username: long_user, password: "p".repeat(MAX_FIELD) };
        match run(&s, &m) {
            Ok(Accepted::PwSubmit { username, password, origin }) => {
                assert_eq!(username.len(), MAX_FIELD);
                assert_eq!(password.len(), MAX_FIELD);
                assert_eq!(origin, "https://example.com");
            }
            other => panic!("{other:?}"),
        }
        let too_long = PageMessage::PwSubmit { username: "a".into(), password: "p".repeat(MAX_FIELD + 1) };
        assert_eq!(run(&s, &too_long), Err(Refusal::BadPayload));
        // Username step: empty password is fine.
        assert!(run(&s, &PageMessage::PwSubmit { username: "a".into(), password: String::new() }).is_ok());
        assert_eq!(run(&s, &PageMessage::PwFill { id: String::new() }), Err(Refusal::BadPayload));
        assert_eq!(run(&s, &PageMessage::PwFill { id: "x".repeat(MAX_ID + 1) }), Err(Refusal::BadPayload));
    }

    #[test]
    fn cards_need_https_or_localhost() {
        let card = |url: &str| run(&web(url), &PageMessage::AfMenu { kind: AutofillKind::Card });
        let addr = |url: &str| run(&web(url), &PageMessage::AfMenu { kind: AutofillKind::Address });
        assert!(card("https://shop.example/").is_ok());
        assert_eq!(card("http://shop.example/"), Err(Refusal::InsecureOrigin));
        assert!(card("http://localhost:3000/").is_ok());
        assert!(card("http://app.localhost/").is_ok());
        assert!(card("http://127.0.0.1:8080/").is_ok());
        assert!(addr("http://shop.example/").is_ok());
        assert_eq!(
            run(&web("https://shop.example/"), &PageMessage::AfFill { kind: AutofillKind::Card, id: "c1".into() }),
            Ok(Accepted::AfFill { origin: "https://shop.example".into(), kind: AutofillKind::Card, id: "c1".into() })
        );
        assert_eq!(
            run(&web("http://shop.example/"), &PageMessage::AfFill { kind: AutofillKind::Card, id: "c1".into() }),
            Err(Refusal::InsecureOrigin)
        );
        // Opening settings only needs a tab with an http(s) origin.
        assert!(run(&web("http://shop.example/"), &PageMessage::AfManage { kind: AutofillKind::Card }).is_ok());
    }

    #[test]
    fn can_fill_and_local_host() {
        assert!(can_fill(AutofillKind::Card, "https://a.b"));
        assert!(!can_fill(AutofillKind::Card, "http://a.b"));
        assert!(can_fill(AutofillKind::Address, "http://a.b"));
        assert!(!can_fill(AutofillKind::Address, "ftp://a.b"));
        assert!(!can_fill(AutofillKind::Address, "garbage"));
        assert!(is_local_host("[::1]") && is_local_host("LOCALHOST") && !is_local_host("evillocalhost"));
    }

    #[test]
    fn autofill_submit_needs_an_object() {
        let s = web("https://example.com/");
        assert!(run(&s, &PageMessage::AfSubmit(json!({ "address": {} }))).is_ok());
        for bad in [json!(null), json!([1]), json!("x"), json!(3)] {
            assert_eq!(run(&s, &PageMessage::AfSubmit(bad)), Err(Refusal::BadPayload));
        }
    }

    #[test]
    fn webauthn_checks_origin_and_frames() {
        let get = json!({ "type": "get", "publicKey": { "challenge": "abc" } });
        let s = web("https://example.com/login");
        match run(&s, &PageMessage::WebauthnRequest(get.clone())) {
            Ok(Accepted::WebauthnRequest(r)) => assert_eq!(r.rp_id, "example.com"),
            other => panic!("{other:?}"),
        }
        let mut insecure = web("http://example.com/");
        insecure.top_origin = insecure.origin.clone();
        match run(&insecure, &PageMessage::WebauthnRequest(get.clone())) {
            Err(Refusal::WebAuthn(e)) => assert_eq!(e.name, "SecurityError"),
            other => panic!("{other:?}"),
        }
        let mut sub = web("https://pay.example.com/");
        sub.is_main_frame = false;
        sub.top_origin = Some("https://example.com".into());
        match run(&sub, &PageMessage::WebauthnRequest(get.clone())) {
            Err(Refusal::WebAuthn(e)) => assert_eq!(e.name, "NotAllowedError"),
            other => panic!("{other:?}"),
        }
        let mut ui = web("https://example.com/");
        ui.is_tab = false;
        assert!(matches!(run(&ui, &PageMessage::WebauthnRequest(get)), Err(Refusal::WebAuthn(_))));
    }

    #[test]
    fn notes_take_the_document_from_the_address() {
        let s = notes_sender();
        assert_eq!(
            run(&s, &PageMessage::NotesLoad),
            Ok(Accepted::Notes { doc: NoteRef { kind: notes::NoteKind::Note, id: ID.into() }, op: NotesOp::Load })
        );
        assert!(run(&s, &PageMessage::NotesRemove).is_ok());
        // A web page, a lookalike page or an app page without a route is refused.
        for url in [
            format!("https://evil.example/index.html#note/{ID}"),
            "signet://app/index.html".to_owned(),
            format!("signet://app/other.html#note/{ID}"),
        ] {
            assert_eq!(run(&web(&url), &PageMessage::NotesLoad), Err(Refusal::NotADocumentPage), "{url}");
        }
        assert_eq!(run(&notes_sender_with(|s| s.is_private = true), &PageMessage::NotesLoad), Err(Refusal::PrivateWindow));
    }

    #[test]
    fn notes_save_is_cleaned_or_refused() {
        let s = notes_sender();
        let ok = run(&s, &PageMessage::NotesSave(json!({ "title": " t ", "html": "<p>a</p>" })));
        assert_eq!(
            ok,
            Ok(Accepted::Notes {
                doc: NoteRef { kind: notes::NoteKind::Note, id: ID.into() },
                op: NotesOp::Save(NotesPatch::Note { title: "t".into(), html: "<p>a</p>".into() })
            })
        );
        let big = json!({ "html": "a".repeat(notes::NOTE_MAX + 1) });
        assert_eq!(run(&s, &PageMessage::NotesSave(big)), Err(Refusal::SaveRefused));
        assert_eq!(run(&s, &PageMessage::NotesSave(json!("x"))), Err(Refusal::SaveRefused));
        // An easel document refuses a note patch without items.
        let easel = Sender::main_frame(&format!("signet://app/index.html#easel/{ID}"), false);
        assert_eq!(run(&easel, &PageMessage::NotesSave(json!({ "title": "t", "html": "x" }))), Err(Refusal::SaveRefused));
        assert!(run(&easel, &PageMessage::NotesSave(json!({ "title": "t", "items": [] }))).is_ok());
    }

    #[test]
    fn refusal_replies_match_electron() {
        let rid = "r";
        let data = |m: &PageMessage, r: &Refusal| match refusal_reply(rid, m, r) {
            Some(Reply::Ok { data, .. }) => data,
            other => panic!("{other:?}"),
        };
        assert_eq!(data(&PageMessage::SwipeCan, &Refusal::NotATab), json!({ "enabled": false, "back": false, "forward": false }));
        assert_eq!(data(&PageMessage::PwAccounts, &Refusal::BadOrigin), json!([]));
        assert_eq!(data(&PageMessage::PwFill { id: "1".into() }, &Refusal::BadOrigin), Value::Null);
        assert_eq!(data(&PageMessage::AfMenu { kind: AutofillKind::Card }, &Refusal::InsecureOrigin), Value::Null);
        assert_eq!(data(&PageMessage::NotesSave(json!({})), &Refusal::SaveRefused), json!(false));
        let w = data(&PageMessage::WebauthnRequest(json!({})), &Refusal::NotATab);
        assert_eq!(w["error"]["name"], "NotAllowedError");
        assert_eq!(refusal_reply(rid, &PageMessage::NotesLoad, &Refusal::NotADocumentPage), Some(Reply::err(rid, "Call refused")));
        assert_eq!(refusal_reply(rid, &PageMessage::PageSubmit, &Refusal::NotATab), None);
        assert_eq!(refusal_reply(rid, &PageMessage::PwSubmit { username: String::new(), password: String::new() }, &Refusal::NotATab), None);
    }
}

//! Typed messages of every page channel (port of `PAGE_IPC` in `shared/ipc/contract.ts`).
//!
//! Page to browser: [`parse`] turns `(channel, json)` as received from `window.__signetPost` into an [`Incoming`].
//! Nothing here is trusted: run [`crate::validate::validate`] on the result before acting.
//! Browser to page: [`Reply`] (answer to a request) and [`PageEvent`] (pushed to notes pages).

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::bridge::{self, REPLY_CHANNEL};

/// Channel names, as the JS sends them.
pub mod channel {
    pub const SWIPE_CAN: &str = "swipe:can";
    pub const SWIPE_NAVIGATE: &str = "swipe:navigate";
    pub const PAGE_SUBMIT: &str = "page:submit";
    pub const PW_ACCOUNTS: &str = "pw:accounts";
    pub const PW_FILL: &str = "pw:fill";
    pub const PW_SUBMIT: &str = "pw:submit";
    pub const AF_MENU: &str = "af:menu";
    pub const AF_FILL: &str = "af:fill";
    pub const AF_MANAGE: &str = "af:manage";
    pub const AF_SUBMIT: &str = "af:submit";
    pub const WEBAUTHN_REQUEST: &str = "webauthn:request";
    pub const WEBAUTHN_ABORT: &str = "webauthn:abort";
    pub const NOTES_LOCALE: &str = "notes:locale";
    pub const NOTES_APPEARANCE: &str = "notes:appearance";
    pub const NOTES_LOAD: &str = "notes:load";
    pub const NOTES_SAVE: &str = "notes:save";
    pub const NOTES_REMOVE: &str = "notes:remove";
    /// Events pushed to notes pages.
    pub const EVENT_LOCALE: &str = "locale";
    pub const EVENT_APPEARANCE: &str = "appearance";
}

/// Size limits of the raw JSON received on a channel (bytes), checked before parsing.
pub fn max_payload_len(channel: &str) -> usize {
    match channel {
        channel::NOTES_SAVE => 40 * 1024 * 1024,
        channel::WEBAUTHN_REQUEST => 1024 * 1024,
        channel::AF_SUBMIT => 64 * 1024,
        _ => 16 * 1024,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Back,
    Forward,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AutofillKind {
    Address,
    Card,
}

/// A message from a page, one variant per channel. The wire form is `{"channel": ..., "data": ...}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "channel", content = "data")]
pub enum PageMessage {
    /// Request. Reply: [`SwipeState`].
    #[serde(rename = "swipe:can")]
    SwipeCan,
    #[serde(rename = "swipe:navigate")]
    SwipeNavigate(Direction),
    /// A real form submission happened in the page.
    #[serde(rename = "page:submit")]
    PageSubmit,
    /// Request. Reply: `Vec<Account>`.
    #[serde(rename = "pw:accounts")]
    PwAccounts,
    /// Request. Reply: `Option<Credentials>`.
    #[serde(rename = "pw:fill")]
    PwFill { id: String },
    #[serde(rename = "pw:submit")]
    PwSubmit { username: String, password: String },
    /// Request. Reply: `Option<AutofillMenu>`.
    #[serde(rename = "af:menu")]
    AfMenu { kind: AutofillKind },
    /// Request. Reply: `Option<AddressFill>` or `Option<CardFill>` according to the kind.
    #[serde(rename = "af:fill")]
    AfFill { kind: AutofillKind, id: String },
    #[serde(rename = "af:manage")]
    AfManage { kind: AutofillKind },
    /// `{ address?: {...}, card?: {...} }`, cleaned by the autofill feature.
    #[serde(rename = "af:submit")]
    AfSubmit(Value),
    /// Request. Reply: [`WebAuthnAnswer`]. The payload is validated by [`crate::webauthn_request`].
    #[serde(rename = "webauthn:request")]
    WebauthnRequest(Value),
    #[serde(rename = "webauthn:abort")]
    WebauthnAbort,
    /// Request. Reply: the locale code (string).
    #[serde(rename = "notes:locale")]
    NotesLocale,
    /// Request. Reply: the appearance (JSON, type of `signet-core`).
    #[serde(rename = "notes:appearance")]
    NotesAppearance,
    /// Request. Reply: the notes environment `{ kind, id, doc, hue }`.
    #[serde(rename = "notes:load")]
    NotesLoad,
    /// Request. Reply: `bool`.
    #[serde(rename = "notes:save")]
    NotesSave(Value),
    /// Request. Reply: null.
    #[serde(rename = "notes:remove")]
    NotesRemove,
}

impl PageMessage {
    /// Channel name of the message.
    pub fn channel(&self) -> &'static str {
        match self {
            Self::SwipeCan => channel::SWIPE_CAN,
            Self::SwipeNavigate(_) => channel::SWIPE_NAVIGATE,
            Self::PageSubmit => channel::PAGE_SUBMIT,
            Self::PwAccounts => channel::PW_ACCOUNTS,
            Self::PwFill { .. } => channel::PW_FILL,
            Self::PwSubmit { .. } => channel::PW_SUBMIT,
            Self::AfMenu { .. } => channel::AF_MENU,
            Self::AfFill { .. } => channel::AF_FILL,
            Self::AfManage { .. } => channel::AF_MANAGE,
            Self::AfSubmit(_) => channel::AF_SUBMIT,
            Self::WebauthnRequest(_) => channel::WEBAUTHN_REQUEST,
            Self::WebauthnAbort => channel::WEBAUTHN_ABORT,
            Self::NotesLocale => channel::NOTES_LOCALE,
            Self::NotesAppearance => channel::NOTES_APPEARANCE,
            Self::NotesLoad => channel::NOTES_LOAD,
            Self::NotesSave(_) => channel::NOTES_SAVE,
            Self::NotesRemove => channel::NOTES_REMOVE,
        }
    }

    /// True for the channels the JS calls with `request` (a [`Reply`] is expected), false for fire and forget ones.
    pub fn expects_reply(&self) -> bool {
        matches!(
            self,
            Self::SwipeCan
                | Self::PwAccounts
                | Self::PwFill { .. }
                | Self::AfMenu { .. }
                | Self::AfFill { .. }
                | Self::WebauthnRequest(_)
                | Self::NotesLocale
                | Self::NotesAppearance
                | Self::NotesLoad
                | Self::NotesSave(_)
                | Self::NotesRemove
        )
    }
}

/// A parsed page message with the id to answer to (requests only).
#[derive(Debug, Clone, PartialEq)]
pub struct Incoming {
    pub rid: Option<String>,
    pub message: PageMessage,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// Unknown channel.
    UnknownChannel,
    /// Raw JSON larger than [`max_payload_len`].
    TooLarge,
    /// Not the `{rid?, data}` envelope.
    BadEnvelope,
    /// `data` does not fit the channel (wrong type, unknown direction...).
    BadData,
    /// A request without `rid`, or a send with one.
    BadRid,
}

/// Parses what `window.__signetPost(channel, json)` delivered.
pub fn parse(channel: &str, json: &str) -> Result<Incoming, ParseError> {
    if json.len() > max_payload_len(channel) {
        return Err(ParseError::TooLarge);
    }
    let env: Value = serde_json::from_str(json).map_err(|_| ParseError::BadEnvelope)?;
    let Value::Object(mut env) = env else { return Err(ParseError::BadEnvelope) };
    let rid = match env.remove("rid") {
        None | Some(Value::Null) => None,
        // Bounded: ids are generated by the bridge (about 20 characters).
        Some(Value::String(s)) if !s.is_empty() && s.len() <= 64 => Some(s),
        Some(_) => return Err(ParseError::BadEnvelope),
    };
    let data = env.remove("data").unwrap_or(Value::Null);
    let mut tagged = Map::new();
    tagged.insert("channel".into(), Value::String(channel.to_owned()));
    tagged.insert("data".into(), data);
    let message: PageMessage = match serde_json::from_value(Value::Object(tagged)) {
        Ok(m) => m,
        Err(_) => {
            return Err(if is_known(channel) { ParseError::BadData } else { ParseError::UnknownChannel });
        }
    };
    if message.expects_reply() != rid.is_some() {
        return Err(ParseError::BadRid);
    }
    Ok(Incoming { rid, message })
}

fn is_known(channel: &str) -> bool {
    use channel::*;
    [
        SWIPE_CAN, SWIPE_NAVIGATE, PAGE_SUBMIT, PW_ACCOUNTS, PW_FILL, PW_SUBMIT, AF_MENU, AF_FILL, AF_MANAGE, AF_SUBMIT,
        WEBAUTHN_REQUEST, WEBAUTHN_ABORT, NOTES_LOCALE, NOTES_APPEARANCE, NOTES_LOAD, NOTES_SAVE, NOTES_REMOVE,
    ]
    .contains(&channel)
}

// ───────────── browser to page ─────────────

/// Answer to a request. `to_dispatch` gives the channel and JSON to hand to the frame.
#[derive(Debug, Clone, PartialEq)]
pub enum Reply {
    Ok { rid: String, data: Value },
    Err { rid: String, error: String },
}

impl Reply {
    pub fn ok<T: Serialize>(rid: &str, data: &T) -> Self {
        match serde_json::to_value(data) {
            Ok(data) => Self::Ok { rid: rid.to_owned(), data },
            Err(_) => Self::err(rid, "error"),
        }
    }

    pub fn err(rid: &str, error: &str) -> Self {
        Self::Err { rid: rid.to_owned(), error: error.to_owned() }
    }

    /// `(channel, json)` for `window.__signetDispatch`.
    pub fn to_dispatch(&self) -> (&'static str, String) {
        let v = match self {
            Self::Ok { rid, data } => json!({ "rid": rid, "ok": true, "data": data }),
            Self::Err { rid, error } => json!({ "rid": rid, "ok": false, "error": error }),
        };
        (REPLY_CHANNEL, v.to_string())
    }

    /// JS to execute in the frame.
    pub fn to_js(&self) -> String {
        let (ch, json) = self.to_dispatch();
        bridge::dispatch_js(ch, &json)
    }
}

/// Event pushed to a notes page.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum PageEvent {
    /// Interface language code.
    Locale(String),
    /// Appearance object (type of `signet-core`).
    Appearance(Value),
}

impl PageEvent {
    pub fn to_dispatch(&self) -> (&'static str, String) {
        match self {
            Self::Locale(l) => (channel::EVENT_LOCALE, Value::String(l.clone()).to_string()),
            Self::Appearance(a) => (channel::EVENT_APPEARANCE, a.to_string()),
        }
    }

    pub fn to_js(&self) -> String {
        let (ch, json) = self.to_dispatch();
        bridge::dispatch_js(ch, &json)
    }
}

// ───────────── reply payloads ─────────────

/// Reply of `swipe:can`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwipeState {
    pub enabled: bool,
    pub back: bool,
    pub forward: bool,
}

impl SwipeState {
    pub const OFF: Self = Self { enabled: false, back: false, forward: false };
}

/// Item of the reply of `pw:accounts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub username: String,
}

/// Reply of `pw:fill`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Credentials {
    pub username: String,
    pub password: String,
}

/// Row of an autofill dropdown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MenuItem {
    pub id: String,
    pub label: String,
    pub detail: String,
    /// Lowercase text the page matches what the user typed against.
    #[serde(rename = "match")]
    pub matching: String,
}

/// Reply of `af:menu`: at most 20 items (the page drops anything larger).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutofillMenu {
    pub kind: AutofillKind,
    pub title: String,
    pub manage: String,
    pub items: Vec<MenuItem>,
}

/// Reply of `af:fill` for an address (`AddressInput` fields, camelCase).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddressFill {
    pub address: Map<String, Value>,
}

/// Reply of `af:fill` for a card. The security code is never sent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CardFill {
    pub network: String,
    pub holder: String,
    pub number: String,
    pub exp_month: u32,
    pub exp_year: u32,
}

/// Error of a passkey request, as a DOMException name and message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebAuthnError {
    pub name: String,
    pub message: String,
}

/// Reply of `webauthn:request`: an error the page rejects with, or the credential (plain data, `CredentialJson`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WebAuthnAnswer {
    Error(WebAuthnError),
    Credential(Value),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(ch: &str, json: &str) -> Result<Incoming, ParseError> {
        parse(ch, json)
    }

    #[test]
    fn parses_sends_and_requests() {
        let m = p("swipe:navigate", r#"{"data":"back"}"#).unwrap();
        assert_eq!(m, Incoming { rid: None, message: PageMessage::SwipeNavigate(Direction::Back) });
        let m = p("swipe:can", r#"{"rid":"abc","data":null}"#).unwrap();
        assert_eq!(m.rid.as_deref(), Some("abc"));
        assert_eq!(m.message, PageMessage::SwipeCan);
        let m = p("pw:submit", r#"{"data":{"username":"a","password":"b"}}"#).unwrap();
        assert_eq!(m.message, PageMessage::PwSubmit { username: "a".into(), password: "b".into() });
        let m = p("af:fill", r#"{"rid":"x","data":{"kind":"card","id":"7"}}"#).unwrap();
        assert_eq!(m.message, PageMessage::AfFill { kind: AutofillKind::Card, id: "7".into() });
    }

    #[test]
    fn rejects_bad_directions_and_types() {
        assert_eq!(p("swipe:navigate", r#"{"data":"up"}"#), Err(ParseError::BadData));
        assert_eq!(p("pw:submit", r#"{"data":{"username":1,"password":"b"}}"#), Err(ParseError::BadData));
        assert_eq!(p("pw:fill", r#"{"rid":"x","data":{"id":5}}"#), Err(ParseError::BadData));
        assert_eq!(p("af:menu", r#"{"rid":"x","data":{"kind":"passport"}}"#), Err(ParseError::BadData));
    }

    #[test]
    fn rejects_unknown_channels_and_envelopes() {
        assert_eq!(p("signet-crx", r#"{"data":1}"#), Err(ParseError::UnknownChannel));
        assert_eq!(p("swipe:can", "not json"), Err(ParseError::BadEnvelope));
        assert_eq!(p("swipe:can", "[1]"), Err(ParseError::BadEnvelope));
        assert_eq!(p("swipe:can", r#"{"rid":5,"data":null}"#), Err(ParseError::BadEnvelope));
    }

    #[test]
    fn requests_need_a_rid_and_sends_must_not_have_one() {
        assert_eq!(p("swipe:can", r#"{"data":null}"#), Err(ParseError::BadRid));
        assert_eq!(p("page:submit", r#"{"rid":"a","data":null}"#), Err(ParseError::BadRid));
        assert_eq!(p("pw:accounts", r#"{"rid":"","data":null}"#), Err(ParseError::BadEnvelope));
    }

    #[test]
    fn limits_sizes() {
        let big = format!(r#"{{"data":"{}"}}"#, "a".repeat(20_000));
        assert_eq!(p("swipe:navigate", &big), Err(ParseError::TooLarge));
        let save = format!(r#"{{"rid":"a","data":{{"title":"t","html":"{}"}}}}"#, "a".repeat(100_000));
        assert!(p("notes:save", &save).is_ok());
    }

    #[test]
    fn every_channel_round_trips_its_name() {
        let samples = [
            (r#"{"rid":"r","data":null}"#, "swipe:can"),
            (r#"{"data":"forward"}"#, "swipe:navigate"),
            (r#"{"data":null}"#, "page:submit"),
            (r#"{"rid":"r","data":null}"#, "pw:accounts"),
            (r#"{"rid":"r","data":{"id":"1"}}"#, "pw:fill"),
            (r#"{"data":{"username":"","password":""}}"#, "pw:submit"),
            (r#"{"rid":"r","data":{"kind":"address"}}"#, "af:menu"),
            (r#"{"rid":"r","data":{"kind":"address","id":"1"}}"#, "af:fill"),
            (r#"{"data":{"kind":"card"}}"#, "af:manage"),
            (r#"{"data":{"address":{}}}"#, "af:submit"),
            (r#"{"rid":"r","data":{"type":"get"}}"#, "webauthn:request"),
            (r#"{"data":null}"#, "webauthn:abort"),
            (r#"{"rid":"r","data":null}"#, "notes:locale"),
            (r#"{"rid":"r","data":null}"#, "notes:appearance"),
            (r#"{"rid":"r","data":null}"#, "notes:load"),
            (r#"{"rid":"r","data":{}}"#, "notes:save"),
            (r#"{"rid":"r","data":null}"#, "notes:remove"),
        ];
        for (json, ch) in samples {
            let m = p(ch, json).unwrap_or_else(|e| panic!("{ch}: {e:?}"));
            assert_eq!(m.message.channel(), ch);
        }
    }

    #[test]
    fn replies_and_events_serialize() {
        let r = Reply::ok("r1", &SwipeState { enabled: true, back: true, forward: false });
        let (ch, json) = r.to_dispatch();
        assert_eq!(ch, "reply");
        let v: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["rid"], "r1");
        assert_eq!(v["ok"], true);
        assert_eq!(v["data"]["back"], true);
        let (_, json) = Reply::err("r2", "refused").to_dispatch();
        let v: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["ok"], false);
        assert_eq!(v["error"], "refused");
        assert_eq!(PageEvent::Locale("fr".into()).to_dispatch(), ("locale", "\"fr\"".to_owned()));
    }

    #[test]
    fn webauthn_answer_shape_matches_the_page() {
        let e = WebAuthnAnswer::Error(WebAuthnError { name: "NotAllowedError".into(), message: "m".into() });
        assert_eq!(serde_json::to_value(&e).unwrap(), json!({ "error": { "name": "NotAllowedError", "message": "m" } }));
        let c = WebAuthnAnswer::Credential(json!({ "id": "x" }));
        assert_eq!(serde_json::to_value(&c).unwrap(), json!({ "credential": { "id": "x" } }));
    }

    #[test]
    fn menu_item_uses_match_key() {
        let i = MenuItem { id: "1".into(), label: "l".into(), detail: "d".into(), matching: "m".into() };
        assert_eq!(serde_json::to_value(&i).unwrap()["match"], "m");
    }
}

//! Validation of a WebAuthn request coming from a page, as a browser does it (port of `main/web/webauthn-request.ts`).
//! Pure: the origin and frame information come from the browser process, never from the page.

use serde_json::{Map, Value};
use url::Url;

pub use crate::messages::WebAuthnError;

pub const NOT_ALLOWED_MESSAGE: &str =
    "The operation either timed out or was not allowed. See: https://www.w3.org/TR/webauthn-2/#sctn-privacy-considerations-client.";
const INSECURE_MESSAGE: &str = "The operation is insecure.";
const RPID_MESSAGE: &str = "The relying party ID is not a registrable domain suffix of, nor equal to the current domain.";

/// A request that passed validation.
#[derive(Debug, Clone, PartialEq)]
pub struct WebAuthnRequest {
    /// `get` or `create`.
    pub kind: RequestKind,
    pub mediation: Option<String>,
    /// Effective relying party id (lowercase).
    pub rp_id: String,
    /// Origin of the calling frame, from the browser process.
    pub origin: String,
    /// Options as plain data (BufferSource as base64url strings).
    pub public_key: Map<String, Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestKind {
    Get,
    Create,
}

/// What the browser knows about the sender.
#[derive(Debug, Clone)]
pub struct WebAuthnInput<'a> {
    pub origin: Option<&'a str>,
    pub top_origin: Option<&'a str>,
    pub is_main_frame: bool,
    pub payload: &'a Value,
}

pub fn error(name: &str, message: &str) -> WebAuthnError {
    WebAuthnError { name: name.to_owned(), message: message.to_owned() }
}

fn not_allowed() -> WebAuthnError {
    error("NotAllowedError", NOT_ALLOWED_MESSAGE)
}

/// The answer for a request that cannot even be validated (not from a tab, internal failure).
pub fn not_allowed_error() -> WebAuthnError {
    not_allowed()
}

fn insecure(message: &str) -> WebAuthnError {
    error("SecurityError", message)
}

fn type_error(message: &str) -> WebAuthnError {
    error("TypeError", message)
}

fn is_localhost(host: &str) -> bool {
    host == "localhost" || host.ends_with(".localhost")
}

fn is_ip_host(host: &str) -> bool {
    host.starts_with('[') || host.contains(':') || {
        let parts: Vec<&str> = host.split('.').collect();
        parts.len() == 4 && parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
    }
}

/// Registrable domain (public suffix list, private domains included) of a host, if it has one.
fn has_registrable_domain(host: &str) -> bool {
    psl::domain(host.as_bytes()).is_some()
}

/// Checks the payload's shape (reasonable, not the whole spec); returns an error message or None.
fn malformed(kind: RequestKind, pk: &Map<String, Value>) -> Option<&'static str> {
    if !pk.get("challenge").is_some_and(Value::is_string) {
        return Some("Failed to read the 'challenge' property from the options: not a BufferSource.");
    }
    if kind == RequestKind::Create {
        if !pk.get("rp").is_some_and(Value::is_object) {
            return Some("Failed to read the 'rp' property from the options: not an object.");
        }
        let user_id = pk.get("user").and_then(Value::as_object).and_then(|u| u.get("id"));
        if !user_id.is_some_and(Value::is_string) {
            return Some("Failed to read the 'user' property from the options: missing 'id'.");
        }
        if !pk.get("pubKeyCredParams").is_some_and(Value::is_array) {
            return Some("Failed to read the 'pubKeyCredParams' property from the options: not a sequence.");
        }
    }
    None
}

/// True if `rp_id` is the host or a parent domain of it (label boundary) that is not a public suffix.
fn rp_id_allowed(host: &str, rp_id: &str) -> bool {
    if rp_id == host {
        return !is_ip_host(host) && (is_localhost(host) || has_registrable_domain(host));
    }
    if !host.ends_with(&format!(".{rp_id}")) {
        return false;
    }
    if rp_id == "localhost" {
        return true;
    }
    has_registrable_domain(rp_id)
}

/// Validates a request like a browser (secure origin, rpId, frame, shape): the request, or the error to answer.
pub fn validate_webauthn_request(input: &WebAuthnInput<'_>) -> Result<WebAuthnRequest, WebAuthnError> {
    let origin = match input.origin {
        Some(o) if !o.is_empty() && o != "null" => o,
        _ => return Err(not_allowed()),
    };
    let url = Url::parse(origin).map_err(|_| not_allowed())?;
    let Value::Object(payload) = input.payload else {
        return Err(type_error("The WebAuthn request is malformed."));
    };
    let kind = match payload.get("type").and_then(Value::as_str) {
        Some("get") => RequestKind::Get,
        Some("create") => RequestKind::Create,
        _ => return Err(type_error("The WebAuthn request type is unknown.")),
    };
    let Some(Value::Object(pk)) = payload.get("publicKey") else {
        return Err(type_error("Failed to read the 'publicKey' property from the options: not an object."));
    };
    if let Some(bad) = malformed(kind, pk) {
        return Err(type_error(bad));
    }

    let host = url.host_str().unwrap_or("").to_lowercase();
    let secure = url.scheme() == "https" || (url.scheme() == "http" && is_localhost(&host));
    // `host_str` keeps the brackets of an IPv6 host.
    if !secure || is_ip_host(&host) {
        return Err(insecure(INSECURE_MESSAGE));
    }

    let rp_value = match kind {
        RequestKind::Get => pk.get("rpId"),
        RequestKind::Create => pk.get("rp").and_then(Value::as_object).and_then(|rp| rp.get("id")),
    };
    let rp_id = match rp_value {
        None => host.clone(),
        Some(Value::String(s)) => s.to_lowercase(),
        Some(_) => return Err(type_error("The 'rpId' property is not a string.")),
    };
    if rp_id.is_empty() || !rp_id_allowed(&host, &rp_id) {
        return Err(insecure(RPID_MESSAGE));
    }

    if !input.is_main_frame && Some(origin) != input.top_origin {
        return Err(not_allowed());
    }

    Ok(WebAuthnRequest {
        kind,
        mediation: payload.get("mediation").and_then(Value::as_str).map(str::to_owned),
        rp_id,
        origin: origin.to_owned(),
        public_key: pk.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn get() -> Value {
        json!({ "type": "get", "publicKey": { "challenge": "abc" } })
    }
    fn create(rp: Value) -> Value {
        json!({ "type": "create", "publicKey": { "challenge": "abc", "rp": rp, "user": { "id": "u", "name": "n", "displayName": "n" },
            "pubKeyCredParams": [{ "type": "public-key", "alg": -7 }] } })
    }
    fn get_with(rp_id: Value) -> Value {
        json!({ "type": "get", "publicKey": { "challenge": "abc", "rpId": rp_id } })
    }
    fn run(origin: Option<&str>, payload: &Value) -> Result<WebAuthnRequest, WebAuthnError> {
        validate_webauthn_request(&WebAuthnInput { origin, top_origin: origin, is_main_frame: true, payload })
    }
    fn run_frame(origin: &str, top: &str, main: bool, payload: &Value) -> Result<WebAuthnRequest, WebAuthnError> {
        validate_webauthn_request(&WebAuthnInput { origin: Some(origin), top_origin: Some(top), is_main_frame: main, payload })
    }
    fn err_name(r: Result<WebAuthnRequest, WebAuthnError>) -> String {
        match r {
            Ok(_) => "ok".into(),
            Err(e) => e.name,
        }
    }

    #[test]
    fn accepts_https_and_defaults_rp_id_to_host() {
        let r = run(Some("https://Login.Example.com"), &get()).unwrap();
        assert_eq!(r.rp_id, "login.example.com");
        assert_eq!(r.origin, "https://Login.Example.com");
        assert_eq!(r.kind, RequestKind::Get);
    }

    #[test]
    fn accepts_http_on_localhost_only() {
        assert!(run(Some("http://localhost:3000"), &get()).is_ok());
        assert!(run(Some("http://app.localhost"), &get()).is_ok());
        assert_eq!(err_name(run(Some("http://example.com"), &get())), "SecurityError");
    }

    #[test]
    fn refuses_ip_hosts_and_other_schemes() {
        assert_eq!(err_name(run(Some("https://192.168.1.2"), &get())), "SecurityError");
        assert_eq!(err_name(run(Some("https://[::1]:8443"), &get())), "SecurityError");
        assert_eq!(err_name(run(Some("ftp://example.com"), &get())), "SecurityError");
    }

    #[test]
    fn refuses_a_missing_or_opaque_origin() {
        assert_eq!(err_name(run(None, &get())), "NotAllowedError");
        assert_eq!(err_name(run(Some("null"), &get())), "NotAllowedError");
        assert_eq!(err_name(run(Some(""), &get())), "NotAllowedError");
    }

    #[test]
    fn rp_id_accepts_host_and_registrable_parent() {
        let host = Some("https://login.microsoftonline.com");
        assert_eq!(run(host, &get_with(json!("login.microsoftonline.com"))).unwrap().rp_id, "login.microsoftonline.com");
        assert_eq!(run(host, &get_with(json!("MicrosoftOnline.com"))).unwrap().rp_id, "microsoftonline.com");
        assert_eq!(run(Some("https://foo.github.io"), &get_with(json!("foo.github.io"))).unwrap().rp_id, "foo.github.io");
        assert!(run(Some("http://localhost"), &get_with(json!("localhost"))).is_ok());
    }

    #[test]
    fn rp_id_reads_rp_id_for_create() {
        let host = Some("https://login.microsoftonline.com");
        assert_eq!(run(host, &create(json!({ "id": "microsoftonline.com", "name": "x" }))).unwrap().rp_id, "microsoftonline.com");
        assert_eq!(run(host, &create(json!({ "name": "x" }))).unwrap().rp_id, "login.microsoftonline.com");
        assert_eq!(err_name(run(host, &create(json!({ "id": "evil.com", "name": "x" })))), "SecurityError");
    }

    #[test]
    fn rp_id_refuses_public_suffixes() {
        assert_eq!(err_name(run(Some("https://example.com"), &get_with(json!("com")))), "SecurityError");
        assert_eq!(err_name(run(Some("https://example.co.uk"), &get_with(json!("co.uk")))), "SecurityError");
        assert_eq!(err_name(run(Some("https://foo.github.io"), &get_with(json!("github.io")))), "SecurityError");
    }

    #[test]
    fn rp_id_refuses_unrelated_domains_and_partial_labels() {
        let host = Some("https://login.microsoftonline.com");
        assert_eq!(err_name(run(host, &get_with(json!("example.com")))), "SecurityError");
        assert_eq!(err_name(run(Some("https://evil-example.com"), &get_with(json!("example.com")))), "SecurityError");
        assert_eq!(err_name(run(Some("https://example.com"), &get_with(json!("sub.example.com")))), "SecurityError");
        assert_eq!(err_name(run(Some("https://example.com"), &get_with(json!("")))), "SecurityError");
    }

    #[test]
    fn rp_id_must_be_a_string() {
        assert_eq!(err_name(run(Some("https://login.microsoftonline.com"), &get_with(json!(42)))), "TypeError");
    }

    #[test]
    fn frames() {
        let cross = run_frame("https://pay.example.com", "https://example.com", false, &get());
        assert_eq!(err_name(cross), "NotAllowedError");
        assert!(run_frame("https://example.com", "https://example.com", false, &get()).is_ok());
    }

    #[test]
    fn malformed_payloads_give_a_type_error() {
        let cases = [
            json!("x"),
            json!(null),
            json!([]),
            json!({ "type": "delete", "publicKey": { "challenge": "a" } }),
            json!({ "type": "get" }),
            json!({ "type": "get", "publicKey": "x" }),
            json!({ "type": "get", "publicKey": {} }),
            json!({ "type": "get", "publicKey": { "challenge": 1 } }),
            json!({ "type": "create", "publicKey": { "challenge": "a", "user": { "id": "u" }, "pubKeyCredParams": [] } }),
            json!({ "type": "create", "publicKey": { "challenge": "a", "rp": {}, "user": {}, "pubKeyCredParams": [] } }),
            json!({ "type": "create", "publicKey": { "challenge": "a", "rp": {}, "pubKeyCredParams": [] } }),
            json!({ "type": "create", "publicKey": { "challenge": "a", "rp": {}, "user": { "id": "u" }, "pubKeyCredParams": {} } }),
        ];
        for c in cases {
            assert_eq!(err_name(run(Some("https://example.com"), &c)), "TypeError", "{c}");
        }
    }

    #[test]
    fn keeps_mediation() {
        let p = json!({ "type": "get", "mediation": "conditional", "publicKey": { "challenge": "abc" } });
        assert_eq!(run(Some("https://example.com"), &p).unwrap().mediation.as_deref(), Some("conditional"));
    }
}

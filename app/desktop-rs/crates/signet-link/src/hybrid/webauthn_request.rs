//! Validation of a WebAuthn request coming from a page, as a browser does it
//! (port of app/desktop/src/main/web/webauthn-request.ts).
//!
//! The page script (injected in CEF) sends `{ type, mediation?, publicKey }` where every BufferSource of `publicKey`
//! is a base64url string. The origin and frame data come from the browser process, never from the page.

use serde::Serialize;
use serde_json::{Map, Value};
use url::Url;

use super::qr::RequestType;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WebAuthnError {
    /// `NotAllowedError`, `SecurityError`, `TypeError` or `InvalidStateError`: the page rejects with a DOMException of that name.
    pub name: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WebAuthnRequest {
    pub ty: RequestType,
    pub mediation: Option<String>,
    /// Effective relying party id (lowercase).
    pub rp_id: String,
    /// Origin of the calling frame, from the browser process.
    pub origin: String,
    /// Options converted to plain data (BufferSource as base64url strings).
    pub public_key: Map<String, Value>,
}

pub struct WebAuthnInput<'a> {
    pub origin: Option<&'a str>,
    pub top_origin: Option<&'a str>,
    pub is_main_frame: bool,
    pub payload: &'a Value,
}

pub const NOT_ALLOWED_MESSAGE: &str =
    "The operation either timed out or was not allowed. See: https://www.w3.org/TR/webauthn-2/#sctn-privacy-considerations-client.";
const INSECURE_MESSAGE: &str = "The operation is insecure.";
const RPID_MESSAGE: &str = "The relying party ID is not a registrable domain suffix of, nor equal to the current domain.";

fn err(name: &str, message: &str) -> WebAuthnError {
    WebAuthnError { name: name.into(), message: message.into() }
}
pub fn not_allowed() -> WebAuthnError {
    err("NotAllowedError", NOT_ALLOWED_MESSAGE)
}
fn insecure(message: &str) -> WebAuthnError {
    err("SecurityError", message)
}
fn type_error(message: &str) -> WebAuthnError {
    err("TypeError", message)
}

fn is_localhost(host: &str) -> bool {
    host == "localhost" || host.ends_with(".localhost")
}

fn is_ip_host(host: &str) -> bool {
    if host.starts_with('[') || host.contains(':') {
        return true;
    }
    let parts: Vec<&str> = host.split('.').collect();
    parts.len() == 4 && parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

/// Registrable domain of a host (None for a public suffix or an unknown shape). Private suffixes (github.io) count.
fn registrable(host: &str) -> Option<&str> {
    psl::domain_str(host)
}

/// Checks the payload's shape (reasonable, not the whole spec); returns an error message or None.
fn malformed(ty: RequestType, pk: &Map<String, Value>) -> Option<&'static str> {
    if !pk.get("challenge").is_some_and(Value::is_string) {
        return Some("Failed to read the 'challenge' property from the options: not a BufferSource.");
    }
    if ty == RequestType::Create {
        if !pk.get("rp").is_some_and(Value::is_object) {
            return Some("Failed to read the 'rp' property from the options: not an object.");
        }
        let user_ok = pk.get("user").and_then(Value::as_object).is_some_and(|u| u.get("id").is_some_and(Value::is_string));
        if !user_ok {
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
        return !is_ip_host(host) && (is_localhost(host) || registrable(host).is_some());
    }
    if !host.ends_with(&format!(".{rp_id}")) {
        return false;
    }
    if rp_id == "localhost" {
        return true;
    }
    registrable(rp_id).is_some()
}

/// Validates a request like a browser (secure origin, rpId, frame, shape): the validated request, or the error to answer.
pub fn validate_webauthn_request(input: WebAuthnInput<'_>) -> Result<WebAuthnRequest, WebAuthnError> {
    let origin = match input.origin {
        Some(o) if !o.is_empty() && o != "null" => o,
        _ => return Err(not_allowed()),
    };
    let url = Url::parse(origin).map_err(|_| not_allowed())?;
    let Some(payload) = input.payload.as_object() else { return Err(type_error("The WebAuthn request is malformed.")) };
    let ty = match payload.get("type").and_then(Value::as_str).and_then(RequestType::parse) {
        Some(t) => t,
        None => return Err(type_error("The WebAuthn request type is unknown.")),
    };
    let Some(pk) = payload.get("publicKey").and_then(Value::as_object) else {
        return Err(type_error("Failed to read the 'publicKey' property from the options: not an object."));
    };
    if let Some(bad) = malformed(ty, pk) {
        return Err(type_error(bad));
    }

    let host = url.host_str().unwrap_or("").to_lowercase();
    let secure = url.scheme() == "https" || (url.scheme() == "http" && is_localhost(&host));
    if !secure || is_ip_host(&host) {
        return Err(insecure(INSECURE_MESSAGE));
    }

    let declared: Option<&Value> = match ty {
        RequestType::Get => pk.get("rpId"),
        RequestType::Create => pk.get("rp").and_then(|r| r.get("id")),
    };
    let rp_id = match declared {
        None => host.clone(),
        Some(Value::String(s)) => s.clone(),
        Some(_) => return Err(type_error("The 'rpId' property is not a string.")),
    }
    .to_lowercase();
    if rp_id.is_empty() || !rp_id_allowed(&host, &rp_id) {
        return Err(insecure(RPID_MESSAGE));
    }

    if !input.is_main_frame && Some(origin) != input.top_origin {
        return Err(not_allowed());
    }

    Ok(WebAuthnRequest {
        ty,
        mediation: payload.get("mediation").and_then(Value::as_str).map(String::from),
        rp_id,
        origin: origin.to_string(),
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
        json!({ "type": "create", "publicKey": { "challenge": "abc", "rp": rp, "user": { "id": "u", "name": "n", "displayName": "n" }, "pubKeyCredParams": [{ "type": "public-key", "alg": -7 }] } })
    }
    fn get_with(rp_id: Value) -> Value {
        json!({ "type": "get", "publicKey": { "challenge": "abc", "rpId": rp_id } })
    }
    fn run(origin: Option<&str>, payload: &Value) -> Result<WebAuthnRequest, WebAuthnError> {
        run_frame(origin, payload, origin, true)
    }
    fn run_frame(origin: Option<&str>, payload: &Value, top: Option<&str>, main: bool) -> Result<WebAuthnRequest, WebAuthnError> {
        validate_webauthn_request(WebAuthnInput { origin, top_origin: top, is_main_frame: main, payload })
    }
    fn name(r: Result<WebAuthnRequest, WebAuthnError>) -> String {
        match r {
            Ok(_) => "ok".into(),
            Err(e) => e.name,
        }
    }

    #[test]
    fn origin_accepts_https_and_defaults_rp_id_to_host() {
        let r = run(Some("https://Login.Example.com"), &get()).unwrap();
        assert_eq!(r.rp_id, "login.example.com");
        assert_eq!(r.origin, "https://Login.Example.com");
    }

    #[test]
    fn origin_accepts_http_on_localhost_only() {
        assert!(run(Some("http://localhost:3000"), &get()).is_ok());
        assert!(run(Some("http://app.localhost"), &get()).is_ok());
        assert_eq!(name(run(Some("http://example.com"), &get())), "SecurityError");
    }

    #[test]
    fn origin_refuses_ip_hosts_and_other_schemes() {
        assert_eq!(name(run(Some("https://192.168.1.2"), &get())), "SecurityError");
        assert_eq!(name(run(Some("https://[::1]:8443"), &get())), "SecurityError");
        assert_eq!(name(run(Some("ftp://example.com"), &get())), "SecurityError");
    }

    #[test]
    fn origin_refuses_missing_or_opaque() {
        assert_eq!(name(run(None, &get())), "NotAllowedError");
        assert_eq!(name(run(Some("null"), &get())), "NotAllowedError");
        assert_eq!(name(run(Some(""), &get())), "NotAllowedError");
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
        assert_eq!(name(run(host, &create(json!({ "id": "evil.com", "name": "x" })))), "SecurityError");
    }

    #[test]
    fn rp_id_refuses_public_suffixes() {
        assert_eq!(name(run(Some("https://example.com"), &get_with(json!("com")))), "SecurityError");
        assert_eq!(name(run(Some("https://example.co.uk"), &get_with(json!("co.uk")))), "SecurityError");
        assert_eq!(name(run(Some("https://foo.github.io"), &get_with(json!("github.io")))), "SecurityError");
    }

    #[test]
    fn rp_id_refuses_unrelated_domains_and_partial_labels() {
        let host = Some("https://login.microsoftonline.com");
        assert_eq!(name(run(host, &get_with(json!("example.com")))), "SecurityError");
        assert_eq!(name(run(Some("https://evil-example.com"), &get_with(json!("example.com")))), "SecurityError");
        assert_eq!(name(run(Some("https://example.com"), &get_with(json!("sub.example.com")))), "SecurityError");
        assert_eq!(name(run(Some("https://example.com"), &get_with(json!("")))), "SecurityError");
        assert_eq!(name(run(host, &get_with(json!(42)))), "TypeError");
    }

    #[test]
    fn frames() {
        let a = get();
        assert_eq!(name(run_frame(Some("https://pay.example.com"), &a, Some("https://example.com"), false)), "NotAllowedError");
        assert!(run_frame(Some("https://example.com"), &a, Some("https://example.com"), false).is_ok());
    }

    #[test]
    fn malformed_payloads_give_a_type_error() {
        let o = Some("https://example.com");
        for (label, payload) in [
            ("not an object", json!("x")),
            ("null", json!(null)),
            ("an array", json!([])),
            ("unknown type", json!({ "type": "delete", "publicKey": { "challenge": "a" } })),
            ("no publicKey", json!({ "type": "get" })),
            ("publicKey not an object", json!({ "type": "get", "publicKey": "x" })),
            ("no challenge", json!({ "type": "get", "publicKey": {} })),
            ("challenge not a string", json!({ "type": "get", "publicKey": { "challenge": 1 } })),
            ("create without rp", json!({ "type": "create", "publicKey": { "challenge": "a", "user": { "id": "u" }, "pubKeyCredParams": [] } })),
            ("create without user.id", json!({ "type": "create", "publicKey": { "challenge": "a", "rp": {}, "user": {}, "pubKeyCredParams": [] } })),
            ("create without user", json!({ "type": "create", "publicKey": { "challenge": "a", "rp": {}, "pubKeyCredParams": [] } })),
            ("pubKeyCredParams not an array", json!({ "type": "create", "publicKey": { "challenge": "a", "rp": {}, "user": { "id": "u" }, "pubKeyCredParams": {} } })),
        ] {
            assert_eq!(name(run(o, &payload)), "TypeError", "{label}");
        }
    }

    #[test]
    fn mediation_is_kept() {
        let p = json!({ "type": "get", "mediation": "conditional", "publicKey": { "challenge": "abc" } });
        assert_eq!(run(Some("https://example.com"), &p).unwrap().mediation.as_deref(), Some("conditional"));
    }
}

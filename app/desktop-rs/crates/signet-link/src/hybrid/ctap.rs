//! CTAP2 side of a passkey request answered by a phone: builds authenticatorMakeCredential / authenticatorGetAssertion from
//! the WebAuthn options like Chromium does, then checks the response and turns it into a credential (plain data, base64url,
//! the shape of WebAuthn's toJSON()). Checked against Chromium (device/fido/ctap_make_credential_request.cc,
//! ctap_get_assertion_request.cc, fido_device_authenticator.cc, make_credential_request_handler.cc,
//! get_assertion_request_handler.cc, device_response_converter.cc, authenticator_data.cc, attested_credential_data.cc,
//! attestation_object.cc, client_data_json.cc, authenticator_common_impl.cc).
//! Contents of requests and responses are never logged.

use serde::Serialize;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use super::cbor::{Cbor, decode_cbor, decode_cbor_prefix, encode_cbor};
use super::qr::RequestType;
use super::webauthn_request::{NOT_ALLOWED_MESSAGE, WebAuthnError, WebAuthnRequest};
use crate::b64;

const CMD_MAKE_CREDENTIAL: u8 = 0x01;
const CMD_GET_ASSERTION: u8 = 0x02;
const STATUS_SUCCESS: u8 = 0x00;
const STATUS_CREDENTIAL_EXCLUDED: u8 = 0x19;

const FLAG_UP: u8 = 0x01;
const FLAG_AT: u8 = 0x40;
const FLAG_ED: u8 = 0x80;
const AUTH_DATA_MIN: usize = 37;
const MAX_CREDENTIAL_ID: usize = 1023;

const ALG_ES256: i128 = -7;
const ALG_EDDSA: i128 = -8;
const ALG_RS256: i128 = -257;

const EXCLUDED_MESSAGE: &str = "The user attempted to register an authenticator that contains one of the credentials already registered with the relying party.";

/// A failure to answer with: `error` goes to the page, `reason` is a log line (no data).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CtapFailure {
    pub error: WebAuthnError,
    pub reason: String,
}

impl std::fmt::Display for CtapFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.reason)
    }
}
impl std::error::Error for CtapFailure {}

fn not_allowed(reason: &str) -> CtapFailure {
    CtapFailure { error: WebAuthnError { name: "NotAllowedError".into(), message: NOT_ALLOWED_MESSAGE.into() }, reason: reason.into() }
}

/// Chrome's mapping of a CTAP status to a DOMException: excluded credential -> InvalidStateError, everything else NotAllowedError.
pub fn map_ctap_status(status: u8) -> WebAuthnError {
    if status == STATUS_CREDENTIAL_EXCLUDED {
        return WebAuthnError { name: "InvalidStateError".into(), message: EXCLUDED_MESSAGE.into() };
    }
    WebAuthnError { name: "NotAllowedError".into(), message: NOT_ALLOWED_MESSAGE.into() }
}

/// The credential as the page gets it, in toJSON() form (binary fields are base64url).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialJson {
    pub id: String,
    pub raw_id: String,
    #[serde(rename = "type")]
    pub ty: &'static str,
    pub authenticator_attachment: &'static str,
    pub response: ResponseJson,
    pub client_extension_results: ClientExtensionResults,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum ResponseJson {
    Assertion(AssertionResponseJson),
    Attestation(AttestationResponseJson),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssertionResponseJson {
    #[serde(rename = "clientDataJSON")]
    pub client_data_json: String,
    pub authenticator_data: String,
    pub signature: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_handle: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttestationResponseJson {
    #[serde(rename = "clientDataJSON")]
    pub client_data_json: String,
    pub attestation_object: String,
    pub authenticator_data: String,
    pub transports: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_key: Option<String>,
    pub public_key_algorithm: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ClientExtensionResults {
    #[serde(rename = "credProps", skip_serializing_if = "Option::is_none")]
    pub cred_props: Option<CredProps>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct CredProps {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rk: Option<bool>,
}

/// JSON string escaping of clientDataJSON: only the quote, the backslash and control characters (lowercase \u00xx).
fn json_string(s: &str) -> String {
    let mut out = String::from("\"");
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u00{:02x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// CollectedClientData exactly as Chrome serializes it for a same-origin frame.
pub fn client_data_json(ty: RequestType, challenge: &[u8], origin: &str) -> String {
    format!(
        "{{\"type\":\"webauthn.{}\",\"challenge\":{},\"origin\":{},\"crossOrigin\":false}}",
        ty.as_str(),
        json_string(&b64::encode(challenge)),
        json_string(origin)
    )
}

/// What the request needs to know about the phone, from its getInfo response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    pub versions: Vec<String>,
    /// options.uv == true: user verification is configured.
    pub uv: bool,
    pub rk: bool,
    pub make_cred_uv_not_rqd: bool,
    pub transports: Option<Vec<String>>,
}

pub fn parse_get_info(bytes: &[u8]) -> Result<DeviceInfo, CtapFailure> {
    let map = decode_cbor(bytes).map_err(|_| not_allowed("invalid getInfo"))?;
    if map.as_map().is_none() {
        return Err(not_allowed("invalid getInfo"));
    }
    let strings = |v: Option<&Cbor>| -> Option<Vec<String>> {
        let a = v?.as_array()?;
        a.iter().map(|x| x.as_text().map(String::from)).collect()
    };
    let options = map.geti(4);
    let flag = |name: &str| options.and_then(|o| o.gets(name)) == Some(&Cbor::Bool(true));
    Ok(DeviceInfo {
        versions: strings(map.geti(1)).unwrap_or_default(),
        uv: flag("uv"),
        rk: flag("rk"),
        make_cred_uv_not_rqd: flag("makeCredUvNotRqd"),
        transports: strings(map.geti(9)),
    })
}

/// base64url like Node's Buffer.from(v, 'base64url'): characters outside the alphabet are ignored.
fn bytes_of(v: Option<&Value>, what: &str) -> Result<Vec<u8>, CtapFailure> {
    let Some(Value::String(s)) = v else { return Err(not_allowed(&format!("{what} is not a buffer"))) };
    let clean: String = s.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect();
    b64::decode(&clean).ok_or_else(|| not_allowed(&format!("{what} is not a buffer")))
}

/// Credential descriptors of type public-key (others are ignored like Blink does); transports are not sent.
fn descriptors(list: Option<&Value>) -> Result<Vec<Vec<u8>>, CtapFailure> {
    let Some(list) = list else { return Ok(vec![]) };
    let Some(items) = list.as_array() else { return Err(not_allowed("credential list is not a sequence")) };
    let mut ids = vec![];
    for d in items {
        let Some(o) = d.as_object() else { return Err(not_allowed("invalid credential descriptor")) };
        if o.get("type").and_then(Value::as_str) == Some("public-key") {
            ids.push(bytes_of(o.get("id"), "credential id")?);
        }
    }
    Ok(ids)
}

fn descriptor(id: &[u8]) -> Cbor {
    Cbor::map(vec![(Cbor::text("id"), Cbor::bytes(id)), (Cbor::text("type"), Cbor::text("public-key"))])
}

/// A request ready to send, with what is needed to check the answer.
#[derive(Debug, Clone)]
pub struct CtapPlan {
    pub ty: RequestType,
    /// Command byte + CBOR.
    pub bytes: Vec<u8>,
    pub client_data_json: Vec<u8>,
    pub rp_id: String,
    pub allow_ids: Vec<Vec<u8>>,
    pub attestation: String,
    /// rk sent to the phone.
    pub resident_key: bool,
    pub cred_props: bool,
    pub info: DeviceInfo,
    /// Names of requested extensions that are not passed to the phone (development log).
    pub skipped: Vec<String>,
}

fn alg_param(alg: i128) -> Cbor {
    Cbor::map(vec![(Cbor::text("alg"), Cbor::Int(alg)), (Cbor::text("type"), Cbor::text("public-key"))])
}

fn as_integer(v: &Value) -> Option<i128> {
    if let Some(i) = v.as_i64() {
        return Some(i as i128);
    }
    let f = v.as_f64()?;
    (f.fract() == 0.0 && f.abs() < 9e15).then_some(f as i128)
}

fn pub_key_cred_params(raw: Option<&Value>) -> Result<Vec<Cbor>, CtapFailure> {
    let Some(arr) = raw.and_then(Value::as_array) else { return Err(not_allowed("pubKeyCredParams is not a sequence")) };
    // An empty list means the defaults (ES256, RS256); a list of unknown types leaves nothing to create.
    if arr.is_empty() {
        return Ok(vec![alg_param(ALG_ES256), alg_param(ALG_RS256)]);
    }
    let params: Vec<Cbor> = arr
        .iter()
        .filter_map(|p| {
            let o = p.as_object()?;
            (o.get("type").and_then(Value::as_str) == Some("public-key")).then_some(())?;
            Some(alg_param(as_integer(o.get("alg")?)?))
        })
        .collect();
    if params.is_empty() {
        return Err(not_allowed("no supported credential type"));
    }
    Ok(params)
}

fn str_of<'a>(m: &'a Map<String, Value>, k: &str) -> Option<&'a str> {
    m.get(k).and_then(Value::as_str)
}

/// Builds the CTAP2 command for a request, once the phone's getInfo is known.
pub fn plan_ctap(request: &WebAuthnRequest, info: &DeviceInfo) -> Result<CtapPlan, CtapFailure> {
    let pk = &request.public_key;
    let challenge = bytes_of(pk.get("challenge"), "challenge")?;
    let json = client_data_json(request.ty, &challenge, &request.origin).into_bytes();
    let hash = Sha256::digest(&json).to_vec();
    let empty = Map::new();
    let extensions = pk.get("extensions").and_then(Value::as_object).unwrap_or(&empty);
    let skipped: Vec<String> = extensions.keys().filter(|k| *k != "credProps").cloned().collect();
    let base = |bytes: Vec<u8>, allow_ids: Vec<Vec<u8>>, attestation: String, resident_key: bool, cred_props: bool| CtapPlan {
        ty: request.ty,
        bytes,
        client_data_json: json.clone(),
        rp_id: request.rp_id.clone(),
        allow_ids,
        attestation,
        resident_key,
        cred_props,
        info: info.clone(),
        skipped: skipped.clone(),
    };

    if request.ty == RequestType::Get {
        let allow_ids = descriptors(pk.get("allowCredentials"))?;
        // Resident credential requests on non-platform authenticators always require UV.
        let wanted = if allow_ids.is_empty() { "required" } else { str_of(pk, "userVerification").unwrap_or("preferred") };
        if wanted == "required" && !info.uv {
            return Err(not_allowed("user verification required but not available"));
        }
        let mut map = vec![(Cbor::int(1), Cbor::text(&request.rp_id)), (Cbor::int(2), Cbor::Bytes(hash))];
        if !allow_ids.is_empty() {
            map.push((Cbor::int(3), Cbor::Array(allow_ids.iter().map(|i| descriptor(i)).collect())));
        }
        if info.uv && wanted != "discouraged" {
            map.push((Cbor::int(5), Cbor::map(vec![(Cbor::text("uv"), Cbor::Bool(true))])));
        }
        let mut bytes = vec![CMD_GET_ASSERTION];
        bytes.extend(encode_cbor(&Cbor::Map(map)).map_err(|_| not_allowed("cannot encode request"))?);
        return Ok(base(bytes, allow_ids, "none".into(), false, false));
    }

    let rp = pk.get("rp").and_then(Value::as_object).unwrap_or(&empty);
    let user = pk.get("user").and_then(Value::as_object).unwrap_or(&empty);
    let selection = pk.get("authenticatorSelection").and_then(Value::as_object).unwrap_or(&empty);
    let resident = match str_of(selection, "residentKey") {
        Some(s) => s,
        None if selection.get("requireResidentKey") == Some(&Value::Bool(true)) => "required",
        None => "discouraged",
    };
    let rk = resident == "required" || (resident == "preferred" && info.rk && info.uv);
    if resident == "required" && !info.rk {
        return Err(not_allowed("resident keys required but not supported"));
    }
    let wanted = if rk { "required" } else { str_of(selection, "userVerification").unwrap_or("preferred") };
    if wanted == "required" && !info.uv {
        return Err(not_allowed("user verification required but not available"));
    }

    let mut rp_map = vec![(Cbor::text("id"), Cbor::text(&request.rp_id))];
    if let Some(n) = str_of(rp, "name") {
        rp_map.push((Cbor::text("name"), Cbor::text(n)));
    }
    // Phones refuse a request without a displayName, even an empty one.
    let mut user_map = vec![
        (Cbor::text("id"), Cbor::Bytes(bytes_of(user.get("id"), "user id")?)),
        (Cbor::text("displayName"), Cbor::text(str_of(user, "displayName").unwrap_or(""))),
    ];
    if let Some(n) = str_of(user, "name") {
        user_map.push((Cbor::text("name"), Cbor::text(n)));
    }
    let mut map = vec![
        (Cbor::int(1), Cbor::Bytes(hash)),
        (Cbor::int(2), Cbor::Map(rp_map)),
        (Cbor::int(3), Cbor::Map(user_map)),
        (Cbor::int(4), Cbor::Array(pub_key_cred_params(pk.get("pubKeyCredParams"))?)),
    ];
    let exclude = descriptors(pk.get("excludeCredentials"))?;
    if !exclude.is_empty() {
        map.push((Cbor::int(5), Cbor::Array(exclude.iter().map(|i| descriptor(i)).collect())));
    }
    let mut options = vec![];
    if rk {
        options.push((Cbor::text("rk"), Cbor::Bool(true)));
    }
    // UV configured on the phone is always used for a registration, unless it says it is not needed.
    if info.uv && !info.make_cred_uv_not_rqd {
        options.push((Cbor::text("uv"), Cbor::Bool(true)));
    }
    if !options.is_empty() {
        map.push((Cbor::int(7), Cbor::Map(options)));
    }
    let mut bytes = vec![CMD_MAKE_CREDENTIAL];
    bytes.extend(encode_cbor(&Cbor::Map(map)).map_err(|_| not_allowed("cannot encode request"))?);
    Ok(base(bytes, vec![], str_of(pk, "attestation").unwrap_or("none").to_string(), rk, extensions.get("credProps") == Some(&Value::Bool(true))))
}

struct AuthData {
    rp_id_hash: Vec<u8>,
    flags: u8,
    attested: Option<(Vec<u8>, Cbor)>,
}

/// Authenticator data: rpIdHash, flags, counter, optional attested credential data, optional extensions (as Chromium parses it).
fn parse_auth_data(buf: &[u8]) -> Result<AuthData, CtapFailure> {
    if buf.len() < AUTH_DATA_MIN {
        return Err(not_allowed("authenticator data too short"));
    }
    let flags = buf[32];
    let mut at = AUTH_DATA_MIN;
    let mut attested = None;
    if flags & FLAG_AT != 0 {
        if buf.len() < at + 18 {
            return Err(not_allowed("truncated attested credential data"));
        }
        let id_len = u16::from_be_bytes([buf[at + 16], buf[at + 17]]) as usize;
        at += 18;
        if id_len == 0 || id_len > MAX_CREDENTIAL_ID || buf.len() < at + id_len {
            return Err(not_allowed("invalid credential id"));
        }
        let id = buf[at..at + id_len].to_vec();
        at += id_len;
        let (key, len) = decode_cbor_prefix(&buf[at..]).map_err(|_| not_allowed("invalid public key"))?;
        if key.as_map().is_none() {
            return Err(not_allowed("invalid public key"));
        }
        at += len;
        attested = Some((id, key));
    }
    if flags & FLAG_ED != 0 {
        let ext = decode_cbor(&buf[at..]).map_err(|_| not_allowed("invalid extensions"))?;
        if ext.as_map().is_none() {
            return Err(not_allowed("invalid extensions"));
        }
    } else if at != buf.len() {
        return Err(not_allowed("trailing authenticator data"));
    }
    Ok(AuthData { rp_id_hash: buf[..32].to_vec(), flags, attested })
}

fn der_len(n: usize, out: &mut Vec<u8>) {
    if n < 0x80 {
        out.push(n as u8);
    } else if n < 0x100 {
        out.extend_from_slice(&[0x81, n as u8]);
    } else {
        out.extend_from_slice(&[0x82, (n >> 8) as u8, n as u8]);
    }
}

fn der(tag: u8, content: &[u8]) -> Vec<u8> {
    let mut out = vec![tag];
    der_len(content.len(), &mut out);
    out.extend_from_slice(content);
    out
}

/// DER INTEGER of an unsigned big-endian value.
fn der_uint(bytes: &[u8]) -> Vec<u8> {
    let trimmed: &[u8] = match bytes.iter().position(|b| *b != 0) {
        Some(i) => &bytes[i..],
        None => &[0],
    };
    let mut content = vec![];
    if trimmed[0] & 0x80 != 0 {
        content.push(0);
    }
    content.extend_from_slice(trimmed);
    der(0x02, &content)
}

/// SPKI DER of a COSE public key (ES256, EdDSA, RS256); None for the DER for anything else, like Chrome.
fn spki_of(key: &Cbor) -> Result<(Option<Vec<u8>>, i64), CtapFailure> {
    let alg = key.geti(3).and_then(Cbor::as_int).ok_or_else(|| not_allowed("invalid public key algorithm"))?;
    let bin = |k: i64| key.geti(k).and_then(Cbor::as_bytes);
    let kty = key.geti(1).and_then(Cbor::as_int);
    let crv = key.geti(-1).and_then(Cbor::as_int);
    let alg_out = alg as i64;
    let der_bytes = if alg == ALG_ES256 && kty == Some(2) && crv == Some(1) {
        match (bin(-2), bin(-3)) {
            (Some(x), Some(y)) if x.len() == 32 && y.len() == 32 => {
                // AlgorithmIdentifier { id-ecPublicKey, prime256v1 } then the BIT STRING of the uncompressed point.
                let mut point = vec![0x00, 0x04];
                point.extend_from_slice(x);
                point.extend_from_slice(y);
                let alg_id = hex_bytes("301306072a8648ce3d020106082a8648ce3d030107");
                // Validate the point is on the curve before emitting it.
                let mut sec1 = vec![0x04];
                sec1.extend_from_slice(x);
                sec1.extend_from_slice(y);
                p256::PublicKey::from_sec1_bytes(&sec1).ok().map(|_| der(0x30, &[alg_id, der(0x03, &point)].concat()))
            }
            _ => None,
        }
    } else if alg == ALG_EDDSA && kty == Some(1) && crv == Some(6) {
        match bin(-2) {
            Some(x) if x.len() == 32 => {
                let alg_id = hex_bytes("300506032b6570");
                let mut bits = vec![0x00];
                bits.extend_from_slice(x);
                Some(der(0x30, &[alg_id, der(0x03, &bits)].concat()))
            }
            _ => None,
        }
    } else if alg == ALG_RS256 && kty == Some(3) {
        match (bin(-1), bin(-2)) {
            (Some(n), Some(e)) if !n.is_empty() && !e.is_empty() => {
                let alg_id = hex_bytes("300d06092a864886f70d0101010500");
                let rsa = der(0x30, &[der_uint(n), der_uint(e)].concat());
                let mut bits = vec![0x00];
                bits.extend_from_slice(&rsa);
                Some(der(0x30, &[alg_id, der(0x03, &bits)].concat()))
            }
            _ => None,
        }
    } else {
        None
    };
    Ok((der_bytes, alg_out))
}

fn hex_bytes(s: &str) -> Vec<u8> {
    hex::decode(s).unwrap_or_default()
}

const KNOWN_TRANSPORTS: [&str; 5] = ["ble", "hybrid", "internal", "nfc", "usb"];

/// Transports as Chrome reports them: the phone's own list plus hybrid, ['hybrid', 'internal'] when it gives none; sorted.
fn transports_of(info: &DeviceInfo) -> Vec<String> {
    let mut set: Vec<String> = match &info.transports {
        Some(t) => t.iter().filter(|x| KNOWN_TRANSPORTS.contains(&x.as_str())).cloned().collect(),
        None => vec!["internal".into()],
    };
    set.push("hybrid".into());
    set.sort();
    set.dedup();
    set
}

fn check_auth_data(plan: &CtapPlan, auth: &AuthData) -> Result<(), CtapFailure> {
    if auth.rp_id_hash != Sha256::digest(plan.rp_id.as_bytes()).to_vec() {
        return Err(not_allowed("rpIdHash mismatch"));
    }
    if auth.flags & FLAG_UP == 0 {
        return Err(not_allowed("user presence flag missing"));
    }
    Ok(())
}

/// Checks the phone's answer to `plan` and builds the credential. Fails with NotAllowedError, or InvalidStateError
/// for an excluded credential.
pub fn parse_ctap_response(plan: &CtapPlan, response: &[u8]) -> Result<CredentialJson, CtapFailure> {
    let Some(&status) = response.first() else { return Err(not_allowed("empty CTAP response")) };
    if status != STATUS_SUCCESS {
        return Err(CtapFailure { error: map_ctap_status(status), reason: format!("CTAP status 0x{status:02x}") });
    }
    let map = decode_cbor(&response[1..]).map_err(|_| not_allowed("invalid CBOR in CTAP response"))?;
    if map.as_map().is_none() {
        return Err(not_allowed("CTAP response is not a map"));
    }
    if plan.ty == RequestType::Get { parse_assertion(plan, &map) } else { parse_attestation(plan, &map) }
}

fn parse_assertion(plan: &CtapPlan, map: &Cbor) -> Result<CredentialJson, CtapFailure> {
    let (Some(auth_bytes), Some(signature)) = (map.geti(2).and_then(Cbor::as_bytes), map.geti(3).and_then(Cbor::as_bytes)) else {
        return Err(not_allowed("missing assertion fields"));
    };
    let auth = parse_auth_data(auth_bytes)?;
    check_auth_data(plan, &auth)?;
    if auth.attested.is_some() {
        return Err(not_allowed("unexpected attested credential data"));
    }

    // The credential is left out when the allow list has a single entry.
    let id: Vec<u8> = match map.geti(1) {
        Some(c @ Cbor::Map(_)) => {
            let raw = c.gets("id").and_then(Cbor::as_bytes);
            match raw {
                Some(r) if !r.is_empty() && r.len() <= MAX_CREDENTIAL_ID => r.to_vec(),
                _ => return Err(not_allowed("invalid credential")),
            }
        }
        None if plan.allow_ids.len() == 1 => plan.allow_ids[0].clone(),
        _ => return Err(not_allowed("missing credential")),
    };
    if !plan.allow_ids.is_empty() && !plan.allow_ids.iter().any(|a| *a == id) {
        return Err(not_allowed("credential not in the allow list"));
    }

    let user_handle: Option<Vec<u8>> = match map.geti(4) {
        None => None,
        Some(u) => Some(u.gets("id").and_then(Cbor::as_bytes).ok_or_else(|| not_allowed("invalid user entity"))?.to_vec()),
    };
    if plan.allow_ids.is_empty() && user_handle.is_none() {
        return Err(not_allowed("user entity missing"));
    }

    let response = AssertionResponseJson {
        client_data_json: b64::encode(&plan.client_data_json),
        authenticator_data: b64::encode(auth_bytes),
        signature: b64::encode(signature),
        user_handle: user_handle.filter(|h| !h.is_empty()).map(|h| b64::encode(&h)),
    };
    Ok(CredentialJson {
        id: b64::encode(&id),
        raw_id: b64::encode(&id),
        ty: "public-key",
        authenticator_attachment: "cross-platform",
        response: ResponseJson::Assertion(response),
        client_extension_results: ClientExtensionResults::default(),
    })
}

fn parse_attestation(plan: &CtapPlan, map: &Cbor) -> Result<CredentialJson, CtapFailure> {
    let (Some(fmt), Some(auth_bytes), Some(att_stmt)) =
        (map.geti(1).and_then(Cbor::as_text), map.geti(2).and_then(Cbor::as_bytes), map.geti(3).filter(|s| matches!(s, Cbor::Map(_))))
    else {
        return Err(not_allowed("missing attestation fields"));
    };
    let auth = parse_auth_data(auth_bytes)?;
    check_auth_data(plan, &auth)?;
    let Some((id, key)) = &auth.attested else { return Err(not_allowed("no attested credential data")) };

    // "none" (the default): the statement is erased, the AAGUID kept (hybrid authenticators are treated like platform ones).
    let none = plan.attestation == "none";
    let attestation_object = encode_cbor(&Cbor::map(vec![
        (Cbor::text("fmt"), Cbor::text(if none { "none" } else { fmt })),
        (Cbor::text("attStmt"), if none { Cbor::Map(vec![]) } else { att_stmt.clone() }),
        (Cbor::text("authData"), Cbor::bytes(auth_bytes)),
    ]))
    .map_err(|_| not_allowed("cannot encode attestation"))?;
    let (der_key, alg) = spki_of(key)?;
    let response = AttestationResponseJson {
        client_data_json: b64::encode(&plan.client_data_json),
        attestation_object: b64::encode(&attestation_object),
        authenticator_data: b64::encode(auth_bytes),
        transports: transports_of(&plan.info),
        public_key: der_key.map(|d| b64::encode(&d)),
        public_key_algorithm: alg,
    };

    let mut results = ClientExtensionResults::default();
    if plan.cred_props {
        // Only known when a resident key was required; CTAP 2.1 (or no rk support) means "not resident", an old phone says nothing.
        let newer = plan.info.versions.iter().any(|v| v == "FIDO_2_1" || v == "FIDO_2_2");
        let rk = if plan.resident_key {
            Some(true)
        } else if !plan.info.rk || newer {
            Some(false)
        } else {
            None
        };
        results.cred_props = Some(CredProps { rk });
    }
    Ok(CredentialJson {
        id: b64::encode(id),
        raw_id: b64::encode(id),
        ty: "public-key",
        authenticator_attachment: "cross-platform",
        response: ResponseJson::Attestation(response),
        client_extension_results: results,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hybrid::webauthn_request::WebAuthnRequest;
    use p256::SecretKey;
    use p256::elliptic_curve::sec1::ToEncodedPoint;
    use p256::pkcs8::EncodePublicKey;
    use rand_core::OsRng;
    use serde_json::json;

    fn sha(b: &[u8]) -> Vec<u8> {
        Sha256::digest(b).to_vec()
    }
    fn b(x: &[u8]) -> String {
        b64::encode(x)
    }
    fn i(n: i64) -> Cbor {
        Cbor::int(n)
    }
    fn t(s: &str) -> Cbor {
        Cbor::text(s)
    }
    fn m(entries: Vec<(Cbor, Cbor)>) -> Cbor {
        Cbor::Map(entries)
    }

    fn challenge() -> Vec<u8> {
        (0..32u8).collect()
    }
    fn phone() -> DeviceInfo {
        DeviceInfo { versions: vec!["U2F_V2".into(), "FIDO_2_0".into()], uv: true, rk: true, make_cred_uv_not_rqd: false, transports: None }
    }

    fn merge(base: Value, extra: Value) -> Map<String, Value> {
        let mut o = base.as_object().unwrap().clone();
        for (k, v) in extra.as_object().unwrap() {
            o.insert(k.clone(), v.clone());
        }
        o
    }

    fn get_request(extra: Value) -> WebAuthnRequest {
        let rp = "login.microsoftonline.com";
        WebAuthnRequest {
            ty: RequestType::Get,
            mediation: None,
            rp_id: rp.into(),
            origin: format!("https://{rp}"),
            public_key: merge(json!({ "challenge": b(&challenge()), "rpId": rp, "userVerification": "preferred", "allowCredentials": [], "timeout": 60000 }), extra),
        }
    }

    const USER_ID: [u8; 3] = [9, 8, 7];
    fn create_request(extra: Value) -> WebAuthnRequest {
        WebAuthnRequest {
            ty: RequestType::Create,
            mediation: None,
            rp_id: "webauthn.io".into(),
            origin: "https://webauthn.io".into(),
            public_key: merge(
                json!({
                    "challenge": b(&challenge()),
                    "rp": { "name": "webauthn.io", "id": "webauthn.io" },
                    "user": { "id": b(&USER_ID), "name": "alice", "displayName": "Alice" },
                    "pubKeyCredParams": [{ "type": "public-key", "alg": -7 }, { "type": "public-key", "alg": -257 }],
                    "authenticatorSelection": { "residentKey": "preferred", "userVerification": "preferred" },
                    "attestation": "none",
                    "extensions": { "credProps": true }
                }),
                extra,
            ),
        }
    }

    fn decoded(plan: &CtapPlan) -> Cbor {
        decode_cbor(&plan.bytes[1..]).unwrap()
    }
    fn plan_get(extra: Value, info: &DeviceInfo) -> Result<CtapPlan, CtapFailure> {
        plan_ctap(&get_request(extra), info)
    }
    fn plan_create(extra: Value, info: &DeviceInfo) -> Result<CtapPlan, CtapFailure> {
        plan_ctap(&create_request(extra), info)
    }

    // ----- clientDataJSON -----

    #[test]
    fn client_data_json_is_byte_exact() {
        assert_eq!(
            client_data_json(RequestType::Get, &[1, 2, 3], "https://example.com"),
            "{\"type\":\"webauthn.get\",\"challenge\":\"AQID\",\"origin\":\"https://example.com\",\"crossOrigin\":false}"
        );
        assert_eq!(
            client_data_json(RequestType::Create, &challenge(), "https://webauthn.io:8443"),
            "{\"type\":\"webauthn.create\",\"challenge\":\"AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8\",\"origin\":\"https://webauthn.io:8443\",\"crossOrigin\":false}"
        );
    }

    #[test]
    fn client_data_json_escapes_only_quotes_backslashes_and_controls() {
        let json = client_data_json(RequestType::Get, &[], "x\"\\\n\u{1f}/é<");
        assert!(json.contains("\"origin\":\"x\\\"\\\\\\u000a\\u001f/é<\""), "{json}");
        assert!(serde_json::from_str::<Value>(&json).is_ok());
    }

    // ----- getInfo -----

    #[test]
    fn get_info_reads_the_options() {
        let bytes = encode_cbor(&m(vec![
            (i(1), Cbor::Array(vec![t("FIDO_2_0"), t("FIDO_2_1")])),
            (i(4), m(vec![(t("uv"), Cbor::Bool(true)), (t("rk"), Cbor::Bool(true)), (t("makeCredUvNotRqd"), Cbor::Bool(true)), (t("up"), Cbor::Bool(true))])),
            (i(9), Cbor::Array(vec![t("internal"), t("hybrid")])),
        ]))
        .unwrap();
        assert_eq!(
            parse_get_info(&bytes).unwrap(),
            DeviceInfo {
                versions: vec!["FIDO_2_0".into(), "FIDO_2_1".into()],
                uv: true,
                rk: true,
                make_cred_uv_not_rqd: true,
                transports: Some(vec!["internal".into(), "hybrid".into()])
            }
        );
    }

    #[test]
    fn get_info_empty_map_and_garbage() {
        assert_eq!(parse_get_info(&[0xa0]).unwrap(), DeviceInfo { versions: vec![], uv: false, rk: false, make_cred_uv_not_rqd: false, transports: None });
        assert!(parse_get_info(&[0x01]).is_err());
        assert!(parse_get_info(&[0xff]).is_err());
    }

    // ----- getAssertion plan -----

    #[test]
    fn microsoft_like_get_request() {
        let plan = plan_get(json!({}), &phone()).unwrap();
        assert_eq!(plan.bytes[0], 0x02);
        let json = client_data_json(RequestType::Get, &challenge(), "https://login.microsoftonline.com");
        assert_eq!(String::from_utf8(plan.client_data_json.clone()).unwrap(), json);
        let d = decoded(&plan);
        assert_eq!(d.geti(1), Some(&t("login.microsoftonline.com")));
        assert_eq!(d.geti(2), Some(&Cbor::Bytes(sha(json.as_bytes()))));
        assert_eq!(d.geti(5), Some(&m(vec![(t("uv"), Cbor::Bool(true))])));
        assert_eq!(d.as_map().unwrap().len(), 3);
        let expected = format!(
            "02a30178{:02x}{}025820{}05a1627576f5",
            "login.microsoftonline.com".len(),
            hex::encode("login.microsoftonline.com"),
            hex::encode(sha(json.as_bytes()))
        );
        assert_eq!(hex::encode(&plan.bytes), expected);
    }

    #[test]
    fn allow_list_is_sent_as_descriptors_without_transports() {
        let plan = plan_get(
            json!({ "allowCredentials": [
                { "type": "public-key", "id": "AQID", "transports": ["internal"] },
                { "type": "other", "id": "BAUG" },
                { "type": "public-key", "id": "BwgJ" }] }),
            &phone(),
        )
        .unwrap();
        assert_eq!(decoded(&plan).geti(3), Some(&Cbor::Array(vec![descriptor(&[1, 2, 3]), descriptor(&[7, 8, 9])])));
        assert!(hex::encode(&plan.bytes).contains("a2626964430102036474797065 6a7075626c69632d6b6579".replace(' ', "").as_str()));
    }

    #[test]
    fn uv_follows_the_phone() {
        let allow = json!([{ "type": "public-key", "id": "AQID" }]);
        let has5 = |extra: Value, info: &DeviceInfo| decoded(&plan_get(extra, info).unwrap()).geti(5).is_some();
        assert!(!has5(json!({ "allowCredentials": allow, "userVerification": "discouraged" }), &phone()));
        assert!(!has5(json!({ "allowCredentials": allow }), &DeviceInfo { uv: false, ..phone() }));
        // No allowed credentials: user verification is required whatever the page says.
        assert_eq!(decoded(&plan_get(json!({ "userVerification": "discouraged" }), &phone()).unwrap()).geti(5), Some(&m(vec![(t("uv"), Cbor::Bool(true))])));
    }

    #[test]
    fn required_uv_without_phone_support_fails() {
        let no_uv = DeviceInfo { uv: false, ..phone() };
        assert!(plan_get(json!({ "userVerification": "required", "allowCredentials": [{ "type": "public-key", "id": "AQID" }] }), &no_uv).is_err());
        assert!(plan_get(json!({}), &no_uv).is_err());
    }

    #[test]
    fn lists_extensions_that_are_not_passed_on() {
        let plan = plan_get(json!({ "extensions": { "prf": {}, "appid": "x", "largeBlob": {} } }), &phone()).unwrap();
        let mut s = plan.skipped.clone();
        s.sort();
        assert_eq!(s, vec!["appid", "largeBlob", "prf"]);
    }

    // ----- makeCredential plan -----

    #[test]
    fn webauthn_io_like_create_request() {
        let plan = plan_create(json!({}), &phone()).unwrap();
        assert_eq!(plan.bytes[0], 0x01);
        let json = client_data_json(RequestType::Create, &challenge(), "https://webauthn.io");
        let d = decoded(&plan);
        assert_eq!(d.geti(1), Some(&Cbor::Bytes(sha(json.as_bytes()))));
        assert_eq!(d.geti(2), Some(&m(vec![(t("id"), t("webauthn.io")), (t("name"), t("webauthn.io"))])));
        assert_eq!(d.geti(3), Some(&m(vec![(t("id"), Cbor::bytes(&USER_ID)), (t("name"), t("alice")), (t("displayName"), t("Alice"))])));
        assert_eq!(d.geti(4), Some(&Cbor::Array(vec![alg_param(-7), alg_param(-257)])));
        assert_eq!(d.geti(7), Some(&m(vec![(t("rk"), Cbor::Bool(true)), (t("uv"), Cbor::Bool(true))])));
        assert!(plan.resident_key && plan.cred_props);
        assert_eq!(plan.attestation, "none");
    }

    #[test]
    fn empty_display_name_is_kept() {
        let plan = plan_create(json!({ "user": { "id": b(&USER_ID), "name": "a", "displayName": "" } }), &phone()).unwrap();
        assert_eq!(decoded(&plan).geti(3).unwrap().gets("displayName"), Some(&t("")));
        let no_name = plan_create(json!({ "user": { "id": b(&USER_ID), "name": "a" } }), &phone()).unwrap();
        assert_eq!(decoded(&no_name).geti(3).unwrap().gets("displayName"), Some(&t("")));
    }

    #[test]
    fn rp_id_is_the_effective_one() {
        let plan = plan_create(json!({ "rp": { "name": "Example" } }), &phone()).unwrap();
        assert_eq!(decoded(&plan).geti(2).unwrap().gets("id"), Some(&t("webauthn.io")));
    }

    #[test]
    fn resident_key_selection() {
        let rk = |sel: Value, info: &DeviceInfo| plan_create(json!({ "authenticatorSelection": sel }), info).map(|p| p.resident_key);
        assert_eq!(rk(json!({ "residentKey": "required" }), &phone()), Ok(true));
        assert_eq!(rk(json!({ "residentKey": "preferred" }), &phone()), Ok(true));
        assert_eq!(rk(json!({ "residentKey": "preferred" }), &DeviceInfo { rk: false, ..phone() }), Ok(false));
        assert_eq!(rk(json!({ "residentKey": "preferred" }), &DeviceInfo { uv: false, ..phone() }), Ok(false));
        assert_eq!(rk(json!({ "residentKey": "discouraged" }), &phone()), Ok(false));
        assert_eq!(rk(json!({ "requireResidentKey": true }), &phone()), Ok(true));
        assert_eq!(rk(json!({}), &phone()), Ok(false));
        assert!(rk(json!({ "residentKey": "required" }), &DeviceInfo { rk: false, ..phone() }).is_err());
    }

    #[test]
    fn options_follow_the_phone() {
        let options = |sel: Value, info: &DeviceInfo| decoded(&plan_create(json!({ "authenticatorSelection": sel }), info).unwrap()).geti(7).cloned();
        assert_eq!(options(json!({ "residentKey": "discouraged" }), &phone()), Some(m(vec![(t("uv"), Cbor::Bool(true))])));
        assert_eq!(options(json!({ "residentKey": "discouraged" }), &DeviceInfo { make_cred_uv_not_rqd: true, ..phone() }), None);
        assert_eq!(options(json!({ "residentKey": "discouraged" }), &DeviceInfo { uv: false, ..phone() }), None);
    }

    #[test]
    fn create_fails_when_uv_is_required_but_unavailable() {
        assert!(plan_create(json!({ "authenticatorSelection": { "userVerification": "required" } }), &DeviceInfo { uv: false, ..phone() }).is_err());
    }

    #[test]
    fn exclude_list_and_params() {
        let plan = plan_create(
            json!({
                "excludeCredentials": [{ "type": "public-key", "id": "AQID", "transports": ["usb"] }],
                "pubKeyCredParams": [{ "type": "other", "alg": 1 }, { "type": "public-key", "alg": -8 }]
            }),
            &phone(),
        )
        .unwrap();
        assert_eq!(decoded(&plan).geti(5), Some(&Cbor::Array(vec![descriptor(&[1, 2, 3])])));
        assert_eq!(decoded(&plan).geti(4), Some(&Cbor::Array(vec![alg_param(-8)])));
        assert_eq!(decoded(&plan_create(json!({ "pubKeyCredParams": [] }), &phone()).unwrap()).geti(4), Some(&Cbor::Array(vec![alg_param(-7), alg_param(-257)])));
        assert!(plan_create(json!({ "pubKeyCredParams": [{ "type": "other", "alg": 1 }] }), &phone()).is_err());
        assert!(decoded(&plan_create(json!({}), &phone()).unwrap()).geti(5).is_none());
    }

    // ----- responses -----

    struct Auth {
        bytes: Vec<u8>,
        spki: Vec<u8>,
    }

    struct AuthOpts {
        rp_id: &'static str,
        flags: Option<u8>,
        id: Option<Vec<u8>>,
        aaguid: Option<Vec<u8>>,
        key: Option<Cbor>,
        trailing: Option<Vec<u8>>,
    }
    fn auth_opts(rp_id: &'static str) -> AuthOpts {
        AuthOpts { rp_id, flags: None, id: None, aaguid: None, key: None, trailing: None }
    }

    /// Authenticator data, optionally with attested credential data for an ES256 key.
    fn auth_data(o: AuthOpts) -> Auth {
        let sk = SecretKey::random(&mut OsRng);
        let pt = sk.public_key().to_encoded_point(false);
        let key = o.key.unwrap_or_else(|| {
            m(vec![(i(1), i(2)), (i(3), i(-7)), (i(-1), i(1)), (i(-2), Cbor::bytes(pt.x().unwrap())), (i(-3), Cbor::bytes(pt.y().unwrap()))])
        });
        let attested = o.id.is_some();
        let flags = o.flags.unwrap_or(0x01 | 0x04 | if attested { 0x40 } else { 0 });
        let mut bytes = sha(o.rp_id.as_bytes());
        bytes.push(flags);
        bytes.extend_from_slice(&[0, 0, 0, 5]);
        if let Some(id) = &o.id {
            bytes.extend_from_slice(&o.aaguid.unwrap_or(vec![0xaa; 16]));
            bytes.extend_from_slice(&(id.len() as u16).to_be_bytes());
            bytes.extend_from_slice(id);
            bytes.extend(encode_cbor(&key).unwrap());
        }
        if let Some(tr) = o.trailing {
            bytes.extend(tr);
        }
        Auth { bytes, spki: sk.public_key().to_public_key_der().unwrap().as_bytes().to_vec() }
    }

    fn ok_resp(map: Cbor) -> Vec<u8> {
        let mut v = vec![0];
        v.extend(encode_cbor(&map).unwrap());
        v
    }
    fn credential_id() -> Vec<u8> {
        vec![3; 32]
    }
    fn failure_name(r: Result<CredentialJson, CtapFailure>) -> String {
        match r {
            Ok(_) => "accepted".into(),
            Err(e) => e.error.name,
        }
    }

    fn assertion_response(auth: &[u8]) -> Vec<u8> {
        ok_resp(m(vec![
            (i(1), m(vec![(t("id"), Cbor::bytes(&credential_id())), (t("type"), t("public-key"))])),
            (i(2), Cbor::bytes(auth)),
            (i(3), Cbor::bytes(&[0x30, 1, 2])),
            (i(4), m(vec![(t("id"), Cbor::bytes(&USER_ID))])),
        ]))
    }
    const MS: &str = "login.microsoftonline.com";

    #[test]
    fn assertion_builds_the_credential() {
        let plan = plan_get(json!({}), &phone()).unwrap();
        let auth = auth_data(auth_opts(MS)).bytes;
        let c = parse_ctap_response(&plan, &assertion_response(&auth)).unwrap();
        assert_eq!(c.id, b(&credential_id()));
        assert_eq!(c.raw_id, b(&credential_id()));
        assert_eq!(c.authenticator_attachment, "cross-platform");
        let ResponseJson::Assertion(r) = c.response else { panic!("not an assertion") };
        assert_eq!(r.client_data_json, b(client_data_json(RequestType::Get, &challenge(), "https://login.microsoftonline.com").as_bytes()));
        assert_eq!(r.authenticator_data, b(&auth));
        assert_eq!(r.signature, b(&[0x30, 1, 2]));
        assert_eq!(r.user_handle, Some(b(&USER_ID)));
        assert_eq!(c.client_extension_results, ClientExtensionResults::default());
    }

    #[test]
    fn assertion_takes_the_credential_from_a_single_entry_allow_list() {
        let plan = plan_get(json!({ "allowCredentials": [{ "type": "public-key", "id": b(&credential_id()) }] }), &phone()).unwrap();
        let resp = ok_resp(m(vec![(i(2), Cbor::bytes(&auth_data(auth_opts(MS)).bytes)), (i(3), Cbor::bytes(&[1]))]));
        let c = parse_ctap_response(&plan, &resp).unwrap();
        assert_eq!(c.id, b(&credential_id()));
        let ResponseJson::Assertion(r) = c.response else { panic!() };
        assert!(r.user_handle.is_none());
    }

    #[test]
    fn assertion_rejections() {
        let plan = plan_get(json!({}), &phone()).unwrap();
        let cases: Vec<(&str, Vec<u8>)> = vec![
            ("rpIdHash of another site", assertion_response(&auth_data(auth_opts("evil.example")).bytes)),
            ("user presence flag missing", assertion_response(&auth_data(AuthOpts { flags: Some(0x04), ..auth_opts(MS) }).bytes)),
            ("attested data in an assertion", assertion_response(&auth_data(AuthOpts { id: Some(credential_id()), ..auth_opts(MS) }).bytes)),
            ("authenticator data too short", assertion_response(&[1; 36])),
            ("trailing authenticator data", assertion_response(&auth_data(AuthOpts { trailing: Some(vec![1]), ..auth_opts(MS) }).bytes)),
            ("empty response", vec![]),
            ("status only", vec![0]),
            ("CBOR that is not a map", vec![0, 0x01]),
            ("truncated CBOR", vec![0, 0xa1, 0x02]),
            ("trailing bytes after the map", [assertion_response(&auth_data(auth_opts(MS)).bytes), vec![0]].concat()),
        ];
        for (name, resp) in cases {
            assert_eq!(failure_name(parse_ctap_response(&plan, &resp)), "NotAllowedError", "{name}");
        }
    }

    #[test]
    fn assertion_credential_must_be_allowed_and_user_present() {
        let other = plan_get(json!({ "allowCredentials": [{ "type": "public-key", "id": "AQID" }] }), &phone()).unwrap();
        assert!(parse_ctap_response(&other, &assertion_response(&auth_data(auth_opts(MS)).bytes)).is_err());
        let plan = plan_get(json!({}), &phone()).unwrap();
        let no_user = ok_resp(m(vec![
            (i(1), m(vec![(t("id"), Cbor::bytes(&credential_id()))])),
            (i(2), Cbor::bytes(&auth_data(auth_opts(MS)).bytes)),
            (i(3), Cbor::bytes(&[1])),
        ]));
        assert!(parse_ctap_response(&plan, &no_user).is_err());
        let several = plan_get(json!({ "allowCredentials": [{ "type": "public-key", "id": "AQID" }, { "type": "public-key", "id": "BAUG" }] }), &phone()).unwrap();
        let missing = ok_resp(m(vec![(i(2), Cbor::bytes(&auth_data(auth_opts(MS)).bytes)), (i(3), Cbor::bytes(&[1]))]));
        assert!(parse_ctap_response(&several, &missing).is_err());
    }

    fn attestation_response(a: &Auth, fmt: &str) -> Vec<u8> {
        ok_resp(m(vec![
            (i(1), t(fmt)),
            (i(2), Cbor::bytes(&a.bytes)),
            (i(3), m(vec![(t("alg"), i(-7)), (t("sig"), Cbor::bytes(&[1, 2]))])),
        ]))
    }
    fn attested(rp: &'static str) -> Auth {
        auth_data(AuthOpts { id: Some(credential_id()), ..auth_opts(rp) })
    }

    #[test]
    fn attestation_is_none_with_the_aaguid_kept() {
        let a = auth_data(AuthOpts { id: Some(credential_id()), aaguid: Some(vec![0x42; 16]), ..auth_opts("webauthn.io") });
        let c = parse_ctap_response(&plan_create(json!({}), &phone()).unwrap(), &attestation_response(&a, "packed")).unwrap();
        assert_eq!(c.id, b(&credential_id()));
        assert_eq!(c.authenticator_attachment, "cross-platform");
        let ResponseJson::Attestation(r) = c.response else { panic!() };
        assert_eq!(r.client_data_json, b(client_data_json(RequestType::Create, &challenge(), "https://webauthn.io").as_bytes()));
        assert_eq!(r.authenticator_data, b(&a.bytes));
        assert_eq!(r.transports, vec!["hybrid", "internal"]);
        assert_eq!(r.public_key_algorithm, -7);
        assert_eq!(r.public_key, Some(b(&a.spki)));
        let object = b64::decode(&r.attestation_object).unwrap();
        assert!(hex::encode(&object).starts_with("a363666d74646e6f6e65676174745374 6d74a068617574684461 7461".replace(' ', "").as_str()));
        let d = decode_cbor(&object).unwrap();
        assert_eq!(d.gets("fmt"), Some(&t("none")));
        assert_eq!(d.gets("authData").and_then(Cbor::as_bytes), Some(a.bytes.as_slice()));
        assert_eq!(&a.bytes[37..53], &[0x42; 16]);
    }

    #[test]
    fn attestation_direct_keeps_the_phone_statement() {
        let a = attested("webauthn.io");
        let c = parse_ctap_response(&plan_create(json!({ "attestation": "direct" }), &phone()).unwrap(), &attestation_response(&a, "packed")).unwrap();
        let ResponseJson::Attestation(r) = c.response else { panic!() };
        let d = decode_cbor(&b64::decode(&r.attestation_object).unwrap()).unwrap();
        assert_eq!(d.gets("fmt"), Some(&t("packed")));
        assert_eq!(d.gets("attStmt"), Some(&m(vec![(t("alg"), i(-7)), (t("sig"), Cbor::bytes(&[1, 2]))])));
    }

    #[test]
    fn transports_are_sorted_with_hybrid_and_unknown_dropped() {
        let a = attested("webauthn.io");
        let tr = |transports: Option<Vec<&str>>| {
            let info = DeviceInfo { transports: transports.map(|v| v.into_iter().map(String::from).collect()), ..phone() };
            let c = parse_ctap_response(&plan_create(json!({}), &info).unwrap(), &attestation_response(&a, "packed")).unwrap();
            let ResponseJson::Attestation(r) = c.response else { panic!() };
            r.transports
        };
        assert_eq!(tr(Some(vec!["usb", "internal", "x"])), vec!["hybrid", "internal", "usb"]);
        assert_eq!(tr(Some(vec!["hybrid"])), vec!["hybrid"]);
        assert_eq!(tr(None), vec!["hybrid", "internal"]);
    }

    fn pk_for(key: Cbor) -> (Option<String>, i64) {
        let a = auth_data(AuthOpts { id: Some(credential_id()), key: Some(key), ..auth_opts("webauthn.io") });
        let c = parse_ctap_response(&plan_create(json!({}), &phone()).unwrap(), &attestation_response(&a, "packed")).unwrap();
        let ResponseJson::Attestation(r) = c.response else { panic!() };
        (r.public_key, r.public_key_algorithm)
    }

    #[test]
    fn public_key_spki_for_rs256_and_eddsa_and_none_for_others() {
        // Vectors generated with OpenSSL.
        let rsa_n = hex::decode("beab0e05e98d1f05da97a6304e6c85bc46bcaa4c579b10c71db0de1134c84943044df514a1a3a840d7ddb8a03346cc17d6ddf737aaa070e9a0eb4748ac1a6e5359fc9d40ba3e760ebf36baa0eab863db1eca7665bb2900f7823afa7b93ad8a8c9aa30e33097148460d2f364e22f543ca53a3dca64816c0f60251975bc3afc5cd").unwrap();
        let rsa_spki = "30819f300d06092a864886f70d010101050003818d0030818902818100beab0e05e98d1f05da97a6304e6c85bc46bcaa4c579b10c71db0de1134c84943044df514a1a3a840d7ddb8a03346cc17d6ddf737aaa070e9a0eb4748ac1a6e5359fc9d40ba3e760ebf36baa0eab863db1eca7665bb2900f7823afa7b93ad8a8c9aa30e33097148460d2f364e22f543ca53a3dca64816c0f60251975bc3afc5cd0203010001";
        let rsa = m(vec![(i(1), i(3)), (i(3), i(-257)), (i(-1), Cbor::bytes(&rsa_n)), (i(-2), Cbor::bytes(&[1, 0, 1]))]);
        assert_eq!(pk_for(rsa), (Some(b(&hex::decode(rsa_spki).unwrap())), -257));

        let ed_x = hex::decode("d093471e239585cceef929a86afbfa8784afd67a713e53334e188c80e18d8471").unwrap();
        let ed_spki = "302a300506032b6570032100d093471e239585cceef929a86afbfa8784afd67a713e53334e188c80e18d8471";
        let ed = m(vec![(i(1), i(1)), (i(3), i(-8)), (i(-1), i(6)), (i(-2), Cbor::bytes(&ed_x))]);
        assert_eq!(pk_for(ed), (Some(b(&hex::decode(ed_spki).unwrap())), -8));

        let unknown = m(vec![(i(1), i(2)), (i(3), i(-35)), (i(-1), i(2)), (i(-2), Cbor::bytes(&[1])), (i(-3), Cbor::bytes(&[2]))]);
        assert_eq!(pk_for(unknown), (None, -35));
    }

    #[test]
    fn cred_props() {
        let a = attested("webauthn.io");
        let results = |request: WebAuthnRequest, info: &DeviceInfo| {
            parse_ctap_response(&plan_ctap(&request, info).unwrap(), &attestation_response(&a, "packed")).unwrap().client_extension_results
        };
        let rk = |v: Option<bool>| ClientExtensionResults { cred_props: Some(CredProps { rk: v }) };
        assert_eq!(results(create_request(json!({})), &phone()), rk(Some(true)));
        let discouraged = || create_request(json!({ "authenticatorSelection": { "residentKey": "discouraged" } }));
        assert_eq!(results(discouraged(), &phone()), rk(None));
        assert_eq!(results(discouraged(), &DeviceInfo { versions: vec!["FIDO_2_1".into()], ..phone() }), rk(Some(false)));
        assert_eq!(results(discouraged(), &DeviceInfo { rk: false, ..phone() }), rk(Some(false)));
        assert_eq!(results(create_request(json!({ "extensions": {} })), &phone()), ClientExtensionResults::default());
    }

    #[test]
    fn attestation_rejections() {
        let plan = plan_create(json!({}), &phone()).unwrap();
        let aut = |o: AuthOpts| auth_data(o);
        let cases: Vec<(&str, Vec<u8>)> = vec![
            ("rpIdHash of another site", attestation_response(&aut(AuthOpts { id: Some(credential_id()), ..auth_opts("evil.example") }), "packed")),
            ("no attested credential data", attestation_response(&aut(auth_opts("webauthn.io")), "packed")),
            ("user presence flag missing", attestation_response(&aut(AuthOpts { id: Some(credential_id()), flags: Some(0x44), ..auth_opts("webauthn.io") }), "packed")),
            ("a credential id of 0 bytes", attestation_response(&aut(AuthOpts { id: Some(vec![]), ..auth_opts("webauthn.io") }), "packed")),
            ("a credential id over 1023 bytes", attestation_response(&aut(AuthOpts { id: Some(vec![1; 1024]), ..auth_opts("webauthn.io") }), "packed")),
            ("a public key that is not a map", attestation_response(&aut(AuthOpts { id: Some(credential_id()), key: Some(i(5)), ..auth_opts("webauthn.io") }), "packed")),
            ("a missing statement", ok_resp(m(vec![(i(1), t("none")), (i(2), Cbor::bytes(&attested("webauthn.io").bytes))]))),
            ("a format that is not text", ok_resp(m(vec![(i(1), i(5)), (i(2), Cbor::bytes(&attested("webauthn.io").bytes)), (i(3), m(vec![]))]))),
        ];
        for (name, resp) in cases {
            assert_eq!(failure_name(parse_ctap_response(&plan, &resp)), "NotAllowedError", "{name}");
        }
    }

    // ----- CTAP errors -----

    #[test]
    fn credential_excluded_is_invalid_state() {
        let plan = plan_create(json!({}), &phone()).unwrap();
        assert_eq!(map_ctap_status(0x19).name, "InvalidStateError");
        let e = parse_ctap_response(&plan, &[0x19]).unwrap_err();
        assert_eq!(e.error.name, "InvalidStateError");
        assert_eq!(e.reason, "CTAP status 0x19");
    }

    #[test]
    fn other_statuses_are_not_allowed() {
        let plan = plan_create(json!({}), &phone()).unwrap();
        for status in [0x01u8, 0x12, 0x2e, 0x27, 0x2f, 0x30, 0x31, 0x32, 0x36, 0x3b, 0x3c, 0x7f, 0xf0, 0xff] {
            assert_eq!(map_ctap_status(status).name, "NotAllowedError");
            assert!(map_ctap_status(status).message.contains("timed out or was not allowed"));
            assert_eq!(parse_ctap_response(&plan, &[status, 0xa0]).unwrap_err().error.name, "NotAllowedError");
        }
    }

    #[test]
    fn es256_spki_matches_the_p256_crate_encoding() {
        // Covered by attestation_is_none_with_the_aaguid_kept; this checks the on-curve guard.
        let bad = m(vec![(i(1), i(2)), (i(3), i(-7)), (i(-1), i(1)), (i(-2), Cbor::bytes(&[1; 32])), (i(-3), Cbor::bytes(&[2; 32]))]);
        assert_eq!(pk_for(bad), (None, -7));
    }
}

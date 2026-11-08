//! Wire format of the phone link (port of packages/shared/src/link/protocol.ts).
//!
//! The QR code content and the one-line JSON messages the two sides exchange.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use super::loopback::is_loopback_url;
use crate::b64;

pub const LINK_PROTOCOL_VERSION: u32 = 1;
/// mDNS service type is `_signet._tcp`; this is the `signet` part.
pub const LINK_SERVICE_TYPE: &str = "signet";
pub const LINK_DEFAULT_PORT: u16 = 47615;
/// Maximum line size (bytes, including the newline).
pub const LINK_MAX_LINE: usize = 64 * 1024;
pub const PAIRING_TTL_MS: u64 = 5 * 60_000;
pub const PAIRING_URL_PREFIX: &str = "signet://link?d=";
pub const MAX_PORTS: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LinkPlatform {
    Macos,
    Windows,
    Linux,
    Ios,
    Android,
}

impl LinkPlatform {
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "macos" => Self::Macos,
            "windows" => Self::Windows,
            "linux" => Self::Linux,
            "ios" => Self::Ios,
            "android" => Self::Android,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkDevice {
    pub id: String,
    pub name: String,
    pub platform: LinkPlatform,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingOffer {
    pub v: u32,
    /// Only the name travels in the QR code.
    pub desktop_name: String,
    pub link_id: String,
    pub cert_sha256: String,
    pub code: String,
    pub hosts: Vec<String>,
    pub port: u16,
    /// Expiry, ms since 1970.
    pub expires_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DevServer {
    pub url: String,
    pub port: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, rename = "fromTab", skip_serializing_if = "Option::is_none")]
    pub from_tab: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkErrorCode {
    Version,
    UnknownLink,
    BadToken,
    BadCode,
    Expired,
    Paused,
    PortClosed,
    PortRefused,
    BadRequest,
}

impl LinkErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Version => "version",
            Self::UnknownLink => "unknown-link",
            Self::BadToken => "bad-token",
            Self::BadCode => "bad-code",
            Self::Expired => "expired",
            Self::Paused => "paused",
            Self::PortClosed => "port-closed",
            Self::PortRefused => "port-refused",
            Self::BadRequest => "bad-request",
        }
    }
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "version" => Self::Version,
            "unknown-link" => Self::UnknownLink,
            "bad-token" => Self::BadToken,
            "bad-code" => Self::BadCode,
            "expired" => Self::Expired,
            "paused" => Self::Paused,
            "port-closed" => Self::PortClosed,
            "port-refused" => Self::PortRefused,
            "bad-request" => Self::BadRequest,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkMessage {
    Pair { v: u32, link_id: String, code: String, device: LinkDevice },
    HelloControl { v: u32, link_id: String, token: String, device: LinkDevice },
    HelloTunnel { v: u32, link_id: String, token: String, port: u16 },
    Paired { token: String, device: LinkDevice },
    Welcome { device: LinkDevice, paused: bool },
    Ok,
    Error { code: LinkErrorCode },
    Status { paused: bool },
    Servers { servers: Vec<DevServer>, ports: Vec<u16> },
    Open { url: String },
    Forget,
    Ping,
    Pong,
}

// --- Identifiers and secrets ---

fn is_b64url(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

pub fn is_link_id(v: &str) -> bool {
    v.len() == 16 && v.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}
pub fn is_link_token(v: &str) -> bool {
    v.len() == 43 && is_b64url(v)
}
pub fn is_pairing_code(v: &str) -> bool {
    v.len() == 22 && is_b64url(v)
}
pub fn is_cert_sha256(v: &str) -> bool {
    is_link_token(v)
}
fn valid_port(n: u64) -> Option<u16> {
    (n > 0 && n < 65536).then_some(n as u16)
}

pub fn server_name_for_link(link_id: &str) -> String {
    format!("{link_id}.link.signet.invalid")
}

pub fn link_id_from_server_name(name: Option<&str>) -> Option<String> {
    let name = name?;
    let id = name.strip_suffix(".link.signet.invalid")?;
    is_link_id(id).then(|| id.to_string())
}

// --- QR code ---

pub fn encode_pairing_url(offer: &PairingOffer) -> String {
    let fields = [
        offer.v.to_string(),
        offer.link_id.clone(),
        offer.cert_sha256.clone(),
        offer.code.clone(),
        offer.port.to_string(),
        to_base36(offer.expires_at.div_ceil(1000)),
        offer.hosts.join(","),
        b64::encode_text(&offer.desktop_name),
    ];
    format!("{PAIRING_URL_PREFIX}{}", fields.join("~"))
}

fn to_base36(mut n: u64) -> String {
    if n == 0 {
        return "0".into();
    }
    let mut out = Vec::new();
    while n > 0 {
        out.push(b"0123456789abcdefghijklmnopqrstuvwxyz"[(n % 36) as usize]);
        n /= 36;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairingDecode {
    Ok(PairingOffer),
    Invalid,
    Version,
    Expired,
}

struct RawOffer {
    v: Option<f64>,
    name: Option<String>,
    link_id: Option<String>,
    cert: Option<String>,
    code: Option<String>,
    hosts: Option<Vec<String>>,
    port: Option<f64>,
    expires_at: Option<f64>,
}

/// Reads a QR code text (or `signet://link?d=...`). `now` in ms. Accepts the legacy JSON format too.
pub fn decode_pairing_url(text: &str, now: u64) -> PairingDecode {
    let t = text.trim();
    let Some(data) = t.strip_prefix(PAIRING_URL_PREFIX) else { return PairingDecode::Invalid };
    let raw = if data.contains('~') { compact_fields(data) } else { legacy_fields(data) };
    let Some(raw) = raw else { return PairingDecode::Invalid };
    let Some(v) = raw.v.filter(|v| v.is_finite()) else { return PairingDecode::Invalid };
    if v != LINK_PROTOCOL_VERSION as f64 {
        return PairingDecode::Version;
    }
    let name = raw.name.as_deref().map(clean_name).unwrap_or_default();
    let hosts: Vec<String> = raw.hosts.unwrap_or_default().into_iter().filter(|h| is_host(h)).take(16).collect();
    let (Some(link_id), Some(cert), Some(code), Some(port), Some(exp)) = (raw.link_id, raw.cert, raw.code, raw.port, raw.expires_at) else {
        return PairingDecode::Invalid;
    };
    let port = if port.fract() == 0.0 && port > 0.0 && port < 65536.0 { port as u16 } else { return PairingDecode::Invalid };
    if name.is_empty() || hosts.is_empty() || !is_link_id(&link_id) || !is_cert_sha256(&cert) || !is_pairing_code(&code) || !exp.is_finite() {
        return PairingDecode::Invalid;
    }
    if exp < now as f64 {
        return PairingDecode::Expired;
    }
    PairingDecode::Ok(PairingOffer {
        v: LINK_PROTOCOL_VERSION,
        desktop_name: name,
        link_id,
        cert_sha256: cert,
        code,
        hosts,
        port,
        expires_at: exp as u64,
    })
}

fn compact_fields(data: &str) -> Option<RawOffer> {
    let f: Vec<&str> = data.split('~').collect();
    if f.len() != 8 {
        return None;
    }
    Some(RawOffer {
        v: f[0].parse::<f64>().ok(),
        link_id: Some(f[1].to_string()),
        cert: Some(f[2].to_string()),
        code: Some(f[3].to_string()),
        port: f[4].parse::<f64>().ok(),
        expires_at: u64::from_str_radix(f[5], 36).ok().map(|s| s as f64 * 1000.0),
        hosts: Some(f[6].split(',').filter(|s| !s.is_empty()).map(String::from).collect()),
        name: Some(b64::decode_text(f[7])?),
    })
}

fn legacy_fields(data: &str) -> Option<RawOffer> {
    let raw: Value = serde_json::from_str(&b64::decode_text(data)?).ok()?;
    let o = raw.as_object()?;
    Some(RawOffer {
        v: o.get("v").and_then(Value::as_f64),
        name: o.get("desktop").and_then(|d| d.get("name")).and_then(Value::as_str).map(String::from),
        link_id: o.get("linkId").and_then(Value::as_str).map(String::from),
        cert: o.get("certSha256").and_then(Value::as_str).map(String::from),
        code: o.get("code").and_then(Value::as_str).map(String::from),
        hosts: o.get("hosts").and_then(Value::as_array).map(|a| a.iter().filter_map(|h| h.as_str().map(String::from)).collect()),
        port: o.get("port").and_then(Value::as_f64),
        expires_at: o.get("expiresAt").and_then(Value::as_f64),
    })
}

// --- Messages ---

fn device_json(d: &LinkDevice) -> Value {
    json!({ "id": d.id, "name": d.name, "platform": d.platform })
}

/// A line ready to write (JSON + newline).
pub fn encode_link_message(msg: &LinkMessage) -> String {
    let v = match msg {
        LinkMessage::Pair { v, link_id, code, device } => {
            json!({ "type": "pair", "v": v, "linkId": link_id, "code": code, "device": device_json(device) })
        }
        LinkMessage::HelloControl { v, link_id, token, device } => {
            json!({ "type": "hello", "v": v, "linkId": link_id, "token": token, "role": "control", "device": device_json(device) })
        }
        LinkMessage::HelloTunnel { v, link_id, token, port } => {
            json!({ "type": "hello", "v": v, "linkId": link_id, "token": token, "role": "tunnel", "port": port })
        }
        LinkMessage::Paired { token, device } => json!({ "type": "paired", "token": token, "device": device_json(device) }),
        LinkMessage::Welcome { device, paused } => json!({ "type": "welcome", "device": device_json(device), "paused": paused }),
        LinkMessage::Ok => json!({ "type": "ok" }),
        LinkMessage::Error { code } => json!({ "type": "error", "code": code.as_str() }),
        LinkMessage::Status { paused } => json!({ "type": "status", "paused": paused }),
        LinkMessage::Servers { servers, ports } => json!({ "type": "servers", "servers": servers, "ports": ports }),
        LinkMessage::Open { url } => json!({ "type": "open", "url": url }),
        LinkMessage::Forget => json!({ "type": "forget" }),
        LinkMessage::Ping => json!({ "type": "ping" }),
        LinkMessage::Pong => json!({ "type": "pong" }),
    };
    format!("{v}\n")
}

fn s<'a>(m: &'a Map<String, Value>, k: &str) -> Option<&'a str> {
    m.get(k).and_then(Value::as_str)
}

fn int(m: &Map<String, Value>, k: &str) -> Option<u64> {
    let v = m.get(k)?;
    if let Some(n) = v.as_u64() {
        return Some(n);
    }
    let f = v.as_f64()?;
    (f.fract() == 0.0 && f >= 0.0).then_some(f as u64)
}

/// Reads a received line. None if malformed.
pub fn parse_link_message(line: &str) -> Option<LinkMessage> {
    if line.chars().count() > LINK_MAX_LINE {
        return None;
    }
    let value: Value = serde_json::from_str(line).ok()?;
    let m = value.as_object()?;
    let ty = s(m, "type")?;
    match ty {
        "pair" => {
            let device = parse_device(m.get("device")?)?;
            let v = m.get("v").and_then(Value::as_f64)?;
            let link_id = s(m, "linkId").filter(|x| is_link_id(x))?;
            let code = s(m, "code").filter(|x| is_pairing_code(x))?;
            Some(LinkMessage::Pair { v: v as u32, link_id: link_id.into(), code: code.into(), device })
        }
        "hello" => {
            let v = m.get("v").and_then(Value::as_f64)?;
            let link_id = s(m, "linkId").filter(|x| is_link_id(x))?.to_string();
            let token = s(m, "token").filter(|x| is_link_token(x))?.to_string();
            match s(m, "role")? {
                "control" => {
                    let device = parse_device(m.get("device")?)?;
                    Some(LinkMessage::HelloControl { v: v as u32, link_id, token, device })
                }
                "tunnel" => {
                    let port = valid_port(int(m, "port")?)?;
                    Some(LinkMessage::HelloTunnel { v: v as u32, link_id, token, port })
                }
                _ => None,
            }
        }
        "paired" => {
            let device = parse_device(m.get("device")?)?;
            let token = s(m, "token").filter(|x| is_link_token(x))?;
            Some(LinkMessage::Paired { token: token.into(), device })
        }
        "welcome" => {
            let device = parse_device(m.get("device")?)?;
            let paused = m.get("paused")?.as_bool()?;
            Some(LinkMessage::Welcome { device, paused })
        }
        "ok" => Some(LinkMessage::Ok),
        "error" => Some(LinkMessage::Error { code: LinkErrorCode::parse(s(m, "code")?)? }),
        "status" => Some(LinkMessage::Status { paused: m.get("paused")?.as_bool()? }),
        "servers" => {
            let arr = m.get("servers")?.as_array()?;
            let servers: Vec<DevServer> = arr.iter().filter_map(parse_dev_server).take(64).collect();
            let mut ports: Vec<u16> = match m.get("ports").and_then(Value::as_array) {
                Some(a) => a.iter().filter_map(|p| p.as_u64().and_then(valid_port)).collect(),
                None => servers.iter().map(|x| x.port).collect(),
            };
            ports.sort_unstable();
            ports.dedup();
            ports.truncate(MAX_PORTS);
            Some(LinkMessage::Servers { servers, ports })
        }
        "open" => {
            let url = s(m, "url")?;
            let l = url.to_ascii_lowercase();
            (url.chars().count() <= 4096 && (l.starts_with("http://") || l.starts_with("https://"))).then(|| LinkMessage::Open { url: url.into() })
        }
        "forget" => Some(LinkMessage::Forget),
        "ping" => Some(LinkMessage::Ping),
        "pong" => Some(LinkMessage::Pong),
        _ => None,
    }
}

fn is_host(v: &str) -> bool {
    !v.is_empty() && v.len() <= 64 && v.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b':' | b'%' | b'-'))
}

fn parse_device(v: &Value) -> Option<LinkDevice> {
    let o = v.as_object()?;
    let id = s(o, "id")?;
    let platform = LinkPlatform::parse(s(o, "platform")?)?;
    let name = clean_name(s(o, "name")?);
    if id.is_empty() || id.chars().count() > 64 || name.is_empty() {
        return None;
    }
    Some(LinkDevice { id: id.into(), name, platform })
}

fn parse_dev_server(v: &Value) -> Option<DevServer> {
    let o = v.as_object()?;
    let url = s(o, "url")?;
    let port = valid_port(o.get("port")?.as_u64()?)?;
    if !is_loopback_url(url) || url.chars().count() > 2048 {
        return None;
    }
    let title = s(o, "title").map(str::trim).filter(|t| !t.is_empty()).map(|t| t.chars().take(200).collect::<String>());
    let from_tab = (o.get("fromTab").and_then(Value::as_bool) == Some(true)).then_some(true);
    Some(DevServer { url: url.into(), port, title, from_tab })
}

/// Displayable name: no control characters, 64 characters at most.
pub fn clean_name(name: &str) -> String {
    let s: String = name.chars().filter(|c| (*c as u32) > 31 && *c as u32 != 127).collect();
    s.trim().chars().take(64).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    const CODE: &str = "bbbbbbbbbbbbbbbbbbbbbb";

    fn desktop() -> LinkDevice {
        LinkDevice { id: "desk-1".into(), name: "MacBook de Christophe".into(), platform: LinkPlatform::Macos }
    }
    fn phone() -> LinkDevice {
        LinkDevice { id: "phone-1".into(), name: "iPhone".into(), platform: LinkPlatform::Ios }
    }
    fn offer() -> PairingOffer {
        PairingOffer {
            v: 1,
            desktop_name: desktop().name,
            link_id: "0123456789abcdef".into(),
            cert_sha256: TOKEN.into(),
            code: CODE.into(),
            hosts: vec!["192.168.1.20".into(), "fe80::1%en0".into()],
            port: 47615,
            expires_at: 2_000,
        }
    }

    #[test]
    fn qr_round_trips() {
        let o = PairingOffer { expires_at: 1_792_935_614_470, ..offer() };
        let url = encode_pairing_url(&o);
        assert!(url.starts_with("signet://link?d=1~"));
        assert_eq!(decode_pairing_url(&url, 1_000), PairingDecode::Ok(PairingOffer { expires_at: 1_792_935_615_000, ..offer() }));
    }

    #[test]
    fn qr_stays_short() {
        let o = PairingOffer { hosts: vec!["192.168.219.102".into()], expires_at: 1_792_935_614_470, ..offer() };
        assert!(encode_pairing_url(&o).len() < 180);
    }

    #[test]
    fn still_reads_the_old_format() {
        let legacy = json!({
            "v": 1, "desktop": { "name": desktop().name }, "linkId": "0123456789abcdef", "certSha256": TOKEN, "code": CODE,
            "hosts": ["192.168.1.20", "fe80::1%en0"], "port": 47615, "expiresAt": 5000
        });
        let url = format!("{PAIRING_URL_PREFIX}{}", b64::encode_text(&legacy.to_string()));
        assert_eq!(decode_pairing_url(&url, 1_000), PairingDecode::Ok(PairingOffer { expires_at: 5_000, ..offer() }));
    }

    #[test]
    fn rejects_bad_qr() {
        assert_eq!(decode_pairing_url(&encode_pairing_url(&offer()), 3_000), PairingDecode::Expired);
        assert_eq!(decode_pairing_url(&encode_pairing_url(&PairingOffer { v: 2, ..offer() }), 0), PairingDecode::Version);
        assert_eq!(decode_pairing_url(&encode_pairing_url(&PairingOffer { code: "court".into(), ..offer() }), 0), PairingDecode::Invalid);
        assert_eq!(decode_pairing_url(&encode_pairing_url(&PairingOffer { hosts: vec![], ..offer() }), 0), PairingDecode::Invalid);
        assert_eq!(decode_pairing_url("https://example.com", 0), PairingDecode::Invalid);
        assert_eq!(decode_pairing_url("signet://link?d=%%%", 0), PairingDecode::Invalid);
        assert_eq!(decode_pairing_url("signet://link?d=1~a~b", 0), PairingDecode::Invalid);
    }

    #[test]
    fn reads_first_lines_from_the_phone() {
        let pair = LinkMessage::Pair { v: 1, link_id: "0123456789abcdef".into(), code: CODE.into(), device: phone() };
        assert_eq!(parse_link_message(&encode_link_message(&pair)), Some(pair));
        let control = LinkMessage::HelloControl { v: 1, link_id: "0123456789abcdef".into(), token: TOKEN.into(), device: phone() };
        assert_eq!(parse_link_message(&encode_link_message(&control)), Some(control));
        let tunnel = LinkMessage::HelloTunnel { v: 1, link_id: "0123456789abcdef".into(), token: TOKEN.into(), port: 5173 };
        assert_eq!(parse_link_message(&encode_link_message(&tunnel)), Some(tunnel));
    }

    #[test]
    fn rejects_malformed_input() {
        let p = |v: Value| parse_link_message(&v.to_string());
        assert_eq!(parse_link_message("pas du json"), None);
        assert_eq!(parse_link_message("[]"), None);
        assert_eq!(p(json!({"type":"hello","v":1,"linkId":"XYZ","token":TOKEN,"role":"tunnel","port":1})), None);
        assert_eq!(p(json!({"type":"hello","v":1,"linkId":"0123456789abcdef","token":TOKEN,"role":"tunnel","port":70000})), None);
        assert_eq!(p(json!({"type":"error","code":"nope"})), None);
        assert_eq!(p(json!({"type":"open","url":"javascript:alert(1)"})), None);
        assert_eq!(p(json!({"type":"inconnu"})), None);
    }

    #[test]
    fn cleans_names_and_the_server_list() {
        let w = parse_link_message(&json!({"type":"welcome","paused":false,"device":{"id":"desk-1","name":"  Mac\u{7} ","platform":"macos"}}).to_string());
        assert_eq!(w, Some(LinkMessage::Welcome { device: LinkDevice { name: "Mac".into(), ..desktop() }, paused: false }));
        let sv = parse_link_message(
            &json!({"type":"servers","servers":[
                {"url":"http://localhost:5173/","port":5173,"title":" Densho ","fromTab":true},
                {"url":"https://example.com/","port":443},
                {"url":"http://localhost:3000/","port":"x"}]})
            .to_string(),
        );
        assert_eq!(
            sv,
            Some(LinkMessage::Servers {
                servers: vec![DevServer { url: "http://localhost:5173/".into(), port: 5173, title: Some("Densho".into()), from_tab: Some(true) }],
                ports: vec![5173]
            })
        );
        let wp = parse_link_message(&json!({"type":"servers","servers":[],"ports":[5173,3000,3000,0,"x",70000]}).to_string());
        assert_eq!(wp, Some(LinkMessage::Servers { servers: vec![], ports: vec![3000, 5173] }));
    }

    #[test]
    fn maps_the_sni_to_the_link() {
        assert_eq!(link_id_from_server_name(Some(&server_name_for_link("0123456789abcdef"))), Some("0123456789abcdef".into()));
        assert_eq!(link_id_from_server_name(Some("example.com")), None);
        assert_eq!(link_id_from_server_name(None), None);
    }

    #[test]
    fn dev_server_json_uses_the_phone_field_names() {
        let line = encode_link_message(&LinkMessage::Servers {
            servers: vec![DevServer { url: "http://localhost:1/".into(), port: 1, title: None, from_tab: Some(true) }],
            ports: vec![1],
        });
        // Key order depends on serde_json features unified by the workspace: compare the values.
        let value: serde_json::Value = serde_json::from_str(line.trim_end()).unwrap();
        assert_eq!(value, json!({"ports":[1],"servers":[{"fromTab":true,"port":1,"url":"http://localhost:1/"}],"type":"servers"}));
        assert!(line.ends_with('\n'));
    }
}

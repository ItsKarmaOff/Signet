//! Chrome `proxy.ProxyConfig`: validation and conversion. Port of `apis/proxyconfig.ts`.
//!
//! The result is an [`EngineProxy`] (what Electron's `session.setProxy` took) and, for CEF, the `proxy` preference
//! dictionary ([`EngineProxy::to_cef_preference`]) to set on the request context with `SetPreference("proxy", ...)`.

use crate::common::{ApiError, ApiResult};
use base64::Engine;
use serde_json::{json, Map, Value};

const MODES: [&str; 5] = ["direct", "auto_detect", "pac_script", "fixed_servers", "system"];
const SCHEMES: [&str; 5] = ["http", "https", "quic", "socks4", "socks5"];
const SERVER_KEYS: [&str; 5] = ["singleProxy", "proxyForHttp", "proxyForHttps", "proxyForFtp", "fallbackProxy"];

/// What `proxy.settings` reports when no extension controls it: the system proxy.
pub fn system_proxy() -> Value {
    json!({ "mode": "system" })
}

fn err(msg: impl Into<String>) -> ApiError {
    ApiError::new(msg)
}

fn parse_server(value: &Value, key: &str) -> ApiResult<Value> {
    let Some(obj) = value.as_object() else {
        let ty = match value {
            Value::Null => "null",
            Value::Array(_) => "array",
            Value::String(_) => "string",
            Value::Number(_) => "number",
            Value::Bool(_) => "boolean",
            Value::Object(_) => "object",
        };
        return Err(err(format!("Invalid value for argument 1. Property 'rules.{key}': Expected 'object' but got '{ty}'.")));
    };
    // Rules are joined into a `;` separated string: reject anything that could add a rule or a scheme.
    let host = obj.get("host").and_then(|h| h.as_str());
    let bad_host = |h: &str| h.is_empty() || h.chars().any(|c| c.is_whitespace() || ";,=/\\".contains(c));
    let Some(host) = host.filter(|h| !bad_host(h)) else {
        return Err(err(format!("Invalid value for argument 1. Property 'rules.{key}.host': Invalid host.")));
    };
    let mut out = Map::new();
    out.insert("host".into(), json!(host));
    if let Some(p) = obj.get("port").filter(|p| !p.is_null()) {
        match p.as_i64().filter(|n| (1..=65535).contains(n) && p.is_i64()) {
            Some(n) => out.insert("port".into(), json!(n)),
            None => return Err(err(format!("Invalid value for argument 1. Property 'rules.{key}.port': Invalid port."))),
        };
    }
    if let Some(s) = obj.get("scheme").filter(|s| !s.is_null()) {
        match s.as_str().filter(|s| SCHEMES.contains(s)) {
            Some(s) => out.insert("scheme".into(), json!(s)),
            None => return Err(err(format!("Invalid value for argument 1. Property 'rules.{key}.scheme': Invalid scheme."))),
        };
    }
    Ok(Value::Object(out))
}

/// Validates an extension-supplied ProxyConfig and returns a clean copy; errors an extension can read.
pub fn parse_proxy_config(value: &Value) -> ApiResult<Value> {
    let Some(obj) = value.as_object() else { return Err(err("Invalid value for argument 1. Property 'value': Expected 'object'.")) };
    let mode = obj.get("mode").and_then(|m| m.as_str()).filter(|m| MODES.contains(m));
    let Some(mode) = mode else {
        return Err(err(format!("Invalid value for argument 1. Property 'mode': Value must be one of: [{}].", MODES.join(", "))));
    };
    let mut config = Map::new();
    config.insert("mode".into(), json!(mode));
    if mode == "fixed_servers" {
        let Some(rules) = obj.get("rules").and_then(|r| r.as_object()) else {
            return Err(err("Invalid value for argument 1. 'rules' is required for fixed_servers mode."));
        };
        let mut clean = Map::new();
        for key in SERVER_KEYS {
            if let Some(v) = rules.get(key).filter(|v| !v.is_null()) {
                clean.insert(key.into(), parse_server(v, key)?);
            }
        }
        if let Some(list) = rules.get("bypassList").filter(|v| !v.is_null()) {
            let ok = list.as_array().filter(|l| l.iter().all(|p| p.as_str().is_some_and(|p| !p.is_empty() && !p.chars().any(|c| c.is_whitespace() || ",;".contains(c)))));
            match ok {
                Some(_) => clean.insert("bypassList".into(), list.clone()),
                None => return Err(err("Invalid value for argument 1. Property 'rules.bypassList': Expected 'array' of non-empty strings without spaces.")),
            };
        }
        if !SERVER_KEYS.iter().any(|k| clean.contains_key(*k)) {
            return Err(err("Invalid value for argument 1. No proxy server given in rules."));
        }
        config.insert("rules".into(), Value::Object(clean));
    } else if mode == "pac_script" {
        let Some(pac) = obj.get("pacScript").and_then(|p| p.as_object()) else {
            return Err(err("Invalid value for argument 1. 'pacScript' is required for pac_script mode."));
        };
        let url = pac.get("url").and_then(|u| u.as_str());
        let data = pac.get("data").and_then(|u| u.as_str());
        if url.is_none() && data.is_none() {
            return Err(err("Invalid value for argument 1. 'pacScript' needs a 'url' or 'data'."));
        }
        if url == Some("") || data == Some("") {
            return Err(err("Invalid value for argument 1. 'pacScript' is empty."));
        }
        let mut out = Map::new();
        if let Some(u) = url {
            out.insert("url".into(), json!(u));
        }
        if let Some(d) = data {
            out.insert("data".into(), json!(d));
        }
        if let Some(m) = pac.get("mandatory").and_then(|m| m.as_bool()) {
            out.insert("mandatory".into(), json!(m));
        }
        config.insert("pacScript".into(), Value::Object(out));
    }
    Ok(Value::Object(config))
}

/// A proxy configuration in the engine's terms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineProxy {
    pub mode: String,
    pub pac_script: Option<String>,
    pub proxy_rules: Option<String>,
    pub proxy_bypass_rules: Option<String>,
}

impl EngineProxy {
    /// The `proxy` preference dictionary of CEF / Chromium (`mode`, `server`, `pac_url`, `bypass_list`).
    pub fn to_cef_preference(&self) -> Value {
        let mut d = Map::new();
        d.insert("mode".into(), json!(self.mode));
        if let Some(p) = &self.pac_script {
            d.insert("pac_url".into(), json!(p));
        }
        if let Some(r) = &self.proxy_rules {
            d.insert("server".into(), json!(r));
        }
        if let Some(b) = &self.proxy_bypass_rules {
            d.insert("bypass_list".into(), json!(b));
        }
        Value::Object(d)
    }
}

/// Chromium's textual form of one proxy: `scheme://host:port` (the port is left out when absent).
fn server_rule(server: &Value) -> String {
    let host = server["host"].as_str().unwrap_or("");
    // IPv6 literals need brackets in a URL-like rule.
    let host = if host.contains(':') && !host.starts_with('[') { format!("[{host}]") } else { host.to_string() };
    let port = server.get("port").and_then(|p| p.as_i64()).map(|p| format!(":{p}")).unwrap_or_default();
    format!("{}://{}{}", server.get("scheme").and_then(|s| s.as_str()).unwrap_or("http"), host, port)
}

/// Chrome ProxyConfig (as returned by [`parse_proxy_config`]) to the engine form.
/// - pac_script with inline `data` becomes a `data:` URL, as Chrome's own proxy API does; `mandatory` is not supported.
/// - fixed_servers: `singleProxy` applies to every scheme; otherwise per-scheme rules, `fallbackProxy` being Chromium's
///   `socks=` entry (used for the schemes with no rule of their own).
pub fn to_engine_proxy(config: &Value) -> EngineProxy {
    let mode = config["mode"].as_str().unwrap_or("system");
    let mut out = EngineProxy { mode: mode.to_string(), pac_script: None, proxy_rules: None, proxy_bypass_rules: None };
    match mode {
        "pac_script" => {
            let pac = &config["pacScript"];
            out.pac_script = Some(match pac.get("url").and_then(|u| u.as_str()) {
                Some(u) => u.to_string(),
                None => format!(
                    "data:application/x-ns-proxy-autoconfig;base64,{}",
                    base64::engine::general_purpose::STANDARD.encode(pac.get("data").and_then(|d| d.as_str()).unwrap_or(""))
                ),
            });
        }
        "fixed_servers" => {
            let rules = &config["rules"];
            out.proxy_rules = Some(if let Some(s) = rules.get("singleProxy") {
                server_rule(s)
            } else {
                let mut parts = Vec::new();
                for (key, prefix) in [("proxyForHttp", "http"), ("proxyForHttps", "https"), ("proxyForFtp", "ftp"), ("fallbackProxy", "socks")] {
                    if let Some(s) = rules.get(key) {
                        parts.push(format!("{prefix}={}", server_rule(s)));
                    }
                }
                parts.join(";")
            });
            if let Some(list) = rules.get("bypassList").and_then(|l| l.as_array()).filter(|l| !l.is_empty()) {
                out.proxy_bypass_rules = Some(list.iter().filter_map(|s| s.as_str()).collect::<Vec<_>>().join(","));
            }
        }
        _ => {}
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn convert(v: Value) -> EngineProxy {
        to_engine_proxy(&parse_proxy_config(&v).unwrap())
    }

    #[test]
    fn maps_the_simple_modes() {
        for m in ["direct", "auto_detect", "system"] {
            let p = convert(json!({ "mode": m }));
            assert_eq!(p, EngineProxy { mode: m.into(), pac_script: None, proxy_rules: None, proxy_bypass_rules: None });
        }
    }

    #[test]
    fn uses_a_pac_url_as_is() {
        assert_eq!(convert(json!({ "mode": "pac_script", "pacScript": { "url": "https://example.com/proxy.pac" } })).pac_script.as_deref(), Some("https://example.com/proxy.pac"));
    }

    #[test]
    fn turns_inline_pac_data_into_a_data_url() {
        let script = "function FindProxyForURL(){return \"DIRECT\";}";
        let out = convert(json!({ "mode": "pac_script", "pacScript": { "data": script } }));
        assert_eq!(out.pac_script.unwrap(), format!("data:application/x-ns-proxy-autoconfig;base64,{}", base64::engine::general_purpose::STANDARD.encode(script)));
    }

    #[test]
    fn single_proxy_covers_every_scheme() {
        let out = convert(json!({ "mode": "fixed_servers", "rules": { "singleProxy": { "scheme": "socks5", "host": "p.example.com", "port": 1080 }, "bypassList": ["<local>", "*.lan"] } }));
        assert_eq!(out.proxy_rules.as_deref(), Some("socks5://p.example.com:1080"));
        assert_eq!(out.proxy_bypass_rules.as_deref(), Some("<local>,*.lan"));
        assert_eq!(convert(json!({ "mode": "fixed_servers", "rules": { "singleProxy": { "host": "p.example.com" } } })).proxy_rules.as_deref(), Some("http://p.example.com"));
    }

    #[test]
    fn per_scheme_rules_fallback_as_socks_ipv6_bracketed() {
        let out = convert(json!({
            "mode": "fixed_servers",
            "rules": {
                "proxyForHttp": { "host": "a.example.com", "port": 80 },
                "proxyForHttps": { "scheme": "https", "host": "b.example.com", "port": 443 },
                "proxyForFtp": { "host": "::1", "port": 21 },
                "fallbackProxy": { "scheme": "socks4", "host": "c.example.com", "port": 1080 }
            }
        }));
        assert_eq!(out.proxy_rules.as_deref(), Some("http=http://a.example.com:80;https=https://b.example.com:443;ftp=http://[::1]:21;socks=socks4://c.example.com:1080"));
        assert!(out.proxy_bypass_rules.is_none());
    }

    #[test]
    fn rejects_invalid_configs() {
        for v in [
            Value::Null,
            json!({}),
            json!({ "mode": "nope" }),
            json!({ "mode": "fixed_servers" }),
            json!({ "mode": "fixed_servers", "rules": {} }),
            json!({ "mode": "fixed_servers", "rules": { "singleProxy": { "host": "a;b=c" } } }),
            json!({ "mode": "fixed_servers", "rules": { "singleProxy": { "host": "a", "port": 70000 } } }),
            json!({ "mode": "fixed_servers", "rules": { "singleProxy": { "host": "a", "scheme": "ftp" } } }),
            json!({ "mode": "fixed_servers", "rules": { "singleProxy": { "host": "a" }, "bypassList": ["a,b"] } }),
            json!({ "mode": "pac_script" }),
            json!({ "mode": "pac_script", "pacScript": {} }),
        ] {
            assert!(parse_proxy_config(&v).is_err(), "{v}");
        }
    }

    #[test]
    fn keeps_a_clean_copy() {
        let out = parse_proxy_config(&json!({ "mode": "fixed_servers", "evil": 1, "rules": { "singleProxy": { "host": "a", "x": 1 } } })).unwrap();
        assert_eq!(out, json!({ "mode": "fixed_servers", "rules": { "singleProxy": { "host": "a" } } }));
    }

    #[test]
    fn cef_preference_dictionary() {
        let p = convert(json!({ "mode": "fixed_servers", "rules": { "singleProxy": { "host": "p", "port": 8 }, "bypassList": ["<local>"] } }));
        assert_eq!(p.to_cef_preference(), json!({ "mode": "fixed_servers", "server": "http://p:8", "bypass_list": "<local>" }));
    }
}

//! Contract between the page-side scripts and the host. Port of `shared/extension-apis.ts` (pure data).

use serde_json::{json, Value};

/// Event name of an alarm.
pub const ALARM_EVENT: &str = "alarms.onAlarm";
pub const PROXY_ERROR_EVENT: &str = "proxy.onProxyError";
/// Internal call: the page script tells the host how its installation went.
pub const INSTALLED_METHOD: &str = "__installed";
/// Event the host sends to ask an extension for proxy credentials: args `[requestKey, details]`.
pub const AUTH_EVENT: &str = "webRequest.onAuthRequired";
/// Call by which a context tells the host how many onAuthRequired listeners it has now: args `[count]`.
pub const AUTH_LISTENERS_METHOD: &str = "webRequest.authListeners";
/// Call by which the extension answers: args `[requestKey, response]`.
pub const AUTH_RESPONSE_METHOD: &str = "webRequest.authResponse";

/// Methods of a ChromeSetting object.
pub const SETTING_METHODS: [&str; 3] = ["get", "set", "clear"];

/// `chrome.privacy` settings other than WebRTC, by namespace, with the default Chrome reports. They are stored and
/// returned but have no effect, so that an extension touching them does not crash.
pub fn privacy_settings() -> Vec<(&'static str, Vec<(&'static str, Value)>)> {
    vec![
        (
            "network",
            vec![
                ("networkPredictionEnabled", json!(true)),
                ("webRTCIPHandlingPolicy", json!("default")),
                ("webRTCMultipleRoutesEnabled", json!(true)),
                ("webRTCNonProxiedUdpEnabled", json!(true)),
            ],
        ),
        (
            "services",
            vec![
                ("alternateErrorPagesEnabled", json!(true)),
                ("autofillAddressEnabled", json!(true)),
                ("autofillCreditCardEnabled", json!(true)),
                ("autofillEnabled", json!(true)),
                ("hotwordSearchEnabled", json!(false)),
                ("passwordSavingEnabled", json!(true)),
                ("safeBrowsingEnabled", json!(true)),
                ("safeBrowsingExtendedReportingEnabled", json!(false)),
                ("searchSuggestEnabled", json!(true)),
                ("spellingServiceEnabled", json!(false)),
                ("translationServiceEnabled", json!(true)),
            ],
        ),
        (
            "websites",
            vec![
                ("adMeasurementEnabled", json!(true)),
                ("doNotTrackEnabled", json!(false)),
                ("fledgeEnabled", json!(true)),
                ("hyperlinkAuditingEnabled", json!(true)),
                ("protectedContentEnabled", json!(true)),
                ("referrersEnabled", json!(true)),
                ("relatedWebsiteSetsEnabled", json!(true)),
                ("thirdPartyCookiesAllowed", json!(true)),
                ("topicsEnabled", json!(true)),
            ],
        ),
    ]
}

/// Namespaces the page script defines, with the manifest permissions that unlock each one (any of them).
pub fn namespace_permissions() -> Value {
    json!({
        "proxy": ["proxy"],
        "privacy": ["privacy"],
        "alarms": ["alarms"],
        "offscreen": ["offscreen"],
        "webRequest": ["webRequest", "webRequestAuthProvider"],
        "declarativeNetRequest": ["declarativeNetRequest", "declarativeNetRequestWithHostAccess"]
    })
}

/// `chrome.offscreen.Reason` values.
pub const OFFSCREEN_REASONS: [&str; 15] = [
    "TESTING",
    "AUDIO_PLAYBACK",
    "IFRAME_SCRIPTING",
    "DOM_SCRAPING",
    "BLOBS",
    "DOM_PARSER",
    "USER_MEDIA",
    "DISPLAY_MEDIA",
    "WEB_RTC",
    "CLIPBOARD",
    "LOCAL_STORAGE",
    "WORKERS",
    "BATTERY_STATUS",
    "MATCH_MEDIA",
    "GEOLOCATION",
];

/// `chrome.declarativeNetRequest` enums: name to VALUE (Chrome uses camelCase and lowercase strings).
pub fn dnr_enums() -> Value {
    json!({
        "RuleActionType": { "BLOCK": "block", "REDIRECT": "redirect", "ALLOW": "allow", "UPGRADE_SCHEME": "upgradeScheme",
                            "MODIFY_HEADERS": "modifyHeaders", "ALLOW_ALL_REQUESTS": "allowAllRequests" },
        "HeaderOperation": { "APPEND": "append", "SET": "set", "REMOVE": "remove" },
        "ResourceType": { "MAIN_FRAME": "main_frame", "SUB_FRAME": "sub_frame", "STYLESHEET": "stylesheet", "SCRIPT": "script",
                          "IMAGE": "image", "FONT": "font", "OBJECT": "object", "XMLHTTPREQUEST": "xmlhttprequest", "PING": "ping",
                          "CSP_REPORT": "csp_report", "MEDIA": "media", "WEBSOCKET": "websocket", "WEBTRANSPORT": "webtransport",
                          "WEBBUNDLE": "webbundle", "OTHER": "other" },
        "RequestMethod": { "CONNECT": "connect", "DELETE": "delete", "GET": "get", "HEAD": "head", "OPTIONS": "options",
                           "PATCH": "patch", "POST": "post", "PUT": "put", "OTHER": "other" },
        "DomainType": { "FIRST_PARTY": "firstParty", "THIRD_PARTY": "thirdParty" },
        "UnsupportedRegexReason": { "SYNTAX_ERROR": "syntaxError", "MEMORY_LIMIT_EXCEEDED": "memoryLimitExceeded" }
    })
}

pub fn dnr_constants() -> Value {
    json!({
        "MAX_NUMBER_OF_DYNAMIC_RULES": 30000,
        "MAX_NUMBER_OF_UNSAFE_DYNAMIC_RULES": 5000,
        "MAX_NUMBER_OF_SESSION_RULES": 5000,
        "MAX_NUMBER_OF_UNSAFE_SESSION_RULES": 5000,
        "GUARANTEED_MINIMUM_STATIC_RULES": 30000,
        "MAX_NUMBER_OF_STATIC_RULESETS": 100,
        "MAX_NUMBER_OF_ENABLED_STATIC_RULESETS": 50,
        "MAX_NUMBER_OF_REGEX_RULES": 1000,
        "DYNAMIC_RULESET_ID": "_dynamic",
        "SESSION_RULESET_ID": "_session"
    })
}

/// Whether a script runs in an extension context. Strict on purpose: only a `chrome-extension://` URL qualifies.
/// - worker: its script URL starts with chrome-extension://
/// - page: a chrome-extension:// document, top frame or subframe, unless it is sandboxed (opaque origin, no APIs).
pub fn is_extension_context(is_worker: bool, url: Option<&str>, sandboxed: bool) -> bool {
    if !url.is_some_and(|u| u.starts_with("chrome-extension://")) {
        return false;
    }
    is_worker || !sandboxed
}

/// The plan handed to the page-side installer (plain data).
pub fn install_plan() -> Value {
    let settings: serde_json::Map<String, Value> = privacy_settings()
        .into_iter()
        .map(|(ns, list)| (ns.to_string(), Value::Array(list.into_iter().map(|(k, _)| json!(k)).collect())))
        .collect();
    json!({
        "settings": settings,
        "settingMethods": SETTING_METHODS,
        "alarmEvent": ALARM_EVENT,
        "proxyErrorEvent": PROXY_ERROR_EVENT,
        "namespacePermissions": namespace_permissions(),
        "offscreenReasons": OFFSCREEN_REASONS,
        "dnrEnums": dnr_enums(),
        "dnrConstants": dnr_constants(),
        "authEvent": AUTH_EVENT,
        "authResponseMethod": AUTH_RESPONSE_METHOD,
        "authListenersMethod": AUTH_LISTENERS_METHOD,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    const ID: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    #[test]
    fn extension_service_worker_yes() {
        assert!(is_extension_context(true, Some(&format!("chrome-extension://{ID}/sw.js")), false));
    }

    #[test]
    fn web_site_service_worker_no() {
        assert!(!is_extension_context(true, Some("https://example.com/sw.js"), false));
        assert!(!is_extension_context(true, None, false));
    }

    #[test]
    fn extension_page_yes_sandboxed_no() {
        let u = format!("chrome-extension://{ID}/popup.html");
        assert!(is_extension_context(false, Some(&u), false));
        assert!(!is_extension_context(false, Some(&u), true));
    }

    #[test]
    fn web_page_no() {
        assert!(!is_extension_context(false, Some("https://example.com/"), false));
        assert!(!is_extension_context(false, None, false));
    }

    #[test]
    fn plan_lists_every_privacy_setting() {
        let plan = install_plan();
        assert_eq!(plan["settings"]["network"][1], "webRTCIPHandlingPolicy");
        assert_eq!(plan["dnrEnums"]["RuleActionType"]["BLOCK"], "block");
    }
}

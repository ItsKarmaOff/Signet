//! Contract with the web layer (`signet-web`): the two hidden functions, and the JS to call to answer a page.
//!
//! - page to browser: `window.<POST_FN>(channel, json)` is provided by `signet-web` in every frame, before the scripts
//!   run. The bridge captures it and removes it from `window`. `json` is `{"rid"?: string, "data": any}`.
//! - browser to page: `window.<DISPATCH_FN>(channel, json)` is defined by the bundle (see [`crate::scripts::bundle`]).
//!   `signet-web` / `signet-app` execute the string built by [`dispatch_js`] in the frame.
//!
//! Both names live here and nowhere else (the JS receives them as arguments).

use serde::Serialize;

/// Function the web layer provides to send a message to the browser process.
pub const POST_FN: &str = "__signetPost";
/// Function the bundle defines to receive replies and events.
pub const DISPATCH_FN: &str = "__signetDispatch";
/// Channel of replies to requests.
pub const REPLY_CHANNEL: &str = "reply";

/// JS statement that delivers `json` (already serialized) on `channel` in a frame.
pub fn dispatch_js(channel: &str, json: &str) -> String {
    // serde_json escapes both strings into valid JS string literals (U+2028/2029 included, they are legal in JS strings since ES2019).
    let ch = serde_json::to_string(channel).unwrap_or_else(|_| "\"\"".into());
    let payload = serde_json::to_string(json).unwrap_or_else(|_| "\"null\"".into());
    format!("window[{}]({},{});", serde_json::to_string(DISPATCH_FN).unwrap_or_default(), ch, payload)
}

/// JS statement that delivers an event (`value` is the event's data).
pub fn event_js<T: Serialize>(channel: &str, value: &T) -> Option<String> {
    serde_json::to_string(value).ok().map(|json| dispatch_js(channel, &json))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatch_js_quotes_everything() {
        let js = dispatch_js("a\"b", "{\"x\":\"</script>\\n\"}");
        assert!(js.starts_with("window[\"__signetDispatch\"](\"a\\\"b\",\"{"));
        assert!(js.ends_with(");"));
        // The inner JSON is one string literal: quotes and newlines are escaped, nothing breaks out of it.
        assert!(!js.contains('\n'));
        assert!(js.contains("{\\\"x\\\":\\\"</script>"));
    }

    #[test]
    fn event_js_serializes_value() {
        let js = event_js("locale", &"fr").unwrap_or_default();
        assert_eq!(js, "window[\"__signetDispatch\"](\"locale\",\"\\\"fr\\\"\");");
    }
}

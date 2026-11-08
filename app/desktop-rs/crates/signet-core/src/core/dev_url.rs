//! Ports `app/desktop/src/main/core/dev-url.ts`.
//!
//! The TS module only exists so that Electron's Vite dev server does not share a Chromium zoom level with
//! `localhost` pages. The Rust UI has no dev server, so there is nothing to port; the function is kept for
//! parity (and for any web-based tooling) and is not used by the app.

pub const DEV_UI_HOST: &str = "signet-ui.localhost";

/// The Vite server address with its host replaced by `DEV_UI_HOST`; unchanged when it is not a loopback address.
pub fn dev_ui_url(address: &str) -> String {
    let Ok(mut u) = url::Url::parse(address) else { return address.to_string() };
    match u.host_str() {
        Some("localhost") | Some("127.0.0.1") | Some("[::1]") => {}
        _ => return address.to_string(),
    }
    if u.set_host(Some(DEV_UI_HOST)).is_err() {
        return address.to_string();
    }
    let s = u.to_string();
    // Keep the original form (no trailing slash added when there was none).
    if address.ends_with('/') { s } else { s.strip_suffix('/').map(String::from).unwrap_or(s) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moves_off_localhost() {
        assert_eq!(dev_ui_url("http://localhost:5173"), "http://signet-ui.localhost:5173");
        assert_eq!(dev_ui_url("http://localhost:5173/"), "http://signet-ui.localhost:5173/");
        assert_eq!(dev_ui_url("http://127.0.0.1:5174/index.html"), "http://signet-ui.localhost:5174/index.html");
    }

    #[test]
    fn leaves_others() {
        assert_eq!(dev_ui_url("http://example.test:5173"), "http://example.test:5173");
        assert_eq!(dev_ui_url("not a url"), "not a url");
    }
}

//! The `signet://` scheme handler (port of `web/internal.ts` `attachInternal`): note, Easel and pages documents are
//! served by `signet_pages`, the information pages by `signet_features::internal`. Runs on a CEF IO thread.

use std::sync::RwLock;

use signet_features::internal;
use signet_web::SchemeResponse;

/// Site languages shown by `signet://version` (kept in sync by `phase3::web_pages`).
static SITE_LANGUAGES: RwLock<Vec<String>> = RwLock::new(Vec::new());

pub fn set_site_languages(langs: Vec<String>) {
    if let Ok(mut l) = SITE_LANGUAGES.write() {
        *l = langs;
    }
}

fn tr(key: &str, args: &[(&str, String)]) -> String {
    let a: Vec<(&str, &str)> = args.iter().map(|(k, v)| (*k, v.as_str())).collect();
    signet_core::i18n::t_args(key, &a)
}

/// An information page: the CSP of `web/internal.ts` goes in a `<meta>` (the scheme handler cannot set headers).
fn html(body: String, status: u16) -> SchemeResponse {
    let meta = format!(r#"<meta http-equiv="Content-Security-Policy" content="{}">"#, attr_escape(internal::CSP));
    let body = body.replacen("<head>", &format!("<head>{meta}"), 1);
    SchemeResponse { mime: "text/html; charset=utf-8".into(), body: body.into_bytes(), status }
}

fn attr_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('"', "&quot;")
}

fn os_name() -> &'static str {
    match std::env::consts::OS {
        "macos" => "macOS",
        "windows" => "Windows",
        _ => "Linux",
    }
}

/// Rows of `signet://version` (port of `versionPage` in `web/internal.ts`).
fn version_rows() -> Vec<(String, String)> {
    let t = |k: &str| signet_core::i18n::t(k);
    let ua = signet_web::default_user_agent();
    let chrome = ua.split("Chrome/").nth(1).and_then(|r| r.split_whitespace().next()).unwrap_or("").to_string();
    let data = signet_core::paths::data_dir().unwrap_or_default();
    let langs = SITE_LANGUAGES.read().map(|l| l.join(", ")).unwrap_or_default();
    vec![
        (t("internal.version.app"), format!("Signet {}", env!("CARGO_PKG_VERSION"))),
        (t("internal.version.engine"), chrome),
        (t("internal.version.os"), format!("{} ({})", os_name(), std::env::consts::ARCH)),
        (t("internal.version.uiLanguage"), signet_core::i18n::current_locale().id().to_string()),
        (t("internal.version.siteLanguages"), langs),
        (t("internal.version.userAgent"), ua),
        (t("internal.version.profile"), data.display().to_string()),
        (t("internal.version.extensions"), data.join("Extensions").display().to_string()),
    ]
}

pub fn handle(url: &str) -> Option<SchemeResponse> {
    let lower = url.to_ascii_lowercase();
    if !lower.starts_with("signet://") {
        return None;
    }
    let locale = signet_core::i18n::current_locale();
    let lang = locale.id();
    if let Some(r) = signet_pages::serve(url, lang, signet_pages::Theme::Auto) {
        return Some(SchemeResponse { mime: r.mime.to_string(), body: r.body, status: r.status });
    }
    match internal::page_for(url) {
        Some("urls") => Some(html(internal::urls_page(&tr, lang), 200)),
        Some("version") => Some(html(internal::version_page(&tr, lang, &format!("Signet {}", env!("CARGO_PKG_VERSION")), &version_rows()), 200)),
        _ => Some(html(internal::not_found_page(&tr, lang), 404)),
    }
}

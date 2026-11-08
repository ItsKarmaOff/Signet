//! Error pages of a tab (port of `main/web/errorpage.ts`): load failure, HTTPS-only warning, crash.
use crate::util::{esc, Translate};
use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PageErrorText {
    pub title: String,
    pub host: String,
    pub reason: String,
    pub code: String,
    pub retry: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct InsecureSiteText {
    pub title: String,
    pub host: String,
    pub reason: String,
    pub back: String,
    #[serde(rename = "continue")]
    pub continue_: String,
}

fn reason_key(code: i32) -> &'static str {
    match code {
        -106 => "offline",
        -105 | -137 => "dns",
        -102 => "refused",
        -100 | -101 | -103 | -324 => "reset",
        -7 | -118 => "timeout",
        -109 | -21 | -104 => "unreachable",
        -219..=-200 => "certificate",
        _ => "other",
    }
}

pub fn host_of(url: &str) -> String {
    match url::Url::parse(url) {
        Ok(u) => {
            let h = u.host_str().unwrap_or("");
            if h.is_empty() {
                return url.to_string();
            }
            match u.port() {
                Some(p) => format!("{h}:{p}"),
                None => h.to_string(),
            }
        }
        Err(_) => url.to_string(),
    }
}

/// Texts of the "load failed" page from the engine's network error code.
pub fn load_error_text(t: Translate, url: &str, code: i32) -> PageErrorText {
    let key = reason_key(code);
    PageErrorText {
        title: t("pageError.title", &[]),
        host: host_of(url),
        reason: t(&format!("pageError.reason.{key}"), &[]),
        code: if key == "other" { t("pageError.code", &[("code", code.to_string())]) } else { String::new() },
        retry: t("pageError.retry", &[]),
    }
}

pub const ERROR_CSS: &str = r#"
:root { color-scheme: light dark; --bg: #fff; --text: rgb(29 29 31); --text-2: rgb(92 92 98); --text-3: rgb(134 134 142); --accent: hsl(250 70% 55%); --on-accent: #fff }
@media (prefers-color-scheme: dark) { :root { --bg: rgb(24 24 27); --text: rgb(236 236 240); --text-2: rgb(165 164 173); --text-3: rgb(120 120 128); --accent: hsl(250 75% 62%) } }
html, body { margin: 0 !important; height: 100% !important; background: var(--bg) !important; color: var(--text) !important; font: 13px/1.5 -apple-system, BlinkMacSystemFont, system-ui, sans-serif !important }
body { display: flex !important; align-items: center; justify-content: center }
.signet-error { max-width: 380px; padding: 24px; text-align: center }
.signet-error h1 { font-size: 20px; font-weight: 600; letter-spacing: -.01em; margin: 0 0 4px; color: var(--text) }
.signet-error .host { color: var(--text-2); margin: 0 0 14px; word-break: break-all }
.signet-error .reason { color: var(--text); margin: 0 0 4px; font-size: 14px }
.signet-error .code { color: var(--text-3); font-size: 12px; margin: 0 }
.signet-error .btn { display: inline-block; float: none; margin-top: 20px; padding: 6px 16px; border: 0; border-radius: 8px; background: var(--accent); color: var(--on-accent); font: inherit; font-weight: 500; cursor: default; text-decoration: none }
.signet-error .actions { display: flex; gap: 8px; justify-content: center; flex-wrap: wrap; margin-top: 20px }
.signet-error .actions .btn { margin-top: 0 }
.signet-error .btn.secondary { background: transparent; color: var(--text); box-shadow: inset 0 0 0 1px var(--text-3) }
.signet-error .btn:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px }
"#;

fn json<T: Serialize>(v: &T) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| "null".into())
}

/// Script replacing the content of the engine's error page (DOM API only, no `innerHTML`).
pub fn injection_script(text: &PageErrorText, css: &str) -> String {
    format!(
        r#"(() => {{
  if (location.protocol !== 'chrome-error:' || document.querySelector('main.signet-error')) return
  const d = {text}
  const style = document.createElement('style')
  style.textContent = {css}
  const parent = document.head || document.documentElement
  parent.append(style)
  const el = (tag, cls, txt) => {{ const e = document.createElement(tag); if (cls) e.className = cls; if (txt) e.textContent = txt; return e }}
  document.title = d.host
  const box = el('main', 'signet-error')
  box.append(el('h1', '', d.title), el('p', 'host', d.host), el('p', 'reason', d.reason))
  if (d.code) box.append(el('p', 'code', d.code))
  const btn = el('button', 'btn', d.retry)
  btn.type = 'button'
  btn.addEventListener('click', () => location.reload())
  box.append(btn)
  document.body.className = ''
  document.body.replaceChildren(box)
  btn.focus()
}})()"#,
        text = json(text),
        css = json(&css)
    )
}

pub fn insecure_site_text(t: Translate, url: &str) -> InsecureSiteText {
    InsecureSiteText {
        title: t("pageError.https.title", &[]),
        host: host_of(url),
        reason: t("pageError.https.text", &[]),
        back: t("pageError.https.back", &[]),
        continue_: t("pageError.https.continue", &[]),
    }
}

/// Script replacing the error page with the HTTPS-only warning. "Continue" sets the title to `title_prefix + token`.
pub fn insecure_site_script(text: &InsecureSiteText, css: &str, token: &str, title_prefix: &str, can_go_back: bool) -> String {
    format!(
        r#"(() => {{
  if (location.protocol !== 'chrome-error:') return
  const d = {text}
  const style = document.createElement('style')
  style.textContent = {css}
  const parent = document.head || document.documentElement
  parent.append(style)
  const el = (tag, cls, txt) => {{ const e = document.createElement(tag); if (cls) e.className = cls; if (txt) e.textContent = txt; return e }}
  document.title = d.host
  const box = el('main', 'signet-error')
  box.append(el('h1', '', d.title), el('p', 'host', d.host), el('p', 'reason', d.reason))
  const actions = el('div', 'actions')
  const button = (cls, label, fn) => {{
    const b = el('button', cls, label)
    b.type = 'button'
    b.addEventListener('click', fn)
    return b
  }}
  const back = {back} ? button('btn', d.back, () => history.back()) : null
  const next = button(back ? 'btn secondary' : 'btn', d.continue, () => {{ document.title = {title} }})
  actions.append(...(back ? [back, next] : [next]))
  box.append(actions)
  document.body.className = ''
  document.body.replaceChildren(box)
  ;(back || next).focus()
}})()"#,
        text = json(text),
        css = json(&css),
        back = if can_go_back { "true" } else { "false" },
        title = json(&format!("{title_prefix}{token}"))
    )
}

/// Standalone "this page crashed" document.
pub fn crash_document(t: Translate, url: &str) -> String {
    let host = host_of(url);
    let lower = url.to_ascii_lowercase();
    let linkable = lower.starts_with("http:") || lower.starts_with("https:") || lower.starts_with("file:");
    let link = if linkable { esc(url) } else { String::new() };
    let a = if link.is_empty() {
        String::new()
    } else {
        format!(r#"<a class="btn" href="{link}" autofocus>{}</a>"#, esc(&t("pageError.crash.retry", &[])))
    };
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><title>{}</title><style>{}</style></head><body><main class="signet-error"><h1>{}</h1><p class="host">{}</p><p class="reason">{}</p>{}</main></body></html>"#,
        esc(&host),
        ERROR_CSS,
        esc(&t("pageError.crash.title", &[])),
        esc(&host),
        esc(&t("pageError.crash.text", &[])),
        a
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tr(k: &str, p: &[(&str, String)]) -> String {
        if p.is_empty() { k.to_string() } else { format!("{k}:{}", p[0].1) }
    }
    #[test]
    fn known_cause_has_no_code() {
        let e = load_error_text(&tr, "https://example.com/a", -105);
        assert_eq!(e.host, "example.com");
        assert_eq!(e.code, "");
        assert_ne!(e.reason, "");
    }
    #[test]
    fn groups_codes() {
        let r = |c| load_error_text(&tr, "https://a.com", c).reason;
        assert_eq!(r(-105), r(-137));
        assert_eq!(r(-201), r(-219));
        assert_ne!(r(-106), r(-105));
    }
    #[test]
    fn unknown_shows_code() {
        assert!(load_error_text(&tr, "https://a.com", -999).code.contains("-999"));
    }
    #[test]
    fn invalid_url() {
        assert_eq!(load_error_text(&tr, "pas une url", -105).host, "pas une url");
    }
    #[test]
    fn crash_escapes() {
        let html = crash_document(&tr, "https://example.com/?a=\"><script>alert(1)</script>");
        assert!(!html.contains("<script>"));
        assert!(html.contains("&quot;&gt;&lt;script&gt;"));
        assert!(!crash_document(&tr, "javascript:alert(1)").contains("<a "));
        assert!(!crash_document(&tr, "signet://settings").contains("<a "));
    }
    #[test]
    fn injection_serializes_json() {
        let text = PageErrorText { title: "T".into(), host: "</script>\"".into(), reason: "r".into(), code: "".into(), retry: "x".into() };
        let js = injection_script(&text, "a{}");
        assert!(js.contains(&serde_json::to_string(&text).unwrap()));
        assert!(js.contains("location.protocol !== 'chrome-error:'"));
    }
    #[test]
    fn insecure() {
        let text = insecure_site_text(&tr, "https://example.com:8443/a");
        assert_eq!(text.host, "example.com:8443");
        assert_ne!(text.continue_, text.back);
        let js = insecure_site_script(&insecure_site_text(&tr, "https://example.com"), ERROR_CSS, "tok-123", "signet-https-continue:", true);
        assert!(js.contains("signet-https-continue:tok-123"));
        assert!(!js.contains("innerHTML"));
        assert!(js.contains("history.back()"));
        let js2 = insecure_site_script(&insecure_site_text(&tr, "https://example.com"), ERROR_CSS, "t", "p:", false);
        assert!(js2.contains("false ? button"));
    }
}

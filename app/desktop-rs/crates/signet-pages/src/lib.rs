//! signet-pages: internal web pages rendered by CEF and served through the `signet://` scheme.
//!
//! Pages (see README.md): Note (`signet://note/<id>`), Easel (`signet://easel/<id>`), their static files
//! (`signet://pages/<path>`), and the JSON viewer script (injected, see [`json_viewer`]).
//! The responsive mode bar is interface chrome and lives in the GPUI crate; [`devices`] only holds its data.

pub mod devices;
pub mod json_viewer;
pub mod strings;

use serde_json::{Value, json};

/// Appearance forced by the app (`Auto` follows the system through `prefers-color-scheme`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Theme {
    #[default]
    Auto,
    Light,
    Dark,
}

impl Theme {
    fn id(self) -> &'static str {
        match self {
            Theme::Auto => "auto",
            Theme::Light => "light",
            Theme::Dark => "dark",
        }
    }
    fn class(self) -> &'static str {
        match self {
            Theme::Auto => "",
            Theme::Light => "theme-light",
            Theme::Dark => "theme-dark",
        }
    }
}

/// What the scheme handler answers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Response {
    pub mime: &'static str,
    pub body: Vec<u8>,
    pub status: u16,
}

/// Kind of document page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocKind {
    Note,
    Easel,
}

/// A route owned by this crate, for the `signet://urls` list and the scheme handler.
#[derive(Clone, Copy, Debug)]
pub struct Route {
    /// Pattern, e.g. `signet://note/<id>`.
    pub pattern: &'static str,
    pub description: &'static str,
    /// Listed in `signet://urls`? (Documents are opened from the interface, not typed.)
    pub listed: bool,
}

pub const ROUTES: &[Route] = &[
    Route { pattern: "signet://note/<id>", description: "A rich text note (UUID).", listed: false },
    Route { pattern: "signet://easel/<id>", description: "An Easel, free-form board (UUID).", listed: false },
    Route { pattern: "signet://pages/<path>", description: "Static files of the pages above (scripts, styles).", listed: false },
];

/// Origin the pages load their files from.
pub const ASSETS_ORIGIN: &str = "signet://pages";

struct Asset {
    path: &'static str,
    mime: &'static str,
    body: &'static str,
}

const CSS: &str = "text/css; charset=utf-8";
const JS: &str = "text/javascript; charset=utf-8";
const HTML: &str = "text/html; charset=utf-8";

const ASSETS: &[Asset] = &[
    Asset { path: "shared/tokens.css", mime: CSS, body: include_str!("../web/shared/tokens.css") },
    Asset { path: "shared/notes.css", mime: CSS, body: include_str!("../web/shared/notes.css") },
    Asset { path: "shared/notes-html.js", mime: JS, body: include_str!("../web/shared/notes-html.js") },
    Asset { path: "shared/common.js", mime: JS, body: include_str!("../web/shared/common.js") },
    Asset { path: "shared/spring.js", mime: JS, body: include_str!("../web/shared/spring.js") },
    Asset { path: "note/note.js", mime: JS, body: include_str!("../web/note/note.js") },
    Asset { path: "easel/easel.js", mime: JS, body: include_str!("../web/easel/easel.js") },
];

const NOTE_HTML: &str = include_str!("../web/note/index.html");
const EASEL_HTML: &str = include_str!("../web/easel/index.html");

/// `signet://note/<id>` or `signet://easel/<id>` (id a UUID, case-insensitive, optional trailing slash, query and
/// fragment ignored) to the kind and the lower case id. Same rule as `parseNoteAddress` of the Electron app.
pub fn parse_document_url(url: &str) -> Option<(DocKind, String)> {
    let rest = strip_scheme(url)?;
    let (kind, id) = rest.split_once('/')?;
    let id = id.strip_suffix('/').unwrap_or(id);
    let kind = match kind.to_ascii_lowercase().as_str() {
        "note" => DocKind::Note,
        "easel" => DocKind::Easel,
        _ => return None,
    };
    is_uuid(id).then(|| (kind, id.to_ascii_lowercase()))
}

/// What follows `signet://`, without query nor fragment.
fn strip_scheme(url: &str) -> Option<&str> {
    let url = url.trim();
    let head = url.get(..9)?;
    if !head.eq_ignore_ascii_case("signet://") {
        return None;
    }
    let rest = &url[9..];
    Some(rest.split(['?', '#']).next().unwrap_or(rest))
}

fn is_uuid(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 36 && b.iter().enumerate().all(|(i, c)| if matches!(i, 8 | 13 | 18 | 23) { *c == b'-' } else { c.is_ascii_hexdigit() })
}

/// Answers a `signet://` address owned by this crate, or `None` (the caller tries its other routes, then "not found").
/// `lang` is the interface language (`fr` or `en`, anything else is English).
pub fn serve(url: &str, lang: &str, theme: Theme) -> Option<Response> {
    if let Some((kind, _id)) = parse_document_url(url) {
        return Some(Response { mime: HTML, body: document_page(kind, lang, theme).into_bytes(), status: 200 });
    }
    let rest = strip_scheme(url)?;
    let (host, path) = rest.split_once('/')?;
    if !host.eq_ignore_ascii_case("pages") {
        return None;
    }
    Some(match ASSETS.iter().find(|a| a.path == path) {
        Some(a) => Response { mime: a.mime, body: a.body.as_bytes().to_vec(), status: 200 },
        None => Response { mime: "text/plain; charset=utf-8", body: b"Not found".to_vec(), status: 404 },
    })
}

/// Content-Security-Policy of the document pages is inside the HTML (meta); this is the same string for the
/// response header, so the app may set either.
pub fn document_csp(kind: DocKind) -> String {
    let img = match kind {
        DocKind::Note => "data: http: https:",
        DocKind::Easel => "data: blob: http: https:",
    };
    format!("default-src 'none'; script-src {ASSETS_ORIGIN}; style-src {ASSETS_ORIGIN} 'unsafe-inline'; img-src {img}; font-src data:")
}

fn document_page(kind: DocKind, lang: &str, theme: Theme) -> String {
    let lang = strings::pick_lang(lang);
    let (template, sections, title_key) = match kind {
        DocKind::Note => (NOTE_HTML, &["notes"][..], "notes.untitled"),
        DocKind::Easel => (EASEL_HTML, &["easel"][..], "easel.untitled"),
    };
    let config = json!({
        "lang": lang,
        "theme": theme.id(),
        "strings": Value::Object(strings::ui_subset(lang, sections, &["common.cancel", "notes.delete"])),
    });
    // The JSON sits in a data block: only `<` must not close it.
    let config = config.to_string().replace('<', "\\u003c");
    let title = strings::ui(lang, title_key).unwrap_or_default();
    template
        .replace("<title>Note</title>", &format!("<title>{}</title>", escape(&title)))
        .replace("<title>Easel</title>", &format!("<title>{}</title>", escape(&title)))
        .replace("{{LANG}}", lang)
        .replace("{{THEME_CLASS}}", theme.class())
        .replace("{{ASSETS}}", ASSETS_ORIGIN)
        .replace("{{CONFIG}}", &config)
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    const ID: &str = "3f2b8c1e-5d4a-4e6f-9a7b-0c1d2e3f4a5b";

    fn body(r: &Response) -> String {
        String::from_utf8(r.body.clone()).unwrap_or_default()
    }

    #[test]
    fn parses_document_urls() {
        assert_eq!(parse_document_url(&format!("signet://note/{ID}")), Some((DocKind::Note, ID.into())));
        assert_eq!(parse_document_url(&format!("SIGNET://Easel/{}/", ID.to_uppercase())), Some((DocKind::Easel, ID.into())));
        assert_eq!(parse_document_url(&format!("signet://note/{ID}?x=1#y")), Some((DocKind::Note, ID.into())));
        for bad in ["signet://note/abc", "signet://note/", "signet://note", "signet://other/3f2b8c1e-5d4a-4e6f-9a7b-0c1d2e3f4a5b", "https://note/x", "signet://note/zzzzzzzz-5d4a-4e6f-9a7b-0c1d2e3f4a5b", ""] {
            assert_eq!(parse_document_url(bad), None, "{bad}");
        }
    }

    #[test]
    fn serves_note_page() {
        let r = serve(&format!("signet://note/{ID}"), "fr", Theme::Dark).expect("served");
        assert_eq!(r.status, 200);
        assert!(r.mime.starts_with("text/html"));
        let html = body(&r);
        assert!(html.contains("lang=\"fr\""));
        assert!(html.contains("class=\"theme-dark\""));
        assert!(html.contains("<title>Note sans titre</title>"));
        assert!(html.contains("signet://pages/note/note.js"));
        assert!(html.contains("\"notes.bold\":\"Gras\"") || html.contains("\"notes.bold\""));
        assert!(!html.contains("{{"));
    }

    #[test]
    fn serves_easel_page_in_english_auto() {
        let html = body(&serve(&format!("signet://easel/{ID}"), "en", Theme::Auto).expect("served"));
        assert!(html.contains("class=\"\""));
        assert!(html.contains("<title>Untitled Easel</title>"));
        assert!(html.contains("signet://pages/easel/easel.js"));
        assert!(html.contains("\"easel.empty\""));
        assert!(!html.contains("\"notes.bold\""));
    }

    #[test]
    fn config_cannot_close_its_script() {
        for kind in [DocKind::Note, DocKind::Easel] {
            let html = document_page(kind, "en", Theme::Auto);
            let start = html.find("id=\"signet-config\">").expect("config") + 19;
            let end = html[start..].find("</script>").expect("closing") + start;
            assert!(!html[start..end].contains('<'));
            assert!(serde_json::from_str::<Value>(&html[start..end]).is_ok());
        }
    }

    #[test]
    fn unknown_or_invalid_addresses_are_not_ours() {
        assert_eq!(serve("signet://note/not-a-uuid", "en", Theme::Auto), None);
        assert_eq!(serve("signet://urls", "en", Theme::Auto), None);
        assert_eq!(serve("https://example.com", "en", Theme::Auto), None);
    }

    #[test]
    fn serves_every_asset_and_404s_others() {
        for a in ASSETS {
            let r = serve(&format!("signet://pages/{}", a.path), "en", Theme::Auto).expect("asset");
            assert_eq!(r.status, 200, "{}", a.path);
            assert!(!r.body.is_empty());
        }
        assert_eq!(serve("signet://pages/shared/nope.js", "en", Theme::Auto).map(|r| r.status), Some(404));
        assert_eq!(serve("signet://pages/../secret", "en", Theme::Auto).map(|r| r.status), Some(404));
    }

    #[test]
    fn every_file_referenced_by_the_pages_is_served() {
        for html in [NOTE_HTML, EASEL_HTML] {
            for part in html.split("{{ASSETS}}/").skip(1) {
                let path = part.split('"').next().unwrap_or("");
                if path.contains(' ') || path.contains(';') {
                    continue;
                }
                assert!(ASSETS.iter().any(|a| a.path == path), "missing asset {path}");
            }
        }
    }

    #[test]
    fn no_dash_characters_in_sources() {
        for text in [NOTE_HTML, EASEL_HTML, include_str!("../README.md")] {
            assert!(!text.contains('\u{2014}') && !text.contains('\u{2013}'));
        }
        for a in ASSETS {
            assert!(!a.body.contains('\u{2014}') && !a.body.contains('\u{2013}'), "{}", a.path);
        }
    }

    /// Syntax check of every script with node, when node is installed (skipped otherwise).
    #[test]
    fn scripts_parse_with_node() {
        let dir = std::env::temp_dir().join(format!("signet-pages-{}", std::process::id()));
        if std::fs::create_dir_all(&dir).is_err() || Command::new("node").arg("-v").output().is_err() {
            return;
        }
        let mut sources: Vec<(String, String)> = ASSETS.iter().filter(|a| a.path.ends_with(".js")).map(|a| (a.path.replace('/', "_"), a.body.to_owned())).collect();
        sources.push(("viewer.js".into(), json_viewer::script("en", false)));
        for (name, src) in sources {
            let file = dir.join(&name);
            std::fs::write(&file, src).expect("write");
            let out = Command::new("node").arg("--check").arg(&file).output().expect("node");
            assert!(out.status.success(), "{name}: {}", String::from_utf8_lossy(&out.stderr));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}

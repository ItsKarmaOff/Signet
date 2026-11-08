//! RSS 2.0 / RSS 1.0 / Atom reading (port of `packages/shared/src/feeds.ts`), pure functions.
use regex::{Captures, Regex};
use serde_json::Value;
use std::sync::OnceLock;
use url::Url;

#[derive(Debug, Clone, PartialEq)]
pub struct FeedEntry {
    pub url: String,
    pub title: String,
    /// Milliseconds, 0 if unknown.
    pub date: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Feed {
    pub title: String,
    pub entries: Vec<FeedEntry>,
}

fn rx(cell: &'static OnceLock<Regex>, p: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(p).expect("static regex"))
}

/// Decodes common XML/HTML entities (`&amp;`, `&#39;`, `&#x2019;`...).
pub fn decode_entities(text: &str) -> String {
    static R: OnceLock<Regex> = OnceLock::new();
    rx(&R, r"(?i)&(#x[0-9a-f]+|#\d+|[a-z]+);").replace_all(text, |c: &Captures| {
        let m = c[0].to_string();
        let code = &c[1];
        if let Some(rest) = code.strip_prefix('#') {
            let n = match rest.strip_prefix(['x', 'X']) {
                Some(h) => u32::from_str_radix(h, 16).ok(),
                None => rest.parse::<u32>().ok(),
            };
            return match n.filter(|n| *n > 0 && *n < 0x110000).and_then(char::from_u32) {
                Some(ch) => ch.to_string(),
                None => m,
            };
        }
        match code.to_lowercase().as_str() {
            "amp" => "&".into(),
            "lt" => "<".into(),
            "gt" => ">".into(),
            "quot" => "\"".into(),
            "apos" => "'".into(),
            "nbsp" => " ".into(),
            _ => m,
        }
    })
    .into_owned()
}

fn text(raw: Option<&str>) -> String {
    let Some(raw) = raw.filter(|r| !r.is_empty()) else { return String::new() };
    static CD: OnceLock<Regex> = OnceLock::new();
    static TAG: OnceLock<Regex> = OnceLock::new();
    static WS: OnceLock<Regex> = OnceLock::new();
    let s = rx(&CD, r"(?s)<!\[CDATA\[(.*?)\]\]>").replace_all(raw, "$1");
    let s = decode_entities(&rx(&TAG, r"<[^>]+>").replace_all(&s, ""));
    let s = rx(&TAG, r"<[^>]+>").replace_all(&s, "");
    rx(&WS, r"\s+").replace_all(&s, " ").trim().to_string()
}

/// Content of the first `name` tag (any namespace prefix).
fn tag<'a>(xml: &'a str, name: &str) -> Option<&'a str> {
    let n = regex::escape(name);
    let re = Regex::new(&format!(r"(?is)<(?:[\w-]+:)?{n}(?:\s[^>]*)?>(.*?)</(?:[\w-]+:)?{n}>")).ok()?;
    re.captures(xml).and_then(|c| c.get(1)).map(|m| m.as_str())
}

/// Attributes of an opening tag, names lowercased.
pub fn attributes(open: &str) -> Vec<(String, String)> {
    static R: OnceLock<Regex> = OnceLock::new();
    rx(&R, r#"([\w:-]+)\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'>]+))"#)
        .captures_iter(open)
        .map(|m| {
            let v = m.get(2).or(m.get(3)).or(m.get(4)).map(|x| x.as_str()).unwrap_or("");
            (m[1].to_lowercase(), decode_entities(v))
        })
        .collect()
}

fn attr<'a>(attrs: &'a [(String, String)], name: &str) -> Option<&'a str> {
    // JS object semantics: the last duplicate wins.
    attrs.iter().rev().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
}

fn absolute(url: &str, base: &str) -> Option<String> {
    let b = Url::parse(base).ok();
    let u = match &b {
        Some(b) => b.join(url.trim()).ok()?,
        None => Url::parse(url.trim()).ok()?,
    };
    matches!(u.scheme(), "http" | "https").then(|| u.to_string())
}

/// Date.parse equivalent for the formats feeds use; 0 when unknown.
fn time(raw: Option<&str>) -> i64 {
    let t = text(raw);
    if t.is_empty() {
        return 0;
    }
    if let Ok(d) = chrono::DateTime::parse_from_rfc2822(&t) {
        return d.timestamp_millis();
    }
    if let Ok(d) = chrono::DateTime::parse_from_rfc3339(&t) {
        return d.timestamp_millis();
    }
    for f in ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%d %H:%M:%S", "%a, %d %b %Y %H:%M:%S"] {
        if let Ok(d) = chrono::NaiveDateTime::parse_from_str(&t, f) {
            return d.and_utc().timestamp_millis();
        }
    }
    if let Ok(d) = chrono::NaiveDate::parse_from_str(&t, "%Y-%m-%d") {
        return d.and_hms_opt(0, 0, 0).map(|x| x.and_utc().timestamp_millis()).unwrap_or(0);
    }
    0
}

/// Reads an RSS or Atom feed; `None` if it is not a feed.
pub fn parse_feed(xml: &str, base: &str) -> Option<Feed> {
    static FEED: OnceLock<Regex> = OnceLock::new();
    static ENTRY: OnceLock<Regex> = OnceLock::new();
    static RSS: OnceLock<Regex> = OnceLock::new();
    static CHAN: OnceLock<Regex> = OnceLock::new();
    static ENTRY_BLOCK: OnceLock<Regex> = OnceLock::new();
    static ITEM: OnceLock<Regex> = OnceLock::new();
    static ITEM_BLOCK: OnceLock<Regex> = OnceLock::new();
    static LINK: OnceLock<Regex> = OnceLock::new();
    let has_feed = rx(&FEED, r"(?i)<feed[\s>]").is_match(xml);
    let atom = has_feed && rx(&ENTRY, r"(?i)<entry[\s>]").is_match(xml);
    let rss = rx(&RSS, r"(?i)<(rss|rdf:RDF)[\s>]").is_match(xml) || rx(&CHAN, r"(?i)<channel[\s>]").is_match(xml);
    if !atom && !rss && !has_feed {
        return None;
    }
    let mut entries = Vec::new();
    if atom || has_feed {
        let head = rx(&ENTRY, r"(?i)<entry[\s>]").split(xml).next().unwrap_or("");
        let title = text(tag(head, "title"));
        for m in rx(&ENTRY_BLOCK, r"(?is)<entry[\s>](.*?)</entry>").captures_iter(xml) {
            let body = &m[1];
            let links: Vec<Vec<(String, String)>> = rx(&LINK, r"(?is)<link\b([^>]*)/?>").captures_iter(body).map(|l| attributes(&l[1])).collect();
            let link = links
                .iter()
                .find(|a| attr(a, "rel").is_none_or(|r| r.is_empty() || r == "alternate"))
                .or(links.first());
            let Some(url) = link.and_then(|a| attr(a, "href")).filter(|h| !h.is_empty()).and_then(|h| absolute(h, base)) else { continue };
            let t = text(tag(body, "title"));
            entries.push(FeedEntry { title: if t.is_empty() { url.clone() } else { t }, date: time(tag(body, "updated").or(tag(body, "published"))), url });
        }
        return Some(Feed { title, entries });
    }
    let channel = tag(xml, "channel").unwrap_or(xml);
    let title = text(tag(rx(&ITEM, r"(?i)<item[\s>]").split(channel).next().unwrap_or(""), "title"));
    for m in rx(&ITEM_BLOCK, r"(?is)<item[\s>](.*?)</item>").captures_iter(xml) {
        let body = &m[1];
        let mut link = text(tag(body, "link"));
        if link.is_empty() {
            let guid = text(tag(body, "guid"));
            if guid.len() >= 7 && (guid[..7].eq_ignore_ascii_case("http://") || guid.get(..8).is_some_and(|p| p.eq_ignore_ascii_case("https://"))) {
                link = guid;
            }
        }
        let Some(url) = (!link.is_empty()).then(|| absolute(&link, base)).flatten() else { continue };
        let t = text(tag(body, "title"));
        entries.push(FeedEntry {
            title: if t.is_empty() { url.clone() } else { t },
            date: time(tag(body, "pubDate").or(tag(body, "date")).or(tag(body, "updated"))),
            url,
        });
    }
    Some(Feed { title, entries })
}

/// Address of the feed announced by an HTML page, or `None`.
pub fn discover_feed(html: &str, base: &str) -> Option<String> {
    static R: OnceLock<Regex> = OnceLock::new();
    static ALT: OnceLock<Regex> = OnceLock::new();
    static TY: OnceLock<Regex> = OnceLock::new();
    for m in rx(&R, r"(?is)<link\b([^>]*)>").captures_iter(html) {
        let a = attributes(&m[1]);
        if !rx(&ALT, r"(?i)\balternate\b").is_match(attr(&a, "rel").unwrap_or("")) {
            continue;
        }
        if !rx(&TY, r"(?i)^application/(rss|atom)\+xml$").is_match(attr(&a, "type").unwrap_or("").trim()) {
            continue;
        }
        if let Some(u) = attr(&a, "href").filter(|h| !h.is_empty()).and_then(|h| absolute(h, base)) {
            return Some(u);
        }
    }
    None
}

/// True if the text looks like an HTML page rather than a feed.
pub fn looks_like_html(body: &str) -> bool {
    static R: OnceLock<Regex> = OnceLock::new();
    let head: String = body.chars().take(2000).collect();
    rx(&R, r"(?i)^\s*(<!doctype html|<html[\s>])").is_match(&head)
}

// ---------------------------------------------------------------- live folder state

pub use signet_core::types::{GithubQuery, LiveFolder, LiveSource};

/// Validated source (from disk or from the UI), or `None`.
pub fn clean_live_source(value: &Value) -> Option<LiveSource> {
    let o = value.as_object()?;
    match o.get("kind")?.as_str()? {
        "feed" => {
            let url = o.get("url")?.as_str()?.trim();
            static R: OnceLock<Regex> = OnceLock::new();
            rx(&R, r"(?i)^https?://\S+$").is_match(url).then(|| LiveSource::Feed { url: url.chars().take(2000).collect() })
        }
        "github" => match o.get("query")?.as_str()? {
            "review-requested" => Some(LiveSource::Github { query: GithubQuery::ReviewRequested }),
            "mine" => Some(LiveSource::Github { query: GithubQuery::Mine }),
            _ => None,
        },
        _ => None,
    }
}

/// State of a live folder re-read from disk: valid source, bounded fields, never "refreshing".
pub fn clean_live_folder(value: &Value) -> Option<LiveFolder> {
    let o = value.as_object()?;
    let source = clean_live_source(o.get("source")?)?;
    let mut live = LiveFolder { source, refreshed_at: None, error: None, refreshing: None, seen: None };
    if let Some(n) = o.get("refreshedAt").and_then(|v| v.as_f64()).filter(|n| n.is_finite()) {
        live.refreshed_at = Some(n as i64);
    }
    if let Some(e) = o.get("error").and_then(|v| v.as_str()).filter(|e| !e.is_empty()) {
        live.error = Some(e.chars().take(300).collect());
    }
    if let Some(a) = o.get("seen").and_then(|v| v.as_array()) {
        let all: Vec<String> = a.iter().filter_map(|u| u.as_str().map(String::from)).collect();
        let start = all.len().saturating_sub(200);
        live.seen = Some(all[start..].to_vec());
    }
    Some(live)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const RSS: &str = r#"<?xml version="1.0"?>
<rss version="2.0"><channel>
  <title>Mon &amp; Blog</title>
  <item><title><![CDATA[Premier <b>billet</b>]]></title><link>/p/1</link><pubDate>Mon, 01 Jan 2024 10:00:00 GMT</pubDate></item>
  <item><title>Sans lien</title><guid>https://example.com/guid</guid></item>
  <item><title>Rien</title></item>
  <item><link>javascript:alert(1)</link><title>Mauvais</title></item>
  <item><link>https://ex.com/3</link></item>
</channel></rss>"#;

    const ATOM: &str = r#"<feed xmlns="http://www.w3.org/2005/Atom"><title>Atom</title>
  <entry><title>Un</title><link rel="self" href="/self"/><link rel="alternate" href="/un"/><updated>2024-02-03T04:05:06Z</updated></entry>
  <entry><title>Deux</title><link href="https://ex.com/deux"/></entry>
  <entry><title>Sans lien</title></entry>
</feed>"#;

    fn ms(s: &str) -> i64 { chrono::DateTime::parse_from_rfc3339(s).unwrap().timestamp_millis() }

    #[test]
    fn entities() {
        assert_eq!(decode_entities("a &amp; b &lt;c&gt; &quot;d&quot; &#39;e&#39; &#x2019;"), "a & b <c> \"d\" 'e' \u{2019}");
        assert_eq!(decode_entities("&foo; &#0; &#x110000;"), "&foo; &#0; &#x110000;");
    }

    #[test]
    fn attrs() {
        let a = attributes(" rel=\"alternate\" type='a/b' href=/x?a=1&amp;b=2 ");
        assert_eq!(attr(&a, "rel"), Some("alternate"));
        assert_eq!(attr(&a, "type"), Some("a/b"));
        assert_eq!(attr(&a, "href"), Some("/x?a=1&b=2"));
    }

    #[test]
    fn rss() {
        let f = parse_feed(RSS, "https://example.com/feed").unwrap();
        assert_eq!(f.title, "Mon & Blog");
        assert_eq!(f.entries, vec![
            FeedEntry { url: "https://example.com/p/1".into(), title: "Premier billet".into(), date: ms("2024-01-01T10:00:00Z") },
            FeedEntry { url: "https://example.com/guid".into(), title: "Sans lien".into(), date: 0 },
            FeedEntry { url: "https://ex.com/3".into(), title: "https://ex.com/3".into(), date: 0 },
        ]);
    }

    #[test]
    fn atom() {
        let f = parse_feed(ATOM, "https://example.com/").unwrap();
        assert_eq!(f.title, "Atom");
        assert_eq!(f.entries, vec![
            FeedEntry { url: "https://example.com/un".into(), title: "Un".into(), date: ms("2024-02-03T04:05:06Z") },
            FeedEntry { url: "https://ex.com/deux".into(), title: "Deux".into(), date: 0 },
        ]);
    }

    #[test]
    fn not_a_feed() {
        assert_eq!(parse_feed("<html><body>hi</body></html>", "https://e.com"), None);
        assert_eq!(parse_feed("", "https://e.com"), None);
        assert_eq!(parse_feed("<rss><channel><title>T</title></channel></rss>", "https://e.com"), Some(Feed { title: "T".into(), entries: vec![] }));
    }

    #[test]
    fn discover() {
        let html = r#"<head><link rel="stylesheet" href="/a.css"><link rel="alternate" type="application/atom+xml" href="/feed.xml"></head>"#;
        assert_eq!(discover_feed(html, "https://e.com/blog/").as_deref(), Some("https://e.com/feed.xml"));
        assert_eq!(discover_feed(r#"<link rel="alternate" type="text/html" href="/x">"#, "https://e.com"), None);
        assert_eq!(discover_feed(r#"<link rel="alternate" type="application/rss+xml" href="javascript:1">"#, "https://e.com"), None);
        assert!(looks_like_html("  <!DOCTYPE html><html>"));
        assert!(looks_like_html("<HTML lang=\"fr\">"));
        assert!(!looks_like_html("<?xml version=\"1.0\"?><rss>"));
    }

    #[test]
    fn clean_source_and_folder() {
        assert_eq!(clean_live_source(&json!({"kind":"feed","url":" https://e.com/rss "})), Some(LiveSource::Feed { url: "https://e.com/rss".into() }));
        assert_eq!(clean_live_source(&json!({"kind":"feed","url":"ftp://e.com"})), None);
        assert_eq!(clean_live_source(&json!({"kind":"feed","url":"https://e.com a"})), None);
        assert_eq!(clean_live_source(&json!({"kind":"github","query":"mine"})), Some(LiveSource::Github { query: GithubQuery::Mine }));
        assert_eq!(clean_live_source(&json!({"kind":"github","query":"other"})), None);
        assert_eq!(clean_live_source(&json!(null)), None);
        assert_eq!(clean_live_source(&json!("x")), None);
        let mut seen: Vec<Value> = (0..250).map(|i| json!(format!("https://e.com/{i}"))).collect();
        seen.push(json!(4));
        let out = clean_live_folder(&json!({"source":{"kind":"github","query":"mine"},"refreshing":true,"refreshedAt":9,"error":"x".repeat(500),"seen":seen})).unwrap();
        assert_eq!(out.refreshed_at, Some(9));
        assert_eq!(out.error.unwrap().len(), 300);
        assert_eq!(out.seen.as_ref().unwrap().len(), 200);
        assert_eq!(out.seen.unwrap()[199], "https://e.com/249");
        assert_eq!(clean_live_folder(&json!({"source":{"kind":"nope"}})), None);
        assert_eq!(clean_live_folder(&json!(null)), None);
    }
}

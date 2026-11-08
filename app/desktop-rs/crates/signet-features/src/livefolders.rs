//! Live folders (port of the data side of `main/features/livefolders/livefolders.ts`): fetching a source (feed or
//! GitHub pull requests), choosing the items to keep, unseen tracking, GitHub token. The folder tree itself
//! (tabs per item) is managed by the app with signet-core.
use std::collections::HashSet;
use std::time::Duration;

use serde_json::Value;

use crate::feeds::{clean_live_source, discover_feed, looks_like_html, parse_feed, FeedEntry, GithubQuery, LiveSource};
use crate::http::Http;
use crate::secrets::SecretStore;
use crate::util::Translate;

pub const MAX_ITEMS: usize = 15;
pub const REFRESH_EVERY: Duration = Duration::from_secs(15 * 60);
pub const FIRST_DELAY: Duration = Duration::from_secs(8);
const TIMEOUT: Duration = Duration::from_secs(15);
const MAX_BYTES: usize = 5 * 1024 * 1024;
pub const TOKEN_SERVICE: &str = "Signet GPUI Safe Storage";
pub const TOKEN_ACCOUNT: &str = "github-token";

/// Same set as JS `encodeURIComponent`.
pub const URI_COMPONENT: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
    .remove(b'-').remove(b'_').remove(b'.').remove(b'!').remove(b'~').remove(b'*').remove(b'\'').remove(b'(').remove(b')');

pub fn github_query_string(q: GithubQuery) -> &'static str {
    match q {
        GithubQuery::ReviewRequested => "is:pr is:open archived:false review-requested:@me",
        GithubQuery::Mine => "is:pr is:open archived:false author:@me",
    }
}

/// Result of reading a source.
pub struct Fetched {
    pub title: String,
    pub entries: Vec<FeedEntry>,
}

pub struct LiveFolders<'a> {
    pub http: &'a dyn Http,
    pub secrets: &'a dyn SecretStore,
    pub t: Translate<'a>,
}

impl<'a> LiveFolders<'a> {
    /// Source typed by the user: a feed address gets a scheme if missing; `None` if invalid.
    pub fn normalize(&self, input: &Value) -> Option<LiveSource> {
        if input.get("kind").and_then(|k| k.as_str()) == Some("feed")
            && let Some(u) = input.get("url").and_then(|u| u.as_str()) {
                let u = u.trim();
                let full = if u.contains("://") { u.to_string() } else { format!("https://{u}") };
                return clean_live_source(&serde_json::json!({"kind": "feed", "url": full}));
            }
        clean_live_source(input)
    }

    pub fn host_of(url: &str) -> String {
        url::Url::parse(url).ok().and_then(|u| u.host_str().map(|h| h.trim_start_matches("www.").to_string())).unwrap_or_else(|| url.to_string())
    }

    /// Default folder name for a new live folder.
    pub fn folder_name(&self, source: &LiveSource, feed_title: &str) -> String {
        match source {
            LiveSource::Github { query } => (self.t)(if *query == GithubQuery::Mine { "liveFolders.github.mine" } else { "liveFolders.github.reviewRequested" }, &[]),
            LiveSource::Feed { url } => if feed_title.is_empty() { Self::host_of(url) } else { feed_title.to_string() },
        }
    }

    /// Reads a source; the error is a message ready for the UI.
    pub fn fetch_source(&self, source: &LiveSource) -> Result<Fetched, String> {
        match source {
            LiveSource::Github { query } => Ok(Fetched { title: String::new(), entries: self.fetch_github(*query)? }),
            LiveSource::Feed { url } => {
                let mut url = url.clone();
                let mut body = self.fetch_text(&url)?;
                if looks_like_html(&body) {
                    url = discover_feed(&body, &url).ok_or_else(|| (self.t)("liveFolders.error.notFeed", &[]))?;
                    body = self.fetch_text(&url)?;
                }
                let feed = parse_feed(&body, &url).ok_or_else(|| (self.t)("liveFolders.error.notFeed", &[]))?;
                Ok(Fetched { title: feed.title, entries: feed.entries })
            }
        }
    }

    fn fetch_text(&self, url: &str) -> Result<String, String> {
        let r = self
            .http
            .get(url, &[("accept", "application/rss+xml, application/atom+xml, application/xml;q=0.9, text/html;q=0.8, */*;q=0.5")], TIMEOUT, MAX_BYTES)
            .map_err(|_| (self.t)("liveFolders.error.network", &[]))?;
        if !r.ok() {
            return Err((self.t)("liveFolders.error.http", &[("status", r.status.to_string())]));
        }
        Ok(r.text())
    }

    fn github_headers(token: &str) -> [(&'static str, String); 3] {
        [("authorization", format!("Bearer {token}")), ("accept", "application/vnd.github+json".into()), ("x-github-api-version", "2022-11-28".into())]
    }

    pub fn token(&self) -> Option<String> {
        self.secrets.get(TOKEN_SERVICE, TOKEN_ACCOUNT).ok().flatten().and_then(|b| String::from_utf8(b).ok())
    }

    pub fn has_token(&self) -> bool {
        self.token().is_some()
    }

    fn fetch_github(&self, query: GithubQuery) -> Result<Vec<FeedEntry>, String> {
        let token = self.token().ok_or_else(|| (self.t)("liveFolders.error.noToken", &[]))?;
        let q = percent_encoding::utf8_percent_encode(github_query_string(query), URI_COMPONENT);
        let url = format!("https://api.github.com/search/issues?q={q}&sort=updated&order=desc&per_page={MAX_ITEMS}");
        let h = Self::github_headers(&token);
        let hs: Vec<(&str, &str)> = h.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let r = self.http.get(&url, &hs, TIMEOUT, MAX_BYTES).map_err(|_| (self.t)("liveFolders.error.network", &[]))?;
        if r.status == 401 {
            return Err((self.t)("liveFolders.error.tokenInvalid", &[]));
        }
        if !r.ok() {
            return Err((self.t)("liveFolders.error.http", &[("status", r.status.to_string())]));
        }
        let data: Value = serde_json::from_slice(&r.body).map_err(|_| (self.t)("liveFolders.error.network", &[]))?;
        Ok(parse_github_items(&data))
    }

    /// Checks the token with GitHub then stores it (`None` removes it); error message or `None`.
    pub fn set_token(&self, input: Option<&str>) -> Option<String> {
        let Some(input) = input else {
            let _ = self.secrets.delete(TOKEN_SERVICE, TOKEN_ACCOUNT);
            return None;
        };
        let token = input.trim();
        let ok_format = (20..=255).contains(&token.chars().count()) && token.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
        if !ok_format {
            return Some((self.t)("liveFolders.error.tokenFormat", &[]));
        }
        let h = Self::github_headers(token);
        let hs: Vec<(&str, &str)> = h.iter().map(|(k, v)| (*k, v.as_str())).collect();
        match self.http.get("https://api.github.com/user", &hs, TIMEOUT, MAX_BYTES) {
            Ok(r) if r.status == 401 || r.status == 403 => return Some((self.t)("liveFolders.error.tokenInvalid", &[])),
            Ok(r) if !r.ok() => return Some((self.t)("liveFolders.error.network", &[])),
            Ok(_) => {}
            Err(_) => return Some((self.t)("liveFolders.error.network", &[])),
        }
        if self.secrets.set(TOKEN_SERVICE, TOKEN_ACCOUNT, token.as_bytes()).is_err() {
            return Some((self.t)("liveFolders.error.encryption", &[]));
        }
        None
    }
}

/// Entries of a GitHub search response (only github.com addresses).
pub fn parse_github_items(data: &Value) -> Vec<FeedEntry> {
    let Some(items) = data.get("items").and_then(|i| i.as_array()) else { return vec![] };
    items
        .iter()
        .filter_map(|it| {
            let url = it.get("html_url")?.as_str().filter(|u| u.starts_with("https://github.com/"))?;
            let repo_url = it.get("repository_url").and_then(|v| v.as_str()).unwrap_or("");
            let parts: Vec<&str> = repo_url.split('/').collect();
            let repo = if parts.len() >= 2 { parts[parts.len() - 2..].join("/") } else { parts.join("/") };
            let number = it.get("number").and_then(|n| n.as_i64()).map(|n| n.to_string()).unwrap_or_default();
            let base = it.get("title").and_then(|t| t.as_str()).unwrap_or("");
            let title = if repo.is_empty() { base.to_string() } else { format!("{base} \u{b7} {repo}#{number}") };
            let title = title.trim().to_string();
            let date = it.get("updated_at").and_then(|d| d.as_str()).and_then(|d| chrono::DateTime::parse_from_rfc3339(d).ok()).map(|d| d.timestamp_millis()).unwrap_or(0);
            Some(FeedEntry { url: url.into(), title: if title.is_empty() { url.into() } else { title }, date })
        })
        .collect()
}

/// Items to keep: newest first when the source dates at least half of them, deduplicated, at most `MAX_ITEMS`.
pub fn select_entries(all: &[FeedEntry]) -> Vec<FeedEntry> {
    let dated = all.iter().filter(|e| e.date > 0).count() as f64 >= all.len() as f64 / 2.0;
    let mut sorted: Vec<FeedEntry> = all.to_vec();
    if dated {
        sorted.sort_by_key(|a| std::cmp::Reverse(a.date));
    }
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for e in sorted {
        if !seen.insert(e.url.clone()) {
            continue;
        }
        out.push(e);
        if out.len() >= MAX_ITEMS {
            break;
        }
    }
    out
}

/// New `seen` list after an update: on creation everything is seen; otherwise only addresses still in the source.
pub fn next_seen(previous: &[String], current_urls: &[String], first: bool) -> Vec<String> {
    if first {
        return current_urls.to_vec();
    }
    let cur: HashSet<&String> = current_urls.iter().collect();
    previous.iter().filter(|u| cur.contains(u)).cloned().collect()
}

/// Marks an address as opened (keeps the last 200).
pub fn mark_seen(seen: &mut Vec<String>, url: &str) {
    if !seen.iter().any(|u| u == url) {
        seen.push(url.to_string());
    }
    let n = seen.len();
    if n > 200 {
        seen.drain(..n - 200);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{resp, FakeHttp};
    use crate::secrets::MemoryStore;
    use serde_json::json;

    fn tr(k: &str, _: &[(&str, String)]) -> String { k.to_string() }
    fn e(url: &str, date: i64) -> FeedEntry { FeedEntry { url: url.into(), title: url.into(), date } }

    #[test]
    fn select_sorts_dedups_and_limits() {
        let mut all: Vec<FeedEntry> = (1..=20).map(|i| e(&format!("https://e.com/{i}"), i)).collect();
        all.push(e("https://e.com/5", 99));
        let out = select_entries(&all);
        assert_eq!(out.len(), MAX_ITEMS);
        assert_eq!(out[0].url, "https://e.com/5");
        let undated = vec![e("b", 0), e("a", 0), e("b", 0)];
        assert_eq!(select_entries(&undated).iter().map(|x| x.url.as_str()).collect::<Vec<_>>(), vec!["b", "a"]);
    }

    #[test]
    fn seen_rules() {
        let cur = vec!["a".to_string(), "b".to_string()];
        assert_eq!(next_seen(&["x".into(), "a".into()], &cur, false), vec!["a".to_string()]);
        assert_eq!(next_seen(&[], &cur, true), cur);
        let mut s: Vec<String> = (0..200).map(|i| i.to_string()).collect();
        mark_seen(&mut s, "new");
        assert_eq!(s.len(), 200);
        assert_eq!(s[199], "new");
        mark_seen(&mut s, "new");
        assert_eq!(s.len(), 200);
    }

    #[test]
    fn follows_announced_feed() {
        let http = FakeHttp(Box::new(|u| match u {
            "https://e.com/" => resp(200, r#"<!doctype html><link rel="alternate" type="application/rss+xml" href="/rss">"#),
            "https://e.com/rss" => resp(200, "<rss><channel><title>T</title><item><link>https://e.com/1</link></item></channel></rss>"),
            _ => resp(404, ""),
        }));
        let s = MemoryStore::default();
        let lf = LiveFolders { http: &http, secrets: &s, t: &tr };
        let f = lf.fetch_source(&LiveSource::Feed { url: "https://e.com/".into() }).unwrap();
        assert_eq!(f.title, "T");
        assert_eq!(f.entries.len(), 1);
        assert_eq!(lf.fetch_source(&LiveSource::Feed { url: "https://e.com/missing".into() }).err().unwrap(), "liveFolders.error.http");
    }

    #[test]
    fn html_without_feed_is_an_error() {
        let http = FakeHttp(Box::new(|_| resp(200, "<html></html>")));
        let s = MemoryStore::default();
        let lf = LiveFolders { http: &http, secrets: &s, t: &tr };
        assert_eq!(lf.fetch_source(&LiveSource::Feed { url: "https://e.com/".into() }).err().unwrap(), "liveFolders.error.notFeed");
    }

    #[test]
    fn github_needs_token_and_parses() {
        let http = FakeHttp(Box::new(|u| {
            assert!(u.contains("review-requested"));
            resp(200, r#"{"items":[{"html_url":"https://github.com/o/r/pull/3","title":"Fix","number":3,"repository_url":"https://api.github.com/repos/o/r","updated_at":"2024-01-01T00:00:00Z"},{"html_url":"https://evil.com/x","title":"No"}]}"#)
        }));
        let s = MemoryStore::default();
        let lf = LiveFolders { http: &http, secrets: &s, t: &tr };
        let src = LiveSource::Github { query: GithubQuery::ReviewRequested };
        assert_eq!(lf.fetch_source(&src).err().unwrap(), "liveFolders.error.noToken");
        s.set(TOKEN_SERVICE, TOKEN_ACCOUNT, b"ghp_xxxxxxxxxxxxxxxxxxxxxxxx").unwrap();
        let f = lf.fetch_source(&src).unwrap();
        assert_eq!(f.entries.len(), 1);
        assert_eq!(f.entries[0].title, "Fix \u{b7} o/r#3");
    }

    #[test]
    fn token_validation() {
        let http = FakeHttp(Box::new(|_| resp(401, "")));
        let s = MemoryStore::default();
        let lf = LiveFolders { http: &http, secrets: &s, t: &tr };
        assert_eq!(lf.set_token(Some("short")).as_deref(), Some("liveFolders.error.tokenFormat"));
        assert_eq!(lf.set_token(Some("ghp_aaaaaaaaaaaaaaaaaaaaaaaa")).as_deref(), Some("liveFolders.error.tokenInvalid"));
        assert!(!lf.has_token());
        let ok = FakeHttp(Box::new(|_| resp(200, "{}")));
        let lf = LiveFolders { http: &ok, secrets: &s, t: &tr };
        assert_eq!(lf.set_token(Some("ghp_aaaaaaaaaaaaaaaaaaaaaaaa")), None);
        assert!(lf.has_token());
        lf.set_token(None);
        assert!(!lf.has_token());
    }

    #[test]
    fn normalize_adds_scheme() {
        let http = FakeHttp(Box::new(|_| resp(200, "")));
        let s = MemoryStore::default();
        let lf = LiveFolders { http: &http, secrets: &s, t: &tr };
        assert_eq!(lf.normalize(&json!({"kind":"feed","url":"example.com/rss"})), Some(LiveSource::Feed { url: "https://example.com/rss".into() }));
        assert_eq!(lf.normalize(&json!({"kind":"feed","url":""})), None);
        assert_eq!(LiveFolders::host_of("https://www.example.com/x"), "example.com");
    }
}

//! Ports `app/desktop/src/main/core/rank.ts`: relevance of a page for what is typed in the command bar.
//! Scores go from 0 (no match) to about 100; `fq` and the fields are already folded (see `urls::fold`).

use regex::Regex;
use std::sync::OnceLock;

const DAY: f64 = 86_400_000.0;

/// Byte index of the first occurrence of `fq` in `s` that starts a word (not in the middle of one).
fn word_start(s: &str, fq: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(rel) = s[from..].find(fq) {
        let i = from + rel;
        let prev = s[..i].chars().next_back();
        if prev.is_none_or(|c| !c.is_alphanumeric()) {
            return Some(i);
        }
        from = i + s[i..].chars().next().map_or(1, char::len_utf8);
    }
    None
}

/// How well the input matches a page, whatever its popularity.
pub fn match_quality(fq: &str, title: &str, url: &str) -> f64 {
    static SCHEME: OnceLock<Regex> = OnceLock::new();
    static PORT: OnceLock<Regex> = OnceLock::new();
    if fq.is_empty() {
        return 0.0;
    }
    let scheme = SCHEME.get_or_init(|| Regex::new(r"^[a-z][a-z0-9+.-]*://").expect("static regex"));
    let port = PORT.get_or_init(|| Regex::new(r":\d+$").expect("static regex"));
    let no_scheme = scheme.replace(url, "");
    let bare = no_scheme.strip_prefix("www.").unwrap_or(&no_scheme);
    if bare.starts_with(fq) {
        return 100.0;
    }
    let host_part = bare.split(['/', '?', '#']).next().unwrap_or("");
    let host = port.replace(host_part, "");
    if host.split('.').any(|label| label.starts_with(fq)) {
        return 80.0;
    }
    match word_start(title, fq) {
        Some(0) => return 60.0,
        Some(_) => return 45.0,
        None => {}
    }
    if host.contains(fq) {
        return 40.0;
    }
    if word_start(bare, fq).is_some() {
        return 30.0;
    }
    if title.contains(fq) || bare.contains(fq) { 15.0 } else { 0.0 }
}

/// Visits needed to inline-complete a page that was never typed.
pub const INLINE_MIN_VISITS: u32 = 3;

/// Whether the input may be completed to this page, like Chrome: typed at least once, or visited often.
pub fn completable(visits_typed: Option<(u32, Option<u32>)>) -> bool {
    match visits_typed {
        Some((visits, typed)) => typed.unwrap_or(0) >= 1 || visits >= INLINE_MIN_VISITS,
        None => false,
    }
}

/// Popularity between 0 and 1: visit frequency (weight 0.4), typed frequency (0.35), recency (0.25, halves every ~10 days).
pub fn frecency(visits: u32, typed: u32, last_visit: i64, now: i64) -> f64 {
    let freq = (f64::from(1 + visits).log2() / 51f64.log2()).min(1.0);
    let typed_freq = (f64::from(1 + typed).log2() / 11f64.log2()).min(1.0);
    let age = (now - last_visit).max(0) as f64;
    let recency = 0.5f64.powf(age / DAY / 10.0);
    0.4 * freq + 0.35 * typed_freq + 0.25 * recency
}

/// Fields of a history entry the score depends on (folded title and url).
pub struct ScoreInput<'a> {
    pub title: &'a str,
    pub url: &'a str,
    pub visits: u32,
    pub typed: u32,
    pub last_visit: i64,
}

/// Score of a history entry: the match decides, popularity moves it within 35% to 100% of that.
pub fn history_score(fq: &str, e: &ScoreInput, now: i64) -> f64 {
    let q = match_quality(fq, e.title, e.url);
    if q == 0.0 { 0.0 } else { q * (0.35 + 0.65 * frecency(e.visits, e.typed, e.last_visit, now)) }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000_000;

    #[test]
    fn frecency_bounds() {
        assert!(frecency(0, 0, 0, NOW) < 0.01);
        assert!((frecency(1000, 1000, NOW, NOW) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn typed_weighs_more() {
        assert!(frecency(5, 5, NOW, NOW) > frecency(40, 0, NOW, NOW));
    }

    #[test]
    fn completable_rules() {
        assert!(completable(Some((1, Some(1)))));
        assert!(completable(Some((3, None))));
        assert!(!completable(Some((2, None))));
        assert!(!completable(Some((2, Some(0)))));
        assert!(!completable(None));
    }

    #[test]
    fn match_quality_levels() {
        assert_eq!(match_quality("exa", "t", "https://www.example.com/"), 100.0);
        assert_eq!(match_quality("m/", "t", "https://example.com/"), 15.0);
        assert_eq!(match_quality("yout", "x", "https://www.youtube.com/"), 100.0);
        assert_eq!(match_quality("tube", "x", "https://www.youtube.com/"), 40.0);
        assert_eq!(match_quality("doc", "doc page", "https://a.com/1"), 60.0);
        assert_eq!(match_quality("page", "doc page", "https://a.com/1"), 45.0);
        assert_eq!(match_quality("", "a", "b"), 0.0);
    }
}

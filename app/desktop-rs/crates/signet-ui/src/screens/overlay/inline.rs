//! Inline completion and match highlighting of the command bar (ports of `shared/inline.ts` and `shared/highlight.ts`).

use signet_core::types::PaletteResult;
use signet_core::urls::fold;

/// Address without scheme, `www.` and trailing slash: what a user types to reach a site.
pub fn bare_url(url: &str) -> String {
    let mut s = url;
    if let Some(i) = s.find("://") {
        let scheme = &s[..i];
        let valid = scheme.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
            && scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'));
        if valid {
            s = &s[i + 3..];
        }
    }
    if s.len() >= 4 && s[..4].eq_ignore_ascii_case("www.") {
        s = &s[4..];
    }
    s.strip_suffix('/').unwrap_or(s).to_string()
}

/// The part of a result needed to complete the input.
pub struct Completable<'a> {
    pub kind_ok: bool,
    pub url: Option<&'a str>,
    pub inline: bool,
}

/// Maps a result to what completion needs: only page rows (tab, history, archive) can complete.
pub fn completable(r: &PaletteResult) -> Completable<'_> {
    match r {
        PaletteResult::Tab { url, inline, .. } => Completable { kind_ok: true, url: url.as_deref(), inline: inline.unwrap_or(false) },
        PaletteResult::History { url, inline, .. } | PaletteResult::Archived { url, inline, .. } => {
            Completable { kind_ok: true, url: Some(url.as_str()), inline: inline.unwrap_or(false) }
        }
        _ => Completable { kind_ok: false, url: None, inline: false },
    }
}

/// Text to append (and show selected) after what is typed, or "". Only a page row flagged `inline` on a web address
/// completes, and only when its bare address starts with the input, ignoring case. Without a `/` typed, only the host.
pub fn inline_completion(typed: &str, top: Option<&PaletteResult>) -> String {
    let Some(top) = top else { return String::new() };
    let c = completable(top);
    let Some(url) = c.url else { return String::new() };
    if typed.is_empty() || typed.contains(char::is_whitespace) || !c.inline || !c.kind_ok {
        return String::new();
    }
    let lower = url.to_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://")) {
        return String::new();
    }
    let mut bare = bare_url(url);
    if !typed.contains('/') {
        let cut = bare.find(['/', '?', '#']).unwrap_or(bare.len());
        bare.truncate(cut);
    }
    let n = typed.chars().count();
    if bare.chars().count() <= n {
        return String::new();
    }
    let head: String = bare.chars().take(n).collect();
    if head.to_lowercase() != typed.to_lowercase() {
        return String::new();
    }
    bare.chars().skip(n).collect()
}

/// Whether an edit may be completed: the text only grew at its end (a character typed), no composition, not a paste.
/// Deleting must never re-complete, or the completion could not be removed.
pub fn can_complete(previous: &str, next: &str, composing: bool) -> bool {
    !composing && next.len() > previous.len() && next.starts_with(previous)
}

/// Byte range of `text` to show in bold for the typed `query`, or None. Same rules as the ranking: case and accent
/// insensitive, the occurrence at the start of the text wins, then a word start, then the first one.
pub fn highlight_range(text: &str, query: &str) -> Option<(usize, usize)> {
    let fq: Vec<char> = fold(query.trim()).chars().collect();
    if fq.is_empty() {
        return None;
    }
    // Folded characters with the original [start, end) byte range of the character they come from.
    let mut folded: Vec<char> = Vec::new();
    let mut from: Vec<usize> = Vec::new();
    let mut to: Vec<usize> = Vec::new();
    for (i, ch) in text.char_indices() {
        let end = i + ch.len_utf8();
        for f in fold(&ch.to_string()).chars() {
            folded.push(f);
            from.push(i);
            to.push(end);
        }
    }
    let mut at: Option<usize> = None;
    let mut i = 0;
    while i + fq.len() <= folded.len() {
        if folded[i..i + fq.len()] == fq[..] {
            if at.is_none() {
                at = Some(i);
            }
            if i == 0 || !folded[i - 1].is_alphanumeric() {
                at = Some(i);
                break;
            }
        }
        i += 1;
    }
    let at = at?;
    let mut end = to[at + fq.len() - 1];
    // Keep the accent attached to its letter.
    while let Some(c) = text[end..].chars().next() {
        if matches!(c as u32, 0x0300..=0x036f | 0x1ab0..=0x1aff | 0x1dc0..=0x1dff | 0x20d0..=0x20ff | 0xfe20..=0xfe2f) {
            end += c.len_utf8();
        } else {
            break;
        }
    }
    Some((from[at], end))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hist(url: &str, inline: bool) -> PaletteResult {
        PaletteResult::History {
            id: "h".into(),
            title: "t".into(),
            subtitle: None,
            url: url.into(),
            favicon: None,
            inline: Some(inline),
        }
    }

    #[test]
    fn bare_urls() {
        assert_eq!(bare_url("https://www.example.com/"), "example.com");
        assert_eq!(bare_url("http://a.b/c?d"), "a.b/c?d");
    }

    #[test]
    fn completes_the_host_only_without_a_slash() {
        let r = hist("https://www.example.com/path/page", true);
        assert_eq!(inline_completion("exa", Some(&r)), "mple.com");
        assert_eq!(inline_completion("example.com/p", Some(&r)), "ath/page");
        assert_eq!(inline_completion("EXA", Some(&r)), "mple.com");
    }

    #[test]
    fn no_completion_when_not_allowed() {
        let r = hist("https://example.com", false);
        assert_eq!(inline_completion("exa", Some(&r)), "");
        let r = hist("https://example.com", true);
        assert_eq!(inline_completion("", Some(&r)), "");
        assert_eq!(inline_completion("a b", Some(&r)), "");
        assert_eq!(inline_completion("zzz", Some(&r)), "");
        assert_eq!(inline_completion("example.com", Some(&r)), "");
        assert_eq!(inline_completion("exa", Some(&hist("ftp://example.com", true))), "");
    }

    #[test]
    fn completion_only_after_typing() {
        assert!(can_complete("ab", "abc", false));
        assert!(!can_complete("abc", "ab", false));
        assert!(!can_complete("ab", "abc", true));
        assert!(!can_complete("ab", "xyz", false));
    }

    #[test]
    fn highlight_prefers_start_then_word_start() {
        assert_eq!(highlight_range("Hello World", "hel"), Some((0, 3)));
        assert_eq!(highlight_range("say hello", "hel"), Some((4, 7)));
        assert_eq!(highlight_range("ahello hello", "hel"), Some((7, 10)));
        assert_eq!(highlight_range("ahello", "hel"), Some((1, 4)));
        assert_eq!(highlight_range("Café", "cafe"), Some((0, 5)));
        assert_eq!(highlight_range("abc", "z"), None);
        assert_eq!(highlight_range("abc", "  "), None);
    }
}

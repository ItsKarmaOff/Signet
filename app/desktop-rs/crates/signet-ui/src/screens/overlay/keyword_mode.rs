//! Keyword mode of the command bar (port of `overlay/keywordMode.ts`): "yt " (or "yt" + Tab) turns the keyword into a
//! chip, the rest is the query.

use signet_core::searchengines::KeywordInfo;

fn find<'a>(keywords: &'a [KeywordInfo], token: &str) -> Option<&'a KeywordInfo> {
    let t = token.to_lowercase();
    keywords.iter().find(|k| k.keyword == t)
}

/// Typed text starting with a keyword followed by whitespace (`yt cats`, `yt `): the keyword and the text after it.
pub fn keyword_prefix<'a>(value: &str, keywords: &'a [KeywordInfo]) -> Option<(&'a KeywordInfo, String)> {
    let trimmed = value.trim_start();
    let end = trimmed.find(char::is_whitespace)?;
    let (token, rest) = trimmed.split_at(end);
    let info = find(keywords, token)?;
    // `rest` starts with exactly the whitespace char that ended the token; the text after it loses its leading blanks.
    let mut chars = rest.chars();
    chars.next();
    Some((info, chars.as_str().trim_start().to_string()))
}

/// Typed text that is exactly a keyword (Tab enters the mode).
pub fn keyword_only<'a>(value: &str, keywords: &'a [KeywordInfo]) -> Option<&'a KeywordInfo> {
    let t = value.trim();
    if t.is_empty() || t.contains(char::is_whitespace) { None } else { find(keywords, t) }
}

/// Text sent to the app for a query typed in keyword mode.
pub fn keyword_query(info: &KeywordInfo, query: &str) -> String {
    format!("{} {}", info.keyword, query)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kws() -> Vec<KeywordInfo> {
        vec![
            KeywordInfo { id: "youtube".into(), keyword: "yt".into(), name: "YouTube".into(), favicon: None },
            KeywordInfo { id: "wikipedia".into(), keyword: "w".into(), name: "Wikipedia".into(), favicon: None },
        ]
    }

    #[test]
    fn keyword_followed_by_a_space_enters_the_mode() {
        let k = kws();
        let (info, rest) = keyword_prefix("yt ", &k).unwrap();
        assert_eq!((info.id.as_str(), rest.as_str()), ("youtube", ""));
        assert_eq!(keyword_prefix("yt cats", &k).unwrap().1, "cats");
        assert_eq!(keyword_prefix("YT  cats", &k).unwrap().1, "cats");
        assert_eq!(keyword_prefix("  w\tfoo", &k).unwrap().0.id, "wikipedia");
    }

    #[test]
    fn no_mode_without_whitespace_or_for_another_word() {
        let k = kws();
        assert!(keyword_prefix("yt", &k).is_none());
        assert!(keyword_prefix("w.org", &k).is_none());
        assert!(keyword_prefix("w.org x", &k).is_none());
        assert!(keyword_prefix("ytx cats", &k).is_none());
    }

    #[test]
    fn tab_only_on_a_lone_keyword() {
        let k = kws();
        assert_eq!(keyword_only("yt", &k).unwrap().id, "youtube");
        assert_eq!(keyword_only(" W ", &k).unwrap().id, "wikipedia");
        assert!(keyword_only("yt cats", &k).is_none());
        assert!(keyword_only("w.org", &k).is_none());
        assert!(keyword_only("", &k).is_none());
    }

    #[test]
    fn query_sent_to_main() {
        let k = kws();
        assert_eq!(keyword_query(&k[0], "cats"), "yt cats");
        assert_eq!(keyword_query(&k[0], ""), "yt ");
    }
}

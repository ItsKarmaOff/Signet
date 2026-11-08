//! Global Privacy Control (port of `main/web/gpc.ts`): policy only. The CEF layer adds the header and injects the script.

// The page script defining `navigator.globalPrivacyControl` is owned by `signet-scripts` (`signet-gpc`).
pub const HEADER_NAME: &str = "Sec-GPC";
pub const HEADER_VALUE: &str = "1";

/// Returns the header to add for a request URL, or `None` (only http and https carry it).
pub fn header_for(enabled: bool, url: &str) -> Option<(&'static str, &'static str)> {
    if !enabled {
        return None;
    }
    let l = url.trim_start().to_ascii_lowercase();
    (l.starts_with("http://") || l.starts_with("https://")).then_some((HEADER_NAME, HEADER_VALUE))
}

/// Adds `Sec-GPC: 1` to a header list (replacing any existing one), keeping the others.
pub fn apply_header(headers: &mut Vec<(String, String)>) {
    headers.retain(|(k, _)| !k.eq_ignore_ascii_case(HEADER_NAME));
    headers.push((HEADER_NAME.into(), HEADER_VALUE.into()));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn header_only_for_web_when_on() {
        assert_eq!(header_for(true, "https://example.com/a"), Some(("Sec-GPC", "1")));
        assert_eq!(header_for(true, "http://example.com/"), Some(("Sec-GPC", "1")));
        assert_eq!(header_for(true, "ftp://example.com/"), None);
        assert_eq!(header_for(true, "file:///tmp/a"), None);
        assert_eq!(header_for(false, "https://example.com/"), None);
    }
    #[test]
    fn keeps_other_headers() {
        let mut h = vec![("Accept".to_string(), "*/*".to_string()), ("sec-gpc".into(), "0".into())];
        apply_header(&mut h);
        assert_eq!(h, vec![("Accept".to_string(), "*/*".to_string()), ("Sec-GPC".into(), "1".into())]);
    }
}

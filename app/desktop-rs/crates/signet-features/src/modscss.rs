//! Neutralizes remote loads in a mod sheet (port of `main/features/mods/modscss.ts`): without this,
//! `input[value^=a]{background:url(https://...)}` leaks UI data. Only `data:` and local references (`#id`) remain.
//! CSS escapes, comments and case are taken into account.

const EMPTY: &str = "data:,";
const IMAGE_FNS: &[&str] = &["image-set", "-webkit-image-set", "cross-fade", "-webkit-cross-fade", "image", "src"];

fn is_hex(c: char) -> bool {
    c.is_ascii_hexdigit()
}
fn is_ws(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{c}')
}
fn is_ident(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-' || c >= '\u{80}'
}

/// Reads an escape starting at `\` (position i); returns the decoded character and the next position.
fn escape_at(s: &[char], i: usize) -> (char, usize) {
    let mut j = i + 1;
    if j >= s.len() {
        return ('\u{fffd}', j);
    }
    if is_hex(s[j]) {
        let mut hex = String::new();
        while j < s.len() && hex.len() < 6 && is_hex(s[j]) {
            hex.push(s[j]);
            j += 1;
        }
        if j < s.len() && is_ws(s[j]) {
            j += if s[j] == '\r' && s.get(j + 1) == Some(&'\n') { 2 } else { 1 };
        }
        let cp = u32::from_str_radix(&hex, 16).unwrap_or(0);
        let ch = if cp == 0 || cp > 0x10ffff || (0xd800..=0xdfff).contains(&cp) { '\u{fffd}' } else { char::from_u32(cp).unwrap_or('\u{fffd}') };
        return (ch, j);
    }
    (s[j], j + 1)
}

/// Reads a string (quote at i); returns its decoded value and the position after the closing quote.
fn read_string(s: &[char], i: usize) -> (String, usize) {
    let q = s[i];
    let mut j = i + 1;
    let mut value = String::new();
    while j < s.len() {
        let c = s[j];
        if c == q {
            return (value, j + 1);
        }
        if matches!(c, '\n' | '\r' | '\u{c}') {
            return (value, j);
        }
        if c == '\\' {
            match s.get(j + 1) {
                Some('\n') | Some('\u{c}') => j += 2,
                Some('\r') => j += if s.get(j + 2) == Some(&'\n') { 3 } else { 2 },
                _ => {
                    let (ch, next) = escape_at(s, j);
                    value.push(ch);
                    j = next;
                }
            }
            continue;
        }
        value.push(c);
        j += 1;
    }
    (value, j)
}

fn allowed(value: &str) -> bool {
    // Control characters and leading spaces are ignored by the browser before reading the scheme.
    let v: String = value.trim_start_matches(|c: char| c <= ' ').chars().filter(|c| !matches!(c, '\t' | '\n' | '\r')).collect();
    v.get(..5).is_some_and(|p| p.eq_ignore_ascii_case("data:")) || v.starts_with('#')
}

fn slice(s: &[char], a: usize, b: usize) -> String {
    s[a.min(s.len())..b.min(s.len())].iter().collect()
}

pub fn sanitize_mod_css(css: &str) -> String {
    let s: Vec<char> = css.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let mut stack: Vec<bool> = Vec::new();
    let mut import_pending = false;
    while i < s.len() {
        let c = s[i];
        if c == '/' && s.get(i + 1) == Some(&'*') {
            let end = (i + 2..s.len().saturating_sub(1)).find(|k| s[*k] == '*' && s[*k + 1] == '/');
            i = end.map(|e| e + 2).unwrap_or(s.len());
            out.push(' ');
        } else if c == '"' || c == '\'' {
            let (value, next) = read_string(&s, i);
            let watched = import_pending || stack.last() == Some(&true);
            import_pending = false;
            if watched && !allowed(&value) {
                out.push_str(&format!("\"{EMPTY}\""));
            } else {
                out.push_str(&slice(&s, i, next));
            }
            i = next;
        } else if c == '(' {
            stack.push(false);
            out.push(c);
            i += 1;
        } else if c == ')' {
            stack.pop();
            out.push(c);
            i += 1;
        } else if c == ';' || c == '{' {
            import_pending = false;
            out.push(c);
            i += 1;
        } else if c == '\\' || is_ident(c) || (c == '@' && s.get(i + 1).is_some_and(|n| *n == '\\' || is_ident(*n))) {
            let start = i;
            let at = c == '@';
            if at {
                i += 1;
            }
            let mut name = String::new();
            while i < s.len() {
                if s[i] == '\\' {
                    let (ch, next) = escape_at(&s, i);
                    name.push(ch);
                    i = next;
                } else if is_ident(s[i]) {
                    name.push(s[i]);
                    i += 1;
                } else {
                    break;
                }
            }
            let lower = name.to_lowercase();
            if at {
                if lower == "import" {
                    import_pending = true;
                }
                out.push_str(&slice(&s, start, i));
            } else if s.get(i) == Some(&'(') && lower == "url" {
                let mut j = i + 1;
                while j < s.len() && is_ws(s[j]) {
                    j += 1;
                }
                let mut value = String::new();
                if s.get(j) == Some(&'"') || s.get(j) == Some(&'\'') {
                    let (v, next) = read_string(&s, j);
                    value = v;
                    j = next;
                    while j < s.len() && s[j] != ')' {
                        j += 1;
                    }
                } else {
                    while j < s.len() && s[j] != ')' {
                        if s[j] == '\\' {
                            let (ch, next) = escape_at(&s, j);
                            value.push(ch);
                            j = next;
                        } else {
                            value.push(s[j]);
                            j += 1;
                        }
                    }
                }
                let end = (j + 1).min(s.len());
                if allowed(value.trim()) {
                    out.push_str(&slice(&s, start, end));
                } else {
                    out.push_str(&format!("url(\"{EMPTY}\")"));
                }
                i = end;
            } else {
                out.push_str(&slice(&s, start, i));
                if s.get(i) == Some(&'(') {
                    stack.push(IMAGE_FNS.contains(&lower.as_str()));
                    out.push('(');
                    i += 1;
                }
            }
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    const E: &str = "data:,";

    #[test]
    fn plain_css_passes() {
        let css = ".a { color: red; background: linear-gradient(#fff, #000) }";
        assert_eq!(sanitize_mod_css(css), css);
    }

    #[test]
    fn remote_url_is_neutralized() {
        let want = format!("a{{background:url(\"{E}\")}}");
        assert_eq!(sanitize_mod_css("a{background:url(https://evil.com/x)}"), want);
        assert_eq!(sanitize_mod_css("a{background:url(\"https://evil.com/x\")}"), want);
        assert_eq!(sanitize_mod_css("a{background:url( 'https://evil.com' )}"), want);
        assert_eq!(sanitize_mod_css("a{background:url(//evil.com/x)}"), want);
    }

    #[test]
    fn data_and_fragments_kept() {
        assert_eq!(sanitize_mod_css("a{background:url(data:image/png;base64,AAAA)}"), "a{background:url(data:image/png;base64,AAAA)}");
        assert_eq!(sanitize_mod_css("a{filter:url(#blur)}"), "a{filter:url(#blur)}");
    }

    #[test]
    fn case_and_escapes() {
        let want = format!("url(\"{E}\")");
        assert!(sanitize_mod_css("a{background:URL(https://evil.com)}").contains(&want));
        assert!(sanitize_mod_css("a{background:\\75rl(https://evil.com)}").contains(&want));
        assert!(sanitize_mod_css("a{background:url(\\68ttps://evil.com)}").contains(&want));
    }

    #[test]
    fn spaces_before_scheme() {
        assert!(sanitize_mod_css("a{background:url(\" \\n data:,x\")}").contains("data:"));
        assert!(sanitize_mod_css("a{background:url(\"  https://evil.com\")}").contains(&format!("url(\"{E}\")")));
    }

    #[test]
    fn import_neutralized() {
        assert_eq!(sanitize_mod_css("@import \"https://evil.com/a.css\";"), format!("@import \"{E}\";"));
        assert_eq!(sanitize_mod_css("@import url(https://evil.com/a.css);"), format!("@import url(\"{E}\");"));
        assert!(sanitize_mod_css("@\\69mport \"https://evil.com\";").contains(E));
    }

    #[test]
    fn image_set_strings() {
        assert_eq!(sanitize_mod_css("a{background:image-set(\"https://evil.com/a.png\" 1x)}"), format!("a{{background:image-set(\"{E}\" 1x)}}"));
        assert!(sanitize_mod_css("a{background:-webkit-image-set(\"https://evil.com/a.png\" 1x)}").contains(E));
    }

    #[test]
    fn ordinary_strings_untouched() {
        let css = "a::after{content:\"https://example.com\"} input[value^=\"https://x\"]{color:red}";
        assert_eq!(sanitize_mod_css(css), css);
    }

    #[test]
    fn comment_does_not_hide_a_load() {
        assert_eq!(sanitize_mod_css("a{background:u/**/rl(https://evil.com)}"), "a{background:u rl(https://evil.com)}");
    }

    #[test]
    fn unclosed_inputs() {
        assert!(!sanitize_mod_css("a{background:url(https://evil.com").contains("evil.com"));
        assert!(!sanitize_mod_css("a{background:url(\"https://evil.com").contains("evil.com"));
    }
}

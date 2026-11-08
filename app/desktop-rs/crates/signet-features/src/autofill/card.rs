//! Card helpers (port of `shared/autofill/card.ts`): Luhn, network, expiry formats.
use chrono::{Datelike, NaiveDate};
use regex::Regex;
use std::sync::OnceLock;

fn rx(cell: &'static OnceLock<Regex>, p: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(p).expect("static regex"))
}

pub use signet_core::types::CardNetwork;

/// Display name of a network (empty for unknown).
pub fn network_name(n: CardNetwork) -> &'static str {
    match n {
        CardNetwork::Visa => "Visa",
        CardNetwork::Mastercard => "Mastercard",
        CardNetwork::Amex => "American Express",
        CardNetwork::Discover => "Discover",
        CardNetwork::Diners => "Diners Club",
        CardNetwork::Jcb => "JCB",
        CardNetwork::Unionpay => "UnionPay",
        CardNetwork::Maestro => "Maestro",
        CardNetwork::Unknown => "",
    }
}

/// Digits of a typed card number (spaces and dashes dropped); `None` if anything else is in it or the length is off.
pub fn card_digits(input: &str) -> Option<String> {
    static R: OnceLock<Regex> = OnceLock::new();
    let d: String = input.chars().filter(|c| !c.is_whitespace() && *c != '-').collect();
    rx(&R, r"^\d{12,19}$").is_match(&d).then_some(d)
}

pub fn luhn(digits: &str) -> bool {
    if !(12..=19).contains(&digits.len()) || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    let mut sum = 0;
    for (i, b) in digits.bytes().rev().enumerate() {
        let mut d = (b - b'0') as u32;
        if i % 2 == 1 {
            d *= 2;
            if d > 9 {
                d -= 9;
            }
        }
        sum += d;
    }
    sum % 10 == 0
}

pub fn detect_network(digits: &str) -> CardNetwork {
    let p = |n: usize| digits.get(..n).and_then(|s| s.parse::<u32>().ok()).unwrap_or(u32::MAX);
    let r = |n: usize, lo: u32, hi: u32| (lo..=hi).contains(&p(n));
    if digits.starts_with("34") || digits.starts_with("37") {
        CardNetwork::Amex
    } else if digits.starts_with('4') {
        CardNetwork::Visa
    } else if r(2, 51, 55) || r(4, 2221, 2720) {
        CardNetwork::Mastercard
    } else if digits.starts_with("6011") || digits.starts_with("65") || r(3, 644, 649) || r(6, 622126, 622925) {
        CardNetwork::Discover
    } else if digits.starts_with("62") {
        CardNetwork::Unionpay
    } else if r(4, 3528, 3589) {
        CardNetwork::Jcb
    } else if r(3, 300, 305) || ["36", "38", "39"].iter().any(|x| digits.starts_with(x)) {
        CardNetwork::Diners
    } else if ["5018", "5020", "5038", "5893", "6304", "6759", "6761", "6762", "6763"].iter().any(|x| digits.starts_with(x)) {
        CardNetwork::Maestro
    } else {
        CardNetwork::Unknown
    }
}

/// Number with spaces, the way it is printed on the card (4-6-5 for Amex).
pub fn group_card_number(digits: &str) -> String {
    if detect_network(digits) == CardNetwork::Amex && digits.len() == 15 {
        return format!("{} {} {}", &digits[..4], &digits[4..10], &digits[10..]);
    }
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && i % 4 == 0 {
            out.push(' ');
        }
        out.push(c);
    }
    out
}

/// Four-digit year from a two or four-digit one; `None` if it makes no sense.
pub fn full_year(year: i64) -> Option<i64> {
    if year < 0 {
        return None;
    }
    let y = if year < 100 { 2000 + year } else { year };
    (2000..=2100).contains(&y).then_some(y)
}

/// Expiry typed in one field: `08/27`, `8/2027`, `0827`, `08 - 2027`.
pub fn parse_expiry(text: &str) -> Option<(u32, i64)> {
    static R: OnceLock<Regex> = OnceLock::new();
    let m = rx(&R, r"^\s*(\d{1,2})\s*[/\-. ]?\s*(\d{2}|\d{4})\s*$").captures(text)?;
    let month: u32 = m[1].parse().ok()?;
    let year = full_year(m[2].parse().ok()?)?;
    (1..=12).contains(&month).then_some((month, year))
}

/// True once the last day of the expiry month is over.
pub fn is_expired(month: u32, year: i64, now: NaiveDate) -> bool {
    let Some(y) = full_year(year) else { return true };
    if !(1..=12).contains(&month) {
        return true;
    }
    let (ny, nm) = (now.year() as i64, now.month());
    y < ny || (y == ny && month < nm)
}

#[derive(Debug, Clone, Default)]
pub struct ExpiryHint {
    pub max_length: Option<u32>,
    pub placeholder: Option<String>,
}

/// Combined expiry field: `MM/YY` by default, `MM/YYYY`, `MMYY`, `MM-YY`... following the placeholder or max length.
pub fn format_expiry(month: u32, year: i64, hint: &ExpiryHint) -> String {
    static LONG: OnceLock<Regex> = OnceLock::new();
    static SLASH: OnceLock<Regex> = OnceLock::new();
    let mm = format!("{month:02}");
    let full = full_year(year).unwrap_or(year).to_string();
    let ph = hint.placeholder.clone().unwrap_or_default();
    let max = hint.max_length.unwrap_or(0);
    let long_year = rx(&LONG, r"(?i)y{4}|a{4}|\d{2}\s*[/-]\s*\d{4}").is_match(&ph) || max == 7 || max == 6;
    let sep = if max == 4 || max == 6 {
        ""
    } else if rx(&SLASH, r"\s/\s").is_match(&ph) {
        " / "
    } else if ph.contains('-') {
        "-"
    } else {
        "/"
    };
    let y = if long_year { full } else { full[full.len().saturating_sub(2)..].to_string() };
    format!("{mm}{sep}{y}")
}

/// Year for its own field: two digits when the field only holds two, four otherwise.
pub fn format_year(year: i64, hint: &ExpiryHint) -> String {
    static R: OnceLock<Regex> = OnceLock::new();
    let full = full_year(year).unwrap_or(year).to_string();
    let short = hint.max_length == Some(2) || rx(&R, r"(?i)^(y{2}|a{2})$").is_match(hint.placeholder.as_deref().unwrap_or("").trim());
    if short { full[full.len().saturating_sub(2)..].to_string() } else { full }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn luhn_valid_and_invalid() {
        for n in ["4242424242424242", "5555555555554444", "378282246310005", "6011111111111117", "3530111333300000"] { assert!(luhn(n), "{n}"); }
        assert!(!luhn("4242424242424241"));
        assert!(!luhn("424242"));
        assert!(!luhn("abcdefghijklmnop"));
    }

    #[test]
    fn digits() {
        assert_eq!(card_digits("4242 4242-4242 4242").as_deref(), Some("4242424242424242"));
        assert_eq!(card_digits("4242 4242 x"), None);
        assert_eq!(card_digits("1234"), None);
    }

    #[test]
    fn networks() {
        for (n, net) in [
            ("4242424242424242", CardNetwork::Visa), ("5555555555554444", CardNetwork::Mastercard), ("2223003122003222", CardNetwork::Mastercard),
            ("378282246310005", CardNetwork::Amex), ("6011111111111117", CardNetwork::Discover), ("6221260000000000", CardNetwork::Discover),
            ("6200000000000005", CardNetwork::Unionpay), ("3530111333300000", CardNetwork::Jcb), ("30569309025904", CardNetwork::Diners),
            ("6759649826438453", CardNetwork::Maestro), ("9999999999999995", CardNetwork::Unknown),
        ] { assert_eq!(detect_network(n), net, "{n}"); }
        assert_eq!(group_card_number("4242424242424242"), "4242 4242 4242 4242");
        assert_eq!(group_card_number("378282246310005"), "3782 822463 10005");
    }

    #[test]
    fn expiry_parse() {
        for (t, m, y) in [("08/27", 8, 2027), ("8/2027", 8, 2027), ("0827", 8, 2027), ("08 / 27", 8, 2027), ("12-2030", 12, 2030), ("1227", 12, 2027)] {
            assert_eq!(parse_expiry(t), Some((m, y)), "{t}");
        }
        for t in ["13/27", "00/27", "ab/cd", ""] { assert_eq!(parse_expiry(t), None, "{t}"); }
        assert_eq!(full_year(27), Some(2027));
        assert_eq!(full_year(2027), Some(2027));
        assert_eq!(full_year(1999), None);
    }

    #[test]
    fn expiry_checks() {
        let now = NaiveDate::from_ymd_opt(2026, 11, 1).unwrap();
        assert!(is_expired(10, 2026, now));
        assert!(!is_expired(11, 2026, now));
        assert!(!is_expired(1, 2027, now));
        assert!(is_expired(12, 2025, now));
        assert!(is_expired(13, 2030, now));
    }

    #[test]
    fn expiry_formats() {
        let h = |p: Option<&str>, m: Option<u32>| ExpiryHint { placeholder: p.map(String::from), max_length: m };
        assert_eq!(format_expiry(8, 2027, &h(None, None)), "08/27");
        assert_eq!(format_expiry(8, 2027, &h(Some("MM/YYYY"), None)), "08/2027");
        assert_eq!(format_expiry(8, 2027, &h(Some("MM / YY"), None)), "08 / 27");
        assert_eq!(format_expiry(8, 2027, &h(Some("MM-YY"), None)), "08-27");
        assert_eq!(format_expiry(8, 2027, &h(None, Some(4))), "0827");
        assert_eq!(format_expiry(8, 2027, &h(None, Some(7))), "08/2027");
        assert_eq!(format_expiry(8, 2027, &h(None, Some(6))), "082027");
        assert_eq!(format_year(2027, &h(None, None)), "2027");
        assert_eq!(format_year(2027, &h(None, Some(2))), "27");
        assert_eq!(format_year(2027, &h(Some("YY"), None)), "27");
        assert_eq!(format_year(2027, &h(None, Some(4))), "2027");
    }
}

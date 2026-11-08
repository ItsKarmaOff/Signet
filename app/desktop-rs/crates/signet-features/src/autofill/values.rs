//! Values to put in fields and options to pick (port of `shared/autofill/values.ts`).
use super::classify::{normalize_text, FieldKind};
use super::countries::COUNTRIES;
use super::AddressInput;

/// Given and family names from a full name (`Last, First` is understood).
pub fn split_name(full: &str) -> (String, String) {
    let name = full.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        return (String::new(), String::new());
    }
    if let Some(c) = name.find(',').filter(|c| *c > 0) {
        return (name[c + 1..].trim().to_string(), name[..c].trim().to_string());
    }
    match name.split_once(' ') {
        None => (name, String::new()),
        Some((g, f)) => (g.to_string(), f.to_string()),
    }
}

pub fn join_name(given: &str, family: &str) -> String {
    [given, family].iter().map(|s| s.trim()).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ")
}

pub fn full_name(a: &AddressInput) -> String {
    join_name(&a.given_name, &a.family_name)
}

/// ISO code of a country given as a code or a name (English or French); empty if unknown.
pub fn country_code(input: &str) -> String {
    let text = input.trim();
    if text.is_empty() {
        return String::new();
    }
    if text.chars().count() == 2 && text.chars().all(|c| c.is_ascii_alphabetic()) {
        return text.to_uppercase();
    }
    let key = normalize_text(text);
    for pick in [|e: &(&str, &str, &str)| e.1.to_string(), |e: &(&str, &str, &str)| e.2.to_string()] {
        for e in COUNTRIES {
            let name = pick(e);
            if name != e.0 && normalize_text(&name) == key {
                return e.0.to_string();
            }
        }
    }
    String::new()
}

/// Names of a country in the languages a form may use (`extra` first, then English and French). Only en and fr exist.
pub fn country_names_of(code: &str, extra: &[&str]) -> Vec<String> {
    let Some(e) = COUNTRIES.iter().find(|c| c.0 == code) else { return vec![] };
    let mut out: Vec<String> = Vec::new();
    for locale in extra.iter().copied().chain(["en", "fr"]) {
        let name = if locale == "fr" { e.2 } else if locale == "en" { e.1 } else { continue };
        if name != e.0 && !out.iter().any(|o| o == name) {
            out.push(name.to_string());
        }
    }
    out
}

#[derive(Debug, Clone, Default)]
pub struct ValueOpts {
    pub multiline: bool,
    pub locales: Vec<String>,
}

/// Text value for an address field.
pub fn address_value(a: &AddressInput, kind: FieldKind, opts: &ValueOpts) -> String {
    use FieldKind::*;
    let lines: Vec<&str> = a.street.split('\n').map(str::trim).filter(|l| !l.is_empty()).collect();
    match kind {
        Name => full_name(a),
        GivenName => a.given_name.trim().to_string(),
        FamilyName => a.family_name.trim().to_string(),
        Organization => a.organization.clone(),
        StreetAddress => lines.join(if opts.multiline { "\n" } else { ", " }),
        AddressLine1 => lines.first().map(|s| s.to_string()).unwrap_or_default(),
        AddressLine2 => lines.get(1).map(|s| s.to_string()).unwrap_or_default(),
        AddressLine3 => lines.iter().skip(2).copied().collect::<Vec<_>>().join(", "),
        City => a.city.clone(),
        Region => a.region.clone(),
        PostalCode => a.postal_code.clone(),
        Country => a.country.clone(),
        CountryName => {
            let loc: Vec<&str> = opts.locales.iter().map(String::as_str).collect();
            country_names_of(&a.country, &loc).into_iter().next().unwrap_or_else(|| a.country.clone())
        }
        Tel => a.phone.clone(),
        Email => a.email.clone(),
        _ => String::new(),
    }
}

#[derive(Debug, Clone)]
pub struct OptionLike {
    pub value: String,
    pub text: String,
}

/// Picks the option standing for one of the `candidates`: exact value or text first, then (4+ letters) a text that
/// starts with it. Returns the index.
pub fn pick_option(options: &[OptionLike], candidates: &[String]) -> Option<usize> {
    let norm: Vec<(String, String)> = options.iter().map(|o| (normalize_text(&o.value), normalize_text(&o.text))).collect();
    let wanted: Vec<String> = candidates.iter().map(|c| normalize_text(c)).filter(|c| !c.is_empty()).collect();
    for c in &wanted {
        if let Some(i) = norm.iter().position(|(v, t)| v == c || t == c) {
            return Some(i);
        }
    }
    for c in &wanted {
        if c.chars().count() < 4 {
            continue;
        }
        if let Some(i) = norm.iter().position(|(_, t)| t.starts_with(&format!("{c} ")) || t.starts_with(&format!("{c}(")) || t.ends_with(&format!(" {c}"))) {
            return Some(i);
        }
    }
    None
}

/// Candidates for a country or region select.
pub fn place_candidates(kind: FieldKind, a: &AddressInput, locales: &[&str]) -> Vec<String> {
    match kind {
        FieldKind::Country | FieldKind::CountryName => std::iter::once(a.country.clone()).chain(country_names_of(&a.country, locales)).filter(|s| !s.is_empty()).collect(),
        FieldKind::Region => Some(a.region.clone()).into_iter().filter(|s| !s.is_empty()).collect(),
        _ => vec![],
    }
}

/// Month option: value or text `8`, `08`, or text that starts with it (`08 - August`).
pub fn pick_month_option(options: &[OptionLike], month: u32) -> Option<usize> {
    if let Some(i) = pick_option(options, &[format!("{month:02}"), month.to_string()]) {
        return Some(i);
    }
    let re = regex::Regex::new(&format!(r"^0*{month}(\D|$)")).ok()?;
    options.iter().position(|o| re.is_match(o.text.trim()) || re.is_match(o.value.trim()))
}

pub fn year_candidates(year: i64) -> Vec<String> {
    let full = year.to_string();
    vec![full.clone(), full[full.len().saturating_sub(2)..].to_string()]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn address() -> AddressInput {
        AddressInput {
            given_name: "Jeanne".into(), family_name: "Dupont".into(), organization: "Acme".into(),
            street: "12 rue de la Paix\nB\u{e2}timent B\nEscalier 2".into(), city: "Paris".into(), postal_code: "75002".into(),
            region: "\u{ce}le-de-France".into(), country: "FR".into(), phone: "+33 1 23 45 67 89".into(), email: "jeanne@example.com".into(),
        }
    }
    fn o(v: &str, t: &str) -> OptionLike { OptionLike { value: v.into(), text: t.into() } }
    fn s(v: &[&str]) -> Vec<String> { v.iter().map(|x| x.to_string()).collect() }

    #[test]
    fn names() {
        assert_eq!(split_name("Jeanne Dupont"), ("Jeanne".into(), "Dupont".into()));
        assert_eq!(split_name("  Jean   Pierre  Martin "), ("Jean".into(), "Pierre Martin".into()));
        assert_eq!(split_name("Dupont, Jeanne"), ("Jeanne".into(), "Dupont".into()));
        assert_eq!(split_name("Madonna"), ("Madonna".into(), String::new()));
        assert_eq!(split_name(""), (String::new(), String::new()));
        assert_eq!(join_name("Jeanne", "Dupont"), "Jeanne Dupont");
        assert_eq!(join_name("Jeanne", ""), "Jeanne");
    }

    #[test]
    fn address_values() {
        let a = address();
        let v = |k| address_value(&a, k, &ValueOpts::default());
        assert_eq!(v(FieldKind::GivenName), "Jeanne");
        assert_eq!(v(FieldKind::AddressLine1), "12 rue de la Paix");
        assert_eq!(v(FieldKind::AddressLine2), "B\u{e2}timent B");
        assert_eq!(v(FieldKind::AddressLine3), "Escalier 2");
        assert_eq!(v(FieldKind::Name), "Jeanne Dupont");
        assert_eq!(v(FieldKind::StreetAddress), "12 rue de la Paix, B\u{e2}timent B, Escalier 2");
        assert_eq!(address_value(&a, FieldKind::StreetAddress, &ValueOpts { multiline: true, ..Default::default() }), "12 rue de la Paix\nB\u{e2}timent B\nEscalier 2");
        assert_eq!(address_value(&a, FieldKind::CountryName, &ValueOpts { locales: vec!["fr".into()], ..Default::default() }), "France");
        let given = AddressInput { family_name: String::new(), ..a.clone() };
        assert_eq!(address_value(&given, FieldKind::FamilyName, &ValueOpts::default()), "");
        assert_eq!(address_value(&given, FieldKind::Name, &ValueOpts::default()), "Jeanne");
        assert_eq!(full_name(&AddressInput { given_name: String::new(), ..a }), "Dupont");
    }

    #[test]
    fn countries() {
        assert_eq!(country_code("fr"), "FR");
        assert_eq!(country_code("France"), "FR");
        assert_eq!(country_code("Allemagne"), "DE");
        assert_eq!(country_code("United States"), "US");
        assert_eq!(country_code("\u{c9}tats-Unis"), "US");
        assert_eq!(country_code("Nowhereland"), "");
        assert_eq!(country_code(""), "");
        assert_eq!(country_names_of("DE", &["fr"]), s(&["Allemagne", "Germany"]));
        assert!(country_names_of("zz", &[]).is_empty());
    }

    #[test]
    fn select_matching() {
        let a = address();
        let countries = [o("", "Choose..."), o("US", "United States"), o("FR", "France"), o("DE", "Germany")];
        assert_eq!(pick_option(&countries, &place_candidates(FieldKind::Country, &a, &[])), Some(2));
        assert_eq!(pick_option(&[o("75", "France")], &place_candidates(FieldKind::Country, &a, &[])), Some(0));
        let de = AddressInput { country: "DE".into(), ..a.clone() };
        assert_eq!(pick_option(&[o("x", "Allemagne")], &place_candidates(FieldKind::Country, &de, &["fr"])), Some(0));
        let regions = [o("IDF", "Ile-de-France"), o("BRE", "Bretagne")];
        assert_eq!(pick_option(&regions, &place_candidates(FieldKind::Region, &a, &[])), Some(0));
        assert_eq!(pick_option(&regions, &s(&["bre"])), Some(1));
        assert_eq!(pick_option(&regions, &s(&["Normandie"])), None);
        assert_eq!(pick_option(&[o("1", "France (m\u{e9}tropolitaine)")], &s(&["France"])), Some(0));
        assert_eq!(pick_option(&[o("1", "Frankreich")], &s(&["FR"])), None);
    }

    #[test]
    fn months_and_years() {
        let months: Vec<OptionLike> = (1..=12).map(|m| o(&format!("{m:02}"), &format!("{m:02}"))).collect();
        assert_eq!(pick_month_option(&months, 8), Some(7));
        assert_eq!(pick_month_option(&[o("8", "August")], 8), Some(0));
        assert_eq!(pick_month_option(&[o("a", "08 - August")], 8), Some(0));
        assert_eq!(pick_month_option(&[o("a", "10 - October")], 1), None);
        assert_eq!(pick_option(&[o("2026", "2026"), o("2027", "2027")], &year_candidates(2027)), Some(1));
        assert_eq!(pick_option(&[o("27", "27")], &year_candidates(2027)), Some(0));
    }
}

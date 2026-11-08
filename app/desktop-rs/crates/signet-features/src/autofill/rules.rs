//! Autofill rules that need no I/O (port of `main/features/autofill/rules.ts`): where filling is allowed, what is
//! worth offering to save, and when a submission matches something already saved.
use chrono::NaiveDate;
use serde_json::Value;

use super::card::*;
use super::classify::normalize_text;
use super::values::{country_code, full_name, split_name};
use super::{AddressExt, AddressFields, AddressInput, AutofillAddress};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutofillKind {
    Address,
    Card,
}

impl AutofillKind {
    pub fn parse(s: &str) -> Option<Self> {
        match s { "address" => Some(Self::Address), "card" => Some(Self::Card), _ => None }
    }
}

const MAX: usize = 200;
const MAX_STREET: usize = 600;

/// Hosts where http is as safe as https for a card: the developer's own machine.
pub fn is_local_host(hostname: &str) -> bool {
    let h = hostname.to_lowercase();
    let h = h.trim_start_matches('[').trim_end_matches(']');
    h == "localhost" || h.ends_with(".localhost") || h == "127.0.0.1" || h == "::1"
}

/// Filling happens on http(s) pages only; a card also needs https (or localhost for development).
pub fn can_fill(kind: AutofillKind, origin: Option<&str>) -> bool {
    let Some(o) = origin.filter(|o| !o.is_empty()) else { return false };
    let Ok(u) = url::Url::parse(o) else { return false };
    match u.scheme() {
        "https" => true,
        "http" => kind == AutofillKind::Address || u.host_str().is_some_and(is_local_host),
        _ => false,
    }
}

pub struct OfferContext<'a> {
    pub kind: AutofillKind,
    pub origin: &'a str,
    pub is_private: bool,
    /// The "Save and fill" switch for this kind (missing = on).
    pub enabled: Option<bool>,
    pub never: &'a [String],
    /// The system can encrypt (cards only).
    pub encryption: bool,
}

pub fn may_offer(c: &OfferContext) -> bool {
    if c.is_private || c.enabled == Some(false) || c.never.iter().any(|o| o == c.origin) {
        return false;
    }
    if c.kind == AutofillKind::Card && !c.encryption {
        return false;
    }
    can_fill(c.kind, Some(c.origin))
}

fn clip(v: Option<&Value>, max: usize) -> String {
    v.and_then(|x| x.as_str()).map(|s| s.trim().chars().take(max).collect()).unwrap_or_default()
}

/// Address from untrusted input (a page, Settings): trimmed, clamped, country turned into its ISO code.
pub fn clean_address(raw: &Value) -> AddressInput {
    let g = |k: &str, max| clip(raw.get(k), max);
    let street = g("street", MAX_STREET).split(['\n', '\r']).map(str::trim).filter(|l| !l.is_empty()).take(4).collect::<Vec<_>>().join("\n");
    let (mut given, mut family) = (g("givenName", MAX), g("familyName", MAX));
    if given.is_empty() && family.is_empty() {
        (given, family) = split_name(&g("name", MAX));
    }
    AddressInput {
        given_name: given,
        family_name: family,
        organization: g("organization", MAX),
        street,
        city: g("city", MAX),
        postal_code: g("postalCode", 40),
        region: g("region", MAX),
        country: country_code(&g("country", MAX)),
        phone: g("phone", 60),
        email: g("email", MAX),
    }
}

/// Worth offering: a name, a street and where (city or postal code).
pub fn address_complete(a: &AddressInput) -> bool {
    !full_name(a).is_empty() && !a.street.is_empty() && (!a.city.is_empty() || !a.postal_code.is_empty())
}

pub fn is_empty_address(a: &AddressInput) -> bool {
    a.fields().iter().all(|v| v.is_empty())
}

fn norm(s: &str) -> String {
    normalize_text(s).split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Two addresses clearly describe the same place: same name and same street.
pub fn same_address(a: &AddressInput, b: &AddressInput) -> bool {
    !full_name(a).is_empty() && norm(&full_name(a)) == norm(&full_name(b)) && !a.street.is_empty() && norm(&a.street) == norm(&b.street)
}

pub fn identical_address(a: &AddressInput, b: &AddressInput) -> bool {
    a.fields().iter().zip(b.fields().iter()).all(|(x, y)| norm(x) == norm(y))
}

/// Saved address updated with what was just typed: a non-empty value wins.
pub fn merge_address(saved: &AddressInput, typed: &AddressInput) -> AddressInput {
    let mut out = saved.clone();
    for (o, t) in out.fields_mut().into_iter().zip(typed.fields()) {
        if !t.is_empty() {
            *o = t.to_string();
        }
    }
    out
}

#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum AddressPlan {
    None,
    Save,
    Update { id: String, merged: AddressInput },
}

pub fn plan_address(saved: &[AutofillAddress], typed: &AddressInput) -> AddressPlan {
    if !address_complete(typed) {
        return AddressPlan::None;
    }
    let Some(known) = saved.iter().find(|s| same_address(&s.input(), typed)) else { return AddressPlan::Save };
    let merged = merge_address(&known.input(), typed);
    if identical_address(&known.input(), &merged) { AddressPlan::None } else { AddressPlan::Update { id: known.id.clone(), merged } }
}

/// Card read from a submission, already checked.
#[derive(Debug, Clone, PartialEq)]
pub struct CardCandidate {
    pub holder: String,
    pub number: String,
    pub last4: String,
    pub network: CardNetwork,
    pub exp_month: u32,
    pub exp_year: i64,
}

/// A card worth offering: number passes the Luhn check and the expiry is not in the past.
pub fn parse_card_submission(raw: &Value, now: NaiveDate) -> Option<CardCandidate> {
    let digits = raw.get("number").and_then(|n| n.as_str()).and_then(card_digits)?;
    if !luhn(&digits) {
        return None;
    }
    let (month, year);
    if let Some(e) = raw.get("exp").and_then(|e| e.as_str()).filter(|e| !e.trim().is_empty()) {
        (month, year) = parse_expiry(e).unwrap_or((0, 0));
    } else {
        month = clip(raw.get("expMonth"), 4).parse::<u32>().unwrap_or(0);
        year = clip(raw.get("expYear"), 4).parse::<i64>().ok().and_then(full_year).unwrap_or(0);
    }
    if !(1..=12).contains(&month) || year == 0 || is_expired(month, year, now) {
        return None;
    }
    let network = detect_network(&digits);
    Some(CardCandidate { holder: clip(raw.get("holder"), MAX), last4: digits[digits.len() - 4..].to_string(), number: digits, network, exp_month: month, exp_year: year })
}

pub struct SavedCardRef {
    pub id: String,
    /// Decrypted number, `None` if it cannot be read.
    pub number: Option<String>,
    pub holder: String,
    pub exp_month: u32,
    pub exp_year: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CardPlan {
    None,
    Save,
    Update { id: String },
}

/// Same number = same card: nothing if the rest matches too, otherwise an update.
pub fn plan_card(saved: &[SavedCardRef], typed: &CardCandidate) -> CardPlan {
    let Some(known) = saved.iter().find(|s| s.number.as_deref() == Some(typed.number.as_str())) else { return CardPlan::Save };
    let same = known.exp_month == typed.exp_month && known.exp_year == typed.exp_year && (typed.holder.is_empty() || norm(&known.holder) == norm(&typed.holder));
    if same { CardPlan::None } else { CardPlan::Update { id: known.id.clone() } }
}

/// Saved items in the order the menu shows them: most used, then most recent.
pub fn by_usage(a: (u32, Option<i64>, i64), b: (u32, Option<i64>, i64)) -> std::cmp::Ordering {
    b.0.cmp(&a.0).then(b.1.unwrap_or(0).cmp(&a.1.unwrap_or(0))).then(b.2.cmp(&a.2))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn typed() -> AddressInput {
        clean_address(&json!({"givenName":"Jeanne","familyName":"Dupont","street":"12 rue de la Paix","city":"Paris","postalCode":"75002","country":"France","email":"j@example.com"}))
    }
    fn saved(over: impl FnOnce(&mut AutofillAddress)) -> AutofillAddress {
        let mut a = AutofillAddress::from_input("a1", typed(), 1);
        over(&mut a);
        a
    }

    #[test]
    fn https_rule() {
        let f = can_fill;
        assert!(f(AutofillKind::Address, Some("https://shop.example")));
        assert!(f(AutofillKind::Address, Some("http://shop.example")));
        assert!(f(AutofillKind::Card, Some("https://shop.example")));
        assert!(!f(AutofillKind::Card, Some("http://shop.example")));
        for ok in ["http://localhost:3000", "http://127.0.0.1:8080", "http://[::1]:8080", "http://app.localhost"] { assert!(f(AutofillKind::Card, Some(ok)), "{ok}"); }
        assert!(!f(AutofillKind::Card, Some("http://localhost.evil.example")));
        for o in [Some("file:///a"), Some("signetui://app"), Some("chrome-extension://abc"), Some("null"), Some(""), None] {
            assert!(!f(AutofillKind::Address, o) && !f(AutofillKind::Card, o), "{o:?}");
        }
    }

    #[test]
    fn offers() {
        let never: Vec<String> = vec![];
        let base = OfferContext { kind: AutofillKind::Address, origin: "https://shop.example", is_private: false, enabled: None, never: &never, encryption: true };
        assert!(may_offer(&base));
        assert!(!may_offer(&OfferContext { is_private: true, ..base }));
        assert!(!may_offer(&OfferContext { enabled: Some(false), ..base }));
        assert!(may_offer(&OfferContext { enabled: Some(true), ..base }));
        let n = vec!["https://shop.example".to_string()];
        assert!(!may_offer(&OfferContext { never: &n, ..base }));
        let other = vec!["https://other.example".to_string()];
        assert!(may_offer(&OfferContext { never: &other, ..base }));
        assert!(!may_offer(&OfferContext { kind: AutofillKind::Card, encryption: false, ..base }));
        assert!(!may_offer(&OfferContext { kind: AutofillKind::Card, origin: "http://shop.example", ..base }));
        assert!(may_offer(&OfferContext { kind: AutofillKind::Card, ..base }));
    }

    #[test]
    fn cleaning() {
        assert_eq!(typed().country, "FR");
        assert_eq!(clean_address(&json!({"street":" a \n\n b \nc\nd\ne"})).street, "a\nb\nc\nd");
        let n = clean_address(&json!({"name":5,"city":null}));
        assert_eq!((n.given_name.as_str(), n.family_name.as_str()), ("", ""));
        let g = clean_address(&json!({"givenName":" Jeanne ","familyName":""}));
        assert_eq!((g.given_name.as_str(), g.family_name.as_str()), ("Jeanne", ""));
        let f = clean_address(&json!({"givenName":"","familyName":"Dupont","name":"Autre Nom"}));
        assert_eq!((f.given_name.as_str(), f.family_name.as_str()), ("", "Dupont"));
        let m = clean_address(&json!({"name":"Jeanne Marie Dupont"}));
        assert_eq!((m.given_name.as_str(), m.family_name.as_str()), ("Jeanne", "Marie Dupont"));
        let c = clean_address(&json!({"name":"Dupont, Jeanne"}));
        assert_eq!((c.given_name.as_str(), c.family_name.as_str()), ("Jeanne", "Dupont"));
        assert_eq!(clean_address(&json!({"name":"Jeanne"})).family_name, "");
    }

    #[test]
    fn worth_saving() {
        let t = typed();
        assert_eq!(plan_address(&[], &t), AddressPlan::Save);
        assert_eq!(plan_address(&[], &AddressInput { city: String::new(), ..t.clone() }), AddressPlan::Save);
        assert_eq!(plan_address(&[], &AddressInput { city: String::new(), postal_code: String::new(), ..t.clone() }), AddressPlan::None);
        assert_eq!(plan_address(&[], &AddressInput { given_name: String::new(), family_name: String::new(), ..t.clone() }), AddressPlan::None);
        assert_eq!(plan_address(&[], &AddressInput { family_name: String::new(), ..t.clone() }), AddressPlan::Save);
        assert_eq!(plan_address(&[], &AddressInput { street: String::new(), ..t.clone() }), AddressPlan::None);
    }

    #[test]
    fn known_address_not_offered_again() {
        let t = typed();
        let s = [saved(|_| {})];
        assert_eq!(plan_address(&s, &t), AddressPlan::None);
        assert_eq!(plan_address(&s, &AddressInput { given_name: "  jeanne ".into(), family_name: " DUPONT ".into(), street: "12 Rue de la Paix".into(), ..t.clone() }), AddressPlan::None);
        assert_eq!(plan_address(&s, &clean_address(&json!({"name":"jeanne  dupont","street":t.street,"city":"Paris"}))), AddressPlan::None);
    }

    #[test]
    fn changed_field_offers_update() {
        let t = typed();
        let s = [saved(|a| a.phone = "0102030405".into())];
        match plan_address(&s, &AddressInput { phone: String::new(), postal_code: "75008".into(), ..t.clone() }) {
            AddressPlan::Update { id, merged } => {
                assert_eq!(id, "a1");
                assert_eq!(merged.postal_code, "75008");
                assert_eq!(merged.phone, "0102030405");
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(plan_address(&s, &AddressInput { street: "1 avenue Foch".into(), ..t.clone() }), AddressPlan::Save);
        assert!(!same_address(&t, &AddressInput { given_name: "Paul".into(), ..t.clone() }));
        assert!(!identical_address(&t, &merge_address(&t, &AddressInput { city: "Lyon".into(), ..t.clone() })));
    }

    fn now() -> NaiveDate { NaiveDate::from_ymd_opt(2026, 11, 1).unwrap() }
    fn sub() -> Value { json!({"holder":" J DUPONT ","number":"4242 4242 4242 4242","exp":"08/27"}) }

    #[test]
    fn card_submission() {
        let c = parse_card_submission(&sub(), now()).unwrap();
        assert_eq!(c, CardCandidate { holder: "J DUPONT".into(), number: "4242424242424242".into(), last4: "4242".into(), network: CardNetwork::Visa, exp_month: 8, exp_year: 2027 });
        let m = parse_card_submission(&json!({"number":"5555555555554444","expMonth":"12","expYear":"28"}), now()).unwrap();
        assert_eq!((m.exp_month, m.exp_year, m.network), (12, 2028, CardNetwork::Mastercard));
        let mut bad = sub();
        bad["number"] = json!("4242 4242 4242 4241");
        assert!(parse_card_submission(&bad, now()).is_none());
        let mut past = sub();
        past["exp"] = json!("10/26");
        assert!(parse_card_submission(&past, now()).is_none());
        let mut none = sub();
        none["exp"] = json!("");
        assert!(parse_card_submission(&none, now()).is_none());
        assert!(parse_card_submission(&json!({"number":42}), now()).is_none());
    }

    #[test]
    fn card_plan() {
        let known = || SavedCardRef { id: "c1".into(), number: Some("4242424242424242".into()), holder: "J Dupont".into(), exp_month: 8, exp_year: 2027 };
        let cand = parse_card_submission(&sub(), now()).unwrap();
        assert_eq!(plan_card(&[], &cand), CardPlan::Save);
        assert_eq!(plan_card(&[SavedCardRef { number: Some("5555555555554444".into()), ..known() }], &cand), CardPlan::Save);
        assert_eq!(plan_card(&[known()], &cand), CardPlan::None);
        assert_eq!(plan_card(&[SavedCardRef { exp_year: 2025, ..known() }], &cand), CardPlan::Update { id: "c1".into() });
        assert_eq!(plan_card(&[SavedCardRef { number: None, ..known() }], &cand), CardPlan::Save);
    }

    #[test]
    fn usage_order() {
        assert_eq!(by_usage((1, Some(5), 0), (2, None, 0)), std::cmp::Ordering::Greater);
        assert_eq!(by_usage((2, Some(5), 0), (2, Some(9), 0)), std::cmp::Ordering::Greater);
        assert_eq!(by_usage((2, Some(5), 3), (2, Some(5), 1)), std::cmp::Ordering::Less);
    }
}

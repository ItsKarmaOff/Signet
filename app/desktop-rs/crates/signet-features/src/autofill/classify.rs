//! Field classifier for autofill (port of `shared/autofill/classify.ts`): decides what a form field asks for.
//! English and French heuristics; the `autocomplete` attribute always wins.
use regex::Regex;
use std::sync::OnceLock;
use unicode_normalization::UnicodeNormalization;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FieldKind {
    Name,
    GivenName,
    FamilyName,
    Organization,
    StreetAddress,
    AddressLine1,
    AddressLine2,
    AddressLine3,
    City,
    Region,
    PostalCode,
    Country,
    CountryName,
    Tel,
    Email,
    CcName,
    CcNumber,
    CcExp,
    CcExpMonth,
    CcExpYear,
    CcCsc,
    CcType,
}

impl FieldKind {
    pub fn is_card(self) -> bool {
        use FieldKind::*;
        matches!(self, CcName | CcNumber | CcExp | CcExpMonth | CcExpYear | CcCsc | CcType)
    }
    /// Token as used in the `autocomplete` attribute and by the page script.
    pub fn token(self) -> &'static str {
        use FieldKind::*;
        match self {
            Name => "name", GivenName => "given-name", FamilyName => "family-name", Organization => "organization",
            StreetAddress => "street-address", AddressLine1 => "address-line1", AddressLine2 => "address-line2", AddressLine3 => "address-line3",
            City => "city", Region => "region", PostalCode => "postal-code", Country => "country", CountryName => "country-name",
            Tel => "tel", Email => "email", CcName => "cc-name", CcNumber => "cc-number", CcExp => "cc-exp",
            CcExpMonth => "cc-exp-month", CcExpYear => "cc-exp-year", CcCsc => "cc-csc", CcType => "cc-type",
        }
    }
}

/// What the page script knows about a field. `type_` is the input type, or `select` / `textarea`.
#[derive(Debug, Clone, Default)]
pub struct FieldDescriptor {
    pub type_: Option<String>,
    pub autocomplete: Option<String>,
    pub name: Option<String>,
    pub id: Option<String>,
    pub label: Option<String>,
    pub placeholder: Option<String>,
    pub aria_label: Option<String>,
    pub hidden: bool,
    pub disabled: bool,
    pub read_only: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Scope {
    #[default]
    None,
    Shipping,
    Billing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldClass {
    pub kind: FieldKind,
    /// `section-xxx` token of the autocomplete attribute, or empty.
    pub section: String,
    pub scope: Scope,
    /// True when the `autocomplete` attribute decided.
    pub explicit: bool,
}

const FILLABLE: &[&str] = &["", "text", "search", "tel", "email", "number", "select", "textarea"];

fn autocomplete_kind(token: &str) -> Option<FieldKind> {
    use FieldKind::*;
    Some(match token {
        "name" => Name, "given-name" | "additional-name" => GivenName, "family-name" => FamilyName, "organization" => Organization,
        "street-address" => StreetAddress, "address-line1" => AddressLine1, "address-line2" => AddressLine2, "address-line3" => AddressLine3,
        "address-level1" => Region, "address-level2" => City, "postal-code" => PostalCode, "country" => Country, "country-name" => CountryName,
        "tel" | "tel-national" => Tel, "email" => Email, "cc-name" | "cc-given-name" | "cc-family-name" => CcName, "cc-number" => CcNumber,
        "cc-exp" => CcExp, "cc-exp-month" => CcExpMonth, "cc-exp-year" => CcExpYear, "cc-csc" => CcCsc, "cc-type" => CcType,
        _ => return None,
    })
}

const FOREIGN: &[&str] = &["username", "current-password", "new-password", "one-time-code", "bday", "bday-day", "bday-month", "bday-year", "url", "photo", "impp"];

#[derive(Debug, PartialEq)]
pub struct Autocomplete {
    pub section: String,
    pub scope: Scope,
    pub kind: Option<FieldKind>,
    pub foreign: bool,
}

pub fn parse_autocomplete(attr: Option<&str>) -> Autocomplete {
    let mut out = Autocomplete { section: String::new(), scope: Scope::None, kind: None, foreign: false };
    for token in attr.unwrap_or("").to_lowercase().split_whitespace() {
        if token.starts_with("section-") {
            out.section = token.to_string();
        } else if token == "shipping" {
            out.scope = Scope::Shipping;
        } else if token == "billing" {
            out.scope = Scope::Billing;
        } else if let Some(k) = autocomplete_kind(token) {
            out.kind = Some(k);
        } else if FOREIGN.contains(&token) {
            out.foreign = true;
        }
    }
    out
}

/// Lowercase, no accents, separators turned into spaces, camelCase split (`postalCode` becomes `postal code`).
pub fn normalize_text(text: &str) -> String {
    static CAMEL: OnceLock<Regex> = OnceLock::new();
    static SEP: OnceLock<Regex> = OnceLock::new();
    static WS: OnceLock<Regex> = OnceLock::new();
    let camel = CAMEL.get_or_init(|| Regex::new(r"([a-z])([A-Z])").expect("static regex"));
    let sep = SEP.get_or_init(|| Regex::new(r"[_.\[\]:'\u{2019}-]+").expect("static regex"));
    let ws = WS.get_or_init(|| Regex::new(r"\s+").expect("static regex"));
    let split = camel.replace_all(text, "$1 $2");
    let stripped: String = split.nfd().filter(|c| !('\u{300}'..='\u{36f}').contains(c)).collect::<String>().to_lowercase();
    let s = sep.replace_all(&stripped, " ");
    ws.replace_all(&s, " ").trim().to_string()
}

fn rules() -> &'static Vec<(FieldKind, Regex)> {
    use FieldKind::*;
    static R: OnceLock<Vec<(FieldKind, Regex)>> = OnceLock::new();
    R.get_or_init(|| {
        let defs: &[(FieldKind, &str)] = &[
            (CcCsc, r"\b(cvc2?|cvv2?|csc|cid|cvn)\b|security code|card verification|code de (securite|verification)|cryptogramme|code de la carte"),
            (CcName, r"name on (the )?card|\bcard ?name\b|\bcc ?name\b|card ?holder|cardholder|holder name|nom (sur|du titulaire|figurant)|titulaire|nom (de|sur) la carte"),
            (CcType, r"card ?type|cc ?type|card brand|type de carte|type of card"),
            (CcExpMonth, r"\bexp\w*\b.*\b(month|mm|mois)\b|\b(month|mois) ?(of |d |de )?exp|\bcc ?(exp ?)?(month|mm)\b|\bexpmonth\b|\bmois d ?expiration"),
            (CcExpYear, r"\bexp\w*\b.*\b(year|yy|yyyy|annee|aa|aaaa)\b|\b(year|annee) ?(of |d |de )?exp|\bcc ?(exp ?)?(year|yy)\b|\bexpyear\b|\bannee d ?expiration"),
            (CcExp, r"\bexp(iry|iration|ire|ires)?\b|valid (thru|until|through|to)|valable jusqu|date d ?exp|\bmm ?/ ?(yy|yyyy|aa|aaaa)\b"),
            (CcNumber, r"card ?(number|num|no)\b|cc ?(number|num|no)\b|ccnum|cardnum|credit ?card|debit ?card|numero ?(de ?)?(la ?)?carte|carte ?(bancaire|de credit|bleue)|number on (the )?card|\bpan\b"),
            (Email, r"e ?mail|courriel|\bmel\b"),
            (Tel, r"phone|\btel\b|telephone|mobile|portable|\bgsm\b|cellulaire|\bcell\b"),
            (PostalCode, r"post(al)? ?code|\bzip\b|zip ?code|code ?postal|\bcp\b|\bplz\b|postcode"),
            (City, r"\bcity\b|\btown\b|\bville\b|commune|locality|localite|suburb|municipality"),
            (Country, r"\bcountry\b|\bpays\b|\bnation\b"),
            (Region, r"\bstate\b|province|\bregion\b|\bcounty\b|departement|prefecture|state ?/ ?province"),
            (Organization, r"company|organi[sz]ation|business name|societe|entreprise|\bcorp\b|employer"),
            (AddressLine3, r"(address|adresse|street|line|ligne) ?3\b"),
            (AddressLine2, r"(address|adresse|street|line|ligne) ?(2|two|deux)\b|apartment|\bapt\b|\bsuite\b|\bunit\b|complement|batiment|\bfloor\b|\betage\b|\bresidence\b"),
            (AddressLine1, r"(address|adresse|street|line|ligne|rue) ?(1|one|un)\b|address ?line|street ?address|ligne d ?adresse"),
            (StreetAddress, r"\baddress\b|\badresse\b|\bstreet\b|\brue\b|\bvoie\b|domicile"),
            (GivenName, r"first ?name|given ?name|\bfname\b|forename|\bprenom\b|firstname"),
            (FamilyName, r"last ?name|family ?name|surname|\blname\b|lastname|nom de famille"),
            (Name, r"full ?name|your name|nom complet|nom et prenom|prenom et nom|\bname\b|\bnom\b"),
        ];
        defs.iter().map(|(k, p)| (*k, Regex::new(p).expect("static regex"))).collect()
    })
}

fn reject() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"user ?name|login|pseudo|identifiant|file ?name|domain|nickname|\burl\b|website|site web|\bip\b").expect("static regex"))
}

fn scope_of(texts: &[&Option<String>]) -> Scope {
    static SHIP: OnceLock<Regex> = OnceLock::new();
    static BILL: OnceLock<Regex> = OnceLock::new();
    let text = texts.iter().map(|t| normalize_text(t.as_deref().unwrap_or(""))).collect::<Vec<_>>().join(" ");
    if SHIP.get_or_init(|| Regex::new(r"\b(shipping|delivery|livraison)\b").expect("static regex")).is_match(&text) {
        Scope::Shipping
    } else if BILL.get_or_init(|| Regex::new(r"\b(billing|facturation|invoice)\b").expect("static regex")).is_match(&text) {
        Scope::Billing
    } else {
        Scope::None
    }
}

fn texts(d: &FieldDescriptor) -> [&Option<String>; 5] {
    [&d.name, &d.id, &d.label, &d.placeholder, &d.aria_label]
}

fn by_type_or_text(d: &FieldDescriptor) -> Option<FieldKind> {
    match d.type_.as_deref().unwrap_or("") {
        "email" => return Some(FieldKind::Email),
        "tel" => return Some(FieldKind::Tel),
        _ => {}
    }
    for raw in texts(d) {
        let text = normalize_text(raw.as_deref().unwrap_or(""));
        if text.is_empty() {
            continue;
        }
        let rej = reject().is_match(&text);
        if let Some((k, _)) = rules().iter().find(|(kind, re)| re.is_match(&text) && !(rej && !kind.is_card() && *kind != FieldKind::Email)) {
            return Some(*k);
        }
    }
    None
}

fn fillable(d: &FieldDescriptor) -> bool {
    FILLABLE.contains(&d.type_.as_deref().unwrap_or(""))
}

/// Classifies a single field without looking at its neighbours; `None` if it is not an autofill field.
pub fn classify_field(d: &FieldDescriptor) -> Option<FieldClass> {
    if d.hidden || d.disabled || d.read_only || !fillable(d) {
        return None;
    }
    let auto = parse_autocomplete(d.autocomplete.as_deref());
    if auto.foreign {
        return None;
    }
    let guessed = |a: Scope| if a != Scope::None { a } else { scope_of(&[&d.name, &d.id]) };
    if let Some(kind) = auto.kind {
        return Some(FieldClass { kind, section: auto.section, scope: guessed(auto.scope), explicit: true });
    }
    by_type_or_text(d).map(|kind| FieldClass { kind, section: auto.section, scope: guessed(auto.scope), explicit: false })
}

/// Classifies all the fields of a form, then settles what needs the neighbours.
pub fn classify_fields(fields: &[FieldDescriptor]) -> Vec<Option<FieldClass>> {
    static MONTH: OnceLock<Regex> = OnceLock::new();
    static YEAR: OnceLock<Regex> = OnceLock::new();
    static BIRTH: OnceLock<Regex> = OnceLock::new();
    static FULL: OnceLock<Regex> = OnceLock::new();
    let mut out: Vec<Option<FieldClass>> = fields.iter().map(classify_field).collect();
    if out.iter().flatten().any(|c| c.kind == FieldKind::CcNumber) {
        let month = MONTH.get_or_init(|| Regex::new(r"\b(month|mois|mm)\b").expect("static regex"));
        let year = YEAR.get_or_init(|| Regex::new(r"\b(year|annee|yy|yyyy|aa|aaaa)\b").expect("static regex"));
        let birth = BIRTH.get_or_init(|| Regex::new(r"birth|naissance|born").expect("static regex"));
        for (i, d) in fields.iter().enumerate() {
            if out[i].is_some() || d.hidden || d.disabled || d.read_only || !fillable(d) {
                continue;
            }
            for raw in texts(d) {
                let text = normalize_text(raw.as_deref().unwrap_or(""));
                if text.is_empty() || birth.is_match(&text) {
                    continue;
                }
                let kind = if month.is_match(&text) { Some(FieldKind::CcExpMonth) } else if year.is_match(&text) { Some(FieldKind::CcExpYear) } else { None };
                if let Some(kind) = kind {
                    out[i] = Some(FieldClass { kind, section: String::new(), scope: Scope::None, explicit: false });
                    break;
                }
            }
        }
    }
    let has_given = out.iter().flatten().any(|c| c.kind == FieldKind::GivenName);
    let has_family = out.iter().flatten().any(|c| c.kind == FieldKind::FamilyName);
    let full = FULL.get_or_init(|| Regex::new(r"full|complet").expect("static regex"));
    for (i, d) in fields.iter().enumerate() {
        let Some(c) = out[i].clone() else { continue };
        if c.explicit || c.kind != FieldKind::Name {
            continue;
        }
        let joined = texts(d).iter().filter_map(|t| t.as_deref()).filter(|t| !t.is_empty()).collect::<Vec<_>>().join(" ");
        let text = normalize_text(&joined);
        if full.is_match(&text) {
            continue;
        }
        if has_given && !has_family {
            out[i] = Some(FieldClass { kind: FieldKind::FamilyName, ..c });
        }
    }
    out
}

fn is_location(k: FieldKind) -> bool {
    use FieldKind::*;
    matches!(k, StreetAddress | AddressLine1 | AddressLine2 | City | Region | PostalCode | Country | CountryName)
}

/// Address form: at least three address fields, one of which says where.
pub fn is_address_form(classes: &[Option<FieldClass>]) -> bool {
    let list: Vec<&FieldClass> = classes.iter().flatten().filter(|c| !c.kind.is_card()).collect();
    list.len() >= 3 && list.iter().any(|c| is_location(c.kind))
}

pub fn is_card_form(classes: &[Option<FieldClass>]) -> bool {
    classes.iter().flatten().any(|c| c.kind == FieldKind::CcNumber)
}

/// Fields that fill together: same section and same shipping/billing scope as the target.
pub fn same_group(a: &FieldClass, b: &FieldClass) -> bool {
    a.section == b.section && (a.scope == b.scope || ((b.kind == FieldKind::Email || b.kind == FieldKind::Tel) && b.scope == Scope::None))
}

#[cfg(test)]
mod tests {
    use super::*;
    use FieldKind::*;

    fn d() -> FieldDescriptor { FieldDescriptor { type_: Some("text".into()), ..Default::default() } }
    fn named(n: &str) -> FieldDescriptor { FieldDescriptor { name: Some(n.into()), ..d() } }
    fn label(l: &str) -> FieldDescriptor { FieldDescriptor { label: Some(l.into()), ..d() } }
    fn kind(f: &FieldDescriptor) -> Option<FieldKind> { classify_field(f).map(|c| c.kind) }

    #[test]
    fn autocomplete_tokens() {
        for (t, k) in [("name", Name), ("given-name", GivenName), ("family-name", FamilyName), ("organization", Organization), ("street-address", StreetAddress),
            ("address-line1", AddressLine1), ("address-line2", AddressLine2), ("address-line3", AddressLine3), ("address-level1", Region), ("address-level2", City),
            ("postal-code", PostalCode), ("country", Country), ("country-name", CountryName), ("tel", Tel), ("email", Email), ("cc-name", CcName),
            ("cc-number", CcNumber), ("cc-exp", CcExp), ("cc-exp-month", CcExpMonth), ("cc-exp-year", CcExpYear), ("cc-csc", CcCsc), ("cc-type", CcType)] {
            assert_eq!(kind(&FieldDescriptor { autocomplete: Some(t.into()), ..d() }), Some(k), "{t}");
        }
        let f = FieldDescriptor { autocomplete: Some("postal-code".into()), name: Some("city".into()), label: Some("Ville".into()), ..d() };
        assert_eq!(kind(&f), Some(PostalCode));
    }

    #[test]
    fn section_and_scope() {
        assert_eq!(parse_autocomplete(Some("section-blue shipping street-address")), Autocomplete { section: "section-blue".into(), scope: Scope::Shipping, kind: Some(StreetAddress), foreign: false });
        assert_eq!(parse_autocomplete(Some("billing work tel")).scope, Scope::Billing);
        let c = classify_field(&FieldDescriptor { autocomplete: Some("section-a billing cc-number".into()), ..d() }).unwrap();
        assert_eq!((c.kind, c.section.as_str(), c.scope, c.explicit), (CcNumber, "section-a", Scope::Billing, true));
    }

    #[test]
    fn foreign_tokens_and_off() {
        assert_eq!(kind(&FieldDescriptor { autocomplete: Some("username".into()), name: Some("email".into()), ..d() }), None);
        assert_eq!(kind(&FieldDescriptor { autocomplete: Some("one-time-code".into()), name: Some("code".into()), ..d() }), None);
        assert_eq!(kind(&FieldDescriptor { autocomplete: Some("off".into()), name: Some("city".into()), ..d() }), Some(City));
    }

    #[test]
    fn english_heuristics() {
        let name_cases = [("firstName", GivenName), ("last_name", FamilyName), ("fullname", Name), ("address1", AddressLine1), ("address_line_2", AddressLine2),
            ("billing_city", City), ("zip", PostalCode), ("phone_number", Tel), ("card_number", CcNumber), ("cardholder", CcName), ("exp_month", CcExpMonth),
            ("expYear", CcExpYear), ("cvv", CcCsc)];
        for (n, k) in name_cases { assert_eq!(kind(&named(n)), Some(k), "{n}"); }
        let label_cases = [("Company", Organization), ("Apartment, suite", AddressLine2), ("Street address", AddressLine1), ("Address", StreetAddress),
            ("State / Province", Region), ("Country", Country), ("Name on card", CcName), ("Expiration date", CcExp), ("Security code", CcCsc)];
        for (l, k) in label_cases { assert_eq!(kind(&label(l)), Some(k), "{l}"); }
        assert_eq!(kind(&FieldDescriptor { placeholder: Some("ZIP code".into()), ..d() }), Some(PostalCode));
        assert_eq!(kind(&FieldDescriptor { placeholder: Some("MM / YY".into()), ..d() }), Some(CcExp));
        assert_eq!(classify_field(&FieldDescriptor { type_: Some("email".into()), name: Some("x".into()), ..Default::default() }).unwrap().kind, Email);
        assert_eq!(classify_field(&FieldDescriptor { type_: Some("tel".into()), name: Some("x".into()), ..Default::default() }).unwrap().kind, Tel);
        assert_eq!(kind(&named("username")), None);
        assert_eq!(kind(&named("filename")), None);
        assert_eq!(kind(&label("Company name")), Some(Organization));
    }

    #[test]
    fn french_heuristics() {
        let cases = [("Pr\u{e9}nom", GivenName), ("Nom de famille", FamilyName), ("Nom complet", Name), ("Soci\u{e9}t\u{e9}", Organization), ("Adresse", StreetAddress),
            ("Adresse ligne 2", AddressLine2), ("Compl\u{e9}ment d\u{2019}adresse", AddressLine2), ("Code postal", PostalCode), ("Ville", City), ("Pays", Country),
            ("D\u{e9}partement", Region), ("T\u{e9}l\u{e9}phone", Tel), ("Courriel", Email), ("Num\u{e9}ro de carte", CcNumber), ("Titulaire de la carte", CcName),
            ("Date d\u{2019}expiration", CcExp), ("Mois d\u{2019}expiration", CcExpMonth), ("Cryptogramme", CcCsc), ("Code de s\u{e9}curit\u{e9}", CcCsc)];
        for (l, k) in cases { assert_eq!(kind(&label(l)), Some(k), "{l}"); }
        assert_eq!(kind(&named("codePostal")), Some(PostalCode));
    }

    #[test]
    fn ignored_fields() {
        for f in [FieldDescriptor { hidden: true, ..named("city") }, FieldDescriptor { disabled: true, ..named("city") }, FieldDescriptor { read_only: true, ..named("city") }] {
            assert!(classify_field(&f).is_none());
        }
        for t in ["password", "checkbox", "radio", "hidden", "file", "submit", "date"] {
            assert!(classify_field(&FieldDescriptor { type_: Some(t.into()), ..named("city") }).is_none(), "{t}");
        }
        assert_eq!(classify_field(&FieldDescriptor { type_: Some("select".into()), ..named("country") }).unwrap().kind, Country);
        assert_eq!(classify_field(&FieldDescriptor { type_: Some("textarea".into()), ..named("address") }).unwrap().kind, StreetAddress);
    }

    #[test]
    fn form_level() {
        assert!(is_address_form(&classify_fields(&[named("name"), named("street"), named("city"), named("zip")])));
        assert!(!is_address_form(&classify_fields(&[named("name"), named("city")])));
        assert!(!is_address_form(&classify_fields(&[named("name"), named("email"), named("phone")])));
        assert!(!is_address_form(&classify_fields(&[named("name"), named("street"), FieldDescriptor { hidden: true, ..named("city") }])));
        assert!(is_card_form(&classify_fields(&[named("card_number"), named("cvv")])));
        assert!(!is_card_form(&classify_fields(&[named("cvv"), named("exp_month")])));
    }

    #[test]
    fn loose_month_year_only_in_card_form() {
        let card = classify_fields(&[named("card_number"), named("month"), named("year")]);
        assert_eq!(card.iter().map(|c| c.as_ref().map(|c| c.kind)).collect::<Vec<_>>(), vec![Some(CcNumber), Some(CcExpMonth), Some(CcExpYear)]);
        assert_eq!(classify_fields(&[named("month"), named("year")]), vec![None, None]);
        assert!(classify_fields(&[named("card_number"), named("birth_month")])[1].is_none());
    }

    #[test]
    fn nom_next_to_prenom_is_family_name() {
        let r = classify_fields(&[FieldDescriptor { name: Some("x".into()), label: Some("Pr\u{e9}nom".into()), ..d() }, FieldDescriptor { name: Some("y".into()), label: Some("Nom".into()), ..d() }]);
        assert_eq!(r[0].as_ref().unwrap().kind, GivenName);
        assert_eq!(r[1].as_ref().unwrap().kind, FamilyName);
        assert_eq!(classify_fields(&[FieldDescriptor { name: Some("x".into()), label: Some("Nom".into()), ..d() }])[0].as_ref().unwrap().kind, Name);
    }

    #[test]
    fn groups_and_scope_guess() {
        let f = |a: &str| FieldDescriptor { name: Some("a".into()), autocomplete: Some(a.into()), ..d() };
        let r = classify_fields(&[f("shipping address-line1"), f("billing address-line1"), f("email")]);
        let (a, b, c) = (r[0].clone().unwrap(), r[1].clone().unwrap(), r[2].clone().unwrap());
        assert!(!same_group(&a, &b));
        assert!(same_group(&a, &c));
        let s1 = classify_field(&FieldDescriptor { autocomplete: Some("section-a address-level2".into()), ..d() }).unwrap();
        let s2 = classify_field(&FieldDescriptor { autocomplete: Some("section-b address-level2".into()), ..d() }).unwrap();
        assert!(!same_group(&s1, &s2));
        assert_eq!(classify_field(&named("shipping_city")).unwrap().scope, Scope::Shipping);
        assert_eq!(classify_field(&named("billing_city")).unwrap().scope, Scope::Billing);
    }

    #[test]
    fn normalize() {
        assert_eq!(normalize_text("postalCode"), "postal code");
        assert_eq!(normalize_text("  Pr\u{e9}nom_X "), "prenom x");
    }
}

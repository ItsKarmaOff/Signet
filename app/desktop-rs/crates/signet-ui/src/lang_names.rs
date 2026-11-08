//! Language names in English and French, shared by the toolbar (translate bar) and the settings (accept languages).

/// (code, English name, French name): the `Intl.DisplayNames` dialect names of the offered languages.
const DIALECTS: [(&str, &str, &str); 24] = [
    ("fr-FR", "French (France)", "fran\u{e7}ais (France)"),
    ("fr-CA", "Canadian French", "fran\u{e7}ais canadien"),
    ("fr-BE", "French (Belgium)", "fran\u{e7}ais (Belgique)"),
    ("fr-CH", "Swiss French", "fran\u{e7}ais suisse"),
    ("en-US", "American English", "anglais am\u{e9}ricain"),
    ("en-GB", "British English", "anglais britannique"),
    ("en-CA", "Canadian English", "anglais canadien"),
    ("en-AU", "Australian English", "anglais australien"),
    ("es-ES", "European Spanish", "espagnol europ\u{e9}en"),
    ("es-MX", "Mexican Spanish", "espagnol mexicain"),
    ("de-DE", "German (Germany)", "allemand (Allemagne)"),
    ("it-IT", "Italian (Italy)", "italien (Italie)"),
    ("pt-PT", "European Portuguese", "portugais europ\u{e9}en"),
    ("pt-BR", "Brazilian Portuguese", "portugais br\u{e9}silien"),
    ("nl-NL", "Dutch (Netherlands)", "n\u{e9}erlandais (Pays-Bas)"),
    ("ja-JP", "Japanese (Japan)", "japonais (Japon)"),
    ("ko-KR", "Korean (South Korea)", "cor\u{e9}en (Cor\u{e9}e du Sud)"),
    ("zh-CN", "Chinese (China)", "chinois (Chine)"),
    ("zh-TW", "Chinese (Taiwan)", "chinois (Ta\u{ef}wan)"),
    ("ar", "Arabic", "arabe"),
    ("ru-RU", "Russian (Russia)", "russe (Russie)"),
    ("pl-PL", "Polish (Poland)", "polonais (Pologne)"),
    ("sv-SE", "Swedish (Sweden)", "su\u{e9}dois (Su\u{e8}de)"),
    ("tr-TR", "Turkish (T\u{fc}rkiye)", "turc (Turquie)"),
];

/// Name of an offered language code with its region (`fr-CA`), in English or French; `None` if not offered.
pub fn dialect_name(code: &str, fr: bool) -> Option<&'static str> {
    DIALECTS.iter().find(|n| n.0 == code).map(|n| if fr { n.2 } else { n.1 })
}

const BASES: &[(&str, &str, &str)] = &[
    ("ar", "Arabic", "arabe"),
    ("cs", "Czech", "tchèque"),
    ("da", "Danish", "danois"),
    ("de", "German", "allemand"),
    ("el", "Greek", "grec"),
    ("en", "English", "anglais"),
    ("es", "Spanish", "espagnol"),
    ("fi", "Finnish", "finnois"),
    ("fr", "French", "français"),
    ("he", "Hebrew", "hébreu"),
    ("hi", "Hindi", "hindi"),
    ("hu", "Hungarian", "hongrois"),
    ("id", "Indonesian", "indonésien"),
    ("it", "Italian", "italien"),
    ("ja", "Japanese", "japonais"),
    ("ko", "Korean", "coréen"),
    ("nl", "Dutch", "néerlandais"),
    ("no", "Norwegian", "norvégien"),
    ("pl", "Polish", "polonais"),
    ("pt", "Portuguese", "portugais"),
    ("ro", "Romanian", "roumain"),
    ("ru", "Russian", "russe"),
    ("sv", "Swedish", "suédois"),
    ("th", "Thai", "thaï"),
    ("tr", "Turkish", "turc"),
    ("uk", "Ukrainian", "ukrainien"),
    ("vi", "Vietnamese", "vietnamien"),
    ("zh", "Chinese", "chinois"),
    ];

/// Name of the primary language of a code (`pt-BR` gives Portuguese); `None` if unknown.
pub fn base_language_name(code: &str, fr: bool) -> Option<&'static str> {
    let primary = code.split(['-', '_']).next().unwrap_or(code).to_lowercase();
    BASES.iter().find(|(c, _, _)| *c == primary).map(|(_, en, f)| if fr { *f } else { *en })
}

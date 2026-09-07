//! Language-code normalization (DESIGN §6.2).
//!
//! Probe tags and user-authored flow values spell the same language
//! differently (`eng`, `EN`, `en-US`, `ger`), so matching normalizes both
//! sides to ISO 639-1 first. Codes that are not a known ISO 639-1 code
//! and do not map through the table collapse to `und` (unknown) — that
//! includes ffprobe's `und`, `mis`, and `zzz`, so "unknown" is a matchable
//! value instead of a swamp of spellings.

/// The ISO 639-2 (B/T) and common-alias source codes that map onto an
/// ISO 639-1 code (all lowercase; media-library languages and their
/// frequent probe spellings).
const TABLE: &[(&str, &str)] = &[
    ("afr", "af"),
    ("alb", "sq"),
    ("sqi", "sq"),
    ("amh", "am"),
    ("ara", "ar"),
    ("arm", "hy"),
    ("hye", "hy"),
    ("asm", "as"),
    ("aze", "az"),
    ("bas", "eu"),
    ("eus", "eu"),
    ("bel", "be"),
    ("ben", "bn"),
    ("bos", "bs"),
    ("bul", "bg"),
    ("cat", "ca"),
    ("ces", "cs"),
    ("cze", "cs"),
    ("dan", "da"),
    ("deu", "de"),
    ("ger", "de"),
    ("ell", "el"),
    ("gre", "el"),
    ("eng", "en"),
    ("epo", "eo"),
    ("spa", "es"),
    ("spn", "es"),
    ("est", "et"),
    ("fas", "fa"),
    ("per", "fa"),
    ("fao", "fo"),
    ("fin", "fi"),
    ("fra", "fr"),
    ("fre", "fr"),
    ("fry", "fy"),
    ("glg", "gl"),
    ("gle", "ga"),
    ("iri", "ga"),
    ("gla", "gd"),
    ("guj", "gu"),
    ("hat", "ht"),
    ("hau", "ha"),
    ("heb", "he"),
    ("iw", "he"),
    ("hin", "hi"),
    ("hrv", "hr"),
    ("hun", "hu"),
    ("ind", "id"),
    ("ice", "is"),
    ("isl", "is"),
    ("ita", "it"),
    ("jpn", "ja"),
    ("jav", "jv"),
    ("kat", "ka"),
    ("geo", "ka"),
    ("kaz", "kk"),
    ("khm", "km"),
    ("kan", "kn"),
    ("kor", "ko"),
    ("kur", "ku"),
    ("kmr", "ku"),
    ("lao", "lo"),
    ("lat", "la"),
    ("lvs", "lv"),
    ("lit", "lt"),
    ("ltz", "lb"),
    ("mkd", "mk"),
    ("mlg", "mg"),
    ("may", "ms"),
    ("msa", "ms"),
    ("mal", "ml"),
    ("mlt", "mt"),
    ("mri", "mi"),
    ("mar", "mr"),
    ("mon", "mn"),
    ("mng", "mn"),
    ("nep", "ne"),
    ("nld", "nl"),
    ("dut", "nl"),
    ("nor", "no"),
    ("nob", "no"),
    ("nno", "no"),
    ("nya", "ny"),
    ("nb", "no"),
    ("nn", "no"),
    ("ori", "or"),
    ("pus", "ps"),
    ("pan", "pa"),
    ("pol", "pl"),
    ("por", "pt"),
    ("ron", "ro"),
    ("rum", "ro"),
    ("rus", "ru"),
    ("snd", "sd"),
    ("sin", "si"),
    ("slk", "sk"),
    ("slv", "sk"),
    ("sln", "sl"),
    ("slo", "sl"),
    ("som", "so"),
    ("sot", "st"),
    ("sna", "sn"),
    ("srp", "sr"),
    ("sun", "su"),
    ("swa", "sw"),
    ("swe", "sv"),
    ("tam", "ta"),
    ("tel", "te"),
    ("tha", "th"),
    ("tir", "ti"),
    ("tgk", "tg"),
    ("tur", "tr"),
    ("tuk", "tk"),
    ("tat", "tt"),
    ("ton", "tw"),
    ("ukr", "uk"),
    ("urd", "ur"),
    ("uzb", "uz"),
    ("vie", "vi"),
    ("cym", "cy"),
    ("xho", "xh"),
    ("yid", "yi"),
    ("yor", "yo"),
    ("zul", "zu"),
    ("chn", "zh"),
    ("chi", "zh"),
    ("zho", "zh"),
];

/// Normalize a language code to ISO 639-1 (DESIGN §6.2).
///
/// Accepts ISO 639-1, ISO 639-2 (B/T), common aliases, and BCP-47 tags
/// (region/script subtags are stripped: `en-US`, `zh-Hans` → `en`, `zh`).
/// Unknown and unlisted codes — including ffprobe's `und`, `mis`, and
/// `zzz` — normalize to `"und"`. Already-normalized input is unchanged
/// (idempotent).
#[must_use]
pub fn normalize(code: &str) -> String {
    // Strip the region/script subtags; the language part is first.
    let lang = code
        .split(['-', '_'])
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if lang.is_empty() {
        return "und".into();
    }
    if let Some((_, one)) = TABLE.iter().find(|(k, _)| *k == lang) {
        return (*one).into();
    }
    // An unknown two-letter code is treated as an ISO 639-1 code as-is
    // (ffprobe reports a long tail of rare 639-1 codes not in the table).
    if lang.len() == 2 && lang.chars().all(|c| c.is_ascii_alphabetic()) {
        return lang;
    }
    "und".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso639_2_and_aliases_map_to_639_1() {
        for (src, want) in [
            ("eng", "en"),
            ("deu", "de"),
            ("ger", "de"),
            ("fra", "fr"),
            ("fre", "fr"),
            ("spa", "es"),
            ("spn", "es"),
            ("nld", "nl"),
            ("dut", "nl"),
            ("chn", "zh"),
            ("chi", "zh"),
            ("zho", "zh"),
            ("jpn", "ja"),
            ("kor", "ko"),
            ("rus", "ru"),
            ("por", "pt"),
            ("ita", "it"),
            ("pol", "pl"),
            ("cze", "cs"),
            ("ces", "cs"),
            ("ell", "el"),
            ("gre", "el"),
            ("heb", "he"),
            ("iw", "he"),
            ("ind", "id"),
            ("swe", "sv"),
            ("nor", "no"),
            ("nob", "no"),
            ("nno", "no"),
            ("slk", "sk"),
            ("slv", "sk"),
            ("ron", "ro"),
            ("rum", "ro"),
            ("tur", "tr"),
            ("ara", "ar"),
            ("ben", "bn"),
            ("tam", "ta"),
            ("tel", "te"),
            ("vie", "vi"),
            ("msa", "ms"),
            ("may", "ms"),
            ("khm", "km"),
            ("kmr", "ku"),
            ("hrv", "hr"),
            ("srp", "sr"),
            ("ice", "is"),
            ("isl", "is"),
            ("yid", "yi"),
            ("ukr", "uk"),
            ("fas", "fa"),
            ("per", "fa"),
            ("hin", "hi"),
            ("pan", "pa"),
            ("kat", "ka"),
            ("geo", "ka"),
            ("kaz", "kk"),
            ("mar", "mr"),
            ("mri", "mi"),
            ("slo", "sl"),
            ("sna", "sn"),
            ("som", "so"),
            ("ton", "tw"),
            ("urd", "ur"),
            ("uzb", "uz"),
            ("zul", "zu"),
            ("xho", "xh"),
            ("yor", "yo"),
            ("cym", "cy"),
            ("gle", "ga"),
            ("est", "et"),
            ("lat", "la"),
            ("mon", "mn"),
        ] {
            assert_eq!(normalize(src), want, "{src}");
        }
    }

    #[test]
    fn iso639_1_passes_through() {
        for c in [
            "en", "de", "fr", "zh", "ja", "ko", "ru", "pt", "it", "nl", "pl",
        ] {
            assert_eq!(normalize(c), c, "{c}");
        }
    }

    #[test]
    fn bcp47_subtags_are_stripped() {
        assert_eq!(normalize("en-US"), "en");
        assert_eq!(normalize("EN-US"), "en");
        assert_eq!(normalize("zh-CN"), "zh");
        assert_eq!(normalize("zh-Hans"), "zh");
        assert_eq!(normalize("pt-BR"), "pt");
        assert_eq!(normalize("nb-NO"), "no"); // Norwegian: nb/nno variants → no
    }

    #[test]
    fn unknown_and_unlisted_collapse_to_und() {
        for c in ["", "und", "mis", "zzz", "zxx", "und-XX", "xxxy", "123"] {
            assert_eq!(normalize(c), "und", "{c}");
        }
    }

    #[test]
    fn unknown_two_letter_code_is_kept() {
        // A rare but valid 639-1 code not in the table stays as-is rather
        // than collapsing to unknown.
        assert_eq!(normalize("ee"), "ee");
        assert_eq!(normalize("qu"), "qu");
    }

    #[test]
    fn normalization_is_idempotent() {
        for c in ["eng", "en-US", "ger", "und", "xxxy", "ee", "nb-NO"] {
            let once = normalize(c);
            assert_eq!(normalize(&once), once, "{c}");
        }
    }
}

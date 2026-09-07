//! Language-code normalization (DESIGN §6.2).
//!
//! Probe tags and user-authored flow values spell the same language
//! differently (`eng`, `EN`, `en-US`, `ger`), so matching normalizes both
//! sides to ISO 639-1 first. The 639-1 / 639-2 (B and T) / 639-3 crosswalk
//! comes from the official ISO 639 database shipped by the `rust-iso639`
//! crate (Apache-2.0), not a hand-maintained list: codes are resolved
//! through the database's own maps, and each 639-1 language's individual
//! 639-3 varieties (e.g. `nob`/`nno` under Norwegian `no`) through the
//! per-language lists the database carries. Codes without an ISO 639-1
//! equivalent and codes the database doesn't know — including ffprobe's
//! `und`, `mis`, and `zzz` — collapse to `und` (unknown), so "unknown"
//! is a matchable value instead of a swamp of spellings. A few retired
//! or non-ISO spellings still seen in container tags, and IANA-registered
//! subtags the crate's table misclassifies as 639-1, live in [`LEGACY`].

use rust_iso639::{ALL_1, from_code_1, from_code_2b, from_code_2t, from_code_3};

/// Spellings that the `rust-iso639` table gets wrong or omits, resolved
/// here from the ISO/SIL definitions instead. Checked before the database
/// so these overrides always win.
///
/// `iw`/`spn`/`chn` are retired or non-ISO spellings the database no longer
/// lists. `nb`/`nn` (and their 639-3 spellings `nob`/`nno`) are subtags the
/// crate's table promotes to 639-1, but ISO 639-1 and SIL 639-3 classify
/// them as varieties of Norwegian `no` — matching normalizes to the family
/// code.
const LEGACY: &[(&str, &str)] = &[
    ("iw", "he"),  // Hebrew's retired 1989 639-1 code (replaced by `he`).
    ("spn", "es"), // Non-standard spelling of Spanish (ISO uses `spa`).
    ("nob", "no"), // Norwegian Bokmål, a variety of `no`.
    ("nb", "no"),  // Bokmål's two-letter subtag, ditto.
    ("nno", "no"), // Norwegian Nynorsk, a variety of `no`.
    ("nn", "no"),  // Nynorsk's two-letter subtag, ditto.
    ("chn", "zh"), // Pre-2005 code for Chinese seen in older MP4 tags.
];

/// Resolve a database code — ISO 639-1, 639-2 (B or T), or 639-3,
/// including a macrolanguage's individual 639-3 varieties — to its
/// 639-1 code. `None` if the code has no 639-1 equivalent (e.g. `zxx`,
/// "no content") or isn't in the database.
fn code_to_639_1(lang: &str) -> Option<&'static str> {
    // Fast path: the database's perfect-hash maps. A hit whose 639-1
    // field is empty (e.g. an individual 639-3 variety like `kmr`) is not
    // final — fall through to the per-language scan for a parent row.
    if let Some(l) = from_code_1(lang)
        .or_else(|| from_code_2t(lang))
        .or_else(|| from_code_2b(lang))
        .or_else(|| from_code_3(lang))
    {
        if !l.code.is_empty() {
            return Some(l.code);
        }
    }
    // The maps don't index individual 639-3 varieties; the 639-1 table's
    // rows carry them per language.
    for l in ALL_1 {
        if l.code.is_empty() {
            continue;
        }
        if l.code == lang
            || l.code_2t == lang
            || l.code_2b == lang
            || l.code_3 == lang
            || l.individual_languages.iter().any(|i| i.code == lang)
        {
            return Some(l.code);
        }
    }
    None
}

/// Normalize a language code to ISO 639-1 (DESIGN §6.2).
///
/// Accepts ISO 639-1, ISO 639-2 (B/T), ISO 639-3, and BCP-47 tags
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
    if let Some((_, one)) = LEGACY.iter().find(|(k, _)| *k == lang) {
        return (*one).into();
    }
    if let Some(one) = code_to_639_1(&lang) {
        return one.into();
    }
    // An unknown two-letter code is treated as an ISO 639-1 code as-is
    // (ffprobe reports a long tail of rare 639-1 codes).
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
            ("nld", "nl"),
            ("dut", "nl"),
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
            ("ind", "id"),
            ("swe", "sv"),
            ("nor", "no"),
            ("nob", "no"),
            ("nno", "no"),
            ("nb", "no"),
            ("nn", "no"),
            ("slk", "sk"),
            ("slv", "sl"),
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
            ("slo", "sk"), // 639-2/B code for Slovak.
            ("sna", "sn"),
            ("som", "so"),
            ("ton", "to"), // 639-2/3 code for Tongan (Tonga Islands).
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
            ("afr", "af"),
            ("alb", "sq"),
            ("sqi", "sq"),
            ("amh", "am"),
            ("arm", "hy"),
            ("hye", "hy"),
            ("asm", "as"),
            ("aze", "az"),
            ("eus", "eu"),
            ("bel", "be"),
            ("bos", "bs"),
            ("bul", "bg"),
            ("cat", "ca"),
            ("dan", "da"),
            ("epo", "eo"),
            ("fin", "fi"),
            ("fry", "fy"),
            ("glg", "gl"),
            ("gla", "gd"),
            ("guj", "gu"),
            ("hat", "ht"),
            ("hau", "ha"),
            ("hun", "hu"),
            ("jav", "jv"),
            ("lao", "lo"),
            ("lvs", "lv"),
            ("lit", "lt"),
            ("ltz", "lb"),
            ("mkd", "mk"),
            ("mlg", "mg"),
            ("mal", "ml"),
            ("mlt", "mt"),
            ("nep", "ne"),
            ("nya", "ny"),
            ("ori", "or"),
            ("pus", "ps"),
            ("sin", "si"),
            ("sun", "su"),
            ("swa", "sw"),
            ("tir", "ti"),
            ("tgk", "tg"),
            ("tuk", "tk"),
            ("tat", "tt"),
            ("kan", "kn"),
            ("kur", "ku"),
            ("fao", "fo"),
            ("sot", "st"),
        ] {
            assert_eq!(normalize(src), want, "{src}");
        }
    }

    #[test]
    fn retired_and_non_iso_spellings_still_map() {
        // Not in the ISO database; kept for container writers that still
        // emit them.
        assert_eq!(normalize("iw"), "he");
        assert_eq!(normalize("spn"), "es");
        assert_eq!(normalize("chn"), "zh");
    }

    #[test]
    fn bas_is_basa_not_basque() {
        // The previous hand-maintained table wrongly mapped `bas` (Basa,
        // Cameroon) to `eu`. Basque is `eu`/`eus` in the official table;
        // Basa has no 639-1 equivalent and collapses to unknown.
        assert_eq!(normalize("bas"), "und");
        assert_eq!(normalize("eus"), "eu");
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
        // `iri`/`mng`/`sln` are real 639-3 codes for languages without a
        // 639-1 equivalent (Rigwe, Eastern Mnong, Salinan) — they collapse
        // too, even though the database knows them.
        for c in [
            "", "und", "mis", "zzz", "zxx", "und-XX", "xxxy", "123", "iri", "mng", "sln",
        ] {
            assert_eq!(normalize(c), "und", "{c}");
        }
    }

    #[test]
    fn unknown_two_letter_code_is_kept() {
        // Two-letter codes the database doesn't list are kept as-is
        // rather than collapsing to unknown. `bh` is a real IANA-registered
        // two-letter subtag outside ISO 639-1; `zz` is a made-up one.
        assert_eq!(normalize("bh"), "bh");
        assert_eq!(normalize("zz"), "zz");
    }

    #[test]
    fn normalization_is_idempotent() {
        for c in ["eng", "en-US", "ger", "und", "xxxy", "ee", "nb-NO"] {
            let once = normalize(c);
            assert_eq!(normalize(&once), once, "{c}");
        }
    }
}

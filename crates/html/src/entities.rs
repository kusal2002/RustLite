//! Named character references.
//!
//! **Subset.** The HTML standard defines 2,231 named references. This table
//! covers every *legacy* reference (the ones recognized without a trailing
//! semicolon: all of Latin-1 plus `amp`/`lt`/`gt`/`quot` and their uppercase
//! forms) and ~120 commonly used semicolon-terminated ones. Unknown names are
//! left as literal text, which is also what browsers do for invalid names.
//! Importing the full table is tracked in docs/compatibility.md.

/// Latin-1 Supplement names for U+00A0..=U+00FF, in code point order. All are
/// legacy references (semicolon optional).
const LATIN1: [&str; 96] = [
    "nbsp", "iexcl", "cent", "pound", "curren", "yen", "brvbar", "sect", "uml", "copy", "ordf",
    "laquo", "not", "shy", "reg", "macr", "deg", "plusmn", "sup2", "sup3", "acute", "micro",
    "para", "middot", "cedil", "sup1", "ordm", "raquo", "frac14", "frac12", "frac34", "iquest",
    "Agrave", "Aacute", "Acirc", "Atilde", "Auml", "Aring", "AElig", "Ccedil", "Egrave", "Eacute",
    "Ecirc", "Euml", "Igrave", "Iacute", "Icirc", "Iuml", "ETH", "Ntilde", "Ograve", "Oacute",
    "Ocirc", "Otilde", "Ouml", "times", "Oslash", "Ugrave", "Uacute", "Ucirc", "Uuml", "Yacute",
    "THORN", "szlig", "agrave", "aacute", "acirc", "atilde", "auml", "aring", "aelig", "ccedil",
    "egrave", "eacute", "ecirc", "euml", "igrave", "iacute", "icirc", "iuml", "eth", "ntilde",
    "ograve", "oacute", "ocirc", "otilde", "ouml", "divide", "oslash", "ugrave", "uacute", "ucirc",
    "uuml", "yacute", "thorn", "yuml",
];

/// Other legacy (semicolon-optional) references.
const LEGACY_OTHER: &[(&str, char)] = &[
    ("amp", '&'),
    ("AMP", '&'),
    ("lt", '<'),
    ("LT", '<'),
    ("gt", '>'),
    ("GT", '>'),
    ("quot", '"'),
    ("QUOT", '"'),
    ("COPY", '\u{A9}'),
    ("REG", '\u{AE}'),
];

/// Semicolon-required references (names stored without the semicolon).
const SEMICOLON_ONLY: &[(&str, char)] = &[
    ("apos", '\''),
    ("OElig", '\u{152}'),
    ("oelig", '\u{153}'),
    ("Scaron", '\u{160}'),
    ("scaron", '\u{161}'),
    ("Yuml", '\u{178}'),
    ("fnof", '\u{192}'),
    ("circ", '\u{2C6}'),
    ("tilde", '\u{2DC}'),
    ("Alpha", '\u{391}'),
    ("Beta", '\u{392}'),
    ("Gamma", '\u{393}'),
    ("Delta", '\u{394}'),
    ("Epsilon", '\u{395}'),
    ("Zeta", '\u{396}'),
    ("Eta", '\u{397}'),
    ("Theta", '\u{398}'),
    ("Iota", '\u{399}'),
    ("Kappa", '\u{39A}'),
    ("Lambda", '\u{39B}'),
    ("Mu", '\u{39C}'),
    ("Nu", '\u{39D}'),
    ("Xi", '\u{39E}'),
    ("Omicron", '\u{39F}'),
    ("Pi", '\u{3A0}'),
    ("Rho", '\u{3A1}'),
    ("Sigma", '\u{3A3}'),
    ("Tau", '\u{3A4}'),
    ("Upsilon", '\u{3A5}'),
    ("Phi", '\u{3A6}'),
    ("Chi", '\u{3A7}'),
    ("Psi", '\u{3A8}'),
    ("Omega", '\u{3A9}'),
    ("alpha", '\u{3B1}'),
    ("beta", '\u{3B2}'),
    ("gamma", '\u{3B3}'),
    ("delta", '\u{3B4}'),
    ("epsilon", '\u{3B5}'),
    ("zeta", '\u{3B6}'),
    ("eta", '\u{3B7}'),
    ("theta", '\u{3B8}'),
    ("iota", '\u{3B9}'),
    ("kappa", '\u{3BA}'),
    ("lambda", '\u{3BB}'),
    ("mu", '\u{3BC}'),
    ("nu", '\u{3BD}'),
    ("xi", '\u{3BE}'),
    ("omicron", '\u{3BF}'),
    ("pi", '\u{3C0}'),
    ("rho", '\u{3C1}'),
    ("sigmaf", '\u{3C2}'),
    ("sigma", '\u{3C3}'),
    ("tau", '\u{3C4}'),
    ("upsilon", '\u{3C5}'),
    ("phi", '\u{3C6}'),
    ("chi", '\u{3C7}'),
    ("psi", '\u{3C8}'),
    ("omega", '\u{3C9}'),
    ("ensp", '\u{2002}'),
    ("emsp", '\u{2003}'),
    ("thinsp", '\u{2009}'),
    ("zwnj", '\u{200C}'),
    ("zwj", '\u{200D}'),
    ("lrm", '\u{200E}'),
    ("rlm", '\u{200F}'),
    ("ndash", '\u{2013}'),
    ("mdash", '\u{2014}'),
    ("lsquo", '\u{2018}'),
    ("rsquo", '\u{2019}'),
    ("sbquo", '\u{201A}'),
    ("ldquo", '\u{201C}'),
    ("rdquo", '\u{201D}'),
    ("bdquo", '\u{201E}'),
    ("dagger", '\u{2020}'),
    ("Dagger", '\u{2021}'),
    ("bull", '\u{2022}'),
    ("hellip", '\u{2026}'),
    ("permil", '\u{2030}'),
    ("prime", '\u{2032}'),
    ("Prime", '\u{2033}'),
    ("lsaquo", '\u{2039}'),
    ("rsaquo", '\u{203A}'),
    ("oline", '\u{203E}'),
    ("frasl", '\u{2044}'),
    ("euro", '\u{20AC}'),
    ("trade", '\u{2122}'),
    ("larr", '\u{2190}'),
    ("uarr", '\u{2191}'),
    ("rarr", '\u{2192}'),
    ("darr", '\u{2193}'),
    ("harr", '\u{2194}'),
    ("lArr", '\u{21D0}'),
    ("uArr", '\u{21D1}'),
    ("rArr", '\u{21D2}'),
    ("dArr", '\u{21D3}'),
    ("hArr", '\u{21D4}'),
    ("forall", '\u{2200}'),
    ("part", '\u{2202}'),
    ("exist", '\u{2203}'),
    ("empty", '\u{2205}'),
    ("nabla", '\u{2207}'),
    ("isin", '\u{2208}'),
    ("notin", '\u{2209}'),
    ("prod", '\u{220F}'),
    ("sum", '\u{2211}'),
    ("minus", '\u{2212}'),
    ("radic", '\u{221A}'),
    ("infin", '\u{221E}'),
    ("and", '\u{2227}'),
    ("or", '\u{2228}'),
    ("cap", '\u{2229}'),
    ("cup", '\u{222A}'),
    ("int", '\u{222B}'),
    ("asymp", '\u{2248}'),
    ("ne", '\u{2260}'),
    ("equiv", '\u{2261}'),
    ("le", '\u{2264}'),
    ("ge", '\u{2265}'),
    ("sub", '\u{2282}'),
    ("sup", '\u{2283}'),
    ("loz", '\u{25CA}'),
    ("spades", '\u{2660}'),
    ("clubs", '\u{2663}'),
    ("hearts", '\u{2665}'),
    ("diams", '\u{2666}'),
    ("check", '\u{2713}'),
];

/// A named reference matched at the start of the input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NamedMatch {
    /// Bytes consumed, including the semicolon if present.
    pub len: usize,
    pub value: char,
    pub has_semicolon: bool,
}

/// Finds the longest named reference at the start of `input` (the text right
/// after `&`), following the standard's longest-match rule.
pub(crate) fn match_named(input: &str) -> Option<NamedMatch> {
    let mut best: Option<NamedMatch> = None;
    let mut consider = |name: &str, value: char, legacy: bool| {
        let Some(rest) = input.strip_prefix(name) else {
            return;
        };
        let candidate = if rest.starts_with(';') {
            NamedMatch {
                len: name.len() + 1,
                value,
                has_semicolon: true,
            }
        } else if legacy {
            NamedMatch {
                len: name.len(),
                value,
                has_semicolon: false,
            }
        } else {
            return;
        };
        if best.is_none_or(|b| candidate.len > b.len) {
            best = Some(candidate);
        }
    };
    for (i, name) in LATIN1.iter().enumerate() {
        // i < 96, so 0xA0 + i is a valid scalar value.
        consider(
            name,
            char::from_u32(0xA0 + i as u32).expect("Latin-1"),
            true,
        );
    }
    for &(name, value) in LEGACY_OTHER {
        consider(name, value, true);
    }
    for &(name, value) in SEMICOLON_ONLY {
        consider(name, value, false);
    }
    best
}

/// Replacement for numeric references in 0x80..=0x9F (windows-1252 mapping).
pub(crate) fn c1_replacement(code: u32) -> Option<char> {
    let c = match code {
        0x80 => '\u{20AC}',
        0x82 => '\u{201A}',
        0x83 => '\u{0192}',
        0x84 => '\u{201E}',
        0x85 => '\u{2026}',
        0x86 => '\u{2020}',
        0x87 => '\u{2021}',
        0x88 => '\u{02C6}',
        0x89 => '\u{2030}',
        0x8A => '\u{0160}',
        0x8B => '\u{2039}',
        0x8C => '\u{0152}',
        0x8E => '\u{017D}',
        0x91 => '\u{2018}',
        0x92 => '\u{2019}',
        0x93 => '\u{201C}',
        0x94 => '\u{201D}',
        0x95 => '\u{2022}',
        0x96 => '\u{2013}',
        0x97 => '\u{2014}',
        0x98 => '\u{02DC}',
        0x99 => '\u{2122}',
        0x9A => '\u{0161}',
        0x9B => '\u{203A}',
        0x9C => '\u{0153}',
        0x9E => '\u{017E}',
        0x9F => '\u{0178}',
        _ => return None,
    };
    Some(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latin1_table_alignment() {
        assert_eq!(match_named("nbsp;").unwrap().value, '\u{A0}');
        assert_eq!(match_named("times;").unwrap().value, '\u{D7}');
        assert_eq!(match_named("yuml;").unwrap().value, '\u{FF}');
        assert_eq!(match_named("eacute").unwrap().value, '\u{E9}');
    }

    #[test]
    fn longest_match_wins() {
        // "not" is legacy; "notin;" is longer and wins when present.
        let m = match_named("notin;").unwrap();
        assert_eq!((m.value, m.len), ('\u{2209}', 6));
        let m = match_named("notit;").unwrap();
        assert_eq!((m.value, m.len, m.has_semicolon), ('\u{AC}', 3, false));
    }

    #[test]
    fn semicolon_required_for_modern_names() {
        assert_eq!(match_named("hellip"), None);
        assert!(match_named("hellip;").is_some());
    }
}

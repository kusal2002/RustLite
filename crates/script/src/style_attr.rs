//! Reading and writing declarations in a `style` attribute, for
//! `element.style` (a simplified CSSStyleDeclaration, CSSOM §6.7).

/// Properties exposed as `style.camelCase` / `style['hyphen-case']`
/// accessors: every property the style system understands.
pub(crate) const PROPERTIES: &[&str] = &[
    "display",
    "color",
    "background",
    "background-color",
    "font",
    "font-size",
    "font-weight",
    "font-style",
    "font-family",
    "line-height",
    "text-align",
    "list-style",
    "list-style-type",
    "overflow",
    "overflow-x",
    "overflow-y",
    "width",
    "height",
    "min-width",
    "max-width",
    "min-height",
    "max-height",
    "margin",
    "margin-top",
    "margin-right",
    "margin-bottom",
    "margin-left",
    "padding",
    "padding-top",
    "padding-right",
    "padding-bottom",
    "padding-left",
    "border",
    "border-top",
    "border-right",
    "border-bottom",
    "border-left",
    "border-width",
    "border-style",
    "border-color",
    "border-top-width",
    "border-right-width",
    "border-bottom-width",
    "border-left-width",
    "border-top-style",
    "border-right-style",
    "border-bottom-style",
    "border-left-style",
    "border-top-color",
    "border-right-color",
    "border-bottom-color",
    "border-left-color",
];

/// `background-color` → `backgroundColor`.
pub(crate) fn camel_case(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut upper = false;
    for c in name.chars() {
        if c == '-' {
            upper = true;
        } else if upper {
            out.push(c.to_ascii_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// Splits `text` at top-level occurrences of `sep` (not inside quotes,
/// parentheses or brackets).
fn split_top_level(text: &str, sep: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let (mut depth, mut quote, mut start) = (0i32, None::<char>, 0);
    let mut escaped = false;
    for (i, c) in text.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match (quote, c) {
            (_, '\\') => escaped = true,
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '(' | '[') => depth += 1,
            (None, ')' | ']') => depth -= 1,
            (None, c) if c == sep && depth <= 0 => {
                parts.push(&text[start..i]);
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(&text[start..]);
    parts
}

/// `(name, value, important)` declarations of a style attribute, in order.
pub(crate) fn parse(text: &str) -> Vec<(String, String, bool)> {
    split_top_level(text, ';')
        .into_iter()
        .filter_map(|decl| {
            let (name, value) = decl.split_once(':')?;
            let name = name.trim();
            let name = if name.starts_with("--") {
                name.to_owned()
            } else {
                name.to_ascii_lowercase()
            };
            let mut value = value.trim();
            let mut important = false;
            if let Some(v) = value.strip_suffix("!important") {
                value = v.trim_end();
                important = true;
            }
            (!name.is_empty() && !value.is_empty()).then(|| (name, value.to_owned(), important))
        })
        .collect()
}

/// Serializes declarations like browsers do: `a: b; c: d !important;`.
pub(crate) fn serialize(decls: &[(String, String, bool)]) -> String {
    decls
        .iter()
        .map(|(n, v, imp)| {
            if *imp {
                format!("{n}: {v} !important;")
            } else {
                format!("{n}: {v};")
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A value scripts may assign: non-empty pieces that cannot break out of the
/// declaration. Invalid assignments are ignored, as in browsers.
pub(crate) fn acceptable_value(value: &str) -> bool {
    split_top_level(value, ';').len() == 1 && !value.contains(['{', '}'])
}

/// Sets (`Some`) or removes (`None` or empty) `name` in `text`.
pub(crate) fn set(text: &str, name: &str, value: Option<&str>, important: bool) -> String {
    let mut decls = parse(text);
    let value = value.map(str::trim).filter(|v| !v.is_empty());
    match (decls.iter().position(|(n, _, _)| n == name), value) {
        (Some(i), Some(v)) => decls[i] = (name.to_owned(), v.to_owned(), important),
        (None, Some(v)) => decls.push((name.to_owned(), v.to_owned(), important)),
        (Some(i), None) => {
            decls.remove(i);
        }
        (None, None) => {}
    }
    serialize(&decls)
}

pub(crate) fn get(text: &str, name: &str) -> String {
    parse(text)
        .into_iter()
        .rev()
        .find(|(n, _, _)| n == name)
        .map(|(_, v, _)| v)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_set_and_serialize() {
        let text = "color: red; background: url(a;b.png) ; WIDTH:10px !important;";
        let decls = parse(text);
        assert_eq!(decls.len(), 3);
        assert_eq!(decls[1].1, "url(a;b.png)");
        assert_eq!(get(text, "width"), "10px");
        assert_eq!(
            set(text, "color", Some("blue"), false),
            "color: blue; background: url(a;b.png); width: 10px !important;"
        );
        assert_eq!(set("color: red", "color", None, false), "");
        assert_eq!(
            set("", "margin-top", Some(" 4px "), false),
            "margin-top: 4px;"
        );
        assert_eq!(camel_case("border-top-left-radius"), "borderTopLeftRadius");
    }

    #[test]
    fn injection_is_rejected() {
        assert!(acceptable_value("red"));
        assert!(acceptable_value("url('a;b')"));
        assert!(!acceptable_value("red; display: none"));
        assert!(!acceptable_value("red } p { color: blue"));
    }
}

//! Resolved-value serialization in the format browsers return from
//! `getComputedStyle()`, used for differential testing.
//!
//! Only properties whose resolved value does not depend on layout are
//! supported (`width`/`height` and percentage/`auto` margins resolve to used
//! values in browsers and are reported here as computed values).

use rustlite_dom::{Document, NodeData};

use crate::computed::{ComputedLength, ComputedLengthAuto, ComputedLineHeight, ComputedStyle};
use crate::properties::FamilyName;
use crate::stylist::ComputedStyles;
use crate::values::Rgba;

/// Formats a number like Blink: at most six significant digits, no trailing zeros.
pub fn format_number(v: f32) -> String {
    let v = f64::from(v);
    if v == v.trunc() && v.abs() < 1e15 {
        return format!("{}", v as i64);
    }
    let magnitude = v.abs().log10().floor() as i32;
    let decimals = (5 - magnitude).clamp(0, 15) as usize;
    let s = format!("{v:.decimals$}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

fn px(v: f32) -> String {
    format!("{}px", format_number(v))
}

/// CSSOM "serialize a CSS component value" for alpha: the shortest of two or
/// three decimal places that maps back to the same 8-bit alpha.
fn serialize_alpha(a: f32) -> f32 {
    let a8 = (a * 255.0).round();
    let two = (a8 / 255.0 * 100.0).round() / 100.0;
    if (two * 255.0).round() == a8 {
        two
    } else {
        (a8 / 255.0 * 1000.0).round() / 1000.0
    }
}

pub fn format_color(c: Rgba) -> String {
    if c.a >= 1.0 {
        format!("rgb({}, {}, {})", c.r, c.g, c.b)
    } else {
        format!(
            "rgba({}, {}, {}, {})",
            c.r,
            c.g,
            c.b,
            format_number(serialize_alpha(c.a))
        )
    }
}

fn length(l: ComputedLength) -> String {
    match l {
        ComputedLength::Px(v) => px(v),
        ComputedLength::Percentage(p) => format!("{}%", format_number(p)),
    }
}

fn length_auto(l: ComputedLengthAuto) -> String {
    match l {
        ComputedLengthAuto::Length(l) => length(l),
        ComputedLengthAuto::Auto => "auto".into(),
    }
}

/// `true` if `s` is a single CSS identifier that needs no escaping.
fn is_identifier(s: &str) -> bool {
    let start = |c: char| c.is_ascii_alphabetic() || c == '_' || !c.is_ascii();
    let rest = s.strip_prefix('-').unwrap_or(s);
    let mut chars = rest.chars();
    chars.next().is_some_and(start) && chars.all(|c| start(c) || c.is_ascii_digit() || c == '-')
}

fn family(f: &FamilyName) -> String {
    match f {
        FamilyName::Generic(g) => g.clone(),
        FamilyName::Named(n) => {
            // Quote anything that is not a single plain identifier, and names
            // that would otherwise read as generic or CSS-wide keywords.
            let keyword = crate::properties::GENERIC_FAMILIES
                .contains(&n.to_ascii_lowercase().as_str())
                || ["inherit", "initial", "unset", "default", "revert"]
                    .contains(&n.to_ascii_lowercase().as_str());
            if is_identifier(n) && !keyword {
                n.clone()
            } else {
                format!("\"{}\"", n.replace('\\', "\\\\").replace('"', "\\\""))
            }
        }
    }
}

/// The resolved value of `property` for `style`, or `None` if unsupported.
pub fn resolved_value(style: &ComputedStyle, property: &str) -> Option<String> {
    let side = |name: &str| match name {
        "top" => Some(0),
        "right" => Some(1),
        "bottom" => Some(2),
        "left" => Some(3),
        _ => None,
    };
    Some(match property {
        "display" => style.display.as_css().into(),
        "color" => format_color(style.color),
        "background-color" => format_color(style.background_color),
        "font-size" => px(style.font_size),
        "font-weight" => style.font_weight.to_string(),
        "font-style" => style.font_style.as_css().into(),
        "font-family" => style
            .font_family
            .iter()
            .map(family)
            .collect::<Vec<_>>()
            .join(", "),
        "line-height" => match style.line_height {
            ComputedLineHeight::Normal => "normal".into(),
            ComputedLineHeight::Number(n) => px(n * style.font_size),
            ComputedLineHeight::Px(v) => px(v),
        },
        "text-align" => style.text_align.as_css().into(),
        "list-style-type" => style.list_style_type.as_css().into(),
        "overflow-x" => style.overflow_x.as_css().into(),
        "overflow-y" => style.overflow_y.as_css().into(),
        "width" => length_auto(style.width),
        "height" => length_auto(style.height),
        _ => {
            if let Some(s) = property.strip_prefix("margin-") {
                length_auto(style.margin[side(s)?])
            } else if let Some(s) = property.strip_prefix("padding-") {
                length(style.padding[side(s)?])
            } else if let Some(rest) = property.strip_prefix("border-") {
                let (s, part) = rest.split_once('-')?;
                let i = side(s)?;
                match part {
                    "width" => px(style.border_width[i]),
                    "style" => style.border_style[i].as_css().into(),
                    "color" => format_color(style.border_color[i]),
                    _ => return None,
                }
            } else {
                return None;
            }
        }
    })
}

/// Default property set compared against the reference browser.
pub const DEFAULT_DUMP_PROPERTIES: &[&str] = &[
    "display",
    "color",
    "background-color",
    "font-size",
    "font-weight",
    "font-style",
    "line-height",
    "text-align",
    "overflow-x",
    "overflow-y",
    "margin-top",
    "margin-right",
    "margin-bottom",
    "margin-left",
    "padding-top",
    "padding-right",
    "padding-bottom",
    "padding-left",
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

/// Dumps `properties` for every element in tree order:
///
/// ```text
/// <p#intro>
///   color: rgb(0, 0, 0)
/// ```
pub fn dump_styles(doc: &Document, styles: &ComputedStyles, properties: &[&str]) -> String {
    let mut out = String::new();
    for n in doc.descendants(doc.root()) {
        let NodeData::Element(e) = doc.data(n) else {
            continue;
        };
        out.push('<');
        out.push_str(&e.name);
        if let Some(id) = e.attribute("id") {
            out.push('#');
            out.push_str(id);
        }
        out.push_str(">\n");
        let Some(style) = styles.get(n) else {
            continue;
        };
        for p in properties {
            let value = resolved_value(style, p).unwrap_or_else(|| "<unsupported>".into());
            out.push_str(&format!("  {p}: {value}\n"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_formatting() {
        assert_eq!(format_number(16.0), "16");
        assert_eq!(format_number(13.333_333), "13.3333");
        assert_eq!(format_number(18.72), "18.72");
        assert_eq!(format_number(22.1776), "22.1776");
        assert_eq!(format_number(0.5), "0.5");
        assert_eq!(format_number(-1.5), "-1.5");
        assert_eq!(format_number(106.666_67), "106.667");
    }

    #[test]
    fn color_formatting() {
        assert_eq!(format_color(Rgba::rgb(1, 2, 3)), "rgb(1, 2, 3)");
        assert_eq!(
            format_color(Rgba {
                r: 0,
                g: 0,
                b: 0,
                a: 0.0
            }),
            "rgba(0, 0, 0, 0)"
        );
        assert_eq!(
            format_color(Rgba {
                r: 0,
                g: 0,
                b: 0,
                a: 0.3
            }),
            "rgba(0, 0, 0, 0.3)"
        );
        // #ff000080: 128/255 = 0.50196… serializes as 0.5 (round-trips to 128).
        assert_eq!(
            format_color(Rgba {
                r: 255,
                g: 0,
                b: 0,
                a: 128.0 / 255.0
            }),
            "rgba(255, 0, 0, 0.5)"
        );
        // Three decimals when two would not round-trip: 1/255 → 0.00 → 0, so 0.004.
        assert_eq!(serialize_alpha(1.0 / 255.0), 0.004);
    }

    #[test]
    fn family_quoting() {
        assert_eq!(family(&FamilyName::Named("Arial".into())), "Arial");
        assert_eq!(
            family(&FamilyName::Named("Times New Roman".into())),
            "\"Times New Roman\""
        );
        assert_eq!(family(&FamilyName::Generic("serif".into())), "serif");
        assert_eq!(family(&FamilyName::Named("serif".into())), "\"serif\"");
    }
}

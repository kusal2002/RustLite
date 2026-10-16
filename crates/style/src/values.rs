//! Specified value types and their parsers.

use rustlite_css::{ComponentValue, Token};

use crate::colors::named_color;

/// An sRGB color. Channels are 8-bit; alpha is a float in `0.0..=1.0`, kept
/// at full precision so `rgba(0, 0, 0, 0.3)` round-trips exactly.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: f32,
}

impl Rgba {
    pub const BLACK: Rgba = Rgba::rgb(0, 0, 0);
    pub const TRANSPARENT: Rgba = Rgba {
        r: 0,
        g: 0,
        b: 0,
        a: 0.0,
    };

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 1.0 }
    }

    fn from_hex24(v: u32) -> Self {
        Self::rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ColorValue {
    Rgba(Rgba),
    CurrentColor,
}

/// Font-relative and viewport-relative units are kept; absolute units are
/// converted to `px` at parse time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LengthUnit {
    Px,
    Em,
    Rem,
    Vw,
    Vh,
    Vmin,
    Vmax,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Length {
    pub value: f32,
    pub unit: LengthUnit,
}

impl Length {
    pub const ZERO: Length = Length {
        value: 0.0,
        unit: LengthUnit::Px,
    };

    pub const fn px(value: f32) -> Self {
        Self {
            value,
            unit: LengthUnit::Px,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LengthPercentage {
    Length(Length),
    /// Percentage, e.g. `50.0` for `50%`.
    Percentage(f32),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LengthPercentageAuto {
    LengthPercentage(LengthPercentage),
    Auto,
}

/// Non-whitespace component values of a declaration value.
pub(crate) fn items(value: &[ComponentValue]) -> Vec<&ComponentValue> {
    value.iter().filter(|v| !v.is_whitespace()).collect()
}

pub(crate) fn single(value: &[ComponentValue]) -> Option<&ComponentValue> {
    match items(value).as_slice() {
        [one] => Some(one),
        _ => None,
    }
}

pub(crate) fn ident(cv: &ComponentValue) -> Option<&str> {
    match cv {
        ComponentValue::Token(Token::Ident(s)) => Some(s),
        _ => None,
    }
}

pub(crate) fn ident_lower(cv: &ComponentValue) -> Option<String> {
    ident(cv).map(str::to_ascii_lowercase)
}

fn finite(v: f64) -> Option<f32> {
    let v = v as f32;
    v.is_finite().then_some(v)
}

pub(crate) fn parse_length(cv: &ComponentValue, allow_negative: bool) -> Option<Length> {
    let length = match cv {
        ComponentValue::Token(Token::Number { value, .. }) if *value == 0.0 => Length::ZERO,
        ComponentValue::Token(Token::Dimension { value, unit }) => {
            let v = finite(*value)?;
            let (factor, unit) = match unit.to_ascii_lowercase().as_str() {
                "px" => (1.0, LengthUnit::Px),
                "em" => (1.0, LengthUnit::Em),
                "rem" => (1.0, LengthUnit::Rem),
                "vw" => (1.0, LengthUnit::Vw),
                "vh" => (1.0, LengthUnit::Vh),
                "vmin" => (1.0, LengthUnit::Vmin),
                "vmax" => (1.0, LengthUnit::Vmax),
                "in" => (96.0, LengthUnit::Px),
                "cm" => (96.0 / 2.54, LengthUnit::Px),
                "mm" => (96.0 / 25.4, LengthUnit::Px),
                "q" => (96.0 / 101.6, LengthUnit::Px),
                "pt" => (96.0 / 72.0, LengthUnit::Px),
                "pc" => (16.0, LengthUnit::Px),
                _ => return None,
            };
            Length {
                value: v * factor,
                unit,
            }
        }
        _ => return None,
    };
    (allow_negative || length.value >= 0.0).then_some(length)
}

pub(crate) fn parse_length_percentage(
    cv: &ComponentValue,
    allow_negative: bool,
) -> Option<LengthPercentage> {
    if let ComponentValue::Token(Token::Percentage(p)) = cv {
        let p = finite(*p)?;
        return (allow_negative || p >= 0.0).then_some(LengthPercentage::Percentage(p));
    }
    parse_length(cv, allow_negative).map(LengthPercentage::Length)
}

pub(crate) fn parse_lpa(cv: &ComponentValue, allow_negative: bool) -> Option<LengthPercentageAuto> {
    if ident(cv).is_some_and(|s| s.eq_ignore_ascii_case("auto")) {
        return Some(LengthPercentageAuto::Auto);
    }
    parse_length_percentage(cv, allow_negative).map(LengthPercentageAuto::LengthPercentage)
}

// ----- colors ------------------------------------------------------------------

pub(crate) fn parse_color(cv: &ComponentValue) -> Option<ColorValue> {
    match cv {
        ComponentValue::Token(Token::Ident(name)) => {
            let lower = name.to_ascii_lowercase();
            match lower.as_str() {
                "currentcolor" => Some(ColorValue::CurrentColor),
                "transparent" => Some(ColorValue::Rgba(Rgba::TRANSPARENT)),
                _ => named_color(&lower).map(|v| ColorValue::Rgba(Rgba::from_hex24(v))),
            }
        }
        ComponentValue::Token(Token::Hash { value, .. }) => parse_hex(value).map(ColorValue::Rgba),
        ComponentValue::Function { name, args } => match name.to_ascii_lowercase().as_str() {
            "rgb" | "rgba" => parse_rgb_function(args),
            "hsl" | "hsla" => parse_hsl_function(args),
            _ => None,
        }
        .map(ColorValue::Rgba),
        _ => None,
    }
}

fn parse_hex(hex: &str) -> Option<Rgba> {
    if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let digit = |i: usize| u8::from_str_radix(&hex[i..=i], 16).ok();
    let pair = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    let (r, g, b, a) = match hex.len() {
        3 | 4 => {
            let d = |i| digit(i).map(|v| v * 17);
            let a = if hex.len() == 4 { d(3)? } else { 255 };
            (d(0)?, d(1)?, d(2)?, a)
        }
        6 | 8 => {
            let a = if hex.len() == 8 { pair(6)? } else { 255 };
            (pair(0)?, pair(2)?, pair(4)?, a)
        }
        _ => return None,
    };
    Some(Rgba {
        r,
        g,
        b,
        a: f32::from(a) / 255.0,
    })
}

/// Splits color-function arguments into (channels, alpha), accepting both the
/// legacy comma syntax and the modern space/slash syntax.
fn function_args(
    args: &[ComponentValue],
) -> Option<(Vec<&ComponentValue>, Option<&ComponentValue>)> {
    let items = items(args);
    let is_comma = |cv: &&ComponentValue| matches!(cv, ComponentValue::Token(Token::Comma));
    if items.iter().any(is_comma) {
        let parts: Vec<_> = items.split(is_comma).collect();
        if !(3..=4).contains(&parts.len()) || parts.iter().any(|p| p.len() != 1) {
            return None;
        }
        let channels = parts[..3].iter().map(|p| p[0]).collect();
        return Some((channels, parts.get(3).map(|p| p[0])));
    }
    let slash = items
        .iter()
        .position(|cv| matches!(cv, ComponentValue::Token(Token::Delim('/'))));
    match slash {
        Some(i) if i == 3 && items.len() == 5 => Some((items[..3].to_vec(), Some(items[4]))),
        None if items.len() == 3 => Some((items, None)),
        _ => None,
    }
}

fn parse_alpha(cv: Option<&ComponentValue>) -> Option<f32> {
    match cv {
        None => Some(1.0),
        Some(ComponentValue::Token(Token::Number { value, .. })) => {
            Some(finite(*value)?.clamp(0.0, 1.0))
        }
        Some(ComponentValue::Token(Token::Percentage(p))) => {
            Some((finite(*p)? / 100.0).clamp(0.0, 1.0))
        }
        _ => None,
    }
}

fn round_channel(v: f64) -> u8 {
    // Clamped to 0..=255 first, so the cast cannot truncate.
    v.clamp(0.0, 255.0).round() as u8
}

fn parse_rgb_function(args: &[ComponentValue]) -> Option<Rgba> {
    let (channels, alpha) = function_args(args)?;
    let mut rgb = [0u8; 3];
    for (out, cv) in rgb.iter_mut().zip(channels) {
        *out = match cv {
            ComponentValue::Token(Token::Number { value, .. }) => round_channel(*value),
            // `p * 255 / 100`, not `p * 2.55`: the latter is inexact (50% → 127.4999…).
            ComponentValue::Token(Token::Percentage(p)) => round_channel(p * 255.0 / 100.0),
            _ => return None,
        };
    }
    Some(Rgba {
        r: rgb[0],
        g: rgb[1],
        b: rgb[2],
        a: parse_alpha(alpha)?,
    })
}

fn parse_hsl_function(args: &[ComponentValue]) -> Option<Rgba> {
    let (channels, alpha) = function_args(args)?;
    let hue_degrees = match channels[0] {
        ComponentValue::Token(Token::Number { value, .. }) => *value,
        ComponentValue::Token(Token::Dimension { value, unit }) => {
            match unit.to_ascii_lowercase().as_str() {
                "deg" => *value,
                "rad" => value.to_degrees(),
                "grad" => value * 0.9,
                "turn" => value * 360.0,
                _ => return None,
            }
        }
        _ => return None,
    };
    let fraction = |cv: &ComponentValue| match cv {
        ComponentValue::Token(Token::Percentage(p)) => Some((p / 100.0).clamp(0.0, 1.0)),
        ComponentValue::Token(Token::Number { value, .. }) => Some((value / 100.0).clamp(0.0, 1.0)),
        _ => None,
    };
    let (s, l) = (fraction(channels[1])?, fraction(channels[2])?);
    let h = hue_degrees.rem_euclid(360.0);
    // CSS Color 4 §7.1 hslToRgb.
    let f = |n: f64| {
        let k = (n + h / 30.0) % 12.0;
        let a = s * l.min(1.0 - l);
        l - a * (k - 3.0).min(9.0 - k).clamp(-1.0, 1.0)
    };
    Some(Rgba {
        r: round_channel(f(0.0) * 255.0),
        g: round_channel(f(8.0) * 255.0),
        b: round_channel(f(4.0) * 255.0),
        a: parse_alpha(alpha)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn color(s: &str) -> Option<ColorValue> {
        let decls = rustlite_css::parse_declaration_list(&format!("x: {s}"));
        single(&decls[0].value).and_then(parse_color)
    }

    fn rgba(r: u8, g: u8, b: u8, a: f32) -> Option<ColorValue> {
        Some(ColorValue::Rgba(Rgba { r, g, b, a }))
    }

    #[test]
    fn color_syntaxes() {
        assert_eq!(color("red"), rgba(255, 0, 0, 1.0));
        assert_eq!(color("#0f8"), rgba(0, 255, 136, 1.0));
        assert_eq!(
            color("#11223344"),
            rgba(0x11, 0x22, 0x33, 0x44 as f32 / 255.0)
        );
        assert_eq!(color("rgb(10, 20, 30)"), rgba(10, 20, 30, 1.0));
        assert_eq!(color("rgba(10,20,30,0.5)"), rgba(10, 20, 30, 0.5));
        assert_eq!(color("rgb(10 20 30 / 25%)"), rgba(10, 20, 30, 0.25));
        assert_eq!(color("rgb(100%, 0%, 50%)"), rgba(255, 0, 128, 1.0));
        assert_eq!(color("rgb(300, -5, 0)"), rgba(255, 0, 0, 1.0));
        assert_eq!(color("hsl(120, 100%, 50%)"), rgba(0, 255, 0, 1.0));
        assert_eq!(color("hsl(0.5turn 100% 50%)"), rgba(0, 255, 255, 1.0));
        assert_eq!(color("transparent"), rgba(0, 0, 0, 0.0));
        assert_eq!(color("CurrentColor"), Some(ColorValue::CurrentColor));
    }

    #[test]
    fn invalid_colors() {
        for bad in [
            "#12",
            "#ggg",
            "rgb(1,2)",
            "rgb(1 2, 3)",
            "bogus",
            "rgb(1 2 3 4)",
            "12px",
        ] {
            assert_eq!(color(bad), None, "{bad}");
        }
    }

    #[test]
    fn lengths() {
        let decls = rustlite_css::parse_declaration_list("x: 1in; y: -2em; z: 0; w: 5foo; v: 3");
        let l = |i: usize, neg| single(&decls[i].value).and_then(|cv| parse_length(cv, neg));
        assert_eq!(l(0, false), Some(Length::px(96.0)));
        assert_eq!(
            l(1, true),
            Some(Length {
                value: -2.0,
                unit: LengthUnit::Em
            })
        );
        assert_eq!(l(1, false), None);
        assert_eq!(l(2, false), Some(Length::ZERO));
        assert_eq!(l(3, false), None);
        assert_eq!(l(4, false), None, "unitless non-zero lengths are invalid");
    }
}

//! Supported properties: longhand list, keyword types, declaration parsing,
//! and shorthand expansion.
//!
//! Unsupported properties and invalid values are dropped, exactly as a
//! browser drops declarations it does not understand.

use std::rc::Rc;

use rustlite_css::{ComponentValue, Declaration, Token};

use crate::values::{
    ColorValue, Length, LengthPercentage, LengthPercentageAuto, ident, ident_lower, items,
    parse_color, parse_length, parse_length_percentage, parse_lpa, single,
};

macro_rules! keyword_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident = $css:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum $name { $($variant),+ }

        impl $name {
            pub fn from_ident(s: &str) -> Option<Self> {
                match s.to_ascii_lowercase().as_str() {
                    $($css => Some(Self::$variant),)+
                    _ => None,
                }
            }

            pub fn as_css(self) -> &'static str {
                match self { $(Self::$variant => $css),+ }
            }
        }
    };
}

keyword_enum!(Display {
    None = "none",
    Block = "block",
    Inline = "inline",
    InlineBlock = "inline-block",
    ListItem = "list-item",
    Flex = "flex",
    InlineFlex = "inline-flex",
    Grid = "grid",
    InlineGrid = "inline-grid",
    Table = "table",
    InlineTable = "inline-table",
    TableRowGroup = "table-row-group",
    TableHeaderGroup = "table-header-group",
    TableFooterGroup = "table-footer-group",
    TableRow = "table-row",
    TableCell = "table-cell",
    TableColumnGroup = "table-column-group",
    TableColumn = "table-column",
    TableCaption = "table-caption",
    FlowRoot = "flow-root",
    Contents = "contents",
});

impl Display {
    /// Blockification (CSS Display 3 §2.7), used for the root element.
    pub fn blockified(self) -> Self {
        match self {
            Display::Inline | Display::InlineBlock | Display::Contents => Display::Block,
            Display::InlineFlex => Display::Flex,
            Display::InlineGrid => Display::Grid,
            Display::InlineTable => Display::Table,
            other => other,
        }
    }
}

keyword_enum!(BorderStyle {
    None = "none",
    Hidden = "hidden",
    Dotted = "dotted",
    Dashed = "dashed",
    Solid = "solid",
    Double = "double",
    Groove = "groove",
    Ridge = "ridge",
    Inset = "inset",
    Outset = "outset",
});

keyword_enum!(FontStyle {
    Normal = "normal",
    Italic = "italic",
    Oblique = "oblique",
});

keyword_enum!(TextAlign {
    Start = "start",
    End = "end",
    Left = "left",
    Right = "right",
    Center = "center",
    Justify = "justify",
});

keyword_enum!(ListStyleType {
    None = "none",
    Disc = "disc",
    Circle = "circle",
    Square = "square",
    Decimal = "decimal",
    LowerAlpha = "lower-alpha",
    UpperAlpha = "upper-alpha",
    LowerLatin = "lower-latin",
    UpperLatin = "upper-latin",
    LowerRoman = "lower-roman",
    UpperRoman = "upper-roman",
});

keyword_enum!(Overflow {
    Visible = "visible",
    Hidden = "hidden",
    Clip = "clip",
    Scroll = "scroll",
    Auto = "auto",
});

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BorderWidth {
    Thin,
    Medium,
    Thick,
    Length(Length),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FontSize {
    /// Absolute keyword size in px (`medium` = 16).
    Keyword(f32),
    Smaller,
    Larger,
    LengthPercentage(LengthPercentage),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FontWeight {
    Absolute(u16),
    Bolder,
    Lighter,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FamilyName {
    Generic(String),
    Named(String),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LineHeight {
    Normal,
    Number(f32),
    LengthPercentage(LengthPercentage),
}

/// Every supported longhand. The discriminant indexes cascade arrays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Longhand {
    Display,
    Color,
    BackgroundColor,
    FontSize,
    FontWeight,
    FontStyle,
    FontFamily,
    LineHeight,
    TextAlign,
    ListStyleType,
    OverflowX,
    OverflowY,
    Width,
    Height,
    MinWidth,
    MaxWidth,
    MinHeight,
    MaxHeight,
    MarginTop,
    MarginRight,
    MarginBottom,
    MarginLeft,
    PaddingTop,
    PaddingRight,
    PaddingBottom,
    PaddingLeft,
    BorderTopWidth,
    BorderRightWidth,
    BorderBottomWidth,
    BorderLeftWidth,
    BorderTopStyle,
    BorderRightStyle,
    BorderBottomStyle,
    BorderLeftStyle,
    BorderTopColor,
    BorderRightColor,
    BorderBottomColor,
    BorderLeftColor,
}

pub const LONGHAND_COUNT: usize = Longhand::BorderLeftColor as usize + 1;

const MARGINS: [Longhand; 4] = [
    Longhand::MarginTop,
    Longhand::MarginRight,
    Longhand::MarginBottom,
    Longhand::MarginLeft,
];
const PADDINGS: [Longhand; 4] = [
    Longhand::PaddingTop,
    Longhand::PaddingRight,
    Longhand::PaddingBottom,
    Longhand::PaddingLeft,
];
const BORDER_WIDTHS: [Longhand; 4] = [
    Longhand::BorderTopWidth,
    Longhand::BorderRightWidth,
    Longhand::BorderBottomWidth,
    Longhand::BorderLeftWidth,
];
const BORDER_STYLES: [Longhand; 4] = [
    Longhand::BorderTopStyle,
    Longhand::BorderRightStyle,
    Longhand::BorderBottomStyle,
    Longhand::BorderLeftStyle,
];
const BORDER_COLORS: [Longhand; 4] = [
    Longhand::BorderTopColor,
    Longhand::BorderRightColor,
    Longhand::BorderBottomColor,
    Longhand::BorderLeftColor,
];

impl Longhand {
    pub fn is_inherited(self) -> bool {
        matches!(
            self,
            Longhand::Color
                | Longhand::FontSize
                | Longhand::FontWeight
                | Longhand::FontStyle
                | Longhand::FontFamily
                | Longhand::LineHeight
                | Longhand::TextAlign
                | Longhand::ListStyleType
        )
    }
}

/// A declared value for one longhand.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Inherit,
    Initial,
    Unset,
    Display(Display),
    Color(ColorValue),
    LengthPercentageAuto(LengthPercentageAuto),
    LengthPercentage(LengthPercentage),
    BorderWidth(BorderWidth),
    BorderStyle(BorderStyle),
    FontSize(FontSize),
    FontWeight(FontWeight),
    FontStyle(FontStyle),
    FontFamily(Rc<[FamilyName]>),
    LineHeight(LineHeight),
    /// `max-width`/`max-height`: `None` is the `none` keyword.
    MaxSize(Option<LengthPercentage>),
    TextAlign(TextAlign),
    ListStyleType(ListStyleType),
    Overflow(Overflow),
}

#[derive(Debug, Clone, PartialEq)]
pub struct PropertyDeclaration {
    pub property: Longhand,
    pub value: Value,
    pub important: bool,
}

fn css_wide(value: &[ComponentValue]) -> Option<Value> {
    match ident_lower(single(value)?)?.as_str() {
        "inherit" => Some(Value::Inherit),
        "initial" => Some(Value::Initial),
        // `revert`/`revert-layer` roll back to the user-agent level, which is
        // close to `unset` for author styles; documented approximation.
        "unset" | "revert" | "revert-layer" => Some(Value::Unset),
        _ => None,
    }
}

/// Expands 1–4 values to (top, right, bottom, left).
fn four_sides<T: Clone>(values: &[T]) -> Option<[T; 4]> {
    let (top, right, bottom, left) = match values {
        [a] => (a, a, a, a),
        [a, b] => (a, b, a, b),
        [a, b, c] => (a, b, c, b),
        [a, b, c, d] => (a, b, c, d),
        _ => return None,
    };
    Some([top.clone(), right.clone(), bottom.clone(), left.clone()])
}

fn parse_each<T>(
    value: &[ComponentValue],
    f: impl Fn(&ComponentValue) -> Option<T>,
) -> Option<Vec<T>> {
    items(value).into_iter().map(f).collect()
}

fn parse_border_width(cv: &ComponentValue) -> Option<BorderWidth> {
    match ident_lower(cv).as_deref() {
        Some("thin") => Some(BorderWidth::Thin),
        Some("medium") => Some(BorderWidth::Medium),
        Some("thick") => Some(BorderWidth::Thick),
        _ => parse_length(cv, false).map(BorderWidth::Length),
    }
}

fn parse_border_style(cv: &ComponentValue) -> Option<BorderStyle> {
    BorderStyle::from_ident(ident(cv)?)
}

fn parse_font_size(cv: &ComponentValue) -> Option<FontSize> {
    if let Some(word) = ident_lower(cv) {
        let px = match word.as_str() {
            "xx-small" => 9.0,
            "x-small" => 10.0,
            "small" => 13.0,
            "medium" => 16.0,
            "large" => 18.0,
            "x-large" => 24.0,
            "xx-large" => 32.0,
            "xxx-large" => 48.0,
            "smaller" => return Some(FontSize::Smaller),
            "larger" => return Some(FontSize::Larger),
            _ => return None,
        };
        return Some(FontSize::Keyword(px));
    }
    parse_length_percentage(cv, false).map(FontSize::LengthPercentage)
}

fn parse_font_weight(cv: &ComponentValue) -> Option<FontWeight> {
    match cv {
        ComponentValue::Token(Token::Number { value, .. }) if (1.0..=1000.0).contains(value) => {
            // In range 1..=1000, so the cast is exact after rounding.
            Some(FontWeight::Absolute(value.round() as u16))
        }
        _ => match ident_lower(cv)?.as_str() {
            "normal" => Some(FontWeight::Absolute(400)),
            "bold" => Some(FontWeight::Absolute(700)),
            "bolder" => Some(FontWeight::Bolder),
            "lighter" => Some(FontWeight::Lighter),
            _ => None,
        },
    }
}

pub(crate) const GENERIC_FAMILIES: &[&str] = &[
    "serif",
    "sans-serif",
    "monospace",
    "cursive",
    "fantasy",
    "system-ui",
    "ui-serif",
    "ui-sans-serif",
    "ui-monospace",
    "ui-rounded",
    "math",
    "emoji",
    "fangsong",
];

fn parse_font_family(value: &[ComponentValue]) -> Option<Rc<[FamilyName]>> {
    let mut families = Vec::new();
    for part in value.split(|cv| matches!(cv, ComponentValue::Token(Token::Comma))) {
        let words = items(part);
        let family = match words.as_slice() {
            [ComponentValue::Token(Token::String(s))] => FamilyName::Named(s.clone()),
            [one]
                if ident(one).is_some_and(|w| {
                    GENERIC_FAMILIES.contains(&w.to_ascii_lowercase().as_str())
                }) =>
            {
                FamilyName::Generic(ident_lower(one)?)
            }
            [] => return None,
            words => {
                let names: Option<Vec<&str>> = words.iter().map(|w| ident(w)).collect();
                FamilyName::Named(names?.join(" "))
            }
        };
        families.push(family);
    }
    Some(families.into())
}

/// `font: [style || weight || variant || stretch]? size [/ line-height]? family`
/// (CSS Fonts 4 §2.8). Variant and stretch are accepted and ignored; system
/// font keywords (`caption`, `menu`, ...) are not supported.
fn parse_font_shorthand(value: &[ComponentValue]) -> Option<Vec<(Longhand, Value)>> {
    let mut style = FontStyle::Normal;
    let mut weight = FontWeight::Absolute(400);
    let mut i = 0;
    let skip_ws = |i: &mut usize| {
        while value.get(*i).is_some_and(ComponentValue::is_whitespace) {
            *i += 1;
        }
    };
    // Prefix keywords, at most one of each kind.
    let (mut seen_style, mut seen_weight) = (false, false);
    loop {
        skip_ws(&mut i);
        let cv = value.get(i)?;
        let word = ident_lower(cv).unwrap_or_default();
        if word == "normal" {
            i += 1;
        } else if !seen_style && let Some(s) = FontStyle::from_ident(&word) {
            style = s;
            seen_style = true;
            i += 1;
        } else if !seen_weight && let Some(w) = parse_font_weight(cv).filter(|_| word != "normal") {
            weight = w;
            seen_weight = true;
            i += 1;
        } else if matches!(
            word.as_str(),
            "small-caps"
                | "condensed"
                | "semi-condensed"
                | "expanded"
                | "semi-expanded"
                | "extra-condensed"
                | "extra-expanded"
                | "ultra-condensed"
                | "ultra-expanded"
        ) {
            i += 1;
        } else {
            break;
        }
    }
    let size = parse_font_size(value.get(i)?)?;
    i += 1;
    skip_ws(&mut i);
    let mut line_height = LineHeight::Normal;
    if matches!(value.get(i), Some(ComponentValue::Token(Token::Delim('/')))) {
        i += 1;
        skip_ws(&mut i);
        line_height = parse_line_height(value.get(i)?)?;
        i += 1;
    }
    let family = parse_font_family(&value[i..])?;
    Some(vec![
        (Longhand::FontStyle, Value::FontStyle(style)),
        (Longhand::FontWeight, Value::FontWeight(weight)),
        (Longhand::FontSize, Value::FontSize(size)),
        (Longhand::LineHeight, Value::LineHeight(line_height)),
        (Longhand::FontFamily, Value::FontFamily(family)),
    ])
}

fn parse_line_height(cv: &ComponentValue) -> Option<LineHeight> {
    match cv {
        ComponentValue::Token(Token::Number { value, .. }) if *value >= 0.0 => {
            Some(LineHeight::Number(*value as f32))
        }
        _ if ident(cv).is_some_and(|s| s.eq_ignore_ascii_case("normal")) => {
            Some(LineHeight::Normal)
        }
        _ => parse_length_percentage(cv, false).map(LineHeight::LengthPercentage),
    }
}

/// Parses one longhand's value (without CSS-wide keywords).
fn parse_longhand(property: Longhand, value: &[ComponentValue]) -> Option<Value> {
    use Longhand as L;
    if let Some(v) = css_wide(value) {
        return Some(v);
    }
    if property == L::FontFamily {
        return parse_font_family(value).map(Value::FontFamily);
    }
    let cv = single(value)?;
    Some(match property {
        L::Display => Value::Display(Display::from_ident(ident(cv)?)?),
        L::Color
        | L::BackgroundColor
        | L::BorderTopColor
        | L::BorderRightColor
        | L::BorderBottomColor
        | L::BorderLeftColor => Value::Color(parse_color(cv)?),
        L::FontSize => Value::FontSize(parse_font_size(cv)?),
        L::FontWeight => Value::FontWeight(parse_font_weight(cv)?),
        L::FontStyle => Value::FontStyle(FontStyle::from_ident(ident(cv)?)?),
        L::LineHeight => Value::LineHeight(parse_line_height(cv)?),
        L::TextAlign => Value::TextAlign(TextAlign::from_ident(ident(cv)?)?),
        L::ListStyleType => Value::ListStyleType(ListStyleType::from_ident(ident(cv)?)?),
        L::OverflowX | L::OverflowY => Value::Overflow(Overflow::from_ident(ident(cv)?)?),
        L::Width | L::Height | L::MinWidth | L::MinHeight => {
            Value::LengthPercentageAuto(parse_lpa(cv, false)?)
        }
        L::MaxWidth | L::MaxHeight => {
            if ident(cv).is_some_and(|s| s.eq_ignore_ascii_case("none")) {
                Value::MaxSize(None)
            } else {
                Value::MaxSize(Some(parse_length_percentage(cv, false)?))
            }
        }
        L::MarginTop | L::MarginRight | L::MarginBottom | L::MarginLeft => {
            Value::LengthPercentageAuto(parse_lpa(cv, true)?)
        }
        L::PaddingTop | L::PaddingRight | L::PaddingBottom | L::PaddingLeft => {
            Value::LengthPercentage(parse_length_percentage(cv, false)?)
        }
        L::BorderTopWidth | L::BorderRightWidth | L::BorderBottomWidth | L::BorderLeftWidth => {
            Value::BorderWidth(parse_border_width(cv)?)
        }
        L::BorderTopStyle | L::BorderRightStyle | L::BorderBottomStyle | L::BorderLeftStyle => {
            Value::BorderStyle(parse_border_style(cv)?)
        }
        L::FontFamily => unreachable!("handled above"),
    })
}

fn longhand_by_name(name: &str) -> Option<Longhand> {
    use Longhand as L;
    Some(match name {
        "display" => L::Display,
        "color" => L::Color,
        "background-color" => L::BackgroundColor,
        "font-size" => L::FontSize,
        "font-weight" => L::FontWeight,
        "font-style" => L::FontStyle,
        "font-family" => L::FontFamily,
        "line-height" => L::LineHeight,
        "text-align" => L::TextAlign,
        "list-style-type" => L::ListStyleType,
        "overflow-x" => L::OverflowX,
        "overflow-y" => L::OverflowY,
        "width" => L::Width,
        "height" => L::Height,
        "min-width" => L::MinWidth,
        "max-width" => L::MaxWidth,
        "min-height" => L::MinHeight,
        "max-height" => L::MaxHeight,
        "margin-top" => L::MarginTop,
        "margin-right" => L::MarginRight,
        "margin-bottom" => L::MarginBottom,
        "margin-left" => L::MarginLeft,
        "padding-top" => L::PaddingTop,
        "padding-right" => L::PaddingRight,
        "padding-bottom" => L::PaddingBottom,
        "padding-left" => L::PaddingLeft,
        "border-top-width" => L::BorderTopWidth,
        "border-right-width" => L::BorderRightWidth,
        "border-bottom-width" => L::BorderBottomWidth,
        "border-left-width" => L::BorderLeftWidth,
        "border-top-style" => L::BorderTopStyle,
        "border-right-style" => L::BorderRightStyle,
        "border-bottom-style" => L::BorderBottomStyle,
        "border-left-style" => L::BorderLeftStyle,
        "border-top-color" => L::BorderTopColor,
        "border-right-color" => L::BorderRightColor,
        "border-bottom-color" => L::BorderBottomColor,
        "border-left-color" => L::BorderLeftColor,
        _ => return None,
    })
}

/// Parses `width style color` in any order (each optional, at least one).
fn parse_border_side(value: &[ComponentValue]) -> Option<(Value, Value, Value)> {
    let (mut width, mut style, mut color) = (None, None, None);
    let parts = items(value);
    if parts.is_empty() {
        return None;
    }
    for cv in parts {
        if width.is_none()
            && let Some(w) = parse_border_width(cv)
        {
            width = Some(w);
        } else if style.is_none()
            && let Some(s) = parse_border_style(cv)
        {
            style = Some(s);
        } else if color.is_none()
            && let Some(c) = parse_color(cv)
        {
            color = Some(c);
        } else {
            return None;
        }
    }
    Some((
        Value::BorderWidth(width.unwrap_or(BorderWidth::Medium)),
        Value::BorderStyle(style.unwrap_or(BorderStyle::None)),
        Value::Color(color.unwrap_or(ColorValue::CurrentColor)),
    ))
}

/// Converts one CSS declaration into longhand declarations. Returns an empty
/// list for unsupported properties or invalid values.
pub fn parse_declaration(decl: &Declaration) -> Vec<PropertyDeclaration> {
    let important = decl.important;
    let make = |pairs: Vec<(Longhand, Value)>| {
        pairs
            .into_iter()
            .map(|(property, value)| PropertyDeclaration {
                property,
                value,
                important,
            })
            .collect()
    };
    let value = decl.value.as_slice();

    if let Some(property) = longhand_by_name(&decl.name) {
        return parse_longhand(property, value)
            .map(|v| make(vec![(property, v)]))
            .unwrap_or_default();
    }

    // Shorthands. CSS-wide keywords apply to every longhand.
    let sides = |longhands: [Longhand; 4], parse: &dyn Fn(&ComponentValue) -> Option<Value>| {
        if let Some(wide) = css_wide(value) {
            return Some(
                longhands
                    .iter()
                    .map(|&l| (l, wide.clone()))
                    .collect::<Vec<_>>(),
            );
        }
        let parsed = four_sides(&parse_each(value, parse)?)?;
        Some(longhands.into_iter().zip(parsed).collect())
    };
    let pairs: Option<Vec<(Longhand, Value)>> = match decl.name.as_str() {
        "margin" => sides(MARGINS, &|cv| {
            parse_lpa(cv, true).map(Value::LengthPercentageAuto)
        }),
        "padding" => sides(PADDINGS, &|cv| {
            parse_length_percentage(cv, false).map(Value::LengthPercentage)
        }),
        "border-width" => sides(BORDER_WIDTHS, &|cv| {
            parse_border_width(cv).map(Value::BorderWidth)
        }),
        "border-style" => sides(BORDER_STYLES, &|cv| {
            parse_border_style(cv).map(Value::BorderStyle)
        }),
        "border-color" => sides(BORDER_COLORS, &|cv| parse_color(cv).map(Value::Color)),
        "border" | "border-top" | "border-right" | "border-bottom" | "border-left" => {
            let side_index = match decl.name.as_str() {
                "border-top" => Some(0),
                "border-right" => Some(1),
                "border-bottom" => Some(2),
                "border-left" => Some(3),
                _ => None,
            };
            let triple = match css_wide(value) {
                Some(w) => Some((w.clone(), w.clone(), w)),
                None => parse_border_side(value),
            };
            triple.map(|(w, s, c)| {
                let indices: Vec<usize> = side_index.map_or_else(|| (0..4).collect(), |i| vec![i]);
                indices
                    .into_iter()
                    .flat_map(|i| {
                        [
                            (BORDER_WIDTHS[i], w.clone()),
                            (BORDER_STYLES[i], s.clone()),
                            (BORDER_COLORS[i], c.clone()),
                        ]
                    })
                    .collect()
            })
        }
        // `list-style`: only the type component is supported; `inside`/`outside`
        // are accepted (markers are always outside) and images make it invalid.
        "list-style" => match css_wide(value) {
            Some(w) => Some(vec![(Longhand::ListStyleType, w)]),
            None => {
                let mut ty = None;
                let mut ok = true;
                for cv in items(value) {
                    let word = ident_lower(cv).unwrap_or_default();
                    match ListStyleType::from_ident(&word) {
                        Some(t) if ty.is_none() => ty = Some(t),
                        _ if matches!(word.as_str(), "inside" | "outside") => {}
                        _ => ok = false,
                    }
                }
                ok.then(|| {
                    vec![(
                        Longhand::ListStyleType,
                        Value::ListStyleType(ty.unwrap_or(ListStyleType::Disc)),
                    )]
                })
            }
        },
        "overflow" => {
            if let Some(w) = css_wide(value) {
                Some(vec![
                    (Longhand::OverflowX, w.clone()),
                    (Longhand::OverflowY, w),
                ])
            } else {
                parse_each(value, |cv| ident(cv).and_then(Overflow::from_ident)).and_then(|v| {
                    match v.as_slice() {
                        [x] => Some(vec![
                            (Longhand::OverflowX, Value::Overflow(*x)),
                            (Longhand::OverflowY, Value::Overflow(*x)),
                        ]),
                        [x, y] => Some(vec![
                            (Longhand::OverflowX, Value::Overflow(*x)),
                            (Longhand::OverflowY, Value::Overflow(*y)),
                        ]),
                        _ => None,
                    }
                })
            }
        }
        // Only the color component of `background` is supported so far; a
        // value with images, positions etc. is dropped rather than half-applied.
        "font" => match css_wide(value) {
            Some(w) => Some(
                [
                    Longhand::FontStyle,
                    Longhand::FontWeight,
                    Longhand::FontSize,
                    Longhand::LineHeight,
                    Longhand::FontFamily,
                ]
                .into_iter()
                .map(|l| (l, w.clone()))
                .collect(),
            ),
            None => parse_font_shorthand(value),
        },
        "background" => css_wide(value)
            .or_else(|| single(value).and_then(parse_color).map(Value::Color))
            .map(|v| vec![(Longhand::BackgroundColor, v)]),
        _ => None,
    };
    pairs.map(make).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::values::{LengthUnit, Rgba};

    fn parse(css: &str) -> Vec<PropertyDeclaration> {
        rustlite_css::parse_declaration_list(css)
            .iter()
            .flat_map(parse_declaration)
            .collect()
    }

    fn props(css: &str) -> Vec<(Longhand, Value)> {
        parse(css)
            .into_iter()
            .map(|d| (d.property, d.value))
            .collect()
    }

    #[test]
    fn margin_shorthand_expansion() {
        let px = |v| {
            Value::LengthPercentageAuto(LengthPercentageAuto::LengthPercentage(
                LengthPercentage::Length(Length::px(v)),
            ))
        };
        assert_eq!(
            props("margin: 1px 2px 3px"),
            [
                (Longhand::MarginTop, px(1.0)),
                (Longhand::MarginRight, px(2.0)),
                (Longhand::MarginBottom, px(3.0)),
                (Longhand::MarginLeft, px(2.0)),
            ]
        );
        assert_eq!(props("margin: 1px 2px 3px 4px 5px"), []);
        assert_eq!(props("margin: auto").len(), 4);
    }

    #[test]
    fn border_shorthand_any_order() {
        let p = props("border-top: red 2px dashed");
        assert_eq!(
            p,
            [
                (
                    Longhand::BorderTopWidth,
                    Value::BorderWidth(BorderWidth::Length(Length::px(2.0)))
                ),
                (
                    Longhand::BorderTopStyle,
                    Value::BorderStyle(BorderStyle::Dashed)
                ),
                (
                    Longhand::BorderTopColor,
                    Value::Color(ColorValue::Rgba(Rgba::rgb(255, 0, 0)))
                ),
            ]
        );
        assert_eq!(props("border: solid").len(), 12);
        assert_eq!(props("border: solid solid"), []);
    }

    #[test]
    fn invalid_values_are_dropped() {
        assert_eq!(
            props("padding: -1px; width: -5px; display: bogus; colour: red"),
            []
        );
        assert_eq!(props("font-weight: 1001; font-weight: 0"), []);
    }

    #[test]
    fn css_wide_keywords() {
        assert_eq!(props("color: inherit"), [(Longhand::Color, Value::Inherit)]);
        assert!(
            props("padding: initial")
                .iter()
                .all(|(_, v)| *v == Value::Initial)
        );
    }

    #[test]
    fn font_family_list() {
        let p = props(r#"font-family: "Open Sans", Times New Roman, serif"#);
        assert_eq!(
            p,
            [(
                Longhand::FontFamily,
                Value::FontFamily(
                    vec![
                        FamilyName::Named("Open Sans".into()),
                        FamilyName::Named("Times New Roman".into()),
                        FamilyName::Generic("serif".into()),
                    ]
                    .into()
                )
            )]
        );
        assert_eq!(props("font-family: a,,b"), []);
    }

    #[test]
    fn font_size_forms() {
        assert_eq!(
            props("font-size: large"),
            [(Longhand::FontSize, Value::FontSize(FontSize::Keyword(18.0)))]
        );
        assert_eq!(
            props("font-size: 2em"),
            [(
                Longhand::FontSize,
                Value::FontSize(FontSize::LengthPercentage(LengthPercentage::Length(
                    Length {
                        value: 2.0,
                        unit: LengthUnit::Em
                    }
                )))
            )]
        );
    }

    #[test]
    fn important_propagates_to_longhands() {
        assert!(parse("padding: 1px !important").iter().all(|d| d.important));
    }

    #[test]
    fn longhand_count() {
        assert_eq!(LONGHAND_COUNT, 38);
    }

    #[test]
    fn font_shorthand() {
        let p = props("font: italic bold 16px/1.6 system-ui, sans-serif");
        assert_eq!(
            p[0],
            (Longhand::FontStyle, Value::FontStyle(FontStyle::Italic))
        );
        assert_eq!(
            p[1],
            (
                Longhand::FontWeight,
                Value::FontWeight(FontWeight::Absolute(700))
            )
        );
        assert_eq!(
            p[3],
            (
                Longhand::LineHeight,
                Value::LineHeight(LineHeight::Number(1.6))
            )
        );
        assert!(matches!(&p[4].1, Value::FontFamily(f) if f.len() == 2));
        // Unspecified parts reset to their initial values.
        let q = props("font: 12px serif");
        assert_eq!(
            q[0],
            (Longhand::FontStyle, Value::FontStyle(FontStyle::Normal))
        );
        assert_eq!(
            q[3],
            (Longhand::LineHeight, Value::LineHeight(LineHeight::Normal))
        );
        // Size and family are required.
        assert_eq!(props("font: bold serif"), []);
        assert_eq!(props("font: 12px"), []);
        assert_eq!(props("font: caption"), []);
    }

    #[test]
    fn min_max_sizes() {
        assert_eq!(
            props("max-width: none"),
            [(Longhand::MaxWidth, Value::MaxSize(None))]
        );
        assert!(matches!(
            props("max-width: 26em")[0].1,
            Value::MaxSize(Some(_))
        ));
        assert_eq!(props("max-width: -1px"), []);
        assert!(matches!(
            props("min-height: 10%")[0].1,
            Value::LengthPercentageAuto(_)
        ));
    }

    #[test]
    fn light_dark_picks_the_light_color() {
        assert_eq!(
            props("background: light-dark(#eee, #222)"),
            [(
                Longhand::BackgroundColor,
                Value::Color(ColorValue::Rgba(Rgba::rgb(0xee, 0xee, 0xee)))
            )]
        );
    }
}

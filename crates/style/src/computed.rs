//! Computed values (CSS Cascade 4 §4.4) for the supported longhands.

use std::rc::Rc;

use crate::properties::{
    BorderStyle, BorderWidth, Display, FamilyName, FontSize, FontStyle, FontWeight, LineHeight,
    ListStyleType, Longhand, Overflow, TextAlign, Value,
};
use crate::values::{ColorValue, Length, LengthPercentage, LengthPercentageAuto, LengthUnit, Rgba};

/// A computed `<length-percentage>`: lengths are absolute px; percentages
/// are resolved during layout.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ComputedLength {
    Px(f32),
    Percentage(f32),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ComputedLengthAuto {
    Length(ComputedLength),
    Auto,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ComputedLineHeight {
    Normal,
    /// Unitless multiplier, inherited as a number.
    Number(f32),
    Px(f32),
}

/// The computed style of one element.
#[derive(Debug, Clone, PartialEq)]
pub struct ComputedStyle {
    pub display: Display,
    pub color: Rgba,
    pub background_color: Rgba,
    pub font_size: f32,
    pub font_weight: u16,
    pub font_style: FontStyle,
    pub font_family: Rc<[FamilyName]>,
    pub line_height: ComputedLineHeight,
    pub text_align: TextAlign,
    pub list_style_type: ListStyleType,
    pub overflow_x: Overflow,
    pub overflow_y: Overflow,
    pub width: ComputedLengthAuto,
    pub height: ComputedLengthAuto,
    /// Top, right, bottom, left.
    pub margin: [ComputedLengthAuto; 4],
    pub padding: [ComputedLength; 4],
    /// Snapped border widths in px (0 when the style is `none`/`hidden`).
    pub border_width: [f32; 4],
    pub border_style: [BorderStyle; 4],
    pub border_color: [Rgba; 4],
}

/// Initial font size (`medium`).
pub const INITIAL_FONT_SIZE: f32 = 16.0;

/// The initial `font-family` is UA-defined. These match Chromium's default
/// "standard font" per platform (Windows observed via the style oracle;
/// macOS and Linux from Chromium's default settings, not yet observed).
pub const INITIAL_FONT_FAMILY: &str = if cfg!(target_os = "macos") {
    "Times"
} else {
    "Times New Roman"
};

impl ComputedStyle {
    pub fn initial() -> Self {
        let zero = ComputedLength::Px(0.0);
        Self {
            display: Display::Inline,
            color: Rgba::BLACK,
            background_color: Rgba::TRANSPARENT,
            font_size: INITIAL_FONT_SIZE,
            font_weight: 400,
            font_style: FontStyle::Normal,
            font_family: vec![FamilyName::Named(INITIAL_FONT_FAMILY.into())].into(),
            line_height: ComputedLineHeight::Normal,
            text_align: TextAlign::Start,
            list_style_type: ListStyleType::Disc,
            overflow_x: Overflow::Visible,
            overflow_y: Overflow::Visible,
            width: ComputedLengthAuto::Auto,
            height: ComputedLengthAuto::Auto,
            margin: [ComputedLengthAuto::Length(zero); 4],
            padding: [zero; 4],
            // Computed value: `medium` (3px) becomes 0 with the initial `border-style: none`.
            border_width: [0.0; 4],
            border_style: [BorderStyle::None; 4],
            border_color: [Rgba::BLACK; 4],
        }
    }

    /// Initial values for non-inherited properties, parent values for inherited ones.
    fn inheriting_from(parent: &ComputedStyle) -> Self {
        Self {
            color: parent.color,
            font_size: parent.font_size,
            font_weight: parent.font_weight,
            font_style: parent.font_style,
            font_family: parent.font_family.clone(),
            line_height: parent.line_height,
            text_align: parent.text_align,
            list_style_type: parent.list_style_type,
            ..Self::initial()
        }
    }
}

/// Inputs to computation that are the same for every element of a document.
#[derive(Debug, Clone, Copy)]
pub struct ComputeContext {
    pub root_font_size: f32,
    pub viewport_width: f32,
    pub viewport_height: f32,
}

/// The winning declared value for each longhand (indexed by `Longhand as usize`).
pub type DeclaredValues<'a> = [Option<&'a Value>; crate::properties::LONGHAND_COUNT];

/// What a declared value means after resolving CSS-wide keywords.
enum Resolved<'a> {
    Specified(&'a Value),
    Inherit,
    Initial,
}

fn resolve(property: Longhand, declared: Option<&Value>) -> Resolved<'_> {
    match declared {
        None if property.is_inherited() => Resolved::Inherit,
        None => Resolved::Initial,
        Some(Value::Inherit) => Resolved::Inherit,
        Some(Value::Initial) => Resolved::Initial,
        Some(Value::Unset) if property.is_inherited() => Resolved::Inherit,
        Some(Value::Unset) => Resolved::Initial,
        Some(v) => Resolved::Specified(v),
    }
}

fn length_px(length: Length, font_size: f32, ctx: &ComputeContext) -> f32 {
    let base = match length.unit {
        LengthUnit::Px => 1.0,
        LengthUnit::Em => font_size,
        LengthUnit::Rem => ctx.root_font_size,
        LengthUnit::Vw => ctx.viewport_width / 100.0,
        LengthUnit::Vh => ctx.viewport_height / 100.0,
        LengthUnit::Vmin => ctx.viewport_width.min(ctx.viewport_height) / 100.0,
        LengthUnit::Vmax => ctx.viewport_width.max(ctx.viewport_height) / 100.0,
    };
    length.value * base
}

fn compute_lp(lp: LengthPercentage, font_size: f32, ctx: &ComputeContext) -> ComputedLength {
    match lp {
        LengthPercentage::Length(l) => ComputedLength::Px(length_px(l, font_size, ctx)),
        LengthPercentage::Percentage(p) => ComputedLength::Percentage(p),
    }
}

fn compute_lpa(
    lpa: LengthPercentageAuto,
    font_size: f32,
    ctx: &ComputeContext,
) -> ComputedLengthAuto {
    match lpa {
        LengthPercentageAuto::Auto => ComputedLengthAuto::Auto,
        LengthPercentageAuto::LengthPercentage(lp) => {
            ComputedLengthAuto::Length(compute_lp(lp, font_size, ctx))
        }
    }
}

/// `bolder`/`lighter` relative weights (CSS Fonts 4 §2.2.1).
fn relative_weight(parent: u16, bolder: bool) -> u16 {
    match (bolder, parent) {
        (true, w) if w < 350 => 400,
        (true, w) if w < 550 => 700,
        (true, _) => 900,
        (false, w) if w < 100 => w,
        (false, w) if w < 550 => 100,
        (false, w) if w < 750 => 400,
        (false, _) => 700,
    }
}

/// CSS Values 4 "snap as a border width" at a device pixel ratio of 1.
fn snap_border_width(px: f32) -> f32 {
    if px > 0.0 && px < 1.0 {
        1.0
    } else {
        px.floor()
    }
}

/// Computes an element's style from its winning declarations.
pub fn compute(
    declared: &DeclaredValues<'_>,
    parent: Option<&ComputedStyle>,
    is_root: bool,
    ctx: &ComputeContext,
) -> ComputedStyle {
    let initial = ComputedStyle::initial();
    let parent_or_initial = parent.unwrap_or(&initial);
    let mut style = match parent {
        Some(p) => ComputedStyle::inheriting_from(p),
        None => ComputedStyle::initial(),
    };
    let get = |l: Longhand| resolve(l, declared[l as usize]);

    // font-size first: em units elsewhere depend on it.
    let parent_size = parent_or_initial.font_size;
    style.font_size = match get(Longhand::FontSize) {
        Resolved::Inherit => parent_size,
        Resolved::Initial => INITIAL_FONT_SIZE,
        Resolved::Specified(Value::FontSize(fs)) => match *fs {
            FontSize::Keyword(px) => px,
            FontSize::Smaller => parent_size / 1.2,
            FontSize::Larger => parent_size * 1.2,
            // `em` and `%` in font-size refer to the parent's font size; `rem`
            // on the root element refers to the initial value.
            FontSize::LengthPercentage(LengthPercentage::Percentage(p)) => parent_size * p / 100.0,
            FontSize::LengthPercentage(LengthPercentage::Length(l)) => {
                let ctx = if is_root {
                    ComputeContext {
                        root_font_size: INITIAL_FONT_SIZE,
                        ..*ctx
                    }
                } else {
                    *ctx
                };
                length_px(l, parent_size, &ctx)
            }
        },
        Resolved::Specified(_) => parent_size,
    };
    let fs = style.font_size;

    style.color = match get(Longhand::Color) {
        Resolved::Inherit => parent_or_initial.color,
        Resolved::Initial => Rgba::BLACK,
        Resolved::Specified(Value::Color(ColorValue::Rgba(c))) => *c,
        // `color: currentcolor` behaves as `inherit`.
        Resolved::Specified(_) => parent_or_initial.color,
    };
    let current = style.color;

    macro_rules! simple {
        ($field:ident, $prop:expr, $variant:ident) => {
            match get($prop) {
                Resolved::Inherit => style.$field = parent_or_initial.$field,
                Resolved::Initial => style.$field = initial.$field,
                Resolved::Specified(Value::$variant(v)) => style.$field = *v,
                Resolved::Specified(_) => {}
            }
        };
    }
    simple!(display, Longhand::Display, Display);
    simple!(font_style, Longhand::FontStyle, FontStyle);
    simple!(text_align, Longhand::TextAlign, TextAlign);
    simple!(list_style_type, Longhand::ListStyleType, ListStyleType);
    simple!(overflow_x, Longhand::OverflowX, Overflow);
    simple!(overflow_y, Longhand::OverflowY, Overflow);

    style.background_color = match get(Longhand::BackgroundColor) {
        Resolved::Inherit => parent_or_initial.background_color,
        Resolved::Initial => Rgba::TRANSPARENT,
        Resolved::Specified(Value::Color(ColorValue::Rgba(c))) => *c,
        Resolved::Specified(_) => current,
    };

    style.font_weight = match get(Longhand::FontWeight) {
        Resolved::Inherit => parent_or_initial.font_weight,
        Resolved::Initial => 400,
        Resolved::Specified(Value::FontWeight(w)) => match *w {
            FontWeight::Absolute(w) => w,
            FontWeight::Bolder => relative_weight(parent_or_initial.font_weight, true),
            FontWeight::Lighter => relative_weight(parent_or_initial.font_weight, false),
        },
        Resolved::Specified(_) => style.font_weight,
    };

    match get(Longhand::FontFamily) {
        Resolved::Inherit => style.font_family = parent_or_initial.font_family.clone(),
        Resolved::Initial => style.font_family = initial.font_family.clone(),
        Resolved::Specified(Value::FontFamily(f)) => style.font_family = f.clone(),
        Resolved::Specified(_) => {}
    }

    style.line_height = match get(Longhand::LineHeight) {
        Resolved::Inherit => parent_or_initial.line_height,
        Resolved::Initial => ComputedLineHeight::Normal,
        Resolved::Specified(Value::LineHeight(lh)) => match *lh {
            LineHeight::Normal => ComputedLineHeight::Normal,
            LineHeight::Number(n) => ComputedLineHeight::Number(n),
            LineHeight::LengthPercentage(LengthPercentage::Percentage(p)) => {
                ComputedLineHeight::Px(fs * p / 100.0)
            }
            LineHeight::LengthPercentage(LengthPercentage::Length(l)) => {
                ComputedLineHeight::Px(length_px(l, fs, ctx))
            }
        },
        Resolved::Specified(_) => style.line_height,
    };

    let lpa =
        |prop: Longhand, inherited: ComputedLengthAuto, init: ComputedLengthAuto| match get(prop) {
            Resolved::Inherit => inherited,
            Resolved::Initial => init,
            Resolved::Specified(Value::LengthPercentageAuto(v)) => compute_lpa(*v, fs, ctx),
            Resolved::Specified(_) => init,
        };
    style.width = lpa(Longhand::Width, parent_or_initial.width, initial.width);
    style.height = lpa(Longhand::Height, parent_or_initial.height, initial.height);

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
    const WIDTHS: [Longhand; 4] = [
        Longhand::BorderTopWidth,
        Longhand::BorderRightWidth,
        Longhand::BorderBottomWidth,
        Longhand::BorderLeftWidth,
    ];
    const STYLES: [Longhand; 4] = [
        Longhand::BorderTopStyle,
        Longhand::BorderRightStyle,
        Longhand::BorderBottomStyle,
        Longhand::BorderLeftStyle,
    ];
    const COLORS: [Longhand; 4] = [
        Longhand::BorderTopColor,
        Longhand::BorderRightColor,
        Longhand::BorderBottomColor,
        Longhand::BorderLeftColor,
    ];

    for i in 0..4 {
        style.margin[i] = lpa(MARGINS[i], parent_or_initial.margin[i], initial.margin[i]);
        style.padding[i] = match get(PADDINGS[i]) {
            Resolved::Inherit => parent_or_initial.padding[i],
            Resolved::Initial => initial.padding[i],
            Resolved::Specified(Value::LengthPercentage(v)) => compute_lp(*v, fs, ctx),
            Resolved::Specified(_) => initial.padding[i],
        };
        style.border_style[i] = match get(STYLES[i]) {
            Resolved::Inherit => parent_or_initial.border_style[i],
            Resolved::Specified(Value::BorderStyle(s)) => *s,
            _ => BorderStyle::None,
        };
        let width = match get(WIDTHS[i]) {
            Resolved::Inherit => parent_or_initial.border_width[i],
            Resolved::Specified(Value::BorderWidth(w)) => match *w {
                BorderWidth::Thin => 1.0,
                BorderWidth::Medium => 3.0,
                BorderWidth::Thick => 5.0,
                BorderWidth::Length(l) => length_px(l, fs, ctx),
            },
            _ => 3.0,
        };
        style.border_width[i] = if matches!(
            style.border_style[i],
            BorderStyle::None | BorderStyle::Hidden
        ) {
            0.0
        } else {
            snap_border_width(width)
        };
        style.border_color[i] = match get(COLORS[i]) {
            Resolved::Inherit => parent_or_initial.border_color[i],
            Resolved::Specified(Value::Color(ColorValue::Rgba(c))) => *c,
            // Initial value and explicit `currentcolor`.
            _ => current,
        };
    }

    if is_root {
        style.display = style.display.blockified();
    }
    // Overflow: `visible`/`clip` paired with a scrollable value compute to
    // `auto`/`hidden` (CSS Overflow 3 §3.1).
    let scrollable = |o: Overflow| !matches!(o, Overflow::Visible | Overflow::Clip);
    if scrollable(style.overflow_x) != scrollable(style.overflow_y) {
        let fix = |o: Overflow| match o {
            Overflow::Visible => Overflow::Auto,
            Overflow::Clip => Overflow::Hidden,
            o => o,
        };
        style.overflow_x = fix(style.overflow_x);
        style.overflow_y = fix(style.overflow_y);
    }
    style
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_weights() {
        assert_eq!(relative_weight(400, true), 700);
        assert_eq!(relative_weight(700, true), 900);
        assert_eq!(relative_weight(100, true), 400);
        assert_eq!(relative_weight(400, false), 100);
        assert_eq!(relative_weight(700, false), 400);
        assert_eq!(relative_weight(900, false), 700);
    }

    #[test]
    fn border_width_snapping() {
        assert_eq!(snap_border_width(0.0), 0.0);
        assert_eq!(snap_border_width(0.5), 1.0);
        assert_eq!(snap_border_width(2.7), 2.0);
    }
}

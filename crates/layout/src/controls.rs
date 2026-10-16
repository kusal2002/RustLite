//! Form controls as atomic inlines: what they show, their intrinsic sizes,
//! and their baselines (HTML §15.5, matched to Chromium 152 measurements).
//!
//! - Text fields: content width = `c × size + (M − c)` where `c` is the
//!   rounded advance of "x" and `M` the rounded font bounding-box width
//!   (Blink's average/max char width), `size` defaulting to 20; content
//!   height is the font's normal line height. Baseline: the text baseline.
//! - Buttons: content box = label width × line height; baseline: text.
//! - Checkboxes and radios: 13 × 13; baseline: the bottom border edge.

use rustlite_dom::{ControlKind, Document, NodeId};
use rustlite_fonts::FontContext;
use rustlite_paint::GlyphRun;
use rustlite_style::{ComputedLength, ComputedLengthAuto, ComputedStyle};

use crate::inline::{StyleFont, shape};

/// What a control shows, captured when the box tree is built.
#[derive(Debug, Clone, PartialEq)]
pub struct Control {
    pub kind: ControlKind,
    /// Text drawn inside: the value (masked for passwords), the
    /// placeholder, or the button label.
    pub text: String,
    /// `text` is the placeholder (drawn in a muted color).
    pub placeholder: bool,
    pub checked: bool,
    /// `size` attribute of text fields (characters wide).
    pub size: u32,
}

/// Checkbox and radio size in px.
pub const TOGGLE_SIZE: f32 = 13.0;

impl Control {
    pub fn from_dom(doc: &Document, node: NodeId, kind: ControlKind) -> Self {
        let attr = |name| doc.attribute(node, name);
        let (text, placeholder) = match kind {
            ControlKind::Text { password } => {
                let value = doc.control_value(node);
                if value.is_empty() {
                    (attr("placeholder").unwrap_or("").to_owned(), true)
                } else if password {
                    ("\u{2022}".repeat(value.chars().count()), false)
                } else {
                    (value, false)
                }
            }
            ControlKind::Button if doc.is_element_named(node, "button") => (
                doc.text_content(node)
                    .split_ascii_whitespace()
                    .collect::<Vec<_>>()
                    .join(" "),
                false,
            ),
            ControlKind::Button => {
                let t = attr("type").unwrap_or("").to_ascii_lowercase();
                let default = match t.as_str() {
                    "submit" => "Submit",
                    "reset" => "Reset",
                    "image" => attr("alt").unwrap_or("Submit"),
                    _ => "",
                };
                (attr("value").unwrap_or(default).to_owned(), false)
            }
            ControlKind::Checkbox | ControlKind::Radio => (String::new(), false),
        };
        let size = attr("size")
            .and_then(|s| s.trim().parse::<u32>().ok())
            .filter(|&s| s > 0)
            .unwrap_or(20)
            .min(10_000);
        Self {
            kind,
            text,
            placeholder,
            checked: doc.checked(node),
            size,
        }
    }
}

/// Laid-out content of a control: content-box size, distance from the
/// content-box top to the baseline (`None` = bottom border edge), and the
/// shaped text with its width.
pub(crate) struct ControlLayout {
    pub width: f32,
    pub height: f32,
    pub baseline: Option<f32>,
    pub runs: Vec<(f32, GlyphRun)>,
    pub text_width: f32,
}

fn specified(l: ComputedLengthAuto, basis: Option<f32>) -> Option<f32> {
    match l {
        ComputedLengthAuto::Auto => None,
        ComputedLengthAuto::Length(ComputedLength::Px(v)) => Some(v),
        ComputedLengthAuto::Length(ComputedLength::Percentage(p)) => basis.map(|b| b * p / 100.0),
    }
}

pub(crate) fn layout_control(
    control: &Control,
    style: &ComputedStyle,
    font: &StyleFont,
    fonts: &mut FontContext,
    cb_width: f32,
) -> ControlLayout {
    let (above, below) = font.extents();
    let (runs, text_width) = match control.kind {
        ControlKind::Checkbox | ControlKind::Radio => (Vec::new(), 0.0),
        _ => shape(&control.text, font, fonts),
    };
    let (intrinsic_w, intrinsic_h, baseline) = match control.kind {
        ControlKind::Text { .. } => {
            let (c, m) = match font.font {
                Some(f) => {
                    let x = fonts.measure(f, font.weight, font.italic, font.size, "x");
                    (x.round(), fonts.max_char_width(f, font.size).round())
                }
                None => ((font.size * 0.5).round(), font.size.round()),
            };
            (
                c * control.size as f32 + (m - c),
                above + below,
                Some(above),
            )
        }
        ControlKind::Button => (text_width, above + below, Some(above)),
        ControlKind::Checkbox | ControlKind::Radio => (TOGGLE_SIZE, TOGGLE_SIZE, None),
    };
    let width = specified(style.width, Some(cb_width)).unwrap_or(intrinsic_w);
    let height = specified(style.height, None).unwrap_or(intrinsic_h);
    // A taller-than-text control keeps its text vertically centered.
    let baseline = baseline.map(|b| b + ((height - intrinsic_h) / 2.0).max(0.0));
    ControlLayout {
        width: width.max(0.0),
        height: height.max(0.0),
        baseline,
        runs,
        text_width,
    }
}

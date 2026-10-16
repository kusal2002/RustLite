//! Inline formatting (CSS 2.1 §9.4.2, §10.8; CSS Text 3 §4 for whitespace).
//!
//! Pipeline: collapse whitespace across the whole inline formatting context
//! → split into words and spaces → shape each word → greedy line breaking at
//! spaces → build line boxes with half-leading and baseline alignment →
//! apply `text-align`.

use std::rc::Rc;

use rustlite_fonts::{Family, FontContext};
use rustlite_paint::{FontId, GlyphRun, PositionedGlyph};
use rustlite_style::properties::{FamilyName, FontStyle, TextAlign};
use rustlite_style::{ComputedLineHeight, ComputedStyle};

use crate::{InlineContent, InlineItem, LayoutContext, LineBox, RectF, TextFragment};

/// Font and vertical metrics for one style, in px.
#[derive(Debug, Clone)]
pub(crate) struct StyleFont {
    pub(crate) font: Option<FontId>,
    pub(crate) size: f32,
    pub(crate) weight: u16,
    pub(crate) italic: bool,
    ascent: f32,
    descent: f32,
    line_height: f32,
}

impl StyleFont {
    /// Space above and below the baseline that this inline box adds to the
    /// line, including half-leading (CSS 2.1 §10.8.1).
    ///
    /// The half-leading above the baseline is floored to whole pixels and the
    /// remainder goes below, as Blink's line layout does (observed via the
    /// layout oracle: without it inline boxes sit 0.5px lower than Chromium's).
    fn extents(&self) -> (f32, f32) {
        let half_leading = (self.line_height - (self.ascent + self.descent)) / 2.0;
        let above = self.ascent + half_leading.floor();
        (above, self.line_height - above)
    }
}

pub(crate) fn families(style: &ComputedStyle) -> Vec<Family<'_>> {
    style
        .font_family
        .iter()
        .map(|f| match f {
            FamilyName::Generic(g) => Family::from_css_generic(g),
            FamilyName::Named(n) => Family::Named(n),
        })
        .collect()
}

pub(crate) fn style_font(style: &ComputedStyle, fonts: &mut FontContext) -> StyleFont {
    let italic = style.font_style != FontStyle::Normal;
    let font = fonts.select(&families(style), style.font_weight, italic);
    let size = style.font_size;
    // Like Blink, round ascent, descent and line gap to whole pixels before
    // using them for line layout, so line heights are integral for `normal`.
    let (ascent, descent, gap) = match font {
        Some(f) => {
            let m = fonts.metrics(f, size);
            (m.ascent.round(), m.descent.round(), m.line_gap.round())
        }
        None => ((size * 0.8).round(), (size * 0.2).round(), 0.0),
    };
    let line_height = match style.line_height {
        ComputedLineHeight::Normal => ascent + descent + gap,
        ComputedLineHeight::Number(n) => n * size,
        ComputedLineHeight::Px(v) => v,
    };
    StyleFont {
        font,
        size,
        weight: style.font_weight,
        italic,
        ascent,
        descent,
        line_height,
    }
}

enum Atom {
    Word { item: usize, text: String },
    Space { item: usize },
    Break { item: usize },
    Atomic { item: usize },
}

/// Geometry of an atomic inline (replaced element), in px.
#[derive(Debug, Clone, Copy)]
struct AtomicBox {
    /// Margin box size: what the line sees.
    margin_width: f32,
    margin_height: f32,
    /// Border box offset inside the margin box, and its size.
    dx: f32,
    dy: f32,
    border_width: f32,
    border_height: f32,
}

fn atomic_box(item: &InlineItem, cb_width: f32) -> Option<AtomicBox> {
    let InlineItem::Atomic { style, image, .. } = item else {
        return None;
    };
    let (w, h) = crate::replaced::content_size(style, image.as_deref(), cb_width);
    let pct = |l: rustlite_style::ComputedLength| match l {
        rustlite_style::ComputedLength::Px(v) => v,
        rustlite_style::ComputedLength::Percentage(p) => cb_width * p / 100.0,
    };
    let margin = |i: usize| match style.margin[i] {
        rustlite_style::ComputedLengthAuto::Auto => 0.0,
        rustlite_style::ComputedLengthAuto::Length(l) => pct(l),
    };
    let frame = |i: usize| style.border_width[i] + pct(style.padding[i]);
    let border_width = w + frame(1) + frame(3);
    let border_height = h + frame(0) + frame(2);
    Some(AtomicBox {
        margin_width: border_width + margin(1) + margin(3),
        margin_height: border_height + margin(0) + margin(2),
        dx: margin(3),
        dy: margin(0),
        border_width,
        border_height,
    })
}

fn is_collapsible(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0C')
}

/// Collapses whitespace (`white-space: normal`) across item boundaries and
/// splits the result into words, spaces and forced breaks. Leading spaces at
/// the start of the context and after a break are removed.
fn atoms(items: &[InlineItem]) -> Vec<Atom> {
    let mut out = Vec::new();
    let mut prev_space = true;
    for (i, item) in items.iter().enumerate() {
        match item {
            InlineItem::Text { text, .. } => {
                let mut word = String::new();
                for c in text.chars() {
                    if is_collapsible(c) {
                        if !word.is_empty() {
                            out.push(Atom::Word {
                                item: i,
                                text: std::mem::take(&mut word),
                            });
                        }
                        if !prev_space {
                            out.push(Atom::Space { item: i });
                            prev_space = true;
                        }
                    } else {
                        word.push(c);
                        prev_space = false;
                    }
                }
                if !word.is_empty() {
                    out.push(Atom::Word {
                        item: i,
                        text: word,
                    });
                }
            }
            InlineItem::LineBreak { .. } => {
                // A space right before a forced break is trailing; drop it.
                if matches!(out.last(), Some(Atom::Space { .. })) {
                    out.pop();
                }
                out.push(Atom::Break { item: i });
                prev_space = true;
            }
            InlineItem::Atomic { .. } => {
                out.push(Atom::Atomic { item: i });
                prev_space = false;
            }
        }
    }
    out
}

/// Builds glyph runs for `text`; returns (runs with x offsets, width).
fn shape(text: &str, f: &StyleFont, fonts: &mut FontContext) -> (Vec<(f32, GlyphRun)>, f32) {
    let Some(font) = f.font else {
        return (Vec::new(), 0.0);
    };
    let mut x = 0.0;
    let mut runs = Vec::new();
    for seg in fonts.shape(font, f.weight, f.italic, f.size, text).iter() {
        let mut pen = 0.0;
        let glyphs: Rc<[PositionedGlyph]> = seg
            .glyphs
            .iter()
            .map(|g| {
                let p = PositionedGlyph {
                    id: g.id,
                    x: pen + g.x_offset,
                    y: -g.y_offset,
                };
                pen += g.x_advance;
                p
            })
            .collect();
        runs.push((
            x,
            GlyphRun {
                font: seg.font,
                size: f.size,
                glyphs,
            },
        ));
        x += seg.advance;
    }
    (runs, x)
}

/// Shapes standalone text (a list marker) in `style`'s font. Returns the glyph
/// runs, the advance width, and the font's ascent.
pub(crate) fn shape_text(
    text: &str,
    style: &ComputedStyle,
    fonts: &mut FontContext,
) -> (Vec<(f32, GlyphRun)>, f32, f32) {
    let f = style_font(style, fonts);
    let (runs, width) = shape(text, &f, fonts);
    (runs, width, f.ascent)
}

/// The baseline of the first line of `content`, relative to its block's
/// content box (valid during layout).
pub(crate) fn first_baseline(content: &InlineContent) -> Option<f32> {
    content.lines.first()?.fragments.first().map(|f| f.baseline)
}

struct LineBuilder<'a> {
    width: f32,
    align: TextAlign,
    strut: (f32, f32),
    fonts: &'a [StyleFont],
    atomics: &'a [Option<AtomicBox>],
    lines: Vec<LineBox>,
    current: Vec<TextFragment>,
    x: f32,
    y: f32,
}

impl LineBuilder<'_> {
    /// Appends `width` of content from `item` at the pen, merging into the
    /// previous fragment when it belongs to the same item.
    fn place(&mut self, item: usize, width: f32, runs: Vec<(f32, GlyphRun)>, text: &str) {
        let x = self.x;
        match self.current.last_mut() {
            Some(f) if f.item == item && (f.rect.right() - x).abs() < 0.001 => {
                let base = f.rect.x;
                f.runs
                    .extend(runs.into_iter().map(|(dx, r)| (x - base + dx, r)));
                f.rect.width += width;
                f.text.push_str(text);
            }
            _ => self.current.push(TextFragment {
                item,
                rect: RectF::new(x, 0.0, width, 0.0),
                baseline: 0.0,
                runs,
                text: text.to_owned(),
            }),
        }
        self.x += width;
    }

    fn finish_line(&mut self) {
        let (mut above, mut below) = self.strut;
        for f in &self.current {
            // Atomic inlines sit with their bottom margin edge on the baseline.
            let (a, b) = match self.atomics[f.item] {
                Some(atomic) => (atomic.margin_height, 0.0),
                None => self.fonts[f.item].extents(),
            };
            above = above.max(a);
            below = below.max(b);
        }
        let baseline = self.y + above;
        let used = self.x;
        let shift = match self.align {
            TextAlign::Center => (self.width - used) / 2.0,
            TextAlign::Right | TextAlign::End => self.width - used,
            TextAlign::Left | TextAlign::Start | TextAlign::Justify => 0.0,
        }
        .max(0.0);
        let mut fragments = std::mem::take(&mut self.current);
        for f in &mut fragments {
            f.rect.x += shift;
            f.baseline = baseline;
            if let Some(a) = self.atomics[f.item] {
                f.rect.x += a.dx;
                f.rect.width = a.border_width;
                f.rect.y = baseline - a.margin_height + a.dy;
                f.rect.height = a.border_height;
            } else {
                let font = &self.fonts[f.item];
                f.rect.y = baseline - font.ascent;
                f.rect.height = font.ascent + font.descent;
            }
        }
        let height = above + below;
        self.lines.push(LineBox {
            rect: RectF::new(0.0, self.y, self.width, height),
            fragments,
        });
        self.y += height;
        self.x = 0.0;
    }
}

/// Lays out `content` into lines of `width` px. Returns the total height.
pub(crate) fn layout_inline(
    content: &mut InlineContent,
    width: f32,
    block_style: &ComputedStyle,
    ctx: &mut LayoutContext<'_>,
) -> f32 {
    let strut = style_font(block_style, ctx.fonts).extents();
    let fonts: Vec<StyleFont> = content
        .items
        .iter()
        .map(|item| match item {
            InlineItem::Text { style, .. }
            | InlineItem::LineBreak { style, .. }
            | InlineItem::Atomic { style, .. } => style_font(style, ctx.fonts),
        })
        .collect();
    let mut space_widths: Vec<Option<f32>> = vec![None; content.items.len()];
    let atomics: Vec<Option<AtomicBox>> =
        content.items.iter().map(|i| atomic_box(i, width)).collect();

    let mut b = LineBuilder {
        width,
        align: block_style.text_align,
        strut,
        fonts: &fonts,
        atomics: &atomics,
        lines: Vec::new(),
        current: Vec::new(),
        x: 0.0,
        y: 0.0,
    };

    let atoms = atoms(&content.items);
    let mut pending_space: Option<usize> = None;
    let mut i = 0;
    while i < atoms.len() {
        match &atoms[i] {
            Atom::Space { item } => {
                pending_space = Some(*item);
                i += 1;
            }
            Atom::Break { item } => {
                // The <br> is a zero-width inline box at the end of its line;
                // its font contributes to the line height, as in browsers.
                b.place(*item, 0.0, Vec::new(), "");
                b.finish_line();
                pending_space = None;
                i += 1;
            }
            Atom::Atomic { item } => {
                // Replaced elements are break opportunities on both sides.
                let w = atomics[*item].map_or(0.0, |a| a.margin_width);
                let has_content = !b.current.is_empty();
                let space = match pending_space.take() {
                    Some(sp) if has_content => {
                        let sw = *space_widths[sp]
                            .get_or_insert_with(|| shape(" ", &fonts[sp], ctx.fonts).1);
                        Some((sp, sw))
                    }
                    _ => None,
                };
                if has_content && b.x + space.map_or(0.0, |s| s.1) + w > width + 0.01 {
                    b.finish_line();
                } else if let Some((sp, sw)) = space {
                    b.place(sp, sw, Vec::new(), " ");
                }
                b.place(*item, w, Vec::new(), "");
                i += 1;
            }
            Atom::Word { .. } => {
                // An unbreakable group: consecutive words with no space between
                // (e.g. "fo<b>o</b>").
                let mut group = Vec::new();
                while let Some(Atom::Word { item, text }) = atoms.get(i) {
                    let (runs, w) = shape(text, &fonts[*item], ctx.fonts);
                    group.push((*item, runs, w, text));
                    i += 1;
                }
                let group_width: f32 = group.iter().map(|g| g.2).sum();
                let has_content = !b.current.is_empty();
                let space = match pending_space.take() {
                    Some(item) if has_content => {
                        let w = *space_widths[item]
                            .get_or_insert_with(|| shape(" ", &fonts[item], ctx.fonts).1);
                        Some((item, w))
                    }
                    _ => None,
                };
                let space_w = space.map_or(0.0, |s| s.1);
                if has_content && b.x + space_w + group_width > width + 0.01 {
                    b.finish_line();
                } else if let Some((item, w)) = space {
                    b.place(item, w, Vec::new(), " ");
                }
                for (item, runs, w, text) in group {
                    b.place(item, w, runs, text);
                }
            }
        }
    }
    if !b.current.is_empty() {
        b.finish_line();
    }
    let height = b.y;
    content.lines = b.lines;
    height
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(items: &[InlineItem]) -> Vec<String> {
        atoms(items)
            .into_iter()
            .map(|a| match a {
                Atom::Word { text, .. } => text,
                Atom::Space { .. } => "_".into(),
                Atom::Break { .. } => "|".into(),
                Atom::Atomic { .. } => "[img]".into(),
            })
            .collect()
    }

    fn text(s: &str) -> InlineItem {
        InlineItem::Text {
            text: s.into(),
            style: Rc::new(ComputedStyle::initial()),
            ancestors: Rc::from(Vec::new()),
        }
    }

    #[test]
    fn whitespace_collapses_across_items() {
        let items = [text("  Hello \n\t "), text("  world  "), text("!")];
        assert_eq!(words(&items), ["Hello", "_", "world", "_", "!"]);
    }

    #[test]
    fn spaces_around_breaks_are_removed() {
        let br = InlineItem::LineBreak {
            node: rustlite_dom::Document::new().root(),
            style: Rc::new(ComputedStyle::initial()),
        };
        let items = [text("a "), br, text(" b")];
        assert_eq!(words(&items), ["a", "|", "b"]);
    }

    #[test]
    fn no_break_opportunity_between_adjacent_items() {
        let items = [text("fo"), text("o bar")];
        assert_eq!(words(&items), ["fo", "o", "_", "bar"]);
    }
}

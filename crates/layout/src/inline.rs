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
    pub(crate) fn extents(&self) -> (f32, f32) {
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
    Word {
        item: usize,
        text: String,
    },
    Space {
        item: usize,
    },
    /// A break opportunity without a space (after a hyphen, UAX #14 HY/BA).
    Opportunity,
    Break {
        item: usize,
    },
    Atomic {
        item: usize,
    },
}

/// Geometry of an atomic inline (replaced element or control), in px.
#[derive(Debug, Clone, Copy)]
struct AtomicBox {
    /// Margin box size: what the line sees.
    margin_width: f32,
    margin_height: f32,
    /// Distance from the margin-box top to the baseline: the bottom margin
    /// edge for images, the text baseline for text controls.
    above: f32,
    /// Border box offset inside the margin box, and its size.
    dx: f32,
    dy: f32,
    border_width: f32,
    border_height: f32,
}

/// Sizes an atomic inline. Controls also return their shaped text, placed
/// relative to the border box (for painting).
fn atomic_box(
    item: &mut InlineItem,
    cb_width: f32,
    font: &StyleFont,
    ctx: &mut LayoutContext<'_>,
) -> Option<(AtomicBox, Vec<(f32, GlyphRun)>)> {
    let InlineItem::Atomic {
        style,
        image,
        control,
        block,
        ..
    } = item
    else {
        return None;
    };
    if let Some(inner) = block {
        return Some((inline_block_box(inner, cb_width, ctx), Vec::new()));
    }
    let control_layout = control
        .as_ref()
        .map(|c| crate::controls::layout_control(c, style, font, ctx.fonts, cb_width));
    let (w, h) = match &control_layout {
        Some(c) => (c.width, c.height),
        None => crate::replaced::content_size(style, image.as_deref(), cb_width),
    };
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
    let margin_height = border_height + margin(0) + margin(2);
    let mut runs = Vec::new();
    let above = match control_layout {
        Some(c) => {
            // Text starts at the content edge; button labels are centered.
            let centered = matches!(
                control.as_ref().map(|c| c.kind),
                Some(rustlite_dom::ControlKind::Button)
            );
            let x0 = frame(3)
                + if centered {
                    ((w - c.text_width) / 2.0).max(0.0)
                } else {
                    0.0
                };
            runs = c.runs.into_iter().map(|(dx, r)| (x0 + dx, r)).collect();
            match c.baseline {
                Some(b) => margin(0) + frame(0) + b,
                None => margin(0) + border_height,
            }
        }
        None => margin_height,
    };
    Some((
        AtomicBox {
            margin_width: border_width + margin(1) + margin(3),
            margin_height,
            above,
            dx: margin(3),
            dy: margin(0),
            border_width,
            border_height,
        },
        runs,
    ))
}

/// Lays out an inline-level block container (`inline-block`,
/// `inline-flex`) with a shrink-to-fit width. Its baseline is its last line
/// box's (first for flex), or its bottom margin edge if it has none or does
/// not have `overflow: visible` (CSS 2.1 §10.8.1).
fn inline_block_box(
    inner: &mut crate::BlockBox,
    cb_width: f32,
    ctx: &mut LayoutContext<'_>,
) -> AtomicBox {
    use rustlite_style::properties::{Display, Overflow};
    let content_width = crate::block::shrink_to_fit_width(inner, cb_width, cb_width, ctx);
    let o = crate::block::layout_block_inner(
        inner,
        cb_width,
        None,
        ctx,
        false,
        (0.0, 0.0),
        crate::block::Forced {
            content_width: Some(content_width),
            content_height: None,
        },
    );
    let m = inner.margin;
    let border_width = inner.rect.width;
    let border_height = o.height;
    let margin_height = border_height + m.top + m.bottom;
    let visible = inner.style.overflow_x == Overflow::Visible;
    let baseline = if !visible {
        None
    } else if inner.style.display == Display::InlineFlex {
        crate::block::first_baseline(&inner.children)
    } else {
        crate::block::last_baseline(&inner.children)
    };
    let above = baseline.map_or(margin_height, |b| {
        m.top + inner.border.top + inner.padding.top + b
    });
    AtomicBox {
        margin_width: border_width + m.left + m.right,
        margin_height,
        above,
        dx: m.left,
        dy: m.top,
        border_width,
        border_height,
    }
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
                let mut chars = text.chars().peekable();
                while let Some(c) = chars.next() {
                    // Lines may break after a hyphen that follows text and
                    // precedes a letter ("shrink-|wrapped", "non--|stop"; not
                    // "-5" or "10-20"), as browsers do (UAX #14 LB21, LB25).
                    if matches!(c, '-' | '\u{2010}')
                        && !word.is_empty()
                        && chars.peek().is_some_and(|n| n.is_alphabetic())
                    {
                        word.push(c);
                        out.push(Atom::Word {
                            item: i,
                            text: std::mem::take(&mut word),
                        });
                        out.push(Atom::Opportunity);
                        prev_space = false;
                        continue;
                    }
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
pub(crate) fn shape(
    text: &str,
    f: &StyleFont,
    fonts: &mut FontContext,
) -> (Vec<(f32, GlyphRun)>, f32) {
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

/// The baseline of the last line of `content` (valid during layout).
pub(crate) fn last_baseline(content: &InlineContent) -> Option<f32> {
    content.lines.last()?.fragments.first().map(|f| f.baseline)
}

struct LineBuilder<'a> {
    /// Width available to the current line (narrowed by floats).
    width: f32,
    /// The block's content width.
    full_width: f32,
    /// Start of the current line, past any left floats.
    line_left: f32,
    line_started: bool,
    /// The content box origin in float coordinates, and the floats to avoid.
    origin: (f32, f32),
    floats: Option<&'a crate::block::Floats>,
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
    /// Computes the current line's free span (CSS 2.1 §9.5: line boxes next
    /// to floats are shortened).
    fn start_line(&mut self) {
        if self.line_started {
            return;
        }
        self.line_started = true;
        self.line_left = 0.0;
        self.width = self.full_width;
        if let Some(f) = self.floats.filter(|f| !f.is_empty()) {
            let h = self.strut.0 + self.strut.1;
            let (l, r) = f.band(
                self.origin.1 + self.y,
                h,
                self.origin.0,
                self.origin.0 + self.full_width,
            );
            self.line_left = l - self.origin.0;
            self.width = (r - l).max(0.0);
        }
    }

    /// Moves an empty line down past floats until `w` fits beside them (or
    /// no float narrows the line any more).
    fn make_room(&mut self, w: f32) {
        self.start_line();
        while self.current.is_empty()
            && w > self.width + 0.01
            && self.width < self.full_width - 0.01
        {
            let Some(next) = self
                .floats
                .and_then(|f| f.next_bottom(self.origin.1 + self.y))
            else {
                break;
            };
            self.y = next - self.origin.1;
            self.line_started = false;
            self.start_line();
        }
    }

    /// Appends `width` of content from `item` at the pen, merging into the
    /// previous fragment when it belongs to the same item.
    fn place(&mut self, item: usize, width: f32, runs: Vec<(f32, GlyphRun)>, text: &str) {
        self.start_line();
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
            // Images sit with their bottom margin edge on the baseline;
            // text controls align their text baseline with the line's.
            let (a, b) = match self.atomics[f.item] {
                Some(atomic) => (atomic.above, atomic.margin_height - atomic.above),
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
            f.rect.x += self.line_left + shift;
            f.baseline = baseline;
            if let Some(a) = self.atomics[f.item] {
                f.rect.x += a.dx;
                f.rect.width = a.border_width;
                f.rect.y = baseline - a.above + a.dy;
                f.rect.height = a.border_height;
            } else {
                let font = &self.fonts[f.item];
                f.rect.y = baseline - font.ascent;
                f.rect.height = font.ascent + font.descent;
            }
        }
        let height = above + below;
        self.lines.push(LineBox {
            rect: RectF::new(self.line_left, self.y, self.width, height),
            fragments,
        });
        self.y += height;
        self.x = 0.0;
        self.line_started = false;
    }
}

/// Lays out `content` into lines of `width` px. `origin` is the content
/// box's position among the current formatting context's floats. Returns
/// the total height.
pub(crate) fn layout_inline(
    content: &mut InlineContent,
    width: f32,
    block_style: &ComputedStyle,
    ctx: &mut LayoutContext<'_>,
    origin: (f32, f32),
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
    let mut atomic_runs: Vec<Vec<(f32, GlyphRun)>> = Vec::new();
    let mut atomics: Vec<Option<AtomicBox>> = Vec::with_capacity(content.items.len());
    for (i, f) in content.items.iter_mut().zip(&fonts) {
        match atomic_box(i, width, f, ctx) {
            Some((b, runs)) => {
                atomic_runs.push(runs);
                atomics.push(Some(b));
            }
            None => {
                atomic_runs.push(Vec::new());
                atomics.push(None);
            }
        }
    }

    let floats = ctx.floats.last();
    let mut b = LineBuilder {
        width,
        full_width: width,
        line_left: 0.0,
        line_started: false,
        origin,
        floats,
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
            Atom::Opportunity => {
                // The next word group may start a new line; nothing to place.
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
                b.start_line();
                if has_content && b.x + space.map_or(0.0, |s| s.1) + w > b.width + 0.01 {
                    b.finish_line();
                } else if let Some((sp, sw)) = space {
                    b.place(sp, sw, Vec::new(), " ");
                }
                b.make_room(w);
                b.place(*item, w, std::mem::take(&mut atomic_runs[*item]), "");
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
                b.start_line();
                if has_content && b.x + space_w + group_width > b.width + 0.01 {
                    b.finish_line();
                } else if let Some((item, w)) = space {
                    b.place(item, w, Vec::new(), " ");
                }
                b.make_room(group_width);
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
                Atom::Opportunity => "/".into(),
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

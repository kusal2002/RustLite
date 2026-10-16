//! Intrinsic sizes (CSS Sizing 3 §5): the min-content and max-content
//! widths of a box's content, for shrink-to-fit widths (floats,
//! inline-blocks, absolutely positioned boxes) and flex bases.
//!
//! Inline content is measured by breaking it into lines at width 0
//! (min-content: the widest unbreakable piece) and at an unbounded width
//! (max-content: the widest forced line), reusing the line breaker so the
//! measurement matches layout exactly.

use rustlite_style::ComputedStyle;
use rustlite_style::properties::{FlexDirection, FlexWrap, Float, TextAlign};

use crate::block::{Floats, frame, length, length_auto, to_content};
use crate::{BlockBox, BlockChildren, InlineContent, LayoutContext, inline};

/// Wide enough for any real line, small enough for exact float arithmetic.
const UNBOUNDED: f32 = 1.0e6;

/// The width the lines of `content` need when broken at `width`.
fn line_extent(
    content: &mut InlineContent,
    style: &ComputedStyle,
    width: f32,
    ctx: &mut LayoutContext<'_>,
) -> f32 {
    // Measure without alignment shifts and without outside floats.
    let mut start_aligned = style.clone();
    start_aligned.text_align = TextAlign::Start;
    ctx.floats.push(Floats::default());
    inline::layout_inline(content, width, &start_aligned, ctx, (0.0, 0.0));
    ctx.floats.pop();
    content
        .lines
        .iter()
        .map(|line| {
            let left = line
                .fragments
                .iter()
                .map(|f| f.rect.x)
                .fold(f32::INFINITY, f32::min);
            let right = line
                .fragments
                .iter()
                .map(|f| f.rect.right())
                .fold(0.0, f32::max);
            if left.is_finite() {
                right - left.min(0.0)
            } else {
                0.0
            }
        })
        .fold(0.0, f32::max)
}

/// `(min-content, max-content)` widths of `b`'s content box.
pub(crate) fn content_widths(b: &mut BlockBox, ctx: &mut LayoutContext<'_>) -> (f32, f32) {
    if let Some(r) = &b.replaced {
        let (w, _) = crate::replaced::content_size(&b.style, r.image.as_deref(), 0.0);
        return (w, w);
    }
    let style = b.style.clone();
    match &mut b.children {
        BlockChildren::Inline(content) => {
            let max = line_extent(content, &style, UNBOUNDED, ctx);
            let min = line_extent(content, &style, 0.0, ctx);
            (min, max.max(min))
        }
        BlockChildren::Blocks(kids) => {
            let (mut min, mut max) = (0.0f32, 0.0f32);
            // Adjacent floats can sit side by side at max-content.
            let mut float_run = 0.0f32;
            for kid in kids.iter_mut() {
                let (kmin, kmax) = outer_widths(kid, ctx);
                min = min.max(kmin);
                if kid.style.float != Float::None {
                    float_run += kmax;
                    max = max.max(float_run);
                } else {
                    float_run = 0.0;
                    max = max.max(kmax);
                }
            }
            (min, max)
        }
        BlockChildren::Flex(items) => {
            let row = matches!(
                style.flex_direction,
                FlexDirection::Row | FlexDirection::RowReverse
            );
            let gap = style.column_gap.map_or(0.0, |g| length(g, 0.0));
            let widths: Vec<(f32, f32)> = items.iter_mut().map(|i| outer_widths(i, ctx)).collect();
            if row {
                let gaps = gap * widths.len().saturating_sub(1) as f32;
                let max = widths.iter().map(|w| w.1).sum::<f32>() + gaps;
                let min = if style.flex_wrap == FlexWrap::Nowrap {
                    widths.iter().map(|w| w.0).sum::<f32>() + gaps
                } else {
                    widths.iter().map(|w| w.0).fold(0.0, f32::max)
                };
                (min, max)
            } else {
                (
                    widths.iter().map(|w| w.0).fold(0.0, f32::max),
                    widths.iter().map(|w| w.1).fold(0.0, f32::max),
                )
            }
        }
        BlockChildren::Empty => (0.0, 0.0),
    }
}

/// `(min-content, max-content)` contributions of `kid` to its parent: its
/// margin box with percentages and `auto` treated as 0 (CSS Sizing 3 §5.2).
pub(crate) fn outer_widths(kid: &mut BlockBox, ctx: &mut LayoutContext<'_>) -> (f32, f32) {
    let s = kid.style.clone();
    let (hframe, _) = frame(&s, 0.0);
    let px = |l| length_auto(l, None).unwrap_or(0.0);
    let margins = px(s.margin[1]) + px(s.margin[3]);
    let (mut min, mut max) = match to_content(&s, length_auto(s.width, None), hframe) {
        Some(w) => (w, w),
        None => content_widths(kid, ctx),
    };
    if let Some(m) = to_content(
        &s,
        s.max_width.and_then(|m| match m {
            rustlite_style::ComputedLength::Px(v) => Some(v),
            rustlite_style::ComputedLength::Percentage(_) => None,
        }),
        hframe,
    ) {
        min = min.min(m);
        max = max.min(m);
    }
    if let Some(m) = to_content(&s, length_auto(s.min_width, None), hframe) {
        min = min.max(m);
        max = max.max(m);
    }
    (min + hframe + margins, max + hframe + margins)
}

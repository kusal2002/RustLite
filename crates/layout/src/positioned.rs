//! Absolutely positioned and fixed boxes (CSS 2.1 §10.3.7, §10.6.4),
//! laid out after normal flow once every box has absolute coordinates.
//!
//! The containing block is the padding box of the nearest positioned
//! ancestor, or the initial containing block (the viewport) when there is
//! none; for `position: fixed` it is always the viewport. With both insets
//! `auto` on an axis the box stays at its static position. Simplifications:
//! `auto` margins resolve to 0 (no centering with both insets set), and the
//! static position inside inline content is the start of its block.

use rustlite_style::properties::Position;

use crate::block::{Forced, frame, layout_block_inner, length, length_auto, to_content};
use crate::geometry::make_absolute;
use crate::{BlockBox, LayoutContext, RectF};

/// Lays out every out-of-flow box under `root`. Returns the lowest bottom
/// edge among absolutely positioned boxes (they extend the scrollable area;
/// fixed boxes do not).
pub(crate) fn layout_out_of_flow(
    root: &mut BlockBox,
    viewport: (f32, f32),
    ctx: &mut LayoutContext<'_>,
) -> f32 {
    let icb = RectF::new(0.0, 0.0, viewport.0, viewport.1);
    let mut bottom = 0.0;
    visit(root, icb, icb, viewport, ctx, &mut bottom, 0);
    bottom
}

fn visit(
    b: &mut BlockBox,
    cb: RectF,
    icb: RectF,
    viewport: (f32, f32),
    ctx: &mut LayoutContext<'_>,
    bottom: &mut f32,
    depth: usize,
) {
    // Bounded like the box tree itself (build caps block depth at 200).
    if depth > 400 {
        return;
    }
    let own_cb = if b.style.is_positioned() {
        b.padding_rect()
    } else {
        cb
    };
    let content = b.content_rect();
    for abs in &mut b.out_of_flow {
        let fixed = abs.style.position == Position::Fixed;
        let containing = if fixed { icb } else { own_cb };
        let static_pos = (
            content.x + abs.static_position.0,
            content.y + abs.static_position.1,
        );
        place(abs, containing, static_pos, viewport, ctx);
        if !fixed {
            *bottom = f32::max(*bottom, abs.rect.bottom() + abs.margin.bottom);
        }
    }
    for child in b.child_boxes_mut() {
        visit(child, own_cb, icb, viewport, ctx, bottom, depth + 1);
    }
}

fn place(
    abs: &mut BlockBox,
    cb: RectF,
    static_pos: (f32, f32),
    viewport: (f32, f32),
    ctx: &mut LayoutContext<'_>,
) {
    let s = abs.style.clone();
    let inset = |i: usize, basis: f32| length_auto(s.inset[i], Some(basis));
    let (top, right, bottom, left) = (
        inset(0, cb.height),
        inset(1, cb.width),
        inset(2, cb.height),
        inset(3, cb.width),
    );
    let margin = |i: usize| length_auto(s.margin[i], Some(cb.width)).unwrap_or(0.0);
    let (mt, mr, mb, ml) = (margin(0), margin(1), margin(2), margin(3));
    let (hframe, vframe) = frame(&s, cb.width);

    let specified_w = to_content(&s, length_auto(s.width, Some(cb.width)), hframe);
    let mut content_w = match (specified_w, left, right) {
        (Some(w), _, _) => w,
        (None, Some(l), Some(r)) => (cb.width - l - r - ml - mr - hframe).max(0.0),
        (None, l, r) => {
            let available = cb.width - l.unwrap_or(0.0) - r.unwrap_or(0.0);
            crate::block::shrink_to_fit_width(abs, available, cb.width, ctx)
        }
    };
    if let Some(max) = to_content(&s, s.max_width.map(|m| length(m, cb.width)), hframe) {
        content_w = content_w.min(max);
    }
    if let Some(min) = to_content(&s, length_auto(s.min_width, Some(cb.width)), hframe) {
        content_w = content_w.max(min);
    }
    let specified_h = to_content(&s, length_auto(s.height, Some(cb.height)), vframe);
    let content_h = specified_h.or(match (top, bottom) {
        (Some(t), Some(b)) => Some((cb.height - t - b - mt - mb - vframe).max(0.0)),
        _ => None,
    });

    let o = layout_block_inner(
        abs,
        cb.width,
        Some(cb.height),
        ctx,
        false,
        (0.0, 0.0),
        Forced {
            content_width: Some(content_w),
            content_height: content_h,
        },
    );
    let (w, h) = (abs.rect.width, o.height);
    let x = match (left, right) {
        (Some(l), _) => cb.x + l + ml,
        (None, Some(r)) => cb.right() - r - mr - w,
        (None, None) => static_pos.0 + ml,
    };
    let y = match (top, bottom) {
        (Some(t), _) => cb.y + t + mt,
        (None, Some(b)) => cb.bottom() - b - mb - h,
        (None, None) => static_pos.1 + mt,
    };
    abs.rect.x = 0.0;
    abs.rect.y = 0.0;
    make_absolute(abs, x, y, viewport);
}

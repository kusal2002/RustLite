use std::collections::HashMap;

use rustlite_dom::NodeId;

use crate::{BlockBox, BlockChildren, InlineItem};

/// A rectangle in CSS px (fractional).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct RectF {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl RectF {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn right(&self) -> f32 {
        self.x + self.width
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.height
    }

    pub fn inset(&self, e: &Edges) -> RectF {
        RectF::new(
            self.x + e.left,
            self.y + e.top,
            (self.width - e.left - e.right).max(0.0),
            (self.height - e.top - e.bottom).max(0.0),
        )
    }

    pub fn union(&self, o: &RectF) -> RectF {
        let x = self.x.min(o.x);
        let y = self.y.min(o.y);
        RectF::new(
            x,
            y,
            self.right().max(o.right()) - x,
            self.bottom().max(o.bottom()) - y,
        )
    }

    /// Integer pixel rectangle with edges rounded independently, so adjacent
    /// boxes share edges without gaps or overlaps.
    pub fn snapped(&self) -> rustlite_paint::Rect {
        let x0 = self.x.round();
        let y0 = self.y.round();
        let x1 = self.right().round();
        let y1 = self.bottom().round();
        // Coordinates are clamped to i32 range; sizes are non-negative.
        rustlite_paint::Rect::new(
            x0.clamp(i32::MIN as f32, i32::MAX as f32) as i32,
            y0.clamp(i32::MIN as f32, i32::MAX as f32) as i32,
            (x1 - x0).max(0.0) as u32,
            (y1 - y0).max(0.0) as u32,
        )
    }
}

/// Per-side lengths in px (top, right, bottom, left).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Edges {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

impl Edges {
    pub fn horizontal(&self) -> f32 {
        self.left + self.right
    }

    pub fn vertical(&self) -> f32 {
        self.top + self.bottom
    }
}

/// `position: relative` offset of `b` (CSS 2.1 §9.4.3): `left` wins over
/// `right`, `top` over `bottom`; percentages refer to the containing block.
fn relative_offset(b: &BlockBox, cb_width: f32, cb_height: Option<f32>) -> (f32, f32) {
    use rustlite_style::properties::Position;
    if !matches!(b.style.position, Position::Relative | Position::Sticky) {
        return (0.0, 0.0);
    }
    let resolve = |l: rustlite_style::ComputedLengthAuto, basis: Option<f32>| match l {
        rustlite_style::ComputedLengthAuto::Auto => None,
        rustlite_style::ComputedLengthAuto::Length(rustlite_style::ComputedLength::Px(v)) => {
            Some(v)
        }
        rustlite_style::ComputedLengthAuto::Length(rustlite_style::ComputedLength::Percentage(
            p,
        )) => basis.map(|b| b * p / 100.0),
    };
    let inset = &b.style.inset;
    let dx = resolve(inset[3], Some(cb_width))
        .or_else(|| resolve(inset[1], Some(cb_width)).map(|r| -r))
        .unwrap_or(0.0);
    let dy = resolve(inset[0], cb_height)
        .or_else(|| resolve(inset[2], cb_height).map(|v| -v))
        .unwrap_or(0.0);
    (dx, dy)
}

/// Converts relative positions (border box relative to the parent's content
/// box; lines and fragments relative to their block's content box) into
/// absolute document coordinates, applying relative-position offsets.
/// Out-of-flow boxes are placed later (see `positioned`). Iterative, so deep
/// trees are safe.
pub(crate) fn make_absolute(
    root: &mut BlockBox,
    origin_x: f32,
    origin_y: f32,
    viewport: (f32, f32),
) {
    // (box, origin x, origin y, containing block width, height)
    let mut stack: Vec<(&mut BlockBox, f32, f32, f32, Option<f32>)> =
        vec![(root, origin_x, origin_y, viewport.0, Some(viewport.1))];
    while let Some((b, ox, oy, cb_w, cb_h)) = stack.pop() {
        let (dx, dy) = relative_offset(b, cb_w, cb_h);
        b.rect.x += ox + dx;
        b.rect.y += oy + dy;
        let content = b.content_rect();
        if let Some(m) = &mut b.marker {
            m.x += content.x;
            m.baseline += content.y;
        }
        let definite_height = match b.style.height {
            rustlite_style::ComputedLengthAuto::Auto => None,
            _ => Some(content.height),
        };
        match &mut b.children {
            BlockChildren::Blocks(kids) | BlockChildren::Flex(kids) => {
                for k in kids.iter_mut() {
                    stack.push((k, content.x, content.y, content.width, definite_height));
                }
            }
            BlockChildren::Inline(inline) => {
                let mut atomic_origins: Vec<(usize, f32, f32)> = Vec::new();
                for line in &mut inline.lines {
                    line.rect.x += content.x;
                    line.rect.y += content.y;
                    for f in &mut line.fragments {
                        f.rect.x += content.x;
                        f.rect.y += content.y;
                        f.baseline += content.y;
                        if matches!(
                            inline.items[f.item],
                            InlineItem::Atomic { block: Some(_), .. }
                        ) {
                            atomic_origins.push((f.item, f.rect.x, f.rect.y));
                        }
                    }
                }
                for (index, item) in inline.items.iter_mut().enumerate() {
                    if let InlineItem::Atomic {
                        block: Some(inner), ..
                    } = item
                        && let Some(&(_, x, y)) = atomic_origins.iter().find(|o| o.0 == index)
                    {
                        // The fragment is the inline-block's border box.
                        inner.rect.x = 0.0;
                        inner.rect.y = 0.0;
                        stack.push((inner, x, y, content.width, None));
                    }
                }
            }
            BlockChildren::Empty => {}
        }
    }
}

pub(crate) fn collect_rects(root: &BlockBox, out: &mut HashMap<NodeId, RectF>) {
    let mut stack = vec![root];
    while let Some(b) = stack.pop() {
        if let Some(n) = b.node {
            out.insert(n, b.rect);
        }
        if let BlockChildren::Inline(inline) = &b.children {
            for f in inline.lines.iter().flat_map(|l| &l.fragments) {
                match &inline.items[f.item] {
                    InlineItem::Text { ancestors, .. } => {
                        for &a in ancestors.iter() {
                            out.entry(a)
                                .and_modify(|r| *r = r.union(&f.rect))
                                .or_insert(f.rect);
                        }
                    }
                    InlineItem::LineBreak { node, .. } | InlineItem::Atomic { node, .. } => {
                        out.insert(*node, f.rect);
                    }
                }
            }
        }
        stack.extend(b.child_boxes());
    }
}

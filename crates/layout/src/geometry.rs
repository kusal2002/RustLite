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

/// Converts relative positions (border box relative to the parent's content
/// box; lines and fragments relative to their block's content box) into
/// absolute document coordinates. Iterative, so deep trees are safe.
pub(crate) fn make_absolute(root: &mut BlockBox, origin_x: f32, origin_y: f32) {
    let mut stack: Vec<(&mut BlockBox, f32, f32)> = vec![(root, origin_x, origin_y)];
    while let Some((b, ox, oy)) = stack.pop() {
        b.rect.x += ox;
        b.rect.y += oy;
        let content = b.content_rect();
        if let Some(m) = &mut b.marker {
            m.x += content.x;
            m.baseline += content.y;
        }
        match &mut b.children {
            BlockChildren::Blocks(kids) => {
                for k in kids.iter_mut() {
                    stack.push((k, content.x, content.y));
                }
            }
            BlockChildren::Inline(inline) => {
                for line in &mut inline.lines {
                    line.rect.x += content.x;
                    line.rect.y += content.y;
                    for f in &mut line.fragments {
                        f.rect.x += content.x;
                        f.rect.y += content.y;
                        f.baseline += content.y;
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
        match &b.children {
            BlockChildren::Blocks(kids) => stack.extend(kids.iter()),
            BlockChildren::Inline(inline) => {
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
            BlockChildren::Empty => {}
        }
    }
}

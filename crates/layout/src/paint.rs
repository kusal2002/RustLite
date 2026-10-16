//! Painting the laid-out tree into a display list (CSS 2.1 Appendix E,
//! simplified to normal flow: background, borders, then content per box).

use rustlite_dom::NodeId;
use rustlite_paint::{Color, DisplayItem, DisplayList};
use rustlite_style::Rgba;
use rustlite_style::properties::{BorderStyle, Overflow};

use crate::{BlockBox, BlockChildren, InlineItem, LayoutTree, RectF};

fn color(c: Rgba) -> Color {
    // Alpha is clamped to 0..=1 at parse time.
    Color::rgba(c.r, c.g, c.b, (c.a * 255.0).round() as u8)
}

/// The canvas background (CSS Backgrounds 3 §2.11.2): the root element's
/// background, or the body's if the root's is transparent. Returns the color
/// and which box's background was propagated (so it is not painted twice).
pub fn canvas_background(tree: &LayoutTree) -> (Color, Option<NodeId>) {
    let Some(root) = &tree.root else {
        return (Color::WHITE, None);
    };
    if root.style.background_color.a > 0.0 {
        return (color(root.style.background_color), root.node);
    }
    if let BlockChildren::Blocks(kids) = &root.children
        && let Some(body) = kids.iter().find(|k| k.node.is_some())
        && body.style.background_color.a > 0.0
    {
        return (color(body.style.background_color), body.node);
    }
    (Color::WHITE, None)
}

/// Paints `tree` into `out`. Layout is in CSS px; `scale` (the device pixel
/// ratio) converts to device px, and `scroll_y` (CSS px) shifts content up.
pub fn paint(tree: &LayoutTree, scroll_y: f32, scale: f32, out: &mut DisplayList) {
    let Some(root) = &tree.root else { return };
    let (bg, propagated) = canvas_background(tree);
    let p = Painter {
        dy: -scroll_y,
        scale,
        propagated,
    };
    // Canvas background covers the whole viewport.
    let viewport = RectF::new(0.0, 0.0, tree.viewport.0, tree.viewport.1);
    out.fill_rect(p.device(viewport, false), bg);
    p.block(root, out);
}

struct Painter {
    dy: f32,
    scale: f32,
    propagated: Option<NodeId>,
}

impl Painter {
    /// Converts a CSS-px rect in document space to device px in viewport space.
    fn device(&self, r: RectF, scrolls: bool) -> rustlite_paint::Rect {
        let dy = if scrolls { self.dy } else { 0.0 };
        let s = self.scale;
        RectF::new(r.x * s, (r.y + dy) * s, r.width * s, r.height * s).snapped()
    }

    fn block(&self, b: &BlockBox, out: &mut DisplayList) {
        if self.propagated.is_none() || b.node != self.propagated {
            out.fill_rect(self.device(b.rect, true), color(b.style.background_color));
        }
        self.borders(b, out);

        let clips = !matches!(b.style.overflow_x, Overflow::Visible)
            || !matches!(b.style.overflow_y, Overflow::Visible);
        if clips {
            out.push_clip(self.device(b.padding_rect(), true));
        }
        match &b.children {
            BlockChildren::Blocks(kids) => {
                for k in kids {
                    self.block(k, out);
                }
            }
            BlockChildren::Inline(inline) => {
                for f in inline.lines.iter().flat_map(|l| &l.fragments) {
                    let InlineItem::Text { style, .. } = &inline.items[f.item] else {
                        continue;
                    };
                    let c = color(style.color);
                    for (dx, run) in &f.runs {
                        out.push(DisplayItem::Text {
                            x: (f.rect.x + dx) * self.scale,
                            y: (f.baseline + self.dy) * self.scale,
                            scale: self.scale,
                            color: c,
                            run: run.clone(),
                        });
                    }
                }
            }
            BlockChildren::Empty => {}
        }
        if clips {
            out.pop_clip();
        }
    }

    /// Paints each border side as a solid band (other styles are drawn solid
    /// for now; dashed/dotted/double/3D styles are a known gap).
    fn borders(&self, b: &BlockBox, out: &mut DisplayList) {
        let s = &b.style;
        let e = &b.border;
        let r = b.rect;
        let visible =
            |i: usize| !matches!(s.border_style[i], BorderStyle::None | BorderStyle::Hidden);
        let inner_height = r.height - e.top - e.bottom;
        let sides = [
            (0, RectF::new(r.x, r.y, r.width, e.top)),
            (2, RectF::new(r.x, r.bottom() - e.bottom, r.width, e.bottom)),
            (3, RectF::new(r.x, r.y + e.top, e.left, inner_height)),
            (
                1,
                RectF::new(r.right() - e.right, r.y + e.top, e.right, inner_height),
            ),
        ];
        for (i, rect) in sides {
            if visible(i) {
                out.fill_rect(self.device(rect, true), color(s.border_color[i]));
            }
        }
    }
}

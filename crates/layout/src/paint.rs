//! Painting the laid-out tree into a display list (CSS 2.1 Appendix E,
//! simplified to normal flow: background, borders, then content per box).

use rustlite_dom::NodeId;
use rustlite_paint::{Color, DisplayItem, DisplayList};
use rustlite_style::Rgba;
use rustlite_style::properties::{BorderStyle, Overflow, Position};

use crate::{BlockBox, BlockChildren, InlineContent, InlineItem, LayoutTree, LineBox, RectF};

/// Chromium's default accent color for checked toggles.
const ACCENT: Color = Color::rgb(0, 117, 255);
const TOGGLE_BORDER: Color = Color::rgb(118, 118, 118);
/// Placeholder text (Chromium's `darkGray`).
const PLACEHOLDER: Color = Color::rgb(117, 117, 117);

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
///
/// Only content that can be visible in the viewport is emitted: a long page
/// paints and rasterizes in time proportional to what is on screen, not to
/// the document's length. Callers repaint when they scroll.
pub fn paint(tree: &LayoutTree, scroll_y: f32, scale: f32, out: &mut DisplayList) {
    let Some(root) = &tree.root else { return };
    let (bg, propagated) = canvas_background(tree);
    let p = Painter {
        dy: -scroll_y,
        scale,
        propagated,
        visible: (scroll_y, scroll_y + tree.viewport.1),
    };
    // Canvas background covers the whole viewport.
    let viewport = RectF::new(0.0, 0.0, tree.viewport.0, tree.viewport.1);
    out.fill_rect(p.device(viewport, false), bg);
    let mut deferred: Vec<&BlockBox> = Vec::new();
    p.block(root, out, &mut deferred);
    // Positioned boxes paint after normal flow (CSS 2.1 Appendix E,
    // simplified): in z-index order, ties in tree order; boxes nested in a
    // positioned box follow it. Fixed boxes ignore scrolling.
    let fixed_painter = Painter {
        dy: 0.0,
        visible: (0.0, tree.viewport.1),
        ..p
    };
    while !deferred.is_empty() {
        deferred.sort_by_key(|b| b.style.z_index.unwrap_or(0));
        let mut next = Vec::new();
        for b in deferred {
            let painter = if b.style.position == Position::Fixed {
                &fixed_painter
            } else {
                &p
            };
            painter.block(b, out, &mut next);
        }
        deferred = next;
    }
}

#[derive(Clone, Copy)]
struct Painter {
    dy: f32,
    scale: f32,
    propagated: Option<NodeId>,
    /// Visible document range in CSS px (top, bottom).
    visible: (f32, f32),
}

impl Painter {
    /// Could anything drawn for `r` show? Text glyphs may overhang their
    /// fragment (tall ascenders, diacritics), so the test allows one rect
    /// height of slack on both sides.
    fn may_show(&self, r: RectF, slack: f32) -> bool {
        r.bottom() + slack >= self.visible.0 && r.y - slack <= self.visible.1
    }

    /// Converts a CSS-px rect in document space to device px in viewport space.
    fn device(&self, r: RectF, scrolls: bool) -> rustlite_paint::Rect {
        let dy = if scrolls { self.dy } else { 0.0 };
        let s = self.scale;
        RectF::new(r.x * s, (r.y + dy) * s, r.width * s, r.height * s).snapped()
    }

    fn block<'a>(&self, b: &'a BlockBox, out: &mut DisplayList, deferred: &mut Vec<&'a BlockBox>) {
        let clips = !matches!(b.style.overflow_x, Overflow::Visible)
            || !matches!(b.style.overflow_y, Overflow::Visible);
        let shows = self.may_show(b.rect, 0.0);
        // Descendants can overflow a block, so an off-screen block's children
        // are still visited, unless the block clips them.
        if !shows && clips {
            return;
        }
        if shows && (self.propagated.is_none() || b.node != self.propagated) {
            out.fill_rect(self.device(b.rect, true), color(b.style.background_color));
        }
        if shows {
            self.borders(b, out);
        }
        if let Some(m) = b.marker.as_ref().filter(|_| shows) {
            let c = color(b.style.color);
            for (dx, run) in &m.runs {
                out.push(DisplayItem::Text {
                    x: (m.x + dx) * self.scale,
                    y: (m.baseline + self.dy) * self.scale,
                    scale: self.scale,
                    color: c,
                    run: run.clone(),
                });
            }
        }

        if clips {
            out.push_clip(self.device(b.padding_rect(), true));
        }
        match &b.children {
            BlockChildren::Blocks(kids) | BlockChildren::Flex(kids) => {
                for k in kids {
                    if k.style.is_positioned() {
                        deferred.push(k);
                    } else {
                        self.block(k, out, deferred);
                    }
                }
            }
            BlockChildren::Inline(inline) => {
                for line in &inline.lines {
                    if !self.may_show(line.rect, line.rect.height) {
                        continue;
                    }
                    self.line(inline, line, out, deferred);
                }
            }
            BlockChildren::Empty => {
                if let Some(r) = &b.replaced
                    && let Some(image) = &r.image
                    && shows
                {
                    out.push(DisplayItem::Image {
                        rect: self.device(b.content_rect(), true),
                        image: image.clone(),
                    });
                }
            }
        }
        if clips {
            out.pop_clip();
        }
        deferred.extend(b.out_of_flow.iter());
    }

    /// Paints one line box's text and atomic inlines.
    fn line<'a>(
        &self,
        inline: &'a InlineContent,
        line: &LineBox,
        out: &mut DisplayList,
        deferred: &mut Vec<&'a BlockBox>,
    ) {
        for f in &line.fragments {
            let style = match &inline.items[f.item] {
                InlineItem::Atomic {
                    block: Some(inner), ..
                } => {
                    if inner.style.is_positioned() {
                        deferred.push(inner);
                    } else {
                        self.block(inner, out, deferred);
                    }
                    continue;
                }
                InlineItem::Text { style, .. } => style,
                InlineItem::Atomic {
                    style,
                    control: Some(control),
                    ..
                } => {
                    self.control(f, style, control, out);
                    continue;
                }
                InlineItem::Atomic { style, image, .. } => {
                    self.replaced(f.rect, style, image.as_ref(), out);
                    continue;
                }
                InlineItem::LineBreak { .. } => continue,
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

    /// Paints an atomic inline replaced element given its border box.
    fn replaced(
        &self,
        border_box: RectF,
        style: &rustlite_style::ComputedStyle,
        image: Option<&std::rc::Rc<rustlite_paint::ImageData>>,
        out: &mut DisplayList,
    ) {
        out.fill_rect(self.device(border_box, true), color(style.background_color));
        let Some(image) = image else { return };
        let inset = |i: usize| {
            style.border_width[i]
                + match style.padding[i] {
                    rustlite_style::ComputedLength::Px(v) => v,
                    // Percentages need the containing block; the fragment
                    // already includes them, so derive content from the box.
                    rustlite_style::ComputedLength::Percentage(_) => 0.0,
                }
        };
        let content = RectF::new(
            border_box.x + inset(3),
            border_box.y + inset(0),
            (border_box.width - inset(1) - inset(3)).max(0.0),
            (border_box.height - inset(0) - inset(2)).max(0.0),
        );
        out.push(DisplayItem::Image {
            rect: self.device(content, true),
            image: image.clone(),
        });
    }

    /// Paints each border side according to its style. `solid`, `groove`,
    /// `ridge`, `inset` and `outset` are drawn as solid bands (3D shading is a
    /// known gap); `dashed`, `dotted` and `double` are patterned.
    fn borders(&self, b: &BlockBox, out: &mut DisplayList) {
        self.border_box(b.rect, &b.style, &b.border, out);
    }

    /// Paints a form control from its border-box fragment.
    fn control(
        &self,
        f: &crate::TextFragment,
        style: &rustlite_style::ComputedStyle,
        control: &crate::Control,
        out: &mut DisplayList,
    ) {
        use rustlite_dom::ControlKind;
        let r = f.rect;
        if matches!(control.kind, ControlKind::Checkbox | ControlKind::Radio) {
            // Chromium-like toggles: a white box with a gray outline, filled
            // with the accent color when checked.
            let (fill, edge) = if control.checked {
                (ACCENT, ACCENT)
            } else {
                (Color::WHITE, TOGGLE_BORDER)
            };
            let device = self.device(r, true);
            let line = (self.scale.round() as u32).max(1);
            out.fill_rect(device, fill);
            out.stroke_rect(device, line, edge);
            if control.checked {
                let inset = (device.width / 4).max(1);
                let mark = rustlite_paint::Rect::new(
                    device.x + inset as i32,
                    device.y + inset as i32,
                    device.width.saturating_sub(2 * inset),
                    device.height.saturating_sub(2 * inset),
                );
                if control.kind == ControlKind::Radio {
                    out.fill_rect(mark, Color::WHITE);
                    let dot = (mark.width / 4).max(1);
                    out.fill_rect(
                        rustlite_paint::Rect::new(
                            mark.x + dot as i32,
                            mark.y + dot as i32,
                            mark.width.saturating_sub(2 * dot),
                            mark.height.saturating_sub(2 * dot),
                        ),
                        ACCENT,
                    );
                } else {
                    out.fill_rect(mark, Color::WHITE);
                }
            }
            return;
        }
        out.fill_rect(self.device(r, true), color(style.background_color));
        let edges = crate::Edges {
            top: style.border_width[0],
            right: style.border_width[1],
            bottom: style.border_width[2],
            left: style.border_width[3],
        };
        self.border_box(r, style, &edges, out);
        // Text stays inside the content box (inputs clip their overflow).
        let content = RectF::new(
            r.x + edges.left,
            r.y + edges.top,
            (r.width - edges.left - edges.right).max(0.0),
            (r.height - edges.top - edges.bottom).max(0.0),
        );
        let text_color = if control.placeholder {
            PLACEHOLDER
        } else {
            color(style.color)
        };
        out.push_clip(self.device(content, true));
        for (dx, run) in &f.runs {
            out.push(DisplayItem::Text {
                x: (r.x + dx) * self.scale,
                y: (f.baseline + self.dy) * self.scale,
                scale: self.scale,
                color: text_color,
                run: run.clone(),
            });
        }
        out.pop_clip();
    }

    /// Paints the four borders of a border box `r` with `style`'s border
    /// styles and colors.
    fn border_box(
        &self,
        r: RectF,
        s: &rustlite_style::ComputedStyle,
        e: &crate::Edges,
        out: &mut DisplayList,
    ) {
        let inner_height = r.height - e.top - e.bottom;
        // (side index, band rect, thickness, horizontal?)
        let sides = [
            (0, RectF::new(r.x, r.y, r.width, e.top), e.top, true),
            (
                2,
                RectF::new(r.x, r.bottom() - e.bottom, r.width, e.bottom),
                e.bottom,
                true,
            ),
            (
                3,
                RectF::new(r.x, r.y + e.top, e.left, inner_height),
                e.left,
                false,
            ),
            (
                1,
                RectF::new(r.right() - e.right, r.y + e.top, e.right, inner_height),
                e.right,
                false,
            ),
        ];
        let widths = [e.top, e.right, e.bottom, e.left];
        for (i, band, thickness, horizontal) in sides {
            let c = color(s.border_color[i]);
            let pieces = if s.border_style[i] == BorderStyle::Double && thickness >= 3.0 {
                double_lines(r, widths, s.border_style, i).to_vec()
            } else {
                border_pieces(s.border_style[i], band, thickness, horizontal)
            };
            for piece in pieces {
                out.fill_rect(self.device(piece, true), c);
            }
        }
    }
}

/// The two lines of a `double` border side (thickness ≥ 3px), as part of an
/// outer ring at the border edge and an inner ring inset by
/// `width - line` on every side, so lines meet properly at the corners.
/// Each line is a third of the side's width, rounded down.
fn double_lines(r: RectF, widths: [f32; 4], styles: [BorderStyle; 4], side: usize) -> [RectF; 2] {
    // Line width per side; non-double sides contribute no inset.
    let line = |i: usize| {
        if styles[i] == BorderStyle::Double && widths[i] >= 3.0 {
            (widths[i] / 3.0).floor().max(1.0)
        } else {
            widths[i]
        }
    };
    let inset = |i: usize| widths[i] - line(i);
    let inner = RectF::new(
        r.x + inset(3),
        r.y + inset(0),
        (r.width - inset(3) - inset(1)).max(0.0),
        (r.height - inset(0) - inset(2)).max(0.0),
    );
    let l = line(side);
    let ring_side = |ring: RectF| match side {
        0 => RectF::new(ring.x, ring.y, ring.width, l),
        2 => RectF::new(ring.x, ring.bottom() - l, ring.width, l),
        3 => RectF::new(ring.x, ring.y, l, ring.height),
        _ => RectF::new(ring.right() - l, ring.y, l, ring.height),
    };
    [ring_side(r), ring_side(inner)]
}

/// Splits one border side's band into the rectangles its style paints
/// (`double` sides of 3px or more are handled by [`double_lines`]).
fn border_pieces(style: BorderStyle, band: RectF, thickness: f32, horizontal: bool) -> Vec<RectF> {
    if thickness <= 0.0 || band.width <= 0.0 || band.height <= 0.0 {
        return Vec::new();
    }
    match style {
        BorderStyle::None | BorderStyle::Hidden => Vec::new(),
        BorderStyle::Dashed | BorderStyle::Dotted => {
            let (dash, gap) = if style == BorderStyle::Dotted {
                (thickness, thickness)
            } else {
                (thickness * 3.0, thickness * 3.0)
            };
            let length = if horizontal { band.width } else { band.height };
            let mut pieces = Vec::new();
            let mut pos = 0.0;
            while pos < length {
                let len = dash.min(length - pos);
                pieces.push(if horizontal {
                    RectF::new(band.x + pos, band.y, len, band.height)
                } else {
                    RectF::new(band.x, band.y + pos, band.width, len)
                });
                pos += dash + gap;
            }
            pieces
        }
        _ => vec![band],
    }
}

#[cfg(test)]
mod border_tests {
    use super::*;

    #[test]
    fn solid_and_none() {
        let band = RectF::new(0.0, 0.0, 100.0, 4.0);
        assert_eq!(border_pieces(BorderStyle::Solid, band, 4.0, true), [band]);
        assert!(border_pieces(BorderStyle::None, band, 4.0, true).is_empty());
        assert!(border_pieces(BorderStyle::Solid, band, 0.0, true).is_empty());
    }

    #[test]
    fn dashed_and_dotted_patterns() {
        let band = RectF::new(0.0, 0.0, 20.0, 2.0);
        let dashes = border_pieces(BorderStyle::Dashed, band, 2.0, true);
        // 6px dash, 6px gap: dashes at 0 and 12 (the second truncated to 6).
        assert_eq!(
            dashes,
            [
                RectF::new(0.0, 0.0, 6.0, 2.0),
                RectF::new(12.0, 0.0, 6.0, 2.0)
            ]
        );
        let dots = border_pieces(BorderStyle::Dotted, band, 2.0, true);
        assert_eq!(dots.len(), 5);
        assert!(dots.iter().all(|d| d.width == 2.0));
        let vertical = border_pieces(
            BorderStyle::Dotted,
            RectF::new(0.0, 0.0, 2.0, 8.0),
            2.0,
            false,
        );
        assert_eq!(
            vertical,
            [
                RectF::new(0.0, 0.0, 2.0, 2.0),
                RectF::new(0.0, 4.0, 2.0, 2.0)
            ]
        );
    }

    #[test]
    fn double_borders_are_two_nested_rings() {
        let r = RectF::new(0.0, 0.0, 30.0, 20.0);
        let d = BorderStyle::Double;
        let styles = [d; 4];
        let widths = [6.0; 4];
        // Lines are 2px; the inner ring is inset by 4px on every side.
        assert_eq!(
            double_lines(r, widths, styles, 0),
            [
                RectF::new(0.0, 0.0, 30.0, 2.0),
                RectF::new(4.0, 4.0, 22.0, 2.0)
            ]
        );
        assert_eq!(
            double_lines(r, widths, styles, 1),
            [
                RectF::new(28.0, 0.0, 2.0, 20.0),
                RectF::new(24.0, 4.0, 2.0, 12.0)
            ]
        );
        let thin = RectF::new(0.0, 0.0, 50.0, 2.0);
        assert_eq!(border_pieces(BorderStyle::Double, thin, 2.0, true), [thin]);
    }
}

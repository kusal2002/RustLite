//! CPU rasterizer for RustLite display lists.
//!
//! Pixels are stored as `u32` in `0x00RRGGBB` order, which is the format
//! `softbuffer` presents on every supported platform. Rendering directly into
//! the presentation buffer avoids an extra full-frame copy per frame.
//!
//! A GPU backend (wgpu) is planned; see `research/decisions/ADR-0001`.

mod canvas;
mod png;

pub use canvas::{Canvas, CanvasError, Framebuffer, pack_rgb, unpack_rgb};
pub use png::{PngError, encode_png};

use rustlite_paint::{Color, DisplayItem, DisplayList, GlyphRasterizer, Rect};

/// Upper bound on nested clips. Deeper nesting is a bug in the paint stage, not
/// something web content should be able to force us to allocate for.
const MAX_CLIP_DEPTH: usize = 256;

/// Statistics about one rasterization pass, for diagnostics and tests.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RasterStats {
    pub items: usize,
    pub pixels_written: u64,
    /// `PopClip` without a matching `PushClip`, or pushes beyond `MAX_CLIP_DEPTH`.
    pub unbalanced_clips: usize,
}

/// Paints `list` onto `canvas` in order, after clearing it to `background`.
/// Text items get their glyph bitmaps from `glyphs`.
pub fn rasterize(
    canvas: &mut Canvas<'_>,
    list: &DisplayList,
    background: Color,
    glyphs: &mut dyn GlyphRasterizer,
) -> RasterStats {
    let bounds = canvas.bounds();
    let mut stats = RasterStats::default();
    stats.pixels_written += canvas.fill(bounds, background);

    let mut clip_stack: Vec<Option<Rect>> = Vec::new();
    let mut clip = Some(bounds);

    for item in list.items() {
        stats.items += 1;
        match *item {
            DisplayItem::Text {
                x,
                y,
                scale,
                color,
                ref run,
            } => {
                let Some(clip) = clip else { continue };
                let size = run.size * scale;
                for g in run.glyphs.iter() {
                    let Some(bitmap) = glyphs.rasterize(run.font, g.id, size) else {
                        continue;
                    };
                    // Glyph origins snap to whole pixels (no subpixel positioning).
                    let ox = (x + g.x * scale).round() as i32 + bitmap.left;
                    let oy = (y + g.y * scale).round() as i32 + bitmap.top;
                    stats.pixels_written += canvas.blend_coverage(ox, oy, &bitmap, color, clip);
                }
            }
            DisplayItem::SolidRect { rect, color } => {
                if let Some(target) = clip.and_then(|c| c.intersect(rect)) {
                    stats.pixels_written += canvas.fill(target, color);
                }
            }
            DisplayItem::Border { rect, width, color } => {
                if let Some(clip) = clip {
                    stats.pixels_written += stroke(canvas, rect, width, color, clip);
                }
            }
            DisplayItem::PushClip { rect } => {
                if clip_stack.len() >= MAX_CLIP_DEPTH {
                    stats.unbalanced_clips += 1;
                    continue;
                }
                clip_stack.push(clip);
                clip = clip.and_then(|c| c.intersect(rect));
            }
            DisplayItem::PopClip => match clip_stack.pop() {
                Some(previous) => clip = previous,
                None => stats.unbalanced_clips += 1,
            },
        }
    }

    stats.unbalanced_clips += clip_stack.len();
    if stats.unbalanced_clips > 0 {
        tracing::warn!(
            count = stats.unbalanced_clips,
            "display list has unbalanced clips"
        );
    }
    stats
}

fn stroke(canvas: &mut Canvas<'_>, rect: Rect, width: u32, color: Color, clip: Rect) -> u64 {
    // Clamp each edge to the space the opposite edge leaves, so edges never
    // overlap; overlapping edges would double-blend translucent borders.
    let top = width.min(rect.height);
    let bottom = width.min(rect.height - top);
    let left = width.min(rect.width);
    let right = width.min(rect.width - left);
    let side_height = rect.height - top - bottom;
    let side_y = rect.y.saturating_add_unsigned(top);
    let edges = [
        Rect::new(rect.x, rect.y, rect.width, top),
        Rect::new(
            rect.x,
            rect.y.saturating_add_unsigned(rect.height - bottom),
            rect.width,
            bottom,
        ),
        Rect::new(rect.x, side_y, left, side_height),
        Rect::new(
            rect.x.saturating_add_unsigned(rect.width - right),
            side_y,
            right,
            side_height,
        ),
    ];
    edges
        .into_iter()
        .filter_map(|edge| clip.intersect(edge))
        .map(|target| canvas.fill(target, color))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(width: u32, height: u32, list: &DisplayList) -> (Framebuffer, RasterStats) {
        let mut fb = Framebuffer::new(width, height).unwrap();
        let stats = rasterize(
            &mut fb.canvas(),
            list,
            Color::WHITE,
            &mut rustlite_paint::NoGlyphs,
        );
        (fb, stats)
    }

    #[test]
    fn clears_to_background() {
        let (fb, stats) = render(4, 3, &DisplayList::new());
        assert!(fb.pixels().iter().all(|&p| p == 0x00FF_FFFF));
        assert_eq!(stats.pixels_written, 12);
    }

    #[test]
    fn solid_rect_is_clipped_to_canvas() {
        let mut list = DisplayList::new();
        list.fill_rect(Rect::new(-2, -2, 4, 4), Color::rgb(255, 0, 0));
        let (fb, _) = render(4, 4, &list);
        assert_eq!(fb.pixel(0, 0), Some(Color::rgb(255, 0, 0)));
        assert_eq!(fb.pixel(1, 1), Some(Color::rgb(255, 0, 0)));
        assert_eq!(fb.pixel(2, 2), Some(Color::WHITE));
    }

    #[test]
    fn border_draws_only_edges() {
        let mut list = DisplayList::new();
        list.stroke_rect(Rect::new(0, 0, 5, 5), 1, Color::BLACK);
        let (fb, stats) = render(5, 5, &list);
        assert_eq!(fb.pixel(0, 0), Some(Color::BLACK));
        assert_eq!(fb.pixel(4, 4), Some(Color::BLACK));
        assert_eq!(fb.pixel(0, 2), Some(Color::BLACK));
        assert_eq!(fb.pixel(2, 2), Some(Color::WHITE));
        // 25 background pixels + 16 perimeter pixels, each written exactly once.
        assert_eq!(stats.pixels_written, 25 + 16);
    }

    #[test]
    fn translucent_border_edges_do_not_overlap() {
        let mut list = DisplayList::new();
        list.stroke_rect(Rect::new(0, 0, 3, 3), 5, Color::rgba(0, 0, 0, 128));
        let (fb, _) = render(3, 3, &list);
        let first = fb.pixel(0, 0).unwrap();
        for y in 0..3 {
            for x in 0..3 {
                assert_eq!(fb.pixel(x, y), Some(first), "pixel ({x},{y}) blended twice");
            }
        }
    }

    #[test]
    fn clip_restricts_and_restores() {
        let mut list = DisplayList::new();
        list.push_clip(Rect::new(0, 0, 2, 2));
        list.fill_rect(Rect::new(0, 0, 4, 4), Color::BLACK);
        list.pop_clip();
        list.fill_rect(Rect::new(3, 3, 1, 1), Color::BLACK);
        let (fb, stats) = render(4, 4, &list);
        assert_eq!(fb.pixel(1, 1), Some(Color::BLACK));
        assert_eq!(fb.pixel(2, 2), Some(Color::WHITE));
        assert_eq!(fb.pixel(3, 3), Some(Color::BLACK));
        assert_eq!(stats.unbalanced_clips, 0);
    }

    #[test]
    fn unbalanced_clips_are_counted_not_fatal() {
        let mut list = DisplayList::new();
        list.pop_clip();
        list.push_clip(Rect::new(0, 0, 1, 1));
        let (_, stats) = render(2, 2, &list);
        assert_eq!(stats.unbalanced_clips, 2);
    }

    /// Rasterizer that returns a solid 2×3 block for every glyph.
    struct BlockGlyphs;

    impl GlyphRasterizer for BlockGlyphs {
        fn rasterize(
            &mut self,
            _: rustlite_paint::FontId,
            _: u16,
            _: f32,
        ) -> Option<std::rc::Rc<rustlite_paint::GlyphBitmap>> {
            Some(std::rc::Rc::new(rustlite_paint::GlyphBitmap {
                width: 2,
                height: 3,
                left: 0,
                top: -3,
                coverage: vec![255; 6],
            }))
        }
    }

    #[test]
    fn text_blends_glyph_coverage_at_baseline() {
        use rustlite_paint::{FontId, GlyphRun, PositionedGlyph};
        let mut list = DisplayList::new();
        list.push(DisplayItem::Text {
            x: 1.0,
            y: 4.0,
            scale: 1.0,
            color: Color::BLACK,
            run: GlyphRun {
                font: FontId(0),
                size: 10.0,
                glyphs: vec![
                    PositionedGlyph {
                        id: 1,
                        x: 0.0,
                        y: 0.0,
                    },
                    PositionedGlyph {
                        id: 2,
                        x: 3.0,
                        y: 0.0,
                    },
                ]
                .into(),
            },
        });
        let mut fb = Framebuffer::new(8, 6).unwrap();
        let stats = rasterize(&mut fb.canvas(), &list, Color::WHITE, &mut BlockGlyphs);
        // Glyph 1 covers x 1..3, y 1..4; glyph 2 covers x 4..6.
        assert_eq!(fb.pixel(1, 1), Some(Color::BLACK));
        assert_eq!(fb.pixel(2, 3), Some(Color::BLACK));
        assert_eq!(fb.pixel(3, 2), Some(Color::WHITE));
        assert_eq!(fb.pixel(5, 2), Some(Color::BLACK));
        assert_eq!(
            fb.pixel(1, 4),
            Some(Color::WHITE),
            "nothing below the baseline"
        );
        assert_eq!(stats.pixels_written, 48 + 12);
    }

    #[test]
    fn text_respects_clip() {
        use rustlite_paint::{FontId, GlyphRun, PositionedGlyph};
        let mut list = DisplayList::new();
        list.push_clip(Rect::new(0, 0, 2, 6));
        list.push(DisplayItem::Text {
            x: 1.0,
            y: 4.0,
            scale: 1.0,
            color: Color::BLACK,
            run: GlyphRun {
                font: FontId(0),
                size: 10.0,
                glyphs: vec![PositionedGlyph {
                    id: 1,
                    x: 0.0,
                    y: 0.0,
                }]
                .into(),
            },
        });
        list.pop_clip();
        let mut fb = Framebuffer::new(8, 6).unwrap();
        rasterize(&mut fb.canvas(), &list, Color::WHITE, &mut BlockGlyphs);
        assert_eq!(fb.pixel(1, 2), Some(Color::BLACK));
        assert_eq!(fb.pixel(2, 2), Some(Color::WHITE), "clipped");
    }

    #[test]
    fn alpha_blends_over_background() {
        let mut list = DisplayList::new();
        list.fill_rect(Rect::new(0, 0, 1, 1), Color::rgba(0, 0, 0, 128));
        let (fb, _) = render(1, 1, &list);
        let c = fb.pixel(0, 0).unwrap();
        assert!((126..=128).contains(&c.r), "got {c:?}");
        assert_eq!((c.r, c.r), (c.g, c.b));
    }
}

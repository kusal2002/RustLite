use std::rc::Rc;

use crate::{Color, GlyphRun, ImageData, Rect};

/// One drawing command. Items are painted in list order (painter's algorithm).
#[derive(Debug, Clone, PartialEq)]
pub enum DisplayItem {
    /// Draw `run` with its baseline origin at (`x`, `y`) device px. Glyph
    /// offsets and the font size are in CSS px and multiplied by `scale`
    /// (the device pixel ratio) at raster time.
    Text {
        x: f32,
        y: f32,
        scale: f32,
        color: Color,
        run: GlyphRun,
    },
    /// Draw `image` scaled to fill `rect`.
    Image { rect: Rect, image: Rc<ImageData> },
    /// Fill `rect` with `color`.
    SolidRect { rect: Rect, color: Color },
    /// Stroke the inside edge of `rect` with a uniform border of `width` pixels.
    Border {
        rect: Rect,
        width: u32,
        color: Color,
    },
    /// Restrict subsequent items to `rect` intersected with the current clip.
    PushClip { rect: Rect },
    /// Restore the clip that was active before the matching `PushClip`.
    PopClip,
}

/// An ordered sequence of [`DisplayItem`]s.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DisplayList {
    items: Vec<DisplayItem>,
}

impl DisplayList {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, item: DisplayItem) {
        self.items.push(item);
    }

    /// Adds a fill, skipping it when it could not change any pixel.
    pub fn fill_rect(&mut self, rect: Rect, color: Color) {
        if !rect.is_empty() && !color.is_transparent() {
            self.push(DisplayItem::SolidRect { rect, color });
        }
    }

    /// Adds a border stroke, skipping it when it could not change any pixel.
    pub fn stroke_rect(&mut self, rect: Rect, width: u32, color: Color) {
        if !rect.is_empty() && width > 0 && !color.is_transparent() {
            self.push(DisplayItem::Border { rect, width, color });
        }
    }

    pub fn push_clip(&mut self, rect: Rect) {
        self.push(DisplayItem::PushClip { rect });
    }

    pub fn pop_clip(&mut self) {
        self.push(DisplayItem::PopClip);
    }

    /// Appends copies of `other`'s items, offset by (`dx`, `dy`).
    pub fn extend_translated(&mut self, other: &DisplayList, dx: i32, dy: i32) {
        let shift = |r: Rect| {
            Rect::new(
                r.x.saturating_add(dx),
                r.y.saturating_add(dy),
                r.width,
                r.height,
            )
        };
        self.items.extend(other.items.iter().map(|item| match item {
            DisplayItem::Text {
                x,
                y,
                scale,
                color,
                run,
            } => DisplayItem::Text {
                x: x + dx as f32,
                y: y + dy as f32,
                scale: *scale,
                color: *color,
                // Cheap: the glyph array is reference-counted.
                run: run.clone(),
            },
            DisplayItem::Image { rect, image } => DisplayItem::Image {
                rect: shift(*rect),
                image: image.clone(),
            },
            DisplayItem::SolidRect { rect, color } => DisplayItem::SolidRect {
                rect: shift(*rect),
                color: *color,
            },
            DisplayItem::Border { rect, width, color } => DisplayItem::Border {
                rect: shift(*rect),
                width: *width,
                color: *color,
            },
            DisplayItem::PushClip { rect } => DisplayItem::PushClip { rect: shift(*rect) },
            DisplayItem::PopClip => DisplayItem::PopClip,
        }));
    }

    /// Removes all items while keeping the allocation for reuse across frames.
    pub fn clear(&mut self) {
        self.items.clear();
    }

    pub fn items(&self) -> &[DisplayItem] {
        &self.items
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invisible_items_are_skipped() {
        let mut list = DisplayList::new();
        list.fill_rect(Rect::new(0, 0, 0, 10), Color::BLACK);
        list.fill_rect(Rect::new(0, 0, 10, 10), Color::TRANSPARENT);
        list.stroke_rect(Rect::new(0, 0, 10, 10), 0, Color::BLACK);
        assert!(list.is_empty());
    }

    #[test]
    fn extend_translated_offsets_every_rect() {
        let mut src = DisplayList::new();
        src.push_clip(Rect::new(0, 0, 5, 5));
        src.fill_rect(Rect::new(1, 2, 3, 4), Color::BLACK);
        src.stroke_rect(Rect::new(0, 0, 2, 2), 1, Color::WHITE);
        src.pop_clip();

        let mut dst = DisplayList::new();
        dst.extend_translated(&src, 10, -1);
        assert_eq!(
            dst.items(),
            [
                DisplayItem::PushClip {
                    rect: Rect::new(10, -1, 5, 5)
                },
                DisplayItem::SolidRect {
                    rect: Rect::new(11, 1, 3, 4),
                    color: Color::BLACK
                },
                DisplayItem::Border {
                    rect: Rect::new(10, -1, 2, 2),
                    width: 1,
                    color: Color::WHITE
                },
                DisplayItem::PopClip,
            ]
        );
    }

    #[test]
    fn clear_keeps_capacity() {
        let mut list = DisplayList::new();
        list.fill_rect(Rect::new(0, 0, 1, 1), Color::BLACK);
        let capacity = list.items.capacity();
        list.clear();
        assert!(list.is_empty());
        assert_eq!(list.items.capacity(), capacity);
    }
}

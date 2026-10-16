//! Browser toolbar: layout, hit testing, and painting.
//!
//! The toolbar is browser UI, not web content, so it is painted directly into
//! the frame display list rather than through the page pipeline. Text labels
//! arrive with font support (roadmap phase 4/5); until then controls are
//! drawn as shapes.

use rustlite_paint::{Color, DisplayList, Point, Rect};

/// Toolbar height in logical pixels.
const HEIGHT: f64 = 44.0;
const PADDING: f64 = 6.0;

const BACKGROUND: Color = Color::rgb(0xF1, 0xF3, 0xF4);
const SEPARATOR: Color = Color::rgb(0xDA, 0xDC, 0xE0);
const BUTTON: Color = Color::rgb(0xE6, 0xE8, 0xEB);
const BUTTON_HOVER: Color = Color::rgb(0xD2, 0xD5, 0xDA);
const ADDRESS_FIELD: Color = Color::WHITE;
const ADDRESS_BORDER: Color = Color::rgb(0xC4, 0xC7, 0xCC);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolbarButton {
    Back,
    Forward,
    Reload,
}

impl ToolbarButton {
    pub const ALL: [ToolbarButton; 3] = [Self::Back, Self::Forward, Self::Reload];
}

/// What a point in the toolbar refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolbarHit {
    Button(ToolbarButton),
    AddressField,
    Background,
}

/// Toolbar geometry for one window width and scale factor, in physical pixels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolbarLayout {
    pub bounds: Rect,
    pub buttons: [(ToolbarButton, Rect); 3],
    pub address_field: Rect,
    separator_width: u32,
}

fn px(logical: f64, scale: f64) -> u32 {
    // Window sizes are bounded far below u32::MAX, so the cast cannot truncate.
    (logical * scale).round().max(1.0) as u32
}

impl ToolbarLayout {
    pub fn new(window_width: u32, scale_factor: f64) -> Self {
        let scale = if scale_factor.is_finite() && scale_factor > 0.0 {
            scale_factor
        } else {
            1.0
        };
        let height = px(HEIGHT, scale);
        let pad = px(PADDING, scale);
        let control = height.saturating_sub(2 * pad);

        let mut x = pad;
        let buttons = ToolbarButton::ALL.map(|button| {
            let rect = Rect::new(x as i32, pad as i32, control, control);
            x += control + pad;
            (button, rect)
        });

        let address_width = window_width.saturating_sub(x + pad);
        Self {
            bounds: Rect::new(0, 0, window_width, height),
            buttons,
            address_field: Rect::new(x as i32, pad as i32, address_width, control),
            separator_width: px(1.0, scale),
        }
    }

    pub fn height(&self) -> u32 {
        self.bounds.height
    }

    pub fn hit_test(&self, point: Point) -> Option<ToolbarHit> {
        if !self.bounds.contains(point) {
            return None;
        }
        if let Some(&(button, _)) = self.buttons.iter().find(|(_, r)| r.contains(point)) {
            return Some(ToolbarHit::Button(button));
        }
        if self.address_field.contains(point) {
            return Some(ToolbarHit::AddressField);
        }
        Some(ToolbarHit::Background)
    }

    pub fn paint(&self, hovered: Option<ToolbarButton>, out: &mut DisplayList) {
        out.fill_rect(self.bounds, BACKGROUND);
        let sep = self.separator_width.min(self.bounds.height);
        out.fill_rect(
            Rect::new(0, (self.bounds.height - sep) as i32, self.bounds.width, sep),
            SEPARATOR,
        );
        for (button, rect) in self.buttons {
            let color = if hovered == Some(button) {
                BUTTON_HOVER
            } else {
                BUTTON
            };
            out.fill_rect(rect, color);
        }
        out.fill_rect(self.address_field, ADDRESS_FIELD);
        out.stroke_rect(self.address_field, self.separator_width, ADDRESS_BORDER);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustlite_paint::DisplayItem;

    #[test]
    fn layout_scales_with_dpi() {
        assert_eq!(ToolbarLayout::new(800, 1.0).height(), 44);
        assert_eq!(ToolbarLayout::new(800, 2.0).height(), 88);
        assert_eq!(ToolbarLayout::new(800, f64::NAN).height(), 44);
    }

    #[test]
    fn controls_do_not_overlap_and_stay_inside() {
        let layout = ToolbarLayout::new(800, 1.25);
        let mut rects: Vec<Rect> = layout.buttons.iter().map(|&(_, r)| r).collect();
        rects.push(layout.address_field);
        for (i, a) in rects.iter().enumerate() {
            assert_eq!(
                layout.bounds.intersect(*a),
                Some(*a),
                "control {i} escapes toolbar"
            );
            for b in &rects[i + 1..] {
                assert_eq!(a.intersect(*b), None);
            }
        }
    }

    #[test]
    fn narrow_window_collapses_address_field() {
        let layout = ToolbarLayout::new(50, 1.0);
        assert_eq!(layout.address_field.width, 0);
    }

    #[test]
    fn hit_testing() {
        let layout = ToolbarLayout::new(800, 1.0);
        let (_, back) = layout.buttons[0];
        assert_eq!(
            layout.hit_test(Point::new(back.x + 1, back.y + 1)),
            Some(ToolbarHit::Button(ToolbarButton::Back))
        );
        let addr = layout.address_field;
        assert_eq!(
            layout.hit_test(Point::new(addr.x + 5, addr.y + 5)),
            Some(ToolbarHit::AddressField)
        );
        assert_eq!(
            layout.hit_test(Point::new(1, 1)),
            Some(ToolbarHit::Background)
        );
        assert_eq!(layout.hit_test(Point::new(1, 500)), None);
    }

    #[test]
    fn hover_changes_button_color() {
        let layout = ToolbarLayout::new(800, 1.0);
        let mut plain = DisplayList::new();
        layout.paint(None, &mut plain);
        let mut hovered = DisplayList::new();
        layout.paint(Some(ToolbarButton::Reload), &mut hovered);
        let reload = layout.buttons[2].1;
        let color_of = |list: &DisplayList| {
            list.items().iter().find_map(|item| match *item {
                DisplayItem::SolidRect { rect, color } if rect == reload => Some(color),
                _ => None,
            })
        };
        assert_eq!(color_of(&plain), Some(BUTTON));
        assert_eq!(color_of(&hovered), Some(BUTTON_HOVER));
    }
}

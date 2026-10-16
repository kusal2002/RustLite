//! Browser toolbar: layout, hit testing, and painting.
//!
//! The toolbar is browser UI, not web content, so it is painted directly into
//! the frame display list rather than through the page pipeline. Its text
//! (URL, button icons) is shaped with the same font context as the page.

use std::rc::Rc;

use rustlite_fonts::{Family, FontContext};
use rustlite_paint::{Color, DisplayItem, DisplayList, GlyphRun, Point, PositionedGlyph, Rect};

/// Toolbar height in logical pixels.
const HEIGHT: f64 = 44.0;
const PADDING: f64 = 6.0;
/// UI font size in logical pixels.
pub const UI_FONT_SIZE: f32 = 14.0;

const BACKGROUND: Color = Color::rgb(0xF1, 0xF3, 0xF4);
const SEPARATOR: Color = Color::rgb(0xDA, 0xDC, 0xE0);
const BUTTON: Color = Color::rgb(0xE6, 0xE8, 0xEB);
const BUTTON_HOVER: Color = Color::rgb(0xD2, 0xD5, 0xDA);
const ICON: Color = Color::rgb(0x3C, 0x40, 0x43);
const ICON_DISABLED: Color = Color::rgb(0xA8, 0xAB, 0xAF);
const ADDRESS_FIELD: Color = Color::WHITE;
const ADDRESS_BORDER: Color = Color::rgb(0xC4, 0xC7, 0xCC);
const ADDRESS_FOCUS_BORDER: Color = Color::rgb(0x1A, 0x73, 0xE8);
const ADDRESS_TEXT: Color = Color::rgb(0x20, 0x21, 0x24);
const SELECTION: Color = Color::rgb(0xC2, 0xDB, 0xFF);
const LOADING_BAR: Color = Color::rgb(0x1A, 0x73, 0xE8);

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

/// Shaped UI text: glyph runs with x offsets, total width, and ascent (CSS px).
#[derive(Debug, Clone, Default)]
pub struct UiText {
    pub runs: Vec<(f32, GlyphRun)>,
    /// Pen x after each char boundary, for caret placement: (byte index, x).
    pub carets: Vec<(usize, f32)>,
    pub width: f32,
    pub ascent: f32,
    pub descent: f32,
}

/// Shapes `text` in the UI font. Caret positions come from shaping each
/// prefix's width, which is exact for the short strings the toolbar shows.
pub fn shape_ui_text(fonts: &mut FontContext, text: &str, size: f32) -> UiText {
    let Some(font) = fonts.select(&[Family::SansSerif], 400, false) else {
        return UiText::default();
    };
    let metrics = fonts.metrics(font, size);
    let mut runs = Vec::new();
    let mut x = 0.0;
    for seg in fonts.shape(font, 400, false, size, text).iter() {
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
                size,
                glyphs,
            },
        ));
        x += seg.advance;
    }
    let mut carets = vec![(0, 0.0)];
    for (i, c) in text.char_indices() {
        let end = i + c.len_utf8();
        carets.push((end, fonts.measure(font, 400, false, size, &text[..end])));
    }
    UiText {
        runs,
        carets,
        width: x,
        ascent: metrics.ascent,
        descent: metrics.descent,
    }
}

/// Everything the toolbar shows that changes at runtime.
#[derive(Debug, Clone, Default)]
pub struct ToolbarView {
    pub hovered: Option<ToolbarButton>,
    pub can_back: bool,
    pub can_forward: bool,
    pub loading: bool,
    pub address: UiText,
    /// Caret byte index when the address field is focused.
    pub caret: Option<usize>,
    pub all_selected: bool,
    /// Icons for back, forward and reload (or stop while loading).
    pub icons: [UiText; 3],
}

/// Toolbar geometry for one window width and scale factor, in physical pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolbarLayout {
    pub bounds: Rect,
    pub buttons: [(ToolbarButton, Rect); 3],
    pub address_field: Rect,
    separator_width: u32,
    scale: f32,
}

pub(crate) fn px(logical: f64, scale: f64) -> u32 {
    // Window sizes are bounded far below u32::MAX, so the cast cannot truncate.
    (logical * scale).round().max(1.0) as u32
}

/// A usable scale factor (1.0 for nonsense values).
pub(crate) fn sane_scale(scale_factor: f64) -> f64 {
    if scale_factor.is_finite() && scale_factor > 0.0 {
        scale_factor
    } else {
        1.0
    }
}

/// Draws `text` with its baseline vertically centered in `rect`.
pub(crate) fn draw_text(
    out: &mut DisplayList,
    scale: f32,
    rect: Rect,
    x: f32,
    text: &UiText,
    color: Color,
) {
    let line = (text.ascent + text.descent) * scale;
    let baseline = rect.y as f32 + (rect.height as f32 - line) / 2.0 + text.ascent * scale;
    for (dx, run) in &text.runs {
        out.push(DisplayItem::Text {
            x: x + dx * scale,
            y: baseline.round(),
            scale,
            color,
            run: run.clone(),
        });
    }
}

impl ToolbarLayout {
    /// A toolbar at the top of the window.
    pub fn new(window_width: u32, scale_factor: f64) -> Self {
        Self::at(0, window_width, scale_factor)
    }

    /// A toolbar whose top edge is at `top` (device px), below other chrome.
    pub fn at(top: u32, window_width: u32, scale_factor: f64) -> Self {
        let scale = sane_scale(scale_factor);
        let height = px(HEIGHT, scale);
        let pad = px(PADDING, scale);
        let control = height.saturating_sub(2 * pad);
        let y = (top + pad) as i32;

        let mut x = pad;
        let buttons = ToolbarButton::ALL.map(|button| {
            let rect = Rect::new(x as i32, y, control, control);
            x += control + pad;
            (button, rect)
        });

        let address_width = window_width.saturating_sub(x + pad);
        Self {
            bounds: Rect::new(0, top as i32, window_width, height),
            buttons,
            address_field: Rect::new(x as i32, y, address_width, control),
            separator_width: px(1.0, scale),
            scale: scale as f32,
        }
    }

    pub fn height(&self) -> u32 {
        self.bounds.height
    }

    /// The first device-px row below the toolbar (where content starts).
    pub fn bottom(&self) -> u32 {
        self.bounds.y as u32 + self.bounds.height
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

    /// Text inset inside the address field, in device px.
    fn text_inset(&self) -> f32 {
        8.0 * self.scale
    }

    fn text(&self, out: &mut DisplayList, rect: Rect, x: f32, text: &UiText, color: Color) {
        draw_text(out, self.scale, rect, x, text, color);
    }

    pub fn paint(&self, view: &ToolbarView, out: &mut DisplayList) {
        out.fill_rect(self.bounds, BACKGROUND);
        let sep = self.separator_width.min(self.bounds.height);
        let bottom = self.bottom() as i32;
        out.fill_rect(
            Rect::new(0, bottom - sep as i32, self.bounds.width, sep),
            SEPARATOR,
        );
        for (i, (button, rect)) in self.buttons.into_iter().enumerate() {
            let color = if view.hovered == Some(button) {
                BUTTON_HOVER
            } else {
                BUTTON
            };
            out.fill_rect(rect, color);
            let enabled = match button {
                ToolbarButton::Back => view.can_back,
                ToolbarButton::Forward => view.can_forward,
                ToolbarButton::Reload => true,
            };
            let icon = &view.icons[i];
            let x = rect.x as f32 + (rect.width as f32 - icon.width * self.scale) / 2.0;
            self.text(
                out,
                rect,
                x.round(),
                icon,
                if enabled { ICON } else { ICON_DISABLED },
            );
        }

        let field = self.address_field;
        out.fill_rect(field, ADDRESS_FIELD);
        let focused = view.caret.is_some();
        let border = if focused {
            ADDRESS_FOCUS_BORDER
        } else {
            ADDRESS_BORDER
        };
        out.stroke_rect(field, self.separator_width, border);

        out.push_clip(field.inset(self.separator_width));
        let text_x = field.x as f32 + self.text_inset();
        if view.all_selected && view.address.width > 0.0 {
            let pad = (4.0 * self.scale) as u32;
            out.fill_rect(
                Rect::new(
                    text_x.round() as i32,
                    field.y + pad as i32,
                    (view.address.width * self.scale).ceil() as u32,
                    field.height.saturating_sub(2 * pad),
                ),
                SELECTION,
            );
        }
        self.text(out, field, text_x, &view.address, ADDRESS_TEXT);
        if let Some(caret) = view.caret.filter(|_| !view.all_selected) {
            let x = view
                .address
                .carets
                .iter()
                .find(|(i, _)| *i == caret)
                .map_or(0.0, |(_, x)| *x);
            let pad = (6.0 * self.scale) as u32;
            out.fill_rect(
                Rect::new(
                    (text_x + x * self.scale).round() as i32,
                    field.y + pad as i32,
                    self.separator_width.max(1),
                    field.height.saturating_sub(2 * pad),
                ),
                ADDRESS_TEXT,
            );
        }
        out.pop_clip();

        if view.loading {
            let h = (2.0 * self.scale).round().max(1.0) as u32;
            out.fill_rect(
                Rect::new(0, self.bottom() as i32 - h as i32, self.bounds.width, h),
                LOADING_BAR,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn offset_toolbar_moves_every_control() {
        let top = ToolbarLayout::new(800, 1.0);
        let below = ToolbarLayout::at(34, 800, 1.0);
        assert_eq!(below.bottom(), 34 + top.height());
        assert_eq!(below.address_field.y, top.address_field.y + 34);
        let (_, back) = below.buttons[0];
        assert_eq!(
            below.hit_test(Point::new(back.x + 1, back.y + 1)),
            Some(ToolbarHit::Button(ToolbarButton::Back))
        );
        assert_eq!(below.hit_test(Point::new(back.x + 1, 2)), None);
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

    fn solid_color_at(list: &DisplayList, rect: Rect) -> Option<Color> {
        list.items().iter().find_map(|item| match *item {
            DisplayItem::SolidRect { rect: r, color } if r == rect => Some(color),
            _ => None,
        })
    }

    #[test]
    fn hover_changes_button_color() {
        let layout = ToolbarLayout::new(800, 1.0);
        let mut plain = DisplayList::new();
        layout.paint(&ToolbarView::default(), &mut plain);
        let mut hovered = DisplayList::new();
        layout.paint(
            &ToolbarView {
                hovered: Some(ToolbarButton::Reload),
                ..ToolbarView::default()
            },
            &mut hovered,
        );
        let reload = layout.buttons[2].1;
        assert_eq!(solid_color_at(&plain, reload), Some(BUTTON));
        assert_eq!(solid_color_at(&hovered, reload), Some(BUTTON_HOVER));
    }

    #[test]
    fn loading_bar_and_focus_border() {
        let layout = ToolbarLayout::new(800, 1.0);
        let mut list = DisplayList::new();
        layout.paint(
            &ToolbarView {
                loading: true,
                caret: Some(0),
                ..ToolbarView::default()
            },
            &mut list,
        );
        assert!(
            list.items().iter().any(
                |i| matches!(i, DisplayItem::SolidRect { color, .. } if *color == LOADING_BAR)
            )
        );
        assert!(list.items().iter().any(
            |i| matches!(i, DisplayItem::Border { color, .. } if *color == ADDRESS_FOCUS_BORDER)
        ));
    }

    #[test]
    fn ui_text_carets_are_monotonic() {
        let mut fonts = FontContext::with_system_fonts();
        if fonts.face_count() == 0 {
            return;
        }
        let t = shape_ui_text(&mut fonts, "https://a.test/", UI_FONT_SIZE);
        assert!(t.width > 0.0 && !t.runs.is_empty());
        assert!(t.carets.windows(2).all(|w| w[1].1 >= w[0].1));
        assert_eq!(t.carets.last().unwrap().0, "https://a.test/".len());
    }
}

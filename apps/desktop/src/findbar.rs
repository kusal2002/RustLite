//! Find-in-page: the floating find bar, match bookkeeping, and highlights.

use rustlite_layout::{FindMatch, RectF};
use rustlite_paint::{Color, DisplayList, Point, Rect};

use crate::address_bar::AddressBar;
use crate::toolbar::{UiText, draw_text, px, sane_scale};

const WIDTH: f64 = 380.0;
const HEIGHT: f64 = 40.0;
const MARGIN: f64 = 8.0;
const PADDING: f64 = 6.0;
const COUNT_WIDTH: f64 = 64.0;

const BACKGROUND: Color = Color::rgb(0xF1, 0xF3, 0xF4);
const BORDER: Color = Color::rgb(0xB4, 0xB8, 0xBE);
const FIELD: Color = Color::WHITE;
const FIELD_NO_MATCH: Color = Color::rgb(0xFC, 0xE8, 0xE6);
const TEXT: Color = Color::rgb(0x20, 0x21, 0x24);
const MUTED: Color = Color::rgb(0x5F, 0x63, 0x68);
const BUTTON_HOVER: Color = Color::rgb(0xD2, 0xD5, 0xDA);
const SELECTION: Color = Color::rgb(0xC2, 0xDB, 0xFF);
/// Highlights are translucent so the text stays readable.
pub const MATCH: Color = Color::rgba(0xFF, 0xEB, 0x3B, 0x99);
pub const CURRENT_MATCH: Color = Color::rgba(0xFF, 0x96, 0x32, 0xB0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindButton {
    Previous,
    Next,
    Close,
}

impl FindButton {
    pub const ALL: [FindButton; 3] = [Self::Previous, Self::Next, Self::Close];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindHit {
    Field,
    Button(FindButton),
    Background,
}

/// One tab's find session.
#[derive(Debug, Default)]
pub struct FindState {
    pub input: AddressBar,
    matches: Vec<FindMatch>,
    current: usize,
    /// The query and page layout generation `matches` were computed for.
    searched: Option<(String, u64)>,
}

impl FindState {
    pub fn new() -> Self {
        let mut state = Self::default();
        state.input.focus();
        state
    }

    pub fn matches(&self) -> &[FindMatch] {
        &self.matches
    }

    pub fn current(&self) -> Option<usize> {
        (!self.matches.is_empty()).then_some(self.current)
    }

    /// True if `matches` is stale for this query and layout generation.
    pub fn needs_search(&self, generation: u64) -> bool {
        self.searched
            .as_ref()
            .is_none_or(|(q, g)| q != self.input.text() || *g != generation)
    }

    /// Stores fresh results. A changed query restarts at the first match at
    /// or below `scroll_y`; a relayout keeps the current index.
    pub fn set_matches(&mut self, matches: Vec<FindMatch>, generation: u64, scroll_y: f32) {
        let query_changed = self
            .searched
            .as_ref()
            .is_none_or(|(q, _)| q != self.input.text());
        if query_changed {
            self.current = matches
                .iter()
                .position(|m| m.first().is_some_and(|r| r.y >= scroll_y))
                .unwrap_or(0);
        } else if self.current >= matches.len() {
            self.current = 0;
        }
        self.matches = matches;
        self.searched = Some((self.input.text().to_owned(), generation));
    }

    /// Moves to the next (`forward`) or previous match, wrapping around.
    pub fn step(&mut self, forward: bool) {
        let n = self.matches.len();
        if n == 0 {
            return;
        }
        self.current = if forward {
            (self.current + 1) % n
        } else {
            (self.current + n - 1) % n
        };
    }

    /// The current match's bounding box in document CSS px.
    pub fn current_rect(&self) -> Option<RectF> {
        let m = self.matches.get(self.current)?;
        let first = *m.first()?;
        Some(m.iter().skip(1).fold(first, |a, b| {
            let x = a.x.min(b.x);
            let y = a.y.min(b.y);
            RectF::new(
                x,
                y,
                a.right().max(b.right()) - x,
                a.bottom().max(b.bottom()) - y,
            )
        }))
    }

    /// "3/12", or "0/0" for a query with no matches; empty with no query.
    pub fn count_label(&self) -> String {
        if self.input.text().is_empty() {
            String::new()
        } else if self.matches.is_empty() {
            "0/0".to_owned()
        } else {
            format!("{}/{}", self.current + 1, self.matches.len())
        }
    }
}

/// The scroll offset (CSS px) that brings `rect` into a viewport of
/// `viewport_h` CSS px scrolled to `scroll_y`, or `None` if it is visible.
pub fn scroll_to_reveal(rect: RectF, scroll_y: f32, viewport_h: f32) -> Option<f32> {
    let visible = rect.y >= scroll_y && rect.bottom() <= scroll_y + viewport_h;
    (!visible).then(|| (rect.y - viewport_h / 3.0).max(0.0))
}

/// Paints match highlights over the page in `content` (device px).
pub fn paint_highlights(
    out: &mut DisplayList,
    state: &FindState,
    content: Rect,
    scale: f32,
    scroll_y: f32,
) {
    let top = scroll_y;
    let bottom = scroll_y + content.height as f32 / scale;
    out.push_clip(content);
    for (i, m) in state.matches.iter().enumerate() {
        let color = if Some(i) == state.current() {
            CURRENT_MATCH
        } else {
            MATCH
        };
        for r in m.iter().filter(|r| r.bottom() >= top && r.y <= bottom) {
            let x0 = (r.x * scale).floor();
            let y0 = ((r.y - scroll_y) * scale).floor();
            let x1 = (r.right() * scale).ceil();
            let y1 = ((r.bottom() - scroll_y) * scale).ceil();
            out.fill_rect(
                Rect::new(
                    content.x + x0 as i32,
                    content.y + y0 as i32,
                    (x1 - x0).max(1.0) as u32,
                    (y1 - y0).max(1.0) as u32,
                ),
                color,
            );
        }
    }
    out.pop_clip();
}

/// Everything the bar shows that changes at runtime.
#[derive(Debug, Clone, Default)]
pub struct FindBarView {
    pub query: UiText,
    pub caret: Option<usize>,
    pub all_selected: bool,
    pub count: UiText,
    pub no_match: bool,
    pub hovered: Option<FindButton>,
    /// Icons for previous, next and close.
    pub icons: [UiText; 3],
}

/// Find bar geometry: a box floating at the top right of the content area.
#[derive(Debug, Clone, PartialEq)]
pub struct FindBarLayout {
    pub bounds: Rect,
    pub field: Rect,
    pub count: Rect,
    pub buttons: [(FindButton, Rect); 3],
    scale: f32,
}

impl FindBarLayout {
    pub fn new(content: Rect, scale_factor: f64) -> Self {
        let scale = sane_scale(scale_factor);
        let margin = px(MARGIN, scale);
        let pad = px(PADDING, scale);
        let width = px(WIDTH, scale).min(content.width.saturating_sub(2 * margin));
        let height = px(HEIGHT, scale);
        let x = content.right() as i32 - (width + margin) as i32;
        let y = content.y + margin as i32;
        let control = height.saturating_sub(2 * pad);

        // Buttons from the right edge inwards, then the count, then the field.
        let mut right = x + width as i32 - pad as i32;
        let mut buttons = FindButton::ALL.map(|b| (b, Rect::default()));
        for slot in buttons.iter_mut().rev() {
            right -= control as i32;
            slot.1 = Rect::new(right, y + pad as i32, control, control);
        }
        let count_w = px(COUNT_WIDTH, scale);
        let count = Rect::new(right - count_w as i32, y + pad as i32, count_w, control);
        let field_x = x + pad as i32;
        let field = Rect::new(
            field_x,
            y + pad as i32,
            u32::try_from(count.x - field_x).unwrap_or(0),
            control,
        );
        Self {
            bounds: Rect::new(x, y, width, height),
            field,
            count,
            buttons,
            scale: scale as f32,
        }
    }

    pub fn hit_test(&self, point: Point) -> Option<FindHit> {
        if !self.bounds.contains(point) {
            return None;
        }
        if let Some(&(b, _)) = self.buttons.iter().find(|(_, r)| r.contains(point)) {
            return Some(FindHit::Button(b));
        }
        if self.field.contains(point) {
            return Some(FindHit::Field);
        }
        Some(FindHit::Background)
    }

    pub fn paint(&self, view: &FindBarView, out: &mut DisplayList) {
        let s = self.scale;
        let line = px(1.0, f64::from(s));
        out.fill_rect(self.bounds, BACKGROUND);
        out.stroke_rect(self.bounds, line, BORDER);

        let field = self.field;
        out.fill_rect(field, if view.no_match { FIELD_NO_MATCH } else { FIELD });
        out.push_clip(field);
        let text_x = field.x as f32 + 6.0 * s;
        if view.all_selected && view.query.width > 0.0 {
            let pad = (4.0 * s) as u32;
            out.fill_rect(
                Rect::new(
                    text_x.round() as i32,
                    field.y + pad as i32,
                    (view.query.width * s).ceil() as u32,
                    field.height.saturating_sub(2 * pad),
                ),
                SELECTION,
            );
        }
        draw_text(out, s, field, text_x, &view.query, TEXT);
        if let Some(caret) = view.caret.filter(|_| !view.all_selected) {
            let x = view
                .query
                .carets
                .iter()
                .find(|(i, _)| *i == caret)
                .map_or(0.0, |(_, x)| *x);
            let pad = (6.0 * s) as u32;
            out.fill_rect(
                Rect::new(
                    (text_x + x * s).round() as i32,
                    field.y + pad as i32,
                    line,
                    field.height.saturating_sub(2 * pad),
                ),
                TEXT,
            );
        }
        out.pop_clip();

        let count_x = self.count.right() as f32 - view.count.width * s - 6.0 * s;
        draw_text(out, s, self.count, count_x.round(), &view.count, MUTED);

        for (i, (button, rect)) in self.buttons.into_iter().enumerate() {
            if view.hovered == Some(button) {
                out.fill_rect(rect, BUTTON_HOVER);
            }
            let icon = &view.icons[i];
            let x = rect.x as f32 + (rect.width as f32 - icon.width * s) / 2.0;
            draw_text(out, s, rect, x.round(), icon, TEXT);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(y: f32) -> FindMatch {
        vec![RectF::new(10.0, y, 20.0, 10.0)]
    }

    #[test]
    fn stepping_wraps_and_counts() {
        let mut f = FindState::new();
        f.input.insert("x");
        assert!(f.needs_search(1));
        f.set_matches(vec![m(0.0), m(100.0), m(200.0)], 1, 0.0);
        assert!(!f.needs_search(1));
        assert!(f.needs_search(2), "a relayout invalidates matches");
        assert_eq!(f.count_label(), "1/3");
        f.step(false);
        assert_eq!(f.current(), Some(2));
        f.step(true);
        assert_eq!(f.count_label(), "1/3");
        f.set_matches(Vec::new(), 2, 0.0);
        assert_eq!(f.count_label(), "0/0");
        assert_eq!(f.current(), None);
        f.step(true);
    }

    #[test]
    fn a_new_query_starts_at_the_first_visible_match() {
        let mut f = FindState::new();
        f.input.insert("x");
        f.set_matches(vec![m(0.0), m(100.0), m(200.0)], 1, 150.0);
        assert_eq!(f.current(), Some(2));
        // Same query after a relayout: keep the position.
        f.set_matches(vec![m(0.0), m(100.0), m(200.0)], 2, 0.0);
        assert_eq!(f.current(), Some(2));
    }

    #[test]
    fn reveal_scrolls_only_when_hidden() {
        let r = RectF::new(0.0, 500.0, 10.0, 10.0);
        assert_eq!(scroll_to_reveal(r, 400.0, 300.0), None);
        assert_eq!(scroll_to_reveal(r, 0.0, 300.0), Some(400.0));
    }

    #[test]
    fn bar_sits_inside_content_with_ordered_controls() {
        let content = Rect::new(0, 78, 1000, 600);
        let bar = FindBarLayout::new(content, 1.0);
        assert_eq!(content.intersect(bar.bounds), Some(bar.bounds));
        assert!(bar.field.right() <= i64::from(bar.count.x));
        assert!(bar.count.right() <= i64::from(bar.buttons[0].1.x));
        let close = bar.buttons[2].1;
        assert_eq!(
            bar.hit_test(Point::new(close.x + 1, close.y + 1)),
            Some(FindHit::Button(FindButton::Close))
        );
        assert_eq!(
            bar.hit_test(Point::new(bar.field.x + 1, bar.field.y + 1)),
            Some(FindHit::Field)
        );
        assert_eq!(bar.hit_test(Point::new(1, 100)), None);
    }

    #[test]
    fn highlights_are_clipped_to_visible_matches() {
        let mut f = FindState::new();
        f.input.insert("x");
        f.set_matches(vec![m(0.0), m(5000.0)], 1, 0.0);
        let mut out = DisplayList::new();
        paint_highlights(&mut out, &f, Rect::new(0, 50, 800, 600), 1.0, 0.0);
        let fills: Vec<_> = out
            .items()
            .iter()
            .filter_map(|i| match i {
                rustlite_paint::DisplayItem::SolidRect { rect, color } => Some((*rect, *color)),
                _ => None,
            })
            .collect();
        assert_eq!(fills, vec![(Rect::new(10, 50, 20, 10), CURRENT_MATCH)]);
    }
}

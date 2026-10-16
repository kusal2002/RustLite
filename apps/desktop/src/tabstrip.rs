//! Tab strip: one button per tab plus a new-tab button, above the toolbar.

use rustlite_paint::{Color, DisplayList, Point, Rect};

use crate::toolbar::{UiText, draw_text, px, sane_scale};

/// Strip height in logical pixels.
const HEIGHT: f64 = 34.0;
const PADDING: f64 = 6.0;
const MAX_TAB_WIDTH: f64 = 220.0;
const MIN_TAB_WIDTH: f64 = 56.0;
const CLOSE_SIZE: f64 = 18.0;

const BACKGROUND: Color = Color::rgb(0xDE, 0xE1, 0xE6);
/// Matches the toolbar so the active tab joins it.
const ACTIVE: Color = Color::rgb(0xF1, 0xF3, 0xF4);
const HOVER: Color = Color::rgb(0xE8, 0xEA, 0xED);
const SEPARATOR: Color = Color::rgb(0xB4, 0xB8, 0xBE);
const TITLE: Color = Color::rgb(0x3C, 0x40, 0x43);
const CLOSE_HOVER: Color = Color::rgb(0xCF, 0xD2, 0xD6);
const LOADING: Color = Color::rgb(0x1A, 0x73, 0xE8);

/// What a point in the tab strip refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabHit {
    Tab(usize),
    Close(usize),
    NewTab,
    Background,
}

/// Per-tab display state.
#[derive(Debug, Clone, Default)]
pub struct TabLabel {
    pub title: UiText,
    pub loading: bool,
}

/// Everything the strip shows that changes at runtime.
#[derive(Debug, Clone, Default)]
pub struct TabStripView {
    pub tabs: Vec<TabLabel>,
    pub active: usize,
    pub hovered: Option<TabHit>,
    pub close_icon: UiText,
    pub new_icon: UiText,
}

/// Tab strip geometry, in physical pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct TabStripLayout {
    pub bounds: Rect,
    /// Each tab's rectangle and its close button.
    pub tabs: Vec<(Rect, Rect)>,
    pub new_tab: Rect,
    scale: f32,
}

impl TabStripLayout {
    pub fn new(window_width: u32, scale_factor: f64, count: usize) -> Self {
        let scale = sane_scale(scale_factor);
        let height = px(HEIGHT, scale);
        let pad = px(PADDING, scale);
        let tab_height = height.saturating_sub(pad);
        let new_size = tab_height.saturating_sub(px(4.0, scale));
        let close = px(CLOSE_SIZE, scale);

        // Tabs shrink to share the space left of the new-tab button.
        let available = window_width.saturating_sub(2 * pad + new_size + pad);
        let n = u32::try_from(count.max(1)).unwrap_or(u32::MAX);
        let width = (available / n).clamp(px(MIN_TAB_WIDTH, scale), px(MAX_TAB_WIDTH, scale));

        let mut x = pad;
        let tabs = (0..count)
            .map(|_| {
                let tab = Rect::new(x as i32, pad as i32, width, tab_height);
                let close_rect = Rect::new(
                    (x + width).saturating_sub(close + pad) as i32,
                    (pad + (tab_height.saturating_sub(close)) / 2) as i32,
                    close,
                    close,
                );
                x += width;
                (tab, close_rect)
            })
            .collect();
        let new_tab = Rect::new(
            (x + pad / 2) as i32,
            (pad + (tab_height - new_size) / 2) as i32,
            new_size,
            new_size,
        );
        Self {
            bounds: Rect::new(0, 0, window_width, height),
            tabs,
            new_tab,
            scale: scale as f32,
        }
    }

    pub fn height(&self) -> u32 {
        self.bounds.height
    }

    pub fn hit_test(&self, point: Point) -> Option<TabHit> {
        if !self.bounds.contains(point) {
            return None;
        }
        for (i, (tab, close)) in self.tabs.iter().enumerate() {
            if close.contains(point) {
                return Some(TabHit::Close(i));
            }
            if tab.contains(point) {
                return Some(TabHit::Tab(i));
            }
        }
        if self.new_tab.contains(point) {
            return Some(TabHit::NewTab);
        }
        Some(TabHit::Background)
    }

    pub fn paint(&self, view: &TabStripView, out: &mut DisplayList) {
        let s = self.scale;
        out.fill_rect(self.bounds, BACKGROUND);
        let sep_w = px(1.0, f64::from(s));
        for (i, &(tab, close)) in self.tabs.iter().enumerate() {
            let active = i == view.active;
            let hovered = matches!(view.hovered, Some(TabHit::Tab(j) | TabHit::Close(j)) if j == i);
            if active {
                out.fill_rect(tab, ACTIVE);
            } else if hovered {
                out.fill_rect(tab, HOVER);
            }
            // Separators between inactive neighbours, as in most browsers.
            let next_active = i + 1 == view.active;
            if !active && !next_active && i + 1 < self.tabs.len() {
                let h = tab.height / 2;
                out.fill_rect(
                    Rect::new(
                        (tab.right() - i64::from(sep_w)) as i32,
                        tab.y + (tab.height - h) as i32 / 2,
                        sep_w,
                        h,
                    ),
                    SEPARATOR,
                );
            }
            let Some(label) = view.tabs.get(i) else {
                continue;
            };
            let inset = (10.0 * s).round();
            let text_right = close.x - (4.0 * s) as i32;
            let clip = Rect::new(
                tab.x,
                tab.y,
                u32::try_from(text_right - tab.x).unwrap_or(0),
                tab.height,
            );
            out.push_clip(clip);
            draw_text(out, s, tab, tab.x as f32 + inset, &label.title, TITLE);
            out.pop_clip();

            if view.hovered == Some(TabHit::Close(i)) {
                out.fill_rect(close, CLOSE_HOVER);
            }
            let icon = &view.close_icon;
            let x = close.x as f32 + (close.width as f32 - icon.width * s) / 2.0;
            draw_text(out, s, close, x.round(), icon, TITLE);

            if label.loading {
                let h = (2.0 * s).round().max(1.0) as u32;
                out.fill_rect(
                    Rect::new(
                        tab.x + inset as i32,
                        (tab.bottom() - i64::from(h) - i64::from(sep_w)) as i32,
                        tab.width.saturating_sub(2 * inset as u32),
                        h,
                    ),
                    LOADING,
                );
            }
        }
        if view.hovered == Some(TabHit::NewTab) {
            out.fill_rect(self.new_tab, HOVER);
        }
        let icon = &view.new_icon;
        let x = self.new_tab.x as f32 + (self.new_tab.width as f32 - icon.width * s) / 2.0;
        draw_text(out, s, self.new_tab, x.round(), icon, TITLE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tabs_share_width_within_limits() {
        let one = TabStripLayout::new(1000, 1.0, 1);
        assert_eq!(one.tabs[0].0.width, 220, "capped at the maximum");
        let many = TabStripLayout::new(1000, 1.0, 10);
        let w = many.tabs[0].0.width;
        assert!((56..220).contains(&w));
        assert!(many.new_tab.right() <= 1000, "new-tab button stays visible");
        let crowded = TabStripLayout::new(400, 1.0, 50);
        assert_eq!(
            crowded.tabs[0].0.width, 56,
            "never narrower than the minimum"
        );
        assert_eq!(TabStripLayout::new(800, 2.0, 1).height(), 68);
    }

    #[test]
    fn hit_testing_prefers_close_buttons() {
        let layout = TabStripLayout::new(1000, 1.0, 3);
        let (tab, close) = layout.tabs[1];
        assert_eq!(
            layout.hit_test(Point::new(tab.x + 2, tab.y + 2)),
            Some(TabHit::Tab(1))
        );
        assert_eq!(
            layout.hit_test(Point::new(close.x + 1, close.y + 1)),
            Some(TabHit::Close(1))
        );
        let plus = layout.new_tab;
        assert_eq!(
            layout.hit_test(Point::new(plus.x + 1, plus.y + 1)),
            Some(TabHit::NewTab)
        );
        assert_eq!(
            layout.hit_test(Point::new(999, 1)),
            Some(TabHit::Background)
        );
        assert_eq!(layout.hit_test(Point::new(1, 500)), None);
    }

    #[test]
    fn active_tab_matches_toolbar() {
        let layout = TabStripLayout::new(600, 1.0, 2);
        let mut list = DisplayList::new();
        layout.paint(
            &TabStripView {
                tabs: vec![TabLabel::default(), TabLabel::default()],
                active: 1,
                ..TabStripView::default()
            },
            &mut list,
        );
        let active = layout.tabs[1].0;
        assert!(list.items().iter().any(|i| matches!(
            i,
            rustlite_paint::DisplayItem::SolidRect { rect, color } if *rect == active && *color == ACTIVE
        )));
    }
}

//! Composes one window frame: browser chrome plus the page's display list.
//!
//! Shared by the windowed path and `--headless`, so both render identically.

use rustlite_browser_core::Page;
use rustlite_paint::{Color, DisplayItem, DisplayList, Rect, Size};

use crate::toolbar::{ToolbarButton, ToolbarLayout};

/// Window background behind everything (visible only if the page does not cover it).
pub const WINDOW_BACKGROUND: Color = Color::rgb(0xFF, 0xFF, 0xFF);
const DEBUG_OUTLINE: Color = Color::rgba(0xFF, 0x00, 0xFF, 0xA0);

/// UI state that affects how the frame is painted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ChromeState {
    pub hovered_button: Option<ToolbarButton>,
    pub debug_overlay: bool,
}

/// Size of the page viewport for a given window size and toolbar.
pub fn content_size(window: Size, toolbar: &ToolbarLayout) -> Size {
    Size::new(window.width, window.height.saturating_sub(toolbar.height()))
}

/// Builds the full frame for `window` into `out` (cleared first, allocation reused).
pub fn compose(
    out: &mut DisplayList,
    window: Size,
    toolbar: &ToolbarLayout,
    page: &mut Page,
    chrome: ChromeState,
) {
    out.clear();
    let content = Rect::new(
        0,
        toolbar.height() as i32,
        window.width,
        window.height.saturating_sub(toolbar.height()),
    );

    page.set_viewport(Size::new(content.width, content.height));
    out.push_clip(content);
    out.extend_translated(page.display_list(), content.x, content.y);
    out.pop_clip();

    toolbar.paint(chrome.hovered_button, out);

    if chrome.debug_overlay {
        let painted: Vec<Rect> = out
            .items()
            .iter()
            .filter_map(|item| match *item {
                DisplayItem::SolidRect { rect, .. } | DisplayItem::Border { rect, .. } => {
                    Some(rect)
                }
                _ => None,
            })
            .collect();
        for rect in painted {
            out.stroke_rect(rect, 1, DEBUG_OUTLINE);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustlite_renderer::{Framebuffer, rasterize};

    fn render(window: Size, chrome: ChromeState) -> (Framebuffer, ToolbarLayout, Page) {
        let toolbar = ToolbarLayout::new(window.width, 1.0);
        let mut page = Page::new(content_size(window, &toolbar));
        let mut list = DisplayList::new();
        compose(&mut list, window, &toolbar, &mut page, chrome);
        let mut fb = Framebuffer::new(window.width, window.height).unwrap();
        let stats = rasterize(&mut fb.canvas(), &list, WINDOW_BACKGROUND);
        assert_eq!(stats.unbalanced_clips, 0);
        (fb, toolbar, page)
    }

    #[test]
    fn page_fills_area_below_toolbar() {
        let window = Size::new(320, 240);
        let (fb, toolbar, page) = render(window, ChromeState::default());
        assert_eq!(page.viewport(), Size::new(320, 240 - toolbar.height()));
        assert_eq!(fb.pixel(0, 0), Some(Color::rgb(0xF1, 0xF3, 0xF4)));
        assert_eq!(fb.pixel(160, 239), Some(Color::WHITE));
    }

    #[test]
    fn window_shorter_than_toolbar_does_not_panic() {
        let (_, _, page) = render(Size::new(100, 10), ChromeState::default());
        assert_eq!(page.viewport(), Size::new(100, 0));
    }

    #[test]
    fn debug_overlay_outlines_rects() {
        let window = Size::new(320, 240);
        let (fb, _, _) = render(
            window,
            ChromeState {
                debug_overlay: true,
                ..ChromeState::default()
            },
        );
        let corner = fb.pixel(0, 0).unwrap();
        assert!(
            corner.r > corner.g,
            "expected magenta-tinted outline, got {corner:?}"
        );
    }
}

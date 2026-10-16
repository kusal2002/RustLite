//! Composes one window frame: browser chrome plus the page's display list.
//!
//! Shared by the windowed path and `--headless`, so both render identically.

use rustlite_browser_core::PageHandle;
use rustlite_paint::{Color, DisplayItem, DisplayList, Rect, Size};

use crate::toolbar::ToolbarLayout;

/// Window background behind everything (visible only if the page does not cover it).
pub const WINDOW_BACKGROUND: Color = Color::rgb(0xFF, 0xFF, 0xFF);
const DEBUG_OUTLINE: Color = Color::rgba(0xFF, 0x00, 0xFF, 0xA0);

/// Size of the page viewport for a given window size and toolbar (the
/// lowest piece of chrome; content starts below it).
pub fn content_size(window: Size, toolbar: &ToolbarLayout) -> Size {
    let r = content_rect(window, toolbar.bottom());
    Size::new(r.width, r.height)
}

/// The page area: everything below `top`.
pub fn content_rect(window: Size, top: u32) -> Rect {
    let top = top.min(window.height);
    Rect::new(0, top as i32, window.width, window.height - top)
}

/// Builds the full frame for `window` into `out` (cleared first, allocation
/// reused): the page, then whatever `chrome` paints over it (toolbar, tab
/// strip, find highlights and bar). `chrome` gets the content rectangle.
pub fn compose(
    out: &mut DisplayList,
    window: Size,
    content_top: u32,
    page: &mut PageHandle,
    debug_overlay: bool,
    chrome: impl FnOnce(&mut DisplayList, &mut PageHandle, Rect),
) {
    out.clear();
    let content = content_rect(window, content_top);

    page.set_viewport(Size::new(content.width, content.height));
    out.push_clip(content);
    out.extend_translated(page.display_list(), content.x, content.y);
    out.pop_clip();

    chrome(out, page, content);

    if debug_overlay {
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
    use crate::toolbar::ToolbarView;
    use rustlite_browser_core::Page;
    use rustlite_renderer::{Framebuffer, rasterize};

    fn render(window: Size, debug_overlay: bool) -> (Framebuffer, ToolbarLayout, PageHandle) {
        let toolbar = ToolbarLayout::new(window.width, 1.0);
        let mut page: PageHandle = Page::new(content_size(window, &toolbar)).into();
        let mut list = DisplayList::new();
        compose(
            &mut list,
            window,
            toolbar.bottom(),
            &mut page,
            debug_overlay,
            |out, _, _| toolbar.paint(&ToolbarView::default(), out),
        );
        let mut fb = Framebuffer::new(window.width, window.height).unwrap();
        let stats = rasterize(
            &mut fb.canvas(),
            &list,
            WINDOW_BACKGROUND,
            &mut rustlite_paint::NoGlyphs,
        );
        assert_eq!(stats.unbalanced_clips, 0);
        (fb, toolbar, page)
    }

    #[test]
    fn page_fills_area_below_toolbar() {
        let window = Size::new(320, 240);
        let (fb, toolbar, page) = render(window, false);
        assert_eq!(page.viewport(), Size::new(320, 240 - toolbar.height()));
        assert_eq!(fb.pixel(0, 0), Some(Color::rgb(0xF1, 0xF3, 0xF4)));
        assert_eq!(fb.pixel(160, 239), Some(Color::WHITE));
    }

    #[test]
    fn window_shorter_than_toolbar_does_not_panic() {
        let (_, _, page) = render(Size::new(100, 10), false);
        assert_eq!(page.viewport(), Size::new(100, 0));
    }

    #[test]
    fn debug_overlay_outlines_rects() {
        let (fb, _, _) = render(Size::new(320, 240), true);
        let corner = fb.pixel(0, 0).unwrap();
        assert!(
            corner.r > corner.g,
            "expected magenta-tinted outline, got {corner:?}"
        );
    }
}

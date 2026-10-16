//! Coordinates the page pipeline: document → (style → layout →) display list.
//!
//! Phase 1 scope: the only document is `about:blank`. The HTML, CSS, style and
//! layout stages slot in between the document and the display list in later
//! phases (see `docs/roadmap.md`). What exists today is the part every later
//! phase depends on: a viewport, and a cached display list that is rebuilt only
//! when something invalidates it.

use rustlite_paint::{Color, DisplayList, Rect, Size};

/// The document currently shown in a page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Document {
    /// `about:blank`: an empty document with the initial white canvas.
    Blank,
}

impl Document {
    pub fn url(&self) -> &str {
        match self {
            Document::Blank => "about:blank",
        }
    }
}

/// One page: a document plus the viewport it is laid out against.
#[derive(Debug)]
pub struct Page {
    document: Document,
    viewport: Size,
    display_list: DisplayList,
    paint_dirty: bool,
    paint_count: u64,
}

impl Page {
    pub fn new(viewport: Size) -> Self {
        Self {
            document: Document::Blank,
            viewport,
            display_list: DisplayList::new(),
            paint_dirty: true,
            paint_count: 0,
        }
    }

    pub fn document(&self) -> &Document {
        &self.document
    }

    pub fn viewport(&self) -> Size {
        self.viewport
    }

    /// Updates the viewport, invalidating paint only if the size actually changed.
    pub fn set_viewport(&mut self, viewport: Size) {
        if self.viewport != viewport {
            tracing::debug!(?viewport, "page viewport changed");
            self.viewport = viewport;
            self.paint_dirty = true;
        }
    }

    pub fn needs_paint(&self) -> bool {
        self.paint_dirty
    }

    /// Number of times the display list has been rebuilt (for invalidation tests).
    pub fn paint_count(&self) -> u64 {
        self.paint_count
    }

    /// Returns the page's display list in page coordinates (origin at the
    /// viewport's top-left), rebuilding it only when invalidated.
    pub fn display_list(&mut self) -> &DisplayList {
        if self.paint_dirty {
            self.display_list.clear();
            match self.document {
                Document::Blank => {
                    self.display_list
                        .fill_rect(Rect::from_size(self.viewport), Color::WHITE);
                }
            }
            self.paint_dirty = false;
            self.paint_count += 1;
        }
        &self.display_list
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_page_paints_viewport_white() {
        let mut page = Page::new(Size::new(800, 600));
        assert_eq!(page.document().url(), "about:blank");
        let list = page.display_list();
        assert_eq!(
            list.items(),
            [rustlite_paint::DisplayItem::SolidRect {
                rect: Rect::new(0, 0, 800, 600),
                color: Color::WHITE
            }]
        );
    }

    #[test]
    fn display_list_is_cached_until_invalidated() {
        let mut page = Page::new(Size::new(100, 100));
        page.display_list();
        page.display_list();
        assert_eq!(page.paint_count(), 1);

        page.set_viewport(Size::new(100, 100));
        assert!(!page.needs_paint(), "same size must not invalidate");

        page.set_viewport(Size::new(200, 100));
        assert!(page.needs_paint());
        page.display_list();
        assert_eq!(page.paint_count(), 2);
    }

    #[test]
    fn zero_viewport_paints_nothing() {
        let mut page = Page::new(Size::new(0, 0));
        assert!(page.display_list().is_empty());
    }
}

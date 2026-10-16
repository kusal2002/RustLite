//! Coordinates the page pipeline: document → style → (layout →) display list.
//!
//! Current scope (Phase 3): a page shows either `about:blank` or an HTML
//! document parsed by `rustlite-html`, with computed styles from
//! `rustlite-style`. Layout does not exist yet (Phase 4), so an HTML document
//! still paints as an empty white canvas. Styles are computed lazily and
//! invalidated when the viewport changes (viewport units depend on it).

use rustlite_dom::Document;
use rustlite_paint::{Color, DisplayList, Rect, Size};
use rustlite_style::{ComputedStyles, Stylist};

/// A parsed HTML document, its stylesheets, and where it came from.
#[derive(Debug)]
pub struct HtmlDocument {
    pub url: String,
    pub dom: Document,
    /// Number of HTML parse errors (repairs) encountered.
    pub parse_errors: usize,
    pub stylist: Stylist,
    styles: Option<ComputedStyles>,
}

/// What a page currently shows.
#[derive(Debug)]
pub enum Content {
    /// `about:blank`: an empty document with the initial white canvas.
    Blank,
    Html(Box<HtmlDocument>),
}

impl Content {
    pub fn url(&self) -> &str {
        match self {
            Content::Blank => "about:blank",
            Content::Html(doc) => &doc.url,
        }
    }
}

/// One page: content plus the viewport it is laid out against.
#[derive(Debug)]
pub struct Page {
    content: Content,
    viewport: Size,
    display_list: DisplayList,
    paint_dirty: bool,
    paint_count: u64,
    style_count: u64,
}

impl Page {
    pub fn new(viewport: Size) -> Self {
        Self {
            content: Content::Blank,
            viewport,
            display_list: DisplayList::new(),
            paint_dirty: true,
            paint_count: 0,
            style_count: 0,
        }
    }

    /// Number of full style recalculations (for invalidation tests).
    pub fn style_count(&self) -> u64 {
        self.style_count
    }

    /// The document's computed styles, recomputed if invalidated.
    /// `None` for `about:blank`.
    pub fn computed_styles(&mut self) -> Option<(&Document, &ComputedStyles)> {
        let Content::Html(doc) = &mut self.content else {
            return None;
        };
        if doc.styles.is_none() {
            let start = std::time::Instant::now();
            let viewport = (self.viewport.width as f32, self.viewport.height as f32);
            let (styles, stats) = rustlite_style::compute_styles(&doc.dom, &doc.stylist, viewport);
            tracing::info!(
                elements = stats.elements,
                shared = stats.shared,
                rules = stats.sheets.rules,
                author_sheets = stats.sheets.author_sheets,
                dropped_rules = stats.sheets.dropped_rules,
                skipped_at_rules = stats.sheets.skipped_at_rules,
                style_ms = start.elapsed().as_secs_f64() * 1000.0,
                "styles computed"
            );
            doc.styles = Some(styles);
            self.style_count += 1;
        }
        let doc = &**doc;
        doc.styles.as_ref().map(|s| (&doc.dom, s))
    }

    pub fn content(&self) -> &Content {
        &self.content
    }

    pub fn url(&self) -> &str {
        self.content.url()
    }

    /// Replaces the page content with `source` parsed as HTML.
    pub fn load_html(&mut self, url: impl Into<String>, source: &str) {
        let url = url.into();
        let start = std::time::Instant::now();
        let out = rustlite_html::parse_document(source);
        tracing::info!(
            %url,
            bytes = source.len(),
            nodes = out.document.node_count(),
            parse_errors = out.error_count,
            parse_ms = start.elapsed().as_secs_f64() * 1000.0,
            "HTML document parsed"
        );
        let stylist = rustlite_style::stylist_for(&out.document);
        self.content = Content::Html(Box::new(HtmlDocument {
            url,
            dom: out.document,
            parse_errors: out.error_count,
            stylist,
            styles: None,
        }));
        self.paint_dirty = true;
    }

    /// The document title per HTML §4.2.2: the first `<title>` element's text
    /// with ASCII whitespace stripped and collapsed. `None` if there is none.
    pub fn title(&self) -> Option<String> {
        let Content::Html(doc) = &self.content else {
            return None;
        };
        let dom = &doc.dom;
        let title = dom.find_element(dom.root(), "title")?;
        let text = dom.text_content(title);
        Some(
            text.split(|c: char| c.is_ascii_whitespace())
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" "),
        )
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
            // Viewport units make computed values depend on the viewport.
            if let Content::Html(doc) = &mut self.content {
                doc.styles = None;
            }
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
            // Both kinds of content paint the initial canvas; HTML content
            // itself is painted once style and layout exist.
            self.display_list
                .fill_rect(Rect::from_size(self.viewport), Color::WHITE);
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
        assert_eq!(page.url(), "about:blank");
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

    #[test]
    fn loading_html_replaces_content_and_invalidates() {
        let mut page = Page::new(Size::new(10, 10));
        page.display_list();
        page.load_html("test:doc", "<title>  Hello \n  world </title><p>x");
        assert!(page.needs_paint());
        assert_eq!(page.url(), "test:doc");
        assert_eq!(page.title().as_deref(), Some("Hello world"));
        let Content::Html(doc) = page.content() else {
            panic!("expected HTML content")
        };
        assert!(doc.dom.find_element(doc.dom.root(), "p").is_some());
    }

    #[test]
    fn styles_are_cached_and_invalidated_by_viewport() {
        let mut page = Page::new(Size::new(1000, 500));
        assert!(
            page.computed_styles().is_none(),
            "about:blank has no styles"
        );
        page.load_html("test:doc", "<style>p { margin-left: 10vw }</style><p>x");
        let margin = |page: &mut Page| {
            let (dom, styles) = page.computed_styles().unwrap();
            let p = dom.find_element(dom.root(), "p").unwrap();
            rustlite_style::resolved_value(styles.get(p).unwrap(), "margin-left").unwrap()
        };
        assert_eq!(margin(&mut page), "100px");
        assert_eq!(margin(&mut page), "100px");
        assert_eq!(page.style_count(), 1, "second access must hit the cache");
        page.set_viewport(Size::new(500, 500));
        assert_eq!(margin(&mut page), "50px");
        assert_eq!(page.style_count(), 2);
    }

    #[test]
    fn missing_title_is_none() {
        let mut page = Page::new(Size::new(10, 10));
        assert_eq!(page.title(), None);
        page.load_html("test:doc", "<p>no title");
        assert_eq!(page.title(), None);
    }
}

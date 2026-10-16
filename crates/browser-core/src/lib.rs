//! Coordinates the page pipeline: document → style → layout → display list.
//!
//! A [`Page`] shows either `about:blank` or an HTML document parsed by
//! `rustlite-html`, styled by `rustlite-style`, laid out and painted by
//! `rustlite-layout`. Each stage is computed lazily and cached; a viewport
//! change invalidates styles (viewport units) and layout, a scroll only
//! repaints. A [`Tab`] adds navigation: network loads, history, subresources
//! and error pages.

pub mod images;
mod tab;

pub use tab::{
    Initiator, NavState, NavigationError, Tab, error_page_html, load_id_space, parse_user_input,
};

use std::collections::HashMap;
use std::rc::Rc;

use rustlite_css::Stylesheet;
use rustlite_dom::{Document, NodeId};
use rustlite_fonts::FontContext;
use rustlite_layout::Images;
use rustlite_layout::LayoutTree;
use rustlite_net::{Destination, Url};
use rustlite_paint::{Color, DisplayList, GlyphRasterizer, NoGlyphs, Rect, Size};
use rustlite_style::{ComputedStyles, Stylist};

/// A parsed HTML document, its stylesheets, and where it came from.
#[derive(Debug)]
pub struct HtmlDocument {
    pub url: String,
    pub dom: Document,
    /// Number of HTML parse errors (repairs) encountered.
    pub parse_errors: usize,
    pub stylist: Stylist,
    /// Decoded `<img>` resources.
    pub images: Images,
    /// Base URL for relative references (document URL or `<base href>`).
    pub base_url: Option<Url>,
    /// Loaded `<link rel=stylesheet>` sheets, by link element.
    external_sheets: HashMap<NodeId, Stylesheet>,
    styles: Option<ComputedStyles>,
    layout: Option<LayoutTree>,
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
    /// Viewport in device px.
    viewport: Size,
    /// Device pixels per CSS px.
    device_scale: f32,
    /// Scroll offset in CSS px.
    scroll_y: f32,
    /// Created on first layout of an HTML document (loading system fonts is
    /// not free), unless supplied with [`Page::with_fonts`].
    fonts: Option<FontContext>,
    no_glyphs: NoGlyphs,
    display_list: DisplayList,
    paint_dirty: bool,
    paint_count: u64,
    style_count: u64,
    layout_count: u64,
}

impl Page {
    pub fn new(viewport: Size) -> Self {
        Self {
            content: Content::Blank,
            viewport,
            device_scale: 1.0,
            scroll_y: 0.0,
            fonts: None,
            no_glyphs: NoGlyphs,
            display_list: DisplayList::new(),
            paint_dirty: true,
            paint_count: 0,
            style_count: 0,
            layout_count: 0,
        }
    }

    /// A page that uses `fonts` instead of loading the system fonts.
    pub fn with_fonts(viewport: Size, fonts: FontContext) -> Self {
        Self {
            fonts: Some(fonts),
            ..Self::new(viewport)
        }
    }

    /// The viewport in CSS px (what layout and viewport units see).
    pub fn css_viewport(&self) -> (f32, f32) {
        (
            self.viewport.width as f32 / self.device_scale,
            self.viewport.height as f32 / self.device_scale,
        )
    }

    pub fn device_scale(&self) -> f32 {
        self.device_scale
    }

    /// Sets the device pixel ratio. Changing it changes the CSS viewport, so
    /// styles and layout are invalidated.
    pub fn set_device_scale(&mut self, scale: f32) {
        let scale = if scale.is_finite() && scale > 0.0 {
            scale
        } else {
            1.0
        };
        if scale != self.device_scale {
            self.device_scale = scale;
            self.invalidate_layout();
        }
    }

    fn invalidate_layout(&mut self) {
        self.paint_dirty = true;
        if let Content::Html(doc) = &mut self.content {
            doc.styles = None;
            doc.layout = None;
        }
        self.scroll_by(0.0);
    }

    /// Number of full style recalculations (for invalidation tests).
    pub fn style_count(&self) -> u64 {
        self.style_count
    }

    /// Number of full layouts (for invalidation tests).
    pub fn layout_count(&self) -> u64 {
        self.layout_count
    }

    fn ensure_styles(&mut self) {
        let Content::Html(doc) = &mut self.content else {
            return;
        };
        if doc.styles.is_some() {
            return;
        }
        let start = std::time::Instant::now();
        let viewport = (
            self.viewport.width as f32 / self.device_scale,
            self.viewport.height as f32 / self.device_scale,
        );
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
        doc.layout = None;
        self.style_count += 1;
    }

    /// The document's computed styles, recomputed if invalidated.
    /// `None` for `about:blank`.
    pub fn computed_styles(&mut self) -> Option<(&Document, &ComputedStyles)> {
        self.ensure_styles();
        let Content::Html(doc) = &self.content else {
            return None;
        };
        doc.styles.as_ref().map(|s| (&doc.dom, s))
    }

    /// The laid-out document, recomputed if invalidated. `None` for `about:blank`.
    pub fn layout(&mut self) -> Option<(&Document, &LayoutTree)> {
        self.ensure_styles();
        let Content::Html(doc) = &mut self.content else {
            return None;
        };
        if doc.layout.is_none() {
            let fonts = self
                .fonts
                .get_or_insert_with(FontContext::with_system_fonts);
            let start = std::time::Instant::now();
            let viewport = (
                self.viewport.width as f32 / self.device_scale,
                self.viewport.height as f32 / self.device_scale,
            );
            let styles = doc.styles.as_ref()?;
            let tree = rustlite_layout::layout_document_with_images(
                &doc.dom,
                styles,
                fonts,
                &doc.images,
                viewport,
            );
            tracing::info!(
                document_height = tree.document_height,
                layout_ms = start.elapsed().as_secs_f64() * 1000.0,
                "layout computed"
            );
            doc.layout = Some(tree);
            self.layout_count += 1;
        }
        let doc = &**doc;
        doc.layout.as_ref().map(|l| (&doc.dom, l))
    }

    pub fn content(&self) -> &Content {
        &self.content
    }

    pub fn url(&self) -> &str {
        self.content.url()
    }

    /// Replaces the page content with `source` parsed as HTML. Subresources
    /// are not loaded (see [`Page::load_html_with_base`]).
    pub fn load_html(&mut self, url: impl Into<String>, source: &str) {
        self.load_html_with_base(url, source, None);
    }

    /// Replaces the page content with `source` parsed as HTML, loading
    /// `<img>` files relative to `base_dir` (a local document's directory).
    pub fn load_html_with_base(
        &mut self,
        url: impl Into<String>,
        source: &str,
        base_dir: Option<&std::path::Path>,
    ) {
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
        let base_url = base_url_for(&out.document, Url::parse(&url).ok());
        let external_sheets = match base_dir {
            Some(dir) => load_local_stylesheets(&out.document, dir),
            None => HashMap::new(),
        };
        let stylist =
            rustlite_style::stylist_with_external(&out.document, |n| external_sheets.get(&n));
        let images = match base_dir {
            Some(dir) => {
                let start = std::time::Instant::now();
                let images = images::load_images(&out.document, dir);
                tracing::info!(
                    count = images.len(),
                    image_ms = start.elapsed().as_secs_f64() * 1000.0,
                    "images loaded"
                );
                images
            }
            None => Images::new(),
        };
        self.content = Content::Html(Box::new(HtmlDocument {
            url,
            dom: out.document,
            parse_errors: out.error_count,
            stylist,
            images,
            base_url,
            external_sheets,
            styles: None,
            layout: None,
        }));
        self.scroll_y = 0.0;
        self.paint_dirty = true;
    }

    /// The document's base URL, if it has one.
    pub fn base_url(&self) -> Option<&Url> {
        match &self.content {
            Content::Html(doc) => doc.base_url.as_ref(),
            Content::Blank => None,
        }
    }

    /// Network subresources the document needs: linked stylesheets and
    /// images, resolved against the base URL (http/https only), with the
    /// element(s) each one is for. Already-loaded resources are skipped.
    pub fn subresource_requests(&self) -> Vec<(Destination, Url, Vec<NodeId>)> {
        let Content::Html(doc) = &self.content else {
            return Vec::new();
        };
        let Some(base) = &doc.base_url else {
            return Vec::new();
        };
        let dom = &doc.dom;
        let mut out: Vec<(Destination, Url, Vec<NodeId>)> = Vec::new();
        let mut add = |dest: Destination, href: &str, node: NodeId| {
            let Ok(url) = base.join(href.trim()) else {
                return;
            };
            if !matches!(url.scheme(), "http" | "https") {
                return;
            }
            match out.iter_mut().find(|(d, u, _)| *d == dest && *u == url) {
                Some((_, _, nodes)) => nodes.push(node),
                None => out.push((dest, url, vec![node])),
            }
        };
        for n in dom.descendants(dom.root()) {
            let wanted = if rustlite_style::is_stylesheet_link(dom, n) {
                (!doc.external_sheets.contains_key(&n))
                    .then(|| dom.attribute(n, "href").map(|h| (Destination::Style, h)))
                    .flatten()
            } else if dom.is_element_named(n, "img") {
                (!doc.images.contains_key(&n))
                    .then(|| dom.attribute(n, "src").map(|s| (Destination::Image, s)))
                    .flatten()
            } else {
                None
            };
            if let Some((destination, reference)) = wanted {
                add(destination, reference, n);
            }
        }
        out
    }

    /// Applies a loaded stylesheet for `<link>` element `node`: rebuilds the
    /// cascade in document order and invalidates styles.
    pub fn set_external_stylesheet(&mut self, node: NodeId, css: &str) {
        let Content::Html(doc) = &mut self.content else {
            return;
        };
        doc.external_sheets
            .insert(node, rustlite_css::parse_stylesheet(css));
        let sheets = &doc.external_sheets;
        doc.stylist = rustlite_style::stylist_with_external(&doc.dom, |n| sheets.get(&n));
        doc.styles = None;
        doc.layout = None;
        self.paint_dirty = true;
    }

    /// Supplies a decoded image for `<img>` element `node`; invalidates layout.
    pub fn set_image(&mut self, node: NodeId, image: Rc<rustlite_paint::ImageData>) {
        let Content::Html(doc) = &mut self.content else {
            return;
        };
        doc.images.insert(node, image);
        doc.layout = None;
        self.paint_dirty = true;
    }

    /// The link (`<a href>` / `<area href>`) under a point of the page
    /// viewport in device px, resolved against the base URL.
    pub fn link_at(&mut self, x: f32, y: f32) -> Option<Url> {
        let (scale, scroll) = (self.device_scale, self.scroll_y);
        let (dom, tree) = self.layout()?;
        let node = rustlite_layout::hit_test(tree, x / scale, y / scale + scroll)?;
        let anchor = std::iter::once(node)
            .chain(dom.ancestors(node))
            .find(|&n| {
                matches!(dom.tag_name(n), Some("a" | "area")) && dom.attribute(n, "href").is_some()
            })?;
        let href = dom.attribute(anchor, "href")?.trim().to_owned();
        let base = self.base_url()?.clone();
        base.join(&href).ok()
    }

    /// Text matches for find-in-page, in document CSS px, document order.
    pub fn find(&mut self, query: &str) -> Vec<rustlite_layout::FindMatch> {
        if query.is_empty() || self.layout().is_none() {
            return Vec::new();
        }
        let Content::Html(doc) = &self.content else {
            return Vec::new();
        };
        let (Some(tree), Some(fonts)) = (&doc.layout, &mut self.fonts) else {
            return Vec::new();
        };
        rustlite_layout::find_text(tree, fonts, query)
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

    /// Updates the viewport, invalidating only if the size actually changed.
    pub fn set_viewport(&mut self, viewport: Size) {
        if self.viewport != viewport {
            tracing::debug!(?viewport, "page viewport changed");
            self.viewport = viewport;
            // Viewport units make computed values depend on the viewport, and
            // layout depends on its width.
            self.invalidate_layout();
        }
    }

    pub fn scroll_y(&self) -> f32 {
        self.scroll_y
    }

    /// Largest valid scroll offset (0 if the document fits the viewport).
    pub fn max_scroll(&mut self) -> f32 {
        let viewport_height = self.css_viewport().1;
        self.layout()
            .map_or(0.0, |(_, t)| (t.document_height - viewport_height).max(0.0))
    }

    /// Scrolls by `dy` px (positive = down), clamped to the document.
    /// Returns true if the position changed.
    pub fn scroll_by(&mut self, dy: f32) -> bool {
        self.scroll_to(self.scroll_y + dy)
    }

    /// Scrolls to `y`, clamped to the document. Returns true if it changed.
    pub fn scroll_to(&mut self, y: f32) -> bool {
        let clamped = y.clamp(0.0, self.max_scroll()).round();
        if clamped == self.scroll_y {
            return false;
        }
        self.scroll_y = clamped;
        self.paint_dirty = true;
        true
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
            let mut list = std::mem::take(&mut self.display_list);
            list.clear();
            let (scroll_y, scale) = (self.scroll_y, self.device_scale);
            match self.layout() {
                Some((_, tree)) => rustlite_layout::paint(tree, scroll_y, scale, &mut list),
                None => list.fill_rect(Rect::from_size(self.viewport), Color::WHITE),
            }
            self.display_list = list;
            self.paint_dirty = false;
            self.paint_count += 1;
        }
        &self.display_list
    }

    /// Glyph source for rasterizing this page's display list.
    pub fn glyph_rasterizer(&mut self) -> &mut dyn GlyphRasterizer {
        match &mut self.fonts {
            Some(f) => f,
            None => &mut self.no_glyphs,
        }
    }

    /// Removes the font context so another page can use it; see
    /// [`Page::set_fonts`]. Tabs in one window pass a single context around
    /// instead of each loading the system fonts and keeping its own caches.
    pub fn take_fonts(&mut self) -> Option<FontContext> {
        self.fonts.take()
    }

    /// Installs a font context. Font ids are per context: a page that
    /// already had a (different) context is laid out again.
    pub fn set_fonts(&mut self, fonts: FontContext) {
        if self.fonts.replace(fonts).is_some() {
            self.invalidate_layout();
        }
    }

    /// The page's font context (created on first use). Browser UI text
    /// shares it so one glyph cache serves the whole window.
    pub fn fonts(&mut self) -> &mut FontContext {
        self.fonts
            .get_or_insert_with(FontContext::with_system_fonts)
    }
}

/// The document's base URL: the first `<base href>` resolved against the
/// document URL, else the document URL (HTML §2.4.1, simplified).
fn base_url_for(dom: &Document, document_url: Option<Url>) -> Option<Url> {
    let base_href = dom
        .descendants(dom.root())
        .find(|&n| dom.is_element_named(n, "base") && dom.attribute(n, "href").is_some())
        .and_then(|n| dom.attribute(n, "href"));
    match (base_href, &document_url) {
        (Some(href), Some(doc_url)) => doc_url.join(href.trim()).ok().or(document_url),
        _ => document_url,
    }
}

/// Reads `<link rel=stylesheet>` files for a local document, under the same
/// directory policy as images.
fn load_local_stylesheets(
    dom: &Document,
    base_dir: &std::path::Path,
) -> HashMap<NodeId, Stylesheet> {
    const MAX_CSS_BYTES: u64 = 4 * 1024 * 1024;
    let mut sheets = HashMap::new();
    for n in dom.descendants(dom.root()) {
        if !rustlite_style::is_stylesheet_link(dom, n) {
            continue;
        }
        let Some(href) = dom.attribute(n, "href") else {
            continue;
        };
        let loaded = images::resolve_local(base_dir, href).ok().and_then(|path| {
            let size = std::fs::metadata(&path).ok()?.len();
            (size <= MAX_CSS_BYTES).then(|| std::fs::read(&path).ok())?
        });
        match loaded {
            Some(bytes) => {
                let css = rustlite_net::decode_text(&bytes, None).text;
                sheets.insert(n, rustlite_css::parse_stylesheet(&css));
            }
            None => tracing::warn!(href, "stylesheet not loaded"),
        }
    }
    sheets
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

    /// A page with no fonts loaded: layout of text-free content is still exact.
    fn fontless(viewport: Size) -> Page {
        Page::with_fonts(viewport, FontContext::new())
    }

    #[test]
    fn layout_is_cached_and_scroll_only_repaints() {
        let mut page = fontless(Size::new(200, 100));
        page.load_html(
            "test:doc",
            "<!DOCTYPE html><body style='margin:0'><div style='height: 300px; background: red'></div>",
        );
        let (_, tree) = page.layout().unwrap();
        assert_eq!(tree.document_height, 300.0);
        assert_eq!(page.max_scroll(), 200.0);
        page.display_list();
        assert!(page.scroll_by(50.0));
        assert!(page.needs_paint());
        page.display_list();
        assert_eq!(page.layout_count(), 1, "scrolling must not relayout");
        assert!(!page.scroll_by(-500.0) || page.scroll_y() == 0.0);
        assert_eq!(page.scroll_y(), 0.0);
        page.scroll_to(1e9);
        assert_eq!(page.scroll_y(), 200.0);
    }

    #[test]
    fn viewport_change_relayouts_and_clamps_scroll() {
        let mut page = fontless(Size::new(200, 100));
        page.load_html(
            "test:doc",
            "<!DOCTYPE html><body style='margin:0'><div style='height: 300px'></div>",
        );
        page.scroll_to(200.0);
        page.set_viewport(Size::new(200, 250));
        assert_eq!(page.scroll_y(), 50.0);
        assert_eq!(page.layout_count(), 2);
    }

    #[test]
    fn html_paints_backgrounds_with_scroll_offset() {
        let mut page = fontless(Size::new(100, 100));
        page.load_html(
            "test:doc",
            "<!DOCTYPE html><body style='margin:0'><div style='height: 40px'></div><div style='height: 300px; background: rgb(0, 0, 255)'></div>",
        );
        page.scroll_to(30.0);
        let blue = page.display_list().items().iter().find_map(|i| match i {
            rustlite_paint::DisplayItem::SolidRect { rect, color }
                if *color == Color::rgb(0, 0, 255) =>
            {
                Some(*rect)
            }
            _ => None,
        });
        assert_eq!(blue, Some(Rect::new(0, 10, 100, 300)));
    }

    #[test]
    fn device_scale_lays_out_in_css_px_and_paints_in_device_px() {
        let mut page = fontless(Size::new(200, 100));
        page.set_device_scale(2.0);
        assert_eq!(page.css_viewport(), (100.0, 50.0));
        page.load_html(
            "test:doc",
            "<!DOCTYPE html><body style='margin:0'><div style='width: 50vw; height: 10px; background: rgb(0, 0, 255)'></div>",
        );
        let blue = page.display_list().items().iter().find_map(|i| match i {
            rustlite_paint::DisplayItem::SolidRect { rect, color }
                if *color == Color::rgb(0, 0, 255) =>
            {
                Some(*rect)
            }
            _ => None,
        });
        // 50vw of a 100 CSS px viewport = 50 CSS px = 100 device px.
        assert_eq!(blue, Some(Rect::new(0, 0, 100, 20)));
    }
}

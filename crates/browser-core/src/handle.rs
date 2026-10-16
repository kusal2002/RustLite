//! [`PageHandle`]: one API over a page in this process or in a renderer
//! process, so `Tab` and the browser window do not care which.

use std::path::Path;
use std::time::Instant;

use rustlite_dom::{Document, NodeId};
use rustlite_fonts::FontContext;
use rustlite_layout::{FindMatch, LayoutTree};
use rustlite_net::{Destination, Url};
use rustlite_paint::{DisplayList, GlyphRasterizer, Size};

use crate::remote::RemotePage;
use crate::{InputOutcome, MemoryReport, Page, PageKey};

/// A page in this process (tests, headless rendering, `--single-process`)
/// or in a sandboxed renderer process (the default window).
#[derive(Debug)]
pub enum PageHandle {
    Local(Box<Page>),
    Remote(Box<RemotePage>),
}

impl From<Page> for PageHandle {
    fn from(page: Page) -> Self {
        PageHandle::Local(Box::new(page))
    }
}

impl From<RemotePage> for PageHandle {
    fn from(page: RemotePage) -> Self {
        PageHandle::Remote(Box::new(page))
    }
}

macro_rules! delegate {
    ($self:ident, $p:ident => $e:expr) => {
        match $self {
            PageHandle::Local($p) => $e,
            PageHandle::Remote($p) => $e,
        }
    };
}

impl PageHandle {
    /// The in-process page, if this is one (layout and DOM inspection are
    /// only available there).
    pub fn as_local_mut(&mut self) -> Option<&mut Page> {
        match self {
            PageHandle::Local(p) => Some(p),
            PageHandle::Remote(_) => None,
        }
    }

    pub fn as_remote(&self) -> Option<&RemotePage> {
        match self {
            PageHandle::Remote(p) => Some(p),
            PageHandle::Local(_) => None,
        }
    }

    /// DOM and layout of an in-process page (`None` for remote pages).
    pub fn layout(&mut self) -> Option<(&Document, &LayoutTree)> {
        self.as_local_mut()?.layout()
    }

    pub fn load_html(&mut self, url: impl Into<String>, html: &str) {
        delegate!(self, p => p.load_html(url, html))
    }

    pub fn load_html_with_base(&mut self, url: impl Into<String>, html: &str, base: Option<&Path>) {
        delegate!(self, p => p.load_html_with_base(url, html, base))
    }

    pub fn set_javascript_enabled(&mut self, enabled: bool) {
        delegate!(self, p => p.set_javascript_enabled(enabled))
    }

    pub fn title(&self) -> Option<String> {
        delegate!(self, p => p.title())
    }

    pub fn mutation_count(&self) -> u64 {
        delegate!(self, p => p.mutation_count())
    }

    /// Changes whenever the layout was recomputed (forces layout locally).
    pub fn layout_generation(&mut self) -> u64 {
        match self {
            PageHandle::Local(p) => {
                p.layout();
                p.layout_count()
            }
            PageHandle::Remote(p) => p.layout_generation(),
        }
    }

    pub fn fonts(&mut self) -> &mut FontContext {
        delegate!(self, p => p.fonts())
    }

    pub fn take_fonts(&mut self) -> Option<FontContext> {
        delegate!(self, p => p.take_fonts())
    }

    pub fn set_fonts(&mut self, fonts: FontContext) {
        delegate!(self, p => p.set_fonts(fonts))
    }

    pub fn glyph_rasterizer(&mut self) -> &mut dyn GlyphRasterizer {
        delegate!(self, p => p.glyph_rasterizer())
    }

    pub fn viewport(&self) -> Size {
        delegate!(self, p => p.viewport())
    }

    pub fn set_viewport(&mut self, viewport: Size) {
        delegate!(self, p => p.set_viewport(viewport))
    }

    pub fn device_scale(&self) -> f32 {
        delegate!(self, p => p.device_scale())
    }

    pub fn set_device_scale(&mut self, scale: f32) {
        delegate!(self, p => p.set_device_scale(scale))
    }

    pub fn css_viewport(&self) -> (f32, f32) {
        delegate!(self, p => p.css_viewport())
    }

    pub fn scroll_y(&self) -> f32 {
        delegate!(self, p => p.scroll_y())
    }

    pub fn max_scroll(&mut self) -> f32 {
        delegate!(self, p => p.max_scroll())
    }

    pub fn scroll_by(&mut self, dy: f32) -> bool {
        delegate!(self, p => p.scroll_by(dy))
    }

    pub fn scroll_to(&mut self, y: f32) -> bool {
        delegate!(self, p => p.scroll_to(y))
    }

    pub fn needs_paint(&self) -> bool {
        delegate!(self, p => p.needs_paint())
    }

    pub fn display_list(&mut self) -> &DisplayList {
        delegate!(self, p => p.display_list())
    }

    pub fn subresource_requests(&mut self) -> Vec<(Destination, Url, Vec<NodeId>)> {
        delegate!(self, p => p.subresource_requests())
    }

    pub fn set_external_stylesheet(&mut self, node: NodeId, css: &str) {
        delegate!(self, p => p.set_external_stylesheet(node, css))
    }

    pub fn set_image_bytes(&mut self, node: NodeId, bytes: &[u8]) -> bool {
        delegate!(self, p => p.set_image_bytes(node, bytes))
    }

    pub fn set_script_source(&mut self, node: NodeId, source: Option<String>) {
        delegate!(self, p => p.set_script_source(node, source))
    }

    pub fn link_at(&mut self, x: f32, y: f32) -> Option<Url> {
        delegate!(self, p => p.link_at(x, y))
    }

    pub fn click(&mut self, x: f32, y: f32) -> InputOutcome {
        delegate!(self, p => p.click(x, y))
    }

    pub fn key_down(&mut self, key: &PageKey) -> InputOutcome {
        delegate!(self, p => p.key_down(key))
    }

    pub fn key_up(&mut self, key: &PageKey) {
        delegate!(self, p => p.key_up(key))
    }

    pub fn focused(&self) -> Option<NodeId> {
        delegate!(self, p => p.focused())
    }

    pub fn next_timer(&mut self) -> Option<Instant> {
        delegate!(self, p => p.next_timer())
    }

    pub fn run_timers(&mut self, now: Instant) -> bool {
        delegate!(self, p => p.run_timers(now))
    }

    pub fn wants_animation_frame(&mut self) -> bool {
        delegate!(self, p => p.wants_animation_frame())
    }

    pub fn run_animation_frame(&mut self, now: Instant) -> bool {
        delegate!(self, p => p.run_animation_frame(now))
    }

    pub fn find(&mut self, query: &str) -> Vec<FindMatch> {
        delegate!(self, p => p.find(query))
    }

    pub fn memory_report(&mut self) -> MemoryReport {
        delegate!(self, p => p.memory_report())
    }

    pub fn console(&mut self) -> Vec<String> {
        delegate!(self, p => p.console())
    }
}

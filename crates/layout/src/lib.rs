//! Layout for RustLite: box tree construction, block formatting (CSS 2.1
//! §9–10, including margin collapsing), inline formatting with line breaking,
//! and painting of the laid-out tree into a display list.
//!
//! Scope of this first layer and its known gaps are in docs/compatibility.md.
//! In short: normal flow only (no floats, positioning, flex, grid or tables,
//! which are laid out as blocks), `white-space: normal` only, breaks only at
//! spaces, no replaced elements, no inline-level padding/borders.

mod block;
mod build;
mod geometry;
mod inline;
mod paint;

use std::collections::HashMap;
use std::rc::Rc;

use rustlite_dom::{Document, NodeId};
use rustlite_fonts::FontContext;
use rustlite_paint::GlyphRun;
use rustlite_style::{ComputedStyle, ComputedStyles};

pub use geometry::{Edges, RectF};
pub use paint::{canvas_background, paint};

/// A block-level box (element or anonymous).
#[derive(Debug)]
pub struct BlockBox {
    /// `None` for anonymous boxes.
    pub node: Option<NodeId>,
    pub style: Rc<ComputedStyle>,
    pub children: BlockChildren,
    /// Border box. Relative to the parent's content box during layout and
    /// absolute (document coordinates) afterwards.
    pub rect: RectF,
    pub border: Edges,
    pub padding: Edges,
    pub margin: Edges,
}

impl BlockBox {
    pub fn content_rect(&self) -> RectF {
        self.rect.inset(&self.border).inset(&self.padding)
    }

    pub fn padding_rect(&self) -> RectF {
        self.rect.inset(&self.border)
    }
}

#[derive(Debug)]
pub enum BlockChildren {
    Blocks(Vec<BlockBox>),
    Inline(InlineContent),
    Empty,
}

/// One piece of inline content: text, or a forced line break.
#[derive(Debug)]
pub enum InlineItem {
    Text {
        text: String,
        style: Rc<ComputedStyle>,
        /// Inline elements enclosing this text, outermost first (for geometry).
        ancestors: Rc<[NodeId]>,
    },
    LineBreak {
        node: NodeId,
        style: Rc<ComputedStyle>,
    },
}

/// Inline formatting context content and the line boxes it produced.
#[derive(Debug, Default)]
pub struct InlineContent {
    pub items: Vec<InlineItem>,
    pub lines: Vec<LineBox>,
}

#[derive(Debug, Clone)]
pub struct LineBox {
    /// Relative to the block's content box during layout, absolute afterwards.
    pub rect: RectF,
    pub fragments: Vec<TextFragment>,
}

/// Contiguous text from one inline item on one line.
#[derive(Debug, Clone)]
pub struct TextFragment {
    /// Index into [`InlineContent::items`].
    pub item: usize,
    /// The content area (ascent + descent of the font), like browsers report
    /// for inline boxes.
    pub rect: RectF,
    pub baseline: f32,
    /// Glyph runs with their x offset from `rect.x`.
    pub runs: Vec<(f32, GlyphRun)>,
}

/// The laid-out document.
#[derive(Debug, Default)]
pub struct LayoutTree {
    pub root: Option<BlockBox>,
    /// Height of the scrollable document (root margin box bottom).
    pub document_height: f32,
    pub viewport: (f32, f32),
}

/// Inputs shared by every layout function.
pub(crate) struct LayoutContext<'a> {
    pub fonts: &'a mut FontContext,
}

/// Builds and lays out the box tree for `doc` in a viewport of `viewport` px.
pub fn layout_document(
    doc: &Document,
    styles: &ComputedStyles,
    fonts: &mut FontContext,
    viewport: (f32, f32),
) -> LayoutTree {
    let mut root = build::build_box_tree(doc, styles);
    let mut ctx = LayoutContext { fonts };
    let mut document_height = 0.0;
    if let Some(root) = root.as_mut() {
        let outcome = block::layout_root(root, viewport, &mut ctx);
        // The root establishes a block formatting context: its margins do not
        // collapse with anything, so it sits at its own top margin.
        let top = root.margin.top;
        root.rect.x = root.margin.left;
        root.rect.y = top;
        geometry::make_absolute(root, 0.0, 0.0);
        document_height = top + outcome.height + root.margin.bottom;
    }
    LayoutTree {
        root,
        document_height,
        viewport,
    }
}

/// Border-box rectangles of every element that generated boxes or text.
/// Inline elements get the union of their text fragments.
pub fn element_rects(tree: &LayoutTree) -> HashMap<NodeId, RectF> {
    let mut rects = HashMap::new();
    if let Some(root) = &tree.root {
        geometry::collect_rects(root, &mut rects);
    }
    rects
}

/// Dumps `<tag#id> x y width height` for every element in tree order, the
/// format the geometry oracle produces from `getBoundingClientRect()`.
pub fn dump_geometry(doc: &Document, tree: &LayoutTree) -> String {
    let rects = element_rects(tree);
    let mut out = String::new();
    for n in doc.descendants(doc.root()) {
        let Some(e) = doc.element(n) else { continue };
        let r = rects.get(&n).copied().unwrap_or_default();
        out.push('<');
        out.push_str(&e.name);
        if let Some(id) = e.attribute("id") {
            out.push('#');
            out.push_str(id);
        }
        out.push_str(&format!(
            "> {} {} {} {}\n",
            fmt(r.x),
            fmt(r.y),
            fmt(r.width),
            fmt(r.height)
        ));
    }
    out
}

fn fmt(v: f32) -> String {
    let rounded = (v * 1000.0).round() / 1000.0;
    let s = format!("{rounded:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

#[cfg(test)]
mod tests;

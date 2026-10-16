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
mod find;
mod geometry;
mod inline;
mod paint;
mod replaced;

use std::collections::HashMap;
use std::rc::Rc;

use rustlite_dom::{Document, NodeId};
use rustlite_fonts::FontContext;
use rustlite_paint::{GlyphRun, ImageData};
use rustlite_style::{ComputedStyle, ComputedStyles};

pub use find::{FindMatch, find_text};
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
    /// The `::marker` of a list item (outside position).
    pub marker: Option<Marker>,
    /// Set for block-level replaced elements (e.g. `img { display: block }`).
    pub replaced: Option<ReplacedContent>,
}

/// An outside list marker: shaped text placed to the left of the list item's
/// content box, on its first baseline. It does not affect layout.
#[derive(Debug, Clone)]
pub struct Marker {
    pub text: String,
    /// Start of the marker text; relative to the item's content box during
    /// layout, absolute afterwards.
    pub x: f32,
    pub baseline: f32,
    pub runs: Vec<(f32, GlyphRun)>,
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
    /// An atomic inline-level box: a replaced element such as `<img>`.
    Atomic {
        node: NodeId,
        style: Rc<ComputedStyle>,
        image: Option<Rc<ImageData>>,
    },
}

/// Decoded images for `<img>` elements, keyed by element.
pub type Images = HashMap<NodeId, Rc<ImageData>>;

/// Content of a block-level replaced element.
#[derive(Debug, Clone)]
pub struct ReplacedContent {
    /// `None` when the image is missing or failed to load.
    pub image: Option<Rc<ImageData>>,
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
    /// The text the runs show (collapsed spaces included), for find-in-page.
    /// Empty for atomic inlines and line breaks.
    pub text: String,
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
    layout_document_with_images(doc, styles, fonts, &Images::new(), viewport)
}

/// Like [`layout_document`], with decoded images for `<img>` elements.
pub fn layout_document_with_images(
    doc: &Document,
    styles: &ComputedStyles,
    fonts: &mut FontContext,
    images: &Images,
    viewport: (f32, f32),
) -> LayoutTree {
    let mut root = build::build_box_tree(doc, styles, images);
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

/// The innermost element at document point (`x`, `y`) in CSS px: the
/// innermost inline element of the text under the point, an image, or else
/// the deepest block containing it. Later content wins, matching paint order.
pub fn hit_test(tree: &LayoutTree, x: f32, y: f32) -> Option<NodeId> {
    let inside = |r: &RectF| x >= r.x && x < r.right() && y >= r.y && y < r.bottom();
    let mut best = None;
    let mut stack: Vec<&BlockBox> = tree.root.iter().collect();
    while let Some(b) = stack.pop() {
        if !inside(&b.rect) {
            // Content can overflow its block, so descendants are still searched.
        } else if b.node.is_some() {
            best = b.node;
        }
        match &b.children {
            BlockChildren::Blocks(kids) => stack.extend(kids.iter().rev()),
            BlockChildren::Inline(inline) => {
                for f in inline.lines.iter().flat_map(|l| &l.fragments) {
                    if !inside(&f.rect) {
                        continue;
                    }
                    best = match &inline.items[f.item] {
                        InlineItem::Text { ancestors, .. } => ancestors.last().copied().or(b.node),
                        InlineItem::Atomic { node, .. } | InlineItem::LineBreak { node, .. } => {
                            Some(*node)
                        }
                    };
                }
            }
            BlockChildren::Empty => {}
        }
    }
    best
}

/// Size of a layout tree, for memory reporting.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TreeSize {
    pub boxes: usize,
    pub lines: usize,
    pub fragments: usize,
    pub glyphs: usize,
    /// Approximate heap bytes (boxes, lines, fragments, fragment text, glyph
    /// arrays). Styles are shared with the style system and not counted.
    pub bytes: usize,
}

/// Counts boxes, lines, fragments and glyphs in `tree`.
pub fn tree_size(tree: &LayoutTree) -> TreeSize {
    use std::mem::size_of;
    let mut size = TreeSize::default();
    let mut stack: Vec<&BlockBox> = tree.root.iter().collect();
    while let Some(b) = stack.pop() {
        size.boxes += 1;
        size.bytes += size_of::<BlockBox>();
        match &b.children {
            BlockChildren::Blocks(kids) => stack.extend(kids.iter()),
            BlockChildren::Inline(inline) => {
                size.bytes += inline.items.capacity() * size_of::<InlineItem>();
                for item in &inline.items {
                    if let InlineItem::Text { text, .. } = item {
                        size.bytes += text.capacity();
                    }
                }
                for line in &inline.lines {
                    size.lines += 1;
                    size.bytes += size_of::<LineBox>();
                    for f in &line.fragments {
                        size.fragments += 1;
                        size.bytes += size_of::<TextFragment>() + f.text.capacity();
                        for (_, run) in &f.runs {
                            size.glyphs += run.glyphs.len();
                            size.bytes +=
                                size_of::<(f32, GlyphRun)>() + std::mem::size_of_val(&*run.glyphs);
                        }
                    }
                }
            }
            BlockChildren::Empty => {}
        }
    }
    size
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

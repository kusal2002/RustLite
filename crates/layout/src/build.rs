//! Box tree construction (CSS 2.1 §9.2): block boxes, anonymous block boxes,
//! and inline content.

use std::rc::Rc;

use rustlite_dom::{Document, NodeData, NodeId};
use rustlite_style::properties::Display;
use rustlite_style::{ComputedStyle, ComputedStyles};

use crate::{BlockBox, BlockChildren, Edges, InlineContent, InlineItem, RectF};

/// Nesting beyond this many block boxes is flattened into inline content of
/// the deepest allowed block. Layout and paint recurse per block level, so
/// this bounds stack use on hostile input (debug builds use large frames).
/// Blink similarly caps parser DOM depth at 512.
const MAX_BLOCK_DEPTH: usize = 200;

enum Child {
    Block(BlockBox),
    Inline(InlineItem),
}

fn is_block_level(d: Display) -> bool {
    !matches!(
        d,
        Display::Inline | Display::InlineBlock | Display::None | Display::Contents
    )
}

pub(crate) fn new_block(
    node: Option<NodeId>,
    style: Rc<ComputedStyle>,
    children: BlockChildren,
) -> BlockBox {
    BlockBox {
        node,
        style,
        children,
        rect: RectF::default(),
        border: Edges::default(),
        padding: Edges::default(),
        margin: Edges::default(),
    }
}

/// Builds the box tree rooted at the document element.
pub(crate) fn build_box_tree(doc: &Document, styles: &ComputedStyles) -> Option<BlockBox> {
    let root = doc
        .children(doc.root())
        .find(|&n| doc.element(n).is_some())?;
    let style = styles.get_rc(root)?;
    if style.display == Display::None {
        return None;
    }
    Some(build_block(doc, styles, root, style, 0))
}

fn build_block(
    doc: &Document,
    styles: &ComputedStyles,
    node: NodeId,
    style: Rc<ComputedStyle>,
    depth: usize,
) -> BlockBox {
    let mut children = Vec::new();
    collect(doc, styles, node, &style, &[], depth, &mut children);
    let content = finish_children(children, &style);
    new_block(Some(node), style, content)
}

fn is_collapsible_whitespace(item: &InlineItem) -> bool {
    match item {
        InlineItem::Text { text, .. } => text
            .chars()
            .all(|c| matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0C')),
        InlineItem::LineBreak { .. } => false,
    }
}

/// Turns a mixed child list into either inline content or blocks, wrapping
/// inline runs between blocks in anonymous block boxes (CSS 2.1 §9.2.1.1).
fn finish_children(children: Vec<Child>, parent_style: &Rc<ComputedStyle>) -> BlockChildren {
    if children.iter().all(|c| matches!(c, Child::Inline(_))) {
        let items: Vec<InlineItem> = children
            .into_iter()
            .filter_map(|c| match c {
                Child::Inline(i) => Some(i),
                Child::Block(_) => None,
            })
            .collect();
        if items.is_empty() || items.iter().all(is_collapsible_whitespace) {
            return BlockChildren::Empty;
        }
        return BlockChildren::Inline(InlineContent {
            items,
            lines: Vec::new(),
        });
    }

    let mut blocks = Vec::new();
    let mut run: Vec<InlineItem> = Vec::new();
    let flush = |run: &mut Vec<InlineItem>, blocks: &mut Vec<BlockBox>| {
        // A run of only collapsible whitespace between blocks generates no box.
        if !run.is_empty() && !run.iter().all(is_collapsible_whitespace) {
            blocks.push(new_block(
                None,
                anonymous_style(parent_style),
                BlockChildren::Inline(InlineContent {
                    items: std::mem::take(run),
                    lines: Vec::new(),
                }),
            ));
        }
        run.clear();
    };
    for c in children {
        match c {
            Child::Inline(i) => run.push(i),
            Child::Block(b) => {
                flush(&mut run, &mut blocks);
                blocks.push(b);
            }
        }
    }
    flush(&mut run, &mut blocks);
    if blocks.is_empty() {
        BlockChildren::Empty
    } else {
        BlockChildren::Blocks(blocks)
    }
}

/// Anonymous boxes inherit inherited properties and use initial values for
/// the rest (CSS 2.1 §9.2.1.1).
fn anonymous_style(parent: &ComputedStyle) -> Rc<ComputedStyle> {
    let initial = ComputedStyle::initial();
    Rc::new(ComputedStyle {
        display: Display::Block,
        color: parent.color,
        font_size: parent.font_size,
        font_weight: parent.font_weight,
        font_style: parent.font_style,
        font_family: parent.font_family.clone(),
        line_height: parent.line_height,
        text_align: parent.text_align,
        ..initial
    })
}

/// Inline elements remembered per text item for geometry reporting. Deeper
/// chains keep only the innermost ones, so memory stays linear in the input.
const MAX_INLINE_ANCESTORS: usize = 32;

/// Collects the children of `parent` (a block container or a transparent
/// element) into `out`. Inline-level descendants are walked with an explicit
/// stack, so arbitrarily deep inline nesting cannot overflow the call stack;
/// only block-level children recurse, bounded by [`MAX_BLOCK_DEPTH`].
fn collect(
    doc: &Document,
    styles: &ComputedStyles,
    parent: NodeId,
    parent_style: &Rc<ComputedStyle>,
    inline_ancestors: &[NodeId],
    depth: usize,
    out: &mut Vec<Child>,
) {
    struct Frame {
        next: Option<NodeId>,
        style: Rc<ComputedStyle>,
        ancestors: Rc<[NodeId]>,
    }
    let mut stack = vec![Frame {
        next: doc.first_child(parent),
        style: parent_style.clone(),
        ancestors: inline_ancestors.into(),
    }];
    while let Some(frame) = stack.last_mut() {
        let Some(child) = frame.next else {
            stack.pop();
            continue;
        };
        frame.next = doc.next_sibling(child);
        let (frame_style, frame_ancestors) = (frame.style.clone(), frame.ancestors.clone());
        match doc.data(child) {
            NodeData::Text(t) => out.push(Child::Inline(InlineItem::Text {
                text: t.clone(),
                style: frame_style,
                ancestors: frame_ancestors,
            })),
            NodeData::Element(e) => {
                let Some(style) = styles.get_rc(child) else {
                    continue;
                };
                match style.display {
                    Display::None => {}
                    Display::Contents => stack.push(Frame {
                        next: doc.first_child(child),
                        style,
                        ancestors: frame_ancestors,
                    }),
                    d if !is_block_level(d) || depth >= MAX_BLOCK_DEPTH => {
                        if &*e.name == "br" {
                            out.push(Child::Inline(InlineItem::LineBreak { node: child, style }));
                            continue;
                        }
                        let keep = frame_ancestors.len().min(MAX_INLINE_ANCESTORS - 1);
                        let mut chain = frame_ancestors[frame_ancestors.len() - keep..].to_vec();
                        chain.push(child);
                        stack.push(Frame {
                            next: doc.first_child(child),
                            style,
                            ancestors: chain.into(),
                        });
                    }
                    _ => out.push(Child::Block(build_block(
                        doc,
                        styles,
                        child,
                        style,
                        depth + 1,
                    ))),
                }
            }
            _ => {}
        }
    }
}

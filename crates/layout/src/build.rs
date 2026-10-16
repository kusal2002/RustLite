//! Box tree construction (CSS 2.1 §9.2): block boxes, anonymous block boxes,
//! and inline content.

use std::rc::Rc;

use rustlite_dom::{Document, NodeData, NodeId};
use rustlite_style::properties::{Display, ListStyleType};
use rustlite_style::{ComputedStyle, ComputedStyles};

use crate::{
    BlockBox, BlockChildren, Edges, Images, InlineContent, InlineItem, RectF, ReplacedContent,
};

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
        marker: None,
        replaced: None,
    }
}

/// Marker text for a list item (CSS Lists 3 §3, counter styles from CSS
/// Counter Styles 3 §6), including the trailing space browsers render.
pub(crate) fn marker_text(ty: ListStyleType, ordinal: i64) -> Option<String> {
    fn alphabetic(mut n: i64, upper: bool) -> Option<String> {
        if n < 1 {
            return None;
        }
        let mut chars = Vec::new();
        while n > 0 {
            n -= 1;
            let base = if upper { b'A' } else { b'a' };
            // n % 26 is in 0..26, so the cast cannot truncate.
            chars.push((base + (n % 26) as u8) as char);
            n /= 26;
        }
        Some(chars.into_iter().rev().collect())
    }
    fn roman(n: i64, upper: bool) -> Option<String> {
        if !(1..=3999).contains(&n) {
            return None;
        }
        const TABLE: [(i64, &str); 13] = [
            (1000, "m"),
            (900, "cm"),
            (500, "d"),
            (400, "cd"),
            (100, "c"),
            (90, "xc"),
            (50, "l"),
            (40, "xl"),
            (10, "x"),
            (9, "ix"),
            (5, "v"),
            (4, "iv"),
            (1, "i"),
        ];
        let mut n = n;
        let mut s = String::new();
        for (value, digits) in TABLE {
            while n >= value {
                s.push_str(digits);
                n -= value;
            }
        }
        Some(if upper { s.to_ascii_uppercase() } else { s })
    }
    let numbered =
        |text: Option<String>| format!("{}. ", text.unwrap_or_else(|| ordinal.to_string()));
    Some(match ty {
        ListStyleType::None => return None,
        ListStyleType::Disc => "\u{2022} ".into(),
        ListStyleType::Circle => "\u{25E6} ".into(),
        ListStyleType::Square => "\u{25AA} ".into(),
        ListStyleType::Decimal => numbered(None),
        ListStyleType::LowerAlpha | ListStyleType::LowerLatin => {
            numbered(alphabetic(ordinal, false))
        }
        ListStyleType::UpperAlpha | ListStyleType::UpperLatin => {
            numbered(alphabetic(ordinal, true))
        }
        ListStyleType::LowerRoman => numbered(roman(ordinal, false)),
        ListStyleType::UpperRoman => numbered(roman(ordinal, true)),
    })
}

/// Builds the box tree rooted at the document element.
/// Everything box construction reads.
pub(crate) struct Inputs<'a> {
    doc: &'a Document,
    styles: &'a ComputedStyles,
    images: &'a Images,
}

pub(crate) fn build_box_tree(
    doc: &Document,
    styles: &ComputedStyles,
    images: &Images,
) -> Option<BlockBox> {
    let inputs = Inputs {
        doc,
        styles,
        images,
    };
    let root = doc
        .children(doc.root())
        .find(|&n| doc.element(n).is_some())?;
    let style = styles.get_rc(root)?;
    if style.display == Display::None {
        return None;
    }
    Some(build_block(&inputs, root, style, 0, 1))
}

fn build_block(
    inputs: &Inputs<'_>,
    node: NodeId,
    style: Rc<ComputedStyle>,
    depth: usize,
    ordinal: i64,
) -> BlockBox {
    let (doc, images) = (inputs.doc, inputs.images);
    if doc.is_element_named(node, "img") {
        let mut block = new_block(Some(node), style, BlockChildren::Empty);
        block.replaced = Some(ReplacedContent {
            image: images.get(&node).cloned(),
        });
        return block;
    }
    let mut children = Vec::new();
    collect(inputs, node, &style, &[], depth, &mut children);
    let content = finish_children(children, &style);
    let marker = (style.display == Display::ListItem)
        .then(|| marker_text(style.list_style_type, ordinal))
        .flatten();
    let mut block = new_block(Some(node), style, content);
    block.marker = marker.map(|text| crate::Marker {
        text,
        x: 0.0,
        baseline: 0.0,
        runs: Vec::new(),
    });
    block
}

/// The first list-item ordinal among `parent`'s children: `<ol start>`, else 1.
fn list_start(doc: &Document, parent: NodeId) -> i64 {
    if doc.is_element_named(parent, "ol") {
        doc.attribute(parent, "start")
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(1)
    } else {
        1
    }
}

fn is_collapsible_whitespace(item: &InlineItem) -> bool {
    match item {
        InlineItem::Text { text, .. } => text
            .chars()
            .all(|c| matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0C')),
        InlineItem::LineBreak { .. } | InlineItem::Atomic { .. } => false,
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
        list_style_type: parent.list_style_type,
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
    inputs: &Inputs<'_>,
    parent: NodeId,
    parent_style: &Rc<ComputedStyle>,
    inline_ancestors: &[NodeId],
    depth: usize,
    out: &mut Vec<Child>,
) {
    let (doc, styles, images) = (inputs.doc, inputs.styles, inputs.images);
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
    // Simplified list-item counter (CSS Lists 3 §4.6): increments per list
    // item child; `<li value>` resets it.
    let mut counter = list_start(doc, parent);
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
                        if &*e.name == "img" {
                            out.push(Child::Inline(InlineItem::Atomic {
                                node: child,
                                style,
                                image: images.get(&child).cloned(),
                                control: None,
                            }));
                            continue;
                        }
                        if let Some(kind) = doc.control_kind(child) {
                            // Controls are atomic: a button's children show
                            // as its (text-only) label, not as inline content.
                            out.push(Child::Inline(InlineItem::Atomic {
                                node: child,
                                style,
                                image: None,
                                control: Some(crate::controls::Control::from_dom(doc, child, kind)),
                            }));
                            continue;
                        }
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
                    d => {
                        let mut ordinal = 0;
                        if d == Display::ListItem {
                            ordinal = e
                                .attribute("value")
                                .and_then(|v| v.trim().parse().ok())
                                .unwrap_or(counter);
                            counter = ordinal + 1;
                        }
                        out.push(Child::Block(build_block(
                            inputs,
                            child,
                            style,
                            depth + 1,
                            ordinal,
                        )));
                    }
                }
            }
            _ => {}
        }
    }
}

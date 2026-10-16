//! Find-in-page: locates text matches in the laid-out tree.
//!
//! Each inline formatting context is searched as one string (its fragments in
//! line order, with a space where a line wrapped), so matches may span
//! inline elements and soft line breaks but never block boundaries. Matching
//! is case-insensitive by simple per-character lowercasing.

use rustlite_fonts::FontContext;

use crate::inline::style_font;
use crate::{BlockBox, BlockChildren, InlineContent, InlineItem, LayoutTree, RectF};

/// One match: highlight rectangles in document CSS px (more than one when
/// the match crosses fragments or lines).
pub type FindMatch = Vec<RectF>;

/// Finds every non-overlapping occurrence of `query`, in document order.
pub fn find_text(tree: &LayoutTree, fonts: &mut FontContext, query: &str) -> Vec<FindMatch> {
    let needle: Vec<char> = query.chars().map(fold).collect();
    let mut out = Vec::new();
    if needle.is_empty() {
        return out;
    }
    let mut stack: Vec<&BlockBox> = tree.root.iter().collect();
    while let Some(b) = stack.pop() {
        if let BlockChildren::Inline(inline) = &b.children {
            search(inline, fonts, &needle, &mut out);
        }
        stack.extend(b.child_boxes().into_iter().rev());
    }
    out
}

fn fold(c: char) -> char {
    let mut lower = c.to_lowercase();
    match (lower.next(), lower.next()) {
        (Some(l), None) => l,
        _ => c,
    }
}

/// Where a searchable character came from.
#[derive(Clone, Copy)]
struct Source {
    line: usize,
    fragment: usize,
    /// Byte offset in the fragment's text; `None` for an inserted line-wrap space.
    byte: Option<usize>,
}

fn search(
    inline: &InlineContent,
    fonts: &mut FontContext,
    needle: &[char],
    out: &mut Vec<FindMatch>,
) {
    let mut chars = Vec::new();
    let mut sources = Vec::new();
    for (li, line) in inline.lines.iter().enumerate() {
        if li > 0 && chars.last().is_some_and(|&c| c != ' ') {
            chars.push(' ');
            sources.push(Source {
                line: li,
                fragment: 0,
                byte: None,
            });
        }
        for (fi, f) in line.fragments.iter().enumerate() {
            for (byte, c) in f.text.char_indices() {
                chars.push(fold(c));
                sources.push(Source {
                    line: li,
                    fragment: fi,
                    byte: Some(byte),
                });
            }
        }
    }
    let mut i = 0;
    while i + needle.len() <= chars.len() {
        if chars[i..i + needle.len()] != *needle {
            i += 1;
            continue;
        }
        let rects = match_rects(inline, fonts, &sources[i..i + needle.len()]);
        if !rects.is_empty() {
            out.push(rects);
        }
        i += needle.len();
    }
}

/// Highlight rectangles for the characters in `span` (one per fragment).
fn match_rects(inline: &InlineContent, fonts: &mut FontContext, span: &[Source]) -> Vec<RectF> {
    let mut rects = Vec::new();
    let mut k = 0;
    while k < span.len() {
        let Source {
            line,
            fragment,
            byte: Some(start),
        } = span[k]
        else {
            k += 1;
            continue;
        };
        let mut end_k = k;
        while end_k + 1 < span.len()
            && span[end_k + 1].byte.is_some()
            && (span[end_k + 1].line, span[end_k + 1].fragment) == (line, fragment)
        {
            end_k += 1;
        }
        let f = &inline.lines[line].fragments[fragment];
        let last = span[end_k].byte.unwrap_or(start);
        let end = last + f.text[last..].chars().next().map_or(0, char::len_utf8);
        if let InlineItem::Text { style, .. } = &inline.items[f.item] {
            let sf = style_font(style, fonts);
            if let Some(font) = sf.font {
                let measure = |fonts: &mut FontContext, s: &str| {
                    fonts.measure(font, sf.weight, sf.italic, sf.size, s)
                };
                // Measured widths drift slightly from the per-word shaping
                // used for layout; clamp so highlights stay on the fragment.
                let x0 = measure(fonts, &f.text[..start]).min(f.rect.width);
                let x1 = measure(fonts, &f.text[..end]).clamp(x0, f.rect.width);
                rects.push(RectF::new(f.rect.x + x0, f.rect.y, x1 - x0, f.rect.height));
            }
        }
        k = end_k + 1;
    }
    rects
}

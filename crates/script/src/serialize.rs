//! `innerHTML`: HTML fragment serialization (HTML §13.3, simplified) and
//! fragment parsing by importing nodes from a separately parsed document.

use rustlite_dom::{Attribute, Document, NodeData, NodeId};

const VOID: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "track",
    "wbr",
];
const RAW_TEXT: &[&str] = &[
    "style",
    "script",
    "xmp",
    "iframe",
    "noembed",
    "noframes",
    "plaintext",
];

fn escape(text: &str, attribute: bool, out: &mut String) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '\u{a0}' => out.push_str("&nbsp;"),
            '"' if attribute => out.push_str("&quot;"),
            '<' if !attribute => out.push_str("&lt;"),
            '>' if !attribute => out.push_str("&gt;"),
            c => out.push(c),
        }
    }
}

/// The HTML serialization of `node`'s children (the `innerHTML` getter).
pub fn serialize_children(doc: &Document, node: NodeId) -> String {
    let mut out = String::new();
    // Explicit stack of (node, closing?) so deep trees cannot overflow.
    let mut stack: Vec<(NodeId, bool)> = doc.children(node).map(|c| (c, false)).collect();
    stack.reverse();
    while let Some((n, closing)) = stack.pop() {
        let parent_raw = doc
            .parent(n)
            .and_then(|p| doc.tag_name(p))
            .is_some_and(|t| RAW_TEXT.contains(&t));
        match doc.data(n) {
            NodeData::Element(e) if closing => {
                out.push_str("</");
                out.push_str(&e.name);
                out.push('>');
            }
            NodeData::Element(e) => {
                out.push('<');
                out.push_str(&e.name);
                for a in &e.attrs {
                    out.push(' ');
                    out.push_str(&a.name);
                    out.push_str("=\"");
                    escape(&a.value, true, &mut out);
                    out.push('"');
                }
                out.push('>');
                if VOID.contains(&&*e.name) {
                    continue;
                }
                stack.push((n, true));
                let mut kids: Vec<_> = doc.children(n).map(|c| (c, false)).collect();
                kids.reverse();
                stack.extend(kids);
            }
            NodeData::Text(t) if parent_raw => out.push_str(t),
            NodeData::Text(t) => escape(t, false, &mut out),
            NodeData::Comment(t) => {
                out.push_str("<!--");
                out.push_str(t);
                out.push_str("-->");
            }
            NodeData::Document | NodeData::Doctype { .. } => {}
        }
    }
    out
}

/// Parses `html` as body content and copies the resulting nodes into `doc`
/// as children of `parent`, replacing its current children (the
/// `innerHTML` setter). Context-sensitive cases (e.g. `<tr>` outside a
/// table) parse as they would in `<body>`.
pub fn set_inner_html(doc: &mut Document, parent: NodeId, html: &str) {
    while let Some(child) = doc.first_child(parent) {
        doc.detach(child);
    }
    let parsed = rustlite_html::parse_document(&format!("<!DOCTYPE html><body>{html}")).document;
    let Some(body) = parsed.find_element(parsed.root(), "body") else {
        return;
    };
    // (source node, destination parent)
    let mut stack: Vec<(NodeId, NodeId)> = parsed.children(body).map(|c| (c, parent)).collect();
    stack.reverse();
    while let Some((src, dest_parent)) = stack.pop() {
        let copy = match parsed.data(src) {
            NodeData::Element(e) => {
                let attrs = e
                    .attrs
                    .iter()
                    .map(|a| Attribute::new(&*a.name, a.value.clone()))
                    .collect();
                doc.create_element(e.name.clone(), attrs)
            }
            NodeData::Text(t) => doc.create_text(t.clone()),
            NodeData::Comment(t) => doc.create_comment(t.clone()),
            NodeData::Document | NodeData::Doctype { .. } => continue,
        };
        if doc.append_child(dest_parent, copy).is_err() {
            continue;
        }
        let mut kids: Vec<_> = parsed.children(src).map(|c| (c, copy)).collect();
        kids.reverse();
        stack.extend(kids);
    }
}

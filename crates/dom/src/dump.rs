//! Deterministic tree dump in the html5lib-tests "#document" format.
//!
//! ```text
//! | <!DOCTYPE html>
//! | <html>
//! |   <head>
//! |   <body>
//! |     <p>
//! |       id="x"
//! |       "text"
//! ```
//!
//! Attributes are sorted by name. Using this format lets golden files be
//! compared directly against html5lib-tests expectations and against
//! reference-browser output serialized the same way.

use std::fmt::Write;

use crate::{Document, NodeData, NodeId};

/// Dumps the children of `root` (normally the document node).
pub fn dump_tree(doc: &Document, root: NodeId) -> String {
    let mut out = String::new();
    // Explicit stack instead of recursion: hostile input can nest arbitrarily deep.
    let mut stack: Vec<(NodeId, usize)> = doc.children(root).map(|c| (c, 0)).collect();
    stack.reverse();
    while let Some((id, depth)) = stack.pop() {
        let indent = "  ".repeat(depth);
        match doc.data(id) {
            NodeData::Document => {}
            NodeData::Doctype {
                name,
                public_id,
                system_id,
            } => {
                if public_id.is_empty() && system_id.is_empty() {
                    let _ = writeln!(out, "| {indent}<!DOCTYPE {name}>");
                } else {
                    let _ = writeln!(
                        out,
                        "| {indent}<!DOCTYPE {name} \"{public_id}\" \"{system_id}\">"
                    );
                }
            }
            NodeData::Element(e) => {
                let _ = writeln!(out, "| {indent}<{}>", e.name);
                let mut attrs: Vec<_> = e.attrs.iter().collect();
                attrs.sort_by(|a, b| a.name.cmp(&b.name));
                for a in attrs {
                    let _ = writeln!(out, "| {indent}  {}=\"{}\"", a.name, a.value);
                }
            }
            NodeData::Text(t) => {
                let _ = writeln!(out, "| {indent}\"{t}\"");
            }
            NodeData::Comment(c) => {
                let _ = writeln!(out, "| {indent}<!-- {c} -->");
            }
        }
        let children: Vec<_> = doc.children(id).collect();
        stack.extend(children.into_iter().rev().map(|c| (c, depth + 1)));
    }
    out
}

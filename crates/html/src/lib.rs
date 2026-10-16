//! HTML parsing for RustLite: tokenizer and tree builder following the WHATWG
//! HTML standard (§13.2), producing a [`rustlite_dom::Document`].
//!
//! ```
//! let out = rustlite_html::parse_document("<p>Hello <b>world");
//! let doc = &out.document;
//! let p = doc.find_element(doc.root(), "p").unwrap();
//! assert_eq!(doc.text_content(p), "Hello world");
//! ```
//!
//! Parsing never fails. Malformed markup is repaired as browsers do; the
//! repairs are reported as [`ParseError`]s.

mod entities;
pub mod tokenizer;
mod tree_builder;

pub use tokenizer::ParseError;
pub use tree_builder::{ParseOutput, parse_document};

//! CSS syntax for RustLite: tokenizer (CSS Syntax Level 3 §4), rule and
//! declaration parsing (§5), and selector parsing (Selectors Level 4 subset).
//!
//! This crate knows nothing about property semantics or the DOM. Property
//! values stay as component values; `rustlite-style` interprets them.
//!
//! ```
//! let sheet = rustlite_css::parse_stylesheet("p.note > b { color: red !important }");
//! let rule = &sheet.rules[0];
//! assert_eq!(rule.selectors.0[0].specificity(), (0, 1, 2).into());
//! assert_eq!(rule.declarations[0].name, "color");
//! assert!(rule.declarations[0].important);
//! ```

pub mod parser;
pub mod selectors;
pub mod tokenizer;

pub use parser::{
    ComponentValue, Declaration, StyleRule, Stylesheet, parse_declaration_list, parse_stylesheet,
};
pub use selectors::{
    AttrOperator, AttributeSelector, Combinator, Complex, Compound, PseudoClass, SelectorList,
    SimpleSelector, Specificity,
};
pub use tokenizer::{Token, tokenize};

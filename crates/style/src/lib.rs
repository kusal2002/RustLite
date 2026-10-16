//! Styling for RustLite: selector matching, the cascade, inheritance, and
//! computed values for the supported properties.
//!
//! ```
//! let out = rustlite_html::parse_document(
//!     "<style>p { color: green } .big { font-size: 2em }</style><p class=big>x",
//! );
//! let doc = &out.document;
//! let stylist = rustlite_style::stylist_for(doc);
//! let (styles, _) = rustlite_style::compute_styles(doc, &stylist, (800.0, 600.0));
//! let p = doc.find_element(doc.root(), "p").unwrap();
//! let style = styles.get(p).unwrap();
//! assert_eq!(rustlite_style::resolved_value(style, "color").unwrap(), "rgb(0, 128, 0)");
//! assert_eq!(style.font_size, 32.0);
//! ```
//!
//! Supported properties, value syntaxes, and known gaps are documented in
//! docs/compatibility.md.

mod colors;
pub mod computed;
mod matching;
pub mod properties;
pub mod serialize;
mod stylist;
pub mod values;

pub use computed::{ComputedLength, ComputedLengthAuto, ComputedLineHeight, ComputedStyle};
pub use serialize::{DEFAULT_DUMP_PROPERTIES, dump_styles, resolved_value};
pub use stylist::{
    ComputedStyles, Origin, SheetStats, StyleStats, Stylist, compute_styles, is_stylesheet_link,
    stylist_for, stylist_with_external,
};
pub use values::Rgba;

use rustlite_css::SelectorList;
use rustlite_dom::{Document, NodeData, NodeId};

/// Parses a selector list as `querySelector`/`matches` accept it; `None`
/// for invalid or unsupported selectors (scripts get a `SyntaxError`).
pub fn parse_selectors(text: &str) -> Option<SelectorList> {
    if text.trim().is_empty() {
        return None;
    }
    // Reuse the stylesheet parser: exactly one rule must come out, so input
    // like `a {} b` cannot smuggle in a second rule.
    let sheet = rustlite_css::parse_stylesheet(&format!("{text}{{}}"));
    if sheet.rules.len() != 1 || sheet.dropped_rules != 0 || sheet.skipped_at_rules != 0 {
        return None;
    }
    sheet.rules.into_iter().next().map(|r| r.selectors)
}

/// Does element `el` match any selector in `list`?
pub fn element_matches(doc: &Document, list: &SelectorList, el: NodeId) -> bool {
    if !matches!(doc.data(el), NodeData::Element(_)) {
        return false;
    }
    let ctx = matching::MatchContext {
        doc,
        quirks: doc.quirks_mode() == rustlite_dom::QuirksMode::Quirks,
    };
    list.0.iter().any(|sel| matching::matches(ctx, sel, el))
}

/// Elements under `root` (exclusive) matching `list`, in tree order.
pub fn query_selector_all(doc: &Document, root: NodeId, list: &SelectorList) -> Vec<NodeId> {
    doc.descendants(root)
        .skip(1)
        .filter(|&n| element_matches(doc, list, n))
        .collect()
}

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
    ComputedStyles, Origin, SheetStats, StyleStats, Stylist, compute_styles, stylist_for,
};
pub use values::Rgba;

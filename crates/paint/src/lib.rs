//! Display-list primitives.
//!
//! The paint stage turns layout geometry into a flat, ordered list of drawing
//! commands. The renderer consumes that list without knowing anything about the
//! DOM, styles, or layout. Coordinates are in device pixels with the origin at
//! the top-left corner of the target surface.

mod color;
mod display_list;
mod geometry;

pub use color::Color;
pub use display_list::{DisplayItem, DisplayList};
pub use geometry::{Point, Rect, Size};

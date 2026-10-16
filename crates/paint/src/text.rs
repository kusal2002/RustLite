//! Text primitives shared by layout (producer) and the renderer (consumer).
//!
//! The display list refers to fonts only through [`FontId`]; turning glyph IDs
//! into pixels is delegated to a [`GlyphRasterizer`], implemented by
//! `rustlite-fonts`. This keeps the renderer independent of font libraries.

use std::rc::Rc;

/// Opaque handle to a loaded font face, assigned by the font context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FontId(pub u32);

/// A glyph positioned relative to the run's baseline origin, in px.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PositionedGlyph {
    pub id: u16,
    pub x: f32,
    pub y: f32,
}

/// A run of glyphs from one face at one size, drawn in one color.
#[derive(Debug, Clone, PartialEq)]
pub struct GlyphRun {
    pub font: FontId,
    /// Font size in px.
    pub size: f32,
    pub glyphs: Rc<[PositionedGlyph]>,
}

/// An 8-bit coverage bitmap for one glyph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlyphBitmap {
    pub width: u32,
    pub height: u32,
    /// Offset from the pen position (on the baseline) to the bitmap's
    /// top-left corner, in px, y pointing down.
    pub left: i32,
    pub top: i32,
    /// Row-major coverage, `width * height` bytes.
    pub coverage: Vec<u8>,
}

/// Produces glyph bitmaps on demand. Implementations are expected to cache.
pub trait GlyphRasterizer {
    fn rasterize(&mut self, font: FontId, glyph: u16, size: f32) -> Option<Rc<GlyphBitmap>>;
}

/// A rasterizer that draws nothing, for contexts without fonts (tests, headless chrome).
#[derive(Debug, Default, Clone, Copy)]
pub struct NoGlyphs;

impl GlyphRasterizer for NoGlyphs {
    fn rasterize(&mut self, _: FontId, _: u16, _: f32) -> Option<Rc<GlyphBitmap>> {
        None
    }
}

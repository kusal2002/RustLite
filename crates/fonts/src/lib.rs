//! Fonts for RustLite: system font discovery, CSS-style face selection,
//! per-character fallback, shaping, metrics, and glyph rasterization.
//!
//! See `research/decisions/ADR-0002-font-stack.md` for the library choices.
//! Everything here is infrastructure; line breaking and text layout live in
//! `rustlite-layout`.

use std::collections::HashMap;
use std::rc::Rc;

mod raster;

use rustlite_paint::{FontId, GlyphBitmap, GlyphRasterizer};

/// One installed face, described without its data, so another process can
/// select fonts without scanning or opening font files (see
/// [`FontContext::brokered`]).
#[derive(Debug, Clone, PartialEq)]
pub struct FaceDescriptor {
    pub path: std::path::PathBuf,
    /// Face index within a collection file.
    pub index: u32,
    pub families: Vec<String>,
    pub post_script_name: String,
    pub weight: u16,
    /// 0 normal, 1 italic, 2 oblique.
    pub style: u8,
    /// CSS `font-stretch` keyword number, 1 (ultra-condensed) to 9.
    pub stretch: u16,
    pub monospaced: bool,
}

/// Supplies font data to a context that may not open font files (a
/// sandboxed renderer). Faces are named by their position in the list
/// given to [`FontContext::brokered`].
pub trait FontBroker {
    /// The font file (or collection) bytes of face `face`.
    fn face_data(&mut self, face: u32) -> Option<Rc<[u8]>>;
    /// A face covering `c` (see [`FontContext::fallback_face`]).
    fn fallback(&mut self, c: char, weight: u16, italic: bool) -> Option<u32>;
}

struct Broker(Box<dyn FontBroker>);

impl std::fmt::Debug for Broker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FontBroker")
    }
}

/// A family in a CSS `font-family` list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family<'a> {
    Named(&'a str),
    Serif,
    SansSerif,
    Monospace,
    Cursive,
    Fantasy,
}

impl<'a> Family<'a> {
    /// Maps a CSS generic family keyword; anything else is a named family.
    pub fn from_css_generic(generic: &'a str) -> Self {
        match generic {
            "serif" | "ui-serif" | "fangsong" | "math" => Family::Serif,
            "sans-serif" | "system-ui" | "ui-sans-serif" | "ui-rounded" | "emoji" => {
                Family::SansSerif
            }
            "monospace" | "ui-monospace" => Family::Monospace,
            "cursive" => Family::Cursive,
            "fantasy" => Family::Fantasy,
            other => Family::Named(other),
        }
    }
}

/// Vertical metrics scaled to a font size, in px. `descent` is positive.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScaledMetrics {
    pub ascent: f32,
    pub descent: f32,
    pub line_gap: f32,
}

/// One shaped glyph, in px.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShapedGlyph {
    pub id: u16,
    pub x_advance: f32,
    pub x_offset: f32,
    pub y_offset: f32,
}

/// Glyphs from a single face for a contiguous piece of text.
#[derive(Debug, Clone, PartialEq)]
pub struct ShapedSegment {
    pub font: FontId,
    pub glyphs: Vec<ShapedGlyph>,
    pub advance: f32,
}

type ShaperFace<'a> = rustybuzz::Face<'a>;

self_cell::self_cell!(
    /// Font bytes plus the rustybuzz face parsed from them. Parsing the
    /// shaping tables costs ~80 µs, ~25x shaping a word, so it is done once
    /// per face instead of once per shaped string.
    struct Shaper {
        owner: Rc<[u8]>,
        #[covariant]
        dependent: ShaperFace,
    }
);

impl std::fmt::Debug for Shaper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shaper")
            .field("bytes", &self.borrow_owner().len())
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
struct Face {
    data: Rc<[u8]>,
    index: u32,
    /// `None` if rustybuzz cannot parse the face (it then shapes to nothing).
    shaper: Option<Shaper>,
    units_per_em: f32,
    /// Width of the font's bounding box (`head` xMax - xMin), font units.
    bbox_width: f32,
    ascent: f32,
    descent: f32,
    line_gap: f32,
}

/// Candidate families for each generic, in preference order. The first one
/// installed becomes the generic's mapping (Windows, macOS, then Linux names).
const SERIF: &[&str] = &[
    "Times New Roman",
    "Times",
    "Liberation Serif",
    "DejaVu Serif",
    "Noto Serif",
];
const SANS: &[&str] = &[
    "Arial",
    "Helvetica",
    "Liberation Sans",
    "DejaVu Sans",
    "Noto Sans",
];
const MONO: &[&str] = &[
    "Courier New",
    "Courier",
    "Liberation Mono",
    "DejaVu Sans Mono",
    "Noto Sans Mono",
];
const CURSIVE: &[&str] = &[
    "Comic Sans MS",
    "Apple Chancery",
    "URW Chancery L",
    "DejaVu Sans",
];
const FANTASY: &[&str] = &["Impact", "Papyrus", "DejaVu Sans"];

/// Families tried first for characters the selected font lacks.
const FALLBACK_FAMILIES: &[&str] = &[
    "Segoe UI",
    "Nirmala UI",
    "Iskoola Pota",
    "Segoe UI Symbol",
    "Segoe UI Emoji",
    "Noto Sans",
    "Noto Sans Sinhala",
    "Noto Color Emoji",
    "DejaVu Sans",
    "Arial Unicode MS",
];

/// Glyph bitmaps kept before the cache is flushed (bounded memory).
const GLYPH_CACHE_LIMIT: usize = 4096;

/// Owns the font database, loaded faces, and caches. Not thread-safe; one per
/// rendering thread.
#[derive(Debug)]
pub struct FontContext {
    db: fontdb::Database,
    faces: Vec<Face>,
    by_db_id: HashMap<fontdb::ID, FontId>,
    query_cache: HashMap<(String, u16, bool), Option<FontId>>,
    fallback_cache: HashMap<(char, u16, bool), Option<FontId>>,
    glyph_cache: HashMap<(FontId, u16, u32), Option<Rc<GlyphBitmap>>>,
    /// Shaped short strings (words), per (primary font, weight, italic, size bits).
    shape_cache: HashMap<ShapeKey, HashMap<Box<str>, Rc<[ShapedSegment]>>>,
    shape_cache_len: usize,
    /// Set for brokered contexts: font data comes from here, never from
    /// files.
    broker: Option<Broker>,
}

/// Shaping cache key: primary font, weight, italic, size (`f32` bits).
type ShapeKey = (FontId, u16, bool, u32);

/// Strings up to this many bytes are cached by [`FontContext::shape`]; longer
/// ones (UI text, measured prefixes) rarely repeat.
const SHAPE_CACHE_MAX_BYTES: usize = 48;
/// Entry cap for the shaping cache (~a few MB at most); cleared when full.
const SHAPE_CACHE_LIMIT: usize = 16_384;

impl Default for FontContext {
    fn default() -> Self {
        Self::new()
    }
}

impl FontContext {
    /// A context with no fonts (add some with [`FontContext::load_font_data`]).
    pub fn new() -> Self {
        Self {
            db: fontdb::Database::new(),
            faces: Vec::new(),
            by_db_id: HashMap::new(),
            query_cache: HashMap::new(),
            fallback_cache: HashMap::new(),
            glyph_cache: HashMap::new(),
            shape_cache: HashMap::new(),
            shape_cache_len: 0,
            broker: None,
        }
    }

    /// The installed faces, in a stable order, for [`FontContext::brokered`]
    /// in another process. Faces loaded from memory are left out.
    pub fn describe_faces(&self) -> Vec<FaceDescriptor> {
        self.db
            .faces()
            .filter_map(|f| {
                let path = match &f.source {
                    fontdb::Source::File(p) | fontdb::Source::SharedFile(p, _) => p.clone(),
                    fontdb::Source::Binary(_) => return None,
                };
                Some(FaceDescriptor {
                    path,
                    index: f.index,
                    families: f.families.iter().map(|(n, _)| n.clone()).collect(),
                    post_script_name: f.post_script_name.clone(),
                    weight: f.weight.0,
                    style: match f.style {
                        fontdb::Style::Normal => 0,
                        fontdb::Style::Italic => 1,
                        fontdb::Style::Oblique => 2,
                    },
                    stretch: f.stretch.to_number(),
                    monospaced: f.monospaced,
                })
            })
            .collect()
    }

    /// A context that selects among `faces` (from [`FontContext::describe_faces`]
    /// in another process) and gets their data from `broker`. It never opens
    /// font files.
    pub fn brokered(faces: Vec<FaceDescriptor>, broker: Box<dyn FontBroker>) -> Self {
        let mut ctx = Self::new();
        for f in faces {
            let stretch = match f.stretch {
                1 => fontdb::Stretch::UltraCondensed,
                2 => fontdb::Stretch::ExtraCondensed,
                3 => fontdb::Stretch::Condensed,
                4 => fontdb::Stretch::SemiCondensed,
                6 => fontdb::Stretch::SemiExpanded,
                7 => fontdb::Stretch::Expanded,
                8 => fontdb::Stretch::ExtraExpanded,
                9 => fontdb::Stretch::UltraExpanded,
                _ => fontdb::Stretch::Normal,
            };
            ctx.db.push_face_info(fontdb::FaceInfo {
                id: fontdb::ID::dummy(),
                source: fontdb::Source::File(f.path),
                index: f.index,
                families: f
                    .families
                    .into_iter()
                    .map(|n| (n, fontdb::Language::English_UnitedStates))
                    .collect(),
                post_script_name: f.post_script_name,
                style: match f.style {
                    1 => fontdb::Style::Italic,
                    2 => fontdb::Style::Oblique,
                    _ => fontdb::Style::Normal,
                },
                weight: fontdb::Weight(f.weight),
                stretch,
                monospaced: f.monospaced,
            });
        }
        ctx.configure_generics();
        ctx.broker = Some(Broker(broker));
        ctx
    }

    /// Position of a database face in [`FontContext::describe_faces`] order.
    fn face_position(&self, id: fontdb::ID) -> Option<u32> {
        let pos = self
            .db
            .faces()
            .filter(|f| !matches!(f.source, fontdb::Source::Binary(_)))
            .position(|f| f.id == id)?;
        u32::try_from(pos).ok()
    }

    fn face_at(&self, position: u32) -> Option<fontdb::ID> {
        self.db
            .faces()
            .filter(|f| !matches!(f.source, fontdb::Source::Binary(_)))
            .nth(position as usize)
            .map(|f| f.id)
    }

    /// The data of face `position` (in [`FontContext::describe_faces`]
    /// order), for a brokered context in another process.
    pub fn face_data(&mut self, position: u32) -> Option<Rc<[u8]>> {
        let id = self.face_at(position)?;
        let fid = self.load(id)?;
        Some(self.faces[fid.0 as usize].data.clone())
    }

    /// The position (in [`FontContext::describe_faces`] order) of a face
    /// covering `c`, for a brokered context in another process.
    pub fn fallback_face(&mut self, c: char, weight: u16, italic: bool) -> Option<u32> {
        let fid = self.fallback_for(c, weight, italic)?;
        let (&id, _) = self.by_db_id.iter().find(|(_, f)| **f == fid)?;
        self.face_position(id)
    }

    /// Empties the word shaping cache (benchmarks measure cold layout with it).
    pub fn clear_shape_cache(&mut self) {
        self.shape_cache.clear();
        self.shape_cache_len = 0;
    }

    /// Entries in the word shaping cache (for memory reporting).
    pub fn shape_cache_len(&self) -> usize {
        self.shape_cache_len
    }

    /// Approximate heap bytes of the word shaping cache.
    pub fn shape_cache_bytes(&self) -> usize {
        self.shape_cache
            .values()
            .flat_map(|m| m.iter())
            .map(|(word, segments)| {
                word.len()
                    + std::mem::size_of::<Rc<[ShapedSegment]>>()
                    + segments
                        .iter()
                        .map(|s| {
                            std::mem::size_of::<ShapedSegment>()
                                + s.glyphs.capacity() * std::mem::size_of::<ShapedGlyph>()
                        })
                        .sum::<usize>()
            })
            .sum()
    }

    /// Entries in the glyph bitmap cache (for memory reporting).
    pub fn glyph_cache_len(&self) -> usize {
        self.glyph_cache.len()
    }

    /// Approximate heap bytes of cached glyph bitmaps.
    pub fn glyph_cache_bytes(&self) -> usize {
        self.glyph_cache
            .values()
            .map(|g| {
                std::mem::size_of::<(FontId, u16, u32)>()
                    + g.as_ref().map_or(0, |b| {
                        std::mem::size_of::<GlyphBitmap>() + b.coverage.capacity()
                    })
            })
            .sum()
    }

    /// Faces loaded for shaping and rasterizing, and their font file bytes.
    /// Font data from the OS may be memory-mapped and shared, so the bytes
    /// are an upper bound on what this process adds.
    pub fn loaded_faces(&self) -> (usize, usize) {
        (
            self.faces.len(),
            self.faces.iter().map(|f| f.data.len()).sum(),
        )
    }

    /// A context with the operating system's installed fonts.
    pub fn with_system_fonts() -> Self {
        let start = std::time::Instant::now();
        let mut ctx = Self::new();
        ctx.db.load_system_fonts();
        ctx.configure_generics();
        tracing::info!(
            faces = ctx.db.len(),
            load_ms = start.elapsed().as_secs_f64() * 1000.0,
            "system fonts loaded"
        );
        ctx
    }

    /// Adds fonts from memory (a font file or collection).
    pub fn load_font_data(&mut self, data: Vec<u8>) {
        self.db.load_font_data(data);
        self.configure_generics();
        self.query_cache.clear();
        self.fallback_cache.clear();
    }

    pub fn face_count(&self) -> usize {
        self.db.len()
    }

    pub fn has_family(&self, name: &str) -> bool {
        self.db
            .faces()
            .any(|f| f.families.iter().any(|(n, _)| n.eq_ignore_ascii_case(name)))
    }

    fn configure_generics(&mut self) {
        fn pick(db: &fontdb::Database, candidates: &[&'static str]) -> Option<&'static str> {
            candidates.iter().copied().find(|c| {
                db.faces()
                    .any(|f| f.families.iter().any(|(n, _)| n.eq_ignore_ascii_case(c)))
            })
        }
        if let Some(f) = pick(&self.db, SERIF) {
            self.db.set_serif_family(f);
        }
        if let Some(f) = pick(&self.db, SANS) {
            self.db.set_sans_serif_family(f);
        }
        if let Some(f) = pick(&self.db, MONO) {
            self.db.set_monospace_family(f);
        }
        if let Some(f) = pick(&self.db, CURSIVE) {
            self.db.set_cursive_family(f);
        }
        if let Some(f) = pick(&self.db, FANTASY) {
            self.db.set_fantasy_family(f);
        }
    }

    /// Position of loaded face `font` in [`FontContext::describe_faces`]
    /// order, for naming it to another process. `None` for faces loaded
    /// from memory.
    pub fn position_of(&self, font: FontId) -> Option<u32> {
        let (&id, _) = self.by_db_id.iter().find(|(_, f)| **f == font)?;
        self.face_position(id)
    }

    /// Loads the face at `position` in [`FontContext::describe_faces`]
    /// order (one this process already knows; nothing is opened by name).
    pub fn font_at_position(&mut self, position: u32) -> Option<FontId> {
        let id = self.face_at(position)?;
        self.load(id)
    }

    fn load(&mut self, id: fontdb::ID) -> Option<FontId> {
        if let Some(&fid) = self.by_db_id.get(&id) {
            return Some(fid);
        }
        let (data, index) = if self.broker.is_some() {
            let index = self.db.face(id)?.index;
            let position = self.face_position(id)?;
            (self.broker.as_mut()?.0.face_data(position)?, index)
        } else {
            self.db
                .with_face_data(id, |data, index| (Rc::<[u8]>::from(data), index))?
        };
        let parsed = ttf_parser::Face::parse(&data, index).ok()?;
        let face = Face {
            units_per_em: f32::from(parsed.units_per_em()),
            bbox_width: {
                let b = parsed.global_bounding_box();
                f32::from(b.x_max) - f32::from(b.x_min)
            },
            ascent: f32::from(parsed.ascender()),
            descent: -f32::from(parsed.descender()),
            line_gap: f32::from(parsed.line_gap()),
            shaper: Shaper::try_new(data.clone(), |d| {
                rustybuzz::Face::from_slice(d, index).ok_or(())
            })
            .ok(),
            data: data.clone(),
            index,
        };
        let fid = FontId(u32::try_from(self.faces.len()).ok()?);
        self.faces.push(face);
        self.by_db_id.insert(id, fid);
        Some(fid)
    }

    /// Selects a face for a CSS family list, weight and style, falling back to
    /// the serif generic and then to any installed face.
    pub fn select(&mut self, families: &[Family<'_>], weight: u16, italic: bool) -> Option<FontId> {
        let key = format!("{families:?}");
        if let Some(&cached) = self.query_cache.get(&(key.clone(), weight, italic)) {
            return cached;
        }
        let mut list: Vec<fontdb::Family<'_>> = families
            .iter()
            .map(|f| match *f {
                Family::Named(n) => fontdb::Family::Name(n),
                Family::Serif => fontdb::Family::Serif,
                Family::SansSerif => fontdb::Family::SansSerif,
                Family::Monospace => fontdb::Family::Monospace,
                Family::Cursive => fontdb::Family::Cursive,
                Family::Fantasy => fontdb::Family::Fantasy,
            })
            .collect();
        list.push(fontdb::Family::Serif);
        let query = fontdb::Query {
            families: &list,
            weight: fontdb::Weight(weight),
            stretch: fontdb::Stretch::Normal,
            style: if italic {
                fontdb::Style::Italic
            } else {
                fontdb::Style::Normal
            },
        };
        let id = self
            .db
            .query(&query)
            .or_else(|| self.db.faces().next().map(|f| f.id));
        let result = id.and_then(|id| self.load(id));
        self.query_cache.insert((key, weight, italic), result);
        result
    }

    /// Vertical metrics of `font` at `size` px.
    pub fn metrics(&self, font: FontId, size: f32) -> ScaledMetrics {
        let f = &self.faces[font.0 as usize];
        let scale = size / f.units_per_em;
        ScaledMetrics {
            ascent: f.ascent * scale,
            descent: f.descent * scale,
            line_gap: f.line_gap * scale,
        }
    }

    /// The widest any glyph can be at `size`: the font bounding box width
    /// (what browsers call the max char width when sizing text fields).
    pub fn max_char_width(&self, font: FontId, size: f32) -> f32 {
        let f = &self.faces[font.0 as usize];
        f.bbox_width * size / f.units_per_em
    }

    fn covers(face: &ttf_parser::Face<'_>, c: char) -> bool {
        // Controls and default-ignorables never need a fallback face.
        c.is_control() || matches!(c, '\u{200B}'..='\u{200F}') || face.glyph_index(c).is_some()
    }

    /// A face covering `c`, preferring well-known fallback families.
    fn fallback_for(&mut self, c: char, weight: u16, italic: bool) -> Option<FontId> {
        if let Some(&cached) = self.fallback_cache.get(&(c, weight, italic)) {
            return cached;
        }
        if let Some(broker) = &mut self.broker {
            let result = broker
                .0
                .fallback(c, weight, italic)
                .and_then(|position| self.face_at(position))
                .and_then(|id| self.load(id));
            self.fallback_cache.insert((c, weight, italic), result);
            return result;
        }
        let preferred: Vec<fontdb::ID> = FALLBACK_FAMILIES
            .iter()
            .filter_map(|name| {
                self.db.query(&fontdb::Query {
                    families: &[fontdb::Family::Name(name)],
                    weight: fontdb::Weight(weight),
                    stretch: fontdb::Stretch::Normal,
                    style: if italic {
                        fontdb::Style::Italic
                    } else {
                        fontdb::Style::Normal
                    },
                })
            })
            .collect();
        let all = self.db.faces().map(|f| f.id);
        let candidates: Vec<fontdb::ID> = preferred.into_iter().chain(all).collect();
        let found = candidates.into_iter().find(|&id| {
            self.db
                .with_face_data(id, |data, index| {
                    ttf_parser::Face::parse(data, index).is_ok_and(|f| f.glyph_index(c).is_some())
                })
                .unwrap_or(false)
        });
        let result = found.and_then(|id| self.load(id));
        self.fallback_cache.insert((c, weight, italic), result);
        result
    }

    /// Shapes `text` with `primary`, switching to fallback faces for
    /// characters `primary` lacks. Returns one segment per face change.
    /// Shapes `text` starting from `primary`, splitting it into segments by
    /// the face that covers each character. Short strings are cached.
    pub fn shape(
        &mut self,
        primary: FontId,
        weight: u16,
        italic: bool,
        size: f32,
        text: &str,
    ) -> Rc<[ShapedSegment]> {
        if text.len() > SHAPE_CACHE_MAX_BYTES {
            return self
                .shape_uncached(primary, weight, italic, size, text)
                .into();
        }
        let key = (primary, weight, italic, size.to_bits());
        if let Some(hit) = self.shape_cache.get(&key).and_then(|m| m.get(text)) {
            return hit.clone();
        }
        let shaped: Rc<[ShapedSegment]> = self
            .shape_uncached(primary, weight, italic, size, text)
            .into();
        if self.shape_cache_len >= SHAPE_CACHE_LIMIT {
            self.shape_cache.clear();
            self.shape_cache_len = 0;
        }
        self.shape_cache
            .entry(key)
            .or_default()
            .insert(text.into(), shaped.clone());
        self.shape_cache_len += 1;
        shaped
    }

    fn shape_uncached(
        &mut self,
        primary: FontId,
        weight: u16,
        italic: bool,
        size: f32,
        text: &str,
    ) -> Vec<ShapedSegment> {
        // Assign a face to every char, keeping combining marks and joiners
        // with the preceding base character.
        let mut runs: Vec<(usize, usize, FontId)> = Vec::new();
        {
            for (i, c) in text.char_indices() {
                let continues_cluster = runs.last().is_some() && is_cluster_continuation(c);
                let font = if continues_cluster {
                    runs.last().map(|r| r.2).unwrap_or(primary)
                } else if self.faces[primary.0 as usize]
                    .shaper
                    .as_ref()
                    .is_none_or(|s| Self::covers(s.borrow_dependent(), c))
                {
                    primary
                } else {
                    self.fallback_for(c, weight, italic).unwrap_or(primary)
                };
                match runs.last_mut() {
                    Some(last) if last.2 == font => last.1 = i + c.len_utf8(),
                    _ => runs.push((i, i + c.len_utf8(), font)),
                }
            }
        }
        runs.into_iter()
            .map(|(start, end, font)| self.shape_single(font, size, &text[start..end]))
            .collect()
    }

    fn shape_single(&self, font: FontId, size: f32, text: &str) -> ShapedSegment {
        let f = &self.faces[font.0 as usize];
        let scale = size / f.units_per_em;
        let mut glyphs = Vec::new();
        let mut advance = 0.0;
        if let Some(face) = f.shaper.as_ref().map(Shaper::borrow_dependent) {
            let mut buffer = rustybuzz::UnicodeBuffer::new();
            buffer.push_str(text);
            buffer.guess_segment_properties();
            let shaped = rustybuzz::shape(face, &[], buffer);
            for (info, pos) in shaped.glyph_infos().iter().zip(shaped.glyph_positions()) {
                let g = ShapedGlyph {
                    // Glyph IDs are 16-bit in OpenType.
                    id: info.glyph_id as u16,
                    x_advance: pos.x_advance as f32 * scale,
                    x_offset: pos.x_offset as f32 * scale,
                    y_offset: pos.y_offset as f32 * scale,
                };
                advance += g.x_advance;
                glyphs.push(g);
            }
        }
        ShapedSegment {
            font,
            glyphs,
            advance,
        }
    }

    /// Width of `text` in px (shaping with fallback).
    pub fn measure(
        &mut self,
        primary: FontId,
        weight: u16,
        italic: bool,
        size: f32,
        text: &str,
    ) -> f32 {
        self.shape(primary, weight, italic, size, text)
            .iter()
            .map(|s| s.advance)
            .sum()
    }
}

/// Characters that must stay in the same face as the preceding character.
fn is_cluster_continuation(c: char) -> bool {
    matches!(c,
        '\u{0300}'..='\u{036F}'   // combining diacritical marks
        | '\u{200C}' | '\u{200D}' // ZWNJ / ZWJ
        | '\u{FE00}'..='\u{FE0F}' // variation selectors
        | '\u{0DCA}'..='\u{0DDF}' // Sinhala vowel signs and al-lakuna
        | '\u{0D82}' | '\u{0D83}' // Sinhala anusvara / visarga
    )
}

impl GlyphRasterizer for FontContext {
    fn rasterize(&mut self, font: FontId, glyph: u16, size: f32) -> Option<Rc<GlyphBitmap>> {
        // Quarter-pixel size buckets bound the cache without visible error.
        let key = (font, glyph, (size * 4.0).round() as u32);
        if let Some(cached) = self.glyph_cache.get(&key) {
            return cached.clone();
        }
        if self.glyph_cache.len() >= GLYPH_CACHE_LIMIT {
            self.glyph_cache.clear();
        }
        let face = self.faces.get(font.0 as usize)?;
        // Parsing only reads the table directory; the glyph itself is decoded
        // on demand, so nothing per-face stays resident besides the file data.
        let bitmap = ttf_parser::Face::parse(&face.data, face.index)
            .ok()
            .and_then(|parsed| raster::rasterize(&parsed, glyph, size))
            .map(Rc::new);
        self.glyph_cache.insert(key, bitmap.clone());
        bitmap
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// System-font tests skip (pass vacuously, with a note) when no fonts exist,
    /// e.g. on minimal CI images.
    fn system() -> Option<FontContext> {
        let ctx = FontContext::with_system_fonts();
        if ctx.face_count() == 0 {
            eprintln!("note: no system fonts installed; skipping");
            return None;
        }
        Some(ctx)
    }

    #[test]
    fn empty_context_selects_nothing() {
        let mut ctx = FontContext::new();
        assert_eq!(ctx.select(&[Family::Serif], 400, false), None);
    }

    #[test]
    fn generic_mapping() {
        assert_eq!(Family::from_css_generic("serif"), Family::Serif);
        assert_eq!(Family::from_css_generic("system-ui"), Family::SansSerif);
        assert_eq!(Family::from_css_generic("Arial"), Family::Named("Arial"));
    }

    #[test]
    fn select_shape_and_measure() {
        let Some(mut ctx) = system() else { return };
        let font = ctx
            .select(&[Family::Serif], 400, false)
            .expect("a serif face");
        let m = ctx.metrics(font, 16.0);
        assert!(m.ascent > 8.0 && m.descent > 1.0, "{m:?}");
        let one = ctx.measure(font, 400, false, 16.0, "a");
        let three = ctx.measure(font, 400, false, 16.0, "aaa");
        assert!(one > 0.0);
        assert!((three - 3.0 * one).abs() < 0.01);
        assert!(ctx.measure(font, 400, false, 32.0, "aaa") > three * 1.9);
        // Repeated selection hits the cache and returns the same face.
        assert_eq!(ctx.select(&[Family::Serif], 400, false), Some(font));
    }

    #[test]
    fn bold_is_wider_or_different_face() {
        let Some(mut ctx) = system() else { return };
        let regular = ctx.select(&[Family::SansSerif], 400, false).unwrap();
        let bold = ctx.select(&[Family::SansSerif], 700, false).unwrap();
        if regular != bold {
            assert!(
                ctx.measure(bold, 700, false, 16.0, "Hello")
                    >= ctx.measure(regular, 400, false, 16.0, "Hello")
            );
        }
    }

    #[test]
    fn glyphs_rasterize_and_cache() {
        let Some(mut ctx) = system() else { return };
        let font = ctx.select(&[Family::SansSerif], 400, false).unwrap();
        let seg = &ctx.shape(font, 400, false, 24.0, "H")[0];
        let bitmap = ctx
            .rasterize(seg.font, seg.glyphs[0].id, 24.0)
            .expect("H has ink");
        assert!(bitmap.width > 0 && bitmap.height > 10);
        assert!(bitmap.top < 0, "glyph sits above the baseline");
        let again = ctx.rasterize(seg.font, seg.glyphs[0].id, 24.0).unwrap();
        assert!(Rc::ptr_eq(&bitmap, &again));
    }

    #[test]
    fn rasterized_o_has_solid_ink_and_an_empty_counter() {
        let Some(mut ctx) = system() else { return };
        let font = ctx.select(&[Family::SansSerif], 400, false).unwrap();
        let seg = &ctx.shape(font, 400, false, 64.0, "O")[0];
        let b = ctx.rasterize(seg.font, seg.glyphs[0].id, 64.0).unwrap();
        let at = |x: u32, y: u32| b.coverage[(y * b.width + x) as usize];
        let (cx, cy) = (b.width / 2, b.height / 2);
        assert_eq!(at(cx, cy), 0, "counter of O must be empty");
        // Scanning left from the center along the middle row hits full ink.
        let max_ink = (0..cx).map(|x| at(x, cy)).max().unwrap();
        assert_eq!(max_ink, 255);
        // Corners of the bounding box are outside the round glyph.
        assert_eq!(at(0, 0), 0);
        assert_eq!(at(b.width - 1, b.height - 1), 0);
    }

    #[test]
    fn missing_characters_use_fallback_faces() {
        let Some(mut ctx) = system() else { return };
        let font = ctx.select(&[Family::Serif], 400, false).unwrap();
        // Sinhala is rarely in Latin serif faces; if any installed face covers
        // it, shaping must switch faces for it.
        let segments = ctx.shape(font, 400, false, 16.0, "a ශ්‍රී b");
        let total_glyphs: usize = segments.iter().map(|s| s.glyphs.len()).sum();
        assert!(total_glyphs >= 5);
        assert!(segments.iter().all(|s| s.advance >= 0.0));
    }
}

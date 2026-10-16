//! Glyph rasterization: outlines from `ttf-parser`, rendered to an 8-bit
//! coverage mask with an exact-area scanline accumulator.
//!
//! The accumulator is the signed-area technique popularized by font-rs
//! (Raph Levien): every line segment adds, to each pixel cell it crosses, the
//! signed area it covers to the cell's right edge; a running sum along each row
//! then yields exact non-zero coverage for non-overlapping contours. Curves are
//! flattened into line segments first.
//!
//! Only the requested glyph is decoded, so memory stays proportional to what
//! is drawn (unlike rasterizers that pre-parse every glyph of a face).

use rustlite_paint::GlyphBitmap;
use ttf_parser::{GlyphId, OutlineBuilder};

#[derive(Clone, Copy, Debug, PartialEq)]
struct P {
    x: f32,
    y: f32,
}

/// Collects flattened outline segments in bitmap space (y down).
struct Accumulator {
    width: usize,
    height: usize,
    /// One extra cell per row end plus slack, so `index + 1` never overflows.
    area: Vec<f32>,
    scale: f32,
    origin_x: f32,
    origin_y: f32,
    start: P,
    last: P,
}

impl Accumulator {
    fn to_bitmap(&self, x: f32, y: f32) -> P {
        P {
            x: x * self.scale - self.origin_x,
            y: self.origin_y - y * self.scale,
        }
    }

    fn line(&mut self, p0: P, p1: P) {
        if (p0.y - p1.y).abs() <= f32::EPSILON {
            return;
        }
        let (dir, p0, p1) = if p0.y < p1.y {
            (1.0, p0, p1)
        } else {
            (-1.0, p1, p0)
        };
        let dxdy = (p1.x - p0.x) / (p1.y - p0.y);
        let mut x = p0.x;
        let y_start = p0.y.max(0.0);
        if p0.y < 0.0 {
            x -= p0.y * dxdy;
        }
        let y_end = (p1.y.ceil() as usize).min(self.height);
        for y in (y_start as usize)..y_end {
            let row = y * self.width;
            let dy = ((y + 1) as f32).min(p1.y) - (y as f32).max(p0.y);
            let x_next = x + dxdy * dy;
            let d = dy * dir;
            let (x0, x1) = if x < x_next { (x, x_next) } else { (x_next, x) };
            // Bitmap bounds include a margin, so x stays within [0, width - 2].
            let x0 = x0.clamp(0.0, (self.width - 2) as f32);
            let x1 = x1.clamp(0.0, (self.width - 2) as f32);
            let x0_floor = x0.floor();
            let x0i = x0_floor as usize;
            let x1_ceil = x1.ceil();
            let x1i = x1_ceil as usize;
            if x1i <= x0i + 1 {
                // The segment stays within one cell on this row.
                let mid = 0.5 * (x0 + x1) - x0_floor;
                self.area[row + x0i] += d - d * mid;
                self.area[row + x0i + 1] += d * mid;
            } else {
                let s = (x1 - x0).recip();
                let x0f = x0 - x0_floor;
                let a0 = 0.5 * s * (1.0 - x0f) * (1.0 - x0f);
                let x1f = x1 - x1_ceil + 1.0;
                let am = 0.5 * s * x1f * x1f;
                self.area[row + x0i] += d * a0;
                if x1i == x0i + 2 {
                    self.area[row + x0i + 1] += d * (1.0 - a0 - am);
                } else {
                    let a1 = s * (1.5 - x0f);
                    self.area[row + x0i + 1] += d * (a1 - a0);
                    for xi in x0i + 2..x1i - 1 {
                        self.area[row + xi] += d * s;
                    }
                    let a2 = a1 + (x1i - x0i - 3) as f32 * s;
                    self.area[row + x1i - 1] += d * (1.0 - a2 - am);
                }
                self.area[row + x1i] += d * am;
            }
            x = x_next;
        }
    }

    fn line_to_point(&mut self, p: P) {
        let last = self.last;
        self.line(last, p);
        self.last = p;
    }
}

/// Number of line segments for a curve whose control polygon deviates by
/// `deviation` px from its chord (≈0.1px flattening error).
fn segments(deviation: f32) -> usize {
    (1.0 + (3.0 * deviation).sqrt()).clamp(1.0, 64.0) as usize
}

impl OutlineBuilder for Accumulator {
    fn move_to(&mut self, x: f32, y: f32) {
        let p = self.to_bitmap(x, y);
        self.start = p;
        self.last = p;
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.to_bitmap(x, y);
        self.line_to_point(p);
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let p0 = self.last;
        let c = self.to_bitmap(x1, y1);
        let p2 = self.to_bitmap(x, y);
        let dev = ((p0.x - 2.0 * c.x + p2.x).powi(2) + (p0.y - 2.0 * c.y + p2.y).powi(2)).sqrt();
        let n = segments(dev);
        for i in 1..=n {
            let t = i as f32 / n as f32;
            let mt = 1.0 - t;
            let p = P {
                x: mt * mt * p0.x + 2.0 * mt * t * c.x + t * t * p2.x,
                y: mt * mt * p0.y + 2.0 * mt * t * c.y + t * t * p2.y,
            };
            self.line_to_point(p);
        }
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let p0 = self.last;
        let c1 = self.to_bitmap(x1, y1);
        let c2 = self.to_bitmap(x2, y2);
        let p3 = self.to_bitmap(x, y);
        let dev = |a: P, b: P, c: P| {
            ((a.x - 2.0 * b.x + c.x).powi(2) + (a.y - 2.0 * b.y + c.y).powi(2)).sqrt()
        };
        let n = segments(dev(p0, c1, c2).max(dev(c1, c2, p3)) * 1.5);
        for i in 1..=n {
            let t = i as f32 / n as f32;
            let mt = 1.0 - t;
            let (a, b, c, d) = (mt * mt * mt, 3.0 * mt * mt * t, 3.0 * mt * t * t, t * t * t);
            let p = P {
                x: a * p0.x + b * c1.x + c * c2.x + d * p3.x,
                y: a * p0.y + b * c1.y + c * c2.y + d * p3.y,
            };
            self.line_to_point(p);
        }
    }

    fn close(&mut self) {
        let start = self.start;
        self.line_to_point(start);
    }
}

/// Glyphs larger than this many pixels per side are not rasterized (bounds
/// memory for absurd font sizes).
const MAX_GLYPH_PX: f32 = 2048.0;

/// Rasterizes `glyph` of `face` at `size` px. `None` for glyphs without
/// outlines (spaces) or with absurd sizes.
pub(crate) fn rasterize(face: &ttf_parser::Face<'_>, glyph: u16, size: f32) -> Option<GlyphBitmap> {
    let id = GlyphId(glyph);
    let bbox = face.glyph_bounding_box(id)?;
    let scale = size / f32::from(face.units_per_em());
    let x_min = (f32::from(bbox.x_min) * scale).floor() - 1.0;
    let x_max = (f32::from(bbox.x_max) * scale).ceil() + 1.0;
    let y_min = (f32::from(bbox.y_min) * scale).floor() - 1.0;
    let y_max = (f32::from(bbox.y_max) * scale).ceil() + 1.0;
    let (w, h) = (x_max - x_min, y_max - y_min);
    if !(w > 0.0 && h > 0.0 && w < MAX_GLYPH_PX && h < MAX_GLYPH_PX) {
        return None;
    }
    // Two extra columns: the accumulator writes one cell past each segment.
    let width = w as usize + 2;
    let height = h as usize;
    let mut acc = Accumulator {
        width,
        height,
        area: vec![0.0; width * height + 2],
        scale,
        origin_x: x_min,
        origin_y: y_max,
        start: P { x: 0.0, y: 0.0 },
        last: P { x: 0.0, y: 0.0 },
    };
    face.outline_glyph(id, &mut acc)?;

    let mut coverage = vec![0u8; width * height];
    let mut sum = 0.0f32;
    for (out, a) in coverage.iter_mut().zip(&acc.area) {
        sum += a;
        // Clamped to 0..=255 before the cast.
        *out = (sum.abs().min(1.0) * 255.0 + 0.5) as u8;
    }
    Some(GlyphBitmap {
        width: width as u32,
        height: height as u32,
        left: x_min as i32,
        top: -(y_max as i32),
        coverage,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_cell_segment_splits_area() {
        let mut acc = Accumulator {
            width: 4,
            height: 1,
            area: vec![0.0; 6],
            scale: 1.0,
            origin_x: 0.0,
            origin_y: 0.0,
            start: P { x: 0.0, y: 0.0 },
            last: P { x: 0.0, y: 0.0 },
        };
        // A vertical edge at x = 1.5 covering the whole row.
        acc.line(P { x: 1.5, y: 0.0 }, P { x: 1.5, y: 1.0 });
        assert!((acc.area[1] - 0.5).abs() < 1e-6);
        assert!((acc.area[2] - 0.5).abs() < 1e-6);
        let total: f32 = acc.area.iter().sum();
        assert!((total - 1.0).abs() < 1e-6);
    }

    #[test]
    fn closed_square_has_full_interior_and_empty_exterior() {
        let mut acc = Accumulator {
            width: 8,
            height: 6,
            area: vec![0.0; 8 * 6 + 2],
            scale: 1.0,
            origin_x: 0.0,
            origin_y: 0.0,
            start: P { x: 0.0, y: 0.0 },
            last: P { x: 0.0, y: 0.0 },
        };
        // Square from (1,1) to (5,5) in bitmap space.
        let pts = [
            P { x: 1.0, y: 1.0 },
            P { x: 5.0, y: 1.0 },
            P { x: 5.0, y: 5.0 },
            P { x: 1.0, y: 5.0 },
        ];
        for i in 0..4 {
            acc.line(pts[i], pts[(i + 1) % 4]);
        }
        let mut sum = 0.0;
        let cov: Vec<f32> = acc
            .area
            .iter()
            .take(48)
            .map(|a| {
                sum += a;
                sum.abs().min(1.0)
            })
            .collect();
        let at = |x: usize, y: usize| cov[y * 8 + x];
        assert!((at(2, 2) - 1.0).abs() < 1e-5);
        assert!((at(4, 4) - 1.0).abs() < 1e-5);
        assert!(at(0, 2) < 1e-5);
        assert!(at(6, 2) < 1e-5);
        assert!(at(2, 0) < 1e-5);
        assert!(at(2, 5) < 1e-5);
    }
}

use rustlite_paint::{Color, GlyphBitmap, Rect, Size};

/// Errors constructing a canvas or framebuffer.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CanvasError {
    #[error("canvas dimensions {width}x{height} overflow addressable memory")]
    TooLarge { width: u32, height: u32 },
    #[error("pixel buffer holds {actual} pixels but {width}x{height} needs {expected}")]
    BufferSize {
        width: u32,
        height: u32,
        expected: usize,
        actual: usize,
    },
}

/// Packs an opaque color into the `0x00RRGGBB` presentation format.
pub const fn pack_rgb(color: Color) -> u32 {
    ((color.r as u32) << 16) | ((color.g as u32) << 8) | color.b as u32
}

/// Unpacks a `0x00RRGGBB` pixel into an opaque color.
pub const fn unpack_rgb(pixel: u32) -> Color {
    Color::rgb((pixel >> 16) as u8, (pixel >> 8) as u8, pixel as u8)
}

fn pixel_count(width: u32, height: u32) -> Result<usize, CanvasError> {
    usize::try_from(u64::from(width) * u64::from(height))
        .map_err(|_| CanvasError::TooLarge { width, height })
}

/// A mutable view over a row-major `0x00RRGGBB` pixel buffer owned elsewhere
/// (for example a `softbuffer` presentation buffer).
#[derive(Debug)]
pub struct Canvas<'a> {
    pixels: &'a mut [u32],
    width: u32,
    height: u32,
}

impl<'a> Canvas<'a> {
    pub fn new(pixels: &'a mut [u32], width: u32, height: u32) -> Result<Self, CanvasError> {
        let expected = pixel_count(width, height)?;
        if pixels.len() != expected {
            return Err(CanvasError::BufferSize {
                width,
                height,
                expected,
                actual: pixels.len(),
            });
        }
        Ok(Self {
            pixels,
            width,
            height,
        })
    }

    pub fn size(&self) -> Size {
        Size::new(self.width, self.height)
    }

    pub fn bounds(&self) -> Rect {
        Rect::from_size(self.size())
    }

    /// Fills `rect` (clipped to the canvas) with `color`, blending when translucent.
    /// Returns the number of pixels written.
    pub fn fill(&mut self, rect: Rect, color: Color) -> u64 {
        let Some(rect) = self.bounds().intersect(rect) else {
            return 0;
        };
        if color.is_transparent() {
            return 0;
        }
        // `rect` lies within the canvas, so its coordinates are non-negative and in range.
        let (x0, y0) = (rect.x as usize, rect.y as usize);
        let (w, h) = (rect.width as usize, rect.height as usize);
        let stride = self.width as usize;
        let rows = self.pixels[y0 * stride..]
            .chunks_exact_mut(stride)
            .take(h)
            .map(|row| &mut row[x0..x0 + w]);

        if color.is_opaque() {
            let packed = pack_rgb(color);
            for row in rows {
                row.fill(packed);
            }
        } else {
            let alpha = u32::from(color.a);
            let inv = 255 - alpha;
            let (sr, sg, sb) = (
                u32::from(color.r) * alpha,
                u32::from(color.g) * alpha,
                u32::from(color.b) * alpha,
            );
            for row in rows {
                for px in row {
                    let d = *px;
                    let r = (sr + ((d >> 16) & 0xFF) * inv + 127) / 255;
                    let g = (sg + ((d >> 8) & 0xFF) * inv + 127) / 255;
                    let b = (sb + (d & 0xFF) * inv + 127) / 255;
                    *px = (r << 16) | (g << 8) | b;
                }
            }
        }
        (w * h) as u64
    }

    /// Blends `color` through a coverage mask whose top-left corner is at
    /// (`x`, `y`), restricted to `clip`. Returns the number of pixels touched.
    pub fn blend_coverage(
        &mut self,
        x: i32,
        y: i32,
        mask: &GlyphBitmap,
        color: Color,
        clip: Rect,
    ) -> u64 {
        let mask_rect = Rect::new(x, y, mask.width, mask.height);
        let Some(area) = self
            .bounds()
            .intersect(clip)
            .and_then(|c| c.intersect(mask_rect))
        else {
            return 0;
        };
        let stride = self.width as usize;
        let mut touched = 0;
        // `area` is inside both the canvas and the mask, so all indices are in range.
        for row in 0..area.height as usize {
            let cy = area.y as usize + row;
            let my = (area.y - y) as usize + row;
            for col in 0..area.width as usize {
                let cx = area.x as usize + col;
                let mx = (area.x - x) as usize + col;
                let coverage = u32::from(mask.coverage[my * mask.width as usize + mx]);
                let alpha = coverage * u32::from(color.a) / 255;
                if alpha == 0 {
                    continue;
                }
                let inv = 255 - alpha;
                let px = &mut self.pixels[cy * stride + cx];
                let d = *px;
                let r = (u32::from(color.r) * alpha + ((d >> 16) & 0xFF) * inv + 127) / 255;
                let g = (u32::from(color.g) * alpha + ((d >> 8) & 0xFF) * inv + 127) / 255;
                let b = (u32::from(color.b) * alpha + (d & 0xFF) * inv + 127) / 255;
                *px = (r << 16) | (g << 8) | b;
                touched += 1;
            }
        }
        touched
    }
}

/// An owned pixel buffer, used for headless rendering and tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Framebuffer {
    pixels: Vec<u32>,
    width: u32,
    height: u32,
}

impl Framebuffer {
    pub fn new(width: u32, height: u32) -> Result<Self, CanvasError> {
        let len = pixel_count(width, height)?;
        Ok(Self {
            pixels: vec![0; len],
            width,
            height,
        })
    }

    pub fn canvas(&mut self) -> Canvas<'_> {
        Canvas {
            pixels: &mut self.pixels,
            width: self.width,
            height: self.height,
        }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn pixels(&self) -> &[u32] {
        &self.pixels
    }

    pub fn pixel(&self, x: u32, y: u32) -> Option<Color> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let index = y as usize * self.width as usize + x as usize;
        Some(unpack_rgb(self.pixels[index]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_mismatched_buffer() {
        let mut buf = vec![0; 5];
        let err = Canvas::new(&mut buf, 2, 2).unwrap_err();
        assert!(matches!(
            err,
            CanvasError::BufferSize {
                expected: 4,
                actual: 5,
                ..
            }
        ));
    }

    #[test]
    fn zero_sized_canvas_is_valid_and_inert() {
        let mut buf = Vec::new();
        let mut canvas = Canvas::new(&mut buf, 0, 10).unwrap();
        assert_eq!(canvas.fill(Rect::new(0, 0, 10, 10), Color::BLACK), 0);
    }

    #[test]
    fn pack_round_trips() {
        let c = Color::rgb(0x12, 0x34, 0x56);
        assert_eq!(pack_rgb(c), 0x0012_3456);
        assert_eq!(unpack_rgb(pack_rgb(c)), c);
    }
}

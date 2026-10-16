use rustlite_paint::{Color, GlyphBitmap, ImageData, Rect, Size};

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

    /// Draws `image` scaled to `dest` with bilinear filtering and alpha
    /// blending, restricted to `clip`. Returns the number of pixels touched.
    ///
    /// Bilinear sampling is accurate for upscaling and mild downscaling; strong
    /// downscaling aliases (no mipmaps yet).
    pub fn draw_image(&mut self, dest: Rect, image: &ImageData, clip: Rect) -> u64 {
        let Some(area) = self
            .bounds()
            .intersect(clip)
            .and_then(|c| c.intersect(dest))
        else {
            return 0;
        };
        let (iw, ih) = (image.width as usize, image.height as usize);
        let sx = image.width as f32 / dest.width as f32;
        let sy = image.height as f32 / dest.height as f32;
        let texel = |x: usize, y: usize| {
            let i = (y * iw + x) * 4;
            [
                f32::from(image.rgba[i]),
                f32::from(image.rgba[i + 1]),
                f32::from(image.rgba[i + 2]),
                f32::from(image.rgba[i + 3]),
            ]
        };
        let stride = self.width as usize;
        let mut touched = 0;
        // `area` lies inside both the canvas and `dest`, so indices are in range.
        for row in 0..area.height as usize {
            let cy = area.y as usize + row;
            // Sample at pixel centers, mapped into image space.
            let v = ((cy as i64 - i64::from(dest.y)) as f32 + 0.5) * sy - 0.5;
            let v = v.clamp(0.0, (ih - 1) as f32);
            let (y0, fy) = (v.floor() as usize, v - v.floor());
            let y1 = (y0 + 1).min(ih - 1);
            for col in 0..area.width as usize {
                let cx = area.x as usize + col;
                let u = ((cx as i64 - i64::from(dest.x)) as f32 + 0.5) * sx - 0.5;
                let u = u.clamp(0.0, (iw - 1) as f32);
                let (x0, fx) = (u.floor() as usize, u - u.floor());
                let x1 = (x0 + 1).min(iw - 1);
                let (a, b, c, d) = (texel(x0, y0), texel(x1, y0), texel(x0, y1), texel(x1, y1));
                let mut p = [0.0f32; 4];
                for k in 0..4 {
                    let top = a[k] + (b[k] - a[k]) * fx;
                    let bottom = c[k] + (d[k] - c[k]) * fx;
                    p[k] = top + (bottom - top) * fy;
                }
                let alpha = p[3] / 255.0;
                if alpha <= 0.0 {
                    continue;
                }
                let px = &mut self.pixels[cy * stride + cx];
                let dst = [
                    ((*px >> 16) & 0xFF) as f32,
                    ((*px >> 8) & 0xFF) as f32,
                    (*px & 0xFF) as f32,
                ];
                let mix = |s: f32, d: f32| {
                    (s * alpha + d * (1.0 - alpha)).round().clamp(0.0, 255.0) as u32
                };
                *px = (mix(p[0], dst[0]) << 16) | (mix(p[1], dst[1]) << 8) | mix(p[2], dst[2]);
                touched += 1;
            }
        }
        touched
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

#[cfg(test)]
mod image_tests {
    use super::*;

    fn image(w: u32, h: u32, pixels: &[[u8; 4]]) -> ImageData {
        ImageData::new(w, h, pixels.concat()).unwrap()
    }

    #[test]
    fn draws_scaled_image_with_nearest_corners() {
        // 2x1 image: red, blue. Drawn at 4x2.
        let img = image(2, 1, &[[255, 0, 0, 255], [0, 0, 255, 255]]);
        let mut fb = Framebuffer::new(6, 3).unwrap();
        let n = fb
            .canvas()
            .draw_image(Rect::new(1, 1, 4, 2), &img, Rect::new(0, 0, 6, 3));
        assert_eq!(n, 8);
        assert_eq!(fb.pixel(1, 1), Some(Color::rgb(255, 0, 0)));
        assert_eq!(fb.pixel(4, 2), Some(Color::rgb(0, 0, 255)));
        assert_eq!(
            fb.pixel(0, 0),
            Some(Color::rgb(0, 0, 0)),
            "outside dest untouched"
        );
        // Middle pixels interpolate between red and blue.
        let mid = fb.pixel(2, 1).unwrap();
        assert!(mid.r > 0 && mid.b > 0, "{mid:?}");
    }

    #[test]
    fn transparent_pixels_keep_the_background_and_clip_applies() {
        let img = image(1, 1, &[[255, 255, 255, 0]]);
        let mut fb = Framebuffer::new(2, 2).unwrap();
        assert_eq!(
            fb.canvas()
                .draw_image(Rect::new(0, 0, 2, 2), &img, Rect::new(0, 0, 2, 2)),
            0
        );
        let solid = image(1, 1, &[[9, 9, 9, 255]]);
        let n = fb
            .canvas()
            .draw_image(Rect::new(0, 0, 2, 2), &solid, Rect::new(0, 0, 1, 1));
        assert_eq!(n, 1);
        assert_eq!(fb.pixel(1, 1), Some(Color::rgb(0, 0, 0)));
    }

    #[test]
    fn rejects_inconsistent_image_data() {
        assert!(ImageData::new(2, 2, vec![0; 15]).is_none());
        assert!(ImageData::new(0, 2, vec![]).is_none());
    }
}

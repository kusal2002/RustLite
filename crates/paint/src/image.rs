/// A decoded raster image: non-premultiplied 8-bit RGBA, row-major.
#[derive(Clone, PartialEq, Eq)]
pub struct ImageData {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl ImageData {
    /// Returns `None` unless `rgba` holds exactly `width * height` pixels.
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Option<Self> {
        let expected = (width as usize)
            .checked_mul(height as usize)?
            .checked_mul(4)?;
        (rgba.len() == expected && width > 0 && height > 0).then_some(Self {
            width,
            height,
            rgba,
        })
    }

    /// Decoded size in bytes (for memory accounting).
    pub fn byte_size(&self) -> usize {
        self.rgba.len()
    }
}

impl std::fmt::Debug for ImageData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImageData")
            .field("width", &self.width)
            .field("height", &self.height)
            .finish_non_exhaustive()
    }
}

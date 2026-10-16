//! Loading `<img>` resources for local documents.
//!
//! Policy (until networking and a site-isolation model exist, Phase 6):
//! - Only relative URLs of a document opened from a local file are loaded.
//! - The resolved file must stay inside the document's directory (no `..`
//!   escapes, no absolute paths, no symlinks pointing outside), so a page
//!   cannot read arbitrary local files.
//! - Files over [`MAX_IMAGE_FILE_BYTES`] and images over
//!   [`MAX_IMAGE_DIMENSION`] px per side are refused.
//! - PNG, JPEG and GIF (first frame) only.
//!
//! Decoding is synchronous during page load for now; it moves off the UI
//! thread together with network loading.

use std::collections::HashMap;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use rustlite_dom::Document;
use rustlite_layout::Images;
use rustlite_paint::ImageData;

pub const MAX_IMAGE_FILE_BYTES: u64 = 32 * 1024 * 1024;
pub const MAX_IMAGE_DIMENSION: u32 = 8192;

/// Why an image was not loaded (logged; the element renders as broken).
#[derive(Debug, PartialEq, Eq)]
pub enum ImageError {
    UnsupportedUrl,
    OutsideDocumentDirectory,
    NotFound,
    TooLarge,
    Decode(String),
}

/// Resolves `src` against `base_dir`, enforcing the directory policy.
pub fn resolve_local(base_dir: &Path, src: &str) -> Result<PathBuf, ImageError> {
    let src = src.trim();
    // Strip fragment and query; they do not name files.
    let src = src.split(['#', '?']).next().unwrap_or("");
    let has_scheme = src.split_once(':').is_some_and(|(scheme, _)| {
        scheme.len() > 1
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
    });
    if src.is_empty() || has_scheme || src.starts_with('/') || src.starts_with('\\') {
        return Err(ImageError::UnsupportedUrl);
    }
    let base = base_dir.canonicalize().map_err(|_| ImageError::NotFound)?;
    let candidate = base.join(src.replace('\\', "/"));
    let resolved = candidate.canonicalize().map_err(|_| ImageError::NotFound)?;
    if !resolved.starts_with(&base) {
        return Err(ImageError::OutsideDocumentDirectory);
    }
    Ok(resolved)
}

/// Reads and decodes one image file.
pub fn decode_file(path: &Path) -> Result<ImageData, ImageError> {
    let size = std::fs::metadata(path)
        .map_err(|_| ImageError::NotFound)?
        .len();
    if size > MAX_IMAGE_FILE_BYTES {
        return Err(ImageError::TooLarge);
    }
    let bytes = std::fs::read(path).map_err(|_| ImageError::NotFound)?;
    decode_bytes(&bytes)
}

/// Decodes PNG, JPEG or GIF bytes with dimension and allocation limits.
pub fn decode_bytes(bytes: &[u8]) -> Result<ImageData, ImageError> {
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| ImageError::Decode(e.to_string()))?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_DIMENSION);
    limits.max_image_height = Some(MAX_IMAGE_DIMENSION);
    limits.max_alloc = Some(u64::from(MAX_IMAGE_DIMENSION) * u64::from(MAX_IMAGE_DIMENSION) * 4);
    reader.limits(limits);
    let decoded = reader
        .decode()
        .map_err(|e| ImageError::Decode(e.to_string()))?;
    let rgba = decoded.to_rgba8();
    let (w, h) = rgba.dimensions();
    ImageData::new(w, h, rgba.into_raw()).ok_or_else(|| ImageError::Decode("empty image".into()))
}

/// Loads every `<img src>` in `dom` relative to `base_dir`. The same file is
/// decoded once and shared between elements.
pub fn load_images(dom: &Document, base_dir: &Path) -> Images {
    let mut by_path: HashMap<PathBuf, Option<Rc<ImageData>>> = HashMap::new();
    let mut images = Images::new();
    for node in dom.descendants(dom.root()) {
        if !dom.is_element_named(node, "img") {
            continue;
        }
        let Some(src) = dom.attribute(node, "src") else {
            continue;
        };
        let loaded = match resolve_local(base_dir, src) {
            Ok(path) => by_path
                .entry(path.clone())
                .or_insert_with(|| match decode_file(&path) {
                    Ok(img) => {
                        tracing::debug!(path = %path.display(), w = img.width, h = img.height, "image decoded");
                        Some(Rc::new(img))
                    }
                    Err(e) => {
                        tracing::warn!(path = %path.display(), error = ?e, "image not loaded");
                        None
                    }
                })
                .clone(),
            Err(e) => {
                tracing::warn!(src, error = ?e, "image not loaded");
                None
            }
        };
        if let Some(img) = loaded {
            images.insert(node, img);
        }
    }
    images
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 2x1 PNG (red, transparent) encoded with the renderer's encoder is RGB
    /// only, so build a tiny RGBA PNG by hand via the `image` crate instead.
    fn png_bytes(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(w, h, image::Rgba([10, 20, 30, 255]));
        let mut out = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut out, image::ImageFormat::Png)
            .unwrap();
        out.into_inner()
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("rustlite-images-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        dir
    }

    #[test]
    fn decodes_png() {
        let img = decode_bytes(&png_bytes(3, 2)).unwrap();
        assert_eq!((img.width, img.height), (3, 2));
        assert_eq!(&img.rgba[..4], &[10, 20, 30, 255]);
    }

    #[test]
    fn rejects_garbage_and_oversized_images() {
        assert!(matches!(
            decode_bytes(b"not an image"),
            Err(ImageError::Decode(_))
        ));
        let huge = png_bytes(MAX_IMAGE_DIMENSION + 1, 1);
        assert!(matches!(decode_bytes(&huge), Err(ImageError::Decode(_))));
    }

    #[test]
    fn path_policy() {
        let dir = temp_dir("policy");
        std::fs::write(dir.join("sub/a.png"), png_bytes(1, 1)).unwrap();
        std::fs::write(dir.join("../rustlite-outside.png"), b"x").unwrap();
        assert!(resolve_local(&dir, "sub/a.png").is_ok());
        assert!(resolve_local(&dir, "./sub/../sub/a.png?x=1#y").is_ok());
        assert_eq!(
            resolve_local(&dir, "../rustlite-outside.png"),
            Err(ImageError::OutsideDocumentDirectory)
        );
        assert_eq!(
            resolve_local(&dir, "/etc/passwd"),
            Err(ImageError::UnsupportedUrl)
        );
        assert_eq!(
            resolve_local(&dir, "http://example.com/a.png"),
            Err(ImageError::UnsupportedUrl)
        );
        assert_eq!(
            resolve_local(&dir, "data:image/png;base64,xx"),
            Err(ImageError::UnsupportedUrl)
        );
        assert_eq!(
            resolve_local(&dir, "missing.png"),
            Err(ImageError::NotFound)
        );
        let _ = std::fs::remove_file(dir.join("../rustlite-outside.png"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_images_shares_decoded_files() {
        let dir = temp_dir("share");
        std::fs::write(dir.join("a.png"), png_bytes(4, 4)).unwrap();
        let out = rustlite_html::parse_document(
            "<img src=a.png><img src='./a.png'><img src=nope.png><img>",
        );
        let images = load_images(&out.document, &dir);
        assert_eq!(images.len(), 2);
        let all: Vec<_> = images.values().collect();
        assert!(Rc::ptr_eq(all[0], all[1]));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

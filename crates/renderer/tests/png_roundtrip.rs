//! Validates the PNG encoder's container structure without an external decoder.

use rustlite_paint::{Color, DisplayList, Rect};
use rustlite_renderer::{Framebuffer, encode_png, rasterize};

fn chunks(png: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    let mut pos = 8;
    while pos < png.len() {
        let len = u32::from_be_bytes(png[pos..pos + 4].try_into().unwrap()) as usize;
        let kind = String::from_utf8(png[pos + 4..pos + 8].to_vec()).unwrap();
        out.push((kind, png[pos + 8..pos + 8 + len].to_vec()));
        pos += 12 + len;
    }
    out
}

#[test]
fn encodes_well_formed_png() {
    let mut fb = Framebuffer::new(3, 2).unwrap();
    let mut list = DisplayList::new();
    list.fill_rect(Rect::new(0, 0, 1, 1), Color::rgb(10, 20, 30));
    rasterize(&mut fb.canvas(), &list, Color::WHITE);

    let mut png = Vec::new();
    encode_png(&fb, &mut png).unwrap();

    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    let chunks = chunks(&png);
    let kinds: Vec<_> = chunks.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(kinds, ["IHDR", "IDAT", "IEND"]);

    let ihdr = &chunks[0].1;
    assert_eq!(&ihdr[0..4], 3u32.to_be_bytes());
    assert_eq!(&ihdr[4..8], 2u32.to_be_bytes());

    // Stored deflate: zlib header (2) + block header (5), then raw scanlines.
    let idat = &chunks[1].1;
    let raw = &idat[7..idat.len() - 4];
    assert_eq!(raw.len(), 2 * (1 + 3 * 3));
    assert_eq!(&raw[..4], [0, 10, 20, 30]);
    assert_eq!(&raw[4..7], [255, 255, 255]);
}

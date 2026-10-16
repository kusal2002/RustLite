//! Minimal, dependency-free PNG encoder for headless frame capture.
//!
//! Uses uncompressed ("stored") deflate blocks: files are large but encoding is
//! trivially correct and fast. Output is meant for tests, debugging, and
//! differential screenshot comparison, not for end users.

use std::io::{self, Write};

use crate::Framebuffer;

#[derive(Debug, thiserror::Error)]
pub enum PngError {
    #[error("cannot encode an empty {width}x{height} image")]
    Empty { width: u32, height: u32 },
    #[error("I/O error while writing PNG")]
    Io(#[from] io::Error),
}

const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1A, b'\n'];
const MAX_STORED_BLOCK: usize = 0xFFFF;

/// Writes `fb` as an 8-bit RGB PNG.
pub fn encode_png(fb: &Framebuffer, out: &mut impl Write) -> Result<(), PngError> {
    let (width, height) = (fb.width(), fb.height());
    if width == 0 || height == 0 {
        return Err(PngError::Empty { width, height });
    }

    out.write_all(&SIGNATURE)?;

    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    // Bit depth 8, color type 2 (RGB), deflate, adaptive filtering, no interlace.
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    write_chunk(out, b"IHDR", &ihdr)?;

    // Raw scanlines: filter byte 0 (None) followed by RGB triples.
    let row_len = 1 + width as usize * 3;
    let mut raw = Vec::with_capacity(row_len * height as usize);
    for row in fb.pixels().chunks_exact(width as usize) {
        raw.push(0);
        for &px in row {
            raw.extend_from_slice(&[(px >> 16) as u8, (px >> 8) as u8, px as u8]);
        }
    }
    write_chunk(out, b"IDAT", &zlib_stored(&raw))?;
    write_chunk(out, b"IEND", &[])?;
    Ok(())
}

fn zlib_stored(data: &[u8]) -> Vec<u8> {
    let blocks = data.len().div_ceil(MAX_STORED_BLOCK).max(1);
    let mut out = Vec::with_capacity(2 + data.len() + blocks * 5 + 4);
    out.extend_from_slice(&[0x78, 0x01]);
    let mut chunks = data.chunks(MAX_STORED_BLOCK).peekable();
    if chunks.peek().is_none() {
        out.extend_from_slice(&[1, 0, 0, 0xFF, 0xFF]);
    }
    while let Some(chunk) = chunks.next() {
        let last = u8::from(chunks.peek().is_none());
        // Chunks are at most MAX_STORED_BLOCK bytes, so the length fits in u16.
        let len = chunk.len() as u16;
        out.push(last);
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(chunk);
    }
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

fn write_chunk(out: &mut impl Write, kind: &[u8; 4], data: &[u8]) -> io::Result<()> {
    let len = u32::try_from(data.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "PNG chunk exceeds 4 GiB"))?;
    out.write_all(&len.to_be_bytes())?;
    out.write_all(kind)?;
    out.write_all(data)?;
    let crc = crc32_update(crc32_update(0xFFFF_FFFF, kind), data) ^ 0xFFFF_FFFF;
    out.write_all(&crc.to_be_bytes())
}

fn crc32_update(mut crc: u32, data: &[u8]) -> u32 {
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    crc
}

fn adler32(data: &[u8]) -> u32 {
    const MOD: u32 = 65_521;
    let (mut a, mut b) = (1u32, 0u32);
    // 5552 is the largest block size for which `b` cannot overflow before reduction.
    for chunk in data.chunks(5552) {
        for &byte in chunk {
            a += u32::from(byte);
            b += a;
        }
        a %= MOD;
        b %= MOD;
    }
    (b << 16) | a
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_known_vector() {
        assert_eq!(
            crc32_update(0xFFFF_FFFF, b"123456789") ^ 0xFFFF_FFFF,
            0xCBF4_3926
        );
    }

    #[test]
    fn adler32_known_vector() {
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
    }

    #[test]
    fn stored_blocks_split_at_64k() {
        let data = vec![7u8; MAX_STORED_BLOCK + 10];
        let z = zlib_stored(&data);
        // header + 2 block headers + payload + adler
        assert_eq!(z.len(), 2 + 5 + 5 + data.len() + 4);
        assert_eq!(z[2], 0, "first block must not be final");
        assert_eq!(z[2 + 5 + MAX_STORED_BLOCK], 1, "second block must be final");
    }

    #[test]
    fn rejects_empty_image() {
        let fb = Framebuffer::new(0, 0).unwrap();
        assert!(matches!(
            encode_png(&fb, &mut Vec::new()),
            Err(PngError::Empty { .. })
        ));
    }
}

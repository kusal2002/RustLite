//! Wire format between the browser and renderer processes.
//!
//! Frames are a little-endian `u32` length followed by that many bytes
//! (capped at [`MAX_FRAME`]). Inside a frame, values are written with
//! [`Writer`] and read back with [`Reader`], whose every read is
//! bounds-checked: a truncated or malicious frame yields an error, never a
//! panic or an oversized allocation.

use std::io::{self, Read, Write};

/// Largest frame accepted in either direction (display lists with images
/// are the big ones).
pub const MAX_FRAME: usize = 64 * 1024 * 1024;
/// Longest string or byte array inside a frame.
const MAX_FIELD: usize = MAX_FRAME;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DecodeError {
    #[error("message truncated")]
    Truncated,
    #[error("invalid value in message: {0}")]
    Invalid(&'static str),
}

pub fn write_frame(w: &mut impl Write, payload: &[u8]) -> io::Result<()> {
    if payload.len() > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "frame too large",
        ));
    }
    w.write_all(&(payload.len() as u32).to_le_bytes())?;
    w.write_all(payload)?;
    w.flush()
}

/// Reads one frame; `Ok(None)` at a clean end of stream.
pub fn read_frame(r: &mut impl Read) -> io::Result<Option<Vec<u8>>> {
    let mut len = [0u8; 4];
    match r.read_exact(&mut len) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u32::from_le_bytes(len) as usize;
    if len > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "frame too large",
        ));
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    Ok(Some(buf))
}

#[derive(Debug, Default)]
pub struct Writer {
    pub buf: Vec<u8>,
}

impl Writer {
    pub fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }
    pub fn bool(&mut self, v: bool) {
        self.u8(u8::from(v));
    }
    pub fn u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    pub fn u64(&mut self, v: u64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    pub fn i32(&mut self, v: i32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    pub fn f32(&mut self, v: f32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    pub fn f64(&mut self, v: f64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    pub fn len(&mut self, n: usize) {
        self.u32(u32::try_from(n).unwrap_or(u32::MAX));
    }
    pub fn bytes(&mut self, v: &[u8]) {
        self.len(v.len());
        self.buf.extend_from_slice(v);
    }
    pub fn str(&mut self, v: &str) {
        self.bytes(v.as_bytes());
    }
    pub fn opt_str(&mut self, v: Option<&str>) {
        match v {
            Some(s) => {
                self.bool(true);
                self.str(s);
            }
            None => self.bool(false),
        }
    }
    pub fn opt_u32(&mut self, v: Option<u32>) {
        match v {
            Some(x) => {
                self.bool(true);
                self.u32(x);
            }
            None => self.bool(false),
        }
    }
    pub fn opt_f64(&mut self, v: Option<f64>) {
        match v {
            Some(x) => {
                self.bool(true);
                self.f64(x);
            }
            None => self.bool(false),
        }
    }
}

#[derive(Debug)]
pub struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], DecodeError> {
        let end = self.pos.checked_add(n).ok_or(DecodeError::Truncated)?;
        let slice = self.data.get(self.pos..end).ok_or(DecodeError::Truncated)?;
        self.pos = end;
        Ok(slice)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], DecodeError> {
        let s = self.take(N)?;
        let mut a = [0u8; N];
        a.copy_from_slice(s);
        Ok(a)
    }

    pub fn u8(&mut self) -> Result<u8, DecodeError> {
        Ok(self.take(1)?[0])
    }
    pub fn bool(&mut self) -> Result<bool, DecodeError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(DecodeError::Invalid("bool")),
        }
    }
    pub fn u32(&mut self) -> Result<u32, DecodeError> {
        Ok(u32::from_le_bytes(self.array()?))
    }
    pub fn u64(&mut self) -> Result<u64, DecodeError> {
        Ok(u64::from_le_bytes(self.array()?))
    }
    pub fn i32(&mut self) -> Result<i32, DecodeError> {
        Ok(i32::from_le_bytes(self.array()?))
    }
    /// A finite `f32` (NaN and infinities are rejected).
    pub fn f32(&mut self) -> Result<f32, DecodeError> {
        let v = f32::from_le_bytes(self.array()?);
        if v.is_finite() {
            Ok(v)
        } else {
            Err(DecodeError::Invalid("non-finite number"))
        }
    }
    pub fn f64(&mut self) -> Result<f64, DecodeError> {
        let v = f64::from_le_bytes(self.array()?);
        if v.is_finite() {
            Ok(v)
        } else {
            Err(DecodeError::Invalid("non-finite number"))
        }
    }
    /// A length prefix for `item_size`-byte items, checked against what is
    /// left in the frame so hostile lengths cannot force huge allocations.
    pub fn len(&mut self, item_size: usize) -> Result<usize, DecodeError> {
        let n = self.u32()? as usize;
        let remaining = self.data.len() - self.pos;
        if n > MAX_FIELD || n.saturating_mul(item_size.max(1)) > remaining {
            return Err(DecodeError::Truncated);
        }
        Ok(n)
    }
    pub fn bytes(&mut self) -> Result<&'a [u8], DecodeError> {
        let n = self.len(1)?;
        self.take(n)
    }
    pub fn str(&mut self) -> Result<String, DecodeError> {
        String::from_utf8(self.bytes()?.to_vec()).map_err(|_| DecodeError::Invalid("utf-8"))
    }
    pub fn opt_str(&mut self) -> Result<Option<String>, DecodeError> {
        Ok(if self.bool()? {
            Some(self.str()?)
        } else {
            None
        })
    }
    pub fn opt_u32(&mut self) -> Result<Option<u32>, DecodeError> {
        Ok(if self.bool()? {
            Some(self.u32()?)
        } else {
            None
        })
    }
    pub fn opt_f64(&mut self) -> Result<Option<f64>, DecodeError> {
        Ok(if self.bool()? {
            Some(self.f64()?)
        } else {
            None
        })
    }
    pub fn finished(&self) -> bool {
        self.pos == self.data.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_round_trip() {
        let mut w = Writer::default();
        w.u8(7);
        w.bool(true);
        w.u32(1 << 30);
        w.i32(-5);
        w.f32(1.5);
        w.f64(-2.25);
        w.str("héllo");
        w.opt_str(None);
        w.opt_u32(Some(9));
        let mut r = Reader::new(&w.buf);
        assert_eq!(r.u8(), Ok(7));
        assert_eq!(r.bool(), Ok(true));
        assert_eq!(r.u32(), Ok(1 << 30));
        assert_eq!(r.i32(), Ok(-5));
        assert_eq!(r.f32(), Ok(1.5));
        assert_eq!(r.f64(), Ok(-2.25));
        assert_eq!(r.str().as_deref(), Ok("héllo"));
        assert_eq!(r.opt_str(), Ok(None));
        assert_eq!(r.opt_u32(), Ok(Some(9)));
        assert!(r.finished());
    }

    #[test]
    fn hostile_input_is_rejected_without_panicking() {
        // Claims a 4 GB string in a 6-byte frame.
        let mut r = Reader::new(&[0xff, 0xff, 0xff, 0xff, 1, 2]);
        assert_eq!(r.str(), Err(DecodeError::Truncated));
        let mut r = Reader::new(&[2]);
        assert_eq!(r.bool(), Err(DecodeError::Invalid("bool")));
        let nan = f32::NAN.to_le_bytes();
        let mut r = Reader::new(&nan);
        assert!(r.f32().is_err());
        let mut r = Reader::new(&[1, 2]);
        assert_eq!(r.u32(), Err(DecodeError::Truncated));
    }

    #[test]
    fn frames_round_trip_and_are_capped() {
        let mut buf = Vec::new();
        write_frame(&mut buf, b"abc").unwrap();
        write_frame(&mut buf, b"").unwrap();
        let mut r = &buf[..];
        assert_eq!(read_frame(&mut r).unwrap().as_deref(), Some(&b"abc"[..]));
        assert_eq!(read_frame(&mut r).unwrap().as_deref(), Some(&b""[..]));
        assert_eq!(read_frame(&mut r).unwrap(), None);
        let huge = (MAX_FRAME as u32 + 1).to_le_bytes();
        assert!(read_frame(&mut &huge[..]).is_err());
    }
}

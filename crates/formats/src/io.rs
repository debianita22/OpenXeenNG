// SPDX-License-Identifier: GPL-3.0-or-later
//! Little-endian byte reader/writer helpers.

use crate::error::{FormatError, Result};

/// A bounds-checked little-endian cursor over a byte slice.
#[derive(Debug, Clone)]
pub struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
    what: &'static str,
}

impl<'a> Reader<'a> {
    /// Creates a reader; `what` names the structure in error messages.
    pub fn new(data: &'a [u8], what: &'static str) -> Self {
        Reader { data, pos: 0, what }
    }

    /// Current byte offset.
    pub fn pos(&self) -> usize {
        self.pos
    }

    /// Total length of the underlying data.
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// True when the underlying data is empty.
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Bytes left to read.
    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    /// True once every byte has been consumed.
    pub fn at_end(&self) -> bool {
        self.pos >= self.data.len()
    }

    /// Moves to an absolute offset (may be equal to the length).
    pub fn seek(&mut self, pos: usize) -> Result<()> {
        if pos > self.data.len() {
            return Err(FormatError::UnexpectedEof {
                what: self.what,
                offset: pos,
                needed: 0,
            });
        }
        self.pos = pos;
        Ok(())
    }

    /// Skips `n` bytes.
    pub fn skip(&mut self, n: usize) -> Result<()> {
        self.bytes(n).map(|_| ())
    }

    /// Reads `n` raw bytes.
    pub fn bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).filter(|&e| e <= self.data.len());
        match end {
            Some(end) => {
                let out = &self.data[self.pos..end];
                self.pos = end;
                Ok(out)
            }
            None => Err(FormatError::UnexpectedEof {
                what: self.what,
                offset: self.pos,
                needed: n,
            }),
        }
    }

    /// Reads one unsigned byte.
    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.bytes(1)?[0])
    }

    /// Reads one signed byte.
    pub fn i8(&mut self) -> Result<i8> {
        Ok(self.u8()? as i8)
    }

    /// Reads a little-endian `u16`.
    pub fn u16(&mut self) -> Result<u16> {
        let b = self.bytes(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    /// Reads a little-endian `u32`.
    pub fn u32(&mut self) -> Result<u32> {
        let b = self.bytes(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// Reads a fixed-size, NUL-padded string field (Latin-1/CP437 bytes kept as-is).
    pub fn fixed_str(&mut self, len: usize) -> Result<String> {
        Ok(cstr_to_string(self.bytes(len)?))
    }
}

/// Converts bytes up to the first NUL into a `String`, mapping each byte to
/// the Unicode code point of the same value (the game stores 8-bit text).
pub fn cstr_to_string(bytes: &[u8]) -> String {
    bytes
        .iter()
        .take_while(|&&b| b != 0)
        .map(|&b| b as char)
        .collect()
}

/// Converts a string produced by [`cstr_to_string`] back to game bytes.
/// Characters above U+00FF are replaced with `?`.
pub fn string_to_bytes(s: &str) -> Vec<u8> {
    s.chars()
        .map(|c| if (c as u32) < 256 { c as u8 } else { b'?' })
        .collect()
}

/// Growable little-endian writer.
#[derive(Debug, Default, Clone)]
pub struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    /// Creates an empty writer.
    pub fn new() -> Self {
        Self::default()
    }

    /// Bytes written so far.
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    /// True if nothing has been written.
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// Appends one byte.
    pub fn u8(&mut self, v: u8) -> &mut Self {
        self.buf.push(v);
        self
    }

    /// Appends one signed byte.
    pub fn i8(&mut self, v: i8) -> &mut Self {
        self.buf.push(v as u8);
        self
    }

    /// Appends a little-endian `u16`.
    pub fn u16(&mut self, v: u16) -> &mut Self {
        self.buf.extend_from_slice(&v.to_le_bytes());
        self
    }

    /// Appends a little-endian `u32`.
    pub fn u32(&mut self, v: u32) -> &mut Self {
        self.buf.extend_from_slice(&v.to_le_bytes());
        self
    }

    /// Appends raw bytes.
    pub fn bytes(&mut self, v: &[u8]) -> &mut Self {
        self.buf.extend_from_slice(v);
        self
    }

    /// Appends a string into a fixed-size NUL-padded field (truncating).
    pub fn fixed_str(&mut self, s: &str, len: usize) -> &mut Self {
        let mut b = string_to_bytes(s);
        b.resize(len, 0);
        self.buf.extend_from_slice(&b);
        self
    }

    /// Overwrites a `u16` at an absolute offset.
    pub fn patch_u16(&mut self, at: usize, v: u16) {
        self.buf[at..at + 2].copy_from_slice(&v.to_le_bytes());
    }

    /// Returns the written bytes.
    pub fn into_inner(self) -> Vec<u8> {
        self.buf
    }

    /// Borrows the written bytes.
    pub fn as_slice(&self) -> &[u8] {
        &self.buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reader_reports_eof_instead_of_panicking() {
        let mut r = Reader::new(&[1, 2, 3], "test");
        assert_eq!(r.u16().unwrap(), 0x0201);
        assert!(matches!(
            r.u16(),
            Err(FormatError::UnexpectedEof {
                needed: 2,
                offset: 2,
                ..
            })
        ));
        assert_eq!(r.u8().unwrap(), 3);
        assert!(r.at_end());
    }

    #[test]
    fn strings_round_trip_8bit() {
        let raw = [b'A', 0x82, b'z', 0, b'x'];
        let s = cstr_to_string(&raw);
        assert_eq!(s.chars().count(), 3);
        assert_eq!(string_to_bytes(&s), vec![b'A', 0x82, b'z']);
    }
}

// SPDX-License-Identifier: GPL-3.0-or-later
//! The game font (`fnt`).
//!
//! Layout used by the English, German, French and Spanish releases (from
//! ScummVM `FontSurface`, engines/mm/xeen/font.cpp, GPL-3.0-or-later):
//!
//! ```text
//! 0x0000  128 glyphs * 8 rows * u16   normal font
//! 0x0800  128 glyphs * 8 rows * u16   reduced ("small") font
//! 0x1000  128 * u8                    normal widths
//! 0x1080  128 * u8                    reduced widths
//! ```
//!
//! Each row packs 8 pixels of 2 bits, least significant pair first:
//! 0 = transparent, 1..=3 = the current text colour set's shades.
//! The Russian release doubles the tables for Cyrillic; see [`FontLayout`].

use crate::error::{FormatError, Result};

/// Glyph height in rows.
pub const GLYPH_HEIGHT: usize = 8;
/// Maximum glyph width in pixels.
pub const GLYPH_WIDTH: usize = 8;

/// Where the glyph and width tables live in the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FontLayout {
    /// Offset of the normal glyphs.
    pub normal: usize,
    /// Offset of the reduced glyphs.
    pub reduced: usize,
    /// Offset of the normal widths.
    pub normal_widths: usize,
    /// Offset of the reduced widths.
    pub reduced_widths: usize,
}

impl FontLayout {
    /// Layout of the western-language releases.
    pub const WESTERN: FontLayout = FontLayout {
        normal: 0x0000,
        reduced: 0x0800,
        normal_widths: 0x1000,
        reduced_widths: 0x1080,
    };
    /// Latin half of the Russian release (Cyrillic tables sit in between).
    pub const RUSSIAN_LATIN: FontLayout = FontLayout {
        normal: 0x0000,
        reduced: 0x1000,
        normal_widths: 0x2000,
        reduced_widths: 0x2100,
    };
    /// Cyrillic half of the Russian release (characters with bit 7 set).
    pub const RUSSIAN_CYRILLIC: FontLayout = FontLayout {
        normal: 0x0800,
        reduced: 0x1800,
        normal_widths: 0x2080,
        reduced_widths: 0x2180,
    };

    fn required_len(&self) -> usize {
        [
            self.normal + 128 * 16,
            self.reduced + 128 * 16,
            self.normal_widths + 128,
            self.reduced_widths + 128,
        ]
        .into_iter()
        .max()
        .unwrap_or(0)
    }
}

/// A loaded font.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Font {
    data: Vec<u8>,
    layout: FontLayout,
}

impl Font {
    /// Parses a font file with the given layout.
    pub fn parse(data: Vec<u8>, layout: FontLayout) -> Result<Self> {
        let need = layout.required_len();
        if data.len() < need {
            return Err(FormatError::UnexpectedEof {
                what: "font",
                offset: data.len(),
                needed: need - data.len(),
            });
        }
        Ok(Font { data, layout })
    }

    /// Parses a western-layout font.
    pub fn parse_western(data: Vec<u8>) -> Result<Self> {
        Self::parse(data, FontLayout::WESTERN)
    }

    /// The 8 packed rows of a glyph (character code & 0x7F).
    pub fn glyph_rows(&self, ch: u8, reduced: bool) -> [u16; GLYPH_HEIGHT] {
        let base = if reduced {
            self.layout.reduced
        } else {
            self.layout.normal
        } + usize::from(ch & 0x7F) * 16;
        let mut rows = [0u16; GLYPH_HEIGHT];
        for (i, r) in rows.iter_mut().enumerate() {
            *r = u16::from_le_bytes([self.data[base + i * 2], self.data[base + i * 2 + 1]]);
        }
        rows
    }

    /// Advance width of a character.
    pub fn width(&self, ch: u8, reduced: bool) -> u8 {
        let base = if reduced {
            self.layout.reduced_widths
        } else {
            self.layout.normal_widths
        };
        self.data[base + usize::from(ch & 0x7F)]
    }

    /// Shade (0..=3) of pixel `x` in row `row` of a glyph.
    pub fn shade(&self, ch: u8, reduced: bool, x: usize, row: usize) -> u8 {
        if x >= GLYPH_WIDTH || row >= GLYPH_HEIGHT {
            return 0;
        }
        ((self.glyph_rows(ch, reduced)[row] >> (2 * x)) & 3) as u8
    }

    /// Characters drawn one row lower (descenders), as the engine does.
    pub fn has_descender(ch: u8) -> bool {
        matches!(ch, b'g' | b'p' | b'q' | b'y')
    }

    /// Raw font bytes.
    pub fn data(&self) -> &[u8] {
        &self.data
    }
}

/// Builds a western-layout font file (for synthetic data and mods).
#[derive(Debug, Clone)]
pub struct FontBuilder {
    data: Vec<u8>,
}

impl Default for FontBuilder {
    fn default() -> Self {
        FontBuilder {
            data: vec![0; 0x1100],
        }
    }
}

impl FontBuilder {
    /// Creates a font with empty glyphs and zero widths.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets a glyph from 8 rows of shades (each row: up to 8 values 0..=3).
    pub fn glyph(&mut self, ch: u8, reduced: bool, rows: [[u8; 8]; 8], width: u8) -> &mut Self {
        let l = FontLayout::WESTERN;
        let base = if reduced { l.reduced } else { l.normal } + usize::from(ch & 0x7F) * 16;
        for (y, row) in rows.iter().enumerate() {
            let mut v: u16 = 0;
            for (x, &s) in row.iter().enumerate() {
                v |= u16::from(s & 3) << (2 * x);
            }
            self.data[base + y * 2..base + y * 2 + 2].copy_from_slice(&v.to_le_bytes());
        }
        let wbase = if reduced {
            l.reduced_widths
        } else {
            l.normal_widths
        };
        self.data[wbase + usize::from(ch & 0x7F)] = width;
        self
    }

    /// Returns the font file bytes.
    pub fn build(&self) -> Vec<u8> {
        self.data.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glyph_round_trip() {
        let mut rows = [[0u8; 8]; 8];
        rows[0] = [1, 2, 3, 0, 0, 0, 0, 3];
        rows[7][0] = 2;
        let mut b = FontBuilder::new();
        b.glyph(b'A', false, rows, 6).glyph(b'A', true, rows, 5);
        let f = Font::parse_western(b.build()).unwrap();
        assert_eq!(f.width(b'A', false), 6);
        assert_eq!(f.width(b'A', true), 5);
        assert_eq!(f.shade(b'A', false, 0, 0), 1);
        assert_eq!(f.shade(b'A', false, 2, 0), 3);
        assert_eq!(f.shade(b'A', false, 7, 0), 3);
        assert_eq!(f.shade(b'A', false, 0, 7), 2);
        // Bit 7 selects the same glyph (inverse video is a rendering matter).
        assert_eq!(f.glyph_rows(b'A' | 0x80, false), f.glyph_rows(b'A', false));
    }

    #[test]
    fn short_font_is_rejected() {
        assert!(Font::parse_western(vec![0; 100]).is_err());
    }
}

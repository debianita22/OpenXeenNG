// SPDX-License-Identifier: GPL-3.0-or-later
//! Small fixed-layout map resources: head danger data (`.hed`), object
//! animation tables and map names.
//!
//! Layouts ported from ScummVM (engines/mm/xeen/map.cpp, GPL-3.0-or-later).

use crate::error::Result;
use crate::io::{Reader, Writer, cstr_to_string};
use crate::maze::MAP_SIZE;

/// "Danger sense" data of a map (`aaze0xxx.hed`, 512 bytes): per cell, the
/// animation levels of the left and right gargoyle heads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeadData {
    /// `[y][x]` → `(left, right)`.
    pub cells: [[(u8, u8); MAP_SIZE]; MAP_SIZE],
}

impl HeadData {
    /// Parses a `.hed` file.
    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut r = Reader::new(data, "head data");
        let mut cells = [[(0u8, 0u8); MAP_SIZE]; MAP_SIZE];
        for row in cells.iter_mut() {
            for c in row.iter_mut() {
                *c = (r.u8()?, r.u8()?);
            }
        }
        Ok(HeadData { cells })
    }

    /// Serialises back to 512 bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut w = Writer::new();
        for row in &self.cells {
            for &(l, r) in row {
                w.u8(l).u8(r);
            }
        }
        w.into_inner()
    }
}

/// One object animation entry (12 bytes): frame per viewing side, flip flags,
/// and the second animation frame per side. Sides are front, left, back, right.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AnimationEntry {
    /// First frame per side.
    pub frame1: [u8; 4],
    /// Horizontal flip per side.
    pub flipped: [bool; 4],
    /// Second frame per side.
    pub frame2: [u8; 4],
}

/// Parses an object animation table (`clouds.dat`, `dark.dat`, `animinfo.cld`).
pub fn parse_animation_info(data: &[u8]) -> Result<Vec<AnimationEntry>> {
    let mut r = Reader::new(data, "animation info");
    let mut out = Vec::new();
    // The engine reads whole entries while bytes remain; a trailing partial
    // entry would read garbage there, here it is ignored.
    while r.remaining() >= 12 {
        let mut e = AnimationEntry::default();
        for v in e.frame1.iter_mut() {
            *v = r.u8()?;
        }
        for v in e.flipped.iter_mut() {
            *v = r.u8()? != 0;
        }
        for v in e.frame2.iter_mut() {
            *v = r.u8()?;
        }
        out.push(e);
    }
    Ok(out)
}

/// Serialises an animation table.
pub fn animation_info_to_bytes(entries: &[AnimationEntry]) -> Vec<u8> {
    let mut w = Writer::new();
    for e in entries {
        w.bytes(&e.frame1);
        for &f in &e.flipped {
            w.u8(u8::from(f));
        }
        w.bytes(&e.frame2);
    }
    w.into_inner()
}

/// Reads a map name file (`xeen0xxx.txt` / `dark0xxx.txt`): a 33-byte
/// NUL-padded string of which the engine uses the first 32 characters.
pub fn parse_maze_name(data: &[u8]) -> String {
    let n = data.len().min(32);
    cstr_to_string(&data[..n])
}

/// Splits a NUL-separated string table (`aaze0xxx.txt`, event texts).
///
/// Mirrors `File::readString` in a loop: every string ends at a NUL or at the
/// end of the data; a trailing NUL does not create an extra empty string.
pub fn parse_string_table(data: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut pos = 0;
    while pos < data.len() {
        let end = data[pos..]
            .iter()
            .position(|&b| b == 0)
            .map_or(data.len(), |p| pos + p);
        out.push(data[pos..end].iter().map(|&b| b as char).collect());
        pos = end + 1;
    }
    out
}

/// Builds a NUL-separated string table.
pub fn string_table_to_bytes(strings: &[String]) -> Vec<u8> {
    let mut out = Vec::new();
    for s in strings {
        out.extend(crate::io::string_to_bytes(s));
        out.push(0);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn head_data_round_trip() {
        let bytes: Vec<u8> = (0..512).map(|i| (i % 7) as u8).collect();
        let h = HeadData::parse(&bytes).unwrap();
        assert_eq!(h.cells[0][1], (2, 3));
        assert_eq!(h.to_bytes(), bytes);
        assert!(HeadData::parse(&bytes[..511]).is_err());
    }

    #[test]
    fn animation_round_trip() {
        let e = AnimationEntry {
            frame1: [1, 2, 3, 4],
            flipped: [false, true, false, true],
            frame2: [5, 6, 7, 8],
        };
        let bytes = animation_info_to_bytes(&[e, e]);
        assert_eq!(bytes.len(), 24);
        assert_eq!(parse_animation_info(&bytes).unwrap(), vec![e, e]);
    }

    #[test]
    fn strings() {
        let t = parse_string_table(b"Where to?\0Isle of ScummVM\0");
        assert_eq!(
            t,
            vec!["Where to?".to_string(), "Isle of ScummVM".to_string()]
        );
        assert_eq!(parse_string_table(b"a\0\0b"), vec!["a", "", "b"]);
        assert_eq!(
            string_table_to_bytes(&t),
            b"Where to?\0Isle of ScummVM\0".to_vec()
        );
        let mut name = b"ScummVM".to_vec();
        name.resize(33, 0);
        assert_eq!(parse_maze_name(&name), "ScummVM");
    }
}

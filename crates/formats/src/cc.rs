// SPDX-License-Identifier: GPL-3.0-or-later
//! CC archives (`XEEN.CC`, `DARK.CC`, `INTRO.CC`, `SWRD.CC`, save files).
//!
//! Layout (ported from ScummVM `BaseCCArchive`, GPL-3.0-or-later):
//!
//! ```text
//! u16          entry count N
//! N * 8 bytes  index, obfuscated with a rolling key:
//!                plain = rol2(byte) + seed;  seed starts at 0xAC, += 0x67 per byte
//!              each plain entry: u16 id, u24 offset, u16 size, u8 zero
//! ...          entry data at absolute offsets; in the game archives every
//!              data byte is XOR-ed with 0x35 ("encoded" archives)
//! ```
//!
//! Entry sizes are 16-bit, so no single resource exceeds 65535 bytes.

use crate::error::{FormatError, Result};
use crate::io::{Reader, Writer};
use crate::names::name_to_id;

/// Key used for the data XOR of encoded archives.
pub const DATA_XOR: u8 = 0x35;
const INDEX_SEED: u8 = 0xAC;
const INDEX_SEED_STEP: u8 = 0x67;

/// One entry of the archive index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CcEntry {
    /// Name hash (see [`name_to_id`]).
    pub id: u16,
    /// Absolute byte offset of the data (24-bit in the file).
    pub offset: u32,
    /// Data size in bytes.
    pub size: u16,
}

/// Decrypts an index block in place.
pub fn decrypt_index(raw: &mut [u8]) {
    let mut seed = INDEX_SEED;
    for b in raw.iter_mut() {
        *b = b.rotate_left(2).wrapping_add(seed);
        seed = seed.wrapping_add(INDEX_SEED_STEP);
    }
}

/// Encrypts an index block in place (inverse of [`decrypt_index`]).
pub fn encrypt_index(raw: &mut [u8]) {
    let mut seed = INDEX_SEED;
    for b in raw.iter_mut() {
        *b = b.wrapping_sub(seed).rotate_right(2);
        seed = seed.wrapping_add(INDEX_SEED_STEP);
    }
}

/// Parses the index at the start of `data`.
///
/// Returns the entries and the number of index bytes consumed. A non-zero
/// padding byte is tolerated (ScummVM asserts on it) because it carries no
/// information.
pub fn parse_index(data: &[u8]) -> Result<(Vec<CcEntry>, usize)> {
    let mut r = Reader::new(data, "CC index");
    let count = r.u16()? as usize;
    let mut raw = r.bytes(count * 8)?.to_vec();
    decrypt_index(&mut raw);
    let entries = raw
        .chunks_exact(8)
        .map(|e| CcEntry {
            id: u16::from_le_bytes([e[0], e[1]]),
            offset: u32::from_le_bytes([e[2], e[3], e[4], 0]),
            size: u16::from_le_bytes([e[5], e[6]]),
        })
        .collect();
    Ok((entries, 2 + count * 8))
}

/// An archive held in memory.
#[derive(Debug, Clone)]
pub struct CcArchive {
    entries: Vec<CcEntry>,
    data: Vec<u8>,
    encoded: bool,
}

impl CcArchive {
    /// Parses a complete archive. `encoded` selects the 0x35 data XOR used by
    /// the game archives (save archives are not encoded).
    ///
    /// Every entry is bounds-checked against the data length.
    pub fn parse(data: Vec<u8>, encoded: bool) -> Result<Self> {
        let (entries, _) = parse_index(&data)?;
        for e in &entries {
            let end = e.offset as usize + e.size as usize;
            if end > data.len() {
                return Err(FormatError::invalid(
                    "CC index",
                    format!(
                        "entry {:04X} spans {}..{} but the archive is {} bytes",
                        e.id,
                        e.offset,
                        end,
                        data.len()
                    ),
                ));
            }
        }
        Ok(CcArchive {
            entries,
            data,
            encoded,
        })
    }

    /// Index entries in file order.
    pub fn entries(&self) -> &[CcEntry] {
        &self.entries
    }

    /// Whether data is XOR-encoded.
    pub fn is_encoded(&self) -> bool {
        self.encoded
    }

    /// Finds the entry for an id (first match, as the engine does).
    pub fn entry(&self, id: u16) -> Option<&CcEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// True when a resource with this name exists.
    pub fn contains(&self, name: &str) -> bool {
        self.entry(name_to_id(name)).is_some()
    }

    /// Returns the decoded bytes of a resource by id.
    pub fn read_id(&self, id: u16) -> Option<Vec<u8>> {
        let e = self.entry(id)?;
        let start = e.offset as usize;
        let mut out = self.data[start..start + e.size as usize].to_vec();
        if self.encoded {
            for b in &mut out {
                *b ^= DATA_XOR;
            }
        }
        Some(out)
    }

    /// Returns the decoded bytes of a resource by name.
    pub fn read(&self, name: &str) -> Option<Vec<u8>> {
        self.read_id(name_to_id(name))
    }
}

/// Builds a CC archive (for tools, tests, mods and synthetic data).
#[derive(Debug, Clone, Default)]
pub struct CcBuilder {
    files: Vec<(u16, Vec<u8>)>,
}

impl CcBuilder {
    /// Creates an empty builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds (or replaces) a named resource.
    pub fn add(&mut self, name: &str, data: Vec<u8>) -> Result<&mut Self> {
        self.add_id(name_to_id(name), data)
    }

    /// Adds (or replaces) a resource by id.
    pub fn add_id(&mut self, id: u16, data: Vec<u8>) -> Result<&mut Self> {
        if data.len() > u16::MAX as usize {
            return Err(FormatError::invalid(
                "CC entry",
                format!(
                    "resource {id:04X} is {} bytes; the limit is 65535",
                    data.len()
                ),
            ));
        }
        if let Some(slot) = self.files.iter_mut().find(|(i, _)| *i == id) {
            slot.1 = data;
        } else {
            self.files.push((id, data));
        }
        Ok(self)
    }

    /// Number of resources added.
    pub fn len(&self) -> usize {
        self.files.len()
    }

    /// True when empty.
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// Serialises the archive. Data follows the index in insertion order.
    pub fn build(&self, encoded: bool) -> Result<Vec<u8>> {
        let count = self.files.len();
        if count > u16::MAX as usize {
            return Err(FormatError::invalid("CC index", "too many entries"));
        }
        let header = 2 + count * 8;
        let total: usize = header + self.files.iter().map(|(_, d)| d.len()).sum::<usize>();
        if total > 0x00FF_FFFF {
            return Err(FormatError::invalid(
                "CC archive",
                "offsets are 24-bit; archive would exceed 16 MiB",
            ));
        }
        let mut index = Vec::with_capacity(count * 8);
        let mut offset = header as u32;
        for (id, data) in &self.files {
            index.extend_from_slice(&id.to_le_bytes());
            index.extend_from_slice(&offset.to_le_bytes()[..3]);
            index.extend_from_slice(&(data.len() as u16).to_le_bytes());
            index.push(0);
            offset += data.len() as u32;
        }
        encrypt_index(&mut index);
        let mut w = Writer::new();
        w.u16(count as u16).bytes(&index);
        for (_, data) in &self.files {
            if encoded {
                let enc: Vec<u8> = data.iter().map(|b| b ^ DATA_XOR).collect();
                w.bytes(&enc);
            } else {
                w.bytes(data);
            }
        }
        Ok(w.into_inner())
    }
}

/// Rebuilds a side's initial save archive (party roster, flags, map state)
/// from the six pieces stored inside `XEEN.CC`/`DARK.CC`.
///
/// Mirrors ScummVM `SaveArchive::reset`: the pieces are concatenated in
/// [`crate::tables::INITIAL_SAVE_PIECES`] order (missing ones skipped) and the
/// result is parsed as an unencoded archive.
pub fn initial_save_archive(cc: &CcArchive) -> Result<CcArchive> {
    let mut joined = Vec::new();
    for id in crate::tables::INITIAL_SAVE_PIECES {
        if let Some(piece) = cc.read_id(id) {
            joined.extend_from_slice(&piece);
        }
    }
    if joined.is_empty() {
        return Err(FormatError::NotFound("initial save archive pieces".into()));
    }
    CcArchive::parse(joined, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_encryption_round_trips() {
        let plain: Vec<u8> = (0..=255u8).chain(0..=255u8).collect();
        let mut buf = plain.clone();
        encrypt_index(&mut buf);
        assert_ne!(buf, plain);
        decrypt_index(&mut buf);
        assert_eq!(buf, plain);
    }

    #[test]
    fn decrypt_matches_scummvm_formula() {
        // ScummVM: rawIndex[i] = (((raw << 2) | (raw >> 6)) + seed) & 0xff
        let raw = [0x00u8, 0x01, 0x80, 0xFF, 0x3C];
        let mut buf = raw;
        decrypt_index(&mut buf);
        let mut seed: i32 = 0xAC;
        for (i, &b) in raw.iter().enumerate() {
            let b = b as i32;
            let expect = ((((b << 2) | (b >> 6)) + seed) & 0xFF) as u8;
            assert_eq!(buf[i], expect);
            seed += 0x67;
        }
    }

    #[test]
    fn build_and_parse_archive() {
        let mut b = CcBuilder::new();
        b.add("mm4.pal", vec![1, 2, 3]).unwrap();
        b.add("fnt", vec![9; 300]).unwrap();
        b.add("mm4.pal", vec![4, 5, 6, 7]).unwrap(); // replace
        for encoded in [true, false] {
            let bytes = b.build(encoded).unwrap();
            let cc = CcArchive::parse(bytes, encoded).unwrap();
            assert_eq!(cc.entries().len(), 2);
            assert_eq!(cc.read("MM4.PAL").unwrap(), vec![4, 5, 6, 7]);
            assert_eq!(cc.read("fnt").unwrap().len(), 300);
            assert!(cc.read("nothere.dat").is_none());
            assert!(cc.contains("fnt"));
        }
    }

    #[test]
    fn encoded_data_is_xored() {
        let mut b = CcBuilder::new();
        b.add("x", vec![0x00, 0x35]).unwrap();
        let bytes = b.build(true).unwrap();
        assert_eq!(&bytes[10..], &[0x35, 0x00]);
    }

    #[test]
    fn rejects_out_of_range_entries_and_oversized_resources() {
        let mut b = CcBuilder::new();
        b.add("a", vec![1; 10]).unwrap();
        let mut bytes = b.build(false).unwrap();
        bytes.truncate(bytes.len() - 1);
        assert!(CcArchive::parse(bytes, false).is_err());
        assert!(CcArchive::parse(vec![5], false).is_err());
        assert!(CcBuilder::new().add("big", vec![0; 70_000]).is_err());
    }

    #[test]
    fn initial_save_is_reassembled_from_pieces() {
        // Build an inner (unencoded) save archive, split it across the pieces.
        let mut inner = CcBuilder::new();
        inner.add("maze.chr", vec![7; 100]).unwrap();
        inner.add("maze.pty", vec![8; 50]).unwrap();
        let inner = inner.build(false).unwrap();
        let (a, b) = inner.split_at(60);
        let mut outer = CcBuilder::new();
        outer.add_id(0x2A0C, a.to_vec()).unwrap();
        outer.add_id(0x2A1C, b.to_vec()).unwrap();
        let outer = CcArchive::parse(outer.build(true).unwrap(), true).unwrap();
        let save = initial_save_archive(&outer).unwrap();
        assert_eq!(save.read("maze.chr").unwrap(), vec![7; 100]);
        assert_eq!(save.read("maze.pty").unwrap(), vec![8; 50]);
    }
}

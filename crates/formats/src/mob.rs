// SPDX-License-Identifier: GPL-3.0-or-later
//! Monster/object placement files (`maze0xxx.mob`).
//!
//! Ported from ScummVM `MonsterObjectData::synchronize`
//! (engines/mm/xeen/map.cpp, GPL-3.0-or-later):
//!
//! ```text
//! 16 * u8  object sprite ids       (0xFF = unused)
//! 16 * u8  monster sprite ids      (0xFF = unused)
//! 16 * u8  wall item sprite ids    (0xFF = unused)
//! records of 4 bytes (i8 x, i8 y, u8 id, u8 direction), three lists,
//! each terminated by an end marker (x = -1, id = 0xFF):
//!   objects, monsters, wall items
//! ```
//!
//! The reading rules below replicate the engine exactly, including its
//! quirks: an object or monster list that *starts* with an end marker is
//! followed by one more record that is read before the list proper, and a
//! file that ends early simply truncates the remaining lists.

use crate::error::Result;
use crate::io::{Reader, Writer};

/// Id value of an end-of-list record.
pub const END_ID: u8 = 0xFF;

/// One placement record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MobRecord {
    /// Cell x (signed; objects removed from play sit at -128).
    pub x: i8,
    /// Cell y.
    pub y: i8,
    /// Index into the matching sprite id table (0xFF = end marker).
    pub id: u8,
    /// Facing direction (0..=3; 0xFF in end markers).
    pub direction: u8,
}

impl MobRecord {
    /// The end-of-list marker the engine writes.
    pub const END: MobRecord = MobRecord {
        x: -1,
        y: -1,
        id: END_ID,
        direction: 0xFF,
    };

    /// True for an end-of-list record (the engine checks `id == -1` and `x == -1`).
    pub fn is_end(&self) -> bool {
        self.id == END_ID && self.x == -1
    }

    /// True for objects that were removed from the map (position ±128).
    pub fn is_removed(&self) -> bool {
        self.x == i8::MIN || self.y == i8::MIN
    }

    fn read(r: &mut Reader<'_>) -> Result<MobRecord> {
        Ok(MobRecord {
            x: r.i8()?,
            y: r.i8()?,
            id: r.u8()?,
            direction: r.u8()?,
        })
    }

    fn write(&self, w: &mut Writer) {
        w.i8(self.x).i8(self.y).u8(self.id).u8(self.direction);
    }
}

/// Parsed contents of a `.mob` file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MobFile {
    /// Object sprite ids (0xFF = unused slot).
    pub object_sprites: [u8; 16],
    /// Monster ids (index into the monster table; 0xFF = unused slot).
    pub monster_sprites: [u8; 16],
    /// Wall item sprite ids (0xFF = unused slot).
    pub wall_item_sprites: [u8; 16],
    /// Object placements.
    pub objects: Vec<MobRecord>,
    /// Monster placements (may contain blank `(0, 0, id 0)` entries, which the
    /// engine ignores when no monster sprites are defined).
    pub monsters: Vec<MobRecord>,
    /// Wall item placements.
    pub wall_items: Vec<MobRecord>,
}

impl MobFile {
    /// Parses a `.mob` file with the engine's reading rules.
    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut r = Reader::new(data, "mob file");
        let mut m = MobFile::default();
        for v in m.object_sprites.iter_mut() {
            *v = r.u8()?;
        }
        for v in m.monster_sprites.iter_mut() {
            *v = r.u8()?;
        }
        for v in m.wall_item_sprites.iter_mut() {
            *v = r.u8()?;
        }

        // Objects: do/while list, first record re-read if it is an end marker.
        let mut rec = match read_opt(&mut r)? {
            Some(v) => v,
            None => return Ok(m),
        };
        if rec.is_end() {
            rec = match read_opt(&mut r)? {
                Some(v) => v,
                None => return Ok(m),
            };
        }
        loop {
            m.objects.push(rec);
            rec = match read_opt(&mut r)? {
                Some(v) => v,
                None => return Ok(m),
            };
            if r.at_end() {
                // ScummVM: `if (s.finished()) return;` right after the read.
                return Ok(m);
            }
            if rec.is_end() {
                break;
            }
        }

        // Monsters: while list, same leading end-marker rule.
        rec = match read_opt(&mut r)? {
            Some(v) => v,
            None => return Ok(m),
        };
        if rec.is_end() {
            rec = match read_opt(&mut r)? {
                Some(v) => v,
                None => return Ok(m),
            };
        }
        while !rec.is_end() {
            if r.at_end() {
                return Ok(m);
            }
            m.monsters.push(rec);
            rec = match read_opt(&mut r)? {
                Some(v) => v,
                None => return Ok(m),
            };
        }

        // Wall items: plain while list, no dummy entry.
        rec = match read_opt(&mut r)? {
            Some(v) => v,
            None => return Ok(m),
        };
        while !rec.is_end() {
            if r.at_end() {
                return Ok(m);
            }
            m.wall_items.push(rec);
            rec = match read_opt(&mut r)? {
                Some(v) => v,
                None => return Ok(m),
            };
        }
        Ok(m)
    }

    /// Serialises with the engine's writing rules (`isSaving` branch).
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut w = Writer::new();
        w.bytes(&self.object_sprites)
            .bytes(&self.monster_sprites)
            .bytes(&self.wall_item_sprites);
        if self.objects.is_empty() {
            MobRecord::END.write(&mut w);
        } else {
            for o in &self.objects {
                o.write(&mut w);
            }
        }
        MobRecord::END.write(&mut w);
        if self.monsters.is_empty() {
            MobRecord::END.write(&mut w);
        } else {
            for m in &self.monsters {
                MobRecord { direction: 0, ..*m }.write(&mut w);
            }
        }
        MobRecord::END.write(&mut w);
        for i in &self.wall_items {
            i.write(&mut w);
        }
        MobRecord::END.write(&mut w);
        w.into_inner()
    }

    /// Used object sprite slots `(slot, sprite id)`, like the engine builds
    /// them (trailing unused slots dropped, gaps kept as `None`).
    pub fn object_sprite_slots(&self) -> Vec<Option<u8>> {
        let last = self.object_sprites.iter().rposition(|&v| v != 0xFF);
        match last {
            Some(last) => self.object_sprites[..=last]
                .iter()
                .map(|&v| (v != 0xFF).then_some(v))
                .collect(),
            None => vec![None],
        }
    }
}

/// Reads a record, or `None` once fewer than 4 bytes remain (the engine stops
/// reading at the end of the stream instead of failing).
fn read_opt(r: &mut Reader<'_>) -> Result<Option<MobRecord>> {
    if r.remaining() < 4 {
        return Ok(None);
    }
    MobRecord::read(r).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(x: i8, y: i8, id: u8, d: u8) -> MobRecord {
        MobRecord {
            x,
            y,
            id,
            direction: d,
        }
    }

    /// Byte layout of ScummVM's own `mazex255.mob` (devtools/create_mm).
    fn scummvm_isle_mob() -> Vec<u8> {
        let mut v = vec![8, 2];
        v.extend([0xFF; 14]);
        v.extend([0xFF; 32]);
        v.extend([
            0x07, 0x00, 0x00, 0x02, 0x07, 0x08, 0x01, 0x00, 0x08, 0x08, 0x01, 0x00, 0xFF, 0xFF,
            0xFF, 0xFF, 0x00, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0x80, 0x80, 0x00, 0x00,
            0xFF, 0xFF, 0xFF, 0xFF,
        ]);
        v
    }

    #[test]
    fn parses_scummvm_isle_layout() {
        let m = MobFile::parse(&scummvm_isle_mob()).unwrap();
        assert_eq!(
            m.objects,
            vec![rec(7, 0, 0, 2), rec(7, 8, 1, 0), rec(8, 8, 1, 0)]
        );
        // The "blank entry" of an empty monster list.
        assert_eq!(m.monsters, vec![rec(0, 0, 0, 0)]);
        assert_eq!(m.wall_items, vec![rec(-128, -128, 0, 0)]);
        assert!(m.wall_items[0].is_removed());
        assert_eq!(m.object_sprite_slots(), vec![Some(8), Some(2)]);
    }

    #[test]
    fn write_then_read_non_empty_lists() {
        let mut m = MobFile {
            object_sprites: [0xFF; 16],
            monster_sprites: [0xFF; 16],
            wall_item_sprites: [0xFF; 16],
            ..Default::default()
        };
        m.object_sprites[0] = 3;
        m.monster_sprites[0] = 10;
        m.objects = vec![rec(1, 2, 0, 1), rec(3, 4, 0, 2)];
        m.monsters = vec![rec(5, 6, 0, 0)];
        m.wall_items = vec![rec(7, 8, 0, 3)];
        let back = MobFile::parse(&m.to_bytes()).unwrap();
        assert_eq!(back, m);
    }

    #[test]
    fn truncated_file_stops_early_without_error() {
        let mut bytes = scummvm_isle_mob();
        bytes.truncate(48 + 8);
        let m = MobFile::parse(&bytes).unwrap();
        assert_eq!(m.objects.len(), 1);
        assert!(MobFile::parse(&bytes[..10]).is_err());
    }
}

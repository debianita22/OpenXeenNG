// SPDX-License-Identifier: GPL-3.0-or-later
//! Map data files (`maze0xxx.dat` / `mazexxxx.dat`, 892 bytes each).
//!
//! Field order ported from ScummVM `MazeData::synchronize`
//! (engines/mm/xeen/map.cpp, GPL-3.0-or-later).

use crate::error::Result;
use crate::io::{Reader, Writer};

/// Maps are 16x16 cells.
pub const MAP_SIZE: usize = 16;
/// Size of a `.dat` file.
pub const MAZE_DAT_BYTES: usize = 892;

/// Compass direction, in the engine's numbering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Direction {
    /// North (0).
    North = 0,
    /// East (1).
    East = 1,
    /// South (2).
    South = 2,
    /// West (3).
    West = 3,
}

impl Direction {
    /// All four directions in engine order.
    pub const ALL: [Direction; 4] = [
        Direction::North,
        Direction::East,
        Direction::South,
        Direction::West,
    ];

    /// Converts from the engine value (masked to 0..=3).
    pub fn from_u8(v: u8) -> Direction {
        Direction::ALL[usize::from(v & 3)]
    }

    /// Direction after turning right.
    pub fn right(self) -> Direction {
        Direction::from_u8(self as u8 + 1)
    }

    /// Direction after turning left.
    pub fn left(self) -> Direction {
        Direction::from_u8(self as u8 + 3)
    }

    /// Opposite direction.
    pub fn opposite(self) -> Direction {
        Direction::from_u8(self as u8 + 2)
    }

    /// Unit step `(dx, dy)`; y grows towards the north, as in the engine.
    pub fn delta(self) -> (i32, i32) {
        match self {
            Direction::North => (0, 1),
            Direction::East => (1, 0),
            Direction::South => (0, -1),
            Direction::West => (-1, 0),
        }
    }
}

/// Maze flags (`_mazeFlags`).
pub mod flags {
    /// Stepping on the cell runs its event automatically.
    pub const AUTOEXECUTE_EVENT: u16 = 0x10;
    /// Etherealize is blocked.
    pub const RESTRICTION_ETHERIALIZE: u16 = 0x40;
    /// Town Portal is blocked.
    pub const RESTRICTION_TOWN_PORTAL: u16 = 0x100;
    /// Super Shelter is blocked.
    pub const RESTRICTION_SUPER_SHELTER: u16 = 0x200;
    /// Time Distortion is blocked.
    pub const RESTRICTION_TIME_DISTORTION: u16 = 0x400;
    /// Lloyd's Beacon is blocked.
    pub const RESTRICTION_LLOYDS_BEACON: u16 = 0x800;
    /// Teleport is blocked.
    pub const RESTRICTION_TELEPORT: u16 = 0x1000;
    /// Resting is blocked.
    pub const RESTRICTION_REST: u16 = 0x4000;
    /// Saving is blocked.
    pub const RESTRICTION_SAVE: u16 = 0x8000;
    /// `_mazeFlags2`: the map is outdoors.
    pub const IS_OUTDOORS: u16 = 0x8000;
    /// `_mazeFlags2`: the map is dark (needs light).
    pub const IS_DARK: u16 = 0x4000;
}

/// Per-cell flags stored in the upper five bits of the cell byte.
pub mod cell_flags {
    /// Outdoors: a grate.
    pub const OUT_GRATE: u8 = 0x80;
    /// Outdoors: a drain.
    pub const OUT_DRAIN: u8 = 0x20;
    /// Outdoors: an object exists here.
    pub const OUT_OBJECT_EXISTS: u8 = 0x08;
    /// Indoors: the cell is inside a building.
    pub const IN_INSIDE: u8 = 0x08;
    /// Stepping here runs the cell's event.
    pub const AUTOEXECUTE_EVENT: u8 = 0x10;
}

/// Difficulty values for actions on the map.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MazeDifficulties {
    /// Wall that can't be passed.
    pub wall_no_pass: u8,
    /// Surface that can't be passed.
    pub surface_no_pass: u8,
    /// Unlock door difficulty.
    pub unlock_door: u8,
    /// Unlock box difficulty.
    pub unlock_box: u8,
    /// Bash door difficulty.
    pub bash_door: u8,
    /// Bash grate difficulty (signed).
    pub bash_grate: i8,
    /// Bash wall difficulty (signed).
    pub bash_wall: i8,
    /// Chance to run from combat (signed).
    pub chance_to_run: i8,
}

/// One cell of the surface/flag layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MazeCell {
    /// Surface id (low 3 bits).
    pub surface_id: u8,
    /// Flags (upper 5 bits, see [`cell_flags`]).
    pub flags: u8,
}

/// Contents of a `.dat` map file. Arrays are indexed `[y][x]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MazeData {
    /// Packed wall/layer nibbles (see [`MazeData::indoor_wall`] and
    /// [`MazeData::outdoor_layers`]).
    pub walls: [[u16; MAP_SIZE]; MAP_SIZE],
    /// Surface ids and cell flags.
    pub cells: [[MazeCell; MAP_SIZE]; MAP_SIZE],
    /// The map's own number.
    pub maze_number: u16,
    /// Neighbouring maps: north, east, south, west (0 = none).
    pub surrounding: [u16; 4],
    /// `_mazeFlags` (see [`flags`]).
    pub maze_flags: u16,
    /// `_mazeFlags2` (see [`flags::IS_OUTDOORS`]).
    pub maze_flags2: u16,
    /// Wall sprite families used by the map.
    pub wall_types: [u8; 16],
    /// Surface types used by the map.
    pub surface_types: [u8; 16],
    /// Floor type.
    pub floor_type: u8,
    /// Where the party lands after running from combat.
    pub run_position: (u8, u8),
    /// Action difficulties.
    pub difficulties: MazeDifficulties,
    /// Damage dealt by traps.
    pub trap_damage: u8,
    /// Terrain set (index into [`crate::tables::TERRAIN_TYPES`] indoors).
    pub wall_kind: u8,
    /// Tavern tip set.
    pub tavern_tips: u8,
    /// Automap: cells seen.
    pub seen: [[bool; MAP_SIZE]; MAP_SIZE],
    /// Cells stepped on.
    pub stepped_on: [[bool; MAP_SIZE]; MAP_SIZE],
}

impl Default for MazeData {
    fn default() -> Self {
        MazeData {
            walls: [[0; MAP_SIZE]; MAP_SIZE],
            cells: [[MazeCell::default(); MAP_SIZE]; MAP_SIZE],
            maze_number: 0,
            surrounding: [0; 4],
            maze_flags: 0,
            maze_flags2: 0,
            wall_types: [0; 16],
            surface_types: [0; 16],
            floor_type: 0,
            run_position: (0, 0),
            difficulties: MazeDifficulties::default(),
            trap_damage: 0,
            wall_kind: 0,
            tavern_tips: 0,
            seen: [[false; MAP_SIZE]; MAP_SIZE],
            stepped_on: [[false; MAP_SIZE]; MAP_SIZE],
        }
    }
}

impl MazeData {
    /// Parses a `.dat` file.
    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut r = Reader::new(data, "maze data");
        let mut m = MazeData::default();
        for row in m.walls.iter_mut() {
            for w in row.iter_mut() {
                *w = r.u16()?;
            }
        }
        for row in m.cells.iter_mut() {
            for c in row.iter_mut() {
                let b = r.u8()?;
                *c = MazeCell {
                    surface_id: b & 7,
                    flags: b & 0xF8,
                };
            }
        }
        m.maze_number = r.u16()?;
        for s in m.surrounding.iter_mut() {
            *s = r.u16()?;
        }
        m.maze_flags = r.u16()?;
        m.maze_flags2 = r.u16()?;
        for v in m.wall_types.iter_mut() {
            *v = r.u8()?;
        }
        for v in m.surface_types.iter_mut() {
            *v = r.u8()?;
        }
        m.floor_type = r.u8()?;
        let run_x = r.u8()?;
        m.difficulties = MazeDifficulties {
            wall_no_pass: r.u8()?,
            surface_no_pass: r.u8()?,
            unlock_door: r.u8()?,
            unlock_box: r.u8()?,
            bash_door: r.u8()?,
            bash_grate: r.i8()?,
            bash_wall: r.i8()?,
            chance_to_run: r.i8()?,
        };
        let run_y = r.u8()?;
        m.run_position = (run_x, run_y);
        m.trap_damage = r.u8()?;
        m.wall_kind = r.u8()?;
        m.tavern_tips = r.u8()?;
        for row in m.seen.iter_mut() {
            read_bit_row(&mut r, row)?;
        }
        for row in m.stepped_on.iter_mut() {
            read_bit_row(&mut r, row)?;
        }
        Ok(m)
    }

    /// Serialises back to the 892-byte file format.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut w = Writer::new();
        for row in &self.walls {
            for &v in row {
                w.u16(v);
            }
        }
        for row in &self.cells {
            for c in row {
                w.u8((c.surface_id & 7) | (c.flags & 0xF8));
            }
        }
        w.u16(self.maze_number);
        for &s in &self.surrounding {
            w.u16(s);
        }
        w.u16(self.maze_flags).u16(self.maze_flags2);
        w.bytes(&self.wall_types).bytes(&self.surface_types);
        w.u8(self.floor_type).u8(self.run_position.0);
        let d = &self.difficulties;
        w.u8(d.wall_no_pass)
            .u8(d.surface_no_pass)
            .u8(d.unlock_door)
            .u8(d.unlock_box)
            .u8(d.bash_door)
            .i8(d.bash_grate)
            .i8(d.bash_wall)
            .i8(d.chance_to_run);
        w.u8(self.run_position.1)
            .u8(self.trap_damage)
            .u8(self.wall_kind)
            .u8(self.tavern_tips);
        for row in &self.seen {
            write_bit_row(&mut w, row);
        }
        for row in &self.stepped_on {
            write_bit_row(&mut w, row);
        }
        w.into_inner()
    }

    /// True for outdoor maps.
    pub fn is_outdoors(&self) -> bool {
        self.maze_flags2 & flags::IS_OUTDOORS != 0
    }

    /// Indoor wall nibble on one side of a cell (bitfield order of the
    /// original structure: north in bits 0-3, east 4-7, south 8-11, west 12-15).
    pub fn indoor_wall(&self, x: usize, y: usize, dir: Direction) -> u8 {
        ((self.walls[y][x] >> (4 * dir as u16)) & 0xF) as u8
    }

    /// Outdoor layers of a cell: `(surface, middle, top, overlay)`.
    pub fn outdoor_layers(&self, x: usize, y: usize) -> (u8, u8, u8, u8) {
        let v = self.walls[y][x];
        (
            (v & 0xF) as u8,
            ((v >> 4) & 0xF) as u8,
            ((v >> 8) & 0xF) as u8,
            ((v >> 12) & 0xF) as u8,
        )
    }

    /// Engine-style lookup `(walls >> shift) & mask` (see ScummVM `mazeLookup`).
    pub fn lookup(&self, x: usize, y: usize, shift: u32, mask: u16) -> u16 {
        (self.walls[y][x] >> shift) & mask
    }
}

/// Bit flags are packed LSB-first, 8 per byte (`File::syncBitFlags`).
fn read_bit_row(r: &mut Reader<'_>, row: &mut [bool; MAP_SIZE]) -> Result<()> {
    let mut byte = 0u8;
    for (i, v) in row.iter_mut().enumerate() {
        if i % 8 == 0 {
            byte = r.u8()?;
        }
        *v = (byte >> (i % 8)) & 1 != 0;
    }
    Ok(())
}

fn write_bit_row(w: &mut Writer, row: &[bool; MAP_SIZE]) {
    for chunk in row.chunks(8) {
        let mut b = 0u8;
        for (i, &v) in chunk.iter().enumerate() {
            if v {
                b |= 1 << i;
            }
        }
        w.u8(b);
    }
}

/// File name of a map resource, e.g. `maze_file_name("maze", 5, "dat")` →
/// `maze0005.dat`, map ids >= 100 use `x` (`mazex105.dat`).
pub fn maze_file_name(prefix: &str, map_id: u16, ext: &str) -> String {
    let c = if map_id >= 100 { 'x' } else { '0' };
    format!("{prefix}{c}{map_id:03}.{ext}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> MazeData {
        let mut m = MazeData::default();
        for y in 0..MAP_SIZE {
            for x in 0..MAP_SIZE {
                m.walls[y][x] = (x as u16) << 12 | (y as u16) << 4 | 3;
                m.cells[y][x] = MazeCell {
                    surface_id: (x % 8) as u8,
                    flags: ((y % 4) as u8) << 4 | 0x08,
                };
                m.seen[y][x] = (x + y) % 3 == 0;
                m.stepped_on[y][x] = x == y;
            }
        }
        m.maze_number = 42;
        m.surrounding = [1, 2, 3, 4];
        m.maze_flags = 0x8010;
        m.maze_flags2 = flags::IS_OUTDOORS;
        m.wall_types[3] = 7;
        m.surface_types[1] = 9;
        m.floor_type = 2;
        m.run_position = (5, 6);
        m.difficulties.bash_wall = -3;
        m.difficulties.chance_to_run = 50;
        m.trap_damage = 11;
        m.wall_kind = 4;
        m.tavern_tips = 1;
        m
    }

    #[test]
    fn round_trip_is_exact_and_sized() {
        let m = sample();
        let bytes = m.to_bytes();
        assert_eq!(bytes.len(), MAZE_DAT_BYTES);
        let back = MazeData::parse(&bytes).unwrap();
        assert_eq!(back, m);
        assert_eq!(back.to_bytes(), bytes);
        assert!(back.is_outdoors());
    }

    #[test]
    fn wall_helpers() {
        let m = sample();
        assert_eq!(m.indoor_wall(9, 2, Direction::West), 9);
        assert_eq!(m.indoor_wall(9, 2, Direction::East), 2);
        assert_eq!(m.indoor_wall(9, 2, Direction::North), 3);
        assert_eq!(m.outdoor_layers(9, 2), (3, 2, 0, 9));
    }

    #[test]
    fn names_and_directions() {
        assert_eq!(maze_file_name("maze", 5, "dat"), "maze0005.dat");
        assert_eq!(maze_file_name("aaze", 105, "hed"), "aazex105.hed");
        assert_eq!(Direction::North.left(), Direction::West);
        assert_eq!(Direction::West.right(), Direction::North);
        assert_eq!(Direction::East.opposite(), Direction::West);
    }

    #[test]
    fn truncated_file_is_an_error() {
        assert!(MazeData::parse(&[0; 100]).is_err());
    }
}

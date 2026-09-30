// SPDX-License-Identifier: GPL-3.0-or-later
//! Constant tables the original executable carries (not stored in CC files).
//!
//! Values are taken from ScummVM's `devtools/create_mm/create_xeen/constants.cpp`
//! (GPL-3.0-or-later), which the ScummVM team extracted from the original
//! game executables. Only language-neutral tables live here; translated text
//! tables will be added per language as the engine needs them.

/// Indoor terrain sets, indexed by `MazeData::wall_kind`.
pub const TERRAIN_TYPES: [&str; 6] = ["town", "cave", "towr", "cstl", "dung", "scfi"];

/// Outdoor wall sprite families (`<name>.wal`), indexed by wall type.
pub const OUTDOORS_WALL_TYPES: [Option<&str>; 16] = [
    None,
    Some("mount"),
    Some("ltree"),
    Some("dtree"),
    Some("grass"),
    Some("snotree"),
    Some("dsnotree"),
    Some("snomnt"),
    Some("dedltree"),
    Some("mount"),
    Some("lavamnt"),
    Some("palm"),
    Some("dmount"),
    Some("dedltree"),
    Some("dedltree"),
    Some("dedltree"),
];

/// Ground surface sprites, indexed by surface type.
pub const SURFACE_NAMES: [&str; 16] = [
    "water.srf", "dirt.srf", "grass.srf", "snow.srf", "swamp.srf", "lava.srf", "desert.srf",
    "road.srf", "dwater.srf", "tflr.srf", "sky.srf", "croad.srf", "sewer.srf", "cloud.srf",
    "scortch.srf", "space.srf",
];

/// Clouds side music, one per environment.
pub const MUSIC_FILES1: [&str; 5] = ["outdoors.m", "town.m", "cavern.m", "dungeon.m", "castle.m"];

/// Darkside music, seven random picks per environment.
pub const MUSIC_FILES2: [[&str; 7]; 6] = [
    ["outday1.m", "outday2.m", "outday4.m", "outnght1.m", "outnght2.m", "outnght4.m", "daydesrt.m"],
    ["townday1.m", "twnwlk.m", "newbrigh.m", "twnnitea.m", "twnniteb.m", "twnwlk.m", "townday1.m"],
    ["cavern1.m", "cavern2.m", "cavern3a.m", "cavern1.m", "cavern2.m", "cavern3a.m", "cavern1.m"],
    ["dngon1.m", "dngon2.m", "dngon3.m", "dngon1.m", "dngon2.m", "dngon3.m", "dngon1.m"],
    ["cstl1rev.m", "cstl2rev.m", "cstl3rev.m", "cstl1rev.m", "cstl2rev.m", "cstl3rev.m", "cstl1rev.m"],
    ["sf05.m", "sf05.m", "sf05.m", "sf05.m", "sf05.m", "sf05.m", "sf05.m"],
];

/// Text colour sets: index 0 is the background, 1..=3 the glyph shades.
pub const TEXT_COLORS: [[u8; 4]; 40] = [
    [0x00, 0x19, 0x19, 0x19],
    [0x00, 0x08, 0x08, 0x08],
    [0x00, 0x0F, 0x0F, 0x0F],
    [0x00, 0x15, 0x15, 0x15],
    [0x00, 0x01, 0x01, 0x01],
    [0x00, 0x21, 0x21, 0x21],
    [0x00, 0x26, 0x26, 0x26],
    [0x00, 0x2B, 0x2B, 0x2B],
    [0x00, 0x31, 0x31, 0x31],
    [0x00, 0x36, 0x36, 0x36],
    [0x00, 0x3D, 0x3D, 0x3D],
    [0x00, 0x41, 0x41, 0x41],
    [0x00, 0x46, 0x46, 0x46],
    [0x00, 0x4C, 0x4C, 0x4C],
    [0x00, 0x50, 0x50, 0x50],
    [0x00, 0x55, 0x55, 0x55],
    [0x00, 0x5D, 0x5D, 0x5D],
    [0x00, 0x60, 0x60, 0x60],
    [0x00, 0x65, 0x65, 0x65],
    [0x00, 0x6C, 0x6C, 0x6C],
    [0x00, 0x70, 0x70, 0x70],
    [0x00, 0x75, 0x75, 0x75],
    [0x00, 0x7B, 0x7B, 0x7B],
    [0x00, 0x80, 0x80, 0x80],
    [0x00, 0x85, 0x85, 0x85],
    [0x00, 0x8D, 0x8D, 0x8D],
    [0x00, 0x90, 0x90, 0x90],
    [0x00, 0x97, 0x97, 0x97],
    [0x00, 0x9D, 0x9D, 0x9D],
    [0x00, 0xA4, 0xA4, 0xA4],
    [0x00, 0xAB, 0xAB, 0xAB],
    [0x00, 0xB0, 0xB0, 0xB0],
    [0x00, 0xB6, 0xB6, 0xB6],
    [0x00, 0xBD, 0xBD, 0xBD],
    [0x00, 0xC0, 0xC0, 0xC0],
    [0x00, 0xC6, 0xC6, 0xC6],
    [0x00, 0xCD, 0xCD, 0xCD],
    [0x00, 0xD0, 0xD0, 0xD0],
    [0x00, 0xD6, 0xD6, 0xD6],
    [0x00, 0xDB, 0xDB, 0xDB],
];

/// Resource ids that, concatenated, form the initial save archive of each
/// side (`SaveArchive::reset` in ScummVM). Each piece is < 64 KiB because CC
/// entries have 16-bit sizes.
pub const INITIAL_SAVE_PIECES: [u16; 6] = [0x2A0C, 0x2A1C, 0x2A2C, 0x2A3C, 0x284C, 0x2A5C];

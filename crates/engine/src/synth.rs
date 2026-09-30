// SPDX-License-Identifier: GPL-3.0-or-later
//! Procedural assets: demo sprites and a complete synthetic data set.
//!
//! The synthetic `xeen.cc` exercises every reader and viewer without any
//! copyrighted material, so tests, CI and the browser demo can run end to end.

use openxeen_formats::cc::CcBuilder;
use openxeen_formats::evt::{MazeEvent, events_to_bytes};
use openxeen_formats::font::FontBuilder;
use openxeen_formats::maze::{Direction, MazeCell, MazeData, flags};
use openxeen_formats::misc::string_table_to_bytes;
use openxeen_formats::monster::{Monster, monster_table_to_bytes};
use openxeen_formats::sprite::{IndexedImage, SpriteBuilder};
use openxeen_formats::voc::build_voc_u8;

use crate::font8x8::FONT8X8;
use crate::present::default_palette;

/// A shaded sphere using hue row `hue` (0..14) of the default palette.
pub fn orb_image(radius: usize, hue: usize) -> IndexedImage {
    let size = radius * 2;
    let mut img = IndexedImage::new(size, size);
    let r = radius as f32;
    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 + 0.5 - r;
            let dy = y as f32 + 0.5 - r;
            let d2 = dx * dx + dy * dy;
            if d2 <= r * r {
                let z = (r * r - d2).sqrt() / r;
                // Light from the upper left.
                let l = (0.35 + 0.65 * (z * 0.7 + (-dx - dy) / (2.0 * r) * 0.5)).clamp(0.0, 1.0);
                let level = (l * 15.0).round() as usize;
                img.set(x, y, Some((32 + (hue % 14) * 16 + level) as u8));
            }
        }
    }
    img
}

/// Renders text with the 8x8 UI font; rows shade through hue row `hue`.
pub fn text_image(text: &str, hue: usize) -> IndexedImage {
    let mut img = IndexedImage::new(text.len() * 8, 8);
    for (i, ch) in text.bytes().enumerate() {
        let glyph = FONT8X8[usize::from(ch & 0x7F)];
        for (row, bits) in glyph.iter().enumerate() {
            for col in 0..8 {
                if bits >> col & 1 != 0 {
                    let level = 15 - row;
                    img.set(i * 8 + col, row, Some((32 + (hue % 14) * 16 + level) as u8));
                }
            }
        }
    }
    img
}

/// Converts the UI font into a game `fnt` file (normal and reduced glyphs).
pub fn synthetic_font() -> Vec<u8> {
    let mut b = FontBuilder::new();
    for ch in 0..128u8 {
        let glyph = FONT8X8[usize::from(ch)];
        let mut rows = [[0u8; 8]; 8];
        let mut width = 0;
        for (y, bits) in glyph.iter().enumerate() {
            for x in 0..8 {
                if bits >> x & 1 != 0 {
                    rows[y][x] = if y < 2 {
                        3
                    } else if y < 5 {
                        2
                    } else {
                        1
                    };
                    width = width.max(x + 2);
                }
            }
        }
        let width = if ch == b' ' { 4 } else { width.max(2) as u8 };
        b.glyph(ch, false, rows, width);
        b.glyph(ch, true, rows, width.saturating_sub(1).max(1));
    }
    b.build()
}

fn palette_file() -> Vec<u8> {
    default_palette()
        .iter()
        .flat_map(|c| c.iter().map(|v| v >> 2))
        .collect()
}

fn maze() -> MazeData {
    let mut m = MazeData {
        maze_number: 1,
        wall_kind: 4,
        maze_flags: flags::RESTRICTION_TOWN_PORTAL,
        ..MazeData::default()
    };
    for y in 0..16 {
        for x in 0..16 {
            // Outer walls plus a few inner ones, one nibble per side.
            if y == 15 || (x % 4 == 1 && y % 3 == 0) {
                m.set_indoor_wall(x, y, Direction::North, 1);
            }
            if x == 15 || (y % 5 == 2 && x % 2 == 0) {
                m.set_indoor_wall(x, y, Direction::East, 1);
            }
            if y == 0 {
                m.set_indoor_wall(x, y, Direction::South, 1);
            }
            if x == 0 {
                m.set_indoor_wall(x, y, Direction::West, 1);
            }
            m.cells[y][x] = MazeCell {
                surface_id: ((x + y) % 3) as u8,
                flags: if (x, y) == (7, 7) { 0x10 } else { 0 },
            };
            m.seen[y][x] = x < 8;
        }
    }
    m
}

fn sprite(images: Vec<IndexedImage>) -> Vec<u8> {
    let mut b = SpriteBuilder::new();
    for i in images {
        b.frame(i);
    }
    b.build().expect("synthetic sprite fits the format")
}

/// Builds a synthetic `xeen.cc` exercising every viewer.
pub fn synthetic_xeen_cc() -> Vec<u8> {
    let mut cc = CcBuilder::new();
    let mut add = |name: &str, data: Vec<u8>| {
        cc.add(name, data).expect("synthetic resource fits");
    };
    add("mm4.pal", palette_file());
    add("fnt", synthetic_font());
    add(
        "001.mon",
        sprite((0..6).map(|i| orb_image(10 + i * 2, i * 2)).collect()),
    );
    add(
        "global.icn",
        sprite(vec![text_image("OPENXEEN", 3), text_image("SYNTH", 9)]),
    );
    let mut back = vec![0u8; 320 * 200];
    for y in 0..200 {
        for x in 0..320 {
            back[y * 320 + x] = (32 + ((x / 23) % 14) * 16 + (y * 16 / 200)) as u8;
        }
    }
    add("back.raw", back);
    add("maze0001.dat", maze().to_bytes());
    let events = vec![
        MazeEvent {
            x: 7,
            y: 7,
            direction: 4,
            line: 0,
            opcode: 0x04,
            params: vec![1],
        },
        MazeEvent {
            x: 7,
            y: 7,
            direction: 4,
            line: 1,
            opcode: 0x12,
            params: vec![],
        },
        MazeEvent {
            x: 2,
            y: 3,
            direction: 0,
            line: 0,
            opcode: 0x01,
            params: vec![0],
        },
        MazeEvent {
            x: 2,
            y: 3,
            direction: 0,
            line: 1,
            opcode: 0x12,
            params: vec![],
        },
    ];
    add(
        "maze0001.evt",
        events_to_bytes(&events).expect("small script"),
    );
    add(
        "aaze0001.txt",
        string_table_to_bytes(&[
            "A synthetic sign".to_string(),
            "Welcome to the synthetic test map".to_string(),
        ]),
    );
    let mut name = b"Synthetic Test Map".to_vec();
    name.resize(33, 0);
    add("xeen0001.txt", name);
    add("aaze0001.hed", vec![0u8; 512]);
    let pcm: Vec<u8> = (0..4000)
        .map(|i| (128.0 + 90.0 * ((i as f32) * 0.09).sin() * (1.0 - i as f32 / 4000.0)) as u8)
        .collect();
    add("click.voc", build_voc_u8(11025, &pcm));
    let goblin = Monster {
        name: "Synth Goblin".into(),
        experience: 150,
        hp: 12,
        armor_class: 3,
        speed: 9,
        number_of_attacks: 1,
        hates_class: 15,
        strikes: 1,
        dmg_per_strike: 6,
        monster_type: 3,
        gold: 20,
        image_number: 1,
        attack_voc: "click".into(),
        ..Monster::default()
    };
    add("xeen.mon", monster_table_to_bytes(&[goblin]));
    cc.build(true).expect("synthetic archive fits")
}

/// The synthetic data set as `(file name, bytes)` pairs.
pub fn synthetic_game_files() -> Vec<(String, Vec<u8>)> {
    vec![("xeen.cc".to_string(), synthetic_xeen_cc())]
}

#[cfg(test)]
mod tests {
    use super::*;
    use openxeen_formats::gamedata::{GameData, GameVariant, Side};
    use openxeen_formats::{font, maze, sprite};

    #[test]
    fn synthetic_data_parses_with_the_real_readers() {
        let g = GameData::from_files(synthetic_game_files()).unwrap();
        assert_eq!(g.variant(), GameVariant::Clouds);
        let s = sprite::SpriteFile::parse(g.require(Side::Clouds, "001.mon").unwrap()).unwrap();
        assert_eq!(s.frame_count(), 6);
        font::Font::parse_western(g.require(Side::Clouds, "fnt").unwrap()).unwrap();
        maze::MazeData::parse(&g.require(Side::Clouds, "maze0001.dat").unwrap()).unwrap();
        assert_eq!(g.require(Side::Clouds, "back.raw").unwrap().len(), 64000);
    }
}

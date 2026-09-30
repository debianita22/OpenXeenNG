// SPDX-License-Identifier: GPL-3.0-or-later
//! Text rendering: the built-in 8x8 UI font and the game's own font.

use openxeen_formats::font::{Font, GLYPH_HEIGHT, GLYPH_WIDTH};

use crate::font8x8::FONT8X8;
use crate::gfx::{Rect, Surface};

/// Width of a UI font cell.
pub const UI_CHAR_W: i32 = 8;
/// Height of a UI font cell.
pub const UI_CHAR_H: i32 = 8;

/// Draws `text` with the built-in font. `bg` fills each cell first.
/// Characters outside ASCII are drawn as `?`. Returns the x after the text.
pub fn draw_ui_text(dest: &mut Surface, x: i32, y: i32, text: &str, fg: u8, bg: Option<u8>) -> i32 {
    let mut cx = x;
    for ch in text.chars() {
        let code = if ch.is_ascii() {
            ch as usize
        } else {
            b'?' as usize
        };
        if let Some(bg) = bg {
            dest.fill_rect(Rect::new(cx, y, cx + UI_CHAR_W, y + UI_CHAR_H), bg);
        }
        let glyph = &FONT8X8[code];
        for (row, bits) in glyph.iter().enumerate() {
            for col in 0..8 {
                if bits >> col & 1 != 0 {
                    dest.put(cx + col, y + row as i32, fg);
                }
            }
        }
        cx += UI_CHAR_W;
    }
    cx
}

/// Draws UI text with a 1-pixel shadow for readability over images.
pub fn draw_ui_text_shadow(
    dest: &mut Surface,
    x: i32,
    y: i32,
    text: &str,
    fg: u8,
    shadow: u8,
) -> i32 {
    draw_ui_text(dest, x + 1, y + 1, text, shadow, None);
    draw_ui_text(dest, x, y, text, fg, None)
}

/// Draws one character with the game font (`FontSurface::writeChar`):
/// shades 1..=3 map to `colors[1..=3]`, descenders drop one row, and the
/// result is clipped to `clip`. Returns the advance width.
pub fn draw_game_char(
    dest: &mut Surface,
    font: &Font,
    ch: u8,
    x: i32,
    y: i32,
    colors: [u8; 4],
    reduced: bool,
    clip: Rect,
) -> i32 {
    // The western releases ignore bit 7 (`getNextChar` masks with 0x7F).
    let ch = ch & 0x7F;
    let y = if Font::has_descender(ch) { y + 1 } else { y };
    let rows = font.glyph_rows(ch, reduced);
    for (row, bits) in rows.iter().enumerate().take(GLYPH_HEIGHT) {
        let py = y + row as i32;
        if py < clip.top || py >= clip.bottom {
            continue;
        }
        for col in 0..GLYPH_WIDTH {
            let shade = (bits >> (2 * col)) & 3;
            let px = x + col as i32;
            if shade != 0 && px >= clip.left && px < clip.right {
                dest.put(px, py, colors[shade as usize]);
            }
        }
    }
    i32::from(font.width(ch, reduced))
}

/// Draws a plain string with the game font (control codes arrive with the
/// window code in milestone 1). Spaces advance 4 pixels (3 reduced) without
/// using the width table, as `FontSurface::writeString` does.
pub fn draw_game_text(
    dest: &mut Surface,
    font: &Font,
    x: i32,
    y: i32,
    text: &[u8],
    colors: [u8; 4],
    reduced: bool,
) -> i32 {
    let clip = dest.rect();
    let mut cx = x;
    for &c in text {
        let c = c & 0x7F;
        if c == b' ' {
            cx += if reduced { 3 } else { 4 };
        } else {
            cx += draw_game_char(dest, font, c, cx, y, colors, reduced, clip);
        }
    }
    cx
}

/// Width of a string in the game font, measured like `getNextCharWidth`:
/// a space counts 4 pixels even in the reduced font, other characters use
/// the width table, control characters count nothing.
pub fn game_text_width(font: &Font, text: &[u8], reduced: bool) -> i32 {
    text.iter()
        .map(|&c| match c & 0x7F {
            b' ' => 4,
            c if c > b' ' => i32::from(font.width(c, reduced)),
            _ => 0,
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use openxeen_formats::font::FontBuilder;

    #[test]
    fn ui_text_draws_pixels() {
        let mut s = Surface::new(64, 16);
        let end = draw_ui_text(&mut s, 0, 0, "Hi", 7, Some(1));
        assert_eq!(end, 16);
        assert!(s.pixels().contains(&7));
        assert!(s.pixels().contains(&1));
    }

    #[test]
    fn game_text_uses_shades_and_widths() {
        let mut rows = [[0u8; 8]; 8];
        rows[0] = [1, 2, 3, 0, 0, 0, 0, 0];
        let mut b = FontBuilder::new();
        b.glyph(b'a', false, rows, 4).glyph(b'g', false, rows, 5);
        let font = Font::parse_western(b.build()).unwrap();
        let mut s = Surface::new(20, 12);
        let end = draw_game_text(&mut s, &font, 0, 0, b"ag", [0, 10, 11, 12], false);
        assert_eq!(end, 9);
        assert_eq!((s.get(0, 0), s.get(1, 0), s.get(2, 0)), (10, 11, 12));
        // 'g' is a descender: drawn one row lower, also with bit 7 set.
        assert_eq!(s.get(4, 1), 10);
        let mut s2 = Surface::new(20, 12);
        draw_game_text(&mut s2, &font, 0, 0, &[b'g' | 0x80], [0, 10, 11, 12], false);
        assert_eq!(s2.get(0, 1), 10);
        assert_eq!(game_text_width(&font, b"ag", false), 9);
        // Spaces: 4 px drawn (3 reduced), always 4 when measured.
        assert_eq!(
            draw_game_text(&mut s, &font, 0, 0, b"a a", [0, 1, 2, 3], false),
            12
        );
        assert_eq!(
            draw_game_text(&mut s, &font, 0, 0, b"a a", [0, 1, 2, 3], true),
            3
        );
        assert_eq!(game_text_width(&font, b"a a", true), 4);
    }
}

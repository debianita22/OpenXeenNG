// SPDX-License-Identifier: GPL-3.0-or-later
//! Palette helpers and conversion of the indexed screen to true colour.

use openxeen_formats::palette::DacScaling;

/// Engine palette: 256 entries of 8-bit levels in the engine's "VGA << 2"
/// domain (0..=252 for full intensity).
pub type Palette = [[u8; 3]; 256];

/// Lookup table mapping engine levels to display levels.
pub fn level_lut(scaling: DacScaling) -> [u8; 256] {
    let mut lut = [0u8; 256];
    for (v, out) in lut.iter_mut().enumerate() {
        *out = match scaling {
            DacScaling::Shift => v as u8,
            DacScaling::Full => ((v as u32 * 255 + 126) / 252).min(255) as u8,
        };
    }
    lut
}

/// Index of the palette entry closest to an engine-level colour.
pub fn nearest(pal: &Palette, rgb: [u8; 3]) -> u8 {
    let mut best = 0usize;
    let mut best_d = u32::MAX;
    for (i, c) in pal.iter().enumerate() {
        let d: u32 = (0..3)
            .map(|k| {
                let e = i32::from(c[k]) - i32::from(rgb[k]);
                (e * e) as u32
            })
            .sum();
        if d < best_d {
            best_d = d;
            best = i;
        }
    }
    best as u8
}

/// Readable UI colours picked from whatever palette is active.
#[derive(Debug, Clone, Copy)]
pub struct UiColors {
    /// Background.
    pub bg: u8,
    /// Normal text.
    pub text: u8,
    /// Dim text.
    pub dim: u8,
    /// Highlight / selection.
    pub hi: u8,
    /// Selection bar background.
    pub bar: u8,
    /// Warnings.
    pub warn: u8,
}

impl UiColors {
    /// Picks colours from a palette.
    pub fn from_palette(pal: &Palette) -> Self {
        UiColors {
            bg: nearest(pal, [0, 0, 0]),
            text: nearest(pal, [230, 230, 230]),
            dim: nearest(pal, [130, 130, 130]),
            hi: nearest(pal, [252, 220, 60]),
            bar: nearest(pal, [40, 60, 140]),
            warn: nearest(pal, [240, 70, 50]),
        }
    }
}

/// A palette for screens shown without game data: 16 EGA colours, a grey
/// ramp at 16..32 and a hue wheel with brightness ramps from 32 upwards.
pub fn default_palette() -> Palette {
    let mut p = [[0u8; 3]; 256];
    const EGA: [[u8; 3]; 16] = [
        [0, 0, 0],
        [0, 0, 168],
        [0, 168, 0],
        [0, 168, 168],
        [168, 0, 0],
        [168, 0, 168],
        [168, 84, 0],
        [168, 168, 168],
        [84, 84, 84],
        [84, 84, 252],
        [84, 252, 84],
        [84, 252, 252],
        [252, 84, 84],
        [252, 84, 252],
        [252, 252, 84],
        [252, 252, 252],
    ];
    p[..16].copy_from_slice(&EGA);
    for i in 0..16 {
        let v = (i * 252 / 15) as u8;
        p[16 + i] = [v, v, v];
    }
    // 14 hues x 16 brightness levels.
    for h in 0..14 {
        let (r, g, b) = hue(h as f32 / 14.0);
        for l in 0..16 {
            let k = (l as f32 + 1.0) / 16.0;
            p[32 + h * 16 + l] = [
                (r * k * 252.0) as u8,
                (g * k * 252.0) as u8,
                (b * k * 252.0) as u8,
            ];
        }
    }
    p
}

fn hue(h: f32) -> (f32, f32, f32) {
    let x = h * 6.0;
    let f = x - x.floor();
    match x as i32 % 6 {
        0 => (1.0, f, 0.0),
        1 => (1.0 - f, 1.0, 0.0),
        2 => (0.0, 1.0, f),
        3 => (0.0, 1.0 - f, 1.0),
        4 => (f, 0.0, 1.0),
        _ => (1.0, 0.0, 1.0 - f),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn luts() {
        assert_eq!(level_lut(DacScaling::Full)[252], 255);
        assert_eq!(level_lut(DacScaling::Full)[0], 0);
        assert_eq!(level_lut(DacScaling::Shift)[252], 252);
    }

    #[test]
    fn nearest_finds_white() {
        let p = default_palette();
        assert_eq!(nearest(&p, [252, 252, 252]), 15);
        let ui = UiColors::from_palette(&p);
        assert_eq!(ui.bg, 0);
    }
}

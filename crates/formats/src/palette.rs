// SPDX-License-Identifier: GPL-3.0-or-later
//! VGA palettes (`mm4.pal`, `dark.pal`, `intro.pal`, ...).
//!
//! A palette file is 256 RGB triplets of 6-bit VGA DAC values (0..=63).

use crate::error::{FormatError, Result};

/// Number of bytes in a palette file.
pub const PALETTE_BYTES: usize = 256 * 3;

/// How 6-bit DAC levels are expanded to 8 bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DacScaling {
    /// `v << 2`, as ScummVM does: full intensity becomes 252.
    Shift,
    /// `round(v * 255 / 63)`: full intensity becomes 255 (closer to a real DAC).
    #[default]
    Full,
}

impl DacScaling {
    /// Expands one 6-bit level.
    pub fn expand(self, v6: u8) -> u8 {
        let v = v6.min(63);
        match self {
            DacScaling::Shift => v << 2,
            DacScaling::Full => ((u16::from(v) * 255 + 31) / 63) as u8,
        }
    }
}

/// A 256-colour palette in 6-bit VGA levels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VgaPalette {
    /// `[r, g, b]` per index, each 0..=63.
    pub colors: [[u8; 3]; 256],
}

impl Default for VgaPalette {
    fn default() -> Self {
        VgaPalette {
            colors: [[0; 3]; 256],
        }
    }
}

impl VgaPalette {
    /// Parses a 768-byte palette file. Extra bytes are ignored; values above
    /// 63 are masked to 6 bits like the VGA DAC does.
    pub fn parse(data: &[u8]) -> Result<Self> {
        if data.len() < PALETTE_BYTES {
            return Err(FormatError::UnexpectedEof {
                what: "palette",
                offset: data.len(),
                needed: PALETTE_BYTES - data.len(),
            });
        }
        let mut colors = [[0u8; 3]; 256];
        for (i, c) in colors.iter_mut().enumerate() {
            for k in 0..3 {
                c[k] = data[i * 3 + k] & 0x3F;
            }
        }
        Ok(VgaPalette { colors })
    }

    /// Serialises back to the 768-byte file format.
    pub fn to_bytes(&self) -> Vec<u8> {
        self.colors.iter().flat_map(|c| c.iter().copied()).collect()
    }

    /// Expands to 8-bit RGB.
    pub fn to_rgb8(&self, scaling: DacScaling) -> [[u8; 3]; 256] {
        let mut out = [[0u8; 3]; 256];
        for (o, c) in out.iter_mut().zip(self.colors.iter()) {
            for k in 0..3 {
                o[k] = scaling.expand(c[k]);
            }
        }
        out
    }

    /// Expands to the 8-bit "shifted" domain the original engine fades in
    /// (identical to ScummVM's in-memory palette).
    pub fn to_engine_levels(&self) -> [[u8; 3]; 256] {
        self.to_rgb8(DacScaling::Shift)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaling_modes() {
        assert_eq!(DacScaling::Shift.expand(63), 252);
        assert_eq!(DacScaling::Full.expand(63), 255);
        assert_eq!(DacScaling::Full.expand(0), 0);
        assert_eq!(DacScaling::Full.expand(32), 130);
    }

    #[test]
    fn parse_and_round_trip() {
        let data: Vec<u8> = (0..PALETTE_BYTES).map(|i| (i % 64) as u8).collect();
        let p = VgaPalette::parse(&data).unwrap();
        assert_eq!(p.colors[1], [3, 4, 5]);
        assert_eq!(p.to_bytes(), data);
        assert!(VgaPalette::parse(&data[..700]).is_err());
    }
}

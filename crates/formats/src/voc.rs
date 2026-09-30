// SPDX-License-Identifier: GPL-3.0-or-later
//! Creative Voice (`.voc`) sound effects and speech.
//!
//! Supports the block types the game uses: 1 (sound data), 2 (continuation),
//! 3 (silence) and 9 (new-style sound data); 8-bit unsigned and 16-bit
//! signed PCM. Other block types are skipped.

use crate::error::{FormatError, Result};
use crate::io::Reader;

const SIGNATURE: &[u8] = b"Creative Voice File\x1A";

/// Decoded mono PCM audio.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Voc {
    /// Sample rate in Hz.
    pub sample_rate: u32,
    /// Signed 16-bit mono samples.
    pub samples: Vec<i16>,
}

/// Decodes a `.voc` file.
pub fn parse_voc(data: &[u8]) -> Result<Voc> {
    let mut r = Reader::new(data, "voc");
    if r.bytes(SIGNATURE.len())? != SIGNATURE {
        return Err(FormatError::invalid(
            "voc",
            "missing Creative Voice File signature",
        ));
    }
    let header_size = r.u16()? as usize;
    r.seek(header_size.max(SIGNATURE.len() + 2))?;

    let mut out = Voc {
        sample_rate: 0,
        samples: Vec::new(),
    };
    let mut current_bits = 8u8;
    while !r.at_end() {
        let block = r.u8()?;
        if block == 0 {
            break;
        }
        let b = r.bytes(3)?;
        let len = usize::from(b[0]) | usize::from(b[1]) << 8 | usize::from(b[2]) << 16;
        let body = r.bytes(len.min(r.remaining()))?;
        match block {
            1 => {
                if body.len() < 2 {
                    return Err(FormatError::invalid("voc", "short sound block"));
                }
                let divisor = u32::from(body[0]);
                let codec = body[1];
                if codec != 0 {
                    return Err(FormatError::invalid(
                        "voc",
                        format!("unsupported codec {codec}"),
                    ));
                }
                set_rate(&mut out, 1_000_000 / (256 - divisor.min(255)))?;
                current_bits = 8;
                push_pcm(&mut out.samples, &body[2..], 8);
            }
            2 => push_pcm(&mut out.samples, body, current_bits),
            3 if body.len() >= 3 => {
                let n = usize::from(u16::from_le_bytes([body[0], body[1]])) + 1;
                if out.sample_rate == 0 {
                    set_rate(&mut out, 1_000_000 / (256 - u32::from(body[2]).min(255)))?;
                }
                out.samples.extend(std::iter::repeat_n(0, n));
            }
            9 => {
                if body.len() < 12 {
                    return Err(FormatError::invalid("voc", "short type 9 block"));
                }
                let rate = u32::from_le_bytes([body[0], body[1], body[2], body[3]]);
                let bits = body[4];
                let channels = body[5];
                let codec = u16::from_le_bytes([body[6], body[7]]);
                if channels != 1 || !matches!((bits, codec), (8, 0) | (16, 4)) {
                    return Err(FormatError::invalid(
                        "voc",
                        format!("unsupported format: {bits} bits, {channels} ch, codec {codec}"),
                    ));
                }
                set_rate(&mut out, rate)?;
                current_bits = bits;
                push_pcm(&mut out.samples, &body[12..], bits);
            }
            _ => {}
        }
    }
    if out.sample_rate == 0 {
        return Err(FormatError::invalid("voc", "no sound data"));
    }
    Ok(out)
}

fn set_rate(out: &mut Voc, rate: u32) -> Result<()> {
    if rate == 0 {
        return Err(FormatError::invalid("voc", "sample rate of zero"));
    }
    if out.sample_rate == 0 {
        out.sample_rate = rate;
    }
    Ok(())
}

fn push_pcm(out: &mut Vec<i16>, data: &[u8], bits: u8) {
    if bits == 16 {
        out.extend(
            data.chunks_exact(2)
                .map(|c| i16::from_le_bytes([c[0], c[1]])),
        );
    } else {
        out.extend(data.iter().map(|&b| (i16::from(b) - 128) << 8));
    }
}

/// Encodes 8-bit mono PCM as a minimal `.voc` file (tests, synthetic data).
pub fn build_voc_u8(sample_rate: u32, samples_u8: &[u8]) -> Vec<u8> {
    let divisor = 256u32
        .saturating_sub(1_000_000 / sample_rate.max(3922))
        .min(255) as u8;
    let mut v = SIGNATURE.to_vec();
    v.extend_from_slice(&0x1Au16.to_le_bytes());
    v.extend_from_slice(&0x010Au16.to_le_bytes());
    v.extend_from_slice(&(!0x010Au16).wrapping_add(0x1234).to_le_bytes());
    let len = samples_u8.len() + 2;
    v.push(1);
    v.extend_from_slice(&(len as u32).to_le_bytes()[..3]);
    v.push(divisor);
    v.push(0);
    v.extend_from_slice(samples_u8);
    v.push(0);
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_8bit() {
        let pcm: Vec<u8> = (0..100).map(|i| (128 + (i % 20) - 10) as u8).collect();
        let voc = parse_voc(&build_voc_u8(11025, &pcm)).unwrap();
        // divisor = 256 - 1_000_000/11025 = 256 - 90 = 166 -> rate 1e6/90
        assert_eq!(voc.sample_rate, 11111);
        assert_eq!(voc.samples.len(), 100);
        assert_eq!(voc.samples[0], (i16::from(pcm[0]) - 128) << 8);
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_voc(b"RIFF....WAVE").is_err());
    }
}

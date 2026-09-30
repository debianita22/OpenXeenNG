// SPDX-License-Identifier: GPL-3.0-or-later
//! Software mixer producing interleaved stereo 16-bit samples.
//!
//! Sources are mono sample buffers at any rate; they are resampled with
//! linear interpolation. Each [`Channel`] plays one sound at a time, like
//! the original (a new effect cuts the previous one on its channel).

use std::rc::Rc;

/// Mixer channels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    /// Sound effects.
    Sfx,
    /// Speech.
    Voice,
    /// Music (rendered PCM, e.g. from the OPL emulator).
    Music,
}

const CHANNELS: usize = 3;

fn slot(c: Channel) -> usize {
    match c {
        Channel::Sfx => 0,
        Channel::Voice => 1,
        Channel::Music => 2,
    }
}

#[derive(Debug, Clone)]
struct Voice {
    samples: Rc<[i16]>,
    /// Position in source samples, 32.32 fixed point.
    pos: u64,
    step: u64,
    volume: u16,
    looping: bool,
}

/// The mixer.
#[derive(Debug, Clone)]
pub struct Mixer {
    rate: u32,
    voices: [Option<Voice>; CHANNELS],
    volumes: [u16; CHANNELS],
}

impl Mixer {
    /// Creates a mixer producing `rate` Hz output.
    pub fn new(rate: u32) -> Self {
        Mixer {
            rate: rate.max(1),
            voices: [None, None, None],
            volumes: [256; CHANNELS],
        }
    }

    /// Output sample rate.
    pub fn rate(&self) -> u32 {
        self.rate
    }

    /// Sets a channel volume (256 = unity).
    pub fn set_volume(&mut self, ch: Channel, volume: u16) {
        self.volumes[slot(ch)] = volume;
    }

    /// Starts a sound on a channel, replacing what was playing there.
    pub fn play(&mut self, ch: Channel, samples: Rc<[i16]>, src_rate: u32, looping: bool) {
        let step = ((u64::from(src_rate.max(1))) << 32) / u64::from(self.rate);
        self.voices[slot(ch)] = Some(Voice {
            samples,
            pos: 0,
            step,
            volume: 256,
            looping,
        });
    }

    /// Stops a channel.
    pub fn stop(&mut self, ch: Channel) {
        self.voices[slot(ch)] = None;
    }

    /// True while a channel is playing.
    pub fn is_playing(&self, ch: Channel) -> bool {
        self.voices[slot(ch)].is_some()
    }

    /// Mixes into `out` (interleaved stereo; odd lengths leave the last sample silent).
    pub fn render(&mut self, out: &mut [i16]) {
        out.fill(0);
        let frames = out.len() / 2;
        let mut acc = vec![0i32; frames];
        for (i, v) in self.voices.iter_mut().enumerate() {
            let Some(voice) = v else { continue };
            let gain = i32::from(voice.volume) * i32::from(self.volumes[i]) / 256;
            let len = voice.samples.len() as u64;
            let mut finished = false;
            for a in acc.iter_mut() {
                let idx = voice.pos >> 32;
                if idx >= len {
                    if voice.looping && len > 0 {
                        voice.pos %= len << 32;
                    } else {
                        finished = true;
                        break;
                    }
                }
                let idx = (voice.pos >> 32) as usize;
                let frac = ((voice.pos >> 16) & 0xFFFF) as i32;
                let s0 = i32::from(voice.samples[idx]);
                let s1 = voice
                    .samples
                    .get(idx + 1)
                    .map_or(s0, |&s| i32::from(s));
                let s = s0 + (((s1 - s0) * frac) >> 16);
                *a += s * gain / 256;
                voice.pos += voice.step;
            }
            if finished {
                *v = None;
            }
        }
        for (i, a) in acc.iter().enumerate() {
            let s = (*a).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
            out[2 * i] = s;
            out[2 * i + 1] = s;
        }
    }
}

/// A short square/sine blip used by menus and the no-data demo.
pub fn tone(freq: f32, ms: u32, rate: u32) -> Rc<[i16]> {
    let n = (rate as u64 * u64::from(ms) / 1000) as usize;
    (0..n)
        .map(|i| {
            let t = i as f32 / rate as f32;
            let env = 1.0 - i as f32 / n as f32;
            let s = (t * freq * std::f32::consts::TAU).sin();
            (s * env * 9000.0) as i16
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plays_and_finishes() {
        let mut m = Mixer::new(1000);
        let s: Rc<[i16]> = vec![1000i16; 10].into();
        m.play(Channel::Sfx, s, 1000, false);
        let mut out = vec![0i16; 40];
        m.render(&mut out);
        assert_eq!(out[0], 1000);
        assert_eq!(out[1], 1000);
        assert_eq!(out[19], 1000);
        assert_eq!(out[20], 0);
        assert!(!m.is_playing(Channel::Sfx));
    }

    #[test]
    fn resamples_by_rate_ratio() {
        let mut m = Mixer::new(2000);
        let s: Rc<[i16]> = vec![500i16; 10].into();
        m.play(Channel::Voice, s, 1000, false);
        let mut out = vec![0i16; 2 * 30];
        m.render(&mut out);
        // 10 source samples at half the output rate last 20 output frames.
        assert_eq!(out[2 * 19], 500);
        assert_eq!(out[2 * 20], 0);
    }

    #[test]
    fn clamps_and_loops() {
        let mut m = Mixer::new(100);
        let loud: Rc<[i16]> = vec![30000i16; 4].into();
        m.play(Channel::Sfx, loud.clone(), 100, true);
        m.play(Channel::Music, loud, 100, true);
        let mut out = vec![0i16; 2 * 20];
        m.render(&mut out);
        assert!(out.iter().all(|&s| s == i16::MAX));
        assert!(m.is_playing(Channel::Music));
    }
}

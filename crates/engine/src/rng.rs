// SPDX-License-Identifier: GPL-3.0-or-later
//! Random numbers.
//!
//! [`ScummRandom`] reproduces ScummVM's `Common::RandomSource` bit for bit
//! (xorshift 13/21/11 on a 32-bit seed, output multiplied by 0xDEADBF03), so
//! a run seeded like a ScummVM run draws the same numbers. Everything that
//! needs randomness takes a `&mut dyn RandomSource`, which lets tests inject
//! fixed sequences.

/// Source of random numbers.
pub trait RandomSource {
    /// Uniform-ish number in `0..=max` (ScummVM `getRandomNumber`).
    fn random_number(&mut self, max: u32) -> u32;

    /// Number in `min..=max`.
    fn random_range(&mut self, min: u32, max: u32) -> u32 {
        self.random_number(max - min) + min
    }
}

/// Port of ScummVM's `Common::RandomSource`.
#[derive(Debug, Clone)]
pub struct ScummRandom {
    seed: u32,
}

impl ScummRandom {
    /// Creates a source; a zero seed is bumped to 1 like the original.
    pub fn new(seed: u32) -> Self {
        ScummRandom {
            seed: seed.max(1),
        }
    }

    /// Current internal seed.
    pub fn seed(&self) -> u32 {
        self.seed
    }

    fn scramble(&mut self) {
        self.seed ^= self.seed >> 13;
        self.seed ^= self.seed << 21;
        self.seed ^= self.seed >> 11;
    }
}

impl RandomSource for ScummRandom {
    fn random_number(&mut self, max: u32) -> u32 {
        self.scramble();
        let v = self.seed.wrapping_mul(0xDEAD_BF03);
        if max == u32::MAX { v } else { v % (max + 1) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_reference_sequence() {
        // Computed with the C++ formula for seed 1.
        let mut r = ScummRandom::new(1);
        let mut seed: u32 = 1;
        for _ in 0..1000 {
            seed ^= seed >> 13;
            seed ^= seed << 21;
            seed ^= seed >> 11;
            let expect = seed.wrapping_mul(0xDEADBF03) % 0x10000;
            assert_eq!(r.random_number(0xFFFF), expect);
        }
    }

    #[test]
    fn zero_seed_is_bumped() {
        assert_eq!(ScummRandom::new(0).seed(), 1);
    }

    #[test]
    fn range_is_inclusive() {
        let mut r = ScummRandom::new(7);
        let mut seen = [false; 3];
        for _ in 0..200 {
            let v = r.random_range(4, 6);
            assert!((4..=6).contains(&v));
            seen[(v - 4) as usize] = true;
        }
        assert!(seen.iter().all(|&s| s));
    }
}

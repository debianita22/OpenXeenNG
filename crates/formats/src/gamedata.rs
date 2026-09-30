// SPDX-License-Identifier: GPL-3.0-or-later
//! The set of original data files, plus optional loose-file overrides.
//!
//! Everything is held in memory so the same code runs natively, inside a
//! libretro core and in the browser (where files come from a file picker).
//!
//! Lookup order for a resource on a given side:
//! 1. mod overrides (loose files supplied by the player/mod author);
//! 2. the side's CC archive (`xeen.cc` for Clouds, `dark.cc` for Darkside);
//! 3. `intro.cc`;
//! 4. loose files found next to the archives.
//!
//! Steps 2-4 follow ScummVM's `File::open` search order.

use std::collections::HashMap;

use crate::cc::CcArchive;
use crate::error::{FormatError, Result};

/// Which game the data belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameVariant {
    /// Clouds + Darkside installed together (`xeen.cc` + `dark.cc`).
    WorldOfXeen,
    /// Clouds of Xeen alone (`xeen.cc`).
    Clouds,
    /// Darkside of Xeen alone (`dark.cc`).
    DarkSide,
    /// Swords of Xeen (`swrd.cc`).
    Swords,
}

impl GameVariant {
    /// Human readable name.
    pub fn title(self) -> &'static str {
        match self {
            GameVariant::WorldOfXeen => "World of Xeen",
            GameVariant::Clouds => "Clouds of Xeen",
            GameVariant::DarkSide => "Darkside of Xeen",
            GameVariant::Swords => "Swords of Xeen",
        }
    }
}

/// Archive selector ("cc number" in the engine).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    /// Clouds side (`xeen.cc`).
    Clouds,
    /// Darkside (`dark.cc`, or `swrd.cc` for Swords of Xeen).
    Dark,
    /// Intro archive (`intro.cc`).
    Intro,
}

/// Loaded game data.
#[derive(Debug, Clone)]
pub struct GameData {
    variant: GameVariant,
    xeen: Option<CcArchive>,
    dark: Option<CcArchive>,
    intro: Option<CcArchive>,
    loose: HashMap<String, Vec<u8>>,
    overrides: HashMap<String, Vec<u8>>,
}

fn key(name: &str) -> String {
    name.rsplit(['/', '\\'])
        .next()
        .unwrap_or(name)
        .to_ascii_lowercase()
}

impl GameData {
    /// Builds the data set from `(file name, bytes)` pairs, as found in the
    /// game directory. Names are matched case-insensitively on their base name.
    pub fn from_files<I, S>(files: I) -> Result<Self>
    where
        I: IntoIterator<Item = (S, Vec<u8>)>,
        S: AsRef<str>,
    {
        let mut loose: HashMap<String, Vec<u8>> = HashMap::new();
        for (name, data) in files {
            loose.insert(key(name.as_ref()), data);
        }
        let mut take = |n: &str| -> Result<Option<CcArchive>> {
            match loose.remove(n) {
                Some(d) => CcArchive::parse(d, true).map(Some),
                None => Ok(None),
            }
        };
        let swrd = take("swrd.cc")?;
        let xeen = take("xeen.cc")?;
        let dark = take("dark.cc")?;
        let intro = take("intro.cc")?;
        let (variant, xeen, dark) = match (swrd, xeen, dark) {
            (Some(s), _, _) => (GameVariant::Swords, None, Some(s)),
            (None, Some(x), Some(d)) => (GameVariant::WorldOfXeen, Some(x), Some(d)),
            (None, Some(x), None) => (GameVariant::Clouds, Some(x), None),
            (None, None, Some(d)) => (GameVariant::DarkSide, None, Some(d)),
            (None, None, None) => {
                return Err(FormatError::NotFound(
                    "no game archive (xeen.cc, dark.cc or swrd.cc)".into(),
                ));
            }
        };
        Ok(GameData {
            variant,
            xeen,
            dark,
            intro,
            loose,
            overrides: HashMap::new(),
        })
    }

    /// The detected game.
    pub fn variant(&self) -> GameVariant {
        self.variant
    }

    /// The side the game starts on.
    pub fn default_side(&self) -> Side {
        match self.variant {
            GameVariant::DarkSide | GameVariant::Swords => Side::Dark,
            _ => Side::Clouds,
        }
    }

    /// The archive for a side, if present.
    pub fn archive(&self, side: Side) -> Option<&CcArchive> {
        match side {
            Side::Clouds => self.xeen.as_ref(),
            Side::Dark => self.dark.as_ref(),
            Side::Intro => self.intro.as_ref(),
        }
    }

    /// Sides whose archive is present.
    pub fn sides(&self) -> Vec<Side> {
        [Side::Clouds, Side::Dark, Side::Intro]
            .into_iter()
            .filter(|&s| self.archive(s).is_some())
            .collect()
    }

    /// Adds a loose-file override (mods). Overrides win over every archive.
    pub fn add_override(&mut self, name: &str, data: Vec<u8>) {
        self.overrides.insert(key(name), data);
    }

    /// Names of the loose files found next to the archives.
    pub fn loose_files(&self) -> impl Iterator<Item = &str> {
        self.loose.keys().map(String::as_str)
    }

    /// Resolves a resource following the documented lookup order.
    pub fn read(&self, side: Side, name: &str) -> Option<Vec<u8>> {
        let k = key(name);
        if let Some(d) = self.overrides.get(&k) {
            return Some(d.clone());
        }
        if let Some(d) = self.archive(side).and_then(|a| a.read(&k)) {
            return Some(d);
        }
        if side != Side::Intro {
            if let Some(d) = self.intro.as_ref().and_then(|a| a.read(&k)) {
                return Some(d);
            }
        }
        self.loose.get(&k).cloned()
    }

    /// Like [`GameData::read`] but returns an error naming the resource.
    pub fn require(&self, side: Side, name: &str) -> Result<Vec<u8>> {
        self.read(side, name)
            .ok_or_else(|| FormatError::NotFound(name.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cc::CcBuilder;

    fn cc(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut b = CcBuilder::new();
        for (n, d) in entries {
            b.add(n, d.to_vec()).unwrap();
        }
        b.build(true).unwrap()
    }

    #[test]
    fn detects_variants() {
        let x = cc(&[("fnt", b"x")]);
        let d = cc(&[("fnt", b"d")]);
        let g = GameData::from_files([("XEEN.CC", x.clone()), ("dark.cc", d.clone())]).unwrap();
        assert_eq!(g.variant(), GameVariant::WorldOfXeen);
        let g = GameData::from_files([("dir/Dark.CC", d.clone())]).unwrap();
        assert_eq!(g.variant(), GameVariant::DarkSide);
        assert_eq!(g.default_side(), Side::Dark);
        let g = GameData::from_files([("swrd.cc", d), ("xeen.cc", x)]).unwrap();
        assert_eq!(g.variant(), GameVariant::Swords);
        assert!(GameData::from_files(Vec::<(String, Vec<u8>)>::new()).is_err());
    }

    #[test]
    fn lookup_order() {
        let x = cc(&[("fnt", b"xeen"), ("only.dat", b"x")]);
        let i = cc(&[("intro.pal", b"intro"), ("fnt", b"introfnt")]);
        let mut g = GameData::from_files([
            ("xeen.cc", x),
            ("intro.cc", i),
            ("readme.txt", b"loose".to_vec()),
        ])
        .unwrap();
        assert_eq!(g.read(Side::Clouds, "FNT").unwrap(), b"xeen");
        assert_eq!(g.read(Side::Clouds, "intro.pal").unwrap(), b"intro");
        assert_eq!(g.read(Side::Intro, "fnt").unwrap(), b"introfnt");
        assert_eq!(g.read(Side::Clouds, "readme.txt").unwrap(), b"loose");
        assert!(g.read(Side::Dark, "only.dat").is_none());
        g.add_override("fnt", b"mod".to_vec());
        assert_eq!(g.read(Side::Clouds, "fnt").unwrap(), b"mod");
        assert!(g.require(Side::Clouds, "missing").is_err());
    }
}

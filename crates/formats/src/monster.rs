// SPDX-License-Identifier: GPL-3.0-or-later
//! Monster tables (`xeen.mon`, `dark.mon`, `monsters.cld`, `monsters.swd`).
//!
//! 60-byte records, layout ported from ScummVM `MonsterStruct::synchronize`
//! (engines/mm/xeen/map.cpp, GPL-3.0-or-later). The table is read straight
//! from the original data: no statistics are hard-coded in OpenXeenNG.

use crate::error::{FormatError, Result};
use crate::io::{Reader, Writer};

/// Size of one monster record.
pub const MONSTER_RECORD_BYTES: usize = 60;

/// Damage types (`DamageType`).
pub const DAMAGE_TYPE_NAMES: [&str; 19] = [
    "physical",
    "magical",
    "fire",
    "electrical",
    "cold",
    "poison",
    "energy",
    "sleep",
    "finger of death",
    "holy word",
    "mass distortion",
    "undead",
    "beastmaster",
    "dragon sleep",
    "golem stopper",
    "hypnotize",
    "insect spray",
    "poison volley",
    "magic arrow",
];

/// Special attack effects (`SpecialAttack`).
pub const SPECIAL_ATTACK_NAMES: [&str; 23] = [
    "none",
    "magic",
    "fire",
    "elec",
    "cold",
    "poison",
    "energy",
    "disease",
    "insane",
    "sleep",
    "curse item",
    "in love",
    "drain sp",
    "curse",
    "paralyze",
    "unconscious",
    "confuse",
    "break weapon",
    "weaken",
    "eradicate",
    "aging",
    "death",
    "stone",
];

/// Monster families (`MonsterType`).
pub const MONSTER_TYPE_NAMES: [&str; 7] = [
    "monster", "animal", "insect", "humanoid", "undead", "golem", "dragon",
];

/// Class names used by `hates_class` (10+ are special values).
pub fn hates_class_name(v: u8) -> &'static str {
    match v {
        0 => "knight",
        1 => "paladin",
        2 => "archer",
        3 => "cleric",
        4 => "sorcerer",
        5 => "robber",
        6 => "ninja",
        7 => "barbarian",
        8 => "druid",
        9 => "ranger",
        12 => "dwarves",
        15 => "whole party",
        16 => "nobody",
        _ => "?",
    }
}

/// One monster definition.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Monster {
    /// Display name (15 characters max).
    pub name: String,
    /// Experience awarded.
    pub experience: u32,
    /// Hit points.
    pub hp: u16,
    /// Armor class.
    pub armor_class: u8,
    /// Speed (initiative).
    pub speed: u8,
    /// Attacks per round.
    pub number_of_attacks: u8,
    /// Preferred target class (see [`hates_class_name`]).
    pub hates_class: u8,
    /// Dice count of the damage roll.
    pub strikes: u16,
    /// Dice size of the damage roll.
    pub dmg_per_strike: u8,
    /// Damage type (index into [`DAMAGE_TYPE_NAMES`]).
    pub attack_type: u8,
    /// Special effect (index into [`SPECIAL_ATTACK_NAMES`]).
    pub special_attack: u8,
    /// Accuracy bonus.
    pub hit_chance: u8,
    /// Ranged attack flag/value.
    pub range_attack: u8,
    /// Family (index into [`MONSTER_TYPE_NAMES`]).
    pub monster_type: u8,
    /// Resistances: fire, electricity, cold, poison, energy, magic, physical.
    pub resistances: [u8; 7],
    /// Unknown byte kept for round-tripping (`_field29`).
    pub field29: u8,
    /// Gold carried.
    pub gold: u16,
    /// Gems carried.
    pub gems: u8,
    /// Item drop category.
    pub item_drop: u8,
    /// Flies over obstacles.
    pub flying: bool,
    /// Sprite number (`NNN.mon` / `NNN.att`).
    pub image_number: u8,
    /// Loop animation flag.
    pub loop_animation: u8,
    /// Animation effect.
    pub animation_effect: u8,
    /// Sound effect id.
    pub fx: u8,
    /// Attack voice sample (without `.voc`).
    pub attack_voc: String,
}

impl Monster {
    /// Parses one 60-byte record.
    pub fn parse(r: &mut Reader<'_>) -> Result<Self> {
        let name = r.fixed_str(16)?.chars().take(15).collect();
        let mut m = Monster {
            name,
            experience: r.u32()?,
            hp: r.u16()?,
            armor_class: r.u8()?,
            speed: r.u8()?,
            number_of_attacks: r.u8()?,
            hates_class: r.u8()?,
            strikes: r.u16()?,
            dmg_per_strike: r.u8()?,
            attack_type: r.u8()?,
            special_attack: r.u8()?,
            hit_chance: r.u8()?,
            range_attack: r.u8()?,
            monster_type: r.u8()?,
            ..Monster::default()
        };
        for v in m.resistances.iter_mut() {
            *v = r.u8()?;
        }
        m.field29 = r.u8()?;
        m.gold = r.u16()?;
        m.gems = r.u8()?;
        m.item_drop = r.u8()?;
        m.flying = r.u8()? != 0;
        m.image_number = r.u8()?;
        m.loop_animation = r.u8()?;
        m.animation_effect = r.u8()?;
        m.fx = r.u8()?;
        m.attack_voc = r.fixed_str(9)?;
        if m.item_drop > 20 {
            // ScummVM asserts 0 <= itemDrop <= 20.
            return Err(FormatError::invalid(
                "monster record",
                format!("{}: item drop {} out of range", m.name, m.item_drop),
            ));
        }
        Ok(m)
    }

    /// Serialises one record.
    pub fn write(&self, w: &mut Writer) {
        w.fixed_str(&self.name, 16)
            .u32(self.experience)
            .u16(self.hp)
            .u8(self.armor_class)
            .u8(self.speed)
            .u8(self.number_of_attacks)
            .u8(self.hates_class)
            .u16(self.strikes)
            .u8(self.dmg_per_strike)
            .u8(self.attack_type)
            .u8(self.special_attack)
            .u8(self.hit_chance)
            .u8(self.range_attack)
            .u8(self.monster_type)
            .bytes(&self.resistances)
            .u8(self.field29)
            .u16(self.gold)
            .u8(self.gems)
            .u8(self.item_drop)
            .u8(u8::from(self.flying))
            .u8(self.image_number)
            .u8(self.loop_animation)
            .u8(self.animation_effect)
            .u8(self.fx)
            .fixed_str(&self.attack_voc, 9);
    }
}

/// Parses a whole monster table. The engine reads records until the end of
/// the stream; a trailing partial record is reported as an error.
pub fn parse_monster_table(data: &[u8]) -> Result<Vec<Monster>> {
    if data.len() % MONSTER_RECORD_BYTES != 0 {
        return Err(FormatError::invalid(
            "monster table",
            format!(
                "{} bytes is not a multiple of {MONSTER_RECORD_BYTES}",
                data.len()
            ),
        ));
    }
    let mut r = Reader::new(data, "monster table");
    let mut out = Vec::with_capacity(data.len() / MONSTER_RECORD_BYTES);
    while !r.at_end() {
        out.push(Monster::parse(&mut r)?);
    }
    Ok(out)
}

/// Serialises a monster table.
pub fn monster_table_to_bytes(monsters: &[Monster]) -> Vec<u8> {
    let mut w = Writer::new();
    for m in monsters {
        m.write(&mut w);
    }
    w.into_inner()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_is_60_bytes_and_round_trips() {
        let m = Monster {
            name: "Test Goblin".into(),
            experience: 150,
            hp: 12,
            armor_class: 3,
            speed: 9,
            number_of_attacks: 1,
            hates_class: 15,
            strikes: 1,
            dmg_per_strike: 6,
            attack_type: 0,
            special_attack: 0,
            hit_chance: 5,
            range_attack: 0,
            monster_type: 3,
            resistances: [0, 0, 0, 10, 0, 0, 0],
            field29: 0,
            gold: 20,
            gems: 1,
            item_drop: 2,
            flying: false,
            image_number: 7,
            loop_animation: 0,
            animation_effect: 0,
            fx: 3,
            attack_voc: "goblin".into(),
        };
        let bytes = monster_table_to_bytes(&[m.clone(), m.clone()]);
        assert_eq!(bytes.len(), 120);
        assert_eq!(parse_monster_table(&bytes).unwrap(), vec![m.clone(), m]);
        assert!(parse_monster_table(&bytes[..100]).is_err());
    }
}

// SPDX-License-Identifier: GPL-3.0-or-later
//! Resource name hashing and name recovery for CC archives.
//!
//! CC archives do not store file names, only a 16-bit hash of each name.
//! The hash below is ported from ScummVM (`BaseCCArchive::convertNameToId`,
//! engines/mm/shared/xeen/cc_archive.cpp, GPL-3.0-or-later). To list an
//! archive with readable names we hash a dictionary of names known to be
//! used by the game and match them against the archive index.

use std::collections::HashMap;

use crate::tables;

/// Hashes a resource name into the 16-bit id stored in CC archive indexes.
///
/// Behaviour matches the original engine as reimplemented by ScummVM:
/// * only the base name is used (anything up to the last `/` or `\` is dropped);
/// * the name is upper-cased (ASCII only);
/// * an empty name, or one ending in `.`, maps to `0xFFFF`;
/// * a 4-character name that parses entirely as a C `strtol(.., 16)` number is
///   taken literally as the id (e.g. `"2a0c"` → `0x2A0C`);
/// * otherwise the running total is rotated right by 7 bits (16-bit) before
///   each following character is added.
pub fn name_to_id(name: &str) -> u16 {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let upper = base.to_ascii_uppercase();
    let bytes = upper.as_bytes();
    if bytes.is_empty() || bytes.ends_with(b".") {
        return 0xFFFF;
    }
    if bytes.len() == 4 {
        if let Some(num) = strtol16_whole(bytes) {
            // C cast of a long to uint16: two's complement truncation.
            return num as u16;
        }
    }
    let mut total: u32 = u32::from(bytes[0]);
    for &c in &bytes[1..] {
        total = ((total & 0x007F) << 9) | ((total & 0xFF80) >> 7);
        total += u32::from(c);
    }
    total as u16
}

/// Emulates `strtol(s, &end, 16)` and returns the value only when the whole
/// input was consumed (`*end == '\0'`), like the ScummVM check does.
fn strtol16_whole(s: &[u8]) -> Option<i64> {
    let mut i = 0;
    while i < s.len() && matches!(s[i], b' ' | b'\t' | b'\n' | b'\r' | 0x0B | 0x0C) {
        i += 1;
    }
    let mut negative = false;
    if i < s.len() && (s[i] == b'+' || s[i] == b'-') {
        negative = s[i] == b'-';
        i += 1;
    }
    // An optional "0x" prefix is only consumed when a hex digit follows it.
    if s.len() >= i + 3
        && s[i] == b'0'
        && (s[i + 1] == b'x' || s[i + 1] == b'X')
        && s[i + 2].is_ascii_hexdigit()
    {
        i += 2;
    }
    let digits_start = i;
    let mut value: i64 = 0;
    while i < s.len() && s[i].is_ascii_hexdigit() {
        value = value * 16 + i64::from((s[i] as char).to_digit(16).unwrap_or(0));
        i += 1;
    }
    if i == digits_start || i != s.len() {
        // No conversion performed (end pointer = start), or trailing junk.
        return None;
    }
    Some(if negative { -value } else { value })
}

/// Literal resource names referenced by the game engine (collected from the
/// ScummVM Xeen engine sources). Numbered families are generated separately
/// by [`candidate_names`].
pub static KNOWN_NAMES: &[&str] = &[
    "fnt",
    "1room.raw", "2room.raw", "3room.raw", "admit2.voc", "ahh.voc", "airmon.voc",
    "alamar1.voc", "alien2.voc", "animinfo.cld", "award.bin", "award.icn", "back.raw",
    "back2.voc", "ball.int", "ball.raw", "bank.icn", "bank.m", "bank1.voc", "bank2.icn",
    "banker.voc", "bigsky.end", "bigtheme.m", "bird.raw", "blank.raw", "bless.icn",
    "border.icn", "box.vga", "branch.voc", "cast.icn", "cast.voc", "castle.m", "cavern.m",
    "cavern1.m", "cavern2.m", "cavern3a.m", "ch1.voc", "charpow.icn", "choice.icn",
    "choice2.voc", "choose.icn", "claw.int", "click.voc", "cloud.srf", "clouds.dat",
    "coina.voc", "combat.icn", "come1.voc", "come2.voc", "comet.voc", "confirm.icn",
    "corak2.voc", "cpanel.icn", "create.icn", "create.raw", "create2.icn", "croad.srf",
    "crodo.end", "crodo.vga", "crodo1.voc", "crodo2.voc", "crodo3.voc", "crodo4a.voc",
    "crodo4b.voc", "crodo4c.voc", "crodo5.voc", "crodo6.voc", "cstl1rev.m", "cstl2rev.m",
    "cstl3rev.m", "cube.eg2", "dark.dat", "dark.mon", "dark.pal", "dark1.voc", "dark2.voc",
    "dark3.voc", "darklaff.voc", "darklord.int", "darkpic.dat", "daydesrt.m", "death.vga",
    "death1.vga", "desert.srf", "detctmon.icn", "detectmn.icn", "dice.vga", "dirt.srf",
    "disint.voc", "disk.end", "diska.end", "diskb.end", "diskc.end", "diskd.end", "diske.end",
    "dngon1.m", "dngon2.m", "dngon3.m", "door.voc", "dragon.int", "dragon1.int", "dragon1.voc",
    "dragon2.voc", "dragon3.voc", "dreams2.voc", "drop.raw", "dse.fac", "dung1.zom",
    "dung2.zom", "dung3.zom", "dungeon.m", "dungeon2.m", "dwarf1.vga", "dwarf10.voc",
    "dwarf11.voc", "dwarf12.voc", "dwarf2.vga", "dwarf3.vga", "dwater.srf", "eg100001.raw",
    "eg140001.raw", "eg23prt2.raw", "eg250001.pal", "eg250001.raw", "eg270001.raw",
    "eg280001.raw", "elect.voc", "element.icn", "ellinger.voc", "end.raw", "end1.int",
    "end2.int", "end3.int", "endgame.m", "endgame.pal", "endtext.end", "equip.icn", "esc.icn",
    "explosio.voc", "face.end", "fail1.voc", "fecp.brd", "fight2.voc", "fire.voc", "fire1.vga",
    "fire2.vga", "fire3.vga", "fire4.vga", "firemain.pal", "firemain.raw", "fizzle.int",
    "fly.int", "fly.pal", "fly.raw", "foura.raw", "frame.fac", "freap.vga", "gascompr.voc",
    "gdluck1.voc", "global.icn", "go2.voc", "goin1.voc", "golem.vga", "golem13.voc",
    "golem14.voc", "golem15.voc", "golem16.voc", "golmback.vga", "goodbye.voc", "goon.int",
    "grass.srf", "grounds.m", "group.vga", "groupo.vga", "gtlost.voc", "guild.m",
    "guild10.voc", "guild11.voc", "guild12.voc", "gulp.voc", "hands.eg2", "hello.voc",
    "hello1.voc", "help2.voc", "howdid1.voc", "hpbars.icn", "iamking.voc", "ido2.voc",
    "inn.icn", "inn.m", "intro.pal", "intro.raw", "intro.vga", "intro1.pal", "intro1.vga",
    "jvc.raw", "key2.voc", "king.end", "king1.voc", "king2.voc", "king3.voc", "king4.voc",
    "kingcord.end", "kludge.int", "laff1.voc", "lake.vga", "lake2.vga", "lake3.vga",
    "later.raw", "lava.srf", "lloyds.icn", "logo.int", "logo.vga", "logo1.vga", "logobak.raw",
    "lose.int", "mae.cld", "mae.xen", "main.icn", "mainback.raw", "mapnames.cld", "marb.raw",
    "maywe2.voc", "maze.chr", "maze.pty", "meangro&.voc", "miror-s.raw", "mirrback.end",
    "mirror.end", "mirror.pal", "miss.voc", "mm.dat", "mm4.pal", "mm4e.pal", "mm4theme.m",
    "monsters.cld", "monsters.swd", "mouse.icn", "needkey1.voc", "newbrigh.m", "night.sky",
    "nordo2.voc", "nowre1.voc", "null.voc", "nwblksmt.m", "nwc1.int", "nwc1.raw", "nwc2.int",
    "nwc2.raw", "nwc3.int", "nwc4.int", "ogre.voc", "ok2.voc", "outday1.m", "outday2.m",
    "outday3.m", "outday4.m", "outdoor.til", "outdoors.m", "outnght1.m", "outnght2.m",
    "outnght4.m", "padspell.voc", "parrot1.voc", "parrot2.voc", "pass2.voc", "people.end",
    "pharoh11.voc", "pharoh1a.voc", "pharoh1b.voc", "pharoh2a.voc", "pharoh2b.voc",
    "pharoh2c.voc", "pharoh2d.voc", "pharoh2e.voc", "pharoh3.voc", "pharoh4.voc",
    "pharoh5a.voc", "pharoh5b.voc", "pharoh6w.voc", "pharoh7t.voc", "pharoh8.voc",
    "pharoh9a.voc", "pharoh9b.voc", "photon.voc", "prec.end", "prtygd.voc", "pyramid.int",
    "pyramid2.raw", "pyramid3.raw", "pyratop.int", "qnotes.bin", "queen.int", "queen1.voc",
    "queen2.voc", "queenhis.voc", "quest.icn", "ready2.voc", "reaper12.voc", "reaper13.voc",
    "reaper14.voc", "reaper15.voc", "restorex.icn", "road.srf", "room.end", "rumble.voc",
    "sc02.eg2", "sc03a.end", "sc03b.end", "sc03c.end", "sc050001.raw", "sc06.eg2", "sc06a.end",
    "sc06b.end", "sc070001.raw", "sc07a.end", "sc07b.end", "sc07c.end", "sc07d.end",
    "sc07e.end", "sc07f.end", "sc07g.end", "sc07h.end", "sc08.end", "sc09.end", "sc090001.raw",
    "sc10.eg2", "sc10.end", "sc11.end", "sc13.eg2", "sc13.end", "sc14.eg2", "sc14.end",
    "sc15.end", "sc16.end", "sc17.eg2", "sc170001.raw", "sc17xa.end", "sc17xb.end",
    "sc180001.raw", "sc18a.end", "sc18b.end", "sc18c.end", "sc18d.end", "sc19.end",
    "sc190001.raw", "sc20.end", "sc20a.eg2", "sc20b.eg2", "sc20c.eg2", "sc20d.eg2", "sc21.end",
    "sc22.end", "sc22a.eg2", "sc22b.eg2", "sc230001.raw", "sc23a.eg2", "sc23a.end",
    "sc23b.eg2", "sc23b.end", "sc23c.eg2", "sc23d.eg2", "sc23e.eg2", "sc23f.eg2", "sc23g.eg2",
    "sc23h.eg2", "sc24.eg2", "sc24a.end", "sc24b.end", "sc25.eg2", "sc250001.raw", "sc25a.end",
    "sc260001.raw", "sc261a.eg2", "sc261b.eg2", "sc262.eg2", "sc263.eg2", "sc264.eg2",
    "sc26a.end", "sc26b.end", "sc26c.end", "sc26d.end", "sc27.eg2", "sc27.end", "sc270001.raw",
    "sc28a.eg2", "sc28b1.eg2", "sc28c.eg2", "sc28d.eg2", "sc28e.eg2", "sc28f.eg2", "sc28g.eg2",
    "sc28h.eg2", "sc28i.eg2", "sc28j.eg2", "sc28k.eg2", "sc28l.eg2", "sc28m.eg2", "sc28n.eg2",
    "sc290001.raw", "sc29a.end", "sc29b.end", "sc29c.end", "sc29d.end", "sc29e.end",
    "sc29f.end", "sc30.eg2", "sc3a.eg2", "sc3b1.eg2", "sc3b2.eg2", "sca28.end", "scb28.end",
    "scc28.end", "scd28.end", "sce28.end", "scene1.raw", "scene12.raw", "scene2-b.raw",
    "scene4-1.raw", "scene4.raw", "scf28.end", "scg28.end", "sch28.end", "sci28.end",
    "sck28.end", "scortch.srf", "scr.pal", "scream.voc", "screech.voc", "scroll.icn",
    "see2.voc", "sewer.srf", "sf05.m", "sf09.m", "sf17.m", "skull1.voc", "sky.sky", "sky.srf",
    "skymain.pal", "skymain.raw", "smith.m", "snow.srf", "space.srf", "special.bin",
    "special.icn", "spellfx.icn", "spells.cld", "spells.xen", "sphinx.vga", "sphinx10.voc",
    "sphinx11.voc", "sphinx12.voc", "sphinx13.voc", "sphinx14.voc", "spit1.voc", "spldesc.bin",
    "staff.eg2", "stars.vga", "start.icn", "start.int", "swamp.srf", "tablmain.raw",
    "tavern.bin", "tavern.icn", "tavern.m", "temple.m", "tflr.srf", "thanks2.voc",
    "thankyou.voc", "throne1.raw", "throne2.raw", "thud.voc", "thud1.voc", "tiger2&.voc",
    "timer.drv", "title.int", "title2.int", "title2a.int", "title2b.int", "title2b.raw",
    "title2c.int", "title2d.int", "title2e.int", "title2f.int", "title2g.int", "title2h.int",
    "title2i.int", "tower.vga", "tower1.eg2", "tower1.zom", "tower2.eg2", "tower2.zom",
    "town.m", "town1.zom", "town2.zom", "town3.zom", "townday1.m", "train.icn", "trainin0.voc",
    "trainin1.voc", "trainin2.voc", "training.voc", "twnnitea.m", "twnniteb.m", "twnwlk.m",
    "twrsky1.raw", "unnh.voc", "version.txt", "view.icn", "vine.voc", "wallpics.cld",
    "water.out", "water.srf", "whaddayo.voc", "what2.voc", "what3.voc", "whereto.voc",
    "white.pal", "whoosh.voc", "win.int", "windstor.voc", "wizard.int", "wizmth.int",
    "wiztower.vga", "wiztwer2.vga", "world.int", "world.raw", "world0.int", "world1.int",
    "world2.int", "xeen.mon", "xeen.vga", "xeen1.vga", "xeenlaff.voc", "xeenmirr.ext",
    "xeenmirr.txt", "xeenpic.dat", "ya1.voc", "yes1.voc", "youtrn1.voc",
];

/// Every name the dictionary can recover: literal names plus the numbered
/// families (maps, monster/object sprites, portraits, terrain sets, ...).
pub fn candidate_names() -> Vec<String> {
    let mut out: Vec<String> = KNOWN_NAMES.iter().map(|s| s.to_string()).collect();
    let map_id = |id: u32| -> String {
        if id >= 100 {
            format!("x{:03}", id)
        } else {
            format!("0{:03}", id)
        }
    };
    for id in 0..=255u32 {
        let m = map_id(id);
        for ext in ["dat", "mob", "evt"] {
            out.push(format!("maze{m}.{ext}"));
        }
        for ext in ["hed", "txt"] {
            out.push(format!("aaze{m}.{ext}"));
        }
        out.push(format!("xeen{m}.txt"));
        out.push(format!("dark{m}.txt"));
        out.push(format!("aaze2{id:03}.txt"));
        out.push(format!("{id:03}.mon"));
        out.push(format!("{id:03}.att"));
        out.push(format!("{id:03}.pic"));
        // Object sprites >= 100 use the ".0bj" extension (digit zero).
        out.push(format!("{id:03}.{}bj", if id >= 100 { '0' } else { 'o' }));
    }
    for n in 0..100u32 {
        for fam in ["char", "face", "hand", "marb", "bird", "casb", "cast"] {
            let ext = match fam {
                "char" | "face" => "fac",
                "hand" | "marb" => "vga",
                "bird" => "int",
                _ => "end",
            };
            out.push(format!("{fam}{n:02}.{ext}"));
        }
        out.push(format!("prec00{n:02}.frm"));
        out.push(format!("vort{n:02}.frm"));
    }
    for n in 0..10u32 {
        out.push(format!("drop{n}.int"));
        out.push(format!("pow{n}.icn"));
        out.push(format!("pow{n}.voc"));
        out.push(format!("wfire{n}.int"));
        out.push(format!("darklrd{n}.end"));
    }
    for t in tables::TERRAIN_TYPES {
        for n in 1..=4 {
            out.push(format!("f{t}{n}.fwl"));
        }
        out.push(format!("s{t}.swl"));
        for ext in ["sky", "gnd", "til", "icn"] {
            out.push(format!("{t}.{ext}"));
        }
        for n in 0..10 {
            out.push(format!("{t}{n}.twn"));
        }
    }
    for w in tables::OUTDOORS_WALL_TYPES.iter().flatten() {
        out.push(format!("{w}.wal"));
    }
    for s in tables::SURFACE_NAMES {
        out.push(s.to_string());
    }
    out.extend(tables::MUSIC_FILES1.iter().map(|s| s.to_string()));
    for row in tables::MUSIC_FILES2 {
        out.extend(row.iter().map(|s| s.to_string()));
    }
    for side in ["xeen", "dark"] {
        out.push(format!("{side}mirr.txt"));
        out.push(format!("{side}mirr.ext"));
    }
    out.sort();
    out.dedup();
    out
}

/// Maps archive ids back to the candidate names that hash to them.
#[derive(Debug, Clone, Default)]
pub struct NameIndex {
    map: HashMap<u16, Vec<String>>,
}

impl NameIndex {
    /// Builds the index from [`candidate_names`].
    pub fn builtin() -> Self {
        Self::from_names(candidate_names())
    }

    /// Builds an index from an arbitrary list of names.
    pub fn from_names<I, S>(names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut idx = NameIndex::default();
        for n in names {
            idx.insert(n.into());
        }
        idx
    }

    /// Adds a name (e.g. discovered from monster data at runtime).
    pub fn insert(&mut self, name: String) {
        let id = name_to_id(&name);
        let list = self.map.entry(id).or_default();
        if !list.iter().any(|n| n.eq_ignore_ascii_case(&name)) {
            list.push(name);
        }
    }

    /// All names hashing to `id` (collisions are possible).
    pub fn names_for(&self, id: u16) -> &[String] {
        self.map.get(&id).map(Vec::as_slice).unwrap_or(&[])
    }

    /// The first known name for `id`, if any.
    pub fn lookup(&self, id: u16) -> Option<&str> {
        self.names_for(id).first().map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Straight transcription of the ScummVM loop, used as a reference.
    fn reference_hash(name: &str) -> u16 {
        let up = name.to_ascii_uppercase();
        let b = up.as_bytes();
        let mut total: i32 = b[0] as i32;
        for &c in &b[1..] {
            total = (total & 0x007F) << 9 | (total & 0xFF80) >> 7;
            total += c as i32;
        }
        total as u16
    }

    #[test]
    fn hash_matches_reference_loop() {
        for n in ["mm4.pal", "xeen.mon", "maze0001.dat", "fnt", "a", "zz", "outdoor.til"] {
            assert_eq!(name_to_id(n), reference_hash(n), "{n}");
        }
    }

    #[test]
    fn hash_is_case_insensitive_and_uses_basename() {
        assert_eq!(name_to_id("MM4.PAL"), name_to_id("mm4.pal"));
        assert_eq!(name_to_id("dir/sub\\mm4.pal"), name_to_id("mm4.pal"));
    }

    #[test]
    fn four_hex_digit_names_are_literal_ids() {
        assert_eq!(name_to_id("2a0c"), 0x2A0C);
        assert_eq!(name_to_id("284C"), 0x284C);
        // "face" is valid hex too: the engine treats it as an id.
        assert_eq!(name_to_id("face"), 0xFACE);
        // Not hex: hashed normally.
        assert_eq!(name_to_id("fnt1"), reference_hash("fnt1"));
        // strtol accepts a sign and a 0x prefix.
        assert_eq!(name_to_id("0x1f"), 0x1F);
        assert_eq!(name_to_id("-001"), 0xFFFF);
        assert_eq!(name_to_id("+abc"), 0x0ABC);
    }

    #[test]
    fn empty_or_trailing_dot_is_ffff() {
        assert_eq!(name_to_id(""), 0xFFFF);
        assert_eq!(name_to_id("abc."), 0xFFFF);
    }

    #[test]
    fn dictionary_contains_generated_families() {
        let names = candidate_names();
        for n in ["maze0001.dat", "mazex255.evt", "aaze0042.hed", "101.0bj", "001.obj", "fcave1.fwl", "scstl.swl", "outdoors.m", "mount.wal"] {
            assert!(names.iter().any(|x| x == n), "{n} missing");
        }
        let idx = NameIndex::builtin();
        assert!(idx.names_for(name_to_id("mm4.pal")).iter().any(|n| n == "mm4.pal"));
    }
}

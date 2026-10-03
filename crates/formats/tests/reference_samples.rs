// SPDX-License-Identifier: GPL-3.0-or-later
//! Checks the parsers against real-format sample files.
//!
//! Set `OPENXEENNG_REF_DIR` to ScummVM's `devtools/create_mm/files/xeen`
//! directory (the CI workflow sparse-checks it out). Without it the test is
//! skipped. The samples are not copied into this repository.

use std::path::PathBuf;

use openxeenng_formats::{evt, maze, misc, mob, monster};

fn ref_dir() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("OPENXEENNG_REF_DIR")?);
    dir.is_dir().then_some(dir)
}

fn read(dir: &std::path::Path, name: &str) -> Vec<u8> {
    std::fs::read(dir.join(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

#[test]
fn scummvm_reference_samples_parse_and_round_trip() {
    let Some(dir) = ref_dir() else {
        eprintln!("OPENXEENNG_REF_DIR not set; skipping reference sample checks");
        return;
    };

    let dat = read(&dir, "mazex255.dat");
    let m = maze::MazeData::parse(&dat).unwrap();
    assert_eq!(m.to_bytes(), dat, "maze .dat must round-trip byte for byte");

    let hed = read(&dir, "aazex255.hed");
    assert_eq!(misc::HeadData::parse(&hed).unwrap().to_bytes(), hed);

    // ScummVM's generated isle script is damaged: the "give gems" record of
    // the second bench declares 9 parameters but carries 7 (BENCH2_EVENTS in
    // devtools/create_mm/create_xeen/map.cpp), so it swallows the following
    // Exit record and leaves a 4-byte tail. The strict parser reports it and
    // the lenient one keeps every complete record.
    let ev_bytes = read(&dir, "mazex255.evt");
    assert!(evt::parse_events(&ev_bytes).is_err());
    let (ev, err) = evt::parse_events_lenient(&ev_bytes);
    assert!(err.is_some());
    assert_eq!(ev.len(), 9);
    assert!(ev.iter().all(|e| evt::opcode_name(e.opcode).is_some()));
    assert_eq!(evt::events_to_bytes(&ev).unwrap(), ev_bytes[..90].to_vec());

    let texts = misc::parse_string_table(&read(&dir, "aazex255.txt"));
    assert_eq!(texts[1], "Isle of ScummVM");
    assert_eq!(
        misc::parse_maze_name(&read(&dir, "xeenx255.txt")),
        "ScummVM"
    );

    let mob = mob::MobFile::parse(&read(&dir, "mazex255.mob")).unwrap();
    assert_eq!(mob.objects.len(), 3);

    for name in ["monsters.cld", "monsters.swd"] {
        let bytes = read(&dir, name);
        let table = monster::parse_monster_table(&bytes).unwrap();
        assert!(!table.is_empty(), "{name}");
        // Slot 0 of the Swords table is an unused blank record.
        assert!(table.iter().skip(1).all(|m| !m.name.is_empty()), "{name}");
        assert_eq!(monster::monster_table_to_bytes(&table), bytes, "{name}");
    }

    let anim = read(&dir, "animinfo.cld");
    let entries = misc::parse_animation_info(&anim).unwrap();
    assert_eq!(entries.len() * 12, anim.len());
    assert_eq!(misc::animation_info_to_bytes(&entries), anim);
}

// SPDX-License-Identifier: GPL-3.0-or-later
//! Differential tests against ScummVM's own code.
//!
//! `oracle/build.sh` compiles `xeen_oracle`, a harness around functions
//! extracted verbatim from ScummVM's Xeen engine. These tests feed the same
//! randomly generated inputs to the oracle and to the Rust ports and require
//! identical output. Set `OPENXEEN_ORACLE` to the harness binary; without it
//! the tests are skipped.

use std::io::Write;
use std::process::{Command, Stdio};

use openxeen_engine::gfx::{self, DrawContext, Rect, SCALE_ENLARGE, Surface};
use openxeen_engine::rng::{RandomSource, ScummRandom};
use openxeen_formats::cc::{CcBuilder, parse_index};
use openxeen_formats::names::{candidate_names, name_to_id};
use openxeen_formats::sprite::SpriteFile;

fn oracle() -> Option<String> {
    std::env::var("OPENXEEN_ORACLE").ok().filter(|p| std::path::Path::new(p).exists())
}

fn run_oracle(bin: &str, args: &[&str], stdin: &[u8]) -> Vec<u8> {
    let mut child = Command::new(bin)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn oracle");
    // Feed stdin from a thread: the oracle's output can exceed the pipe
    // buffer before it has consumed all of its input.
    let mut pipe = child.stdin.take().expect("stdin");
    let input = stdin.to_vec();
    let writer = std::thread::spawn(move || pipe.write_all(&input));
    let out = child.wait_with_output().expect("oracle output");
    writer.join().expect("writer thread").expect("write oracle stdin");
    assert!(out.status.success(), "oracle failed: {:?} {args:?}", out.status);
    out.stdout
}

/// Small deterministic generator for test inputs.
struct Gen(ScummRandom);

impl Gen {
    fn new(seed: u32) -> Self {
        Gen(ScummRandom::new(seed))
    }
    fn below(&mut self, n: u32) -> u32 {
        self.0.random_number(n.max(1) - 1)
    }
    fn range(&mut self, lo: i32, hi: i32) -> i32 {
        lo + self.below((hi - lo + 1) as u32) as i32
    }
    fn chance(&mut self, percent: u32) -> bool {
        self.below(100) < percent
    }
    fn byte(&mut self) -> u8 {
        self.below(256) as u8
    }
}

fn tmp(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("openxeen-oracle-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

#[test]
fn rng_matches_scummvm() {
    let Some(bin) = oracle() else { return };
    for seed in [1u32, 2, 12345, 0xDEAD_BEEF, 0] {
        let out = run_oracle(&bin, &["rng", &seed.to_string(), "3000"], b"");
        let expect: Vec<u32> = String::from_utf8(out)
            .unwrap()
            .lines()
            .map(|l| l.parse().unwrap())
            .collect();
        let mut r = ScummRandom::new(seed);
        let got: Vec<u32> = (0..3000).map(|_| r.random_number(0xFFFF)).collect();
        assert_eq!(got, expect, "seed {seed}");
    }
}

#[test]
fn name_hash_matches_scummvm() {
    let Some(bin) = oracle() else { return };
    let mut names = candidate_names();
    let mut g = Gen::new(99);
    let alphabet = b"abcdefghijklmnopqrstuvwxyzABCDEFXYZ0123456789._-+& x";
    for _ in 0..20_000 {
        let len = g.range(1, 12) as usize;
        let s: String = (0..len)
            .map(|_| alphabet[g.below(alphabet.len() as u32) as usize] as char)
            .collect();
        names.push(s);
    }
    names.extend(["0x1f", "-001", "+abc", "FACE", "2A0C", " 1f", "abc.", "x"].map(String::from));
    let input = names.join("\n") + "\n";
    let out = String::from_utf8(run_oracle(&bin, &["hash"], input.as_bytes())).unwrap();
    let expect: Vec<&str> = out.lines().collect();
    assert_eq!(expect.len(), names.len());
    for (n, e) in names.iter().zip(expect) {
        assert_eq!(format!("{:04X}", name_to_id(n)), e, "name {n:?}");
    }
}

#[test]
fn cc_index_matches_scummvm() {
    let Some(bin) = oracle() else { return };
    let mut g = Gen::new(7);
    for round in 0..40 {
        let mut b = CcBuilder::new();
        let n = g.range(1, 300);
        for _ in 0..n {
            let id = g.below(0x10000) as u16;
            let len = g.below(700) as usize;
            b.add_id(id, (0..len).map(|_| g.byte()).collect()).unwrap();
        }
        let bytes = b.build(round % 2 == 0).unwrap();
        let path = tmp(&format!("index{round}.cc"));
        std::fs::write(&path, &bytes).unwrap();
        let out = String::from_utf8(run_oracle(&bin, &["index", path.to_str().unwrap()], b"")).unwrap();
        let (entries, _) = parse_index(&bytes).unwrap();
        let ours: Vec<String> = entries
            .iter()
            .map(|e| format!("{} {} {}", e.id, e.offset, e.size))
            .collect();
        assert_eq!(ours, out.lines().collect::<Vec<_>>(), "round {round}");
    }
}

/// Builds a valid sprite resource using every opcode family, including the
/// pattern and stream-copy commands the OpenXeen encoder never emits.
fn random_sprite(g: &mut Gen) -> Vec<u8> {
    let frames = g.range(1, 4) as usize;
    let mut data: Vec<u8> = Vec::new();
    data.extend((frames as u16).to_le_bytes());
    data.resize(2 + frames * 4, 0);
    for f in 0..frames {
        let cells = if g.chance(30) { 2 } else { 1 };
        for c in 0..cells {
            let off = data.len() as u16;
            data[2 + f * 4 + c * 2..2 + f * 4 + c * 2 + 2].copy_from_slice(&off.to_le_bytes());
            let width = g.range(1, 80) as u16;
            let height = g.range(1, 60) as u16;
            for v in [g.range(0, 40) as u16, width, g.range(0, 30) as u16, height] {
                data.extend(v.to_le_bytes());
            }
            let mut remaining = i32::from(height);
            while remaining > 0 {
                if g.chance(12) {
                    let n = g.range(0, remaining.min(10)) as u8;
                    data.extend([0, n]);
                    remaining -= i32::from(n) + 1;
                    continue;
                }
                let line_start = data.len(); // position of the length byte
                let mut line = vec![g.range(0, i32::from(width) - 1) as u8];
                let mut pixels = 0;
                while line.len() < 180 && pixels < 240 && !(line.len() > 1 && g.chance(15)) {
                    match g.below(8) {
                        0 | 1 => {
                            let n = g.range(1, 64) as usize;
                            line.push((n - 1) as u8);
                            line.extend((0..n).map(|_| g.byte()));
                            pixels += n;
                        }
                        2 => {
                            let l = g.below(32) as u8;
                            line.extend([0x40 | l, g.byte()]);
                            pixels += usize::from(l) + 3;
                        }
                        3 => {
                            let l = g.below(32) as u8;
                            let n = usize::from(l) + 4;
                            if line_start < n {
                                continue;
                            }
                            let src = g.below((line_start - n + 1) as u32) as usize;
                            let pos_after = line_start + 1 + line.len() + 3;
                            let back = (pos_after - src) as u16;
                            line.push(0x60 | l);
                            line.extend(back.to_le_bytes());
                            pixels += n;
                        }
                        4 => {
                            let l = g.below(32) as u8;
                            line.extend([0x80 | l, g.byte(), g.byte()]);
                            pixels += 2 * (usize::from(l) + 2);
                        }
                        5 => {
                            let l = g.below(32) as u8;
                            line.push(0xA0 | l);
                            pixels += usize::from(l) + 1;
                        }
                        _ => {
                            line.extend([0xC0 | g.below(64) as u8, g.byte()]);
                            pixels += 10;
                        }
                    }
                }
                data.push(line.len() as u8);
                data.extend(line);
                remaining -= 1;
            }
        }
    }
    assert!(data.len() < 0xFFFF);
    data
}

#[test]
fn sprite_renderer_matches_scummvm() {
    let Some(bin) = oracle() else { return };
    let mut g = Gen::new(2024);
    let mut total_cases = 0;
    let mut drawn_pixels = 0usize;
    for round in 0..300 {
        let bytes = random_sprite(&mut g);
        let sprite = SpriteFile::parse(bytes.clone()).expect("generator makes valid sprites");
        let mut pal = [[0u8; 3]; 256];
        for c in pal.iter_mut() {
            if !g.chance(20) {
                *c = [g.byte(), g.byte(), g.byte()];
            }
        }
        if round % 10 == 0 {
            pal = [[0; 3]; 256]; // drawer 3 must stay inert without a palette
        }
        let sprite_path = tmp(&format!("s{round}.spr"));
        let pal_path = tmp(&format!("s{round}.pal"));
        std::fs::write(&sprite_path, &bytes).unwrap();
        std::fs::write(&pal_path, pal.iter().flatten().copied().collect::<Vec<u8>>()).unwrap();

        let mut cases = Vec::new();
        let mut script = String::new();
        for _ in 0..40 {
            let frame = g.range(0, sprite.frame_count() as i32) as usize; // may be out of range
            let pos = if g.chance(60) {
                (g.range(0, 240), g.range(0, 140))
            } else {
                (g.range(-60, 300), g.range(-40, 190))
            };
            let bounds = if g.chance(40) {
                Rect::new(0, 0, 320, 200)
            } else {
                let l = g.range(0, 319);
                let t = g.range(0, 199);
                Rect::new(l, t, g.range(l, 320), g.range(t, 200))
            };
            let drawer = g.below(7);
            let idx = match drawer {
                1 => g.below(24),
                2 | 6 => g.below(16),
                3..=5 => g.below(4),
                _ => g.below(32),
            };
            let mut flags = (drawer << 8) | idx;
            for bit in [gfx::flags::SCENE_CLIPPED, gfx::flags::BOTTOM_CLIPPED, gfx::flags::HORIZ_FLIPPED] {
                if g.chance(30) {
                    flags |= bit;
                }
            }
            let mut scale = g.below(16) as u16;
            if g.chance(20) {
                scale |= SCALE_ENLARGE;
                if g.chance(50) {
                    scale = SCALE_ENLARGE;
                }
            }
            let seed = g.below(u32::MAX) | 1;
            let clipped_bottom = g.range(100, 200);
            script.push_str(&format!(
                "{frame} {} {} {} {} {} {} {flags} {scale} {seed} {clipped_bottom}\n",
                pos.0, pos.1, bounds.left, bounds.top, bounds.right, bounds.bottom
            ));
            cases.push((frame, pos, bounds, flags, scale, seed, clipped_bottom));
        }
        let out = run_oracle(
            &bin,
            &["draw", sprite_path.to_str().unwrap(), pal_path.to_str().unwrap()],
            script.as_bytes(),
        );
        assert_eq!(out.len(), cases.len() * 64000);
        for (i, (frame, pos, bounds, flags, scale, seed, cb)) in cases.into_iter().enumerate() {
            let mut dest = Surface::screen();
            for (k, p) in dest.pixels_mut().iter_mut().enumerate() {
                *p = ((k * 7 + 3) & 0xFF) as u8;
            }
            let before = dest.clone();
            let mut rng = ScummRandom::new(seed);
            let mut ctx = DrawContext {
                rng: &mut rng,
                palette: &pal,
                clipped_bottom: cb,
            };
            gfx::draw_sprite(&mut dest, &sprite, frame, pos, bounds, flags, scale, &mut ctx)
                .expect("valid sprite draws");
            let expect = &out[i * 64000..(i + 1) * 64000];
            if dest.pixels() != expect {
                let first = dest
                    .pixels()
                    .iter()
                    .zip(expect)
                    .position(|(a, b)| a != b)
                    .unwrap_or(0);
                panic!(
                    "round {round} case {i}: frame {frame} pos {pos:?} bounds {bounds:?} flags {flags:#x} \
                     scale {scale:#x}: first difference at ({}, {}): ours {} scummvm {}",
                    first % 320,
                    first / 320,
                    dest.pixels()[first],
                    expect[first]
                );
            }
            drawn_pixels += dest.pixels().iter().zip(before.pixels()).filter(|(a, b)| a != b).count();
            total_cases += 1;
        }
    }
    assert!(drawn_pixels > 300_000, "cases barely drew anything ({drawn_pixels} px)");
    eprintln!("{total_cases} draw cases identical to ScummVM ({drawn_pixels} pixels changed)");
}

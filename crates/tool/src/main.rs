// SPDX-License-Identifier: GPL-3.0-or-later
//! `xeentool`: command-line companion of OpenXeenNG.
//!
//! Inspects, extracts and rebuilds World of Xeen data files, converts
//! graphics to PNG, disassembles map scripts and renders headless engine
//! screenshots. Run `xeentool help` for the command list.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use openxeenng_engine::gfx::Surface;
use openxeenng_engine::present::{default_palette, level_lut};
use openxeenng_engine::scenes::browser::classify;
use openxeenng_engine::{Engine, EngineConfig, InputEvent, Key, synth};
use openxeenng_formats::cc::{CcArchive, CcBuilder};
use openxeenng_formats::names::{NameIndex, name_to_id};
use openxeenng_formats::palette::{DacScaling, VgaPalette};
use openxeenng_formats::sprite::SpriteFile;
use openxeenng_formats::{evt, misc};

type Res<T> = Result<T, String>;

const HELP: &str = "\
xeentool - World of Xeen data tool (OpenXeenNG)

USAGE:
  xeentool list <file.cc> [--unencoded]
  xeentool extract <file.cc> <out-dir> [--unencoded]
  xeentool pack <dir> <out.cc> [--unencoded]
  xeentool hash <name>...
  xeentool sprite <file.cc> <name> <out.png> [--palette <name>] [--scale 1|2|3]
  xeentool palette <file.cc> <name.pal> <out.png>
  xeentool raw <file.cc> <name.raw> <out.png> [--palette <name>]
  xeentool evt <file.cc> <mazeNNNN.evt>
  xeentool synth <out-dir>
  xeentool screenshot <out.png> [--data <dir>] [--frames N] [--keys K1,K2,...]

Notes:
  * Archives are XOR-encoded unless --unencoded (save files, .CUR).
  * Names are case-insensitive; unknown entries appear as #XXXX (their id).
  * --palette defaults to mm4.pal from the same archive (built-in if absent).
  * screenshot keys: up,down,left,right,enter,esc,space,tab,pgup,pgdn or a letter.
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

struct Opts {
    positional: Vec<String>,
    flags: HashMap<String, Option<String>>,
}

fn parse_opts(args: &[String]) -> Opts {
    let mut positional = Vec::new();
    let mut flags = HashMap::new();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if let Some(name) = a.strip_prefix("--") {
            let takes_value = matches!(name, "palette" | "scale" | "data" | "frames" | "keys");
            if takes_value && i + 1 < args.len() {
                flags.insert(name.to_string(), Some(args[i + 1].clone()));
                i += 2;
                continue;
            }
            flags.insert(name.to_string(), None);
        } else {
            positional.push(a.clone());
        }
        i += 1;
    }
    Opts { positional, flags }
}

fn run(args: &[String]) -> Res<()> {
    let Some(cmd) = args.first() else {
        print!("{HELP}");
        return Ok(());
    };
    let o = parse_opts(&args[1..]);
    let p = |i: usize, what: &str| -> Res<&str> {
        o.positional
            .get(i)
            .map(String::as_str)
            .ok_or_else(|| format!("missing <{what}> (see `xeentool help`)"))
    };
    let encoded = !o.flags.contains_key("unencoded");
    match cmd.as_str() {
        "help" | "--help" | "-h" => {
            print!("{HELP}");
            Ok(())
        }
        "list" => cmd_list(Path::new(p(0, "file.cc")?), encoded),
        "extract" => cmd_extract(
            Path::new(p(0, "file.cc")?),
            Path::new(p(1, "out-dir")?),
            encoded,
        ),
        "pack" => cmd_pack(Path::new(p(0, "dir")?), Path::new(p(1, "out.cc")?), encoded),
        "hash" => {
            for n in &o.positional {
                println!("{:04X}  {n}", name_to_id(n));
            }
            Ok(())
        }
        "sprite" => cmd_sprite(
            Path::new(p(0, "file.cc")?),
            p(1, "name")?,
            Path::new(p(2, "out.png")?),
            o.flags.get("palette").cloned().flatten(),
            o.flags.get("scale").cloned().flatten(),
        ),
        "palette" => cmd_palette(
            Path::new(p(0, "file.cc")?),
            p(1, "name.pal")?,
            Path::new(p(2, "out.png")?),
        ),
        "raw" => cmd_raw(
            Path::new(p(0, "file.cc")?),
            p(1, "name.raw")?,
            Path::new(p(2, "out.png")?),
            o.flags.get("palette").cloned().flatten(),
        ),
        "evt" => cmd_evt(Path::new(p(0, "file.cc")?), p(1, "mazeNNNN.evt")?),
        "synth" => cmd_synth(Path::new(p(0, "out-dir")?)),
        "screenshot" => cmd_screenshot(
            Path::new(p(0, "out.png")?),
            o.flags.get("data").cloned().flatten(),
            o.flags.get("frames").cloned().flatten(),
            o.flags.get("keys").cloned().flatten(),
        ),
        other => Err(format!("unknown command `{other}` (see `xeentool help`)")),
    }
}

fn open_cc(path: &Path, encoded: bool) -> Res<CcArchive> {
    let data = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    CcArchive::parse(data, encoded).map_err(|e| format!("{}: {e}", path.display()))
}

fn cmd_list(path: &Path, encoded: bool) -> Res<()> {
    let cc = open_cc(path, encoded)?;
    let names = NameIndex::builtin();
    let mut known = 0;
    for e in cc.entries() {
        let name = names.lookup(e.id);
        known += usize::from(name.is_some());
        let data = cc.read_id(e.id).unwrap_or_default();
        let kind = classify(name, &data);
        let alt = names.names_for(e.id);
        let alt = if alt.len() > 1 {
            format!("  (also: {})", alt[1..].join(", "))
        } else {
            String::new()
        };
        println!(
            "{:04X}  {:>8}  {:>6}  {:<14} {:?}{alt}",
            e.id,
            e.offset,
            e.size,
            name.unwrap_or("?"),
            kind
        );
    }
    eprintln!(
        "{} entries, {} named ({}%)",
        cc.entries().len(),
        known,
        known * 100 / cc.entries().len().max(1)
    );
    Ok(())
}

fn cmd_extract(path: &Path, out: &Path, encoded: bool) -> Res<()> {
    let cc = open_cc(path, encoded)?;
    let names = NameIndex::builtin();
    fs::create_dir_all(out).map_err(|e| e.to_string())?;
    for e in cc.entries() {
        let name = names
            .lookup(e.id)
            .map(str::to_string)
            .unwrap_or_else(|| format!("{:04x}", e.id));
        let data = cc.read_id(e.id).unwrap_or_default();
        fs::write(out.join(&name), data).map_err(|err| format!("{name}: {err}"))?;
    }
    eprintln!(
        "extracted {} entries to {}",
        cc.entries().len(),
        out.display()
    );
    Ok(())
}

fn cmd_pack(dir: &Path, out: &Path, encoded: bool) -> Res<()> {
    let mut b = CcBuilder::new();
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file())
        .collect();
    files.sort();
    for f in &files {
        let name = f.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        let data = fs::read(f).map_err(|e| format!("{}: {e}", f.display()))?;
        // 4-hex-digit names (as written by `extract`) are literal ids.
        b.add(name, data).map_err(|e| format!("{name}: {e}"))?;
    }
    let bytes = b.build(encoded).map_err(|e| e.to_string())?;
    fs::write(out, bytes).map_err(|e| e.to_string())?;
    eprintln!("packed {} files into {}", files.len(), out.display());
    Ok(())
}

fn load_palette(cc: &CcArchive, name: Option<&str>) -> Res<[[u8; 3]; 256]> {
    let wanted = name.unwrap_or("mm4.pal");
    match cc.read(wanted) {
        Some(d) => Ok(VgaPalette::parse(&d)
            .map_err(|e| format!("{wanted}: {e}"))?
            .to_engine_levels()),
        None if name.is_some() => Err(format!("palette {wanted} not found")),
        None => Ok(default_palette()),
    }
}

fn write_png(path: &Path, w: usize, h: usize, rgba: &[u8]) -> Res<()> {
    let file = fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w as u32, h as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut wr = enc.write_header().map_err(|e| e.to_string())?;
    wr.write_image_data(rgba).map_err(|e| e.to_string())
}

fn indexed_to_rgba(pixels: &[u8], pal: &[[u8; 3]; 256], transparent: Option<u8>) -> Vec<u8> {
    let lut = level_lut(DacScaling::Full);
    pixels
        .iter()
        .flat_map(|&i| {
            let c = pal[usize::from(i)];
            let a = if Some(i) == transparent { 0 } else { 255 };
            [
                lut[usize::from(c[0])],
                lut[usize::from(c[1])],
                lut[usize::from(c[2])],
                a,
            ]
        })
        .collect()
}

fn cmd_sprite(
    path: &Path,
    name: &str,
    out: &Path,
    pal: Option<String>,
    scale: Option<String>,
) -> Res<()> {
    let cc = open_cc(path, true)?;
    let data = cc.read(name).ok_or_else(|| format!("{name} not found"))?;
    let sprite = SpriteFile::parse(data).map_err(|e| format!("{name}: {e}"))?;
    let pal = load_palette(&cc, pal.as_deref())?;
    let k = match scale.as_deref() {
        None | Some("1") => 1usize,
        Some("x2") | Some("2") => 2,
        Some("x3") | Some("3") => 3,
        Some(s) => return Err(format!("bad --scale {s} (use 1, 2 or 3)")),
    };
    let frames: Vec<_> = (0..sprite.frame_count())
        .map(|f| {
            sprite
                .decode_frame(f)
                .map_err(|e| format!("frame {f}: {e}"))
        })
        .collect::<Res<_>>()?;
    // Lay all frames out on one sheet, left to right, wrapping at 1024 px.
    let (mut x, mut y, mut row_h, mut sheet_w) = (0usize, 0usize, 0usize, 0usize);
    let mut places = Vec::new();
    for img in &frames {
        let (w, h) = (img.width * k + 2, img.height * k + 2);
        if x > 0 && x + w > 1024 {
            x = 0;
            y += row_h;
            row_h = 0;
        }
        places.push((x + 1, y + 1));
        x += w;
        sheet_w = sheet_w.max(x);
        row_h = row_h.max(h);
    }
    let (sheet_w, sheet_h) = (sheet_w.max(1), (y + row_h).max(1));
    let lut = level_lut(DacScaling::Full);
    let mut rgba = vec![0u8; sheet_w * sheet_h * 4];
    for (img, &(px, py)) in frames.iter().zip(&places) {
        for iy in 0..img.height * k {
            for ix in 0..img.width * k {
                if let Some(v) = img.get(ix / k, iy / k) {
                    let c = pal[usize::from(v)];
                    let o = ((py + iy) * sheet_w + px + ix) * 4;
                    rgba[o..o + 4].copy_from_slice(&[
                        lut[usize::from(c[0])],
                        lut[usize::from(c[1])],
                        lut[usize::from(c[2])],
                        255,
                    ]);
                }
            }
        }
    }
    write_png(out, sheet_w, sheet_h, &rgba)?;
    eprintln!("{} frame(s) -> {}", frames.len(), out.display());
    Ok(())
}

fn cmd_palette(path: &Path, name: &str, out: &Path) -> Res<()> {
    let cc = open_cc(path, true)?;
    let pal = load_palette(&cc, Some(name))?;
    let mut s = Surface::new(16 * 16, 16 * 16);
    for i in 0..256usize {
        let (x, y) = ((i % 16) as i32 * 16, (i / 16) as i32 * 16);
        s.fill_rect(
            openxeenng_engine::gfx::Rect::new(x, y, x + 16, y + 16),
            i as u8,
        );
    }
    write_png(out, 256, 256, &indexed_to_rgba(s.pixels(), &pal, None))
}

fn cmd_raw(path: &Path, name: &str, out: &Path, pal: Option<String>) -> Res<()> {
    let cc = open_cc(path, true)?;
    let data = cc.read(name).ok_or_else(|| format!("{name} not found"))?;
    if data.len() != 64000 {
        return Err(format!("{name} is {} bytes, expected 64000", data.len()));
    }
    let pal = load_palette(&cc, pal.as_deref())?;
    write_png(out, 320, 200, &indexed_to_rgba(&data, &pal, None))
}

fn cmd_evt(path: &Path, name: &str) -> Res<()> {
    let cc = open_cc(path, true)?;
    let data = cc.read(name).ok_or_else(|| format!("{name} not found"))?;
    let (events, err) = evt::parse_events_lenient(&data);
    let text_name = name
        .to_ascii_lowercase()
        .replacen("maze", "aaze", 1)
        .replace(".evt", ".txt");
    let texts = cc.read(&text_name).map(|d| misc::parse_string_table(&d));
    print!("{}", evt::disassemble(&events, texts.as_deref()));
    if let Some(e) = err {
        eprintln!("warning: {e}");
    }
    Ok(())
}

fn cmd_synth(out: &Path) -> Res<()> {
    fs::create_dir_all(out).map_err(|e| e.to_string())?;
    for (name, data) in synth::synthetic_game_files() {
        fs::write(out.join(&name), data).map_err(|e| e.to_string())?;
        eprintln!("wrote {}", out.join(&name).display());
    }
    Ok(())
}

fn parse_key(k: &str) -> Res<Key> {
    Ok(match k.to_ascii_lowercase().as_str() {
        "up" => Key::Up,
        "down" => Key::Down,
        "left" => Key::Left,
        "right" => Key::Right,
        "enter" => Key::Enter,
        "esc" | "escape" => Key::Escape,
        "space" => Key::Space,
        "tab" => Key::Tab,
        "pgup" => Key::PageUp,
        "pgdn" => Key::PageDown,
        s if s.chars().count() == 1 => Key::Char(s.chars().next().unwrap_or(' ')),
        s => return Err(format!("unknown key `{s}`")),
    })
}

/// Loads every regular file of a directory as `(name, bytes)`.
fn load_dir(dir: &Path) -> Res<Vec<(String, Vec<u8>)>> {
    let mut out = Vec::new();
    for e in fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let p = e.map_err(|e| e.to_string())?.path();
        if p.is_file() {
            let name = p
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_string();
            out.push((name, fs::read(&p).map_err(|e| e.to_string())?));
        }
    }
    Ok(out)
}

fn cmd_screenshot(
    out: &Path,
    data: Option<String>,
    frames: Option<String>,
    keys: Option<String>,
) -> Res<()> {
    let files = match data {
        Some(d) => load_dir(Path::new(&d))?,
        None => Vec::new(),
    };
    let frames: u32 = frames.map_or(Ok(30), |f| {
        f.parse().map_err(|_| format!("bad --frames {f}"))
    })?;
    let keys: Vec<Key> = match keys {
        Some(k) => k
            .split(',')
            .filter(|s| !s.is_empty())
            .map(parse_key)
            .collect::<Res<_>>()?,
        None => Vec::new(),
    };
    let mut engine = Engine::from_files(EngineConfig::default(), files);
    engine.run_frame(16_667);
    for k in keys {
        engine.push_input(InputEvent::KeyDown(k));
        engine.run_frame(16_667);
        engine.push_input(InputEvent::KeyUp(k));
        engine.run_frame(16_667);
    }
    for _ in 0..frames {
        engine.run_frame(16_667);
    }
    let mut rgba = vec![0u8; openxeenng_engine::WIDTH * openxeenng_engine::HEIGHT * 4];
    engine.frame_rgba(&mut rgba);
    write_png(
        out,
        openxeenng_engine::WIDTH,
        openxeenng_engine::HEIGHT,
        &rgba,
    )?;
    eprintln!("{} -> {}", engine.status(), out.display());
    Ok(())
}

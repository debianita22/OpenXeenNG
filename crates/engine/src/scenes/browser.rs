// SPDX-License-Identifier: GPL-3.0-or-later
//! Asset browser: lists every resource of the loaded archives and shows it
//! with the matching reader (sprites, images, palettes, fonts, maps, scripts,
//! monsters, sounds). It is the milestone-0 front end and a debugging tool.

use std::rc::Rc;

use openxeen_formats::cc::CcEntry;
use openxeen_formats::evt;
use openxeen_formats::font::Font;
use openxeen_formats::gamedata::{GameData, Side};
use openxeen_formats::maze::{Direction, MAP_SIZE, MazeData};
use openxeen_formats::misc::{self, HeadData};
use openxeen_formats::mob::MobFile;
use openxeen_formats::monster::{self, parse_monster_table};
use openxeen_formats::names::NameIndex;
use openxeen_formats::palette::VgaPalette;
use openxeen_formats::sprite::SpriteFile;
use openxeen_formats::tables::TEXT_COLORS;
use openxeen_formats::voc::parse_voc;

use crate::audio::Channel;
use crate::exec::{Cx, Io};
use crate::gfx::{self, DrawContext, Rect, SCALE_ENLARGE, Surface};
use crate::input::Key;
use crate::present::{Palette, UiColors, default_palette};
use crate::text::{UI_CHAR_H, draw_game_char, draw_ui_text};

/// What a resource looks like.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Sprite resource.
    Sprite,
    /// 320x200 raw image.
    RawImage,
    /// VGA palette.
    Palette,
    /// VOC sound.
    Sound,
    /// Game font.
    Font,
    /// Map data.
    Maze,
    /// Monster/object placement.
    Mob,
    /// Event script.
    Events,
    /// Head (danger sense) data.
    Head,
    /// Monster table.
    Monsters,
    /// Object animation table.
    AnimInfo,
    /// Map name.
    MazeName,
    /// NUL separated strings.
    Strings,
    /// Adlib music (player not implemented yet).
    Music,
    /// Anything else.
    Unknown,
}

impl Kind {
    fn label(self) -> &'static str {
        match self {
            Kind::Sprite => "sprite",
            Kind::RawImage => "image",
            Kind::Palette => "palette",
            Kind::Sound => "sound",
            Kind::Font => "font",
            Kind::Maze => "map",
            Kind::Mob => "placement",
            Kind::Events => "script",
            Kind::Head => "danger",
            Kind::Monsters => "monsters",
            Kind::AnimInfo => "anim",
            Kind::MazeName => "map name",
            Kind::Strings => "text",
            Kind::Music => "music",
            Kind::Unknown => "data",
        }
    }
}

/// Classifies a resource from its (possibly unknown) name and content.
pub fn classify(name: Option<&str>, data: &[u8]) -> Kind {
    let lname = name.map(|n| n.to_ascii_lowercase());
    let (stem, ext) = match lname.as_deref().and_then(|n| n.rsplit_once('.')) {
        Some((s, e)) => (s.to_string(), e.to_string()),
        None => (lname.clone().unwrap_or_default(), String::new()),
    };
    let is_sprite = || {
        SpriteFile::parse(data.to_vec())
            .map(|s| s.frame_count() > 0 && s.decode_frame(0).is_ok())
            .unwrap_or(false)
    };
    match ext.as_str() {
        _ if stem == "fnt" && ext.is_empty() => Kind::Font,
        "pal" if data.len() >= 768 => Kind::Palette,
        "voc" => Kind::Sound,
        "raw" if data.len() == 64000 => Kind::RawImage,
        "dat" if stem.starts_with("maze") && data.len() == 892 => Kind::Maze,
        "dat" if stem == "clouds" || stem == "dark" => Kind::AnimInfo,
        "cld" if stem == "animinfo" => Kind::AnimInfo,
        "mob" => Kind::Mob,
        "evt" => Kind::Events,
        "hed" if data.len() == 512 => Kind::Head,
        "mon" | "cld" | "swd"
            if (stem == "xeen" || stem == "dark" || stem == "monsters")
                && data.len() % monster::MONSTER_RECORD_BYTES == 0 =>
        {
            Kind::Monsters
        }
        "txt" if (stem.starts_with("xeen") || stem.starts_with("dark")) && data.len() == 33 => {
            Kind::MazeName
        }
        "txt" | "bin" | "ext" => Kind::Strings,
        "m" => Kind::Music,
        _ if is_sprite() => Kind::Sprite,
        _ if data.len() == 64000 => Kind::RawImage,
        _ => Kind::Unknown,
    }
}

struct Entry {
    entry: CcEntry,
    name: Option<String>,
    kind: Kind,
}

#[allow(clippy::large_enum_variant)] // one view at a time; boxing buys nothing
enum View {
    List,
    Sprite(SpriteView),
    Image(Vec<u8>),
    Palette(Palette),
    Text(TextView),
    Maze(MazeView),
    Font(FontView),
    Sound(SoundView),
}

struct SpriteView {
    sprite: SpriteFile,
    frame: usize,
    scale: u16,
    flipped: bool,
    drawer: u32,
    param: u32,
    animate: bool,
    last_step: u64,
}

struct TextView {
    title: String,
    lines: Vec<String>,
    scroll: usize,
}

struct MazeView {
    maze: MazeData,
    title: String,
    cursor: (usize, usize),
}

struct FontView {
    font: Font,
    reduced: bool,
    color_set: usize,
}

struct SoundView {
    samples: Rc<[i16]>,
    rate: u32,
}

struct Browser {
    data: Rc<GameData>,
    names: NameIndex,
    sides: Vec<Side>,
    side_idx: usize,
    entries: Vec<Entry>,
    selected: usize,
    view: View,
    view_name: String,
    palettes: Vec<(String, Palette)>,
    palette_idx: usize,
    error: Option<String>,
}

const LIST_ROWS: usize = 20;

fn side_label(side: Side) -> &'static str {
    match side {
        Side::Clouds => "XEEN.CC",
        Side::Dark => "DARK.CC",
        Side::Intro => "INTRO.CC",
    }
}

impl Browser {
    fn new(data: Rc<GameData>) -> Self {
        let mut names = NameIndex::builtin();
        // Monster attack sounds are named in the monster tables.
        for side in data.sides() {
            for table in ["xeen.mon", "dark.mon"] {
                if let Some(t) = data.read(side, table).and_then(|d| parse_monster_table(&d).ok()) {
                    for m in t {
                        if !m.attack_voc.is_empty() {
                            names.insert(format!("{}.voc", m.attack_voc.to_ascii_lowercase()));
                        }
                    }
                }
            }
        }
        let sides = data.sides();
        let side_idx = sides
            .iter()
            .position(|&s| s == data.default_side())
            .unwrap_or(0);
        let mut b = Browser {
            data,
            names,
            sides,
            side_idx,
            entries: Vec::new(),
            selected: 0,
            view: View::List,
            view_name: String::new(),
            palettes: Vec::new(),
            palette_idx: 0,
            error: None,
        };
        b.load_side();
        b
    }

    fn side(&self) -> Side {
        self.sides[self.side_idx]
    }

    fn load_side(&mut self) {
        let side = self.side();
        let Some(archive) = self.data.archive(side) else {
            self.entries.clear();
            return;
        };
        let mut entries: Vec<Entry> = archive
            .entries()
            .iter()
            .map(|e| {
                let name = self.names.lookup(e.id).map(str::to_string);
                let bytes = archive.read_id(e.id).unwrap_or_default();
                let kind = classify(name.as_deref(), &bytes);
                Entry {
                    entry: *e,
                    name,
                    kind,
                }
            })
            .collect();
        entries.sort_by(|a, b| match (&a.name, &b.name) {
            (Some(x), Some(y)) => x.cmp(y),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a.entry.id.cmp(&b.entry.id),
        });
        self.entries = entries;
        self.selected = 0;

        // Palettes available on this side, the main game palette first.
        let mut pals: Vec<(String, Palette)> = Vec::new();
        let candidates = [
            "mm4.pal", "dark.pal", "intro.pal", "intro1.pal", "mm4e.pal", "white.pal", "fly.pal",
            "mirror.pal", "endgame.pal", "firemain.pal", "skymain.pal", "scr.pal",
            "eg250001.pal",
        ];
        for n in candidates {
            if let Some(p) = self.data.read(side, n).and_then(|d| VgaPalette::parse(&d).ok()) {
                pals.push((n.to_string(), p.to_engine_levels()));
            }
        }
        if pals.is_empty() {
            pals.push(("built-in".to_string(), default_palette()));
        }
        self.palettes = pals;
        self.palette_idx = 0;
    }

    fn palette(&self) -> Palette {
        self.palettes[self.palette_idx].1
    }

    fn open_selected(&mut self, io: &mut Io) {
        let Some(e) = self.entries.get(self.selected) else {
            return;
        };
        let side = self.side();
        let bytes = self
            .data
            .archive(side)
            .and_then(|a| a.read_id(e.entry.id))
            .unwrap_or_default();
        let title = e
            .name
            .clone()
            .unwrap_or_else(|| format!("#{:04X}", e.entry.id));
        self.view_name = title.clone();
        self.error = None;
        let result: Result<View, String> = match e.kind {
            Kind::Sprite => SpriteFile::parse(bytes.clone()).map_err(|x| x.to_string()).map(|s| {
                View::Sprite(SpriteView {
                    sprite: s,
                    frame: 0,
                    scale: 0,
                    flipped: false,
                    drawer: 0,
                    param: 0,
                    animate: false,
                    last_step: 0,
                })
            }),
            Kind::RawImage => Ok(View::Image(bytes.clone())),
            Kind::Palette => VgaPalette::parse(&bytes)
                .map(|p| View::Palette(p.to_engine_levels()))
                .map_err(|x| x.to_string()),
            Kind::Font => Font::parse_western(bytes.clone())
                .map(|f| {
                    View::Font(FontView {
                        font: f,
                        reduced: false,
                        color_set: 0,
                    })
                })
                .map_err(|x| x.to_string()),
            Kind::Maze => MazeData::parse(&bytes)
                .map(|m| {
                    let id = m.maze_number;
                    let name_file = format!(
                        "{}{}{:03}.txt",
                        if side == Side::Dark { "dark" } else { "xeen" },
                        if id >= 100 { 'x' } else { '0' },
                        id
                    );
                    let map_name = self
                        .data
                        .read(side, &name_file)
                        .map(|d| misc::parse_maze_name(&d))
                        .unwrap_or_default();
                    View::Maze(MazeView {
                        maze: m,
                        title: format!("{title} {map_name}"),
                        cursor: (0, 0),
                    })
                })
                .map_err(|x| x.to_string()),
            Kind::Sound => parse_voc(&bytes)
                .map(|v| {
                    let samples: Rc<[i16]> = v.samples.into();
                    io.mixer.play(Channel::Sfx, samples.clone(), v.sample_rate, false);
                    View::Sound(SoundView {
                        samples,
                        rate: v.sample_rate,
                    })
                })
                .map_err(|x| x.to_string()),
            _ => Ok(View::Text(TextView {
                title: title.clone(),
                lines: fit_lines(self.describe(e.kind, &title, &bytes)),
                scroll: 0,
            })),
        };
        match result {
            Ok(v) => self.view = v,
            Err(msg) => {
                self.view = View::Text(TextView {
                    title: title.clone(),
                    lines: fit_lines(
                        std::iter::once(format!("Cannot open: {msg}"))
                            .chain(hex_dump(&bytes))
                            .collect(),
                    ),
                    scroll: 0,
                });
            }
        }
    }

    fn describe(&self, kind: Kind, title: &str, bytes: &[u8]) -> Vec<String> {
        match kind {
            Kind::Events => {
                let (events, err) = evt::parse_events_lenient(bytes);
                let text_name = title.replacen("maze", "aaze", 1).replace(".evt", ".txt");
                let texts = self
                    .data
                    .read(self.side(), &text_name)
                    .map(|d| misc::parse_string_table(&d));
                let mut lines: Vec<String> = evt::disassemble(&events, texts.as_deref())
                    .lines()
                    .map(str::to_string)
                    .collect();
                if let Some(e) = err {
                    lines.push(format!("! {e}"));
                }
                lines
            }
            Kind::Mob => match MobFile::parse(bytes) {
                Ok(m) => {
                    let mut l = vec![
                        format!("object sprites  {:?}", m.object_sprite_slots()),
                        format!(
                            "monster ids     {:?}",
                            m.monster_sprites.iter().filter(|&&v| v != 0xFF).collect::<Vec<_>>()
                        ),
                        format!(
                            "wall item ids   {:?}",
                            m.wall_item_sprites.iter().filter(|&&v| v != 0xFF).collect::<Vec<_>>()
                        ),
                    ];
                    for (label, list) in [
                        ("object", &m.objects),
                        ("monster", &m.monsters),
                        ("wall item", &m.wall_items),
                    ] {
                        for r in list {
                            l.push(format!(
                                "{label:<9} ({:4},{:4}) id {:3} dir {}",
                                r.x, r.y, r.id, r.direction
                            ));
                        }
                    }
                    l
                }
                Err(e) => vec![format!("! {e}")],
            },
            Kind::Head => match HeadData::parse(bytes) {
                Ok(h) => (0..MAP_SIZE)
                    .rev()
                    .map(|y| {
                        (0..MAP_SIZE)
                            .map(|x| {
                                let (l, r) = h.cells[y][x];
                                format!("{:X}", (l.max(r)).min(15))
                            })
                            .collect::<String>()
                    })
                    .collect(),
                Err(e) => vec![format!("! {e}")],
            },
            Kind::Monsters => match parse_monster_table(bytes) {
                Ok(t) => t
                    .iter()
                    .enumerate()
                    .map(|(i, m)| {
                        format!(
                            "{i:3} {:<15} hp{:5} ac{:3} x{:<6}",
                            m.name, m.hp, m.armor_class, m.experience
                        )
                    })
                    .collect(),
                Err(e) => vec![format!("! {e}")],
            },
            Kind::AnimInfo => match misc::parse_animation_info(bytes) {
                Ok(t) => t
                    .iter()
                    .enumerate()
                    .map(|(i, a)| format!("{i:3} {:?} {:?}", a.frame1, a.frame2))
                    .collect(),
                Err(e) => vec![format!("! {e}")],
            },
            Kind::MazeName => vec![misc::parse_maze_name(bytes)],
            Kind::Strings => {
                let mut out = Vec::new();
                for (i, s) in misc::parse_string_table(bytes).iter().enumerate() {
                    let clean: String = s
                        .chars()
                        .map(|c| if (' '..='~').contains(&c) { c } else { '.' })
                        .collect();
                    let wrapped = super::demo::wrap(&clean, 34);
                    if wrapped.is_empty() {
                        out.push(format!("{i:3}:"));
                    }
                    for (k, w) in wrapped.into_iter().enumerate() {
                        out.push(if k == 0 {
                            format!("{i:3}: {w}")
                        } else {
                            format!("     {w}")
                        });
                    }
                }
                out
            }
            Kind::Music => std::iter::once("Adlib music: player arrives with milestone 1".to_string())
                .chain(hex_dump(bytes))
                .collect(),
            _ => hex_dump(bytes),
        }
    }

    /// Handles a key; returns `false` when the browser should exit.
    fn key(&mut self, k: Key, io: &mut Io) -> bool {
        let pal_count = self.palettes.len();
        match &mut self.view {
            View::List => match k {
                Key::Escape => return false,
                Key::Up => self.selected = self.selected.saturating_sub(1),
                Key::Down => {
                    self.selected = (self.selected + 1).min(self.entries.len().saturating_sub(1))
                }
                Key::PageUp => self.selected = self.selected.saturating_sub(LIST_ROWS),
                Key::PageDown => {
                    self.selected =
                        (self.selected + LIST_ROWS).min(self.entries.len().saturating_sub(1))
                }
                Key::Home => self.selected = 0,
                Key::End => self.selected = self.entries.len().saturating_sub(1),
                Key::Tab => {
                    self.side_idx = (self.side_idx + 1) % self.sides.len();
                    self.load_side();
                }
                Key::Enter | Key::Right | Key::Space => self.open_selected(io),
                Key::Char(c) => {
                    // Jump to the next name starting with the typed letter.
                    let start = self.selected + 1;
                    let n = self.entries.len();
                    if let Some(i) = (0..n).map(|i| (start + i) % n.max(1)).find(|&i| {
                        self.entries[i]
                            .name
                            .as_deref()
                            .is_some_and(|nm| nm.starts_with(c))
                    }) {
                        self.selected = i;
                    }
                }
                _ => {}
            },
            View::Sprite(v) => match k {
                Key::Escape => self.view = View::List,
                Key::Right => v.frame = (v.frame + 1) % v.sprite.frame_count().max(1),
                Key::Left => {
                    let n = v.sprite.frame_count().max(1);
                    v.frame = (v.frame + n - 1) % n
                }
                Key::Up => {
                    v.scale = match v.scale {
                        0 => SCALE_ENLARGE,
                        s if s == SCALE_ENLARGE => SCALE_ENLARGE,
                        s => s - 1,
                    }
                }
                Key::Down => {
                    v.scale = match v.scale {
                        s if s == SCALE_ENLARGE => 0,
                        15 => 15,
                        s => s + 1,
                    }
                }
                Key::Char('f') => v.flipped = !v.flipped,
                Key::Char('d') => v.drawer = (v.drawer + 1) % 7,
                Key::Char('p') => v.param = (v.param + 1) % 32,
                Key::Char('a') | Key::Space => v.animate = !v.animate,
                Key::Char('c') | Key::Tab => self.palette_idx = (self.palette_idx + 1) % pal_count,
                _ => {}
            },
            View::Image(_) => match k {
                Key::Escape => self.view = View::List,
                Key::Char('c') | Key::Tab => self.palette_idx = (self.palette_idx + 1) % pal_count,
                _ => {}
            },
            View::Palette(_) => {
                if k == Key::Escape {
                    self.view = View::List;
                }
            }
            View::Text(t) => match k {
                Key::Escape => self.view = View::List,
                Key::Up => t.scroll = t.scroll.saturating_sub(1),
                Key::Down => t.scroll = (t.scroll + 1).min(t.lines.len().saturating_sub(1)),
                Key::PageUp => t.scroll = t.scroll.saturating_sub(LIST_ROWS),
                Key::PageDown => {
                    t.scroll = (t.scroll + LIST_ROWS).min(t.lines.len().saturating_sub(1))
                }
                _ => {}
            },
            View::Maze(m) => match k {
                Key::Escape => self.view = View::List,
                Key::Up => m.cursor.1 = (m.cursor.1 + 1).min(MAP_SIZE - 1),
                Key::Down => m.cursor.1 = m.cursor.1.saturating_sub(1),
                Key::Left => m.cursor.0 = m.cursor.0.saturating_sub(1),
                Key::Right => m.cursor.0 = (m.cursor.0 + 1).min(MAP_SIZE - 1),
                _ => {}
            },
            View::Font(f) => match k {
                Key::Escape => self.view = View::List,
                Key::Char('r') | Key::Space => f.reduced = !f.reduced,
                Key::Right => f.color_set = (f.color_set + 1) % TEXT_COLORS.len(),
                Key::Left => f.color_set = (f.color_set + TEXT_COLORS.len() - 1) % TEXT_COLORS.len(),
                _ => {}
            },
            View::Sound(s) => match k {
                Key::Escape => {
                    io.mixer.stop(Channel::Sfx);
                    self.view = View::List;
                }
                Key::Enter | Key::Space => io.mixer.play(Channel::Sfx, s.samples.clone(), s.rate, false),
                _ => {}
            },
        }
        true
    }

    fn draw(&mut self, io: &mut Io) {
        let side_name = side_label(self.side());
        let variant = self.data.variant().title();
        let pal = match &self.view {
            View::Palette(p) => *p,
            View::List | View::Text(_) | View::Maze(_) | View::Sound(_) => self.palette(),
            _ => self.palette(),
        };
        io.palette = pal;
        let ui = UiColors::from_palette(&pal);
        io.status = format!("OpenXeen asset browser - {variant}");
        let ticks = io.ticks;
        let s = &mut io.screen;
        s.clear(ui.bg);

        match &mut self.view {
            View::List => {
                let header = format!("{variant} {side_name} {:>5}", self.entries.len());
                draw_bar(s, 0, &header, &ui);
                let top = self.selected.saturating_sub(LIST_ROWS / 2).min(
                    self.entries.len().saturating_sub(LIST_ROWS),
                );
                for (row, e) in self.entries.iter().enumerate().skip(top).take(LIST_ROWS) {
                    let y = 12 + (row - top) as i32 * 9;
                    let name = e
                        .name
                        .clone()
                        .unwrap_or_else(|| format!("#{:04X}", e.entry.id));
                    let line = format!("{:<14}{:>6} {:<9}", name, e.entry.size, e.kind.label());
                    if row == self.selected {
                        s.fill_rect(Rect::new(0, y - 1, 320, y + 8), ui.bar);
                        draw_ui_text(s, 4, y, &line, ui.hi, None);
                    } else {
                        let c = if e.name.is_some() { ui.text } else { ui.dim };
                        draw_ui_text(s, 4, y, &line, c, None);
                    }
                }
                draw_help(s, "^v move Enter open Tab side Esc quit", &ui);
            }
            View::Sprite(v) => {
                if v.animate && ticks >= v.last_step + 2 {
                    v.last_step = ticks;
                    v.frame = (v.frame + 1) % v.sprite.frame_count().max(1);
                }
                let (fw, fh) = v.sprite.frame_size(v.frame).unwrap_or((0, 0));
                let mut flags = if v.drawer == 0 { 0 } else { (v.drawer << 8) | v.param };
                if v.flipped {
                    flags |= gfx::flags::HORIZ_FLIPPED;
                }
                let area = Rect::new(0, 11, 320, 180);
                // Checkerboard shows transparency.
                for y in (area.top..area.bottom).step_by(8) {
                    for x in (0..320).step_by(8) {
                        let c = if ((x + y) / 8) % 2 == 0 { ui.bg } else { ui.bar };
                        s.fill_rect(Rect::new(x, y, x + 8, (y + 8).min(area.bottom)), c);
                    }
                }
                let enlarge = v.scale == SCALE_ENLARGE;
                let (dw, dh) = if enlarge { (fw * 2, fh * 2) } else { (fw, fh) };
                let px = ((320 - dw as i32) / 2).max(0);
                let py = (area.top + (area.height() - dh as i32) / 2).max(area.top);
                let mut ctx = DrawContext {
                    rng: &mut io.rng,
                    palette: &io.palette,
                    clipped_bottom: area.bottom,
                };
                let res = gfx::draw_sprite(s, &v.sprite, v.frame, (px, py), area, flags, v.scale, &mut ctx);
                if let Err(e) = res {
                    self.error = Some(e.to_string());
                }
                let scale_txt = if enlarge { "x2".to_string() } else { format!("/{}", v.scale) };
                let header = format!(
                    "{} {}/{} {}x{} {}",
                    self.view_name,
                    v.frame + 1,
                    v.sprite.frame_count(),
                    fw,
                    fh,
                    scale_txt
                );
                draw_bar(s, 0, &header, &ui);
                let pal_name = &self.palettes[self.palette_idx].0;
                draw_ui_text(s, 4, 182, &format!("pal {pal_name} fx {} p{}", v.drawer, v.param), ui.dim, None);
                draw_help(s, "<> frame ^v scale F flip D fx C pal", &ui);
            }
            View::Image(bytes) => {
                for (i, &b) in bytes.iter().enumerate().take(64000) {
                    s.pixels_mut()[i] = b;
                }
            }
            View::Palette(p) => {
                draw_bar(s, 0, &self.view_name, &ui);
                for i in 0..256 {
                    let x = (i % 16) * 20;
                    let y = 11 + (i / 16) * 11;
                    s.fill_rect(Rect::new(x + 1, y + 1, x + 19, y + 10), i as u8);
                }
                let _ = p;
            }
            View::Text(t) => {
                draw_bar(s, 0, &t.title, &ui);
                for (row, line) in t.lines.iter().skip(t.scroll).take(LIST_ROWS).enumerate() {
                    let clipped: String = line.chars().take(39).collect();
                    draw_ui_text(s, 4, 12 + row as i32 * 9, &clipped, ui.text, None);
                }
                let pos = format!("{}/{}", (t.scroll + 1).min(t.lines.len()), t.lines.len());
                draw_help(s, &format!("^v scroll Esc back {pos}"), &ui);
            }
            View::Maze(m) => draw_maze(s, m, &ui),
            View::Font(f) => {
                draw_bar(s, 0, &self.view_name, &ui);
                let colors = TEXT_COLORS[f.color_set];
                let colors = [ui.bg, colors[1], colors[2], colors[3]];
                let clip = s.rect();
                for ch in 0..128u8 {
                    let x = 8 + (ch % 16) as i32 * 19;
                    let y = 14 + (ch / 16) as i32 * 12;
                    draw_game_char(s, &f.font, ch, x, y, colors, f.reduced, clip);
                }
                let sample = b"The quick brown fox jumps";
                let mut x = 8;
                for &c in sample {
                    x += draw_game_char(s, &f.font, c, x, 116, colors, f.reduced, clip);
                }
                draw_ui_text(
                    s,
                    4,
                    182,
                    &format!("colour set {} {}", f.color_set, if f.reduced { "reduced" } else { "" }),
                    ui.dim,
                    None,
                );
                draw_help(s, "<> colours R reduced Esc back", &ui);
            }
            View::Sound(snd) => {
                draw_bar(s, 0, &format!("{} {} Hz", self.view_name, snd.rate), &ui);
                let n = snd.samples.len().max(1);
                for x in 0..320usize {
                    let i = x * n / 320;
                    let v = i32::from(snd.samples.get(i).copied().unwrap_or(0));
                    let h = (v.abs() * 70 / 32768).max(1);
                    s.fill_rect(Rect::new(x as i32, 95 - h, x as i32 + 1, 95 + h), ui.hi);
                }
                let secs = snd.samples.len() as f32 / snd.rate.max(1) as f32;
                draw_ui_text(s, 4, 182, &format!("{} samples {:.2}s", snd.samples.len(), secs), ui.dim, None);
                draw_help(s, "Enter play Esc back", &ui);
            }
        }
        if let Some(e) = &self.error {
            draw_ui_text(s, 4, 172, &e.chars().take(39).collect::<String>(), ui.warn, None);
        }
    }
}

fn draw_bar(s: &mut Surface, y: i32, text: &str, ui: &UiColors) {
    s.fill_rect(Rect::new(0, y, 320, y + 10), ui.bar);
    let t: String = text.chars().take(39).collect();
    draw_ui_text(s, 4, y + 1, &t, ui.text, None);
}

fn draw_help(s: &mut Surface, text: &str, ui: &UiColors) {
    let y = 200 - UI_CHAR_H - 2;
    s.fill_rect(Rect::new(0, y - 1, 320, 200), ui.bar);
    let t: String = text.chars().take(39).collect();
    draw_ui_text(s, 4, y, &t, ui.text, None);
}

fn draw_maze(s: &mut Surface, m: &MazeView, ui: &UiColors) {
    draw_bar(s, 0, &m.title, ui);
    let cell = 10i32;
    let ox = 4;
    let oy = 14;
    let maze = &m.maze;
    let outdoors = maze.is_outdoors();
    for y in 0..MAP_SIZE {
        for x in 0..MAP_SIZE {
            // North is up: map row 15 at the top.
            let sx = ox + x as i32 * cell;
            let sy = oy + (MAP_SIZE - 1 - y) as i32 * cell;
            let seen = maze.seen[y][x];
            let fill = if outdoors {
                32 + (maze.outdoor_layers(x, y).0 % 14) * 16 + 6
            } else if seen {
                ui.bar
            } else {
                ui.bg
            };
            s.fill_rect(Rect::new(sx, sy, sx + cell, sy + cell), fill);
            if !outdoors {
                for dir in Direction::ALL {
                    if maze.indoor_wall(x, y, dir) == 0 {
                        continue;
                    }
                    let r = match dir {
                        Direction::North => Rect::new(sx, sy, sx + cell, sy + 1),
                        Direction::South => Rect::new(sx, sy + cell - 1, sx + cell, sy + cell),
                        Direction::West => Rect::new(sx, sy, sx + 1, sy + cell),
                        Direction::East => Rect::new(sx + cell - 1, sy, sx + cell, sy + cell),
                    };
                    s.fill_rect(r, ui.text);
                }
            }
            if maze.cells[y][x].flags & 0x10 != 0 {
                s.put(sx + cell / 2, sy + cell / 2, ui.hi);
            }
        }
    }
    let (cx, cy) = m.cursor;
    let sx = ox + cx as i32 * cell;
    let sy = oy + (MAP_SIZE - 1 - cy) as i32 * cell;
    s.frame_rect(Rect::new(sx - 1, sy - 1, sx + cell + 1, sy + cell + 1), ui.hi);

    let px = 170;
    let c = maze.cells[cy][cx];
    let lines = [
        format!("map {}", maze.maze_number),
        if outdoors { "outdoors".to_string() } else { format!("indoor kind {}", maze.wall_kind) },
        format!("N{} E{} S{} W{}", maze.surrounding[0], maze.surrounding[1], maze.surrounding[2], maze.surrounding[3]),
        format!("flags {:04X}/{:04X}", maze.maze_flags, maze.maze_flags2),
        String::new(),
        format!("cell {},{}", cx, cy),
        format!("walls {:04X}", maze.walls[cy][cx]),
        format!("surface {} fl {:02X}", c.surface_id, c.flags),
        format!("seen {} step {}", u8::from(maze.seen[cy][cx]), u8::from(maze.stepped_on[cy][cx])),
    ];
    for (i, l) in lines.iter().enumerate() {
        draw_ui_text(s, px, oy + i as i32 * 10, l, ui.text, None);
    }
    draw_help(s, "arrows cell Esc back", ui);
}

/// Columns available to the UI font on a 320-pixel line (minus margins).
const TEXT_COLS: usize = 39;

/// Makes listing lines fit the 39-column screen: runs of spaces inside a
/// line collapse to one (leading indentation stays) and long lines wrap with
/// a hanging indent.
fn fit_lines(lines: Vec<String>) -> Vec<String> {
    let mut out = Vec::with_capacity(lines.len());
    for line in lines {
        let indent = line.len() - line.trim_start().len();
        let mut compact = " ".repeat(indent.min(4));
        let mut prev_space = false;
        for c in line.trim_start().chars() {
            if c == ' ' {
                if !prev_space {
                    compact.push(c);
                }
                prev_space = true;
            } else {
                compact.push(c);
                prev_space = false;
            }
        }
        let chars: Vec<char> = compact.chars().collect();
        if chars.len() <= TEXT_COLS {
            out.push(compact);
            continue;
        }
        let hang = "      ";
        let mut first = true;
        let mut rest: &[char] = &chars;
        while !rest.is_empty() {
            let width = if first { TEXT_COLS } else { TEXT_COLS - hang.len() };
            let take = rest.len().min(width);
            let piece: String = rest[..take].iter().collect();
            out.push(if first { piece } else { format!("{hang}{piece}") });
            rest = &rest[take..];
            first = false;
        }
    }
    out
}

fn hex_dump(bytes: &[u8]) -> Vec<String> {
    bytes
        .chunks(8)
        .enumerate()
        .map(|(i, c)| {
            let hex: Vec<String> = c.iter().map(|b| format!("{b:02X}")).collect();
            let asc: String = c
                .iter()
                .map(|&b| if (0x20..0x7F).contains(&b) { b as char } else { '.' })
                .collect();
            format!("{:04X} {:<24}{asc}", i * 8, hex.join(" "))
        })
        .collect()
}

/// Runs the browser until Escape on the list.
pub async fn run(cx: Cx, data: Rc<GameData>) {
    let mut b = Browser::new(data);
    loop {
        let keep_going = cx.with(|io| {
            while let Some(ev) = io.events.pop_front() {
                if let Some(k) = ev.pressed_key() {
                    if !b.key(k, io) {
                        return false;
                    }
                }
            }
            b.draw(io);
            true
        });
        if !keep_going {
            cx.with(|io| io.quit = true);
            return;
        }
        cx.next_frame().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth::synthetic_game_files;
    use crate::{Engine, EngineConfig, InputEvent};

    #[test]
    fn listing_lines_fit_the_screen() {
        let lines = fit_lines(vec![
            "    0: SignText               01    ; \"Welcome to the synthetic test map\"".to_string(),
            "short".to_string(),
        ]);
        assert!(lines.iter().all(|l| l.chars().count() <= TEXT_COLS));
        assert!(lines[0].starts_with("    0: SignText 01 ;"));
        assert_eq!(lines.last().unwrap(), "short");
        assert!(lines.len() >= 3);
    }

    #[test]
    fn classifies_by_name_and_content() {
        assert_eq!(classify(Some("mm4.pal"), &[0; 768]), Kind::Palette);
        assert_eq!(classify(Some("fnt"), &[0; 4352]), Kind::Font);
        assert_eq!(classify(Some("maze0001.dat"), &[0; 892]), Kind::Maze);
        assert_eq!(classify(Some("xeen.mon"), &[0; 120]), Kind::Monsters);
        assert_eq!(classify(None, &[0; 64000]), Kind::RawImage);
        assert_eq!(classify(None, &[1, 2, 3]), Kind::Unknown);
    }

    fn press(e: &mut Engine, k: Key) {
        e.push_input(InputEvent::KeyDown(k));
        e.run_frame(16_667);
    }

    #[test]
    fn browses_every_synthetic_entry_without_errors() {
        let mut e = Engine::from_files(EngineConfig::default(), synthetic_game_files());
        e.run_frame(16_667);
        assert!(e.status().contains("asset browser"));
        let first = e.frame_indexed();
        // Open each entry, poke the viewer, go back.
        for _ in 0..16 {
            press(&mut e, Key::Enter);
            for k in [Key::Right, Key::Up, Key::Char('d'), Key::Char('c'), Key::Down, Key::Left] {
                press(&mut e, k);
            }
            press(&mut e, Key::Escape);
            press(&mut e, Key::Down);
        }
        assert_ne!(first, e.frame_indexed());
        press(&mut e, Key::Escape);
        e.run_frame(16_667);
        assert!(e.quit_requested());
    }
}

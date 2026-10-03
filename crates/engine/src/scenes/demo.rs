// SPDX-License-Identifier: GPL-3.0-or-later
//! Screen shown without game data: exercises the whole pipeline (sprite
//! encoder -> decoder -> scaled/effect renderer -> palette -> audio -> input).

use openxeenng_formats::sprite::{SpriteBuilder, SpriteFile};

use crate::audio::{Channel, tone};
use crate::exec::Cx;
use crate::gfx::{self, DrawContext, Rect, SCALE_ENLARGE, draw_sprite_simple};
use crate::input::Key;
use crate::present::{UiColors, default_palette};
use crate::synth::{orb_image, text_image};
use crate::text::{draw_ui_text, draw_ui_text_shadow};

const DRAWER_NAMES: [&str; 7] = [
    "normal",
    "remap",
    "scramble",
    "ghost",
    "threshold",
    "fuzz",
    "xor",
];

/// Runs the demo until Escape.
pub async fn run(cx: Cx, message: Option<String>) {
    let orbs = {
        let mut b = SpriteBuilder::new();
        for hue in 0..14 {
            b.frame(orb_image(16, hue));
        }
        SpriteFile::parse(b.build().expect("orb sprite")).expect("orb sprite parses")
    };
    let title = {
        let mut b = SpriteBuilder::new();
        b.frame(text_image("OpenXeenNG", 2));
        SpriteFile::parse(b.build().expect("title sprite")).expect("title parses")
    };
    let base_palette = default_palette();
    let ui = UiColors::from_palette(&base_palette);
    let beep = tone(660.0, 120, cx.with(|io| io.mixer.rate()));

    let mut drawer: u32 = 0;
    let mut param: u32 = 0;
    let mut flipped = false;
    cx.with(|io| {
        io.palette = base_palette;
        io.status = match &message {
            Some(_) => "OpenXeenNG demo (game data rejected)".into(),
            None => "OpenXeenNG demo (no game data)".into(),
        };
    });

    loop {
        while let Some(ev) = cx.poll_event() {
            match ev.pressed_key() {
                Some(Key::Escape) => {
                    cx.with(|io| io.quit = true);
                    return;
                }
                Some(Key::Right) => drawer = (drawer + 1) % 7,
                Some(Key::Left) => drawer = (drawer + 6) % 7,
                Some(Key::Up) => param = (param + 1) % 16,
                Some(Key::Down) => param = (param + 15) % 16,
                Some(Key::Char('f')) => flipped = !flipped,
                Some(Key::Enter) | Some(Key::Space) => {
                    let b = beep.clone();
                    cx.with(|io| io.mixer.play(Channel::Sfx, b, io.mixer.rate(), false));
                }
                _ => {}
            }
        }

        cx.with(|io| {
            let t = io.ticks;
            // Palette cycling on the hue rows animates the orbs' colours.
            let mut pal = base_palette;
            let shift = (t / 2) as usize % 14;
            for h in 0..14 {
                let src = 32 + ((h + shift) % 14) * 16;
                pal[32 + h * 16..32 + h * 16 + 16].copy_from_slice(&base_palette[src..src + 16]);
            }
            io.palette = pal;

            let s = &mut io.screen;
            for y in 0..200 {
                let v = 16 + (y as u8 / 40).min(3);
                s.fill_rect(Rect::new(0, y, 320, y + 1), v);
            }
            let mut ctx = DrawContext {
                rng: &mut io.rng,
                palette: &io.palette,
                clipped_bottom: 200,
            };
            let _ = draw_sprite_simple(s, &title, 0, (96, 10), 0, SCALE_ENLARGE, &mut ctx);

            // Orbs at every scale step, like objects receding in the 3D view.
            let mut x = 4;
            for scale in (0..16).step_by(2) {
                let bob = (((t as f32 + scale as f32 * 3.0) * 0.3).sin() * 6.0) as i32;
                let mut f = if drawer == 0 {
                    0
                } else {
                    (drawer << 8) | param
                };
                if flipped {
                    f |= gfx::flags::HORIZ_FLIPPED;
                }
                let frame = (scale as usize + t as usize / 4) % 14;
                let _ = draw_sprite_simple(s, &orbs, frame, (x, 70 + bob), f, scale, &mut ctx);
                x += 38 - scale as i32 * 2;
            }

            let mut y = 120;
            let lines: Vec<String> = match &message {
                Some(m) => vec![m.clone()],
                None => vec!["No game data loaded.".to_string()],
            };
            for l in lines {
                for chunk in wrap(&l, 38) {
                    draw_ui_text_shadow(
                        s,
                        8,
                        y,
                        &chunk,
                        if message.is_some() { ui.warn } else { ui.text },
                        ui.bg,
                    );
                    y += 10;
                }
            }
            draw_ui_text_shadow(
                s,
                8,
                y,
                "Add XEEN.CC / DARK.CC to browse assets.",
                ui.dim,
                ui.bg,
            );
            let drawer_line = format!(
                "Effect: {:<9} param {:>2} {}",
                DRAWER_NAMES[drawer as usize],
                param,
                if flipped { "flipped" } else { "" }
            );
            draw_ui_text_shadow(s, 8, 164, &drawer_line, ui.hi, ui.bg);
            draw_ui_text(s, 8, 178, "</> effect  ^/v param  F flip", ui.text, None);
            draw_ui_text(s, 8, 188, "Enter beep  Esc quit", ui.text, None);
        });
        cx.next_frame().await;
    }
}

/// Greedy word wrap for the UI font.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && line.len() + 1 + word.len() > width {
            out.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
        while line.len() > width {
            let rest = line.split_off(width);
            out.push(std::mem::replace(&mut line, rest));
        }
    }
    if !line.is_empty() {
        out.push(line);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::wrap;

    #[test]
    fn wraps_words() {
        assert_eq!(wrap("aaa bbb ccc", 7), vec!["aaa bbb", "ccc"]);
        assert_eq!(wrap("abcdefghij", 4), vec!["abcd", "efgh", "ij"]);
    }
}

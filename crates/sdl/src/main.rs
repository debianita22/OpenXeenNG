// SPDX-License-Identifier: GPL-3.0-or-later
//! OpenXeenNG desktop frontend (SDL2).
//!
//! ```text
//! openxeenng [--data DIR] [--scale N] [--fullscreen] [--no-aspect] [--dac full|shift]
//!          [--frames N --screenshot out.bmp] [--keys up,enter,...]
//! ```
//!
//! Without `--data`, the current directory is used when it holds the game
//! archives; otherwise the demo runs. Dropping a file or folder of the game
//! onto the window (re)loads it.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use openxeenng_engine::{Engine, EngineConfig, GamepadButton, HEIGHT, InputEvent, Key, WIDTH};
use openxeenng_formats::palette::DacScaling;
use sdl2::audio::{AudioQueue, AudioSpecDesired};
use sdl2::controller::{Button, GameController};
use sdl2::event::{Event, WindowEvent};
use sdl2::keyboard::Keycode;
use sdl2::mouse::MouseButton;
use sdl2::pixels::PixelFormatEnum;
use sdl2::rect::Rect;

const AUDIO_RATE: i32 = 48_000;

struct Options {
    data: Option<PathBuf>,
    scale: u32,
    fullscreen: bool,
    aspect: bool,
    dac: DacScaling,
    frames: Option<u64>,
    screenshot: Option<PathBuf>,
    keys: Vec<Key>,
}

fn usage() -> ! {
    eprintln!(
        "usage: openxeenng [--data DIR] [--scale N] [--fullscreen] [--no-aspect] [--dac full|shift]\n\
         \x20               [--frames N] [--screenshot out.bmp] [--keys k1,k2,...]"
    );
    std::process::exit(2);
}

fn parse_args() -> Options {
    let mut o = Options {
        data: None,
        scale: 3,
        fullscreen: false,
        aspect: true,
        dac: DacScaling::Full,
        frames: None,
        screenshot: None,
        keys: Vec::new(),
    };
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut val = || args.next().unwrap_or_else(|| usage());
        match a.as_str() {
            "--data" => o.data = Some(PathBuf::from(val())),
            "--scale" => o.scale = val().parse().unwrap_or_else(|_| usage()),
            "--fullscreen" => o.fullscreen = true,
            "--no-aspect" => o.aspect = false,
            "--dac" => {
                o.dac = match val().as_str() {
                    "shift" => DacScaling::Shift,
                    "full" => DacScaling::Full,
                    _ => usage(),
                }
            }
            "--frames" => o.frames = Some(val().parse().unwrap_or_else(|_| usage())),
            "--screenshot" => o.screenshot = Some(PathBuf::from(val())),
            "--keys" => {
                o.keys = val()
                    .split(',')
                    .filter(|s| !s.is_empty())
                    .map(|k| parse_key_name(k).unwrap_or_else(|| usage()))
                    .collect()
            }
            "-h" | "--help" => usage(),
            _ => usage(),
        }
    }
    o.scale = o.scale.clamp(1, 8);
    o
}

fn parse_key_name(k: &str) -> Option<Key> {
    Some(match k.to_ascii_lowercase().as_str() {
        "up" => Key::Up,
        "down" => Key::Down,
        "left" => Key::Left,
        "right" => Key::Right,
        "enter" => Key::Enter,
        "esc" => Key::Escape,
        "space" => Key::Space,
        "tab" => Key::Tab,
        s if s.len() == 1 => Key::Char(s.chars().next()?),
        _ => return None,
    })
}

/// Reads every file of the directory holding the game (case-insensitive names).
fn load_game_dir(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for e in rd.flatten() {
        let p = e.path();
        let Some(name) = p.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        // Only what the engine can use; avoids reading unrelated big files.
        let lower = name.to_ascii_lowercase();
        let wanted = lower.ends_with(".cc")
            || lower.ends_with(".cur")
            || lower.ends_with(".dat")
            || lower.ends_with(".pal");
        if p.is_file() && wanted {
            if let Ok(bytes) = std::fs::read(&p) {
                out.push((name.to_string(), bytes));
            }
        }
    }
    out
}

fn has_game(dir: &Path) -> bool {
    ["xeen.cc", "dark.cc", "swrd.cc"].iter().any(|n| {
        std::fs::read_dir(dir)
            .map(|rd| {
                rd.flatten()
                    .any(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(n))
            })
            .unwrap_or(false)
    })
}

fn map_keycode(k: Keycode) -> Option<Key> {
    Some(match k {
        Keycode::Up | Keycode::Kp8 => Key::Up,
        Keycode::Down | Keycode::Kp2 => Key::Down,
        Keycode::Left | Keycode::Kp4 => Key::Left,
        Keycode::Right | Keycode::Kp6 => Key::Right,
        Keycode::Return | Keycode::KpEnter => Key::Enter,
        Keycode::Escape => Key::Escape,
        Keycode::Space => Key::Space,
        Keycode::Tab => Key::Tab,
        Keycode::Backspace => Key::Backspace,
        Keycode::PageUp => Key::PageUp,
        Keycode::PageDown => Key::PageDown,
        Keycode::Home => Key::Home,
        Keycode::End => Key::End,
        Keycode::F1 => Key::F(1),
        Keycode::F2 => Key::F(2),
        Keycode::F3 => Key::F(3),
        Keycode::F4 => Key::F(4),
        Keycode::F5 => Key::F(5),
        Keycode::F6 => Key::F(6),
        Keycode::F7 => Key::F(7),
        Keycode::F8 => Key::F(8),
        Keycode::F9 => Key::F(9),
        Keycode::F10 => Key::F(10),
        Keycode::F12 => Key::F(12),
        other => {
            let code = other.into_i32();
            if (0x20..0x7F).contains(&code) {
                Key::Char((code as u8 as char).to_ascii_lowercase())
            } else {
                return None;
            }
        }
    })
}

fn map_button(b: Button) -> Option<GamepadButton> {
    Some(match b {
        Button::DPadUp => GamepadButton::Up,
        Button::DPadDown => GamepadButton::Down,
        Button::DPadLeft => GamepadButton::Left,
        Button::DPadRight => GamepadButton::Right,
        Button::A => GamepadButton::South,
        Button::B => GamepadButton::East,
        Button::X => GamepadButton::West,
        Button::Y => GamepadButton::North,
        Button::LeftShoulder => GamepadButton::L,
        Button::RightShoulder => GamepadButton::R,
        Button::Start => GamepadButton::Start,
        Button::Back => GamepadButton::Select,
        _ => return None,
    })
}

/// Destination rectangle of the picture inside the window.
fn picture_rect(win_w: u32, win_h: u32, aspect: bool) -> Rect {
    let (aw, ah) = if aspect { (4.0, 3.0) } else { (320.0, 200.0) };
    let target = aw / ah;
    let (w, h) = if win_w as f32 / win_h as f32 > target {
        ((win_h as f32 * target) as u32, win_h)
    } else {
        (win_w, (win_w as f32 / target) as u32)
    };
    Rect::new(
        ((win_w - w) / 2) as i32,
        ((win_h - h) / 2) as i32,
        w.max(1),
        h.max(1),
    )
}

fn main() {
    if let Err(e) = run(parse_args()) {
        eprintln!("openxeenng: {e}");
        std::process::exit(1);
    }
}

fn run(opts: Options) -> Result<(), String> {
    // Crisp integer upscale first, then a smooth final stretch ("sharp bilinear").
    sdl2::hint::set("SDL_RENDER_SCALE_QUALITY", "0");
    let sdl = sdl2::init()?;
    let video = sdl.video()?;
    let config = EngineConfig {
        dac: opts.dac,
        audio_rate: AUDIO_RATE as u32,
        ..EngineConfig::default()
    };

    let data_dir = opts.data.clone().or_else(|| {
        let cwd = std::env::current_dir().ok()?;
        has_game(&cwd).then_some(cwd)
    });
    let mut engine = Engine::from_files(
        config,
        data_dir.as_deref().map(load_game_dir).unwrap_or_default(),
    );

    let win_w = WIDTH as u32 * opts.scale;
    let win_h = if opts.aspect {
        win_w * 3 / 4
    } else {
        HEIGHT as u32 * opts.scale
    };
    let mut wb = video.window("OpenXeenNG", win_w, win_h);
    wb.position_centered().resizable().allow_highdpi();
    if opts.fullscreen {
        wb.fullscreen_desktop();
    }
    let window = wb.build().map_err(|e| e.to_string())?;
    let headless = opts.frames.is_some();
    let mut cb = window.into_canvas().accelerated().target_texture();
    if !headless {
        cb = cb.present_vsync();
    }
    let mut canvas = match cb.build() {
        Ok(c) => c,
        Err(_) => {
            // Software fallback (e.g. offscreen/dummy drivers in CI).
            let window = video
                .window("OpenXeenNG", win_w, win_h)
                .build()
                .map_err(|e| e.to_string())?;
            window
                .into_canvas()
                .software()
                .target_texture()
                .build()
                .map_err(|e| e.to_string())?
        }
    };
    let creator = canvas.texture_creator();
    let mut frame_tex = creator
        .create_texture_streaming(PixelFormatEnum::ARGB8888, WIDTH as u32, HEIGHT as u32)
        .map_err(|e| e.to_string())?;
    let up = opts.scale.max(2);
    sdl2::hint::set("SDL_RENDER_SCALE_QUALITY", "1");
    let mut big_tex = creator
        .create_texture_target(
            PixelFormatEnum::ARGB8888,
            WIDTH as u32 * up,
            HEIGHT as u32 * up,
        )
        .map_err(|e| e.to_string())?;

    let audio_sub = sdl.audio().ok();
    let queue: Option<AudioQueue<i16>> = audio_sub.as_ref().and_then(|a| {
        a.open_queue(
            None,
            &AudioSpecDesired {
                freq: Some(AUDIO_RATE),
                channels: Some(2),
                samples: Some(1024),
            },
        )
        .ok()
    });
    if let Some(q) = &queue {
        q.resume();
    }
    let controller_sub = sdl.game_controller().ok();
    let mut controllers: Vec<GameController> = Vec::new();

    let mut events = sdl.event_pump()?;
    let mut pixels = vec![0u32; WIDTH * HEIGHT];
    let mut audio_buf: Vec<i16> = Vec::new();
    let mut last = Instant::now();
    let mut frame_no: u64 = 0;
    let mut scripted = opts.keys.clone().into_iter();

    'main: loop {
        for ev in events.poll_iter() {
            match ev {
                Event::Quit { .. } => break 'main,
                Event::KeyDown {
                    keycode: Some(k), ..
                } => {
                    if k == Keycode::F11 {
                        use sdl2::video::FullscreenType;
                        let w = canvas.window_mut();
                        let next = if w.fullscreen_state() == FullscreenType::Off {
                            FullscreenType::Desktop
                        } else {
                            FullscreenType::Off
                        };
                        let _ = w.set_fullscreen(next);
                    } else if let Some(key) = map_keycode(k) {
                        engine.push_input(InputEvent::KeyDown(key));
                    }
                }
                Event::KeyUp {
                    keycode: Some(k), ..
                } => {
                    if let Some(key) = map_keycode(k) {
                        engine.push_input(InputEvent::KeyUp(key));
                    }
                }
                Event::ControllerDeviceAdded { which, .. } => {
                    if let Some(c) = controller_sub.as_ref().and_then(|s| s.open(which).ok()) {
                        controllers.push(c);
                    }
                }
                Event::ControllerButtonDown { button, .. } => {
                    if let Some(b) = map_button(button) {
                        engine.push_input(InputEvent::KeyDown(b.default_key()));
                    }
                }
                Event::ControllerButtonUp { button, .. } => {
                    if let Some(b) = map_button(button) {
                        engine.push_input(InputEvent::KeyUp(b.default_key()));
                    }
                }
                Event::MouseMotion { x, y, .. } | Event::MouseButtonDown { x, y, .. } => {
                    let (ww, wh) = canvas.output_size()?;
                    let (lw, lh) = canvas.window().size();
                    let (sx, sy) = (x * ww as i32 / lw as i32, y * wh as i32 / lh as i32);
                    let r = picture_rect(ww, wh, opts.aspect);
                    let gx = (sx - r.x()) * WIDTH as i32 / r.width() as i32;
                    let gy = (sy - r.y()) * HEIGHT as i32 / r.height() as i32;
                    if let Event::MouseButtonDown { mouse_btn, .. } = ev {
                        engine.push_input(InputEvent::PointerDown {
                            x: gx,
                            y: gy,
                            secondary: mouse_btn == MouseButton::Right,
                        });
                    } else {
                        engine.push_input(InputEvent::PointerMove { x: gx, y: gy });
                    }
                }
                Event::MouseButtonUp { .. } => engine.push_input(InputEvent::PointerUp),
                Event::DropFile { filename, .. } => {
                    let p = PathBuf::from(filename);
                    let dir = if p.is_dir() {
                        p
                    } else {
                        p.parent().map(Path::to_path_buf).unwrap_or(p)
                    };
                    engine = Engine::from_files(config, load_game_dir(&dir));
                }
                Event::Window {
                    win_event: WindowEvent::Close,
                    ..
                } => break 'main,
                _ => {}
            }
        }

        // Scripted keys (automation/tests): one per frame.
        if let Some(k) = scripted.next() {
            engine.push_input(InputEvent::KeyDown(k));
        }

        let now = Instant::now();
        let dt = if headless {
            16_667
        } else {
            (now - last).as_micros().min(100_000) as u64
        };
        last = now;
        engine.run_frame(dt);
        frame_no += 1;
        if engine.quit_requested() {
            break;
        }

        // Audio: keep ~70 ms queued.
        if let Some(q) = &queue {
            let target = AUDIO_RATE as u32 * 2 * 2 * 70 / 1000;
            let queued = q.size();
            if queued < target {
                let frames = ((target - queued) / 4) as usize;
                audio_buf.resize(frames * 2, 0);
                engine.mix_audio(&mut audio_buf);
                let _ = q.queue_audio(&audio_buf);
            }
        }

        // Video.
        engine.frame_xrgb8888(&mut pixels);
        frame_tex
            .with_lock(None, |buf: &mut [u8], pitch: usize| {
                for (y, row) in pixels.chunks_exact(WIDTH).enumerate() {
                    let line = &mut buf[y * pitch..y * pitch + WIDTH * 4];
                    for (dst, &px) in line.chunks_exact_mut(4).zip(row) {
                        dst.copy_from_slice(&(px | 0xFF00_0000).to_ne_bytes());
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        canvas
            .with_texture_canvas(&mut big_tex, |c| {
                let _ = c.copy(&frame_tex, None, None);
            })
            .map_err(|e| e.to_string())?;
        canvas.set_draw_color(sdl2::pixels::Color::RGB(0, 0, 0));
        canvas.clear();
        let (ww, wh) = canvas.output_size()?;
        canvas.copy(&big_tex, None, Some(picture_rect(ww, wh, opts.aspect)))?;
        canvas.present();
        let title = engine.status();
        if canvas.window().title() != title {
            let _ = canvas.window_mut().set_title(&title);
        }

        if let Some(n) = opts.frames {
            if frame_no >= n {
                if let Some(path) = &opts.screenshot {
                    save_screenshot(&engine, path)?;
                }
                break;
            }
        }
        if headless {
            continue;
        }
        // Without vsync (e.g. some drivers), don't spin.
        let spent = now.elapsed();
        if spent < Duration::from_millis(4) {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    Ok(())
}

/// Saves the game picture (not the scaled window) as a 24-bit BMP.
fn save_screenshot(engine: &Engine, path: &Path) -> Result<(), String> {
    let mut rgba = vec![0u8; WIDTH * HEIGHT * 4];
    engine.frame_rgba(&mut rgba);
    let mut rgb: Vec<u8> = rgba
        .chunks_exact(4)
        .flat_map(|s| [s[0], s[1], s[2]])
        .collect();
    let surf = sdl2::surface::Surface::from_data(
        &mut rgb,
        WIDTH as u32,
        HEIGHT as u32,
        WIDTH as u32 * 3,
        PixelFormatEnum::RGB24,
    )?;
    surf.save_bmp(path)
}

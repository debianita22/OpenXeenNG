// SPDX-License-Identifier: GPL-3.0-or-later
//! OpenXeen engine core: platform-neutral, frame-stepped, no I/O.
//!
//! Frontends (SDL2 desktop, libretro core, browser) own the window, input
//! devices, audio device and file access. Each video frame they:
//! 1. forward input with [`Engine::push_input`];
//! 2. call [`Engine::run_frame`] with the elapsed time;
//! 3. read the picture with [`Engine::frame_rgba`] / [`Engine::frame_xrgb8888`];
//! 4. pull audio with [`Engine::mix_audio`].
//!
//! Game logic runs as `async` flows advanced once per frame (see [`exec`]).

#![warn(missing_docs)]

pub mod audio;
pub mod exec;
mod font8x8;
pub mod gfx;
pub mod input;
pub mod present;
pub mod rng;
pub mod scenes;
pub mod synth;
pub mod text;

use std::cell::RefCell;
use std::rc::Rc;

use openxeen_formats::gamedata::GameData;
use openxeen_formats::palette::DacScaling;

use crate::exec::{Cx, Io, Task};
pub use crate::input::{GamepadButton, InputEvent, Key};

/// Width of the game screen.
pub const WIDTH: usize = gfx::SCREEN_WIDTH;
/// Height of the game screen.
pub const HEIGHT: usize = gfx::SCREEN_HEIGHT;
/// Display aspect ratio of the original (320x200 shown on a 4:3 monitor).
pub const DISPLAY_ASPECT: f32 = 4.0 / 3.0;

/// Engine settings chosen by the frontend.
#[derive(Debug, Clone, Copy)]
pub struct EngineConfig {
    /// How 6-bit VGA levels are expanded for display.
    pub dac: DacScaling,
    /// Output sample rate for [`Engine::mix_audio`].
    pub audio_rate: u32,
    /// Seed for the random number generator.
    pub seed: u32,
}

impl Default for EngineConfig {
    fn default() -> Self {
        EngineConfig {
            dac: DacScaling::Full,
            audio_rate: 48_000,
            seed: 0x5EED_1234,
        }
    }
}

/// The engine instance a frontend drives.
pub struct Engine {
    io: Rc<RefCell<Io>>,
    task: Task,
    lut: [u8; 256],
}

impl Engine {
    /// Starts the engine. With game data the asset browser runs (the game
    /// itself arrives with the next milestones); without it, the demo.
    pub fn new(config: EngineConfig, data: Option<GameData>) -> Self {
        Self::start(config, data.map(Rc::new), None)
    }

    /// Starts from raw `(file name, bytes)` pairs, e.g. everything the user
    /// picked in a browser file dialog. Invalid or missing data falls back to
    /// the demo, which shows the reason on screen.
    pub fn from_files(config: EngineConfig, files: Vec<(String, Vec<u8>)>) -> Self {
        if files.is_empty() {
            return Self::start(config, None, None);
        }
        match GameData::from_files(files) {
            Ok(d) => Self::start(config, Some(Rc::new(d)), None),
            Err(e) => Self::start(config, None, Some(format!("Game data not usable: {e}"))),
        }
    }

    fn start(config: EngineConfig, data: Option<Rc<GameData>>, message: Option<String>) -> Self {
        let io = Rc::new(RefCell::new(Io::new(config.audio_rate, config.seed)));
        let cx = Cx::new(io.clone());
        let task = Task::new(scenes::main_flow(cx, data, message));
        Engine {
            io,
            task,
            lut: present::level_lut(config.dac),
        }
    }

    /// Queues an input event for the next frame.
    pub fn push_input(&mut self, ev: InputEvent) {
        let mut io = self.io.borrow_mut();
        if io.events.len() < 64 {
            io.events.push_back(ev);
        }
    }

    /// Advances time by `dt_us` microseconds and runs one frame of logic.
    pub fn run_frame(&mut self, dt_us: u64) {
        self.io.borrow_mut().advance(dt_us);
        if self.task.poll_once() {
            self.io.borrow_mut().quit = true;
        }
    }

    /// Writes the screen as RGBA8888 bytes (`WIDTH * HEIGHT * 4`).
    pub fn frame_rgba(&self, out: &mut [u8]) {
        let io = self.io.borrow();
        for (px, &idx) in out.chunks_exact_mut(4).zip(io.screen.pixels()) {
            let c = io.palette[usize::from(idx)];
            px[0] = self.lut[usize::from(c[0])];
            px[1] = self.lut[usize::from(c[1])];
            px[2] = self.lut[usize::from(c[2])];
            px[3] = 255;
        }
    }

    /// Writes the screen as `0x00RRGGBB` words (`WIDTH * HEIGHT`).
    pub fn frame_xrgb8888(&self, out: &mut [u32]) {
        let io = self.io.borrow();
        for (px, &idx) in out.iter_mut().zip(io.screen.pixels()) {
            let c = io.palette[usize::from(idx)];
            *px = u32::from(self.lut[usize::from(c[0])]) << 16
                | u32::from(self.lut[usize::from(c[1])]) << 8
                | u32::from(self.lut[usize::from(c[2])]);
        }
    }

    /// The raw indexed screen (for tests and screenshots).
    pub fn frame_indexed(&self) -> Vec<u8> {
        self.io.borrow().screen.pixels().to_vec()
    }

    /// Fills interleaved stereo samples at [`Engine::audio_rate`].
    pub fn mix_audio(&mut self, out: &mut [i16]) {
        self.io.borrow_mut().mixer.render(out);
    }

    /// Output sample rate.
    pub fn audio_rate(&self) -> u32 {
        self.io.borrow().mixer.rate()
    }

    /// True once the game asked to exit.
    pub fn quit_requested(&self) -> bool {
        self.io.borrow().quit
    }

    /// Short status text (window title / OSD).
    pub fn status(&self) -> String {
        self.io.borrow().status.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_renders_something() {
        let mut e = Engine::new(EngineConfig::default(), None);
        for _ in 0..10 {
            e.run_frame(16_667);
        }
        let mut rgba = vec![0u8; WIDTH * HEIGHT * 4];
        e.frame_rgba(&mut rgba);
        let distinct: std::collections::HashSet<_> =
            rgba.chunks(4).map(|c| [c[0], c[1], c[2]]).collect();
        assert!(distinct.len() > 8, "only {} colours", distinct.len());
        assert!(!e.quit_requested());
        e.push_input(InputEvent::KeyDown(Key::Escape));
        e.run_frame(16_667);
        e.run_frame(16_667);
        assert!(e.quit_requested());
    }

    #[test]
    fn bad_files_fall_back_to_demo_with_message() {
        let mut e = Engine::from_files(
            EngineConfig::default(),
            vec![("xeen.cc".into(), vec![1, 2, 3])],
        );
        e.run_frame(16_667);
        assert!(e.status().contains("demo"));
    }
}

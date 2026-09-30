// SPDX-License-Identifier: GPL-3.0-or-later
//! OpenXeen for the browser.
//!
//! A plain WebAssembly module with a tiny C-style API used by `www/main.js`
//! (no wasm-bindgen): the page copies the files the player picks into wasm
//! memory, starts the engine, then calls [`ox_frame`] from
//! `requestAnimationFrame` and reads back RGBA pixels and PCM audio.
//! Files never leave the player's browser.

use std::cell::RefCell;

use openxeen_engine::{Engine, EngineConfig, HEIGHT, InputEvent, Key, WIDTH};

struct State {
    files: Vec<(String, Vec<u8>)>,
    engine: Option<Engine>,
    rgba: Vec<u8>,
    audio: Vec<i16>,
    status: Vec<u8>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State {
        files: Vec::new(),
        engine: None,
        rgba: vec![0; WIDTH * HEIGHT * 4],
        audio: Vec::new(),
        status: Vec::new(),
    });
}

/// Allocates `len` bytes the page can fill (file names and contents).
#[unsafe(no_mangle)]
pub extern "C" fn ox_alloc(len: usize) -> *mut u8 {
    let mut v = vec![0u8; len.max(1)];
    let p = v.as_mut_ptr();
    std::mem::forget(v);
    p
}

/// Frees a buffer from [`ox_alloc`] that was not handed to [`ox_add_file`].
///
/// # Safety
/// `ptr`/`len` must come from `ox_alloc` and not be used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ox_free(ptr: *mut u8, len: usize) {
    // SAFETY: caller contract.
    drop(unsafe { Vec::from_raw_parts(ptr, len.max(1), len.max(1)) });
}

/// Hands a picked file to the engine (takes ownership of both buffers).
///
/// # Safety
/// Both buffers must come from `ox_alloc` with exactly these lengths; the
/// name must be UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ox_add_file(
    name: *mut u8,
    name_len: usize,
    data: *mut u8,
    data_len: usize,
) {
    // SAFETY: caller contract; ox_alloc allocates max(len, 1) bytes.
    let name = unsafe { Vec::from_raw_parts(name, name_len, name_len.max(1)) };
    let data = unsafe { Vec::from_raw_parts(data, data_len, data_len.max(1)) };
    let name = String::from_utf8_lossy(&name).into_owned();
    STATE.with_borrow_mut(|s| s.files.push((name, data)));
}

/// Starts (or restarts) the engine with the files added so far.
#[unsafe(no_mangle)]
pub extern "C" fn ox_start(audio_rate: u32) {
    STATE.with_borrow_mut(|s| {
        let files = std::mem::take(&mut s.files);
        let config = EngineConfig {
            audio_rate: audio_rate.clamp(8_000, 192_000),
            ..EngineConfig::default()
        };
        s.engine = Some(Engine::from_files(config, files));
    });
}

/// Starts the engine on the built-in synthetic data set (no game needed).
#[unsafe(no_mangle)]
pub extern "C" fn ox_start_synthetic(audio_rate: u32) {
    STATE.with_borrow_mut(|s| {
        s.files = openxeen_engine::synth::synthetic_game_files();
    });
    ox_start(audio_rate);
}

/// Key codes shared with `main.js`: 1..=13 named keys, 101..=112 F-keys,
/// `0x1_0000 + codepoint` for printable characters.
pub fn decode_key(code: u32) -> Option<Key> {
    Some(match code {
        1 => Key::Up,
        2 => Key::Down,
        3 => Key::Left,
        4 => Key::Right,
        5 => Key::Enter,
        6 => Key::Escape,
        7 => Key::Space,
        8 => Key::Tab,
        9 => Key::Backspace,
        10 => Key::PageUp,
        11 => Key::PageDown,
        12 => Key::Home,
        13 => Key::End,
        101..=112 => Key::F((code - 100) as u8),
        c if c >= 0x1_0000 => Key::Char(char::from_u32(c - 0x1_0000)?.to_ascii_lowercase()),
        _ => return None,
    })
}

/// Key press (`down` = 1) or release (0).
#[unsafe(no_mangle)]
pub extern "C" fn ox_key(code: u32, down: u32) {
    let Some(k) = decode_key(code) else { return };
    STATE.with_borrow_mut(|s| {
        if let Some(e) = s.engine.as_mut() {
            e.push_input(if down != 0 {
                InputEvent::KeyDown(k)
            } else {
                InputEvent::KeyUp(k)
            });
        }
    });
}

/// Pointer event in game pixels: kind 0 = move, 1 = primary down,
/// 2 = secondary down, 3 = up.
#[unsafe(no_mangle)]
pub extern "C" fn ox_pointer(kind: u32, x: i32, y: i32) {
    let ev = match kind {
        0 => InputEvent::PointerMove { x, y },
        1 => InputEvent::PointerDown {
            x,
            y,
            secondary: false,
        },
        2 => InputEvent::PointerDown {
            x,
            y,
            secondary: true,
        },
        _ => InputEvent::PointerUp,
    };
    STATE.with_borrow_mut(|s| {
        if let Some(e) = s.engine.as_mut() {
            e.push_input(ev);
        }
    });
}

/// Runs one frame and returns a pointer to `WIDTH*HEIGHT*4` RGBA bytes.
#[unsafe(no_mangle)]
pub extern "C" fn ox_frame(dt_us: u32) -> *const u8 {
    STATE.with_borrow_mut(|s| {
        let State { engine, rgba, .. } = s;
        if let Some(e) = engine.as_mut() {
            e.run_frame(u64::from(dt_us.min(250_000)));
            e.frame_rgba(rgba);
        }
        rgba.as_ptr()
    })
}

/// Mixes `frames` stereo frames; returns a pointer to `frames*2` `i16`s.
#[unsafe(no_mangle)]
pub extern "C" fn ox_audio(frames: u32) -> *const i16 {
    STATE.with_borrow_mut(|s| {
        let State { engine, audio, .. } = s;
        audio.resize(frames as usize * 2, 0);
        match engine.as_mut() {
            Some(e) => e.mix_audio(audio),
            None => audio.fill(0),
        }
        audio.as_ptr()
    })
}

/// 1 once the game asked to quit.
#[unsafe(no_mangle)]
pub extern "C" fn ox_quit_requested() -> u32 {
    STATE.with_borrow(|s| u32::from(s.engine.as_ref().is_some_and(Engine::quit_requested)))
}

/// Status line as UTF-8: returns the pointer, writes the length to `len_out`.
///
/// # Safety
/// `len_out` must be a valid pointer to a `u32`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ox_status(len_out: *mut u32) -> *const u8 {
    STATE.with_borrow_mut(|s| {
        s.status = s
            .engine
            .as_ref()
            .map(Engine::status)
            .unwrap_or_default()
            .into_bytes();
        // SAFETY: caller contract.
        unsafe { *len_out = s.status.len() as u32 };
        s.status.as_ptr()
    })
}

/// Screen width (for the page).
#[unsafe(no_mangle)]
pub extern "C" fn ox_width() -> u32 {
    WIDTH as u32
}

/// Screen height (for the page).
#[unsafe(no_mangle)]
pub extern "C" fn ox_height() -> u32 {
    HEIGHT as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_round_trip() {
        let name = b"xeen.cc";
        let data = openxeen_engine::synth::synthetic_xeen_cc();
        let np = ox_alloc(name.len());
        let dp = ox_alloc(data.len());
        unsafe {
            std::ptr::copy_nonoverlapping(name.as_ptr(), np, name.len());
            std::ptr::copy_nonoverlapping(data.as_ptr(), dp, data.len());
            ox_add_file(np, name.len(), dp, data.len());
        }
        ox_start(48_000);
        let p = ox_frame(16_667);
        let px = unsafe { std::slice::from_raw_parts(p, WIDTH * HEIGHT * 4) };
        assert!(px.chunks(4).any(|c| c[0] > 100));
        let mut len = 0u32;
        let s = unsafe { ox_status(&mut len) };
        let status = unsafe { std::slice::from_raw_parts(s, len as usize) };
        assert!(
            std::str::from_utf8(status)
                .unwrap()
                .contains("asset browser")
        );
        ox_key(6, 1);
        ox_frame(16_667);
        ox_frame(16_667);
        assert_eq!(ox_quit_requested(), 1);
        assert_eq!(decode_key(0x1_0000 + 'Q' as u32), Some(Key::Char('q')));
        let a = ox_audio(800);
        assert!(!a.is_null());
    }
}

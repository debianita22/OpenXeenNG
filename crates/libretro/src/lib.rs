// SPDX-License-Identifier: GPL-3.0-or-later
//! OpenXeen as a libretro core.
//!
//! Load any file of the game directory (e.g. `XEEN.CC`) as content; every
//! archive next to it is picked up. Started without content, the core runs
//! the demo. Video is 320x200 XRGB8888 shown at 4:3, audio 48 kHz stereo,
//! 60 frames per second.
//!
//! Input: the RetroPad is mapped like the other frontends (D-pad = arrows,
//! A = Enter, B = Escape, X = Space, Y = Tab, L/R = page up/down), and a
//! keyboard works directly when the frontend forwards it.
//!
//! Only the small subset of `libretro.h` the core needs is declared here.

#![allow(clippy::missing_safety_doc)]

use std::ffi::{CStr, c_char, c_uint, c_void};
use std::path::Path;
use std::sync::Mutex;

use openxeen_engine::{Engine, EngineConfig, GamepadButton, HEIGHT, InputEvent, Key, WIDTH};

// --- libretro.h subset -------------------------------------------------------

/// `RETRO_API_VERSION`.
pub const RETRO_API_VERSION: c_uint = 1;
const RETRO_DEVICE_JOYPAD: c_uint = 1;
const RETRO_ENVIRONMENT_SHUTDOWN: c_uint = 7;
const RETRO_ENVIRONMENT_SET_PIXEL_FORMAT: c_uint = 10;
const RETRO_ENVIRONMENT_SET_INPUT_DESCRIPTORS: c_uint = 11;
const RETRO_ENVIRONMENT_SET_KEYBOARD_CALLBACK: c_uint = 12;
const RETRO_ENVIRONMENT_SET_SUPPORT_NO_GAME: c_uint = 18;
const RETRO_PIXEL_FORMAT_XRGB8888: c_uint = 1;
const RETRO_REGION_NTSC: c_uint = 0;

/// `struct retro_system_info`.
#[repr(C)]
pub struct RetroSystemInfo {
    /// Core name.
    pub library_name: *const c_char,
    /// Core version.
    pub library_version: *const c_char,
    /// Content extensions, `|` separated.
    pub valid_extensions: *const c_char,
    /// The core wants a path, not a memory buffer.
    pub need_fullpath: bool,
    /// Don't extract archives.
    pub block_extract: bool,
}

/// `struct retro_game_geometry`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RetroGameGeometry {
    /// Nominal width.
    pub base_width: c_uint,
    /// Nominal height.
    pub base_height: c_uint,
    /// Maximum width.
    pub max_width: c_uint,
    /// Maximum height.
    pub max_height: c_uint,
    /// Display aspect ratio.
    pub aspect_ratio: f32,
}

/// `struct retro_system_timing`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RetroSystemTiming {
    /// Frames per second.
    pub fps: f64,
    /// Audio sample rate.
    pub sample_rate: f64,
}

/// `struct retro_system_av_info`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RetroSystemAvInfo {
    /// Geometry.
    pub geometry: RetroGameGeometry,
    /// Timing.
    pub timing: RetroSystemTiming,
}

/// `struct retro_game_info`.
#[repr(C)]
pub struct RetroGameInfo {
    /// Content path.
    pub path: *const c_char,
    /// Content data (unused: `need_fullpath`).
    pub data: *const c_void,
    /// Content size.
    pub size: usize,
    /// Metadata.
    pub meta: *const c_char,
}

#[repr(C)]
struct RetroInputDescriptor {
    port: c_uint,
    device: c_uint,
    index: c_uint,
    id: c_uint,
    description: *const c_char,
}

#[repr(C)]
struct RetroKeyboardCallback {
    callback: KeyboardEventFn,
}

/// `retro_environment_t`.
pub type EnvironmentFn = unsafe extern "C" fn(cmd: c_uint, data: *mut c_void) -> bool;
/// `retro_video_refresh_t`.
pub type VideoRefreshFn =
    unsafe extern "C" fn(data: *const c_void, width: c_uint, height: c_uint, pitch: usize);
/// `retro_audio_sample_t`.
pub type AudioSampleFn = unsafe extern "C" fn(left: i16, right: i16);
/// `retro_audio_sample_batch_t`.
pub type AudioSampleBatchFn = unsafe extern "C" fn(data: *const i16, frames: usize) -> usize;
/// `retro_input_poll_t`.
pub type InputPollFn = unsafe extern "C" fn();
/// `retro_input_state_t`.
pub type InputStateFn =
    unsafe extern "C" fn(port: c_uint, device: c_uint, index: c_uint, id: c_uint) -> i16;
type KeyboardEventFn =
    unsafe extern "C" fn(down: bool, keycode: c_uint, character: u32, key_modifiers: u16);

// --- state -------------------------------------------------------------------

const FPS: f64 = 60.0;
const AUDIO_RATE: u32 = 48_000;
const FRAME_US: u64 = 16_667;
const AUDIO_FRAMES_PER_VIDEO_FRAME: usize = (AUDIO_RATE as usize) / 60;

#[derive(Default, Clone, Copy)]
struct Callbacks {
    environment: Option<EnvironmentFn>,
    video: Option<VideoRefreshFn>,
    audio_batch: Option<AudioSampleBatchFn>,
    input_poll: Option<InputPollFn>,
    input_state: Option<InputStateFn>,
}

struct Core {
    engine: Engine,
    pad: [bool; 16],
    video: Vec<u32>,
    audio: Vec<i16>,
    shutdown_sent: bool,
}

/// The engine is single-threaded (`Rc` inside). libretro guarantees that API
/// calls are never concurrent, so moving it between the threads a frontend
/// may call from is sound as long as every access goes through `CORE`.
struct SendCore(Core);
unsafe impl Send for SendCore {}

static CORE: Mutex<Option<SendCore>> = Mutex::new(None);
static CALLBACKS: Mutex<Callbacks> = Mutex::new(Callbacks {
    environment: None,
    video: None,
    audio_batch: None,
    input_poll: None,
    input_state: None,
});
static KEY_QUEUE: Mutex<Vec<InputEvent>> = Mutex::new(Vec::new());

fn callbacks() -> Callbacks {
    *CALLBACKS.lock().unwrap_or_else(|e| e.into_inner())
}

fn with_callbacks(f: impl FnOnce(&mut Callbacks)) {
    f(&mut CALLBACKS.lock().unwrap_or_else(|e| e.into_inner()));
}

/// RetroPad id → engine button (`RETRO_DEVICE_ID_JOYPAD_*`).
const PAD_MAP: [(c_uint, GamepadButton); 12] = [
    (0, GamepadButton::South),  // B
    (1, GamepadButton::West),   // Y
    (2, GamepadButton::Select), // SELECT
    (3, GamepadButton::Start),  // START
    (4, GamepadButton::Up),
    (5, GamepadButton::Down),
    (6, GamepadButton::Left),
    (7, GamepadButton::Right),
    (8, GamepadButton::East),  // A
    (9, GamepadButton::North), // X
    (10, GamepadButton::L),
    (11, GamepadButton::R),
];

/// `RETROK_*` → engine key.
pub fn map_retro_key(code: c_uint) -> Option<Key> {
    Some(match code {
        8 => Key::Backspace,
        9 => Key::Tab,
        13 | 271 => Key::Enter,
        27 => Key::Escape,
        32 => Key::Space,
        273 | 264 => Key::Up,
        274 | 258 => Key::Down,
        275 | 262 => Key::Right,
        276 | 260 => Key::Left,
        278 => Key::Home,
        279 => Key::End,
        280 => Key::PageUp,
        281 => Key::PageDown,
        282..=293 => Key::F((code - 281) as u8),
        33..=126 => Key::Char((code as u8 as char).to_ascii_lowercase()),
        _ => return None,
    })
}

unsafe extern "C" fn keyboard_event(down: bool, keycode: c_uint, _character: u32, _mods: u16) {
    if let Some(k) = map_retro_key(keycode) {
        let ev = if down {
            InputEvent::KeyDown(k)
        } else {
            InputEvent::KeyUp(k)
        };
        let mut q = KEY_QUEUE.lock().unwrap_or_else(|e| e.into_inner());
        if q.len() < 64 {
            q.push(ev);
        }
    }
}

fn load_dir(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    rd.flatten()
        .filter_map(|e| {
            let p = e.path();
            let name = p.file_name()?.to_str()?.to_string();
            let lower = name.to_ascii_lowercase();
            let wanted =
                lower.ends_with(".cc") || lower.ends_with(".cur") || lower.ends_with(".dat");
            if !p.is_file() || !wanted {
                return None;
            }
            std::fs::read(&p).ok().map(|b| (name, b))
        })
        .collect()
}

fn start(files: Vec<(String, Vec<u8>)>) {
    let config = EngineConfig {
        audio_rate: AUDIO_RATE,
        ..EngineConfig::default()
    };
    let core = Core {
        engine: Engine::from_files(config, files),
        pad: [false; 16],
        video: vec![0; WIDTH * HEIGHT],
        audio: vec![0; AUDIO_FRAMES_PER_VIDEO_FRAME * 2],
        shutdown_sent: false,
    };
    *CORE.lock().unwrap_or_else(|e| e.into_inner()) = Some(SendCore(core));
}

// --- exported API -------------------------------------------------------------

/// Reports the libretro API version.
#[unsafe(no_mangle)]
pub extern "C" fn retro_api_version() -> c_uint {
    RETRO_API_VERSION
}

/// Stores the environment callback and announces no-content support.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn retro_set_environment(cb: EnvironmentFn) {
    with_callbacks(|c| c.environment = Some(cb));
    let mut yes = true;
    // SAFETY: the frontend gave us a valid callback; the pointer is valid
    // for the duration of the call.
    unsafe {
        cb(
            RETRO_ENVIRONMENT_SET_SUPPORT_NO_GAME,
            (&mut yes as *mut bool).cast(),
        );
    }
}

/// Stores the video callback.
#[unsafe(no_mangle)]
pub extern "C" fn retro_set_video_refresh(cb: VideoRefreshFn) {
    with_callbacks(|c| c.video = Some(cb));
}

/// Single-sample audio is unused (batch only).
#[unsafe(no_mangle)]
pub extern "C" fn retro_set_audio_sample(_cb: AudioSampleFn) {}

/// Stores the batch audio callback.
#[unsafe(no_mangle)]
pub extern "C" fn retro_set_audio_sample_batch(cb: AudioSampleBatchFn) {
    with_callbacks(|c| c.audio_batch = Some(cb));
}

/// Stores the input poll callback.
#[unsafe(no_mangle)]
pub extern "C" fn retro_set_input_poll(cb: InputPollFn) {
    with_callbacks(|c| c.input_poll = Some(cb));
}

/// Stores the input state callback.
#[unsafe(no_mangle)]
pub extern "C" fn retro_set_input_state(cb: InputStateFn) {
    with_callbacks(|c| c.input_state = Some(cb));
}

/// Nothing to do: the engine starts in `retro_load_game`.
#[unsafe(no_mangle)]
pub extern "C" fn retro_init() {}

/// Drops the engine.
#[unsafe(no_mangle)]
pub extern "C" fn retro_deinit() {
    *CORE.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// Static core information.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn retro_get_system_info(info: *mut RetroSystemInfo) {
    static NAME: &CStr = c"OpenXeen";
    static VERSION: &CStr = c"0.1.0";
    static EXTS: &CStr = c"cc";
    if info.is_null() {
        return;
    }
    // SAFETY: the frontend passes a valid, writable struct.
    unsafe {
        info.write(RetroSystemInfo {
            library_name: NAME.as_ptr(),
            library_version: VERSION.as_ptr(),
            valid_extensions: EXTS.as_ptr(),
            need_fullpath: true,
            block_extract: false,
        });
    }
}

/// Audio/video parameters.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn retro_get_system_av_info(info: *mut RetroSystemAvInfo) {
    if info.is_null() {
        return;
    }
    // SAFETY: the frontend passes a valid, writable struct.
    unsafe {
        info.write(RetroSystemAvInfo {
            geometry: RetroGameGeometry {
                base_width: WIDTH as c_uint,
                base_height: HEIGHT as c_uint,
                max_width: WIDTH as c_uint,
                max_height: HEIGHT as c_uint,
                aspect_ratio: openxeen_engine::DISPLAY_ASPECT,
            },
            timing: RetroSystemTiming {
                fps: FPS,
                sample_rate: f64::from(AUDIO_RATE),
            },
        });
    }
}

/// Only the RetroPad is supported.
#[unsafe(no_mangle)]
pub extern "C" fn retro_set_controller_port_device(_port: c_uint, _device: c_uint) {}

/// Restarts with the same content.
#[unsafe(no_mangle)]
pub extern "C" fn retro_reset() {
    // The engine owns its data; a reset restarts the flow from scratch by
    // rebuilding it from the files on disk on the next load. For now reset
    // is a no-op (the asset browser has no state worth resetting).
}

/// Loads content (or none: demo).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn retro_load_game(game: *const RetroGameInfo) -> bool {
    let Some(env) = callbacks().environment else {
        return false;
    };
    let mut fmt = RETRO_PIXEL_FORMAT_XRGB8888;
    // SAFETY: valid callback, pointer valid during the call.
    if !unsafe {
        env(
            RETRO_ENVIRONMENT_SET_PIXEL_FORMAT,
            (&mut fmt as *mut c_uint).cast(),
        )
    } {
        return false;
    }
    set_input_descriptors(env);
    let mut kb = RetroKeyboardCallback {
        callback: keyboard_event,
    };
    // SAFETY: as above; the frontend copies the struct.
    unsafe {
        env(
            RETRO_ENVIRONMENT_SET_KEYBOARD_CALLBACK,
            (&mut kb as *mut RetroKeyboardCallback).cast(),
        );
    }

    let mut files = Vec::new();
    // SAFETY: `game` is null or points to a valid struct whose path is a
    // NUL-terminated string (need_fullpath = true).
    if let Some(g) = unsafe { game.as_ref() } {
        if !g.path.is_null() {
            let path = unsafe { CStr::from_ptr(g.path) }
                .to_string_lossy()
                .into_owned();
            let p = Path::new(&path);
            let dir = if p.is_dir() {
                p
            } else {
                p.parent().unwrap_or(p)
            };
            files = load_dir(dir);
        }
    }
    start(files);
    true
}

fn set_input_descriptors(env: EnvironmentFn) {
    const DESCS: [(c_uint, &CStr); 12] = [
        (4, c"Up / Forward"),
        (5, c"Down / Back"),
        (6, c"Left / Turn left"),
        (7, c"Right / Turn right"),
        (8, c"Confirm (Enter)"),
        (0, c"Cancel (Esc)"),
        (9, c"Space"),
        (1, c"Tab"),
        (10, c"Page up"),
        (11, c"Page down"),
        (3, c"Start (Enter)"),
        (2, c"Select (Tab)"),
    ];
    let mut list: Vec<RetroInputDescriptor> = DESCS
        .iter()
        .map(|(id, d)| RetroInputDescriptor {
            port: 0,
            device: RETRO_DEVICE_JOYPAD,
            index: 0,
            id: *id,
            description: d.as_ptr(),
        })
        .collect();
    list.push(RetroInputDescriptor {
        port: 0,
        device: 0,
        index: 0,
        id: 0,
        description: std::ptr::null(),
    });
    // SAFETY: the list is NUL-terminated and outlives the call.
    unsafe {
        env(
            RETRO_ENVIRONMENT_SET_INPUT_DESCRIPTORS,
            list.as_mut_ptr().cast(),
        );
    }
}

/// Special content types are not supported.
#[unsafe(no_mangle)]
pub extern "C" fn retro_load_game_special(
    _t: c_uint,
    _info: *const RetroGameInfo,
    _n: usize,
) -> bool {
    false
}

/// Drops the engine.
#[unsafe(no_mangle)]
pub extern "C" fn retro_unload_game() {
    *CORE.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// NTSC timing.
#[unsafe(no_mangle)]
pub extern "C" fn retro_get_region() -> c_uint {
    RETRO_REGION_NTSC
}

/// Runs one frame.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn retro_run() {
    let cbs = callbacks();
    let mut guard = CORE.lock().unwrap_or_else(|e| e.into_inner());
    let Some(SendCore(core)) = guard.as_mut() else {
        return;
    };
    if let Some(poll) = cbs.input_poll {
        // SAFETY: frontend callback.
        unsafe { poll() };
    }
    if let Some(state) = cbs.input_state {
        for (id, button) in PAD_MAP {
            // SAFETY: frontend callback.
            let down = unsafe { state(0, RETRO_DEVICE_JOYPAD, 0, id) } != 0;
            let was = &mut core.pad[id as usize];
            if down != *was {
                *was = down;
                let k = button.default_key();
                core.engine.push_input(if down {
                    InputEvent::KeyDown(k)
                } else {
                    InputEvent::KeyUp(k)
                });
            }
        }
    }
    for ev in KEY_QUEUE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .drain(..)
    {
        core.engine.push_input(ev);
    }

    core.engine.run_frame(FRAME_US);

    core.engine.frame_xrgb8888(&mut core.video);
    if let Some(video) = cbs.video {
        // SAFETY: buffer of WIDTH*HEIGHT u32, pitch in bytes.
        unsafe {
            video(
                core.video.as_ptr().cast(),
                WIDTH as c_uint,
                HEIGHT as c_uint,
                WIDTH * 4,
            )
        };
    }
    core.engine.mix_audio(&mut core.audio);
    if let Some(batch) = cbs.audio_batch {
        let mut sent = 0;
        let total = core.audio.len() / 2;
        while sent < total {
            // SAFETY: pointer/length within the buffer.
            let n = unsafe { batch(core.audio[sent * 2..].as_ptr(), total - sent) };
            if n == 0 {
                break;
            }
            sent += n;
        }
    }
    if core.engine.quit_requested() && !core.shutdown_sent {
        core.shutdown_sent = true;
        if let Some(env) = cbs.environment {
            // SAFETY: frontend callback, no data.
            unsafe { env(RETRO_ENVIRONMENT_SHUTDOWN, std::ptr::null_mut()) };
        }
    }
}

/// Savestates are not supported yet.
#[unsafe(no_mangle)]
pub extern "C" fn retro_serialize_size() -> usize {
    0
}

/// Savestates are not supported yet.
#[unsafe(no_mangle)]
pub extern "C" fn retro_serialize(_data: *mut c_void, _size: usize) -> bool {
    false
}

/// Savestates are not supported yet.
#[unsafe(no_mangle)]
pub extern "C" fn retro_unserialize(_data: *const c_void, _size: usize) -> bool {
    false
}

/// No cheats.
#[unsafe(no_mangle)]
pub extern "C" fn retro_cheat_reset() {}

/// No cheats.
#[unsafe(no_mangle)]
pub extern "C" fn retro_cheat_set(_index: c_uint, _enabled: bool, _code: *const c_char) {}

/// No exposed memory.
#[unsafe(no_mangle)]
pub extern "C" fn retro_get_memory_data(_id: c_uint) -> *mut c_void {
    std::ptr::null_mut()
}

/// No exposed memory.
#[unsafe(no_mangle)]
pub extern "C" fn retro_get_memory_size(_id: c_uint) -> usize {
    0
}

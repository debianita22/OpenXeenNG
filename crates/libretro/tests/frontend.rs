// SPDX-License-Identifier: GPL-3.0-or-later
//! Drives the core through its C API the way a libretro frontend does.

use std::ffi::{CStr, CString, c_uint, c_void};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use openxeenng_libretro::*;

static ENV_CMDS: Mutex<Vec<c_uint>> = Mutex::new(Vec::new());
static FRAMES: AtomicUsize = AtomicUsize::new(0);
static AUDIO_FRAMES: AtomicUsize = AtomicUsize::new(0);
static LAST_FRAME: Mutex<Vec<u32>> = Mutex::new(Vec::new());
static PRESS_A: AtomicBool = AtomicBool::new(false);
static SHUTDOWN: AtomicBool = AtomicBool::new(false);

unsafe extern "C" fn env(cmd: c_uint, _data: *mut c_void) -> bool {
    ENV_CMDS.lock().unwrap().push(cmd);
    if cmd == 7 {
        SHUTDOWN.store(true, Ordering::SeqCst);
    }
    true
}

unsafe extern "C" fn video(data: *const c_void, w: c_uint, h: c_uint, pitch: usize) {
    assert_eq!((w, h, pitch), (320, 200, 1280));
    let px = unsafe { std::slice::from_raw_parts(data.cast::<u32>(), 320 * 200) };
    *LAST_FRAME.lock().unwrap() = px.to_vec();
    FRAMES.fetch_add(1, Ordering::SeqCst);
}

unsafe extern "C" fn audio_batch(_data: *const i16, frames: usize) -> usize {
    AUDIO_FRAMES.fetch_add(frames, Ordering::SeqCst);
    frames
}

unsafe extern "C" fn audio_sample(_l: i16, _r: i16) {}
unsafe extern "C" fn poll() {}

unsafe extern "C" fn state(port: c_uint, device: c_uint, _index: c_uint, id: c_uint) -> i16 {
    i16::from(port == 0 && device == 1 && id == 8 && PRESS_A.load(Ordering::SeqCst))
}

fn distinct_colours() -> usize {
    let f = LAST_FRAME.lock().unwrap();
    let mut v = f.clone();
    v.sort_unstable();
    v.dedup();
    v.len()
}

#[test]
fn core_runs_demo_and_content_like_a_frontend() {
    assert_eq!(retro_api_version(), 1);
    let mut info = std::mem::MaybeUninit::<RetroSystemInfo>::uninit();
    unsafe { retro_get_system_info(info.as_mut_ptr()) };
    let info = unsafe { info.assume_init() };
    assert_eq!(
        unsafe { CStr::from_ptr(info.library_name) }
            .to_str()
            .unwrap(),
        "OpenXeenNG"
    );
    assert!(info.need_fullpath);

    unsafe { retro_set_environment(Some(env)) };
    assert!(
        ENV_CMDS.lock().unwrap().contains(&18),
        "SET_SUPPORT_NO_GAME"
    );
    retro_set_video_refresh(Some(video));
    retro_set_audio_sample(Some(audio_sample));
    retro_set_audio_sample_batch(Some(audio_batch));
    retro_set_input_poll(Some(poll));
    retro_set_input_state(Some(state));
    retro_init();

    let mut av = std::mem::MaybeUninit::<RetroSystemAvInfo>::uninit();
    unsafe { retro_get_system_av_info(av.as_mut_ptr()) };
    let av = unsafe { av.assume_init() };
    assert_eq!(
        (av.geometry.base_width, av.geometry.base_height),
        (320, 200)
    );
    assert!((av.geometry.aspect_ratio - 4.0 / 3.0).abs() < 1e-6);
    assert_eq!(av.timing.sample_rate, 48_000.0);

    // 1. No content: demo.
    assert!(unsafe { retro_load_game(std::ptr::null()) });
    assert!(ENV_CMDS.lock().unwrap().contains(&10), "SET_PIXEL_FORMAT");
    for _ in 0..30 {
        unsafe { retro_run() };
    }
    assert_eq!(FRAMES.load(Ordering::SeqCst), 30);
    assert_eq!(AUDIO_FRAMES.load(Ordering::SeqCst), 30 * 800);
    assert!(distinct_colours() > 8);
    retro_unload_game();

    // 2. Synthetic game data loaded through a content path.
    let dir = std::env::temp_dir().join(format!("openxeenng-libretro-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for (name, bytes) in openxeenng_engine::synth::synthetic_game_files() {
        std::fs::write(dir.join(name.to_uppercase()), bytes).unwrap();
    }
    let path = CString::new(dir.join("XEEN.CC").to_str().unwrap()).unwrap();
    let game = RetroGameInfo {
        path: path.as_ptr(),
        data: std::ptr::null(),
        size: 0,
        meta: std::ptr::null(),
    };
    assert!(unsafe { retro_load_game(&game) });
    for _ in 0..5 {
        unsafe { retro_run() };
    }
    let list = LAST_FRAME.lock().unwrap().clone();
    // Press A (Enter) to open the first entry: the picture must change.
    PRESS_A.store(true, Ordering::SeqCst);
    unsafe { retro_run() };
    PRESS_A.store(false, Ordering::SeqCst);
    for _ in 0..3 {
        unsafe { retro_run() };
    }
    assert_ne!(
        *LAST_FRAME.lock().unwrap(),
        list,
        "RetroPad A should open the entry"
    );

    // Keyboard callback path: Escape twice leaves the viewer and quits.
    assert!(!SHUTDOWN.load(Ordering::SeqCst));
    retro_unload_game();
    retro_deinit();
}

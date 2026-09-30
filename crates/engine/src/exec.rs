// SPDX-License-Identifier: GPL-3.0-or-later
//! Frame-stepped execution of game logic written as `async` code.
//!
//! The original engine (and ScummVM's port) is full of blocking loops:
//! "draw, then wait until a key is pressed", "fade for 20 ticks", nested
//! menus that each run their own loop. Hosts such as libretro and the
//! browser forbid blocking: they call us once per video frame.
//!
//! Game flows are therefore `async fn`s. Awaiting [`Cx::next_frame`] (or a
//! helper built on it) suspends the flow until the host's next frame; the
//! [`Task`] is polled exactly once per [`crate::Engine::run_frame`]. This keeps
//! ported code shaped like the original while running on every host.
//!
//! Rule: every loop in a flow must await something frame-based, otherwise
//! it would stall the host.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};

use crate::audio::Mixer;
use crate::gfx::Surface;
use crate::input::{InputEvent, Key};
use crate::rng::ScummRandom;

/// Game ticks per second (the engine's frame counter runs every 50 ms).
pub const TICKS_PER_SECOND: u64 = 20;
const TICK_US: u64 = 1_000_000 / TICKS_PER_SECOND;

/// State shared between the host-facing [`crate::Engine`] and the flows.
pub struct Io {
    /// The 320x200 indexed screen.
    pub screen: Surface,
    /// Current palette in the engine's 8-bit levels (6-bit VGA << 2).
    pub palette: [[u8; 3]; 256],
    /// Pending input.
    pub events: VecDeque<InputEvent>,
    /// Frames run so far.
    pub frame: u64,
    /// Game ticks elapsed (20 per second of host time).
    pub ticks: u64,
    /// Host time in microseconds.
    pub time_us: u64,
    /// Audio mixer.
    pub mixer: Mixer,
    /// Random numbers.
    pub rng: ScummRandom,
    /// Set by flows to ask the host to exit.
    pub quit: bool,
    /// Short status line for window titles / OSD.
    pub status: String,
}

impl Io {
    /// Creates the shared state.
    pub fn new(audio_rate: u32, seed: u32) -> Self {
        Io {
            screen: Surface::screen(),
            palette: [[0; 3]; 256],
            events: VecDeque::new(),
            frame: 0,
            ticks: 0,
            time_us: 0,
            mixer: Mixer::new(audio_rate),
            rng: ScummRandom::new(seed),
            quit: false,
            status: String::new(),
        }
    }

    /// Advances host time and derives game ticks.
    pub fn advance(&mut self, dt_us: u64) {
        self.frame += 1;
        self.time_us += dt_us;
        self.ticks = self.time_us / TICK_US;
    }
}

/// Handle to the shared state, cloned into every flow.
#[derive(Clone)]
pub struct Cx {
    io: Rc<RefCell<Io>>,
}

impl Cx {
    /// Wraps shared state.
    pub fn new(io: Rc<RefCell<Io>>) -> Self {
        Cx { io }
    }

    /// Runs `f` with exclusive access to the shared state. Never await inside.
    pub fn with<R>(&self, f: impl FnOnce(&mut Io) -> R) -> R {
        f(&mut self.io.borrow_mut())
    }

    /// Suspends until the next host frame.
    pub fn next_frame(&self) -> NextFrame {
        NextFrame { yielded: false }
    }

    /// Suspends for `n` game ticks (50 ms each).
    pub async fn wait_ticks(&self, n: u64) {
        let target = self.with(|io| io.ticks) + n;
        while self.with(|io| io.ticks) < target {
            self.next_frame().await;
        }
    }

    /// Like [`Cx::wait_ticks`] but returns early (with `true`) on a key press.
    pub async fn wait_ticks_interruptible(&self, n: u64) -> bool {
        let target = self.with(|io| io.ticks) + n;
        loop {
            if self.with(|io| io.events.iter().any(|e| e.pressed_key().is_some())) {
                return true;
            }
            if self.with(|io| io.ticks) >= target {
                return false;
            }
            self.next_frame().await;
        }
    }

    /// Takes the next pending event, if any.
    pub fn poll_event(&self) -> Option<InputEvent> {
        self.with(|io| io.events.pop_front())
    }

    /// Waits for the next key press, discarding other events.
    pub async fn wait_key(&self) -> Key {
        loop {
            while let Some(ev) = self.poll_event() {
                if let Some(k) = ev.pressed_key() {
                    return k;
                }
            }
            self.next_frame().await;
        }
    }

    /// Drops all pending input (`EventsManager::clearEvents`).
    pub fn clear_events(&self) {
        self.with(|io| io.events.clear());
    }

    /// Current game tick count.
    pub fn ticks(&self) -> u64 {
        self.with(|io| io.ticks)
    }
}

/// Future returned by [`Cx::next_frame`].
#[derive(Debug)]
pub struct NextFrame {
    yielded: bool,
}

impl Future for NextFrame {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<()> {
        if self.yielded {
            Poll::Ready(())
        } else {
            self.yielded = true;
            Poll::Pending
        }
    }
}

/// A flow being executed, one poll per host frame.
pub struct Task {
    fut: Option<Pin<Box<dyn Future<Output = ()>>>>,
}

impl Task {
    /// Wraps a future.
    pub fn new(f: impl Future<Output = ()> + 'static) -> Self {
        Task {
            fut: Some(Box::pin(f)),
        }
    }

    /// Polls once. Returns `true` once the flow has finished.
    pub fn poll_once(&mut self) -> bool {
        let Some(fut) = self.fut.as_mut() else {
            return true;
        };
        let mut cx = Context::from_waker(Waker::noop());
        if fut.as_mut().poll(&mut cx).is_ready() {
            self.fut = None;
            true
        } else {
            false
        }
    }

    /// True once finished.
    pub fn is_finished(&self) -> bool {
        self.fut.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn io() -> Rc<RefCell<Io>> {
        Rc::new(RefCell::new(Io::new(48000, 1)))
    }

    #[test]
    fn next_frame_suspends_exactly_once() {
        let shared = io();
        let cx = Cx::new(shared.clone());
        let steps = Rc::new(RefCell::new(Vec::new()));
        let s2 = steps.clone();
        let mut t = Task::new(async move {
            for i in 0..3 {
                s2.borrow_mut().push(i);
                cx.next_frame().await;
            }
        });
        assert!(!t.poll_once());
        assert_eq!(*steps.borrow(), vec![0]);
        assert!(!t.poll_once());
        assert_eq!(*steps.borrow(), vec![0, 1]);
        assert!(!t.poll_once());
        assert!(t.poll_once());
        assert!(t.is_finished());
    }

    #[test]
    fn wait_ticks_follows_host_time() {
        let shared = io();
        let cx = Cx::new(shared.clone());
        let mut t = Task::new(async move { cx.wait_ticks(2).await });
        let mut frames = 0;
        while !t.poll_once() {
            shared.borrow_mut().advance(16_667);
            frames += 1;
            assert!(frames < 100);
        }
        // 2 ticks = 100 ms ≈ 6 frames at 60 Hz.
        assert!((6..=7).contains(&frames), "{frames}");
    }

    #[test]
    fn wait_key_returns_pressed_keys_only() {
        let shared = io();
        let cx = Cx::new(shared.clone());
        let got = Rc::new(RefCell::new(None));
        let g2 = got.clone();
        let mut t = Task::new(async move {
            *g2.borrow_mut() = Some(cx.wait_key().await);
        });
        assert!(!t.poll_once());
        shared
            .borrow_mut()
            .events
            .push_back(InputEvent::KeyUp(Key::Up));
        assert!(!t.poll_once());
        shared
            .borrow_mut()
            .events
            .push_back(InputEvent::KeyDown(Key::Enter));
        assert!(t.poll_once());
        assert_eq!(*got.borrow(), Some(Key::Enter));
    }
}

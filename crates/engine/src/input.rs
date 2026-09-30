// SPDX-License-Identifier: GPL-3.0-or-later
//! Frontend-neutral input events.
//!
//! Frontends translate keyboards, gamepads and pointers into these events.
//! The game itself is keyboard driven (as the original), so gamepad buttons
//! are mapped onto keys by the frontends; see [`GamepadButton::default_key`].

/// Logical keys the engine understands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    /// Arrow up / forward.
    Up,
    /// Arrow down / back.
    Down,
    /// Arrow left / turn left.
    Left,
    /// Arrow right / turn right.
    Right,
    /// Enter / confirm.
    Enter,
    /// Escape / cancel.
    Escape,
    /// Space.
    Space,
    /// Tab.
    Tab,
    /// Backspace.
    Backspace,
    /// Page up.
    PageUp,
    /// Page down.
    PageDown,
    /// Home.
    Home,
    /// End.
    End,
    /// Function keys F1..=F12.
    F(u8),
    /// A printable character (letters are lower case, digits, punctuation).
    Char(char),
}

/// Gamepad buttons (RetroPad / SDL game controller layout).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GamepadButton {
    /// D-pad up.
    Up,
    /// D-pad down.
    Down,
    /// D-pad left.
    Left,
    /// D-pad right.
    Right,
    /// Bottom face button (SNES B / Xbox A).
    South,
    /// Right face button (SNES A / Xbox B).
    East,
    /// Left face button (SNES Y / Xbox X).
    West,
    /// Top face button (SNES X / Xbox Y).
    North,
    /// Left shoulder.
    L,
    /// Right shoulder.
    R,
    /// Start.
    Start,
    /// Select / back.
    Select,
}

impl GamepadButton {
    /// Default key binding (shared by all frontends).
    pub fn default_key(self) -> Key {
        match self {
            GamepadButton::Up => Key::Up,
            GamepadButton::Down => Key::Down,
            GamepadButton::Left => Key::Left,
            GamepadButton::Right => Key::Right,
            GamepadButton::East => Key::Enter,
            GamepadButton::South => Key::Escape,
            GamepadButton::North => Key::Space,
            GamepadButton::West => Key::Tab,
            GamepadButton::L => Key::PageUp,
            GamepadButton::R => Key::PageDown,
            GamepadButton::Start => Key::Enter,
            GamepadButton::Select => Key::Tab,
        }
    }
}

/// An input event delivered to the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputEvent {
    /// A key went down (repeats are sent again as presses).
    KeyDown(Key),
    /// A key went up.
    KeyUp(Key),
    /// Pointer moved, in 320x200 screen coordinates.
    PointerMove {
        /// X in game pixels.
        x: i32,
        /// Y in game pixels.
        y: i32,
    },
    /// Pointer button pressed at a screen position (left = primary).
    PointerDown {
        /// X in game pixels.
        x: i32,
        /// Y in game pixels.
        y: i32,
        /// True for the secondary button.
        secondary: bool,
    },
    /// Pointer button released.
    PointerUp,
}

impl InputEvent {
    /// The key of a key-down event.
    pub fn pressed_key(&self) -> Option<Key> {
        match self {
            InputEvent::KeyDown(k) => Some(*k),
            _ => None,
        }
    }
}

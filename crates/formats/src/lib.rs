// SPDX-License-Identifier: GPL-3.0-or-later
//! Readers and writers for the data formats of *Might and Magic IV: Clouds of
//! Xeen*, *Might and Magic V: Darkside of Xeen* (together *World of Xeen*)
//! and *Swords of Xeen*.
//!
//! This crate contains no game data. It parses the files of a legally owned
//! copy (GOG ships them in the "Might and Magic 6-pack"), and can also write
//! every format, which the tests and the modding tools use.
//!
//! Much of the format knowledge comes from the ScummVM team's reimplementation
//! of the Xeen engine (GPL-3.0-or-later); modules that port its logic say so.
//! Every parser is bounds-checked and returns [`FormatError`] instead of
//! panicking on malformed input.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod cc;
pub mod error;
pub mod evt;
pub mod font;
pub mod gamedata;
pub mod io;
pub mod maze;
pub mod misc;
pub mod mob;
pub mod monster;
pub mod names;
pub mod palette;
pub mod sprite;
pub mod tables;
pub mod voc;

pub use error::{FormatError, Result};

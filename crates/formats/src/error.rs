// SPDX-License-Identifier: GPL-3.0-or-later
//! Error type shared by every parser in this crate.

use std::fmt;

/// Result alias used across the crate.
pub type Result<T> = std::result::Result<T, FormatError>;

/// A problem found while decoding (or encoding) original game data.
///
/// Parsers never panic on malformed input: every out-of-range read or
/// inconsistent field is reported through this type instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatError {
    /// The data ended before a structure was complete.
    UnexpectedEof {
        /// What was being read.
        what: &'static str,
        /// Byte offset of the failed read.
        offset: usize,
        /// Number of bytes that were needed.
        needed: usize,
    },
    /// A field holds a value the format does not allow.
    Invalid {
        /// What was being read.
        what: &'static str,
        /// Human readable detail.
        detail: String,
    },
    /// A requested resource does not exist.
    NotFound(String),
}

impl FormatError {
    /// Shorthand for [`FormatError::Invalid`].
    pub fn invalid(what: &'static str, detail: impl Into<String>) -> Self {
        FormatError::Invalid {
            what,
            detail: detail.into(),
        }
    }
}

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FormatError::UnexpectedEof {
                what,
                offset,
                needed,
            } => write!(
                f,
                "unexpected end of data while reading {what} at offset {offset} ({needed} byte(s) needed)"
            ),
            FormatError::Invalid { what, detail } => write!(f, "invalid {what}: {detail}"),
            FormatError::NotFound(name) => write!(f, "resource not found: {name}"),
        }
    }
}

impl std::error::Error for FormatError {}

// SPDX-License-Identifier: GPL-3.0-or-later
//! Map event scripts (`maze0xxx.evt`).
//!
//! A script is a flat list of records (ScummVM `MazeEvent::synchronize`,
//! engines/mm/xeen/scripts.cpp, GPL-3.0-or-later):
//!
//! ```text
//! u8 len (= 5 + parameter count)
//! u8 x, u8 y, u8 direction, u8 line, u8 opcode
//! (len - 5) parameter bytes
//! ```
//!
//! Records sharing a position and direction form one script, ordered by
//! `line`. Texts referenced by the scripts live in `aaze0xxx.txt`.

use crate::error::{FormatError, Result};
use crate::io::{Reader, Writer};

/// Script opcodes (names follow the ScummVM reimplementation).
pub const OPCODE_NAMES: [&str; 0x3D] = [
    "None",
    "Display0x01",
    "DoorTextSml",
    "DoorTextLrg",
    "SignText",
    "NPC",
    "PlayFX",
    "TeleportAndExit",
    "If1",
    "If2",
    "If3",
    "MoveObj",
    "TakeOrGive",
    "NoAction",
    "Remove",
    "SetChar",
    "Spawn",
    "DoTownEvent",
    "Exit",
    "AfterMap",
    "GiveMulti",
    "ConfirmWord",
    "Damage",
    "JumpRnd",
    "AfterEvent",
    "CallEvent",
    "Return",
    "SetVar",
    "TakeOrGive_2",
    "TakeOrGive_3",
    "CutsceneEndClouds",
    "TeleportAndContinue",
    "WhoWill",
    "RndDamage",
    "MoveWallObj",
    "AlterCellFlag",
    "AlterHed",
    "DisplayStat",
    "TakeOrGive_4",
    "SeatTextSml",
    "PlayEventVoc",
    "DisplayBottom",
    "IfMapFlag",
    "SelectRandomChar",
    "GiveEnchanted",
    "ItemType",
    "MakeNothingHere",
    "NoAction_2",
    "ChooseNumeric",
    "DisplayBottomTwoLines",
    "DisplayLarge",
    "ExchObj",
    "FallToMap",
    "DisplayMain",
    "Goto",
    "ConfirmWord_2",
    "GotoRandom",
    "CutsceneEndDarkside",
    "CutsceneEdWorld",
    "FlipWorld",
    "PlayCD",
];

/// Name of an opcode, or `None` for values the engine does not define.
pub fn opcode_name(op: u8) -> Option<&'static str> {
    OPCODE_NAMES.get(usize::from(op)).copied()
}

/// One script instruction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MazeEvent {
    /// Cell x.
    pub x: u8,
    /// Cell y.
    pub y: u8,
    /// Facing required to trigger (4 = any, as used by the engine).
    pub direction: u8,
    /// Line number inside the script.
    pub line: u8,
    /// Opcode (see [`OPCODE_NAMES`]).
    pub opcode: u8,
    /// Raw parameters.
    pub params: Vec<u8>,
}

impl MazeEvent {
    /// Record length in the file (length byte included).
    pub fn encoded_len(&self) -> usize {
        6 + self.params.len()
    }
}

/// Parses an event file.
pub fn parse_events(data: &[u8]) -> Result<Vec<MazeEvent>> {
    let mut r = Reader::new(data, "event script");
    let mut out = Vec::new();
    while !r.at_end() {
        let len = r.u8()?;
        if len < 5 {
            return Err(FormatError::invalid(
                "event record",
                format!("length {len} at offset {} is shorter than the header", r.pos() - 1),
            ));
        }
        let x = r.u8()?;
        let y = r.u8()?;
        let direction = r.u8()?;
        let line = r.u8()?;
        let opcode = r.u8()?;
        let params = r.bytes(usize::from(len) - 5)?.to_vec();
        out.push(MazeEvent {
            x,
            y,
            direction,
            line,
            opcode,
            params,
        });
    }
    Ok(out)
}

/// Parses an event file the way the engine tolerates damaged data: records
/// are read until the first one that does not fit, which is dropped. Returns
/// the events and, if the data was damaged, the error describing where.
///
/// The original engine (and ScummVM) keep reading past the end of the buffer
/// in that case; the extra record they build is never reachable in play.
pub fn parse_events_lenient(data: &[u8]) -> (Vec<MazeEvent>, Option<FormatError>) {
    let mut events = Vec::new();
    let mut pos = 0usize;
    while pos < data.len() {
        let len = usize::from(data[pos]);
        let end = pos + 1 + len;
        if len < 5 || end > data.len() {
            let err = FormatError::invalid(
                "event record",
                format!(
                    "record at offset {pos} declares {len} bytes, {} available",
                    data.len() - pos - 1
                ),
            );
            return (events, Some(err));
        }
        match parse_events(&data[pos..end]) {
            Ok(mut one) => events.append(&mut one),
            Err(e) => return (events, Some(e)),
        }
        pos = end;
    }
    (events, None)
}

/// Serialises events back to the file format.
pub fn events_to_bytes(events: &[MazeEvent]) -> Result<Vec<u8>> {
    let mut w = Writer::new();
    for e in events {
        let len = 5 + e.params.len();
        if len > 255 {
            return Err(FormatError::invalid("event record", "more than 250 parameters"));
        }
        w.u8(len as u8)
            .u8(e.x)
            .u8(e.y)
            .u8(e.direction)
            .u8(e.line)
            .u8(e.opcode)
            .bytes(&e.params);
    }
    Ok(w.into_inner())
}

/// Renders a readable listing grouped by script (position + direction).
///
/// Text references are resolved when `texts` is given and the first
/// parameter of a text-display opcode is a valid index.
pub fn disassemble(events: &[MazeEvent], texts: Option<&[String]>) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let mut last_key = None;
    for e in events {
        let key = (e.x, e.y, e.direction);
        if last_key != Some(key) {
            let dir = match e.direction {
                0 => "N",
                1 => "E",
                2 => "S",
                3 => "W",
                4 => "any",
                _ => "?",
            };
            let _ = writeln!(out, "\n@ ({:2},{:2}) facing {dir}", e.x, e.y);
            last_key = Some(key);
        }
        let name = opcode_name(e.opcode).unwrap_or("<unknown>");
        let params: Vec<String> = e.params.iter().map(|p| format!("{p:02X}")).collect();
        let _ = write!(out, "  {:3}: {:<22} {}", e.line, name, params.join(" "));
        let is_text_op = matches!(
            e.opcode,
            0x01..=0x05 | 0x27 | 0x29 | 0x31 | 0x32 | 0x35
        );
        if let (true, Some(texts), Some(&idx)) = (is_text_op, texts, e.params.first()) {
            if let Some(t) = texts.get(usize::from(idx)) {
                let _ = write!(out, "    ; \"{}\"", t.replace('\n', "\\n"));
            }
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// First three records of ScummVM's own `mazex255.evt` (devtools/create_mm).
    const ISLE: [u8; 23] = [
        0x06, 0x07, 0x00, 0x02, 0x00, 0x28, 0x01, // PlayEventVoc 01
        0x09, 0x07, 0x00, 0x02, 0x01, 0x15, 0x00, 0x03, 0x00, 0x00, // ConfirmWord
        0x05, 0x07, 0x00, 0x02, 0x02, 0x12, // Exit
    ];

    #[test]
    fn parses_isle_records() {
        let ev = parse_events(&ISLE).unwrap();
        assert_eq!(ev.len(), 3);
        assert_eq!(ev[0].opcode, 0x28);
        assert_eq!(opcode_name(ev[0].opcode), Some("PlayEventVoc"));
        assert_eq!(ev[0].params, vec![1]);
        assert_eq!(opcode_name(ev[1].opcode), Some("ConfirmWord"));
        assert_eq!(ev[1].params, vec![0, 3, 0, 0]);
        assert_eq!(opcode_name(ev[2].opcode), Some("Exit"));
        assert!(ev[2].params.is_empty());
        assert_eq!(events_to_bytes(&ev).unwrap(), ISLE.to_vec());
    }

    #[test]
    fn disassembly_mentions_opcodes_and_texts() {
        let mut ev = parse_events(&ISLE).unwrap();
        ev.push(MazeEvent {
            x: 7,
            y: 0,
            direction: 2,
            line: 2,
            opcode: 0x04,
            params: vec![1],
        });
        let texts = vec!["Where to?".to_string(), "Isle of ScummVM".to_string()];
        let s = disassemble(&ev, Some(&texts));
        assert!(s.contains("PlayEventVoc"));
        assert!(s.contains("\"Isle of ScummVM\""));
    }

    #[test]
    fn lenient_parse_stops_at_damaged_tail() {
        let mut bytes = ISLE.to_vec();
        bytes.extend([0x08, 0x00, 0x02]); // truncated record
        assert!(parse_events(&bytes).is_err());
        let (ev, err) = parse_events_lenient(&bytes);
        assert_eq!(ev.len(), 3);
        assert!(err.is_some());
        let (ev, err) = parse_events_lenient(&ISLE);
        assert_eq!(ev.len(), 3);
        assert!(err.is_none());
    }

    #[test]
    fn rejects_short_records() {
        assert!(parse_events(&[3, 0, 0, 0]).is_err());
        assert!(parse_events(&[9, 0, 0, 0, 0, 1]).is_err());
    }
}

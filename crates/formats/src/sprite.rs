// SPDX-License-Identifier: GPL-3.0-or-later
//! Xeen sprite resources (`*.icn`, `*.mon`, `*.att`, `*.obj`, `*.srf`,
//! `*.fwl`, `*.swl`, `*.wal`, `*.sky`, `*.gnd`, `*.til`, `*.fac`, ...).
//!
//! Format, as reverse-engineered by the ScummVM team (`SpriteResource`,
//! engines/mm/shared/xeen/sprites.cpp, GPL-3.0-or-later):
//!
//! ```text
//! u16 frame count F
//! F * (u16 cell1 offset, u16 cell2 offset)   cell2 == 0: single cell
//! cell:  u16 x_offset, u16 width, u16 y_offset, u16 height, then rows:
//!   u8 line_len == 0 -> u8 n: skip n + 1 rows
//!   u8 line_len  > 0 -> u8 x (first pixel column), then opcodes until
//!                       line_len bytes (counted from x) are consumed
//! opcode = ccclllll:
//!   c=0,1  copy the next opcode+1 bytes as pixels (1..=64)
//!   c=2    next byte repeated l+3 times
//!   c=3    u16 back offset d: copy l+4 raw bytes from (position - d)
//!   c=4    next two bytes repeated as a pair l+2 times
//!   c=5    skip l+1 transparent pixels
//!   c=6,7  pattern: byte v repeated (opcode & 7) + 3 times, adding
//!          PATTERN_STEPS[((opcode >> 2) & 0xE) + i % 2] after each pixel
//! ```
//!
//! Pixels are palette indexes; untouched positions are transparent.

use crate::error::{FormatError, Result};
use crate::io::{Reader, Writer};

/// Size of the scratch line buffer used while decoding a row.
pub const LINE_BUF: usize = 320 * 3;
/// Index in the line buffer that maps to the cell's first column.
pub const LINE_ORIGIN: usize = 320;
/// Marker for a transparent entry in the line buffer.
pub const TRANSPARENT: i32 = -1;

const PATTERN_STEPS: [i32; 16] = [0, 1, 1, 1, 2, 2, 3, 3, 0, -1, -1, -1, -2, -2, -3, -3];

/// Offsets of the (up to) two cells making up a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameCells {
    /// Offset of the first cell.
    pub cell1: u16,
    /// Offset of the optional second cell drawn over the first (0 = none).
    pub cell2: u16,
}

/// Header of one cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellHeader {
    /// Horizontal offset of the cell inside the frame.
    pub x_offset: u16,
    /// Width in pixels.
    pub width: u16,
    /// Vertical offset of the cell inside the frame.
    pub y_offset: u16,
    /// Height in rows.
    pub height: u16,
}

/// A row record inside a cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowRecord {
    /// Skip this many rows (`n + 1` from the file).
    Skip(u16),
    /// A drawn row: `start` is the offset of the x byte, `len` the byte count
    /// from there (the next record starts at `start + len`).
    Line {
        /// Offset of the row's first-column byte.
        start: usize,
        /// Number of bytes belonging to the row.
        len: u8,
    },
}

/// A parsed sprite resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpriteFile {
    data: Vec<u8>,
    frames: Vec<FrameCells>,
}

/// A decoded frame: palette indexes, `None` where transparent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedImage {
    /// Width in pixels.
    pub width: usize,
    /// Height in pixels.
    pub height: usize,
    /// Row-major pixels.
    pub pixels: Vec<Option<u8>>,
}

impl IndexedImage {
    /// Creates a fully transparent image.
    pub fn new(width: usize, height: usize) -> Self {
        IndexedImage {
            width,
            height,
            pixels: vec![None; width * height],
        }
    }

    /// Reads a pixel (`None` outside the image or when transparent).
    pub fn get(&self, x: usize, y: usize) -> Option<u8> {
        if x < self.width && y < self.height {
            self.pixels[y * self.width + x]
        } else {
            None
        }
    }

    /// Writes a pixel; out-of-range writes are ignored.
    pub fn set(&mut self, x: usize, y: usize, v: Option<u8>) {
        if x < self.width && y < self.height {
            self.pixels[y * self.width + x] = v;
        }
    }
}

impl SpriteFile {
    /// Parses a sprite resource and validates every frame's cell headers.
    pub fn parse(data: Vec<u8>) -> Result<Self> {
        let mut r = Reader::new(&data, "sprite index");
        let count = r.u16()? as usize;
        let mut frames = Vec::with_capacity(count);
        for _ in 0..count {
            let cell1 = r.u16()?;
            let cell2 = r.u16()?;
            frames.push(FrameCells { cell1, cell2 });
        }
        let sprite = SpriteFile { data, frames };
        for f in &sprite.frames {
            sprite.cell_header(f.cell1)?;
            if f.cell2 != 0 {
                sprite.cell_header(f.cell2)?;
            }
        }
        Ok(sprite)
    }

    /// Raw resource bytes.
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Frame table.
    pub fn frames(&self) -> &[FrameCells] {
        &self.frames
    }

    /// Number of frames.
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    /// Reads the header of the cell at `offset`.
    pub fn cell_header(&self, offset: u16) -> Result<CellHeader> {
        let mut r = Reader::new(&self.data, "sprite cell header");
        r.seek(offset as usize)?;
        Ok(CellHeader {
            x_offset: r.u16()?,
            width: r.u16()?,
            y_offset: r.u16()?,
            height: r.u16()?,
        })
    }

    /// Size of a frame: the union of its cells' extents
    /// (`SpriteResource::getFrameSize`).
    pub fn frame_size(&self, frame: usize) -> Result<(usize, usize)> {
        let f = self.frame(frame)?;
        let mut w = 0usize;
        let mut h = 0usize;
        let cells = if f.cell2 != 0 { 2 } else { 1 };
        for &off in [f.cell1, f.cell2].iter().take(cells) {
            let c = self.cell_header(off)?;
            w = w.max(c.x_offset as usize + c.width as usize);
            h = h.max(c.y_offset as usize + c.height as usize);
        }
        Ok((w, h))
    }

    /// The cells of a frame.
    pub fn frame(&self, frame: usize) -> Result<FrameCells> {
        self.frames.get(frame).copied().ok_or_else(|| {
            FormatError::invalid(
                "sprite frame",
                format!("frame {frame} of {}", self.frames.len()),
            )
        })
    }

    /// Iterates over the row records of a cell.
    pub fn rows(&self, cell_offset: u16) -> Result<CellRows<'_>> {
        let header = self.cell_header(cell_offset)?;
        Ok(CellRows {
            data: &self.data,
            pos: cell_offset as usize + 8,
            rows_left: i32::from(header.height),
        })
    }

    /// Decodes a frame without scaling or flipping.
    pub fn decode_frame(&self, frame: usize) -> Result<IndexedImage> {
        let (w, h) = self.frame_size(frame)?;
        let mut img = IndexedImage::new(w, h);
        let f = self.frame(frame)?;
        self.decode_cell_into(f.cell1, &mut img)?;
        if f.cell2 != 0 {
            self.decode_cell_into(f.cell2, &mut img)?;
        }
        Ok(img)
    }

    fn decode_cell_into(&self, cell_offset: u16, img: &mut IndexedImage) -> Result<()> {
        let header = self.cell_header(cell_offset)?;
        let mut y = header.y_offset as usize;
        let mut line = [TRANSPARENT; LINE_BUF];
        for rec in self.rows(cell_offset)? {
            match rec? {
                RowRecord::Skip(n) => y += n as usize,
                RowRecord::Line { start, len } => {
                    line.fill(TRANSPARENT);
                    decode_line(&self.data, start, len, header.width, false, &mut line)?;
                    for x in 0..header.width as usize {
                        let v = line[LINE_ORIGIN + x];
                        if v != TRANSPARENT {
                            img.set(header.x_offset as usize + x, y, Some(v as u8));
                        }
                    }
                    y += 1;
                }
            }
        }
        Ok(())
    }
}

/// Iterator over the row records of a cell (see [`SpriteFile::rows`]).
#[derive(Debug, Clone)]
pub struct CellRows<'a> {
    data: &'a [u8],
    pos: usize,
    rows_left: i32,
}

impl<'a> CellRows<'a> {
    /// Offset of the next record.
    pub fn position(&self) -> usize {
        self.pos
    }
}

impl Iterator for CellRows<'_> {
    type Item = Result<RowRecord>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.rows_left <= 0 {
            return None;
        }
        let mut r = Reader::new(self.data, "sprite row");
        if let Err(e) = r.seek(self.pos) {
            self.rows_left = 0;
            return Some(Err(e));
        }
        let len = match r.u8() {
            Ok(v) => v,
            Err(e) => {
                self.rows_left = 0;
                return Some(Err(e));
            }
        };
        if len == 0 {
            let n = match r.u8() {
                Ok(v) => v,
                Err(e) => {
                    self.rows_left = 0;
                    return Some(Err(e));
                }
            };
            self.pos = r.pos();
            // The original loop does `yCtr -= n` then `--yCtr`.
            self.rows_left -= i32::from(n) + 1;
            Some(Ok(RowRecord::Skip(u16::from(n) + 1)))
        } else {
            let start = r.pos();
            if start + len as usize > self.data.len() {
                self.rows_left = 0;
                return Some(Err(FormatError::UnexpectedEof {
                    what: "sprite row",
                    offset: start,
                    needed: len as usize,
                }));
            }
            self.pos = start + len as usize;
            self.rows_left -= 1;
            Some(Ok(RowRecord::Line { start, len }))
        }
    }
}

/// Decodes one row into `buf` (which the caller pre-fills with
/// [`TRANSPARENT`]). Column `x` of the cell lands at `buf[LINE_ORIGIN + x]`.
///
/// `start`/`len` come from [`RowRecord::Line`]. When `flipped` is set the row
/// is written right-to-left from column `width - 1 - first_x`, exactly like the
/// original renderer. Values are kept as `i32` because pattern runs may step
/// outside `0..=255` (they are truncated to a byte when drawn, except `-1`,
/// which stays transparent — a quirk of the original code).
pub fn decode_line(
    data: &[u8],
    start: usize,
    len: u8,
    width: u16,
    flipped: bool,
    buf: &mut [i32; LINE_BUF],
) -> Result<()> {
    let mut r = Reader::new(data, "sprite line");
    r.seek(start)?;
    let first_x = i32::from(r.u8()?);
    let x_inc: i32 = if flipped { -1 } else { 1 };
    let mut p: i32 = if flipped {
        LINE_ORIGIN as i32 + i32::from(width) - 1 - first_x
    } else {
        LINE_ORIGIN as i32 + first_x
    };
    let mut put = |p: &mut i32, v: i32| {
        if (0..LINE_BUF as i32).contains(p) {
            buf[*p as usize] = v;
        }
        *p += x_inc;
    };

    let len = len as usize;
    let mut byte_count = 1usize;
    while byte_count < len {
        let opcode = r.u8()?;
        byte_count += 1;
        let l = i32::from(opcode & 0x1F);
        match opcode >> 5 {
            0 | 1 => {
                let n = usize::from(opcode) + 1;
                for &b in r.bytes(n)? {
                    put(&mut p, i32::from(b));
                }
                byte_count += n;
            }
            2 => {
                let v = i32::from(r.u8()?);
                byte_count += 1;
                for _ in 0..l + 3 {
                    put(&mut p, v);
                }
            }
            3 => {
                let back = r.u16()? as usize;
                byte_count += 2;
                let src = r.pos().checked_sub(back).ok_or_else(|| {
                    FormatError::invalid("sprite line", "stream copy before start of data")
                })?;
                let n = (l + 4) as usize;
                let chunk = data.get(src..src + n).ok_or(FormatError::UnexpectedEof {
                    what: "sprite stream copy",
                    offset: src,
                    needed: n,
                })?;
                for &b in chunk {
                    put(&mut p, i32::from(b));
                }
            }
            4 => {
                let a = i32::from(r.u8()?);
                let b = i32::from(r.u8()?);
                byte_count += 2;
                for _ in 0..l + 2 {
                    put(&mut p, a);
                    put(&mut p, b);
                }
            }
            5 => {
                p += (l + 1) * x_inc;
            }
            _ => {
                let n = i32::from(opcode & 0x07) + 3;
                let base = usize::from((opcode >> 2) & 0x0E);
                let mut v = i32::from(r.u8()?);
                byte_count += 1;
                for i in 0..n {
                    put(&mut p, v);
                    v += PATTERN_STEPS[base + (i as usize % 2)];
                }
            }
        }
    }
    if byte_count != len {
        return Err(FormatError::invalid(
            "sprite line",
            format!("opcodes consumed {byte_count} bytes, row declares {len}"),
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Encoder
// ---------------------------------------------------------------------------

/// Builds sprite resources from indexed images.
///
/// The encoder emits only literal, run and skip opcodes, which every version
/// of the engine decodes identically. Each row must fit in 255 bytes; very
/// wide, noisy rows can be split across the frame's second cell by the caller.
#[derive(Debug, Default, Clone)]
pub struct SpriteBuilder {
    frames: Vec<(IndexedImage, Option<IndexedImage>)>,
}

impl SpriteBuilder {
    /// Creates an empty builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a single-cell frame.
    pub fn frame(&mut self, img: IndexedImage) -> &mut Self {
        self.frames.push((img, None));
        self
    }

    /// Adds a frame made of a background cell and an overlay cell.
    pub fn frame2(&mut self, background: IndexedImage, overlay: IndexedImage) -> &mut Self {
        self.frames.push((background, Some(overlay)));
        self
    }

    /// Serialises the sprite resource.
    pub fn build(&self) -> Result<Vec<u8>> {
        let mut w = Writer::new();
        w.u16(self.frames.len() as u16);
        let table = w.len();
        for _ in &self.frames {
            w.u16(0).u16(0);
        }
        for (i, (a, b)) in self.frames.iter().enumerate() {
            let off1 = cell_offset(&w)?;
            encode_cell(&mut w, a)?;
            w.patch_u16(table + i * 4, off1);
            if let Some(b) = b {
                let off2 = cell_offset(&w)?;
                encode_cell(&mut w, b)?;
                w.patch_u16(table + i * 4 + 2, off2);
            }
        }
        if w.len() > u16::MAX as usize {
            return Err(FormatError::invalid("sprite", "resource exceeds 64 KiB"));
        }
        Ok(w.into_inner())
    }
}

fn cell_offset(w: &Writer) -> Result<u16> {
    u16::try_from(w.len()).map_err(|_| FormatError::invalid("sprite", "resource exceeds 64 KiB"))
}

fn encode_cell(w: &mut Writer, img: &IndexedImage) -> Result<()> {
    if img.width > u16::MAX as usize || img.height > u16::MAX as usize {
        return Err(FormatError::invalid("sprite cell", "image too large"));
    }
    // Trim fully transparent columns on the left and rows on the top; the
    // right/bottom margins are kept through width/height so the frame size
    // is preserved.
    let opaque_col = |x: usize| (0..img.height).any(|y| img.get(x, y).is_some());
    let opaque_row = |y: usize| (0..img.width).any(|x| img.get(x, y).is_some());
    let x0 = (0..img.width).find(|&x| opaque_col(x)).unwrap_or(0);
    let y0 = (0..img.height).find(|&y| opaque_row(y)).unwrap_or(0);
    let width = img.width - x0;
    let height = img.height - y0;
    w.u16(x0 as u16).u16(width as u16).u16(y0 as u16).u16(height as u16);

    let mut y = y0;
    while y < img.height {
        if !opaque_row(y) {
            let mut n = 0usize;
            while y + n < img.height && n < 256 && !opaque_row(y + n) {
                n += 1;
            }
            w.u8(0).u8((n - 1) as u8);
            y += n;
            continue;
        }
        let row: Vec<Option<u8>> = (x0..img.width).map(|x| img.get(x, y)).collect();
        let bytes = encode_row(&row)?;
        w.u8(bytes.len() as u8).bytes(&bytes);
        y += 1;
    }
    Ok(())
}

/// Encodes one non-empty row: returns the bytes after the length byte.
fn encode_row(row: &[Option<u8>]) -> Result<Vec<u8>> {
    let first = row.iter().position(Option::is_some).unwrap_or(0);
    let last = row.iter().rposition(Option::is_some).unwrap_or(0);
    let mut out = Vec::new();
    let start_x = first.min(255);
    out.push(start_x as u8);
    let mut x = start_x;
    while x <= last {
        match row[x] {
            None => {
                let mut n = 0;
                while x + n <= last && row[x + n].is_none() && n < 32 {
                    n += 1;
                }
                out.push(0xA0 | (n as u8 - 1));
                x += n;
            }
            Some(v) => {
                let mut run = 1;
                while x + run <= last && row[x + run] == Some(v) && run < 34 {
                    run += 1;
                }
                if run >= 3 {
                    out.push(0x40 | (run as u8 - 3));
                    out.push(v);
                    x += run;
                } else {
                    // Literal span up to the next long run or gap.
                    let mut n = 0;
                    while x + n <= last && n < 64 {
                        let Some(c) = row[x + n] else { break };
                        let mut r = 1;
                        while x + n + r <= last && row[x + n + r] == Some(c) && r < 3 {
                            r += 1;
                        }
                        if r >= 3 {
                            break;
                        }
                        n += 1;
                    }
                    let n = n.max(1);
                    out.push((n - 1) as u8);
                    for k in 0..n {
                        out.push(row[x + k].unwrap_or(0));
                    }
                    x += n;
                }
            }
        }
    }
    if out.len() > 255 {
        return Err(FormatError::invalid(
            "sprite row",
            format!("row needs {} bytes; the format allows 255", out.len()),
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checker(w: usize, h: usize) -> IndexedImage {
        let mut img = IndexedImage::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let v = match (x + y) % 7 {
                    0 => None,
                    1 | 2 => Some(5),
                    _ => Some(((x * 3 + y) % 250) as u8),
                };
                img.set(x, y, v);
            }
        }
        img
    }

    #[test]
    fn encode_decode_round_trip() {
        let mut img = checker(40, 30);
        // Transparent margins, empty rows, long runs.
        for x in 0..40 {
            img.set(x, 0, None);
            img.set(x, 12, None);
            img.set(x, 13, None);
            img.set(x, 20, Some(9));
        }
        for y in 0..30 {
            img.set(0, y, None);
            img.set(1, y, None);
        }
        let mut b = SpriteBuilder::new();
        b.frame(img.clone()).frame(checker(8, 3));
        let bytes = b.build().unwrap();
        let s = SpriteFile::parse(bytes).unwrap();
        assert_eq!(s.frame_count(), 2);
        assert_eq!(s.frame_size(0).unwrap(), (40, 30));
        assert_eq!(s.decode_frame(0).unwrap(), img);
        assert_eq!(s.decode_frame(1).unwrap(), checker(8, 3));
    }

    #[test]
    fn two_cell_frames_overlay() {
        let mut bg = IndexedImage::new(4, 4);
        let mut fg = IndexedImage::new(4, 4);
        for y in 0..4 {
            for x in 0..4 {
                bg.set(x, y, Some(1));
            }
        }
        fg.set(2, 2, Some(7));
        let mut b = SpriteBuilder::new();
        b.frame2(bg, fg);
        let s = SpriteFile::parse(b.build().unwrap()).unwrap();
        let img = s.decode_frame(0).unwrap();
        assert_eq!(img.get(2, 2), Some(7));
        assert_eq!(img.get(0, 0), Some(1));
    }

    /// Hand-assembled cell exercising every opcode family.
    #[test]
    fn decode_all_opcodes() {
        let mut d = Writer::new();
        d.u16(1).u16(6).u16(0); // one frame, cell at 6
        d.u16(0).u16(24).u16(0).u16(2); // header: w=24, h=2
        // Row 0: x=1, literal 2 [10,11], run 3x 20, pair 2x (30,31),
        // skip 2, pattern (0xC0 | (1 << 2)? ) and stream copy.
        let row: Vec<u8> = vec![
            1, // first x
            0x01, 10, 11, // literal 2
            0x40, 20, // run of 3
            0x80, 30, 31, // pair x2
            0xA1, // skip 2
            0xC4, 50, // pattern: base=(0xC4>>2)&0xE = 0x31&0xE = 0 -> steps 0,1 ; n=3 (opcode&7=4? no)
        ];
        // 0xC4: n = (0xC4 & 7) + 3 = 4 + 3 = 7; base = (0xC4 >> 2) & 0xE = 0x31 & 0xE = 0
        d.u8(row.len() as u8).bytes(&row);
        // Row 1: stream copy of 4 bytes starting at "10, 11, 0x40, 20".
        let copy_src = 6 + 8 + 1 + 1 + 1; // offset of the literal bytes 10,11
        let here = d.len() + 1 + 1 + 1 + 2; // pos after reading the u16
        let back = (here - copy_src) as u16;
        let row1 = [0u8, 0x60, (back & 0xFF) as u8, (back >> 8) as u8];
        d.u8(row1.len() as u8).bytes(&row1);
        let s = SpriteFile::parse(d.into_inner()).unwrap();
        let img = s.decode_frame(0).unwrap();
        let r0: Vec<Option<u8>> = (0..24).map(|x| img.get(x, 0)).collect();
        let mut expect = vec![None; 24];
        let vals = [10u8, 11, 20, 20, 20, 30, 31, 30, 31];
        for (i, v) in vals.iter().enumerate() {
            expect[1 + i] = Some(*v);
        }
        // skip 2 -> columns 10, 11 transparent; pattern starts at 12
        let pattern = [50u8, 50, 51, 51, 52, 52, 53];
        for (i, v) in pattern.iter().enumerate() {
            expect[12 + i] = Some(*v);
        }
        assert_eq!(r0, expect);
        let r1: Vec<Option<u8>> = (0..4).map(|x| img.get(x, 1)).collect();
        assert_eq!(r1, vec![Some(10), Some(11), Some(0x40), Some(20)]);
    }

    #[test]
    fn flipped_line_writes_right_to_left() {
        let mut d = Writer::new();
        let row = [0u8, 0x02, 1, 2, 3];
        d.bytes(&row);
        let mut buf = [TRANSPARENT; LINE_BUF];
        decode_line(d.as_slice(), 0, row.len() as u8, 5, true, &mut buf).unwrap();
        let got: Vec<i32> = (0..5).map(|x| buf[LINE_ORIGIN + x]).collect();
        assert_eq!(got, vec![TRANSPARENT, TRANSPARENT, 3, 2, 1]);
    }

    #[test]
    fn malformed_data_is_an_error_not_a_panic() {
        assert!(SpriteFile::parse(vec![1, 0, 200, 0, 0, 0]).is_err());
        let mut d = Writer::new();
        d.u16(1).u16(6).u16(0).u16(0).u16(4).u16(0).u16(1);
        d.u8(3).u8(0).u8(0x1F).u8(1); // literal of 32 bytes but only 1 present
        let s = SpriteFile::parse(d.into_inner()).unwrap();
        assert!(s.decode_frame(0).is_err());
    }
}

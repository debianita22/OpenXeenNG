// SPDX-License-Identifier: GPL-3.0-or-later
//! 8-bit indexed surfaces and the Xeen sprite renderer.
//!
//! `draw_sprite` is a port of ScummVM's `SpriteResource::draw` / `drawSprite`
//! (engines/mm/shared/xeen/sprites.cpp, GPL-3.0-or-later), including the
//! scaling masks and the special-effect "drawers". Deliberate differences:
//! * memory safety: writes outside the surface are dropped (the C++ code
//!   relies on well-formed data and callers);
//! * enlarged pixels use the destination width as the row stride (ScummVM
//!   hard-codes 320, which only matters for non-screen surfaces);
//! * drawer 5 recomputes its threshold on every call (ScummVM caches the first
//!   value in a function-level `static`, an accident of the port).

use openxeenng_formats::sprite::{
    LINE_BUF, LINE_ORIGIN, MAX_FRAME_DIM, RowRecord, SpriteFile, TRANSPARENT, decode_line,
};

use crate::rng::RandomSource;

/// Screen width of the original game.
pub const SCREEN_WIDTH: usize = 320;
/// Screen height of the original game.
pub const SCREEN_HEIGHT: usize = 200;

/// Sprite drawing flags (`SpriteFlags`).
pub mod flags {
    /// Mask selecting the drawer.
    pub const MODE_MASK: u32 = 0xF00;
    /// Colour remap drawer.
    pub const DRAWER1: u32 = 0x100;
    /// Scramble drawer (pixels jitter randomly).
    pub const DRAWER2: u32 = 0x200;
    /// Ghost drawer (translucent).
    pub const DRAWER3: u32 = 0x300;
    /// Threshold drawer.
    pub const DRAWER4: u32 = 0x400;
    /// Fuzz drawer (random pixels skipped).
    pub const DRAWER5: u32 = 0x500;
    /// XOR drawer.
    pub const DRAWER6: u32 = 0x600;
    /// Clip to the 3D view (8,8)-(223,141).
    pub const SCENE_CLIPPED: u32 = 0x2000;
    /// Clip at the configured bottom line.
    pub const BOTTOM_CLIPPED: u32 = 0x4000;
    /// Mirror horizontally.
    pub const HORIZ_FLIPPED: u32 = 0x8000;
    /// Grow the destination surface to fit the sprite.
    pub const RESIZE: u32 = 0x10000;
    /// Low bits select the drawer's parameter set.
    pub const PARAM_MASK: u32 = 0x1F;
}

/// Scale value: indexes 1..=15 shrink (higher = smaller).
pub const SCALE_MASK: u16 = 0x7FFF;
/// Scale flag: draw every pixel as 2x2.
pub const SCALE_ENLARGE: u16 = 0x8000;

const SCALE_TABLE: [u16; 16] = [
    0xFFFF, 0xFFEF, 0xEFEF, 0xEFEE, 0xEEEE, 0xEEAE, 0xAEAE, 0xAEAA, 0xAAAA, 0xAA8A, 0x8A8A, 0x8A88,
    0x8888, 0x8880, 0x8080, 0x8000,
];

const SCENE_CLIP_LEFT: i32 = 8;
const SCENE_CLIP_RIGHT: i32 = 223;

/// Integer rectangle with exclusive right/bottom edges (like `Common::Rect`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    /// Left edge.
    pub left: i32,
    /// Top edge.
    pub top: i32,
    /// Right edge (exclusive).
    pub right: i32,
    /// Bottom edge (exclusive).
    pub bottom: i32,
}

impl Rect {
    /// Creates a rectangle.
    pub const fn new(left: i32, top: i32, right: i32, bottom: i32) -> Self {
        Rect {
            left,
            top,
            right,
            bottom,
        }
    }

    /// Rectangle from a size.
    pub const fn sized(w: usize, h: usize) -> Self {
        Rect::new(0, 0, w as i32, h as i32)
    }

    /// Clips in place with the exact semantics of `Common::Rect::clip`.
    pub fn clip(&mut self, r: Rect) {
        if self.top < r.top {
            self.top = r.top;
        } else if self.top > r.bottom {
            self.top = r.bottom;
        }
        if self.left < r.left {
            self.left = r.left;
        } else if self.left > r.right {
            self.left = r.right;
        }
        if self.bottom > r.bottom {
            self.bottom = r.bottom;
        } else if self.bottom < r.top {
            self.bottom = r.top;
        }
        if self.right > r.right {
            self.right = r.right;
        } else if self.right < r.left {
            self.right = r.left;
        }
    }

    /// Width.
    pub fn width(&self) -> i32 {
        self.right - self.left
    }

    /// Height.
    pub fn height(&self) -> i32 {
        self.bottom - self.top
    }

    /// True if the point lies inside.
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.left && x < self.right && y >= self.top && y < self.bottom
    }
}

/// An 8-bit indexed surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Surface {
    w: usize,
    h: usize,
    pixels: Vec<u8>,
}

impl Surface {
    /// Creates a surface filled with index 0.
    pub fn new(w: usize, h: usize) -> Self {
        Surface {
            w,
            h,
            pixels: vec![0; w * h],
        }
    }

    /// A 320x200 screen surface.
    pub fn screen() -> Self {
        Self::new(SCREEN_WIDTH, SCREEN_HEIGHT)
    }

    /// Reallocates (and clears) the surface.
    pub fn create(&mut self, w: usize, h: usize) {
        self.w = w;
        self.h = h;
        self.pixels = vec![0; w * h];
    }

    /// Width in pixels.
    pub fn width(&self) -> usize {
        self.w
    }

    /// Height in pixels.
    pub fn height(&self) -> usize {
        self.h
    }

    /// Full-surface rectangle.
    pub fn rect(&self) -> Rect {
        Rect::sized(self.w, self.h)
    }

    /// Row-major pixel data.
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// Mutable pixel data.
    pub fn pixels_mut(&mut self) -> &mut [u8] {
        &mut self.pixels
    }

    /// Reads a pixel (0 outside).
    pub fn get(&self, x: i32, y: i32) -> u8 {
        if x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h {
            self.pixels[y as usize * self.w + x as usize]
        } else {
            0
        }
    }

    /// Writes a pixel; ignored outside the surface.
    pub fn put(&mut self, x: i32, y: i32, v: u8) {
        if x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h {
            self.pixels[y as usize * self.w + x as usize] = v;
        }
    }

    /// Fills everything.
    pub fn clear(&mut self, v: u8) {
        self.pixels.fill(v);
    }

    /// Fills a rectangle (clipped to the surface).
    pub fn fill_rect(&mut self, r: Rect, v: u8) {
        let mut r = r;
        r.clip(self.rect());
        for y in r.top..r.bottom {
            let row = y as usize * self.w;
            self.pixels[row + r.left as usize..row + r.right as usize].fill(v);
        }
    }

    /// Draws a 1-pixel rectangle outline.
    pub fn frame_rect(&mut self, r: Rect, v: u8) {
        for x in r.left..r.right {
            self.put(x, r.top, v);
            self.put(x, r.bottom - 1, v);
        }
        for y in r.top..r.bottom {
            self.put(r.left, y, v);
            self.put(r.right - 1, y, v);
        }
    }

    /// Copies another surface at `(x, y)` (clipped); `transparent` pixels are skipped.
    pub fn blit(&mut self, src: &Surface, x: i32, y: i32, transparent: Option<u8>) {
        for sy in 0..src.h as i32 {
            for sx in 0..src.w as i32 {
                let v = src.pixels[sy as usize * src.w + sx as usize];
                if Some(v) != transparent {
                    self.put(x + sx, y + sy, v);
                }
            }
        }
    }
}

/// Context needed by the special-effect drawers.
pub struct DrawContext<'a> {
    /// Random numbers for drawers 2 and 5.
    pub rng: &'a mut dyn RandomSource,
    /// Current hardware palette (8-bit levels) for drawer 3.
    pub palette: &'a [[u8; 3]; 256],
    /// Bottom line used by [`flags::BOTTOM_CLIPPED`].
    pub clipped_bottom: i32,
}

/// Rolls a scale mask one step and returns the bit that left the top.
#[inline]
fn roll(mask: &mut u16) -> bool {
    let bit = (*mask >> 15) & 1;
    *mask = ((*mask & 0x7FFF) << 1) | bit;
    bit != 0
}

/// `getScaledVal`: number of set bits over `n` rolls of the mask.
fn scaled_val(n: i32, mask: &mut u16) -> i32 {
    let mut result = 0;
    for _ in 0..n.max(0) {
        if roll(mask) {
            result += 1;
        }
    }
    result
}

/// Error raised by the renderer on damaged sprite data.
pub type DrawResult = openxeenng_formats::Result<()>;

/// Draws frame `frame` of `sprite` at `pos` (ScummVM `SpriteResource::draw`).
///
/// `bounds` is the clip rectangle; frames past the end are ignored like the
/// original (which works around a crash in Clouds that way).
pub fn draw_sprite(
    dest: &mut Surface,
    sprite: &SpriteFile,
    frame: usize,
    pos: (i32, i32),
    bounds: Rect,
    flags: u32,
    scale: u16,
    ctx: &mut DrawContext<'_>,
) -> DrawResult {
    let mut r = bounds;
    if flags & flags::BOTTOM_CLIPPED != 0 {
        r.clip(Rect::new(0, 0, SCREEN_WIDTH as i32, ctx.clipped_bottom));
    }
    let Some(cells) = sprite.frames().get(frame).copied() else {
        return Ok(());
    };
    draw_cell(dest, sprite, cells.cell1, pos, r, flags, scale, ctx)?;
    if cells.cell2 != 0 {
        draw_cell(dest, sprite, cells.cell2, pos, r, flags, scale, ctx)?;
    }
    Ok(())
}

/// Convenience: draw clipped to the whole destination surface.
pub fn draw_sprite_simple(
    dest: &mut Surface,
    sprite: &SpriteFile,
    frame: usize,
    pos: (i32, i32),
    flags: u32,
    scale: u16,
    ctx: &mut DrawContext<'_>,
) -> DrawResult {
    let r = dest.rect();
    draw_sprite(dest, sprite, frame, pos, r, flags, scale, ctx)
}

fn draw_cell(
    dest: &mut Surface,
    sprite: &SpriteFile,
    offset: u16,
    pt: (i32, i32),
    clip_rect: Rect,
    flags: u32,
    scale: u16,
    ctx: &mut DrawContext<'_>,
) -> DrawResult {
    let drawer = flags & flags::MODE_MASK;
    let scale_index = usize::from(scale & SCALE_MASK);
    let Some(&base_mask) = SCALE_TABLE.get(scale_index) else {
        return Err(openxeenng_formats::FormatError::invalid(
            "sprite scale",
            format!("scale {scale_index} out of range 0..16"),
        ));
    };
    let mut mask_x = base_mask;
    let mut mask_y = base_mask;
    let flipped = flags & flags::HORIZ_FLIPPED != 0;
    let enlarge = scale & SCALE_ENLARGE != 0;

    let header = sprite.cell_header(offset)?;
    let x_offset = i32::from(header.x_offset);
    let width = i32::from(header.width);
    let y_offset = i32::from(header.y_offset);
    let height = i32::from(header.height);

    let mut dest_x = pt.0 + scaled_val(x_offset, &mut mask_x);
    dest_x += (width - scaled_val(width, &mut mask_x)) / 2;
    let mut dest_y = pt.1 + scaled_val(y_offset, &mut mask_y);

    let mut bounds = clip_rect;
    if flags & flags::RESIZE != 0 {
        let (need_w, need_h) = ((x_offset + width) as usize, (y_offset + height) as usize);
        if need_w > MAX_FRAME_DIM || need_h > MAX_FRAME_DIM {
            return Err(openxeenng_formats::FormatError::invalid(
                "sprite resize",
                format!("{need_w}x{need_h} exceeds {MAX_FRAME_DIM}x{MAX_FRAME_DIM}"),
            ));
        }
        if dest.width() < need_w || dest.height() < need_h {
            dest.create(need_w, need_h);
        }
        bounds = dest.rect();
    }
    if flags & flags::SCENE_CLIPPED != 0 {
        bounds.clip(Rect::new(8, 8, 223, 141));
    }

    // Linear-address limits used by drawer 2 (it compares raw pointers).
    let pitch = dest.width() as i64;
    let dest_top = i64::from(clip_rect.top) * pitch + i64::from(clip_rect.left);
    let dest_bottom = i64::from(clip_rect.bottom - 1) * pitch + i64::from(clip_rect.right);

    let mask_x_copy = mask_x;
    let idx = (flags & flags::PARAM_MASK) as usize;
    let has_palette =
        drawer == flags::DRAWER3 && ctx.palette.iter().any(|c| c.iter().any(|&v| v != 0));
    let mut line = [TRANSPARENT; LINE_BUF];

    for rec in sprite.rows(offset)? {
        match rec? {
            RowRecord::Skip(n) => {
                dest_y += scaled_val(i32::from(n), &mut mask_y);
            }
            RowRecord::Line { start, len } => {
                if !roll(&mut mask_y) {
                    continue;
                }
                if dest_y < bounds.top || dest_y >= bounds.bottom {
                    dest_y += 1;
                    continue;
                }
                mask_x = mask_x_copy;
                line.fill(TRANSPARENT);
                decode_line(sprite.data(), start, len, header.width, flipped, &mut line)?;

                let row_left_x = if flags & flags::SCENE_CLIPPED != 0 {
                    SCENE_CLIP_LEFT
                } else {
                    clip_rect.left
                };
                let row_right_x = if flags & flags::SCENE_CLIPPED != 0 {
                    SCENE_CLIP_RIGHT
                } else {
                    clip_rect.right
                };
                let row_base = i64::from(dest_y) * pitch;
                let limits = PixelLimits {
                    left: row_base + i64::from(row_left_x),
                    right: row_base + i64::from(row_right_x),
                    top: dest_top,
                    bottom: dest_bottom,
                    pitch,
                    has_palette,
                };

                let mut xp = dest_x;
                for x in 0..width as usize {
                    if !roll(&mut mask_x) {
                        continue;
                    }
                    let v = line[LINE_ORIGIN + x];
                    if v != TRANSPARENT && xp >= bounds.left && xp < bounds.right {
                        let pixel = v as u8;
                        let at = row_base + i64::from(xp);
                        plot(dest, drawer, idx, pixel, at, enlarge, &limits, ctx);
                    }
                    xp += 1;
                    if enlarge {
                        xp += 1;
                    }
                }
                dest_y += 1;
                if enlarge {
                    dest_y += 1;
                }
            }
        }
    }
    Ok(())
}

struct PixelLimits {
    left: i64,
    right: i64,
    top: i64,
    bottom: i64,
    pitch: i64,
    /// Drawer 3 only runs once a palette is loaded (all-black = none).
    has_palette: bool,
}

#[inline]
fn write_at(dest: &mut Surface, at: i64, v: u8) {
    if at >= 0 && (at as usize) < dest.pixels.len() {
        dest.pixels[at as usize] = v;
    }
}

#[inline]
fn read_at(dest: &Surface, at: i64) -> Option<u8> {
    (at >= 0 && (at as usize) < dest.pixels.len()).then(|| dest.pixels[at as usize])
}

fn plot(
    dest: &mut Surface,
    drawer: u32,
    idx: usize,
    pixel: u8,
    at: i64,
    enlarge: bool,
    lim: &PixelLimits,
    ctx: &mut DrawContext<'_>,
) {
    let stride = lim.pitch;
    let targets: [i64; 4] = [at, at + stride, at + 1, at + 1 + stride];
    let count = if enlarge { 4 } else { 1 };
    match drawer {
        flags::DRAWER1 => {
            const OFFSET: [u8; 24] = [
                0x30, 0xC0, 0xB0, 0x10, 0x41, 0x20, 0x40, 0x21, 0x48, 0x46, 0x43, 0x40, 0xD0, 0xD3,
                0xD6, 0xD8, 0x01, 0x04, 0x07, 0x0A, 0xEA, 0xEE, 0xF2, 0xF6,
            ];
            const MASK: [u8; 24] = [
                0x0F, 0x0F, 0x0F, 0x0F, 0x0F, 0x07, 0x07, 0x0F, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07,
                0x07, 0x07, 0x0F, 0x0F, 0x0F, 0x0F, 0x07, 0x07, 0x07, 0x07,
            ];
            let off = OFFSET.get(idx).copied().unwrap_or(0);
            let mask = MASK.get(idx).copied().unwrap_or(0);
            let v = (pixel & mask).wrapping_add(off);
            for &t in &targets[..count] {
                write_at(dest, t, v);
            }
        }
        flags::DRAWER2 => {
            const MASK1: [u16; 32] = [
                3, 0, 3, 0, 3, 0, 3, 0, 2, 0, 2, 0, 2, 0, 2, 0, 1, 0, 1, 0, 1, 0, 1, 0, 0, 0, 0, 0,
                0, 0, 0, 0,
            ];
            const MASK2: [u16; 16] = [
                0x7E, 0x7E, 0x7E, 0x7E, 0x3E, 0x3E, 0x3E, 0x3E, 0x1E, 0x1E, 0x1E, 0x1E, 0x0E, 0x0E,
                0x0E, 0x0E,
            ];
            let m1 = MASK1.get(idx).copied().unwrap_or(0);
            let m2 = MASK2.get(idx).copied().unwrap_or(0);
            let mut r1 = ctx.rng.random_number(0xFFFF) as u16;
            let mut r2 = ctx.rng.random_number(0xFFFF) as u16;
            for &t in &targets[..count] {
                draw_pixel2(dest, t, pixel, lim, &mut r1, &mut r2, m1, m2);
            }
        }
        flags::DRAWER3 => {
            const MASK: [u16; 4] = [1, 3, 7, 15];
            const OFFSET: [u16; 4] = [1, 2, 4, 8];
            if lim.has_palette {
                let mask = MASK.get(idx).copied().unwrap_or(0);
                let off = OFFSET.get(idx).copied().unwrap_or(0);
                for &t in &targets[..count] {
                    draw_pixel3(dest, t, pixel, mask, off, ctx.palette);
                }
            }
        }
        flags::DRAWER4 => {
            const THRESHOLD: [u8; 4] = [4, 7, 10, 13];
            let th = THRESHOLD.get(idx).copied().unwrap_or(0);
            if pixel & 0xF >= th {
                for &t in &targets[..count] {
                    write_at(dest, t, pixel);
                }
            }
        }
        flags::DRAWER5 => {
            const THRESHOLD: [u16; 4] = [0x3333, 0x6666, 0x999A, 0xCCCD];
            let th = THRESHOLD.get(idx).copied().unwrap_or(0);
            let mut r1 = ctx.rng.random_number(0xFFFF) as u16;
            let mut r2 = ctx.rng.random_number(0xFFFF) as u16;
            for &t in &targets[..count] {
                if scramble(&mut r1, &mut r2) > th {
                    write_at(dest, t, pixel);
                }
            }
        }
        flags::DRAWER6 => {
            const MASK: [u8; 16] = [1, 2, 4, 8, 1, 3, 7, 15, 8, 12, 14, 15, 1, 2, 1, 2];
            let v = pixel ^ MASK.get(idx).copied().unwrap_or(0);
            for &t in &targets[..count] {
                write_at(dest, t, v);
            }
        }
        _ => {
            for &t in &targets[..count] {
                write_at(dest, t, pixel);
            }
        }
    }
}

/// The shift-register step shared by drawers 2 and 5; returns the new `r2`.
#[inline]
fn scramble(r1: &mut u16, r2: &mut u16) -> u16 {
    let flag = *r1 & 0x8000 != 0;
    *r1 = ((i32::from(*r1) << 1) - i32::from(*r2) - i32::from(flag)) as u16;
    let mut carry = flag;
    let new_cf = *r2 & 1 != 0;
    *r2 = (*r2 >> 1) | if carry { 0x8000 } else { 0 };
    carry = new_cf;
    *r2 = (*r2 >> 1) | if carry { 0x8000 } else { 0 };
    *r2 ^= *r1;
    *r2
}

fn draw_pixel2(
    dest: &mut Surface,
    at: i64,
    pixel: u8,
    lim: &PixelLimits,
    r1: &mut u16,
    r2: &mut u16,
    m1: u16,
    m2: u16,
) {
    const DELTA: [i8; 64] = [
        -3, 3, 0, 0, 0, 0, 0, 0, -5, 5, 0, 0, 0, 0, 0, 0, -7, 7, 0, 0, 0, 0, 0, 0, -9, 9, 0, 0, 0,
        0, 0, 0, -7, 7, 0, 0, 0, 0, 0, 0, -9, 9, 0, 0, 0, 0, 0, 0, -11, 11, 0, 0, 0, 0, 0, 0, -13,
        13, 0, 0, 0, 0, 0, 0,
    ];
    let r = scramble(r1, r2);
    let mut p = at + i64::from(DELTA[usize::from((r & m1 & m2) / 2) & 63]);
    if p >= lim.left && p < lim.right {
        p += lim.pitch * i64::from(DELTA[usize::from(((r >> 8) & m1 & m2) / 2) & 63]);
        if p >= lim.top && p < lim.bottom {
            write_at(dest, p, pixel);
        }
    }
}

fn draw_pixel3(
    dest: &mut Surface,
    at: i64,
    pixel: u8,
    mask: u16,
    offset: u16,
    pal: &[[u8; 3]; 256],
) {
    let Some(cur) = read_at(dest, at) else {
        return;
    };
    // Byte arithmetic, as in the C++ (`const byte level = ...`).
    let level = ((u16::from(pixel) & mask)
        .wrapping_sub(offset)
        .wrapping_add(u16::from(cur & 0xF))) as u8;
    let mut v = if level >= 0x80 {
        cur & 0xF0
    } else if level <= 0xF {
        (cur & 0xF0) | level
    } else {
        cur | 0xF
    };
    while v < 0xFF && pal[usize::from(v)] == [0, 0, 0] {
        v += 1;
    }
    write_at(dest, at, v);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::ScummRandom;
    use openxeenng_formats::sprite::{IndexedImage, SpriteBuilder};

    fn test_sprite() -> SpriteFile {
        let mut img = IndexedImage::new(16, 12);
        for y in 0..12 {
            for x in 0..16 {
                if (x + y) % 5 != 0 {
                    img.set(x, y, Some((16 + x + y * 3) as u8));
                }
            }
        }
        let mut b = SpriteBuilder::new();
        b.frame(img);
        SpriteFile::parse(b.build().unwrap()).unwrap()
    }

    fn ctx<'a>(rng: &'a mut ScummRandom, pal: &'a [[u8; 3]; 256]) -> DrawContext<'a> {
        DrawContext {
            rng,
            palette: pal,
            clipped_bottom: 200,
        }
    }

    #[test]
    fn unscaled_draw_matches_decoder() {
        let s = test_sprite();
        let img = s.decode_frame(0).unwrap();
        let mut dest = Surface::new(64, 64);
        let mut rng = ScummRandom::new(1);
        let pal = [[0u8; 3]; 256];
        draw_sprite_simple(&mut dest, &s, 0, (5, 7), 0, 0, &mut ctx(&mut rng, &pal)).unwrap();
        for y in 0..12 {
            for x in 0..16 {
                let expect = img.get(x, y).unwrap_or(0);
                assert_eq!(dest.get(5 + x as i32, 7 + y as i32), expect, "({x},{y})");
            }
        }
    }

    #[test]
    fn flipped_draw_mirrors() {
        let s = test_sprite();
        let img = s.decode_frame(0).unwrap();
        let mut dest = Surface::new(32, 32);
        let mut rng = ScummRandom::new(1);
        let pal = [[0u8; 3]; 256];
        draw_sprite_simple(
            &mut dest,
            &s,
            0,
            (0, 0),
            flags::HORIZ_FLIPPED,
            0,
            &mut ctx(&mut rng, &pal),
        )
        .unwrap();
        for y in 0..12 {
            for x in 0..16 {
                assert_eq!(
                    dest.get(15 - x as i32, y as i32),
                    img.get(x, y).unwrap_or(0)
                );
            }
        }
    }

    #[test]
    fn scaling_shrinks_and_enlarge_doubles() {
        let s = test_sprite();
        let pal = [[0u8; 3]; 256];
        let count = |scale: u16| {
            let mut dest = Surface::new(80, 80);
            let mut rng = ScummRandom::new(1);
            draw_sprite_simple(&mut dest, &s, 0, (2, 2), 0, scale, &mut ctx(&mut rng, &pal))
                .unwrap();
            dest.pixels().iter().filter(|&&p| p != 0).count()
        };
        let full = count(0);
        let half = count(8);
        let big = count(SCALE_ENLARGE);
        assert!(half < full / 2 + 10, "half={half} full={full}");
        assert_eq!(big, full * 4);
        assert!(count(15) < half);
    }

    #[test]
    fn clipping_and_scene_clip() {
        let s = test_sprite();
        let pal = [[0u8; 3]; 256];
        let mut rng = ScummRandom::new(1);
        let mut dest = Surface::screen();
        draw_sprite(
            &mut dest,
            &s,
            0,
            (-4, -4),
            Rect::new(0, 0, 10, 6),
            0,
            0,
            &mut ctx(&mut rng, &pal),
        )
        .unwrap();
        for y in 0..200 {
            for x in 0..320 {
                if x >= 10 || y >= 6 {
                    assert_eq!(dest.get(x, y), 0);
                }
            }
        }
        let mut dest = Surface::screen();
        draw_sprite_simple(
            &mut dest,
            &s,
            0,
            (0, 0),
            flags::SCENE_CLIPPED,
            0,
            &mut ctx(&mut rng, &pal),
        )
        .unwrap();
        for y in 0..8 {
            for x in 0..16 {
                assert_eq!(dest.get(x, y), 0);
            }
        }
    }

    #[test]
    fn drawers_do_not_panic_at_edges() {
        let s = test_sprite();
        let mut pal = [[0u8; 3]; 256];
        pal[200] = [10, 10, 10];
        let mut rng = ScummRandom::new(42);
        for drawer in 1..=7u32 {
            for idx in [0u32, 3, 15, 23, 31] {
                for scale in [0, 5, SCALE_ENLARGE] {
                    let mut dest = Surface::new(20, 20);
                    let f = (drawer << 8) | idx | flags::HORIZ_FLIPPED;
                    draw_sprite_simple(
                        &mut dest,
                        &s,
                        0,
                        (12, 15),
                        f,
                        scale,
                        &mut ctx(&mut rng, &pal),
                    )
                    .unwrap();
                }
            }
        }
    }

    #[test]
    fn rect_clip_matches_common_rect() {
        let mut r = Rect::new(-5, -5, 400, 300);
        r.clip(Rect::new(0, 0, 320, 200));
        assert_eq!(r, Rect::new(0, 0, 320, 200));
        let mut r = Rect::new(330, 210, 340, 220);
        r.clip(Rect::new(0, 0, 320, 200));
        assert_eq!(r, Rect::new(320, 200, 320, 200));
    }
}

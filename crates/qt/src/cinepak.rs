//! Cinepak (`cvid`) video decoder.
//!
//! Cinepak codes each frame as horizontal strips. A strip carries up to two
//! codebooks of 256 entries: V1 for flat areas, where one entry covers a whole
//! 4x4 block, and V4 for detail, where four entries tile the block as 2x2
//! quadrants. A codebook entry is four luma samples plus a shared chroma pair,
//! so a V1 block is effectively a 2x upscale of its entry and a V4 block is
//! full resolution.
//!
//! Frames may be inter-coded, updating only the blocks that changed and reusing
//! the previous frame elsewhere, so the decoder keeps its output buffer between
//! calls and callers must decode forward from a keyframe.

use crate::{Error, Result};

/// One codebook entry: four luma samples and a chroma pair.
#[derive(Copy, Clone, Default)]
struct Entry {
    y: [u8; 4],
    u: i8,
    v: i8,
}

#[derive(Clone, Copy)]
struct Codebooks {
    v1: [Entry; 256],
    v4: [Entry; 256],
}

impl Default for Codebooks {
    fn default() -> Self {
        // Arrays this long have no derived Default.
        Codebooks {
            v1: [Entry::default(); 256],
            v4: [Entry::default(); 256],
        }
    }
}

pub struct Cinepak {
    pub width: usize,
    pub height: usize,
    /// RGBA output, retained across frames because inter-frames reference it.
    pixels: Vec<u8>,
    /// Codebooks persist per strip index across frames, which is what lets an
    /// inter-frame send only the entries that changed.
    strips: Vec<Codebooks>,
    decoded_any: bool,
}

impl Cinepak {
    pub fn new(width: usize, height: usize) -> Cinepak {
        Cinepak {
            width,
            height,
            pixels: vec![0; width * height * 4],
            strips: Vec::new(),
            decoded_any: false,
        }
    }

    pub fn frame(&self) -> &[u8] {
        &self.pixels
    }

    /// Decodes one sample into the retained frame buffer.
    pub fn decode(&mut self, data: &[u8]) -> Result<()> {
        if data.len() < 10 {
            return Err(Error::Truncated);
        }
        let flags = data[0];
        let width = be16(data, 4) as usize;
        let height = be16(data, 6) as usize;
        let strip_count = be16(data, 8) as usize;

        // The frame header is authoritative; the sample description can lie.
        if width != 0 && height != 0 && (width != self.width || height != self.height) {
            self.width = width;
            self.height = height;
            self.pixels = vec![0; width * height * 4];
            self.decoded_any = false;
        }

        // Bit 0 clear means a keyframe: every block is coded, so nothing carries
        // over and a decode can safely start here.
        let is_keyframe = flags & 1 == 0;
        if !is_keyframe && !self.decoded_any {
            return Err(Error::Unsupported(
                "inter-frame before any keyframe".into(),
            ));
        }

        if self.strips.len() < strip_count {
            self.strips.resize(strip_count, Codebooks::default());
        }

        let mut off = 10usize;
        // Strips stack downward; a strip's rectangle gives its height, and its
        // vertical position is where the previous strip ended.
        let mut y0 = 0usize;
        for strip in 0..strip_count {
            if off + 12 > data.len() {
                break;
            }
            let strip_len = be16(data, off + 2) as usize;
            let top = be16(data, off + 4) as usize;
            let bottom = be16(data, off + 8) as usize;
            let end = (off + strip_len.max(12)).min(data.len());

            // Some encoders write the rectangle relative to the strip, others
            // absolutely. Treating it as a height and stacking handles both.
            let strip_height = bottom.saturating_sub(top);
            let y1 = (y0 + strip_height).min(self.height);

            // On a keyframe every strip after the first inherits the previous
            // strip's codebooks, because it may send only partial updates and
            // rely on those tables being populated. Without this the first
            // strip decodes correctly and the rest come out as noise.
            if is_keyframe && strip > 0 {
                let previous = self.strips[strip - 1];
                self.strips[strip] = previous;
            }

            self.decode_strip(data, off + 12, end, strip, y0, y1)?;

            y0 = y1;
            off = end;
            if y0 >= self.height {
                break;
            }
        }

        self.decoded_any = true;
        Ok(())
    }

    fn decode_strip(
        &mut self,
        data: &[u8],
        mut off: usize,
        end: usize,
        strip: usize,
        y0: usize,
        y1: usize,
    ) -> Result<()> {
        // A strip's codebooks start as whatever the previous frame left, which
        // is how partial updates work.
        let mut books = self.strips.get(strip).copied().unwrap_or_default();

        // A chunk header is four bytes: a 16-bit id and a 16-bit length that
        // counts the header itself.
        while off + 4 <= end {
            let id = be16(data, off);
            let len = be16(data, off + 2) as usize;
            let body = off + 4;
            let body_end = (off + len.max(4)).min(end);
            if body > body_end {
                break;
            }
            let chunk = &data[body..body_end];

            // Chunk ids are a bit field, and the flags live in the high
            // byte, not the low one:
            //   0x0200  this is the V1 codebook rather than V4
            //   0x0100  partial update, entries preceded by a selection mask
            //   0x0400  four-byte entries, luma only, no chroma
            // Masking the low byte instead reads every partial update as a
            // full one, which silently overwrites the inherited codebook from
            // index zero and turns everything after the first keyframe strip
            // into noise.
            const PARTIAL: u16 = 0x0100;
            const V1: u16 = 0x0200;
            const LUMA_ONLY: u16 = 0x0400;

            match id & 0xf000 {
                0x2000 => {
                    let book = if id & V1 != 0 {
                        &mut books.v1
                    } else {
                        &mut books.v4
                    };
                    load_codebook(book, chunk, id & LUMA_ONLY != 0, id & PARTIAL != 0);
                }
                0x3000 => self.vectors(id, chunk, &books, y0, y1),
                _ => {}
            }

            off = body_end;
        }

        if std::env::var_os("CINEPAK_TRACE").is_some() {
            let nz1 = books.v1.iter().filter(|e| e.y != [0; 4]).count();
            let nz4 = books.v4.iter().filter(|e| e.y != [0; 4]).count();
            eprintln!("  strip {strip} rows {y0}..{y1}: v1 {nz1}/256 populated, v4 {nz4}/256");
        }
        if let Some(slot) = self.strips.get_mut(strip) {
            *slot = books;
        }
        Ok(())
    }

    /// Decodes a vector chunk.
    ///
    /// The flag bits and the codebook indices share one byte stream: indices are
    /// read as plain bytes, and whenever a flag bit is needed with none left,
    /// the next four bytes are consumed as a fresh 32-bit mask read most
    /// significant bit first. Treating the flags as a continuous bitstream and
    /// pulling indices out of it as well produces plausible-looking noise, which
    /// is the failure this shape is easy to fall into.
    ///
    /// The chunk id says which flags are present, again in the high byte:
    /// 0x0100 adds a per-block "is this block coded at all" flag, and 0x0200
    /// means every block is V1 so no selector flag is sent.
    fn vectors(&mut self, chunk_id: u16, chunk: &[u8], books: &Codebooks, y0: usize, y1: usize) {
        let has_skip_flags = chunk_id & 0x0100 != 0;
        let v1_only = chunk_id & 0x0200 != 0;

        let mut cursor = 0usize;
        let mut flag = 0u32;
        let mut mask = 0u32;

        // Pulls the next flag bit, refilling from the stream when exhausted.
        macro_rules! next_flag {
            () => {{
                mask >>= 1;
                if mask == 0 {
                    if cursor + 4 > chunk.len() {
                        return;
                    }
                    flag = be32(chunk, cursor);
                    cursor += 4;
                    mask = 0x8000_0000;
                }
                flag & mask != 0
            }};
        }

        for (bx, by) in self.blocks(y0, y1) {
            if has_skip_flags && !next_flag!() {
                continue;
            }
            let use_v4 = if v1_only { false } else { next_flag!() };

            if use_v4 {
                if cursor + 4 > chunk.len() {
                    return;
                }
                let idx = [
                    chunk[cursor],
                    chunk[cursor + 1],
                    chunk[cursor + 2],
                    chunk[cursor + 3],
                ];
                cursor += 4;
                self.put_v4(bx, by, books, idx);
            } else {
                let Some(&i) = chunk.get(cursor) else { return };
                cursor += 1;
                self.put_v1(bx, by, books, i);
            }
        }
    }

    /// Block origins in raster order within a strip, 4x4 pixels each.
    fn blocks(&self, y0: usize, y1: usize) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        let mut y = y0;
        while y < y1 {
            let mut x = 0;
            while x < self.width {
                out.push((x, y));
                x += 4;
            }
            y += 4;
        }
        out
    }

    /// One entry upscaled 2x over the whole 4x4 block.
    fn put_v1(&mut self, bx: usize, by: usize, books: &Codebooks, index: u8) {
        let e = books.v1[index as usize];
        for qy in 0..2 {
            for qx in 0..2 {
                let luma = e.y[qy * 2 + qx];
                for dy in 0..2 {
                    for dx in 0..2 {
                        self.put(bx + qx * 2 + dx, by + qy * 2 + dy, luma, e.u, e.v);
                    }
                }
            }
        }
    }

    /// Four entries, one per 2x2 quadrant, at full resolution.
    fn put_v4(&mut self, bx: usize, by: usize, books: &Codebooks, index: [u8; 4]) {
        for (q, &i) in index.iter().enumerate() {
            let e = books.v4[i as usize];
            let (qx, qy) = (q % 2, q / 2);
            for dy in 0..2 {
                for dx in 0..2 {
                    self.put(
                        bx + qx * 2 + dx,
                        by + qy * 2 + dy,
                        e.y[dy * 2 + dx],
                        e.u,
                        e.v,
                    );
                }
            }
        }
    }

    #[inline]
    fn put(&mut self, x: usize, y: usize, luma: u8, u: i8, v: i8) {
        if x >= self.width || y >= self.height {
            return;
        }
        let (r, g, b) = yuv_to_rgb(luma, u, v);
        let o = (y * self.width + x) * 4;
        self.pixels[o] = r;
        self.pixels[o + 1] = g;
        self.pixels[o + 2] = b;
        self.pixels[o + 3] = 255;
    }
}

/// Full-range YUV to RGB. Cinepak stores chroma as a signed offset from grey.
#[inline]
fn yuv_to_rgb(y: u8, u: i8, v: i8) -> (u8, u8, u8) {
    let y = y as f32;
    let (u, v) = (u as f32, v as f32);
    let r = y + 2.0 * v;
    let g = y - 0.5 * u - v;
    let b = y + 2.0 * u;
    (clamp(r), clamp(g), clamp(b))
}

#[inline]
fn clamp(v: f32) -> u8 {
    v.clamp(0.0, 255.0) as u8
}

/// Reads codebook entries, optionally only those a leading bitmask selects.
fn load_codebook(book: &mut [Entry; 256], data: &[u8], luma_only: bool, partial: bool) {
    let stride = if luma_only { 4 } else { 6 };
    let read = |d: &[u8], o: usize| -> Entry {
        let mut e = Entry::default();
        e.y.copy_from_slice(&d[o..o + 4]);
        if !luma_only {
            e.u = d[o + 4] as i8;
            e.v = d[o + 5] as i8;
        }
        e
    };

    if !partial {
        let count = (data.len() / stride).min(256);
        for (i, entry) in book.iter_mut().enumerate().take(count) {
            *entry = read(data, i * stride);
        }
        return;
    }

    // Partial updates: a stream of 32-bit masks, each bit standing for one
    // entry, with the entries themselves following inline.
    let mut off = 0usize;
    let mut index = 0usize;
    while index < 256 && off + 4 <= data.len() {
        let mask = be32(data, off);
        off += 4;
        for bit in 0..32 {
            if index >= 256 {
                break;
            }
            if mask & (0x8000_0000 >> bit) != 0 {
                if off + stride > data.len() {
                    return;
                }
                book[index] = read(data, off);
                off += stride;
            }
            index += 1;
        }
    }
}

#[inline]
fn be16(d: &[u8], o: usize) -> u16 {
    d.get(o..o + 2)
        .map(|b| u16::from_be_bytes(b.try_into().unwrap()))
        .unwrap_or(0)
}

#[inline]
fn be32(d: &[u8], o: usize) -> u32 {
    d.get(o..o + 4)
        .map(|b| u32::from_be_bytes(b.try_into().unwrap()))
        .unwrap_or(0)
}

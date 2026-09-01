use crate::chunk::Reader;
use crate::movie::CastMember;
use crate::{Error, Result};

/// An 8-bit indexed image lifted out of a `BITD` chunk.
pub struct Bitmap {
    pub width: u16,
    pub height: u16,
    /// One palette index per pixel, `width * height` long, row-major and already
    /// trimmed of the row padding Director stores on disk.
    pub pixels: Vec<u8>,
    pub reg_x: i16,
    pub reg_y: i16,
    pub palette_ref: i16,
}

/// Director compresses `BITD` with a PackBits variant: a control byte under 0x80
/// introduces `n + 1` literal bytes, and one at or above 0x80 repeats the next
/// byte `0x101 - n` times. Small images are sometimes stored flat, which shows up
/// as the payload already being exactly the uncompressed size.
pub fn decode(member: &CastMember, raw: &[u8]) -> Result<Bitmap> {
    let width = member.width as usize;
    let height = member.height as usize;
    if width == 0 || height == 0 {
        return Err(Error::Unsupported("zero-sized bitmap".into()));
    }
    // The stored stride can disagree with the header on 1-bit members, so derive
    // it from the depth and only trust `pitch` when it is at least that wide.
    let min_stride = match member.bit_depth {
        1 => (width + 7) / 8,
        2 => (width + 3) / 4,
        4 => (width + 1) / 2,
        _ => width,
    };
    let stride = (member.pitch as usize).max(min_stride);
    let expected = stride * height;

    let packed = if raw.len() >= expected {
        raw[..expected].to_vec()
    } else {
        unpack(raw, expected)
    };
    if packed.len() < expected {
        return Err(Error::Truncated {
            need: expected,
            have: packed.len(),
        });
    }

    // Expand sub-byte depths and drop the padding columns in one pass.
    let mut pixels = Vec::with_capacity(width * height);
    for y in 0..height {
        let row = &packed[y * stride..y * stride + stride];
        match member.bit_depth {
            1 => {
                for x in 0..width {
                    let bit = (row[x / 8] >> (7 - (x % 8))) & 1;
                    // In Director a set bit is black, matching the 1-bit palette.
                    pixels.push(if bit == 0 { 0xff } else { 0x00 });
                }
            }
            2 => {
                for x in 0..width {
                    pixels.push((row[x / 4] >> (6 - 2 * (x % 4))) & 0x03);
                }
            }
            4 => {
                for x in 0..width {
                    let b = row[x / 2];
                    pixels.push(if x % 2 == 0 { b >> 4 } else { b & 0x0f });
                }
            }
            _ => pixels.extend_from_slice(&row[..width]),
        }
    }

    Ok(Bitmap {
        width: member.width,
        height: member.height,
        pixels,
        reg_x: member.reg_x,
        reg_y: member.reg_y,
        palette_ref: member.palette_ref,
    })
}

fn unpack(src: &[u8], want: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(want);
    let mut p = 0usize;
    while p < src.len() && out.len() < want {
        let n = src[p];
        p += 1;
        if n < 0x80 {
            let count = n as usize + 1;
            let end = (p + count).min(src.len());
            out.extend_from_slice(&src[p..end]);
            p = end;
        } else {
            let count = 0x101 - n as usize;
            let Some(&b) = src.get(p) else { break };
            p += 1;
            out.resize((out.len() + count).min(want), b);
        }
    }
    out
}

impl Bitmap {
    /// Expands to RGBA using `palette`, treating index 0 as transparent when
    /// `transparent_index` is set (Director's background-transparent ink).
    pub fn to_rgba(&self, palette: &crate::Palette, transparent_index: Option<u8>) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.pixels.len() * 4);
        for &i in &self.pixels {
            let [r, g, b] = palette.color(i);
            let a = if Some(i) == transparent_index { 0 } else { 255 };
            out.extend_from_slice(&[r, g, b, a]);
        }
        out
    }
}

/// Reads a `BITD` payload when the caller already knows the geometry, used by
/// tooling that walks chunks directly rather than going through the cast.
pub fn decode_raw(raw: &[u8], width: u16, height: u16, stride: u16) -> Result<Vec<u8>> {
    let _ = Reader::new(raw, crate::Endian::Big);
    let expected = stride as usize * height as usize;
    let packed = if raw.len() >= expected {
        raw[..expected].to_vec()
    } else {
        unpack(raw, expected)
    };
    let mut pixels = Vec::with_capacity(width as usize * height as usize);
    for y in 0..height as usize {
        let row = &packed[y * stride as usize..];
        pixels.extend_from_slice(&row[..width as usize]);
    }
    Ok(pixels)
}

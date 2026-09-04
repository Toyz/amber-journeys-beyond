use crate::movie::CastMember;
use crate::{Error, Result};

/// An 8-bit indexed image lifted out of a `BITD` chunk.
pub struct Bitmap {
    pub width: u16,
    pub height: u16,
    /// One palette index per pixel, `width * height` long, row-major and already
    /// trimmed of the row padding Director stores on disk.
    pub pixels: Vec<u8>,
    /// Registration offset within the image, with the member's rectangle
    /// origin already removed.
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
        1 => width.div_ceil(8),
        2 => width.div_ceil(4),
        4 => width.div_ceil(2),
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
        // The registration point is given in the member's rectangle space, so
        // the rectangle's origin has to come off to make it an offset within
        // the image. Most members have a zero origin and are unaffected; the
        // ones that do not were landing tens of pixels away from the plates
        // they belong with.
        reg_x: member.reg_x - member.origin_x,
        reg_y: member.reg_y - member.origin_y,
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
            // Clamped to the geometry the cast entry declares, as the repeat
            // branch below already is. A final literal run that overruns the
            // declared pixel count would otherwise grow the buffer past
            // width*height.
            let take = count.min(want - out.len());
            out.extend_from_slice(&src[p..(p + take).min(end)]);
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
    /// The colour key, but only where the key colour reaches in from the edge.
    ///
    /// Director's `#bgTransparent` keys every pixel of the sprite's background
    /// colour, wherever it is. That is fine when the colour is only ever the
    /// field around the art, and this game's members do not keep to it: the
    /// letter from the mailbox lays its paper in the same index its border
    /// uses, so keying the colour outright ate holes through the middle of
    /// every line of the letter.
    ///
    /// The field is what surrounds the art, so that is what is keyed. It is
    /// the same flood the matte does, and the two now differ only in which
    /// index they start from -- which is the honest shape of the difference,
    /// since neither ink is given a background colour by the room data.
    pub fn to_rgba_keyed(&self, palette: &crate::Palette, key: u8) -> Vec<u8> {
        self.to_rgba_outside(palette, key)
    }

    pub fn to_rgba(&self, palette: &crate::Palette, transparent_index: Option<u8>) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.pixels.len() * 4);
        for &i in &self.pixels {
            let [r, g, b] = palette.color(i);
            let a = if Some(i) == transparent_index { 0 } else { 255 };
            out.extend_from_slice(&[r, g, b, a]);
        }
        out
    }

    /// The index the member uses for the field around its art.
    ///
    /// Director keys on a sprite's background colour rather than on a fixed
    /// index, and the game's members do not agree on one: a room's plates lay
    /// their field in index 0 and the inventory's icons lay theirs in 255. So
    /// it is read off the border, which is what the field is.
    ///
    /// Getting this wrong is invisible on a plate, which is drawn whole, and
    /// obvious on an icon: the bar's glowing outlines came up in opaque black
    /// boxes that covered the room behind them.
    pub fn background(&self) -> u8 {
        let (w, h) = (self.width as usize, self.height as usize);
        if w == 0 || h == 0 {
            return 0;
        }
        let mut counts = [0usize; 256];
        for x in 0..w {
            counts[self.pixels[x] as usize] += 1;
            counts[self.pixels[(h - 1) * w + x] as usize] += 1;
        }
        for y in 0..h {
            counts[self.pixels[y * w] as usize] += 1;
            counts[self.pixels[y * w + w - 1] as usize] += 1;
        }
        counts
            .iter()
            .enumerate()
            .max_by_key(|(_, n)| **n)
            .map(|(i, _)| i as u8)
            .unwrap_or(0)
    }

    /// The same, with Director's `#matte` ink rather than a colour key.
    ///
    /// Matte does not make every pixel of the background colour transparent;
    /// it makes the background *outside the shape* transparent. Director
    /// derives a mask from the member's outline, so a hole in the middle of
    /// the art that happens to be the background colour stays painted.
    ///
    /// The difference is the whole PeeK unit. Its body is a slab of the same
    /// colour as the field around it, so keying on the colour punched the
    /// middle out and left a frame with the room showing through it.
    pub fn to_rgba_matte(&self, palette: &crate::Palette, background: u8) -> Vec<u8> {
        self.to_rgba_outside(palette, background)
    }

    /// Everything of `background` that the edge can reach, made transparent.
    fn to_rgba_outside(&self, palette: &crate::Palette, background: u8) -> Vec<u8> {
        let (w, h) = (self.width as usize, self.height as usize);
        let mut outside = vec![false; self.pixels.len()];
        // Flood from every edge pixel that is the background colour; anything
        // the flood cannot reach is inside the shape.
        let mut stack: Vec<usize> = Vec::new();
        let push = |stack: &mut Vec<usize>, outside: &mut Vec<bool>, i: usize| {
            if i < outside.len() && !outside[i] && self.pixels[i] == background {
                outside[i] = true;
                stack.push(i);
            }
        };
        for x in 0..w {
            push(&mut stack, &mut outside, x);
            if h > 0 {
                push(&mut stack, &mut outside, (h - 1) * w + x);
            }
        }
        for y in 0..h {
            push(&mut stack, &mut outside, y * w);
            if w > 0 {
                push(&mut stack, &mut outside, y * w + w - 1);
            }
        }
        while let Some(i) = stack.pop() {
            let (x, y) = (i % w, i / w);
            if x > 0 {
                push(&mut stack, &mut outside, i - 1);
            }
            if x + 1 < w {
                push(&mut stack, &mut outside, i + 1);
            }
            if y > 0 {
                push(&mut stack, &mut outside, i - w);
            }
            if y + 1 < h {
                push(&mut stack, &mut outside, i + w);
            }
        }

        let mut out = Vec::with_capacity(self.pixels.len() * 4);
        for (i, &p) in self.pixels.iter().enumerate() {
            let [r, g, b] = palette.color(p);
            out.extend_from_slice(&[r, g, b, if outside[i] { 0 } else { 255 }]);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // PackBits: a byte below 0x80 introduces n+1 literals, one at or above
    // introduces 0x101-n copies of the byte that follows.

    /// The colour key has the same rule, for the same reason. The letter from
    /// the mailbox lays its paper in the index its border uses, so keying the
    /// colour wherever it appeared ate holes through every line of the text.
    #[test]
    fn the_key_takes_the_field_and_leaves_the_writing() {
        // A sheet of 1 with a border of 1 around it and some 2s written on
        // it -- and one stray 1 in the middle of the writing, which is what
        // the anti-aliased text amounts to.
        let w = 5usize;
        let pixels = vec![
            1, 1, 1, 1, 1, //
            1, 2, 2, 2, 1, //
            1, 2, 1, 2, 1, //
            1, 2, 2, 2, 1, //
            1, 1, 1, 1, 1, //
        ];
        let bmp = Bitmap {
            width: w as u16,
            height: 5,
            pixels,
            reg_x: 0,
            reg_y: 0,
            palette_ref: 0,
        };
        let palette = crate::Palette::default();
        let rgba = bmp.to_rgba_keyed(&palette, 1);
        let alpha = |x: usize, y: usize| rgba[(y * w + x) * 4 + 3];

        // The field around it goes.
        assert_eq!(alpha(0, 0), 0);
        assert_eq!(alpha(4, 4), 0);
        // The writing stays, and so does the pixel inside it that happens to
        // be the same colour as the field.
        assert_eq!(alpha(1, 1), 255);
        assert_eq!(alpha(2, 2), 255, "a keyed pixel the edge cannot reach");
    }

    #[test]
    fn matte_keys_out_the_field_but_not_a_hole_in_the_middle() {
        // Director's `#matte` derives its mask from the member's outline, so
        // the background outside the shape goes and the background *inside*
        // it stays. Keying on the colour instead punches out both, which is
        // what left the PeeK unit as a frame with the room showing through.
        //
        //   0 0 0 0 0
        //   0 1 1 1 0
        //   0 1 0 1 0      <- the 0 in the middle is inside the shape
        //   0 1 1 1 0
        //   0 0 0 0 0
        let pixels = vec![
            0, 0, 0, 0, 0,
            0, 1, 1, 1, 0,
            0, 1, 0, 1, 0,
            0, 1, 1, 1, 0,
            0, 0, 0, 0, 0,
        ];
        let bmp = Bitmap { width: 5, height: 5, pixels, reg_x: 0, reg_y: 0, palette_ref: 0 };
        let palette = crate::Palette::default();
        let rgba = bmp.to_rgba_matte(&palette, 0);
        let alpha = |x: usize, y: usize| rgba[(y * 5 + x) * 4 + 3];

        assert_eq!(alpha(0, 0), 0, "the corner is outside the shape");
        assert_eq!(alpha(2, 0), 0, "and so is the edge");
        assert_eq!(alpha(1, 1), 255, "the shape itself is painted");
        assert_eq!(alpha(2, 2), 255, "and so is the hole inside it");
    }

    #[test]
    fn literal_runs_copy_verbatim() {
        assert_eq!(unpack(&[0x02, b'a', b'b', b'c'], 3), b"abc");
    }

    #[test]
    fn repeat_runs_expand() {
        assert_eq!(unpack(&[0xfd, b'z'], 4), b"zzzz");
    }

    #[test]
    fn a_row_mixes_both_kinds() {
        let src = [0x01, b'a', b'b', 0xfe, b'c', 0x00, b'd'];
        assert_eq!(unpack(&src, 7), b"abcccd");
    }

    #[test]
    fn output_never_exceeds_the_expected_length() {
        // Geometry comes from the cast entry, the payload from the chunk. A
        // disagreement must not grow the row buffer and shear the image.
        assert_eq!(unpack(&[0x80, b'x'], 4).len(), 4);
        assert_eq!(unpack(&[0x05, 1, 2, 3, 4, 5, 6], 3), vec![1, 2, 3]);
    }

    #[test]
    fn truncated_input_stops_instead_of_panicking() {
        // A literal run promising more bytes than the chunk holds, and a
        // repeat run whose value byte was cut off.
        assert_eq!(unpack(&[0x7f, b'a'], 128), b"a");
        assert_eq!(unpack(&[0xfd], 4), b"");
        assert_eq!(unpack(&[], 16), b"");
    }

    #[test]
    fn a_run_of_one_is_the_boundary_case() {
        // 0x80 is 0x101-0x80 = 129 copies, not zero; 0x7f is 128 literals.
        assert_eq!(unpack(&[0x80, b'q'], 200).len(), 129);
        assert_eq!(unpack(&[0xff, b'q'], 200), b"qq");
    }
}

#[cfg(test)]
mod ink_tests {
    use super::*;
    use crate::palette::Palette;

    /// Amber uses two inks and only two: 0 on the 2345 sprites that are a
    /// room's own plates, and 36 on the fifteen that are something held up in
    /// front of one -- a phone lifted to the ear, a bottle turned over, a
    /// newspaper being read. Those fifteen are drawn on a white field, and
    /// index zero is the only pure white in every one of this game's palettes.
    #[test]
    fn a_transparent_index_drops_out_and_nothing_else_does() {
        let mut palette = Palette::default();
        palette.colors[0] = [255, 255, 255];
        palette.colors[7] = [10, 20, 30];

        let bmp = Bitmap {
            width: 2,
            height: 1,
            pixels: vec![0, 7],
            reg_x: 0,
            reg_y: 0,
            palette_ref: 0,
        };

        // Painted: both pixels opaque, the white among them.
        let opaque = bmp.to_rgba(&palette, None);
        assert_eq!(&opaque[0..4], &[255, 255, 255, 255]);
        assert_eq!(&opaque[4..8], &[10, 20, 30, 255]);

        // Matted: the white field goes, the phone stays.
        let matted = bmp.to_rgba(&palette, Some(0));
        assert_eq!(matted[3], 0, "index zero should not be painted");
        assert_eq!(&matted[4..8], &[10, 20, 30, 255]);
    }
}


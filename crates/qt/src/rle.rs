//! Apple Animation (`rle`), the other half of the game's video.
//!
//! A hundred and thirty-three of the two hundred and seventy-eight movies on
//! the disc use this rather than Cinepak, and a hundred and thirty-one of those
//! are eight-bit indexed. It is a lossless run-length codec meant for material
//! with flat colour and little movement -- which is most of this game's
//! close-ups and inserts, where a single object animates over a still ground.
//!
//! A frame carries only the lines that changed, and within a line only the runs
//! that changed, so a frame that changes nothing is a handful of bytes.
//!
//! ```text
//! frame:  size(4) header(2)
//!         if header & 8:  start line(2) _(2) lines(2) _(2)
//!         per line:       skip(1) then ops until -1
//! op:     n > 0   n units copied literally
//!         n < -1  the next unit repeated -n times
//!         n == 0  another skip byte follows
//!         n == -1 end of frame
//! ```
//!
//! At eight bits a "unit" is four pixels, not one: the counts are in groups of
//! four and the skip byte steps four at a time. Reading them as single pixels
//! decodes a quarter of each line and smears it across the rest.

use crate::{Error, Result};

/// A 256-entry palette, as the video sample description carries it.
pub type Palette = [[u8; 3]; 256];

pub struct Rle {
    width: usize,
    height: usize,
    /// RGBA, retained between frames: this codec sends only what changed.
    pixels: Vec<u8>,
    palette: Palette,
}

impl Rle {
    pub fn new(width: usize, height: usize, palette: Palette) -> Rle {
        Rle {
            width,
            height,
            pixels: vec![0; width * height * 4],
            palette,
        }
    }

    pub fn frame(&self) -> &[u8] {
        &self.pixels
    }

    /// Decodes one sample into the retained frame buffer.
    ///
    /// `depth` is the track's bits per pixel; only the eight-bit form is
    /// indexed through the palette.
    pub fn decode(&mut self, data: &[u8], depth: u16) -> Result<()> {
        // A frame that changes nothing is written as a stub.
        if data.len() < 8 {
            return Ok(());
        }
        let mut p = 4usize; // past the chunk size
        let header = be16(data, p)?;
        p += 2;

        let (start_line, lines) = if header & 0x0008 != 0 {
            let start = be16(data, p)? as usize;
            let count = be16(data, p + 4)? as usize;
            p += 8;
            (start, count)
        } else {
            (0, self.height)
        };

        // How many pixels one count step covers. Below eight bits a unit is
        // wider still, but nothing on the disc uses those depths.
        let unit: usize = match depth {
            0..=8 => 4,
            _ => 1,
        };
        let bytes_per_pixel: usize = match depth {
            0..=8 => 1,
            16 => 2,
            24 => 3,
            _ => 4,
        };

        let mut row = start_line;
        while row < start_line + lines && row < self.height {
            let Some(&skip) = data.get(p) else { break };
            p += 1;
            // The skip byte is one-based, and it steps whole units.
            let mut x = (skip as usize).saturating_sub(1) * unit;

            loop {
                let Some(&raw) = data.get(p) else { return Ok(()) };
                p += 1;
                let code = raw as i8;
                // -1 ends this line, not the frame: the outer loop moves on to
                // the next one. Treating it as the end of the frame decodes
                // only the first changed line and leaves the rest of the
                // picture as it was.
                if code == -1 {
                    break;
                }
                if code == 0 {
                    let Some(&more) = data.get(p) else { return Ok(()) };
                    p += 1;
                    x += (more as usize).saturating_sub(1) * unit;
                    continue;
                }
                if code < 0 {
                    // A run: one unit's worth of source, repeated.
                    let times = (-(code as i32)) as usize;
                    let width_bytes = unit * bytes_per_pixel;
                    let Some(src) = data.get(p..p + width_bytes) else {
                        return Ok(());
                    };
                    p += width_bytes;
                    for _ in 0..times {
                        self.put(row, &mut x, src, depth, bytes_per_pixel);
                    }
                } else {
                    // A literal run of `code` units.
                    let width_bytes = code as usize * unit * bytes_per_pixel;
                    let Some(src) = data.get(p..p + width_bytes) else {
                        return Ok(());
                    };
                    p += width_bytes;
                    self.put(row, &mut x, src, depth, bytes_per_pixel);
                }
            }
            row += 1;
        }
        Ok(())
    }

    /// Writes decoded source bytes at `x` on `row`, advancing `x`.
    fn put(&mut self, row: usize, x: &mut usize, src: &[u8], depth: u16, bpp: usize) {
        for chunk in src.chunks_exact(bpp) {
            if *x >= self.width || row >= self.height {
                *x += 1;
                continue;
            }
            let [r, g, b] = match depth {
                0..=8 => self.palette[chunk[0] as usize],
                16 => {
                    // 5-5-5 with the high bit unused, widened by replicating
                    // the top bits rather than shifting in zeros.
                    let v = u16::from_be_bytes([chunk[0], chunk[1]]);
                    let c5 = |c: u16| ((c << 3) | (c >> 2)) as u8;
                    [c5((v >> 10) & 31), c5((v >> 5) & 31), c5(v & 31)]
                }
                24 => [chunk[0], chunk[1], chunk[2]],
                // 32-bit is stored ARGB, so the colour starts one byte in.
                _ => [chunk[1], chunk[2], chunk[3]],
            };
            let o = (row * self.width + *x) * 4;
            self.pixels[o] = r;
            self.pixels[o + 1] = g;
            self.pixels[o + 2] = b;
            self.pixels[o + 3] = 255;
            *x += 1;
        }
    }
}

fn be16(data: &[u8], at: usize) -> Result<u16> {
    data.get(at..at + 2)
        .map(|b| u16::from_be_bytes([b[0], b[1]]))
        .ok_or(Error::Truncated)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grey_ramp() -> Palette {
        let mut p = [[0u8; 3]; 256];
        for (i, c) in p.iter_mut().enumerate() {
            *c = [i as u8, i as u8, i as u8];
        }
        p
    }

    /// Wraps op bytes in the frame header the codec expects: a size, the
    /// header word with bit 3 set, then the start line and line count.
    fn frame(start: u16, lines: u16, ops: &[u8]) -> Vec<u8> {
        let mut v = vec![0, 0, 0, 0];
        v.extend_from_slice(&0x0008u16.to_be_bytes());
        v.extend_from_slice(&start.to_be_bytes());
        v.extend_from_slice(&[0, 0]);
        v.extend_from_slice(&lines.to_be_bytes());
        v.extend_from_slice(&[0, 0]);
        v.extend_from_slice(ops);
        let size = v.len() as u32;
        v[..4].copy_from_slice(&size.to_be_bytes());
        v
    }

    fn index_at(r: &Rle, x: usize, y: usize) -> u8 {
        r.frame()[(y * r.width + x) * 4]
    }

    #[test]
    fn a_literal_run_writes_four_pixels_per_unit() {
        // At eight bits a count of 1 is four pixels, not one.
        let mut r = Rle::new(8, 1, grey_ramp());
        r.decode(&frame(0, 1, &[1, 1, 10, 20, 30, 40, 0xff]), 8).unwrap();
        assert_eq!(
            [0, 1, 2, 3].map(|x| index_at(&r, x, 0)),
            [10, 20, 30, 40]
        );
    }

    #[test]
    fn a_negative_count_repeats_one_unit() {
        let mut r = Rle::new(8, 1, grey_ramp());
        // -2 repeats the next four bytes twice.
        r.decode(&frame(0, 1, &[1, 0xfe, 7, 8, 9, 10, 0xff]), 8).unwrap();
        assert_eq!(
            (0..8).map(|x| index_at(&r, x, 0)).collect::<Vec<_>>(),
            [7, 8, 9, 10, 7, 8, 9, 10]
        );
    }

    #[test]
    fn minus_one_ends_the_line_and_not_the_frame() {
        // The bug this guards: treating -1 as the end of the frame decodes
        // only the first changed line and leaves the rest of the picture as
        // it was, which on a mostly-still film is almost invisible until you
        // look for it.
        let mut r = Rle::new(4, 2, grey_ramp());
        let ops = [
            1, 1, 11, 12, 13, 14, 0xff, // line 0
            1, 1, 21, 22, 23, 24, 0xff, // line 1
        ];
        r.decode(&frame(0, 2, &ops), 8).unwrap();
        assert_eq!(index_at(&r, 0, 0), 11, "first line");
        assert_eq!(index_at(&r, 0, 1), 21, "second line must decode too");
    }

    #[test]
    fn the_skip_byte_is_one_based_and_steps_whole_units() {
        // A skip of 2 leaves the first four pixels untouched.
        let mut r = Rle::new(8, 1, grey_ramp());
        r.decode(&frame(0, 1, &[2, 1, 5, 6, 7, 8, 0xff]), 8).unwrap();
        assert_eq!(index_at(&r, 0, 0), 0, "skipped pixels keep their value");
        assert_eq!(index_at(&r, 4, 0), 5);
    }

    #[test]
    fn only_the_declared_lines_are_touched() {
        let mut r = Rle::new(4, 3, grey_ramp());
        r.decode(&frame(1, 1, &[1, 1, 9, 9, 9, 9, 0xff]), 8).unwrap();
        assert_eq!(index_at(&r, 0, 0), 0, "line 0 is untouched");
        assert_eq!(index_at(&r, 0, 1), 9, "line 1 is the one declared");
        assert_eq!(index_at(&r, 0, 2), 0, "line 2 is untouched");
    }

    #[test]
    fn the_frame_survives_between_calls() {
        // The codec sends only what changed, so anything it does not mention
        // has to still be there next frame.
        let mut r = Rle::new(4, 1, grey_ramp());
        r.decode(&frame(0, 1, &[1, 1, 3, 3, 3, 3, 0xff]), 8).unwrap();
        r.decode(&[0, 0, 0, 4], 8).unwrap(); // a stub frame: nothing changed
        assert_eq!(index_at(&r, 0, 0), 3);
    }

    #[test]
    fn a_truncated_frame_stops_instead_of_panicking() {
        let mut r = Rle::new(8, 4, grey_ramp());
        // A literal run promising more bytes than the frame holds.
        assert!(r.decode(&frame(0, 4, &[1, 40, 1, 2]), 8).is_ok());
        assert!(r.decode(&frame(0, 4, &[1]), 8).is_ok());
    }
}

//! Making a 640 by 480 stage fill a modern screen.
//!
//! There is no detail to recover. The game's room plates are 600 by 300 and
//! its films 320 by 240, so whatever a display is asked to show, that is what
//! exists. The only question is which way to grow it, and the answer is not
//! the usual one for a game of this age: this is not pixel art.
//!
//! The plates are pre-rendered 3D scenes crushed to 256 colours with ordered
//! dithering. At one to one the dither reads as texture; at three times it
//! reads as a grid of dots laid over the picture, because every dot is now
//! nine. The filters written for pixel art -- hq2x and its relatives -- make
//! that worse: they look for hard edges to follow and find the dither, and
//! then follow it.
//!
//! So there are three here. [`Filter::Nearest`] is the honest one and what the
//! game looked like on a CRT that was not doing anything clever.
//! [`Filter::Smooth`] is a plain bilinear, which softens the dither by
//! averaging it away along with everything else. [`Filter::Undither`] takes
//! the dither out first -- it is a lossy encoding of a smoother original, and
//! some of that original comes back -- and then interpolates.

/// How to grow the stage.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum Filter {
    /// Every pixel becomes a block. Sharp, faithful, and at three times the
    /// dither is a visible grid.
    #[default]
    Nearest,
    /// Bilinear. Softer, and the dither goes with the sharpness.
    Smooth,
    /// Take the dither out, then interpolate.
    Undither,
}

impl Filter {
    pub fn parse(name: &str) -> Option<Filter> {
        match name.to_ascii_lowercase().as_str() {
            "nearest" | "sharp" | "none" => Some(Filter::Nearest),
            "smooth" | "bilinear" => Some(Filter::Smooth),
            "undither" | "clean" => Some(Filter::Undither),
            _ => None,
        }
    }
}

/// Grows `src` by `factor`, writing `(w * factor, h * factor)` pixels.
pub fn up(src: &[u32], w: usize, h: usize, factor: usize, filter: Filter) -> Vec<u32> {
    let factor = factor.max(1);
    let (dw, dh) = (w * factor, h * factor);
    if factor == 1 && filter == Filter::Nearest {
        return src.to_vec();
    }
    let cleaned;
    let src = match filter {
        Filter::Undither => {
            cleaned = undither(src, w, h);
            &cleaned[..]
        }
        _ => src,
    };
    match filter {
        Filter::Nearest => nearest(src, w, h, dw, dh),
        _ => bilinear(src, w, h, dw, dh),
    }
}

fn nearest(src: &[u32], w: usize, h: usize, dw: usize, dh: usize) -> Vec<u32> {
    let mut out = vec![0u32; dw * dh];
    for y in 0..dh {
        let sy = y * h / dh;
        for x in 0..dw {
            out[y * dw + x] = src[sy * w + x * w / dw];
        }
    }
    out
}

fn bilinear(src: &[u32], w: usize, h: usize, dw: usize, dh: usize) -> Vec<u32> {
    let mut out = vec![0u32; dw * dh];
    let at = |x: usize, y: usize| -> (f32, f32, f32) {
        let p = src[y.min(h - 1) * w + x.min(w - 1)];
        (
            ((p >> 16) & 0xff) as f32,
            ((p >> 8) & 0xff) as f32,
            (p & 0xff) as f32,
        )
    };
    for y in 0..dh {
        // Sampled at pixel centres, so the edges do not creep half a pixel.
        let fy = ((y as f32 + 0.5) * h as f32 / dh as f32 - 0.5).max(0.0);
        let (y0, ty) = (fy as usize, fy.fract());
        for x in 0..dw {
            let fx = ((x as f32 + 0.5) * w as f32 / dw as f32 - 0.5).max(0.0);
            let (x0, tx) = (fx as usize, fx.fract());
            let (a, b, c, d) = (
                at(x0, y0),
                at(x0 + 1, y0),
                at(x0, y0 + 1),
                at(x0 + 1, y0 + 1),
            );
            let mix = |p: f32, q: f32, r: f32, s: f32| {
                let top = p + (q - p) * tx;
                let bottom = r + (s - r) * tx;
                (top + (bottom - top) * ty).round().clamp(0.0, 255.0) as u32
            };
            out[y * dw + x] = 0xff00_0000
                | (mix(a.0, b.0, c.0, d.0) << 16)
                | (mix(a.1, b.1, c.1, d.1) << 8)
                | mix(a.2, b.2, c.2, d.2);
        }
    }
    out
}

/// Averages away an ordered dither without averaging away the picture.
///
/// Each pixel is replaced by the mean of the neighbours close to it in colour,
/// which is a small bilateral filter. Dither is neighbouring pixels a short
/// distance apart in colour standing for a colour between them, so it
/// averages; an edge is neighbours a long way apart, so it does not. The
/// threshold is what separates the two, and 40 out of 255 is about where this
/// game's palettes put adjacent entries.
///
/// Measured per channel rather than summed across three: a grey ramp moves
/// all three at once, so a sum counts the same step three times and calls a
/// neighbouring shade an edge.
pub fn undither(src: &[u32], w: usize, h: usize) -> Vec<u32> {
    const NEAR: i32 = 40;
    let mut out = vec![0u32; src.len()];
    for y in 0..h {
        for x in 0..w {
            let here = src[y * w + x];
            let (r, g, b) = (
                ((here >> 16) & 0xff) as i32,
                ((here >> 8) & 0xff) as i32,
                (here & 0xff) as i32,
            );
            let (mut sr, mut sg, mut sb, mut n) = (0i32, 0i32, 0i32, 0i32);
            for dy in -1i32..=1 {
                for dx in -1i32..=1 {
                    let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                    if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                        continue;
                    }
                    let p = src[ny as usize * w + nx as usize];
                    let (pr, pg, pb) = (
                        ((p >> 16) & 0xff) as i32,
                        ((p >> 8) & 0xff) as i32,
                        (p & 0xff) as i32,
                    );
                    let apart = (pr - r).abs().max((pg - g).abs()).max((pb - b).abs());
                    if apart > NEAR {
                        continue;
                    }
                    sr += pr;
                    sg += pg;
                    sb += pb;
                    n += 1;
                }
            }
            let n = n.max(1);
            out[y * w + x] =
                0xff00_0000 | ((sr / n) as u32) << 16 | ((sg / n) as u32) << 8 | (sb / n) as u32;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_repeats_a_pixel_and_keeps_its_colour() {
        let src = vec![0xff00_0000, 0xffff_ffff, 0xffff_ffff, 0xff00_0000];
        let out = up(&src, 2, 2, 2, Filter::Nearest);
        assert_eq!(out.len(), 16);
        assert_eq!(out[0], 0xff00_0000);
        assert_eq!(out[1], 0xff00_0000);
        assert_eq!(out[2], 0xffff_ffff);
    }

    /// A checkerboard of two near colours is a dither and should become the
    /// colour between them; a checkerboard of black and white is an edge and
    /// should not.
    #[test]
    fn the_undither_takes_dither_and_leaves_edges() {
        let (a, b) = (0xff40_4040, 0xff50_5050);
        let dither: Vec<u32> = (0..64)
            .map(|i| if (i / 8 + i % 8) % 2 == 0 { a } else { b })
            .collect();
        let cleaned = undither(&dither, 8, 8);
        let middle = cleaned[3 * 8 + 3] & 0xff;
        assert!(
            (0x47..=0x49).contains(&middle),
            "the two should have averaged, got {middle:#x}"
        );

        let edges: Vec<u32> = (0..64)
            .map(|i| if (i / 8 + i % 8) % 2 == 0 { 0xff00_0000 } else { 0xffff_ffff })
            .collect();
        let kept = undither(&edges, 8, 8);
        assert_eq!(kept[3 * 8 + 3] & 0xff, 0, "black stays black beside white");
    }

    #[test]
    fn a_filter_is_named_the_obvious_ways() {
        assert_eq!(Filter::parse("smooth"), Some(Filter::Smooth));
        assert_eq!(Filter::parse("UNDITHER"), Some(Filter::Undither));
        assert_eq!(Filter::parse("bicubic"), None);
    }
}

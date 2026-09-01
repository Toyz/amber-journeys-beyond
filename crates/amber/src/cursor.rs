//! Cursor drawing.
//!
//! The game's own cursors are 1-bit image and mask pairs at cast 2500 onward,
//! indexed as `2500 + (cursorID - 6000) * 2`, and which cursor a verb uses is
//! decided in `castCursor` and its callers rather than in the room data. Until
//! those are decoded, the shapes here stand in: what matters for playing is
//! that the pointer says what a click will do.
//!
//! Cursors are drawn into the framebuffer rather than handed to the window,
//! so they scale with the stage and need no platform cursor support.

use crate::world::Verb;

/// Colour of the cursor body and its outline, chosen to stay legible over both
/// the bright exteriors and the very dark interiors.
const INK: u32 = 0xffff_ffff;
const EDGE: u32 = 0xff00_0000;

/// Draws the cursor for `verb` at `(x, y)`.
pub fn draw(frame: &mut [u32], w: i32, h: i32, x: i32, y: i32, verb: Option<Verb>) {
    match verb {
        Some(Verb::Forward) => arrow(frame, w, h, x, y, 0, -1),
        Some(Verb::Left) => arrow(frame, w, h, x, y, -1, 0),
        Some(Verb::Right) => arrow(frame, w, h, x, y, 1, 0),
        Some(Verb::Up) => arrow(frame, w, h, x, y, 0, -1),
        Some(Verb::Down) => arrow(frame, w, h, x, y, 0, 1),
        Some(Verb::Examine) => lens(frame, w, h, x, y),
        Some(Verb::Pointer) | Some(Verb::NextPage) => hand(frame, w, h, x, y),
        Some(Verb::ItemInUse) => target(frame, w, h, x, y),
        Some(Verb::RotateLeft) => arrow(frame, w, h, x, y, -1, 0),
        Some(Verb::RotateRight) => arrow(frame, w, h, x, y, 1, 0),
        // Browse blankets whole rooms and means nothing in particular, so it
        // gets the same neutral dot as empty space.
        Some(Verb::Browse) | None => dot(frame, w, h, x, y),
    }
}

fn put(frame: &mut [u32], w: i32, h: i32, x: i32, y: i32, colour: u32) {
    if x >= 0 && x < w && y >= 0 && y < h {
        frame[(y * w + x) as usize] = colour;
    }
}

/// A filled shape with a one-pixel outline, so it reads on any background.
fn stamp(frame: &mut [u32], w: i32, h: i32, points: &[(i32, i32)], x: i32, y: i32) {
    for &(dx, dy) in points {
        for (ex, ey) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
            put(frame, w, h, x + dx + ex, y + dy + ey, EDGE);
        }
    }
    for &(dx, dy) in points {
        put(frame, w, h, x + dx, y + dy, INK);
    }
}

/// A triangular arrow pointing along `(sx, sy)`.
fn arrow(frame: &mut [u32], w: i32, h: i32, x: i32, y: i32, sx: i32, sy: i32) {
    let mut pts = Vec::new();
    for step in 0..10 {
        let spread = step / 2;
        for side in -spread..=spread {
            // Along the pointing axis, widening as it goes back.
            let (dx, dy) = if sy != 0 {
                (side, sy * step)
            } else {
                (sx * step, side)
            };
            pts.push((dx, dy));
        }
    }
    stamp(frame, w, h, &pts, x, y);
}

/// A ring with a handle, for examine.
fn lens(frame: &mut [u32], w: i32, h: i32, x: i32, y: i32) {
    let mut pts = Vec::new();
    for a in 0..48 {
        let t = a as f32 * std::f32::consts::TAU / 48.0;
        pts.push(((t.cos() * 6.0) as i32, (t.sin() * 6.0) as i32));
    }
    for d in 6..12 {
        pts.push((d, d));
    }
    stamp(frame, w, h, &pts, x, y);
}

/// A blunt pointing shape for operable things.
fn hand(frame: &mut [u32], w: i32, h: i32, x: i32, y: i32) {
    let mut pts = Vec::new();
    for dy in 0..10 {
        for dx in 0..6 {
            if dy < 4 && dx > 2 {
                continue;
            }
            pts.push((dx, dy));
        }
    }
    stamp(frame, w, h, &pts, x, y);
}

/// Crosshairs, shown while an item is in hand.
fn target(frame: &mut [u32], w: i32, h: i32, x: i32, y: i32) {
    let mut pts = Vec::new();
    for d in -8i32..=8 {
        if d.abs() > 2 {
            pts.push((d, 0));
            pts.push((0, d));
        }
    }
    stamp(frame, w, h, &pts, x, y);
}

/// A small neutral dot for regions that do nothing in particular.
fn dot(frame: &mut [u32], w: i32, h: i32, x: i32, y: i32) {
    let pts: Vec<(i32, i32)> = (-1..=1).flat_map(|a| (-1..=1).map(move |b| (a, b))).collect();
    stamp(frame, w, h, &pts, x, y);
}

/// Outlines a rectangle, used to show where a hotspot actually lies.
pub fn outline(frame: &mut [u32], w: i32, h: i32, r: lingo::Rect, colour: u32) {
    for x in r.left.max(0)..r.right.min(w) {
        put(frame, w, h, x, r.top, colour);
        put(frame, w, h, x, r.bottom - 1, colour);
    }
    for y in r.top.max(0)..r.bottom.min(h) {
        put(frame, w, h, r.left, y, colour);
        put(frame, w, h, r.right - 1, y, colour);
    }
}

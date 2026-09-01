//! Cursor drawing.
//!
//! The game's cursors are its own art, not shapes drawn here. `setUpGame` in
//! the hub movie builds a table called `YugoCursors` mapping a cursor's name to
//! an id, and `castCursor` turns an id into a pair of cast members:
//!
//! ```text
//! image = 2500 + (id - 6000) * 2
//! mask  = image + 1
//! ```
//!
//! Two one-bit members: the image says black or white and the mask says which
//! of its pixels are drawn at all, which is how a Macintosh cursor has always
//! worked.
//!
//! ```text
//! browse 6018   left 6012     right 6006    forward 6001
//! examine 6024  up 6111       down 6112     pointer 6100
//! back 3003     noCursor 128  nextPage 6110
//! rotateLeft 6119             rotateRight 6109
//! ```
//!
//! And an item in hand has its own cursor -- the scan unit, the crowbar, the
//! weedkiller -- so the pointer is the thing being carried rather than an arrow
//! with the thing implied. Those are in the same table, keyed by item name.
//!
//! Two ids fall outside the arithmetic: `#back` at 3003 and `#noCursor` at 128
//! are system cursors rather than cast members, and the second of those is how
//! the game hides the pointer.
//!
//! Cursors are drawn into the framebuffer rather than handed to the window, so
//! they scale with the stage and need no platform cursor support.

use crate::world::Verb;

/// `YugoCursors`, from `setUpGame` in the hub movie.
///
/// The ids below 6000 are system cursors and have no cast behind them:
/// `#back` is 3003 and `#noCursor` is 128.
pub const YUGO_CURSORS: [(&str, i32); 22] = [
    ("browse", 6018),
    ("left", 6012),
    ("right", 6006),
    ("forward", 6001),
    ("back", 3003),
    ("examine", 6024),
    ("up", 6111),
    ("down", 6112),
    ("pointer", 6100),
    ("noCursor", 128),
    ("WeedKiller", 6102),
    ("ScanDevice", 6103),
    ("Oscillator", 6108),
    ("Headgear", 6107),
    ("BedroomKey", 6106),
    ("Crowbar", 6105),
    ("Videotape", 6104),
    ("None", 6100),
    ("PeekUnit", 6100),
    ("nextPage", 6110),
    ("rotateRight", 6109),
    ("rotateLeft", 6119),
];

/// The cast members an id names: the image, then its mask.
///
/// From `castCursor`, and the ordering is the part to get right:
///
/// ```text
/// whichCursor = cursorID - 6000
/// cMask = 2500 + whichCursor * 2
/// cursor( [cMask - 1, cMask] )
/// ```
///
/// The **mask** sits at the computed offset and the image is one *below* it.
/// Reading it the other way round -- image at the offset, mask above -- draws
/// each cursor's mask as its picture and the next cursor's picture as its
/// mask, which is wrong in a way that still looks like a cursor: a mask is a
/// filled silhouette of the right shape, so the arrow is arrow-shaped and only
/// the shading is nonsense.
pub fn casts_for(id: i32) -> Option<(u32, u32)> {
    if id < 6000 {
        // A system cursor, with no art on the disc.
        return None;
    }
    let mask = 2500 + (id - 6000) * 2;
    Some((mask as u32 - 1, mask as u32))
}

/// The cursor a verb asks for, by name in the table above.
pub fn name_for(verb: Option<Verb>, holding: Option<&str>) -> &'static str {
    // Something in hand replaces the pointer with itself, whatever the region
    // underneath would otherwise have asked for.
    if let Some(item) = holding {
        if let Some((name, _)) = YUGO_CURSORS
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(item))
        {
            return name;
        }
    }
    match verb {
        Some(Verb::Forward) => "forward",
        Some(Verb::Left) => "left",
        Some(Verb::Right) => "right",
        Some(Verb::Up) => "up",
        Some(Verb::Down) => "down",
        Some(Verb::Examine) => "examine",
        Some(Verb::Pointer) => "pointer",
        Some(Verb::NextPage) => "nextPage",
        Some(Verb::RotateLeft) => "rotateLeft",
        Some(Verb::RotateRight) => "rotateRight",
        Some(Verb::ItemInUse) => "pointer",
        Some(Verb::Browse) | None => "browse",
    }
}

/// The id a verb asks for.
pub fn id_for(verb: Option<Verb>, holding: Option<&str>) -> Option<i32> {
    let want = name_for(verb, holding);
    YUGO_CURSORS
        .iter()
        .find(|(n, _)| *n == want)
        .map(|(_, id)| *id)
}

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

#[cfg(test)]
mod cursor_tests {
    use super::*;

    /// `castCursor` is four lines and the whole of the mapping:
    /// `2500 + (id - 6000) * 2` for the image and one past it for the mask.
    #[test]
    fn an_id_names_a_pair_of_casts() {
        // `cursor( [cMask - 1, cMask] )`: the mask is at the offset and the
        // image is the cast below it.
        assert_eq!(casts_for(6000), Some((2499, 2500)));
        assert_eq!(casts_for(6001), Some((2501, 2502))); // forward
        assert_eq!(casts_for(6018), Some((2535, 2536))); // browse
        assert_eq!(casts_for(6024), Some((2547, 2548))); // examine
    }

    /// Two entries in the table are system cursors with no art behind them:
    /// `#back` at 3003 and `#noCursor` at 128, which is how the game hides the
    /// pointer. Running those through the arithmetic would ask for a cast
    /// below 2500, which is somebody else's picture.
    #[test]
    fn and_a_system_cursor_names_none() {
        assert_eq!(casts_for(3003), None);
        assert_eq!(casts_for(128), None);
    }

    #[test]
    fn a_verb_asks_for_its_own_cursor() {
        assert_eq!(id_for(Some(Verb::Forward), None), Some(6001));
        assert_eq!(id_for(Some(Verb::Examine), None), Some(6024));
        assert_eq!(id_for(Some(Verb::Browse), None), Some(6018));
        // Nothing under the pointer is the browse cursor too: the game covers
        // whole rooms with a browse region and means nothing in particular.
        assert_eq!(id_for(None, None), Some(6018));
    }

    #[test]
    fn but_something_in_hand_replaces_it() {
        // Carrying the scan unit, the pointer *is* the scan unit, whatever
        // the region underneath would have asked for.
        assert_eq!(id_for(Some(Verb::Forward), Some("ScanDevice")), Some(6103));
        assert_eq!(id_for(Some(Verb::Examine), Some("Crowbar")), Some(6105));
        // And an item with no cursor of its own leaves the verb's alone.
        assert_eq!(id_for(Some(Verb::Forward), Some("nosuchthing")), Some(6001));
    }
}


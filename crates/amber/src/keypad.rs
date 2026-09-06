//! The keyboard Roxy's laptop needs, drawn on the stage.
//!
//! A departure, and written down as one. Exactly one thing in this game reads
//! the keyboard: the office laptop wants a password typed at it, and the
//! password is WISDOM. On a phone there is no keyboard to type it with, and
//! this engine had no text input on any front end -- so the laptop was a dead
//! end everywhere, and behind it sit the PT suite and the fragment alignment
//! puzzle.
//!
//! So the engine draws one. It is in the engine rather than in a front end for
//! the reason the menu is: three front ends with a keyboard each is three
//! keyboards that will disagree, and only one of them would ever be looked at.
//!
//! Letters only. The original accepts anything alphanumeric -- `mIsAlphaNum` --
//! but the one password in the game is six letters, and a digit row would push
//! the keys up over the prompt the player is trying to read.

use crate::state::State;

/// Whether the laptop is waiting to be typed at.
///
/// Both of the states that draw the prompt: `#prompting` is the machine
/// asking, `#password` is a player part way through an answer.
pub fn showing(state: &State) -> bool {
    let screen = state.get("playerIsUsingLaptop");
    screen.is_symbol("prompting") || screen.is_symbol("password")
}

/// The rows, in the order they are drawn.
///
/// `<` is backspace and `OK` is return, which is what the handler wants; they
/// sit on the bottom row so the third row is as wide as the second and the
/// whole thing squares up.
const ROWS: [&[&str]; 3] = [
    &["Q", "W", "E", "R", "T", "Y", "U", "I", "O", "P"],
    &["A", "S", "D", "F", "G", "H", "J", "K", "L"],
    &["Z", "X", "C", "V", "B", "N", "M", "<", "OK"],
];

const KEY_W: i32 = 52;
const KEY_H: i32 = 30;
const GAP: i32 = 4;
/// Three rows, sitting on the bottom edge. The laptop's screen ends at 357 and
/// the prompt it draws is at 220, so nothing the player is reading is covered.
const TOP: i32 = 366;

/// Where every key is, as `(what it sends, x, y, w, h)`.
pub fn layout(w: usize) -> Vec<(&'static str, i32, i32, i32, i32)> {
    let mut out = Vec::new();
    for (r, row) in ROWS.iter().enumerate() {
        // Each row centred on its own, which is what keeps the stagger even
        // when the rows are different lengths.
        let span = row.len() as i32 * (KEY_W + GAP) - GAP;
        let mut x = (w as i32 - span) / 2;
        let y = TOP + r as i32 * (KEY_H + GAP);
        for key in row.iter() {
            out.push((*key, x, y, KEY_W, KEY_H));
            x += KEY_W + GAP;
        }
    }
    out
}

/// What a tap sends to the laptop, if it landed on a key.
///
/// The name is what [`crate::natives`] takes: a letter as itself, and the two
/// special keys by name, because a carriage return does not survive being
/// written down as a Lingo value.
pub fn hit(x: i32, y: i32, w: usize) -> Option<String> {
    layout(w)
        .into_iter()
        .find(|(_, kx, ky, kw, kh)| x >= *kx && x < kx + kw && y >= *ky && y < ky + kh)
        .map(|(key, ..)| match key {
            "<" => "backspace".to_string(),
            "OK" => "return".to_string(),
            letter => letter.to_string(),
        })
}

/// Whether a point is anywhere on the keyboard.
///
/// The caller needs this as well as [`hit`]: a tap in the gap between two keys
/// is not a key, but it must not fall through to the room's hotspots either,
/// or missing a key by two pixels backs the player out of the laptop.
pub fn covers(_x: i32, y: i32, _w: usize) -> bool {
    // The whole band, edge to edge: the rows are centred and different
    // lengths, so the space beside the short ones is keyboard too as far as
    // the room is concerned.
    y >= TOP - GAP
}

/// Draws the keyboard, and how much has been typed.
pub fn draw(out: &mut [u32], w: usize, h: usize, typed: usize, pointer: Option<(i32, i32)>) {
    let top = TOP - GAP - 22;
    crate::menu::shade(out, w, h, 0, top, w as i32, h as i32);

    // Six boxes, filling as the password is typed. The game's own prompt shows
    // the same count in the middle of the screen, but that is a row of small
    // dashes on a dark monitor and on a phone it is not enough to tell whether
    // a tap registered.
    let boxes = 6;
    let bw = 16;
    let span = boxes * (bw + 6) - 6;
    let mut bx = (w as i32 - span) / 2;
    for i in 0..boxes {
        let lit = (i as usize) < typed;
        let colour = if lit { 0x00c8_a55a } else { 0x0044_3a28 };
        crate::menu::fill(out, w, h, bx, top + 6, bx + bw, top + 8, colour);
        bx += bw + 6;
    }

    for (key, kx, ky, kw, kh) in layout(w) {
        let over = pointer
            .is_some_and(|(px, py)| px >= kx && px < kx + kw && py >= ky && py < ky + kh);
        crate::menu::fill(
            out,
            w,
            h,
            kx,
            ky,
            kx + kw,
            ky + kh,
            if over { 0x0038_2d1c } else { 0x001a_150e },
        );
        // A lit top edge rather than a full border: at this size a box around
        // every key reads as a grid and the letters get lost in it.
        crate::menu::fill(out, w, h, kx, ky, kx + kw, ky + 1, 0x0044_3a28);
        let ink = if over { 0x00ff_e9b0 } else { 0x00b5_a88a };
        let scale = 2;
        crate::menu::label(
            out,
            w,
            h,
            kx + (kw - crate::menu::text_width(key, scale)) / 2,
            ky + (kh - 7 * scale) / 2,
            key,
            scale,
            ink,
        );
    }
}

#[cfg(test)]
mod eyeball {
    /// Draws the laptop with the keyboard over it, to be looked at.
    ///
    /// `AMBER_SHOTS=/somewhere cargo test -p amber --release -- --ignored
    /// --nocapture keypad::eyeball`.
    #[test]
    #[ignore]
    fn over_the_laptop() {
        const W: usize = 640;
        const H: usize = 480;
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../extract");
        if !root.is_dir() {
            return;
        }
        let mut game = crate::game::Game::new(&root).expect("extract/ is not a game");
        let Some(room) = game.world.resolve("OfficeMonitorCU", Some("ROXY")) else { return };
        game.room = room;
        game.state.set("playerIsUsingLaptop", lingo::Value::Symbol("password".into()));
        game.state.set_all(
            "passwordAttempt",
            "WIS".chars().map(|c| lingo::Value::String(c.to_string())).collect(),
        );
        game.start_room_video();

        let mut frame = vec![0u32; W * H];
        game.draw(&mut frame, W as u32, H as u32);
        super::draw(&mut frame, W, H, 3, Some((320, 380)));

        let rgba: Vec<u8> = frame
            .iter()
            .flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8, 0xff])
            .collect();
        let out = std::path::Path::new(
            &std::env::var("AMBER_SHOTS").unwrap_or_else(|_| "/tmp".into()),
        )
        .join("laptop-keys.png");
        crate::write_png(&out, W as u32, H as u32, &rgba).expect("write");
        println!("wrote {}", out.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lingo::Value;

    const W: usize = 640;
    const H: usize = 480;

    #[test]
    fn it_shows_only_while_the_laptop_is_asking() {
        let mut s = State::new();
        assert!(!showing(&s), "offered a keyboard with nothing to type into");
        s.set("playerIsUsingLaptop", Value::Symbol("prompting".into()));
        assert!(showing(&s));
        s.set("playerIsUsingLaptop", Value::Symbol("password".into()));
        assert!(showing(&s));
        s.set("playerIsUsingLaptop", Value::Symbol("crashed".into()));
        assert!(!showing(&s));
    }

    /// Every key is on the stage, and no two overlap.
    #[test]
    fn the_keys_are_laid_out_without_touching() {
        let keys = layout(W);
        assert_eq!(keys.len(), 10 + 9 + 9);
        for (name, x, y, kw, kh) in &keys {
            assert!(*x >= 0 && x + kw <= W as i32, "{name} is off the side");
            assert!(*y >= 0 && y + kh <= H as i32, "{name} is off the bottom");
        }
        for (i, a) in keys.iter().enumerate() {
            for b in keys.iter().skip(i + 1) {
                let apart = a.1 + a.3 <= b.1 || b.1 + b.3 <= a.1 || a.2 + a.4 <= b.2 || b.2 + b.4 <= a.2;
                assert!(apart, "{} and {} overlap", a.0, b.0);
            }
        }
    }

    /// Every key can be pressed, and reports itself.
    #[test]
    fn every_key_answers_to_a_tap_in_the_middle_of_it() {
        for (name, x, y, kw, kh) in layout(W) {
            let got = hit(x + kw / 2, y + kh / 2, W);
            let want = match name {
                "<" => "backspace",
                "OK" => "return",
                other => other,
            };
            assert_eq!(got.as_deref(), Some(want), "{name} did not answer");
        }
    }

    /// The password can be spelled out on it.
    ///
    /// The one thing this keyboard exists for, so it is worth asserting
    /// outright rather than trusting that the rows contain the alphabet.
    #[test]
    fn wisdom_can_be_typed() {
        for c in "WISDOM".chars() {
            let found = layout(W).into_iter().any(|(k, ..)| k == c.to_string());
            assert!(found, "no {c} key");
        }
    }

    /// A miss between keys is still the keyboard's, not the room's.
    #[test]
    fn the_gaps_do_not_fall_through() {
        let keys = layout(W);
        let (_, x, y, kw, _) = keys[0];
        // The gap to the right of the first key.
        assert!(hit(x + kw + 1, y + 4, W).is_none(), "the gap is a key");
        assert!(covers(x + kw + 1, y + 4, W), "the gap fell through to the room");
        assert!(!covers(x, TOP - 40, W), "the keyboard claimed the whole stage");
    }
}

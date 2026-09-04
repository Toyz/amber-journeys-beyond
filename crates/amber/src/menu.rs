//! The pause menu, and the only text this engine draws.
//!
//! A departure, and written down as one. The original hung its menu off
//! Director's menu bar -- `installMenu`, which appears when the pointer reaches
//! the top of the screen -- and this engine has no menu bar to hang anything
//! from. A phone has no top-of-screen hover either, and a game with no way to
//! save is a game you cannot put down, so this is an addition rather than a
//! port.
//!
//! It lives in the engine rather than in a front end for the reason the rest of
//! this project keeps arriving at: two front ends with a menu each is two menus
//! that will disagree.

/// A 5 by 7 font, written as the shapes rather than as hex so a wrong pixel can
/// be seen rather than decoded. Only the characters the menu uses.
const GLYPHS: &[(char, &str)] = &[
    ('A', ".###.#...##...#######...##...##...#"),
    ('B', "####.#...##...#####.#...##...#####."),
    ('C', ".###.#...##....#....#....#...#.###."),
    ('D', "####.#...##...##...##...##...#####."),
    ('E', "######....#....####.#....#....#####"),
    ('F', "######....#....####.#....#....#...."),
    ('G', ".###.#...##....#.####...##...#.###."),
    ('I', "#####..#....#....#....#....#..#####"),
    ('L', "#....#....#....#....#....#....#####"),
    ('M', "#...###.###.#.##...##...##...##...#"),
    ('N', "#...###..##.#.##..###...##...##...#"),
    ('O', ".###.#...##...##...##...##...#.###."),
    ('Q', ".###.#...##...##...##.#.##..#..##.#"),
    ('R', "####.#...##...#####.#.#..#..#.#...#"),
    ('S', ".###.#...##.....###.....##...#.###."),
    ('T', "#####..#....#....#....#....#....#.."),
    ('U', "#...##...##...##...##...##...#.###."),
    ('V', "#...##...##...##...##...#.#.#...#.."),
    ('W', "#...##...##...##...##.#.###.###...#"),
    ('Y', "#...##...#.#.#...#....#....#....#.."),
    ('K', "#...##..#.#.#..##...#.#..#..#.#...#"),
    ('P', "####.#...##...#####.#....#....#...."),
    ('H', "#...##...##...#######...##...##...#"),
    ('J', "....#....#....#....##...##...#.###."),
    ('X', "#...##...#.#.#...#...#.#.#...##...#"),
    ('Z', "#####....#...#...#...#...#....#####"),
    ('0', ".###.#...##..###.#.###..##...#.###."),
    ('1', "..#...##....#....#....#....#...###."),
    ('2', ".###.#...#....#...#...#...#...#####"),
    ('3', "#####...#...##.....#....##...#.###."),
    ('4', "...#...##..#.#.#..#.#####...#....#."),
    ('5', "######....####.....#....##...#.###."),
    ('6', "..##..#...#....####.#...##...#.###."),
    ('7', "#####....#...#...#...#....#....#..."),
    ('8', ".###.#...##...#.###.#...##...#.###."),
    ('9', ".###.#...##...#.####....#...#..##.."),
    (' ', "..................................."),
];

const GW: usize = 5;
const GH: usize = 7;

fn glyph(c: char) -> Option<&'static str> {
    let c = c.to_ascii_uppercase();
    GLYPHS.iter().find(|(g, _)| *g == c).map(|(_, s)| *s)
}

/// Draws a string at `scale`, returning how wide it was.
#[allow(clippy::too_many_arguments)]
fn text(out: &mut [u32], w: usize, h: usize, x: i32, y: i32, s: &str, scale: i32, rgb: u32) {
    let mut pen = x;
    for c in s.chars() {
        if let Some(shape) = glyph(c) {
            let bytes = shape.as_bytes();
            for row in 0..GH {
                for col in 0..GW {
                    if bytes.get(row * GW + col) != Some(&b'#') {
                        continue;
                    }
                    for dy in 0..scale {
                        for dx in 0..scale {
                            let px = pen + col as i32 * scale + dx;
                            let py = y + row as i32 * scale + dy;
                            if px >= 0 && py >= 0 && (px as usize) < w && (py as usize) < h {
                                out[py as usize * w + px as usize] = rgb;
                            }
                        }
                    }
                }
            }
        }
        pen += (GW as i32 + 1) * scale;
    }
}

fn width(s: &str, scale: i32) -> i32 {
    s.chars().count() as i32 * (GW as i32 + 1) * scale
}

/// Which screen the menu is showing.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum Page {
    #[default]
    Root,
    Save,
    Load,
    Settings,
}

/// What the loop has to act on. Everything else the menu handles itself.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Action {
    None,
    Resume,
    Quit,
    Save(usize),
    Load(usize),
}

/// How many save slots. Three is enough to keep a run, a experiment and a
/// spare without turning the menu into a file manager.
pub const SLOTS: usize = 3;

/// The player's own settings, as against the game's.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Settings {
    pub volume: f32,
    pub filter: crate::scale::Filter,
    /// Whether to draw the directional pad. On by default because a phone
    /// needs it; a mouse may not want it.
    pub pad: bool,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings { volume: 1.0, filter: crate::scale::Filter::default(), pad: true }
    }
}

/// A row on whichever page is showing.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum Row {
    Resume,
    Go(Page),
    Quit,
    Slot(usize),
    Volume,
    Filter,
    Pad,
}

/// The pause menu.
pub struct Menu {
    pub page: Page,
    /// Something to say back -- "saved", or why a load failed. A menu that
    /// does a thing and says nothing is indistinguishable from one that did
    /// nothing.
    pub note: Option<String>,
    /// What each slot holds, read when the menu opens. `None` is empty.
    pub slots: [Option<String>; SLOTS],
    pub settings: Settings,
}

/// Panel geometry, derived once and used by both the drawing and the hit test
/// -- so a row can never be somewhere other than where it looks.
const ROW: i32 = 46;
const SCALE: i32 = 3;
const PANEL_W: i32 = 460;
const TITLE_Y: i32 = 30;
const FIRST_ROW: i32 = 96;
const FOOT: i32 = 38;

fn panel(rows: usize, w: usize, h: usize) -> (i32, i32, i32, i32) {
    let ph = FIRST_ROW + rows as i32 * ROW + FOOT;
    ((w as i32 - PANEL_W) / 2, (h as i32 - ph) / 2, PANEL_W, ph)
}

fn row_rect(i: usize, rows: usize, w: usize, h: usize) -> (i32, i32, i32, i32) {
    let (px, py, pw, _) = panel(rows, w, h);
    (px + 22, py + FIRST_ROW + i as i32 * ROW, pw - 44, ROW - 8)
}

/// A filled rectangle, clipped to the frame.
#[allow(clippy::too_many_arguments)]
fn fill(out: &mut [u32], w: usize, h: usize, x0: i32, y0: i32, x1: i32, y1: i32, rgb: u32) {
    for y in y0.max(0)..y1.min(h as i32) {
        for x in x0.max(0)..x1.min(w as i32) {
            out[y as usize * w + x as usize] = rgb;
        }
    }
}

fn filter_name(f: crate::scale::Filter) -> &'static str {
    match f {
        crate::scale::Filter::Nearest => "SHARP",
        crate::scale::Filter::Smooth => "SMOOTH",
        crate::scale::Filter::Undither => "CLEAN",
    }
}

impl Default for Menu {
    fn default() -> Menu {
        Menu::new(Settings::default(), Default::default())
    }
}

impl Menu {
    pub fn new(settings: Settings, slots: [Option<String>; SLOTS]) -> Menu {
        Menu { page: Page::Root, note: None, slots, settings }
    }

    /// The rows on the page showing now, with what each says.
    fn rows(&self) -> Vec<(Row, String)> {
        match self.page {
            Page::Root => {
                let mut rows = vec![
                    (Row::Resume, "RESUME".to_string()),
                    (Row::Go(Page::Save), "SAVE GAME".to_string()),
                ];
                // Load is offered only when there is something to load. An
                // item that is always there and sometimes does nothing teaches
                // the player to distrust it.
                if self.slots.iter().any(Option::is_some) {
                    rows.push((Row::Go(Page::Load), "LOAD GAME".to_string()));
                }
                rows.push((Row::Go(Page::Settings), "SETTINGS".to_string()));
                rows.push((Row::Quit, "QUIT".to_string()));
                rows
            }
            Page::Save | Page::Load => {
                let mut rows: Vec<(Row, String)> = (0..SLOTS)
                    .map(|i| {
                        let what = self.slots[i].clone().unwrap_or_else(|| "EMPTY".to_string());
                        (Row::Slot(i), format!("{} {}", i + 1, what))
                    })
                    .collect();
                rows.push((Row::Go(Page::Root), "BACK".to_string()));
                rows
            }
            Page::Settings => vec![
                (Row::Volume, format!("VOLUME {}", (self.settings.volume * 100.0) as i32)),
                (Row::Filter, format!("PICTURE {}", filter_name(self.settings.filter))),
                (Row::Pad, format!("PAD {}", if self.settings.pad { "ON" } else { "OFF" })),
                (Row::Go(Page::Root), "BACK".to_string()),
            ],
        }
    }

    fn title(&self) -> &'static str {
        match self.page {
            Page::Root => "AMBER",
            Page::Save => "SAVE",
            Page::Load => "LOAD",
            Page::Settings => "SETTINGS",
        }
    }

    fn row_at(&self, x: i32, y: i32, w: usize, h: usize) -> Option<Row> {
        let rows = self.rows();
        rows.iter().enumerate().find_map(|(i, (row, _))| {
            let (rx, ry, rw, rh) = row_rect(i, rows.len(), w, h);
            (x >= rx && x < rx + rw && y >= ry && y < ry + rh).then_some(*row)
        })
    }

    /// Takes a click. Navigation and settings are handled here; only what the
    /// loop must do comes back.
    pub fn click(&mut self, x: i32, y: i32, w: usize, h: usize) -> Action {
        let Some(row) = self.row_at(x, y, w, h) else { return Action::None };
        self.note = None;
        match row {
            Row::Resume => Action::Resume,
            Row::Quit => Action::Quit,
            Row::Go(page) => {
                self.page = page;
                Action::None
            }
            Row::Slot(i) => match self.page {
                Page::Save => Action::Save(i),
                // Loading an empty slot is not a failure worth a message; it
                // is a row that should not have been pressed.
                Page::Load if self.slots[i].is_some() => Action::Load(i),
                _ => Action::None,
            },
            // Cycling rather than a slider: one tap region per row, which is
            // the same gesture everywhere else in this menu and needs no drag.
            Row::Volume => {
                let step = ((self.settings.volume * 100.0).round() as i32 - 25).rem_euclid(125);
                self.settings.volume = step as f32 / 100.0;
                Action::None
            }
            Row::Filter => {
                use crate::scale::Filter::*;
                self.settings.filter = match self.settings.filter {
                    Nearest => Smooth,
                    Smooth => Undither,
                    Undither => Nearest,
                };
                Action::None
            }
            Row::Pad => {
                self.settings.pad = !self.settings.pad;
                Action::None
            }
        }
    }

    pub fn draw(&self, out: &mut [u32], w: usize, h: usize, pointer: Option<(i32, i32)>) {
        // The scene is dimmed rather than covered -- the game is paused, not
        // gone -- but far enough down that a bright film behind it cannot be
        // read through the panel.
        for pixel in out.iter_mut() {
            let (r, g, b) = (*pixel >> 16 & 0xff, *pixel >> 8 & 0xff, *pixel & 0xff);
            *pixel = (r / 8) << 16 | (g / 8) << 8 | (b / 8);
        }

        let rows = self.rows();
        let (px, py, pw, ph) = panel(rows.len(), w, h);
        fill(out, w, h, px, py, px + pw, py + ph, 0x000d_0b08);
        // A rule top and bottom rather than a box: the game's palette is warm
        // and a full border reads as a dialog from somewhere else entirely.
        fill(out, w, h, px, py, px + pw, py + 2, 0x00c8_a55a);
        fill(out, w, h, px, py + ph - 2, px + pw, py + ph, 0x00c8_a55a);

        let title = self.title();
        text(out, w, h, px + (pw - width(title, 4)) / 2, py + TITLE_Y, title, 4, 0x00c8_a55a);
        fill(out, w, h, px + 140, py + TITLE_Y + 44, px + pw - 140, py + TITLE_Y + 45, 0x0044_3a28);

        let over = pointer.and_then(|(x, y)| self.row_at(x, y, w, h));
        for (i, (row, label)) in rows.iter().enumerate() {
            let (rx, ry, rw, rh) = row_rect(i, rows.len(), w, h);
            let lit = over == Some(*row);
            if lit {
                fill(out, w, h, rx, ry, rx + rw, ry + rh, 0x0026_1f14);
                // A marker rather than a full outline, so the eye is led along
                // the row instead of boxed in.
                fill(out, w, h, rx, ry, rx + 3, ry + rh, 0x00c8_a55a);
            }
            // An empty slot is dimmer: it is a row you can save to and not one
            // you can load from, and it should look like it.
            let empty = matches!(row, Row::Slot(i) if self.slots[*i].is_none());
            let ink = match (lit, empty) {
                (true, _) => 0x00ff_e9b0,
                (false, true) => 0x0060_5949,
                (false, false) => 0x0099_8f78,
            };
            let ty = ry + (rh - GH as i32 * SCALE) / 2;
            text(out, w, h, rx + (rw - width(label, SCALE)) / 2, ty, label, SCALE, ink);
        }

        if let Some(note) = &self.note {
            let ny = py + ph - 26;
            text(out, w, h, px + (pw - width(note, 2)) / 2, ny, note, 2, 0x0087_7a5e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filled() -> Menu {
        Menu::new(
            Settings::default(),
            [Some("ROXY HALL".into()), None, Some("EDWIN ICE".into())],
        )
    }

    /// Every glyph is exactly the grid it claims to be.
    ///
    /// Written as shapes so a mistake is visible, which only helps if the
    /// shapes are all the same size -- a short one would silently shift every
    /// row after it and draw a different letter.
    #[test]
    fn every_glyph_is_five_by_seven() {
        for (c, shape) in GLYPHS {
            assert_eq!(shape.len(), GW * GH, "glyph {c} is {} cells", shape.len());
            assert!(
                shape.bytes().all(|b| b == b'#' || b == b'.'),
                "glyph {c} has something other than # and ."
            );
        }
    }

    /// Every label on every page is drawable.
    ///
    /// A missing glyph draws a gap, so a typo -- or a room name with a
    /// character the font has never seen -- comes out as a word with a hole in
    /// it rather than as any kind of error.
    #[test]
    fn every_label_has_its_letters() {
        let mut menu = filled();
        for page in [Page::Root, Page::Save, Page::Load, Page::Settings] {
            menu.page = page;
            for (_, label) in menu.rows() {
                for c in label.chars() {
                    assert!(glyph(c).is_some(), "no glyph for {c:?} in {label:?} on {page:?}");
                }
            }
        }
    }

    /// The hit test agrees with the drawing, row for row, on every page.
    ///
    /// Both read `row_rect`, so this asserts that every row is reachable and
    /// that none overlaps its neighbour -- a menu whose second row answers for
    /// the first is worse than one that does not respond, because it looks
    /// like it worked.
    #[test]
    fn the_hit_test_follows_the_drawing() {
        let mut menu = filled();
        let (w, h) = (640usize, 480usize);
        for page in [Page::Root, Page::Save, Page::Load, Page::Settings] {
            menu.page = page;
            let rows = menu.rows();
            for (i, (row, _)) in rows.iter().enumerate() {
                let (rx, ry, rw, rh) = row_rect(i, rows.len(), w, h);
                assert_eq!(
                    menu.row_at(rx + rw / 2, ry + rh / 2, w, h),
                    Some(*row),
                    "row {i} of {page:?} did not answer for itself"
                );
            }
            let (px, py, _, _) = panel(rows.len(), w, h);
            assert_eq!(menu.row_at(px + 8, py + 8, w, h), None, "the title picked a row");
        }
    }

    /// The panel fits on the stage on every page, including the longest.
    #[test]
    fn the_panel_fits_the_stage() {
        let mut menu = filled();
        for page in [Page::Root, Page::Save, Page::Load, Page::Settings] {
            menu.page = page;
            let (px, py, pw, ph) = panel(menu.rows().len(), 640, 480);
            assert!(px >= 0 && py >= 0, "{page:?} panel starts off stage");
            assert!(px + pw <= 640 && py + ph <= 480, "{page:?} panel runs off stage");
        }
    }

    /// Load is offered only when a slot holds something, and an empty slot
    /// cannot be loaded from even if the row is pressed.
    #[test]
    fn load_needs_something_to_load() {
        let empty = Menu::new(Settings::default(), Default::default());
        assert!(
            !empty.rows().iter().any(|(r, _)| matches!(r, Row::Go(Page::Load))),
            "offered LOAD with every slot empty"
        );

        let mut menu = filled();
        menu.page = Page::Load;
        let rows = menu.rows();
        let (_, ry, _, rh) = row_rect(1, rows.len(), 640, 480);
        // Slot 2 is the empty one.
        assert_eq!(menu.click(320, ry + rh / 2, 640, 480), Action::None);
        let (_, ry, _, rh) = row_rect(0, rows.len(), 640, 480);
        assert_eq!(menu.click(320, ry + rh / 2, 640, 480), Action::Load(0));
    }

    /// Saving is offered for every slot, full or not -- that is what
    /// overwriting is.
    #[test]
    fn any_slot_can_be_saved_over() {
        let mut menu = filled();
        menu.page = Page::Save;
        let rows = menu.rows();
        for i in 0..SLOTS {
            let (_, ry, _, rh) = row_rect(i, rows.len(), 640, 480);
            assert_eq!(menu.click(320, ry + rh / 2, 640, 480), Action::Save(i));
        }
    }

    /// Volume cycles through every step and comes back, and never leaves 0..1.
    #[test]
    fn volume_cycles_and_stays_in_range() {
        let mut menu = filled();
        menu.page = Page::Settings;
        let rows = menu.rows();
        let (_, ry, _, rh) = row_rect(0, rows.len(), 640, 480);
        let mut seen = Vec::new();
        for _ in 0..5 {
            menu.click(320, ry + rh / 2, 640, 480);
            let v = menu.settings.volume;
            assert!((0.0..=1.0).contains(&v), "volume left its range at {v}");
            seen.push((v * 100.0).round() as i32);
        }
        seen.sort_unstable();
        assert_eq!(seen, vec![0, 25, 50, 75, 100], "volume did not visit every step");
    }
}

/// The on-screen buttons.
///
/// A phone has no keyboard, so the two keys the game actually needs -- pause
/// and skip -- have to be things you can touch. Drawn by the engine rather than
/// by a front end so the desktop and the browser get the same two, and a mouse
/// can use them instead of remembering which key does what.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Tap {
    Menu,
    Skip,
    /// Put down whatever is being held up. Only there when there is something.
    Close,
}

/// A button: what it does, what it says, and where it sits.
type Button = (Tap, &'static str, (i32, i32, i32, i32));

/// Buttons live in the letterbox band above the scene.
///
/// The band is free: rooms are 600 by 300 centred on a 640 by 480 stage, and
/// the catch-all `#itemInUse` region every room opens with starts at y = 60.
/// So a button here takes no click the game wanted.
const BUTTON_TOP: i32 = 8;
const BUTTON_H: i32 = 40;
const PAD: i32 = 10;

/// The buttons showing now, laid out right to left.
fn buttons(film: bool, close: bool, w: usize) -> Vec<Button> {
    let mut out = Vec::new();
    let mut right = w as i32 - 12;
    for (tap, label) in [(Tap::Menu, "MENU"), (Tap::Skip, "SKIP"), (Tap::Close, "CLOSE")] {
        // Each is only there while it does something. A button that is always
        // present and usually does nothing teaches the player to ignore it --
        // and CLOSE in particular has to mean "there is something to close",
        // because that is the only way it says the game is waiting.
        if tap == Tap::Skip && !film {
            continue;
        }
        if tap == Tap::Close && !close {
            continue;
        }
        let bw = width(label, 2) + PAD * 2;
        let rect = (right - bw, BUTTON_TOP, bw, BUTTON_H);
        right = rect.0 - 8;
        out.push((tap, label, rect));
    }
    out
}

pub fn hud_hit(x: i32, y: i32, w: usize, film: bool, close: bool) -> Option<Tap> {
    buttons(film, close, w)
        .into_iter()
        .find_map(|(tap, _, (bx, by, bw, bh))| {
            (x >= bx && x < bx + bw && y >= by && y < by + bh).then_some(tap)
        })
}

pub fn draw_hud(
    out: &mut [u32],
    w: usize,
    h: usize,
    film: bool,
    close: bool,
    pointer: Option<(i32, i32)>,
) {
    let over = pointer.and_then(|(x, y)| hud_hit(x, y, w, film, close));
    for (tap, label, (bx, by, bw, bh)) in buttons(film, close, w) {
        let lit = over == Some(tap);
        let (face, edge, ink) = if lit {
            (0x0032_2c22, 0x00c8_a55a, 0x00ff_e9b0)
        } else {
            (0x0019_1712, 0x0055_4c3a, 0x0098_8e76)
        };
        for y in by..by + bh {
            for x in bx..bx + bw {
                if x < 0 || y < 0 || x as usize >= w || y as usize >= h {
                    continue;
                }
                let border = x == bx || x == bx + bw - 1 || y == by || y == by + bh - 1;
                out[y as usize * w + x as usize] = if border { edge } else { face };
            }
        }
        text(out, w, h, bx + PAD, by + (bh - GH as i32 * 2) / 2, label, 2, ink);
    }
}

#[cfg(test)]
mod hud_tests {
    use super::*;

    /// Both button labels are drawable, and the buttons sit above the region
    /// every room opens with -- otherwise they would be stealing clicks the
    /// game wanted.
    #[test]
    fn the_buttons_are_drawable_and_clear_of_the_scene() {
        for (_, label, (_, by, _, bh)) in buttons(true, true, 640) {
            for c in label.chars() {
                assert!(glyph(c).is_some(), "no glyph for {c:?} in {label:?}");
            }
            assert!(by + bh <= 60, "{label} reaches y={} and rooms start at 60", by + bh);
        }
    }

    /// Skip is only offered while a film is running.
    #[test]
    fn skip_appears_only_with_a_film() {
        assert!(hud_hit(600, 20, 640, false, false).is_some(), "menu button missing");
        let with = buttons(true, false, 640).len();
        let without = buttons(false, false, 640).len();
        assert_eq!(with, without + 1, "skip did not come and go with the film");
        // And close comes and goes with there being something to close.
        assert_eq!(
            buttons(false, true, 640).len(),
            without + 1,
            "close did not come and go with the way out"
        );
    }
}

/// The directional pad.
///
/// Amber is navigated by clicking small regions of the scene, which is fine
/// with a mouse and poor with a thumb. The pad puts the directions the room
/// actually offers somewhere reliable -- and only those, so it is a readout of
/// where you can go as much as a control.
///
/// Each button carries the point the engine worked out for it, so a tap is the
/// same click a player would have made on the scene rather than a second way
/// of moving that could drift from the first.
use crate::world::Verb;

/// A pad button: the direction, the click it stands for, and where it sits.
type PadButton = (Verb, (i32, i32), (i32, i32));

/// Where each direction sits, clear of the top buttons and the inventory bar.
///
/// The sides are fixed. The bottom is laid out as a group so that forward on
/// its own is centred on the stage and forward-with-down straddles the middle,
/// rather than forward sitting off to one side whenever down happens to exist.
const PAD_SIZE: i32 = 48;
const PAD_GAP: i32 = 10;
const BOTTOM_Y: i32 = 316;

/// Mixes `src` over `dst`. `alpha` is 0 to 255.
///
/// The pad sits on the scene rather than beside it -- there is nowhere beside
/// it to sit on a 4:3 stage -- so it is drawn through: enough to find with a
/// thumb, not enough to take the room away.
fn blend(dst: u32, src: u32, alpha: u32) -> u32 {
    let mix = |shift: u32| {
        let (d, s) = (dst >> shift & 0xff, src >> shift & 0xff);
        (d * (255 - alpha) + s * alpha) / 255
    };
    mix(16) << 16 | mix(8) << 8 | mix(0)
}

fn pad_layout(dirs: &[(Verb, (i32, i32))], w: usize) -> Vec<PadButton> {
    let has = |v: Verb| dirs.iter().find(|(d, _)| *d == v).map(|(_, at)| *at);
    let mut out = Vec::new();

    if let Some(at) = has(Verb::Left) {
        out.push((Verb::Left, at, (8, 208)));
    }
    if let Some(at) = has(Verb::Right) {
        out.push((Verb::Right, at, (w as i32 - 8 - PAD_SIZE, 208)));
    }
    if let Some(at) = has(Verb::Up) {
        out.push((Verb::Up, at, ((w as i32 - PAD_SIZE) / 2, 62)));
    }

    // The bottom row, centred as a whole.
    let bottom: Vec<(Verb, (i32, i32))> = [Verb::Down, Verb::Forward]
        .into_iter()
        .filter_map(|v| has(v).map(|at| (v, at)))
        .collect();
    let span = bottom.len() as i32 * PAD_SIZE + (bottom.len() as i32 - 1).max(0) * PAD_GAP;
    let mut x = (w as i32 - span) / 2;
    for (verb, at) in bottom {
        out.push((verb, at, (x, BOTTOM_Y)));
        x += PAD_SIZE + PAD_GAP;
    }
    out
}

/// The point to click for a tap on the pad, if it landed on a button.
pub fn pad_hit(x: i32, y: i32, dirs: &[(Verb, (i32, i32))], w: usize) -> Option<(i32, i32)> {
    pad_layout(dirs, w).into_iter().find_map(|(_, at, (bx, by))| {
        (x >= bx && x < bx + PAD_SIZE && y >= by && y < by + PAD_SIZE).then_some(at)
    })
}

/// A solid triangle, centred in its button and pointing whichever way it goes.
fn arrow(out: &mut [u32], w: usize, h: usize, bx: i32, by: i32, verb: Verb, rgb: u32) {
    // Measured from the middle of the button outwards, so it is centred on
    // both axes by construction rather than by an offset that has to be right.
    let (cx, cy) = (bx + PAD_SIZE / 2, by + PAD_SIZE / 2);
    let span = PAD_SIZE - 26;
    for step in 0..span {
        let half = step / 2;
        for off in -half..=half {
            let (px, py) = match verb {
                // `step` is the distance from the tip, so the tip is the near
                // edge and the base is the far one.
                Verb::Up | Verb::Forward => (cx + off, cy - span / 2 + step),
                Verb::Down => (cx + off, cy + span / 2 - step),
                Verb::Left => (cx - span / 2 + step, cy + off),
                _ => (cx + span / 2 - step, cy + off),
            };
            if px >= 0 && py >= 0 && (px as usize) < w && (py as usize) < h {
                let at = py as usize * w + px as usize;
                out[at] = blend(out[at], rgb, 210);
            }
        }
    }
}

pub fn draw_pad(
    out: &mut [u32],
    w: usize,
    h: usize,
    dirs: &[(Verb, (i32, i32))],
    pointer: Option<(i32, i32)>,
) {
    let over = pointer.and_then(|(x, y)| pad_hit(x, y, dirs, w));
    for (verb, at, (bx, by)) in pad_layout(dirs, w) {
        let lit = over == Some(at);
        let (face, edge, ink) = if lit {
            (0x0032_2c22, 0x00c8_a55a, 0x00ff_e9b0)
        } else {
            (0x0014_120e, 0x004a_4232, 0x008d_8368)
        };
        for y in by..by + PAD_SIZE {
            for x in bx..bx + PAD_SIZE {
                if x < 0 || y < 0 || x as usize >= w || y as usize >= h {
                    continue;
                }
                let border =
                    x == bx || x == bx + PAD_SIZE - 1 || y == by || y == by + PAD_SIZE - 1;
                let at = y as usize * w + x as usize;
                // A faint wash with a slightly firmer edge: enough to say where
                // the button is without becoming a panel over the room.
                out[at] = if border {
                    blend(out[at], edge, if lit { 190 } else { 110 })
                } else {
                    blend(out[at], face, if lit { 150 } else { 70 })
                };
            }
        }
        arrow(out, w, h, bx, by, verb, ink);
    }
}

#[cfg(test)]
mod pad_tests {
    use super::*;

    fn dirs(list: &[Verb]) -> Vec<(Verb, (i32, i32))> {
        list.iter().enumerate().map(|(i, v)| (*v, (100 + i as i32, 200))).collect()
    }

    /// Every button is clear of the inventory bar and of the top buttons.
    ///
    /// The bar's icons are 67 square centred on y = 410, so they start at 377;
    /// the top buttons end at 48. A pad button overlapping either would take
    /// clicks meant for something else.
    #[test]
    fn the_pad_clears_the_bar_and_the_buttons() {
        let all = dirs(&[Verb::Left, Verb::Right, Verb::Forward, Verb::Up, Verb::Down]);
        for (verb, _, (_, by)) in pad_layout(&all, 640) {
            assert!(by >= 56, "{verb:?} at y={by} runs into the top buttons");
            assert!(by + PAD_SIZE <= 377, "{verb:?} runs into the inventory bar");
        }
    }

    /// The bottom row is centred on the stage whether it holds one button or
    /// two.
    ///
    /// Each direction used to have a fixed slot, so forward sat off to one
    /// side the moment down existed beside it -- the row was never centred as
    /// a row.
    #[test]
    fn the_bottom_row_is_centred() {
        let centre = |list: &[Verb]| {
            let laid = pad_layout(&dirs(list), 640);
            let xs: Vec<i32> = laid
                .iter()
                .filter(|(v, _, _)| matches!(v, Verb::Forward | Verb::Down))
                .map(|(_, _, (bx, _))| *bx)
                .collect();
            let left = *xs.iter().min().expect("nothing on the bottom row");
            let right = *xs.iter().max().expect("nothing on the bottom row") + PAD_SIZE;
            (left + right) / 2
        };
        assert_eq!(centre(&[Verb::Forward]), 320, "forward alone is not centred");
        assert_eq!(centre(&[Verb::Down, Verb::Forward]), 320, "the pair is not centred");
    }

    /// A tap gives back the click the engine worked out, not the button's own
    /// middle -- the whole point is that it is the same click a player would
    /// have made on the scene.
    #[test]
    fn a_tap_returns_the_engines_own_point() {
        let only = vec![(Verb::Forward, (321, 199))];
        let (_, _, (bx, by)) = pad_layout(&only, 640)[0];
        assert_eq!(pad_hit(bx + 4, by + 4, &only, 640), Some((321, 199)));
        assert_eq!(pad_hit(bx - 30, by, &only, 640), None);
        // A direction the room does not offer has no button at all.
        assert_eq!(pad_hit(8, 208, &only, 640), None, "drew a button for a dead direction");
    }
}

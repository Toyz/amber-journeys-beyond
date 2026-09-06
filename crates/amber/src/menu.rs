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

    // Lower case and punctuation, added when the menu grew a reader for the
    // three documents that shipped on the disc. A ninety kilobyte manual set
    // in capitals is a manual nobody reads, and the hints file is the one
    // thing in this game that talks a stuck player through the first hour.
    //
    // Seven rows leave no room below the baseline, so the descenders on g, j,
    // p, q and y are drawn a row high with the body lifted to match. Every
    // 5 by 7 font of this vintage does the same thing.
    ('a', "...........###.....#.#####...#.####"),
    ('b', "#....#....####.#...##...##...#####."),
    ('c', "...........###.#....#....#.....###."),
    ('d', "....#....#.#####...##...##...#.####"),
    ('e', "...........###.#...#######.....###."),
    ('f', "..##..#..#.#...###...#....#....#..."),
    ('g', "...........#####...#.####....#####."),
    ('h', "#....#....####.#...##...##...##...#"),
    ('i', "..#........##....#....#....#...###."),
    ('j', "...#........##....#....#.#..#..##.."),
    ('k', "#....#....#..#.#.#..##...#.#..#..#."),
    ('l', ".##....#....#....#....#....#...###."),
    ('m', "..........##.#.#.#.##.#.##...##...#"),
    ('n', "..........####.#...##...##...##...#"),
    ('o', "...........###.#...##...##...#.###."),
    ('p', "..........####.#...#####.#....#...."),
    ('q', "...........#####...#.####....#....#"),
    ('r', "..........#.##.##..##....#....#...."),
    ('s', "...........#####.....###.....#####."),
    ('t', "..#....#..#####..#....#....#.#...#."),
    ('u', "..........#...##...##...##..##.##.#"),
    ('v', "..........#...##...##...#.#.#...#.."),
    ('w', "..........#...##...##.#.##.#.#.#.#."),
    ('x', "..........#...#.#.#...#...#.#.#...#"),
    ('y', "..........#...##...#.####....#.###."),
    ('z', "..........#####...#...#...#...#####"),
    ('.', "................................#.."),
    (',', "...........................#...#..."),
    (':', "............#..............#......."),
    (';', "............#..............#...#..."),
    ('\'', "..#....#...#......................."),
    ('"', ".#.#..#.#.........................."),
    ('!', "..#....#....#....#....#.........#.."),
    ('?', ".###.#...#....#...#...#.........#.."),
    ('(', "...#...#...#....#....#.....#.....#."),
    (')', ".#.....#.....#....#....#...#...#..."),
    ('[', ".###..#....#....#....#....#....###."),
    (']', ".###....#....#....#....#....#..###."),
    ('-', "................###................"),
    ('_', "..............................#####"),
    ('/', "....#....#...#...#...#...#....#...."),
    ('&', ".##..#..#.#.#...#...#.#.##..#..##.#"),
    ('%', "##..###.#...#....#...#...#.##.#..##"),
    ('+', ".......#....#..#####..#....#......."),
    ('=', "..........#####.....#####.........."),
    ('*', ".......#..#.#.#.###.#.#.#..#......."),
    ('#', ".#.#..#.#.#####.#.#.#####.#.#..#.#."),
    ('$', "..#...#####.#...###...#.#####...#.."),
    ('@', ".###.#...##.####.#.##.####.....###."),
    ('<', "........#...#...#.....#.....#......"),
    ('>', "......#.....#.....#...#...#........"),
    ('\\', "#....#.....#.....#.....#.....#....#"),
    ('|', "..#....#....#....#....#....#....#.."),
    ('{', "...#...#....#...#.....#....#.....#."),
    ('}', ".#.....#....#.....#...#....#...#..."),
    ('~', "...........#..##.#.##..#..........."),
    ('^', "..#...#.#.#...#...................."),
    ('`', ".#.....#..........................."),
];

const GW: usize = 5;
const GH: usize = 7;

fn glyph(c: char) -> Option<&'static str> {
    // The character as written first, and only then folded. Folding first
    // would draw every lower case letter as a capital now that there are
    // shapes for both -- which is what this did before there were.
    GLYPHS
        .iter()
        .find(|(g, _)| *g == c)
        .or_else(|| {
            let up = c.to_ascii_uppercase();
            GLYPHS.iter().find(|(g, _)| *g == up)
        })
        .map(|(_, s)| *s)
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

/// How wide a string is drawn, for anything laying out its own buttons.
pub fn text_width(s: &str, scale: i32) -> i32 {
    width(s, scale)
}

/// Draws a string, for the same.
#[allow(clippy::too_many_arguments)]
pub fn label(out: &mut [u32], w: usize, h: usize, x: i32, y: i32, s: &str, scale: i32, rgb: u32) {
    text(out, w, h, x, y, s, scale, rgb);
}

/// Darkens a band of the frame, so something drawn over the scene can be read
/// against it.
pub fn shade(out: &mut [u32], w: usize, h: usize, x0: i32, y0: i32, x1: i32, y1: i32) {
    for y in y0.max(0)..y1.min(h as i32) {
        for x in x0.max(0)..x1.min(w as i32) {
            let p = &mut out[y as usize * w + x as usize];
            let (r, g, b) = (*p >> 16 & 0xff, *p >> 8 & 0xff, *p & 0xff);
            *p = (r / 5) << 16 | (g / 5) << 8 | (b / 5);
        }
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
    /// The list of documents the disc carries.
    Docs,
    /// One of them, open at `Menu::scroll`.
    Reading(usize),
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
    Doc(usize),
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
    /// The documents that shipped on the disc, already wrapped to
    /// [`READ_COLUMNS`]. Empty if the disc has none, and then the menu does
    /// not offer them.
    pub docs: Vec<crate::docs::Doc>,
    /// The first line showing on the reading page.
    pub scroll: usize,
}

/// The reading page: how wide the column is, and how it is set.
///
/// The documents are wrapped to `READ_COLUMNS` when they are loaded, so this
/// is the one number the wrap and the drawing have to agree on. Everything
/// else here is derived from it.
pub const READ_COLUMNS: usize = 49;
const READ_SCALE: i32 = 2;
const READ_MARGIN: i32 = 26;
const READ_TOP: i32 = 58;
/// Baseline to baseline, which is the glyph plus a little air. Set solid it
/// reads as a wall.
const READ_LINE: i32 = GH as i32 * READ_SCALE + 5;
const READ_FOOT: i32 = 46;

/// How many lines fit on screen at once.
fn read_rows(h: usize) -> usize {
    (((h as i32 - READ_TOP - READ_FOOT - 6) / READ_LINE).max(1)) as usize
}

/// The three buttons along the foot of the reading page.
///
/// Back, and a page each way. Buttons rather than a drag because every other
/// thing in this menu is a tap, and a reader that alone wanted a gesture is a
/// reader people will not find their way out of.
const READ_KEYS: [&str; 3] = ["BACK", "UP", "DOWN"];

fn read_key_rect(i: usize, w: usize, h: usize) -> (i32, i32, i32, i32) {
    let span = w as i32 - READ_MARGIN * 2;
    let each = span / READ_KEYS.len() as i32;
    (
        READ_MARGIN + i as i32 * each + 4,
        h as i32 - READ_FOOT + 4,
        each - 8,
        READ_FOOT - 12,
    )
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
pub fn fill(out: &mut [u32], w: usize, h: usize, x0: i32, y0: i32, x1: i32, y1: i32, rgb: u32) {
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
        Menu { page: Page::Root, note: None, slots, settings, docs: Vec::new(), scroll: 0 }
    }

    /// Hands the menu the disc's documents.
    ///
    /// Separate from `new` because the menu is built every time it is opened
    /// and the documents are read once: the caller keeps them and passes a
    /// copy in.
    pub fn with_docs(mut self, docs: Vec<crate::docs::Doc>) -> Menu {
        self.docs = docs;
        self
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
                // Only when the disc carried them. A pressing without the
                // documents should not offer a page that opens onto nothing.
                if !self.docs.is_empty() {
                    rows.push((Row::Go(Page::Docs), "HINTS AND MANUAL".to_string()));
                }
                rows.push((Row::Go(Page::Settings), "SETTINGS".to_string()));
                rows.push((Row::Quit, "QUIT".to_string()));
                rows
            }
            Page::Docs => {
                let mut rows: Vec<(Row, String)> = self
                    .docs
                    .iter()
                    .enumerate()
                    .map(|(i, d)| (Row::Doc(i), d.title.clone()))
                    .collect();
                rows.push((Row::Go(Page::Root), "BACK".to_string()));
                rows
            }
            // Not a list of rows. `row_at` never asks, and `draw` takes its
            // own branch well before this.
            Page::Reading(_) => Vec::new(),
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

    fn title(&self) -> &str {
        match self.page {
            Page::Root => "AMBER",
            Page::Save => "SAVE",
            Page::Load => "LOAD",
            Page::Settings => "SETTINGS",
            Page::Docs => "READING",
            Page::Reading(i) => {
                // Borrowed from the document itself, so the page says what is
                // open rather than "READING" again.
                return self.docs.get(i).map(|d| d.title.as_str()).unwrap_or("READING");
            }
        }
    }

    fn row_at(&self, x: i32, y: i32, w: usize, h: usize) -> Option<Row> {
        let rows = self.rows();
        rows.iter().enumerate().find_map(|(i, (row, _))| {
            let (rx, ry, rw, rh) = row_rect(i, rows.len(), w, h);
            (x >= rx && x < rx + rw && y >= ry && y < ry + rh).then_some(*row)
        })
    }

    /// Steps back one page, and says whether there was nowhere left to go.
    ///
    /// What the phone's back key and the desktop's Escape both mean. Without
    /// it, backing out of the manual closed the menu entirely and dropped the
    /// player into the room -- which is a long way to fall from page forty of
    /// a document they were reading.
    pub fn back(&mut self) -> bool {
        self.note = None;
        self.page = match self.page {
            Page::Root => return true,
            Page::Reading(_) => Page::Docs,
            _ => Page::Root,
        };
        self.scroll = 0;
        false
    }

    /// Takes a click. Navigation and settings are handled here; only what the
    /// loop must do comes back.
    pub fn click(&mut self, x: i32, y: i32, w: usize, h: usize) -> Action {
        if let Page::Reading(_) = self.page {
            return self.read_click(x, y, w, h);
        }
        let Some(row) = self.row_at(x, y, w, h) else { return Action::None };
        self.note = None;
        match row {
            Row::Resume => Action::Resume,
            Row::Quit => Action::Quit,
            Row::Go(page) => {
                self.page = page;
                Action::None
            }
            Row::Doc(i) => {
                self.page = Page::Reading(i);
                self.scroll = 0;
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

    /// A tap while a document is open.
    ///
    /// The three buttons, and the body of the page as a fourth: tapping the
    /// text turns forward, which is what every reader does and what a player
    /// will try before they look for a button.
    fn read_click(&mut self, x: i32, y: i32, w: usize, h: usize) -> Action {
        let step = read_rows(h).saturating_sub(2).max(1);
        let last = self.reading().map_or(0, |lines| {
            lines.len().saturating_sub(read_rows(h))
        });

        for (i, _) in READ_KEYS.iter().enumerate() {
            let (bx, by, bw, bh) = read_key_rect(i, w, h);
            if x < bx || x >= bx + bw || y < by || y >= by + bh {
                continue;
            }
            match i {
                0 => {
                    self.page = Page::Docs;
                    self.scroll = 0;
                }
                1 => self.scroll = self.scroll.saturating_sub(step),
                _ => self.scroll = (self.scroll + step).min(last),
            }
            return Action::None;
        }

        // The page itself, above the buttons.
        if y < h as i32 - READ_FOOT {
            self.scroll = (self.scroll + step).min(last);
        }
        Action::None
    }

    /// The lines of the document that is open, if one is.
    fn reading(&self) -> Option<&[String]> {
        match self.page {
            Page::Reading(i) => self.docs.get(i).map(|d| d.lines.as_slice()),
            _ => None,
        }
    }

    /// A document, filling the stage.
    ///
    /// Not the panel the other pages use: a manual set inside a 460 pixel box
    /// is eight words a line, and the whole point of this page is that it can
    /// be read.
    fn draw_reading(&self, out: &mut [u32], w: usize, h: usize, pointer: Option<(i32, i32)>) {
        fill(out, w, h, 0, 0, w as i32, h as i32, 0x000d_0b08);
        fill(out, w, h, 0, 0, w as i32, 2, 0x00c8_a55a);

        let title = self.title();
        text(out, w, h, READ_MARGIN, 18, title, 3, 0x00c8_a55a);

        let lines = self.reading().unwrap_or(&[]);
        let rows = read_rows(h);
        let shown = lines.iter().skip(self.scroll).take(rows);
        for (i, line) in shown.enumerate() {
            text(
                out,
                w,
                h,
                READ_MARGIN,
                READ_TOP + i as i32 * READ_LINE,
                line,
                READ_SCALE,
                0x00b5_a88a,
            );
        }

        // How far through, as a bar down the right hand edge rather than a
        // number: it is the thing a reader glances at, not something they
        // want to read.
        let last = lines.len().saturating_sub(rows).max(1);
        let track = h as i32 - READ_TOP - READ_FOOT;
        let thumb = (track * rows as i32 / lines.len().max(rows) as i32).max(12);
        let at = READ_TOP + (track - thumb) * self.scroll.min(last) as i32 / last as i32;
        fill(out, w, h, w as i32 - 12, READ_TOP, w as i32 - 9, READ_TOP + track, 0x0022_1c13);
        fill(out, w, h, w as i32 - 12, at, w as i32 - 9, at + thumb, 0x0087_7a5e);

        fill(out, w, h, 0, h as i32 - READ_FOOT, w as i32, h as i32 - READ_FOOT + 1, 0x0044_3a28);
        for (i, key) in READ_KEYS.iter().enumerate() {
            let (bx, by, bw, bh) = read_key_rect(i, w, h);
            let over = pointer
                .is_some_and(|(px, py)| px >= bx && px < bx + bw && py >= by && py < by + bh);
            // A button that would do nothing is drawn as one that would not.
            let dead = (i == 1 && self.scroll == 0)
                || (i == 2 && self.scroll >= lines.len().saturating_sub(rows));
            if over && !dead {
                fill(out, w, h, bx, by, bx + bw, by + bh, 0x0026_1f14);
            }
            let ink = match (over, dead) {
                (_, true) => 0x0045_3f34,
                (true, _) => 0x00ff_e9b0,
                _ => 0x0099_8f78,
            };
            text(out, w, h, bx + (bw - width(key, 2)) / 2, by + (bh - GH as i32 * 2) / 2, key, 2, ink);
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

        if matches!(self.page, Page::Reading(_)) {
            self.draw_reading(out, w, h, pointer);
            return;
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
        // No character twice: `glyph` takes the first match, so a duplicate is
        // a shape that can never be drawn and will not be noticed.
        let mut seen: Vec<char> = GLYPHS.iter().map(|(c, _)| *c).collect();
        seen.sort_unstable();
        let before = seen.len();
        seen.dedup();
        assert_eq!(before, seen.len(), "the font declares a character twice");
    }

    /// Lower case draws as lower case, and every character the documents use
    /// has a shape.
    ///
    /// The reader shows the disc's own prose, which is mixed case and full of
    /// punctuation. Before it existed `glyph` folded to upper case on the way
    /// in, so adding the shapes without changing that would have drawn a
    /// manual in capitals anyway and looked like the shapes were missing.
    #[test]
    fn the_documents_are_drawable_in_the_case_they_were_written() {
        assert_ne!(
            glyph('a'),
            glyph('A'),
            "lower case is still being folded to upper"
        );
        // What the three files actually contain, near enough: prose, the
        // punctuation a word processor emits, and the odd number.
        let sample = "AMBER: Journeys Beyond -- \"patience is a virtue.\"                       (The values are 6, 5, and 8.) 100% of it? Yes; see MANUAL.WRI.";
        for c in sample.chars() {
            assert!(glyph(c).is_some(), "no glyph for {c:?}");
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

#[cfg(test)]
mod reader_tests {
    use super::*;

    fn paged(lines: usize) -> Menu {
        let doc = crate::docs::Doc {
            title: "HINTS".into(),
            lines: (0..lines).map(|i| format!("line {i}")).collect(),
        };
        let mut menu = Menu::default().with_docs(vec![doc]);
        menu.page = Page::Reading(0);
        menu
    }

    const W: usize = 640;
    const H: usize = 480;

    /// The documents are only offered when the disc had them.
    #[test]
    fn a_disc_without_documents_offers_no_reading() {
        let bare = Menu::default();
        assert!(
            !bare.rows().iter().any(|(r, _)| matches!(r, Row::Go(Page::Docs))),
            "offered a reading page with nothing to read"
        );
        let stocked = paged(10);
        let mut root = Menu::default().with_docs(stocked.docs);
        root.page = Page::Root;
        assert!(root.rows().iter().any(|(r, _)| matches!(r, Row::Go(Page::Docs))));
    }

    /// Scrolling stops at both ends rather than running off.
    ///
    /// The bottom stop is the one that matters: past it the page is blank and
    /// the reader looks broken, with no way to tell that the document ended
    /// twenty taps ago.
    #[test]
    fn the_page_turns_and_stops() {
        let mut menu = paged(200);
        let body = (W as i32 / 2, 100);

        assert_eq!(menu.scroll, 0);
        menu.click(body.0, body.1, W, H);
        let first = menu.scroll;
        assert!(first > 0, "tapping the page did not turn it");

        // Far past the end.
        for _ in 0..500 {
            menu.click(body.0, body.1, W, H);
        }
        let rows = read_rows(H);
        assert_eq!(menu.scroll, 200 - rows, "ran past the last line");

        // And back to the top.
        let (ux, uy, uw, uh) = read_key_rect(1, W, H);
        for _ in 0..500 {
            menu.click(ux + uw / 2, uy + uh / 2, W, H);
        }
        assert_eq!(menu.scroll, 0, "ran past the first line");
    }

    /// A document shorter than the page does not scroll at all.
    #[test]
    fn a_short_document_does_not_move() {
        let mut menu = paged(4);
        menu.click(W as i32 / 2, 100, W, H);
        assert_eq!(menu.scroll, 0);
    }

    /// BACK leaves the document, and back-out is one page at a time.
    #[test]
    fn backing_out_goes_one_page_at_a_time() {
        let mut menu = paged(200);
        menu.scroll = 40;
        let (bx, by, bw, bh) = read_key_rect(0, W, H);
        menu.click(bx + bw / 2, by + bh / 2, W, H);
        assert_eq!(menu.page, Page::Docs, "BACK did not leave the document");
        assert_eq!(menu.scroll, 0, "the place was kept after leaving");

        let mut menu = paged(200);
        menu.scroll = 40;
        assert!(!menu.back(), "the back key closed the menu from inside a document");
        assert_eq!(menu.page, Page::Docs);
        assert!(!menu.back());
        assert_eq!(menu.page, Page::Root);
        assert!(menu.back(), "the back key did not close the menu at the root");
    }

    /// Every line of every document on the disc is drawable, and fits.
    ///
    /// The wrap and the drawing agree on `READ_COLUMNS` or the text runs off
    /// the edge, and a character with no shape comes out as a hole rather
    /// than as any kind of error -- so both are checked against the real
    /// prose rather than against a sample.
    #[test]
    fn the_discs_own_prose_fits_and_draws() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../extract");
        let Ok(content) = crate::iso::open(&root) else { return };
        let docs = crate::docs::load(content.as_ref(), READ_COLUMNS);
        assert!(!docs.is_empty(), "the disc carries no documents");

        // From the left margin to the near side of the scroll bar. The right
        // margin is where the text stops, not a second inset.
        let room = W as i32 - READ_MARGIN - 16;
        for doc in &docs {
            for line in &doc.lines {
                assert!(
                    width(line, READ_SCALE) <= room,
                    "{} runs off the page: {line:?}",
                    doc.title
                );
                for c in line.chars() {
                    assert!(glyph(c).is_some(), "no glyph for {c:?} in {}: {line:?}", doc.title);
                }
            }
        }
    }
}

#[cfg(test)]
mod reader_eyeball {
    use super::*;

    fn shot(menu: &Menu, name: &str) {
        const W: usize = 640;
        const H: usize = 480;
        let mut frame = vec![0x0018_1410u32; W * H];
        menu.draw(&mut frame, W, H, None);
        let rgba: Vec<u8> = frame
            .iter()
            .flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8, 0xff])
            .collect();
        let out = std::path::Path::new(
            &std::env::var("AMBER_SHOTS").unwrap_or_else(|_| "/tmp".into()),
        )
        .join(name);
        crate::write_png(&out, W as u32, H as u32, &rgba).expect("write");
        println!("wrote {}", out.display());
    }

    /// Renders the menu pages so they can be looked at.
    ///
    /// `AMBER_SHOTS=/somewhere cargo test -p amber --release -- --ignored
    /// --nocapture reader_eyeball`. There is no assertion worth writing about
    /// whether a page looks right.
    /// Prints a line of text as characters, to check a glyph without a PNG.
    #[test]
    #[ignore]
    fn letters() {
        const W: usize = 200;
        const H: usize = 20;
        let mut frame = vec![0u32; W * H];
        text(&mut frame, W, H, 2, 2, "agpqyj Ag.", 2, 0x00ff_ffff);
        for y in 0..H {
            let row: String = (0..W)
                .map(|x| if frame[y * W + x] != 0 { '#' } else { '.' })
                .collect();
            println!("{row}");
        }
    }

    #[test]
    #[ignore]
    fn pages() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../extract");
        let Ok(content) = crate::iso::open(&root) else { return };
        let docs = crate::docs::load(content.as_ref(), READ_COLUMNS);

        let mut menu = Menu::new(Settings::default(), [Some("ROXY HALL".into()), None, None])
            .with_docs(docs);
        shot(&menu, "menu-root.png");
        menu.page = Page::Docs;
        shot(&menu, "menu-docs.png");
        menu.page = Page::Reading(0);
        shot(&menu, "menu-hints.png");
        menu.scroll = 40;
        shot(&menu, "menu-hints-40.png");
        menu.page = Page::Reading(1);
        menu.scroll = 0;
        shot(&menu, "menu-manual.png");
    }
}

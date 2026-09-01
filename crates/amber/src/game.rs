//! The runtime: holds the player's position and progress, resolves what the
//! current room should look like, and turns clicks into moves.
//!
//! Rendering and input live in `render`; this module is the part that would be
//! identical whatever the front end is.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use director::{Bitmap, Movie, Palette};
use lingo::Rect;

use crate::media::MovieIndex;
use crate::player::VideoPlayer;
use crate::schema::Schema;
use crate::script::{self, Effect, Outcome};
use crate::state::State;
use crate::world::{Channel, Node, Verb, World};

/// A decoded image ready to blit, cached so a room revisit costs nothing.
struct CachedArt {
    rgba: Vec<u8>,
    width: u32,
    height: u32,
    reg_x: i16,
    reg_y: i16,
}

/// One chapter's movie, opened lazily because they are large.
struct Chapter {
    movie: Movie,
    palettes: Vec<Palette>,
    art: HashMap<u32, Option<CachedArt>>,
    schema: Option<Schema>,
}

pub struct Game {
    pub world: World,
    pub state: State,
    /// Index into `world.nodes`.
    pub room: usize,
    /// Where the player came from, for `goBack`.
    history: Vec<usize>,
    root: PathBuf,
    chapters: HashMap<String, Chapter>,
    /// Effects the last click produced, for the front end to play back.
    pub pending: Vec<Effect>,
    movies: MovieIndex,
    /// The movie currently on screen, if the room has one.
    pub player: Option<VideoPlayer>,
}

impl Game {
    /// The chapter the game opens in. Roxy's is the present-day frame story
    /// that the other three are reached from.
    const FIRST_CHAPTER: &'static str = "ROXY";

    pub fn new(root: &Path) -> std::io::Result<Game> {
        let world = World::load(root)?;
        let mut game = Game {
            world,
            state: State::new(),
            room: 0,
            history: Vec::new(),
            root: root.to_path_buf(),
            chapters: HashMap::new(),
            pending: Vec::new(),
            movies: MovieIndex::build(root),
            player: None,
        };
        game.enter_chapter(Self::FIRST_CHAPTER);
        game.start_room_video();
        Ok(game)
    }

    /// Seeds state from a chapter's declared schema and moves to the room that
    /// schema names as its starting point.
    ///
    /// Both matter for a correct first frame: without the seed, guards run
    /// against an empty store and sprites that test a flag against its initial
    /// value stay hidden. `DEFAULT_LOCATION` is deliberately not used as a
    /// fallback start; it is an empty placeholder room with no art at all.
    pub fn enter_chapter(&mut self, domain: &str) {
        let start = self.chapter(domain).and_then(|c| {
            let schema = c.schema.as_ref()?;
            Some((schema.start_location()?.to_string(), ()))
        });

        if let Some(chapter) = self.chapters.get(domain) {
            if let Some(schema) = &chapter.schema {
                schema.seed(&mut self.state);
            }
        }

        let target = start
            .and_then(|(name, _)| self.world.resolve(&name, Some(domain)))
            .or_else(|| self.first_room_with_art(domain))
            .or_else(|| self.world.domains.get(domain).map(|(s, _)| *s));
        if let Some(t) = target {
            self.room = t;
        }
    }

    /// The first room anywhere that draws something, used to get past a
    /// video-only opening.
    pub fn first_playable(&self) -> Option<usize> {
        let domain = self.node().domain.clone();
        self.first_room_with_art(&domain)
            .or_else(|| (0..self.world.nodes.len()).find(|&i| {
                self.world.nodes[i]
                    .sprites
                    .iter()
                    .any(|s| matches!(s.channel, Channel::Sprite(_)) && s.cast_number > 0)
            }))
    }

    /// Falls back to the first room of a chapter that actually draws something,
    /// so a missing or unreadable schema still opens on a visible frame.
    fn first_room_with_art(&self, domain: &str) -> Option<usize> {
        let &(start, end) = self.world.domains.get(domain)?;
        (start..end).find(|&i| {
            self.world.nodes[i]
                .sprites
                .iter()
                .any(|s| matches!(s.channel, Channel::Sprite(_)) && s.cast_number > 0)
        })
    }

    pub fn node(&self) -> &Node {
        &self.world.nodes[self.room]
    }

    fn chapter(&mut self, domain: &str) -> Option<&mut Chapter> {
        if !self.chapters.contains_key(domain) {
            let path = self.root.join(domain).join(format!("{domain}.DXR"));
            let movie = Movie::open(path).ok()?;
            let palettes = movie.palettes();
            let schema = Schema::from_texts(&movie.texts());
            self.chapters.insert(
                domain.to_string(),
                Chapter {
                    movie,
                    palettes,
                    art: HashMap::new(),
                    schema,
                },
            );
        }
        self.chapters.get_mut(domain)
    }

    /// Decodes a cast member to RGBA, caching both hits and misses so a missing
    /// member is not re-decoded every frame.
    fn art(&mut self, domain: &str, cast: u32) -> Option<&CachedArt> {
        let chapter = self.chapter(domain)?;
        if !chapter.art.contains_key(&cast) {
            let decoded = chapter.movie.bitmap(cast).ok().map(|b: Bitmap| {
                // The member names the palette cast it was authored against.
                let palette = chapter
                    .movie
                    .palette_for_cast(b.palette_ref)
                    .or_else(|| chapter.palettes.first().cloned())
                    .unwrap_or_default();
                CachedArt {
                    rgba: b.to_rgba(&palette, None),
                    width: b.width as u32,
                    height: b.height as u32,
                    reg_x: b.reg_x,
                    reg_y: b.reg_y,
                }
            });
            chapter.art.insert(cast, decoded);
        }
        chapter.art.get(&cast).and_then(Option::as_ref)
    }

    /// The movie a room wants to play, from its `#video` channel element.
    ///
    /// Rooms that consist only of a movie are common at chapter boundaries: the
    /// intro, the montages and the endings all place a single element on this
    /// channel and nothing on the sprite channels.
    pub fn video(&self) -> Option<&str> {
        self.node()
            .sprites
            .iter()
            .find(|s| matches!(s.channel, Channel::Video))
            .filter(|s| self.state.test(&s.condition))
            .and_then(|s| s.cast_name.as_deref())
    }

    /// True when a room places nothing on the sprite channels. Such rooms are
    /// not blank: they are the ones carried entirely by their movie.
    pub fn draws_nothing(&self) -> bool {
        self.visible().is_empty()
    }

    /// Loads and starts the current room's movie, if it has one.
    pub fn start_room_video(&mut self) {
        self.player = None;
        let Some(name) = self.video().map(str::to_owned) else {
            return;
        };
        match self.movies.find(&name) {
            Some(path) => self.player = VideoPlayer::open(path),
            None => eprintln!("warning: no file for movie {name}"),
        }
    }

    /// The stage elements that should currently draw, back to front.
    pub fn visible(&self) -> Vec<(u8, u32, Option<(i32, i32)>)> {
        let mut out: Vec<(u8, u32, Option<(i32, i32)>)> = self
            .node()
            .sprites
            .iter()
            .filter(|s| matches!(s.channel, Channel::Sprite(_)))
            .filter(|s| s.cast_number > 0)
            .filter(|s| self.state.test(&s.condition))
            .map(|s| {
                let ch = match s.channel {
                    Channel::Sprite(n) => n,
                    _ => 0,
                };
                (ch, s.cast_number, s.center)
            })
            .collect();
        out.sort_by_key(|(ch, _, _)| *ch);
        out
    }

    /// Draws the current room into a 640x480 BGRA framebuffer.
    pub fn draw(&mut self, frame: &mut [u32], width: u32, height: u32) {
        frame.fill(0xff00_0000);

        // The movie sits behind the sprite channels, which is how the game
        // frames video inside static scenery.
        if let Some(player) = &self.player {
            let centre = self
                .world
                .nodes[self.room]
                .sprites
                .iter()
                .find(|s| matches!(s.channel, Channel::Video))
                .and_then(|s| s.center)
                .unwrap_or((width as i32 / 2, height as i32 / 2));
            let (w, h) = (player.width as u32, player.height as u32);
            blit(
                frame,
                width,
                height,
                player.frame(),
                w,
                h,
                centre.0 - w as i32 / 2,
                centre.1 - h as i32 / 2,
            );
        }

        let domain = self.node().domain.clone();
        for (_, cast, center) in self.visible() {
            let Some(art) = self.art(&domain, cast) else {
                continue;
            };
            // Director positions a sprite by its registration point, which
            // defaults to the image centre. `#coords` gives where that point
            // lands on the stage.
            let (cx, cy) = center.unwrap_or((width as i32 / 2, height as i32 / 2));
            let ox = cx - if art.reg_x != 0 {
                art.reg_x as i32
            } else {
                art.width as i32 / 2
            };
            let oy = cy - if art.reg_y != 0 {
                art.reg_y as i32
            } else {
                art.height as i32 / 2
            };
            blit(frame, width, height, &art.rgba, art.width, art.height, ox, oy);
        }
    }

    /// Whether a cast member in the current room's chapter decodes to art.
    pub fn has_art(&mut self, cast: u32) -> bool {
        let domain = self.node().domain.clone();
        self.art(&domain, cast).is_some()
    }

    /// The hotspot under the cursor, if any.
    pub fn hotspot_at(&self, x: i32, y: i32) -> Option<(Verb, Rect)> {
        let state = &self.state;
        self.node()
            .hit_test(x, y, |c| state.test(c))
            .map(|h| (h.verb, h.bounds))
    }

    /// Handles a click, moving the player if the hotspot says to.
    pub fn click(&mut self, x: i32, y: i32) -> Option<Outcome> {
        let actions = {
            let state = &self.state;
            self.node()
                .hit_test(x, y, |c| state.test(c))?
                .actions
                .clone()
        };
        let outcome = script::run(&actions, &mut self.state);
        self.apply(&outcome);
        Some(outcome)
    }

    fn apply(&mut self, outcome: &Outcome) {
        if outcome.go_back {
            if let Some(prev) = self.history.pop() {
                self.room = prev;
            }
        } else if let Some(dest) = &outcome.destination {
            let from = self.node().domain.clone();
            if let Some(next) = self.world.resolve(dest, Some(&from)) {
                if next != self.room {
                    self.history.push(self.room);
                    // The history only needs to reach back far enough for the
                    // `goBack` the scripts use, which is always one step.
                    if self.history.len() > 64 {
                        self.history.remove(0);
                    }
                }
                self.room = next;
            }
        }
        // A move changes which movie is on screen, so reload it either way.
        if outcome.destination.is_some() || outcome.go_back {
            self.start_room_video();
        }
        self.pending.extend(outcome.effects.iter().cloned());
    }
}

/// Blits RGBA source pixels onto a BGRA framebuffer, clipped to its bounds.
fn blit(
    dst: &mut [u32],
    dst_w: u32,
    dst_h: u32,
    src: &[u8],
    src_w: u32,
    src_h: u32,
    ox: i32,
    oy: i32,
) {
    for y in 0..src_h as i32 {
        let ty = y + oy;
        if ty < 0 || ty >= dst_h as i32 {
            continue;
        }
        for x in 0..src_w as i32 {
            let tx = x + ox;
            if tx < 0 || tx >= dst_w as i32 {
                continue;
            }
            let s = ((y as u32 * src_w + x as u32) * 4) as usize;
            if src[s + 3] == 0 {
                continue;
            }
            let (r, g, b) = (src[s] as u32, src[s + 1] as u32, src[s + 2] as u32);
            dst[(ty as u32 * dst_w + tx as u32) as usize] = 0xff00_0000 | (r << 16) | (g << 8) | b;
        }
    }
}

//! The runtime: holds the player's position and progress, resolves what the
//! current room should look like, and turns clicks into moves.
//!
//! Rendering and input live in `render`; this module is the part that would be
//! identical whatever the front end is.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use director::{Bitmap, Movie, Palette};
use lingo::Rect;

use crate::inventory::Inventory;
use crate::media::MovieIndex;
use crate::presentation::Presentation;
use crate::player::VideoPlayer;
use crate::schema::Schema;
use crate::sound::{self, SoundBank, Source};
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
    /// Sprite channels a script has taken over, keyed by channel so they
    /// composite in the same back-to-front order as the room's own sprites.
    puppets: BTreeMap<u8, Puppet>,
    /// Actions still to run from the current hotspot, and what they are
    /// waiting on. In-world animation depends on this: a switch sets a flag,
    /// redraws so the movie appears, waits for it to finish, then clears the
    /// flag. Running the list straight through does all of that inside one
    /// frame and nothing is ever seen.
    script: Vec<String>,
    waiting: Option<Wait>,
    movies: MovieIndex,
    pub sounds: SoundBank,
    pub inventory: Inventory,
    /// Per chapter, the cast members its handlers name rather than number.
    presentation: HashMap<String, Presentation>,
    /// The radio or clock programme currently running, if any.
    program: Option<Program>,
    /// Decoded sounds, keyed by symbol. Effects fire repeatedly, so decoding
    /// each one once matters more here than it does for the movies.
    pcm_cache: HashMap<String, Option<Arc<Vec<i16>>>>,
    pcm_meta: HashMap<String, (u32, u16)>,
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
            puppets: BTreeMap::new(),
            script: Vec::new(),
            waiting: None,
            movies: MovieIndex::build(root),
            sounds: SoundBank::new(root),
            inventory: Inventory::from_texts(&[]),
            presentation: HashMap::new(),
            program: None,
            pcm_cache: HashMap::new(),
            pcm_meta: HashMap::new(),
            player: None,
        };
        // The symbol tables live in the chapter movies, so every chapter has to
        // be opened once to collect them.
        for domain in world_domains(&game.world) {
            if let Some(chapter) = game.chapter(&domain) {
                let texts = chapter.movie.texts();
                game.sounds.add_tables(&texts);
                // The icon table is the same in every chapter; take the first
                // that yields one.
                if game.inventory.is_empty() {
                    game.inventory = Inventory::from_texts(&texts);
                }
                game.presentation
                    .insert(domain.clone(), Presentation::from_texts(&texts));
            }
        }
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
                schema.seed(&mut self.state, &self.world.list_flags);
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

    /// Stops the current movie and moves on if the room has nothing else.
    ///
    /// Rooms carried entirely by a movie have no live exit to click, because
    /// the original advances them from script when the movie ends. Skipping
    /// therefore has to supply the destination: the opening leads to the
    /// chapter's game entry, and anything else falls back to the first room
    /// that draws, so a skip never strands the player on a blank screen.
    pub fn skip_video(&mut self) -> bool {
        if self.player.is_none() {
            return false;
        }
        self.player = None;
        if !self.draws_nothing() {
            return true;
        }
        let from = self.node().domain.clone();
        let onward = self
            .world
            .resolve("Gbhs_gameEntry", Some(&from))
            .filter(|_| self.node().name.as_deref() == Some("Gbhs_playIntro"))
            .or_else(|| self.first_playable());
        if let Some(i) = onward {
            self.room = i;
            self.start_room_video();
        }
        true
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

    /// Applies an effect that acts on a script-controlled sprite channel.
    ///
    /// Director lets a script take a channel away from the score with
    /// `puppetSprite` and then drive it directly. The room's own `#onStage`
    /// list cannot express that, so these channels are held separately and
    /// composited over the room.
    pub fn apply_puppet(&mut self, effect: &Effect) -> bool {
        match effect {
            // Deferred state writes, so they land in timeline order rather
            // than when the handler ran.
            Effect::SetState { key, value } => {
                self.state.set(key, value.clone());
            }
            Effect::TrimState { key, item } => {
                self.state.trim_item(key, item);
            }
            Effect::PuppetSprite { channel, on } => {
                if *on {
                    self.puppets.entry(*channel).or_default();
                } else {
                    self.puppets.remove(channel);
                }
            }
            Effect::SpriteCast { channel, cast } => {
                self.puppets.entry(*channel).or_default().cast = *cast;
            }
            Effect::SpriteLoc { channel, x, y } => {
                let p = self.puppets.entry(*channel).or_default();
                p.loc = Some((*x, *y));
            }
            Effect::SpriteCastNamed { channel, name } => {
                if let Some(cast) = self.presentation_cast(name) {
                    self.puppets.entry(*channel).or_default().cast = cast;
                }
            }
            Effect::SpriteCastIcon { channel, item, index } => {
                if let Some(cast) = self.inventory.icon_at(item, *index) {
                    self.puppets.entry(*channel).or_default().cast = cast;
                }
            }
            Effect::SpriteVisible { channel, visible } => {
                self.puppets.entry(*channel).or_default().hidden = !*visible;
            }
            _ => return false,
        }
        true
    }

    /// Releases every claimed channel, which a room change does.
    pub fn clear_puppets(&mut self) {
        self.puppets.clear();
    }

    pub fn puppet_count(&self) -> usize {
        self.puppets.len()
    }

    /// Draws the current room into a 640x480 BGRA framebuffer.
    pub fn draw(&mut self, frame: &mut [u32], width: u32, height: u32) {
        frame.fill(0xff00_0000);

        let domain = self.node().domain.clone();
        for (_, cast, center) in self.visible() {
            let Some(art) = self.art(&domain, cast) else {
                continue;
            };
            // `#coords` gives where the sprite's registration point lands on
            // the stage. Without one there is no anchor and the registration
            // point alone says nothing, so the image is centred instead.
            let (ox, oy) = match center {
                Some((cx, cy)) => (
                    cx - if art.reg_x != 0 { art.reg_x as i32 } else { art.width as i32 / 2 },
                    cy - if art.reg_y != 0 { art.reg_y as i32 } else { art.height as i32 / 2 },
                ),
                None => (
                    (width as i32 - art.width as i32) / 2,
                    (height as i32 - art.height as i32) / 2,
                ),
            };
            if std::env::var_os("AMBER_TRACE_SPRITES").is_some() {
                eprintln!(
                    "  sprite cast {cast:<6} {}x{} reg=({},{}) coords={:?} -> ({ox},{oy})",
                    art.width, art.height, art.reg_x, art.reg_y, center
                );
            }
            blit(frame, width, height, &art.rgba, art.width, art.height, ox, oy);
        }

        // The movie draws over the room's plates, not under them. A haunt is
        // a film of something appearing in a mirror or out on the lake, and
        // the room it plays in is a full-scene plate; underneath, it is
        // invisible. The intro, where this was first written, has no plates at
        // all, so the order it needed could not be observed there.
        if let Some(player) = &self.player {
            let centre = self
                .world
                .nodes[self.room]
                .sprites
                .iter()
                .find(|s| matches!(s.channel, Channel::Video))
                .and_then(|s| s.center)
                .unwrap_or((width as i32 / 2, height as i32 / 2));
            // The decoder is authoritative: a frame header can disagree with
            // the container, and it is the decoder that resized its buffer.
            let (w, h) = player.frame_size();
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

        // Script-controlled channels sit over the room, in channel order.
        let domain = self.node().domain.clone();
        let puppets: Vec<(u8, Puppet)> =
            self.puppets.iter().map(|(k, v)| (*k, *v)).collect();
        for (_, puppet) in puppets {
            if puppet.cast == 0 || puppet.hidden {
                continue;
            }
            let Some(art) = self.art(&domain, puppet.cast) else {
                continue;
            };
            let (w, h) = (art.width, art.height);
            let (ox, oy) = match puppet.loc {
                Some((x, y)) => (x - art.reg_x as i32, y - art.reg_y as i32),
                None => ((width as i32 - w as i32) / 2, (height as i32 - h as i32) / 2),
            };
            blit(frame, width, height, &art.rgba, w, h, ox, oy);
        }
    }

    /// Starts a radio or clock programme, if `group` names one.
    ///
    /// A programme is a group of takes plus an order to play them in, so it
    /// cannot be handed to the mixer as a single looping voice: each item has
    /// to be queued as its predecessor ends.
    pub fn start_program(&mut self, group: &str, gain: f32) -> bool {
        // A programme is declared by the chapter that uses it, so the current
        // one is checked first; the search widens because a room can name a
        // programme that belongs to another chapter's schema.
        let domain = self.node().domain.clone();
        let mut order = self
            .chapter(&domain)
            .and_then(|c| c.schema.as_ref())
            .and_then(|s| s.playlist(group));
        if order.is_none() {
            for other in world_domains(&self.world) {
                if other == domain {
                    continue;
                }
                order = self
                    .chapter(&other)
                    .and_then(|c| c.schema.as_ref())
                    .and_then(|s| s.playlist(group));
                if order.is_some() {
                    break;
                }
            }
        }
        let Some(order) = order else { return false };
        if order.is_empty() || !self.sounds.is_group(group) {
            return false;
        }
        self.program = Some(Program {
            group: group.to_string(),
            order,
            next: 0,
            due: Instant::now(),
            gain,
            misses: 0,
        });
        true
    }

    pub fn stop_program(&mut self, group: &str) {
        if self
            .program
            .as_ref()
            .is_some_and(|p| p.group.eq_ignore_ascii_case(group))
        {
            self.program = None;
        }
    }

    /// Brings the next item forward, so a programme can be stepped through
    /// without waiting out each take. Diagnostics only.
    pub fn force_program_step(&mut self) {
        if let Some(p) = self.program.as_mut() {
            p.due = Instant::now();
        }
    }

    /// The item index the programme will play next, for diagnostics.
    pub fn program_position(&self) -> Option<(usize, usize)> {
        let p = self.program.as_ref()?;
        Some((p.next % p.order.len(), p.order.len()))
    }

    pub fn program_running(&self) -> Option<&str> {
        self.program.as_ref().map(|p| p.group.as_str())
    }

    /// Returns the next item to play when the current one has run its course.
    ///
    /// The caller plays it and the programme schedules the following item from
    /// the length of what was just handed over, which keeps the sequence
    /// running without the mixer having to report completions.
    pub fn tick_program(&mut self) -> Option<(Arc<Vec<i16>>, u32, u16, f32)> {
        let program = self.program.as_ref()?;
        if Instant::now() < program.due {
            return None;
        }
        let (group, item, gain) = {
            let p = self.program.as_mut()?;
            let item = p.order[p.next % p.order.len()].clone();
            p.next = p.next.wrapping_add(1);
            (p.group.clone(), item, p.gain)
        };

        // Advance the clock before anything can fail. An item that does not
        // resolve must still cost a beat: returning early without setting
        // `due` leaves the programme due again on the very next frame, so it
        // races through the running order at frame rate and re-fires every
        // item that does resolve, many times a second.
        let pcm = self.group_sound(&group, &item);
        let seconds = match &pcm {
            Some((samples, rate, channels)) => {
                samples.len() as f64 / ((*rate).max(1) as f64 * (*channels).max(1) as f64)
            }
            None => 0.0,
        };
        if let Some(p) = self.program.as_mut() {
            // A missing item waits a short beat rather than no time at all.
            p.due = Instant::now() + Duration::from_secs_f64(seconds.max(0.25));
            p.misses = if pcm.is_some() { 0 } else { p.misses + 1 };
            // A programme whose items all fail would otherwise poll for ever.
            if p.misses > p.order.len() {
                self.program = None;
                return None;
            }
        }
        let (samples, rate, channels) = pcm?;
        Some((samples, rate, channels, gain))
    }

    /// Decodes one item of a sound group, for callers outside this module.
    pub fn group_sound_public(
        &mut self,
        group: &str,
        item: &str,
    ) -> Option<(Arc<Vec<i16>>, u32, u16)> {
        self.group_sound(group, item)
    }

    /// Decodes one item of a sound group, cached like any other sound.
    fn group_sound(&mut self, group: &str, item: &str) -> Option<(Arc<Vec<i16>>, u32, u16)> {
        let key = format!(
            "{}::{}",
            group.to_ascii_lowercase(),
            item.trim_start_matches('#').to_ascii_lowercase()
        );
        if !self.pcm_cache.contains_key(&key) {
            let decoded = match self.sounds.source_in(group, item)?.clone() {
                Source::Files(takes) => {
                    let name = takes.first()?;
                    let path = self.sounds.file(name)?.to_path_buf();
                    sound::load(&path)
                }
                Source::Cast(number) => {
                    let domain = self.node().domain.clone();
                    let chapter = self.chapter(&domain)?;
                    chapter.movie.sound(number).ok().map(|s| sound::Pcm {
                        samples: s.samples,
                        rate: s.sample_rate,
                        channels: s.channels,
                    })
                }
            };
            let (pcm, meta) = match decoded {
                Some(p) => {
                    let meta = (p.rate, p.channels);
                    (Some(Arc::new(p.samples)), meta)
                }
                None => (None, (22050, 1)),
            };
            self.pcm_cache.insert(key.clone(), pcm);
            self.pcm_meta.insert(key.clone(), meta);
        }
        let samples = self.pcm_cache.get(&key)?.clone()?;
        let &(rate, channels) = self.pcm_meta.get(&key)?;
        Some((samples, rate, channels))
    }

    /// Decodes a named sound, from a file on the disc or a `snd ` cast member,
    /// and caches the result. Returns the samples with their rate and channel
    /// count.
    pub fn sound(&mut self, symbol: &str) -> Option<(Arc<Vec<i16>>, u32, u16)> {
        let key = symbol.trim_start_matches('#').to_ascii_lowercase();
        if !self.pcm_cache.contains_key(&key) {
            let decoded = self.decode_sound(&key);
            let (pcm, meta) = match decoded {
                Some(p) => {
                    let meta = (p.rate, p.channels);
                    (Some(Arc::new(p.samples)), meta)
                }
                None => (None, (22050, 1)),
            };
            self.pcm_cache.insert(key.clone(), pcm);
            self.pcm_meta.insert(key.clone(), meta);
        }
        let samples = self.pcm_cache.get(&key)?.clone()?;
        let &(rate, channels) = self.pcm_meta.get(&key)?;
        Some((samples, rate, channels))
    }

    fn decode_sound(&mut self, key: &str) -> Option<sound::Pcm> {
        // A name the bank does not carry is tried as a filename. The ghost
        // calls are external files named by convention, Bcall1 through
        // Ecall12, rather than symbols in the bank, and this is how they are
        // reached.
        if self.sounds.source(key).is_none() {
            let path = self.sounds.file(key)?.to_path_buf();
            return sound::load(&path);
        }
        match self.sounds.source(key)?.clone() {
            // Several takes of the same sound; the game varies between them,
            // and rotating by a cheap hash keeps that without needing a RNG.
            Source::Files(takes) => {
                let pick = takes.len().min(1 + (self.room % takes.len().max(1)));
                let name = takes.get(pick - 1).or_else(|| takes.first())?;
                let path = self.sounds.file(name)?.to_path_buf();
                sound::load(&path)
            }
            Source::Cast(number) => {
                let domain = self.node().domain.clone();
                let chapter = self.chapter(&domain)?;
                let s = chapter.movie.sound(number).ok()?;
                Some(sound::Pcm {
                    samples: s.samples,
                    rate: s.sample_rate,
                    channels: s.channels,
                })
            }
        }
    }

    /// The ambient loops the current room asks for, as `(symbol, gain)`.
    ///
    /// A room declares its mix as `#earShot: [#houseHum: 224, ...]`, levels out
    /// of 255, and separately places named loops on the `#sound` channel.
    pub fn ambience(&self) -> Vec<(String, f32)> {
        let node = self.node();
        let mut out: Vec<(String, f32)> = node
            .sprites
            .iter()
            .filter(|s| matches!(s.channel, Channel::Sound))
            .filter(|s| self.state.test(&s.condition))
            .filter_map(|s| {
                let name = s.cast_name.as_ref()?.trim_start_matches('#').to_string();
                let level = s.volume.unwrap_or(255) as f32 / 255.0;
                Some((name, level))
            })
            .collect();
        for (key, level) in &node.ambience {
            // The mix keys are the loop names with a volume suffix; the house
            // hum is the one that is always present.
            if key == "househum" && *level > 0 {
                out.push(("houseHum".into(), *level as f32 / 255.0));
            }
        }
        out
    }

    /// A cast member the current chapter names, as the original reads it off
    /// the puppeteer with `getProp(oPuppeteer, #name)`.
    pub fn presentation_cast(&self, name: &str) -> Option<u32> {
        self.presentation
            .get(&self.node().domain)
            .and_then(|p| p.cast(name))
    }

    /// Whether a cast member in the current room's chapter decodes to art.
    pub fn has_art(&mut self, cast: u32) -> bool {
        let domain = self.node().domain.clone();
        self.art(&domain, cast).is_some()
    }

    /// Draws the inventory bar over the stage.
    ///
    /// The item in hand is drawn lit, which is how the player can tell what a
    /// click on the scene will be carrying.
    pub fn draw_inventory(&mut self, frame: &mut [u32], width: u32, height: u32) {
        let held: Vec<String> = self.state.inventory().to_vec();
        let in_use = self.state.item_in_use().map(str::to_ascii_lowercase);
        let placed = self
            .inventory
            .layout(&held, width as i32, height as i32);
        let domain = self.node().domain.clone();

        for (item, x, y) in placed {
            let Some(icons) = self.inventory.icons(&item) else { continue };
            let lit = in_use.as_deref() == Some(item.to_ascii_lowercase().as_str());
            let cast = if lit { icons.lit } else { icons.plain };
            let Some(art) = self.art(&domain, cast) else { continue };
            let (w, h) = (art.width, art.height);
            blit(frame, width, height, &art.rgba, w, h, x, y);
        }
    }

    /// Handles a click on the inventory bar, returning true if it was one.
    ///
    /// Clicking an item takes it in hand; clicking the item already in hand
    /// puts it back, which is what `stowInventory` does from script.
    pub fn click_inventory(&mut self, x: i32, y: i32, width: i32, height: i32) -> bool {
        let held: Vec<String> = self.state.inventory().to_vec();
        let Some(item) = self.inventory.hit(&held, width, height, x, y) else {
            return false;
        };
        let already = self
            .state
            .item_in_use()
            .is_some_and(|c| c.eq_ignore_ascii_case(&item));
        if already {
            self.state.stow();
        } else {
            self.state.stow();
            self.state.set("itemInUse", lingo::Value::Symbol(item));
        }
        true
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
        // Handlers such as `stashClick` want the click position, which the
        // scripts read from the mouse rather than being passed.
        self.state.set("gMouseLoc", lingo::Value::Point(x, y));
        let actions = {
            let state = &self.state;
            self.node()
                .hit_test(x, y, |c| state.test(c))?
                .actions
                .clone()
        };
        // A click abandons whatever the previous one was still waiting on.
        self.script = actions;
        self.waiting = None;
        Some(self.pump())
    }

    /// True while a script is part-way through, so the caller keeps pumping.
    pub fn script_running(&self) -> bool {
        !self.script.is_empty() || self.waiting.is_some()
    }

    /// Runs queued actions until one asks to wait, or the queue empties.
    ///
    /// Returns the outcome of everything that ran this call, so a move or a
    /// redraw part-way through a sequence still reaches the caller.
    pub fn pump(&mut self) -> Outcome {
        let mut combined = Outcome::default();

        if let Some(wait) = &self.waiting {
            if !self.wait_satisfied(wait) {
                return combined;
            }
            self.waiting = None;
        }

        while !self.script.is_empty() {
            let action = self.script.remove(0);
            let outcome = script::run(std::slice::from_ref(&action), &mut self.state);
            let hold = outcome.effects.iter().find_map(|e| match e {
                Effect::WaitTicks(t) => Some(Wait::Until(
                    Instant::now() + Duration::from_secs_f64(*t as f64 / 60.0),
                )),
                Effect::WaitForVideo => Some(Wait::Video),
                Effect::WaitForSound(_) => Some(Wait::Until(
                    Instant::now() + Duration::from_millis(250),
                )),
                _ => None,
            });
            merge(&mut combined, outcome);
            self.apply(&combined.clone());
            if let Some(w) = hold {
                self.waiting = Some(w);
                break;
            }
        }
        combined
    }

    fn wait_satisfied(&self, wait: &Wait) -> bool {
        match wait {
            Wait::Until(t) => Instant::now() >= *t,
            // A room with no movie has nothing to wait for; treating that as
            // satisfied stops a missing video from stalling the sequence.
            Wait::Video => self.player.as_ref().map_or(true, |p| p.finished),
        }
    }

    /// Applies an outcome from outside the click path, used by the
    /// walkthrough so it exercises the same movement code as the game.
    pub fn apply_outcome(&mut self, outcome: &Outcome) {
        self.apply(outcome);
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

fn world_domains(world: &World) -> Vec<String> {
    let mut names: Vec<String> = world.domains.keys().cloned().collect();
    names.sort();
    names
}

/// What a part-run script is waiting on.
enum Wait {
    Until(Instant),
    Video,
}

/// Folds one action's outcome into the running total for a sequence.
fn merge(into: &mut Outcome, from: Outcome) {
    into.destination = from.destination.or(into.destination.take());
    into.transition = from.transition.or(into.transition.take());
    into.new_domain = from.new_domain.or(into.new_domain.take());
    into.go_back |= from.go_back;
    into.redraw |= from.redraw;
    into.credits |= from.credits;
    into.effects.extend(from.effects);
    into.unhandled.extend(from.unhandled);
}

/// One sprite channel under script control.
#[derive(Copy, Clone, Default)]
struct Puppet {
    /// Cast member to draw; zero means the channel is claimed but empty.
    cast: u32,
    /// A claimed channel can be prepared while hidden and shown later.
    hidden: bool,
    /// Where the sprite's registration point sits, if the script set it.
    loc: Option<(i32, i32)>,
}

/// A running radio or clock programme.
struct Program {
    group: String,
    order: Vec<String>,
    /// Index of the next item to play, wrapping so the programme cycles.
    next: usize,
    /// When the current item is expected to finish.
    due: Instant,
    gain: f32,
    /// Consecutive items that failed to resolve, so a wholly unresolvable
    /// programme stops instead of polling.
    misses: usize,
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
            // The source may be shorter than its declared size: a video
            // decoder can resize its buffer mid-stream when a frame header
            // disagrees with the container. Clip rather than trusting the
            // dimensions, so a mismatch costs pixels instead of the process.
            let Some(px) = src.get(s..s + 4) else { return };
            if px[3] == 0 {
                continue;
            }
            let (r, g, b) = (px[0] as u32, px[1] as u32, px[2] as u32);
            dst[(ty as u32 * dst_w + tx as u32) as usize] = 0xff00_0000 | (r << 16) | (g << 8) | b;
        }
    }
}

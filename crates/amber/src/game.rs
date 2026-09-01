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
use crate::casttable::CastTables;
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
    /// Lookup tables for sprites whose cast is chosen by a state flag.
    tables: CastTables,
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
    /// What the effect queue is holding for, if anything.
    effect_wait: Option<Wait>,
    /// Sprite channels a script has taken over, keyed by channel so they
    /// composite in the same back-to-front order as the room's own sprites.
    puppets: BTreeMap<u8, Puppet>,
    /// The hotspot to re-run while the button is held, and when next to do it.
    repeating: Option<(Vec<String>, Instant)>,
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
            effect_wait: None,
            puppets: BTreeMap::new(),
            repeating: None,
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

        self.seed_chapter(domain);

        let target = start
            .and_then(|(name, _)| self.world.resolve(&name, Some(domain)))
            .or_else(|| self.first_room_with_art(domain))
            .or_else(|| self.world.domains.get(domain).map(|(s, _)| *s));
        if let Some(t) = target {
            self.move_to(t);
        }
    }

    /// Resolves a state-indexed cast to a cast number.
    ///
    /// `table[state[flag]]`, with the one exception `updateDisplay` makes:
    ///
    /// ```text
    /// if triggerVar = #AMBERVISION and getState(#AMBERVISION) <> #on then
    ///   assignedCast = getaProp(frameStack, #off)
    /// else
    ///   assignedCast = getaProp(frameStack, getState(triggerVar))
    /// ```
    ///
    /// So a sprite keyed on the monitor shows its `#off` art for every state
    /// but `#on` -- including `#inBetween`, which the tables list separately
    /// and which this rule means is never selected through here.
    pub fn resolve_cast(&mut self, flag: &str, table: &str) -> Option<u32> {
        let held = self.state.get(flag);
        let key = if flag.eq_ignore_ascii_case("AMBERVISION")
            && !held.as_str().is_some_and(|v| v.eq_ignore_ascii_case("on"))
        {
            lingo::Value::Symbol("off".into())
        } else {
            held
        };
        self.cast_lookup(table, &key)
    }

    /// Resolves a lookup-table entry for the current room's chapter.
    pub fn cast_lookup(&mut self, table: &str, key: &lingo::Value) -> Option<u32> {
        let domain = self.node().domain.clone();
        self.chapter(&domain);
        self.chapters.get(&domain)?.tables.lookup(table, key)
    }

    /// Writes a chapter's declared starting state into the world state.
    ///
    /// Separate from `enter_chapter` because a room can be rendered without
    /// travelling to it -- the screenshot tool jumps straight to one -- and an
    /// unseeded chapter reads every flag as void, which for a state-indexed
    /// sprite means its art resolves to nothing and the room comes up bare.
    pub fn seed_chapter(&mut self, domain: &str) {
        self.chapter(domain);
        if let Some(chapter) = self.chapters.get(domain) {
            if let Some(schema) = &chapter.schema {
                schema.seed(&mut self.state);
            }
        }
        // Handlers of the same name differ between chapters -- the door
        // setters cue different sounds -- so the active chapter has to be
        // readable from the state a handler is given.
        self.state
            .set_all("gChapter", vec![lingo::Value::Symbol(domain.to_string())]);
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

    /// Moves the player and records where they now are.
    ///
    /// `#currentLocation` is a flag like any other and handlers read it:
    /// `tryToOpenGrate` checks it before moving the player to the trapdoor,
    /// and the door setters use it to decide whether an ambience loop should
    /// be audible from where the player is standing. Nothing had been keeping
    /// it up to date, so it held whatever the chapter was seeded with.
    /// Moves without recording history, for the tools that jump to a room.
    pub fn jump_to(&mut self, room: usize) {
        self.move_to(room);
    }

    fn move_to(&mut self, room: usize) {
        let from = self.world.nodes[self.room].name.clone().unwrap_or_default();
        self.room = room;
        let node = &self.world.nodes[room];
        crate::trace::room(node.name.as_deref().unwrap_or("?"));
        trace!(
            crate::trace::Topic::Room,
            "enter {} (zone {}, {} hotspots) from {from}",
            node.name.clone().unwrap_or_default(),
            node.zone.clone().unwrap_or_else(|| "-".into()),
            node.hotspots.len()
        );
        if let Some(name) = self.world.nodes[room].name.clone() {
            self.state
                .set_all("currentLocation", vec![lingo::Value::Symbol(name)]);
        }
        // The area, which is what handlers compare against when what matters
        // is roughly where the player is rather than which wall is in view.
        if let Some(zone) = self.world.nodes[room].zone.clone() {
            self.state
                .set_all("gZone", vec![lingo::Value::Symbol(zone)]);
        }
    }

    fn chapter(&mut self, domain: &str) -> Option<&mut Chapter> {
        if !self.chapters.contains_key(domain) {
            let path = self.root.join(domain).join(format!("{domain}.DXR"));
            let movie = Movie::open(path).ok()?;
            let palettes = movie.palettes();
            let texts = movie.texts();
            let schema = Schema::from_texts(&texts);
            let tables = CastTables::from_texts(&texts);
            self.chapters.insert(
                domain.to_string(),
                Chapter {
                    movie,
                    palettes,
                    art: HashMap::new(),
                    schema,
                    tables,
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
    /// The movie the room currently places on its video channel.
    ///
    /// A video sprite may name its cast the same way a plate does, as
    /// `[#AMBERVISION, #QTsc_patio]`, and twenty-eight of them do. Returning
    /// the sprite's own `#castName` ignores that: the name is a placeholder --
    /// `SC_PATIO.multiframe` -- and the movie actually wanted is whichever
    /// cast member the table names, which for a monitor that is off is a
    /// dummy parked off stage rather than the film.
    pub fn video(&mut self) -> Option<String> {
        let sprite = self
            .node()
            .sprites
            .iter()
            .find(|s| matches!(s.channel, Channel::Video) && self.state.test(&s.condition))
            .cloned()?;

        if let Some((flag, table)) = &sprite.cast_lookup {
            let (flag, table) = (flag.clone(), table.clone());
            if let Some(cast) = self.resolve_cast(&flag, &table) {
                let domain = self.node().domain.clone();
                let named = self
                    .chapter(&domain)
                    .and_then(|c| c.movie.member(cast))
                    .and_then(|m| m.name.clone());
                if let Some(name) = named {
                    return Some(name);
                }
                trace!(
                    crate::trace::Topic::Video,
                    "{table}[{flag}] is cast {cast}, which has no name"
                );
            }
        }
        sprite.cast_name.clone()
    }

    /// True when a room places nothing on the sprite channels. Such rooms are
    /// not blank: they are the ones carried entirely by their movie.
    pub fn draws_nothing(&mut self) -> bool {
        self.visible().is_empty()
    }

    /// Loads and starts a named movie, or the current room's own.
    ///
    /// `pushVideo` takes no argument in most of the scripts, meaning the movie
    /// the room already places on its video channel: the montages play by
    /// swapping which movie that is and asking for it again.
    pub fn play_movie(&mut self, name: Option<&str>) {
        match name {
            Some(n) => {
                let n = n.trim_start_matches('#').to_string();
                self.player = None;
                match self.movies.find(&n) {
                    Some(path) => {
                        trace!(crate::trace::Topic::Video, "push {n} -> {}", path.display());
                        self.player = VideoPlayer::open(path);
                    }
                    None => trace!(crate::trace::Topic::Video, "no file for movie {n}"),
                }
            }
            None => self.start_room_video(),
        }
    }

    /// Loads and starts the current room's movie, if it has one.
    pub fn start_room_video(&mut self) {
        self.player = None;
        let Some(name) = self.video() else {
            return;
        };
        match self.movies.find(&name) {
            Some(path) => {
                trace!(crate::trace::Topic::Video, "open {name} -> {}", path.display());
                self.player = VideoPlayer::open(path);
            }
            None => {
                trace!(crate::trace::Topic::Video, "no file for movie {name}");
                eprintln!("warning: no file for movie {name}");
            }
        }

        // Whether a film loops is a property of the cast member, not of the
        // room that places it or of what else is on screen. Director stores
        // it there, which is why nothing in a room's own record says which
        // kind of film it has.
        let domain = self.node().domain.clone();
        let loops = self
            .chapter(&domain)
            .and_then(|c| c.movie.member_by_name(&name).map(|m| m.loops))
            .unwrap_or(false);
        if let Some(p) = &mut self.player {
            p.set_looping(loops);
        }
        trace!(
            crate::trace::Topic::Video,
            "{name} {}",
            if loops { "loops" } else { "plays once" }
        );
    }

    /// The stage elements that should currently draw, back to front.
    ///
    /// Takes `&mut self` so the chapter is loaded before its lookup tables are
    /// consulted; a sprite that picks its cast by state needs them on the very
    /// first frame of the room, not the second.
    pub fn visible(&mut self) -> Vec<(u8, u32, Option<(i32, i32)>)> {
        let domain = self.node().domain.clone();
        self.chapter(&domain);
        let tables = self.chapters.get(&domain).map(|c| &c.tables);

        let mut out: Vec<(u8, u32, Option<(i32, i32)>)> = self
            .node()
            .sprites
            .iter()
            .filter(|s| matches!(s.channel, Channel::Sprite(_)))
            .filter(|s| self.state.test(&s.condition))
            .filter_map(|s| {
                let cast = match (s.cast_number, &s.cast_lookup) {
                    (0, Some((flag, table))) => {
                        // The monitor's sprites show their `#off` art for
                        // every state but `#on`; see `resolve_cast`.
                        let held = self.state.get(flag);
                        let key = if flag.eq_ignore_ascii_case("AMBERVISION")
                            && !held.as_str().is_some_and(|v| v.eq_ignore_ascii_case("on"))
                        {
                            lingo::Value::Symbol("off".into())
                        } else {
                            held
                        };
                        let found = tables.and_then(|t| t.lookup(table, &key));
                        if found.is_none() {
                            trace!(
                                crate::trace::Topic::Sprite,
                                "cast lookup miss {table}[{flag} = {key:?}], {} tables loaded",
                                tables.map(|t| t.len()).unwrap_or(0)
                            );
                        }
                        found?
                    }
                    (n, _) if n > 0 => n,
                    _ => return None,
                };
                let ch = match s.channel {
                    Channel::Sprite(n) => n,
                    _ => 0,
                };
                Some((ch, cast, s.center))
            })
            .collect();
        out.sort_by_key(|(ch, _, _)| *ch);
        out
    }

    /// The effects that are due now, stopping at the first one that waits.
    ///
    /// A handler emits its whole sequence in one go -- the mirror message is
    /// `cursorOff`, `suspendSounds`, `pushVideo`, `wait #videoStop`,
    /// `restoreSounds`, `trimState` -- and the waits inside it are the timing.
    /// Draining the queue in one pass applied all six in a single frame, so
    /// the sounds were suspended and restored in the same instant and the
    /// video played with nothing sequenced around it. `pump` already honours
    /// waits between actions; this honours them within one action's effects.
    pub fn drain_ready(&mut self) -> Vec<Effect> {
        if let Some(wait) = &self.effect_wait {
            // A movie something is waiting on has to be allowed to end, and
            // this is the first moment it can be said: the `pushVideo` that
            // started it is applied *after* the wait is armed, so anything
            // done at arming time lands on the previous movie and the new one
            // comes up looping. It then never finishes, and the wait never
            // clears -- the sequence stops for good part way through.
            if matches!(wait, Wait::Video) {
                if let Some(p) = &mut self.player {
                    p.set_looping(false);
                }
            }
            let wait = self.effect_wait.as_ref().expect("just checked");
            if !self.wait_satisfied(wait) {
                return Vec::new();
            }
            self.effect_wait = None;
        }

        let mut ready = Vec::new();
        while !self.pending.is_empty() {
            let effect = self.pending.remove(0);
            if let Some(w) = wait_for(&effect) {
                // The wait is armed after the effects before it are handed
                // over, so the video this is waiting on has been started by
                // the time the wait is first tested.
                trace!(
                    crate::trace::Topic::Script,
                    "will hold on {effect:?} once the effects above are applied"
                );
                self.effect_wait = Some(w);
                break;
            }
            ready.push(effect);
        }
        ready
    }

    /// Runs the whole effect queue at once, ignoring its waits.
    ///
    /// The waits are pacing, and the walkthrough has no clock to pace against.
    /// What it does need is the state the effects carry: `trimState` and
    /// `setState` reach the flags through this queue, so a route replayed
    /// without draining it ends somewhere the same route in the window would
    /// not. Everything that is sound or picture is dropped, and everything
    /// that is state is applied.
    pub fn settle(&mut self) -> Vec<String> {
        let mut audio = Vec::new();
        // A sequence that holds leaves its remaining actions queued, and in
        // the window the next frame pumps them. Nothing pumps in the
        // walkthrough, so a route replayed there stopped at the first `wait`
        // -- the breaker switch threw and the lights never came on.
        //
        // The bound is a guard against a handler that asks to repeat while the
        // button is held, which in a window is the player's finger and here is
        // nothing at all.
        for _ in 0..64 {
            if self.script.is_empty() {
                break;
            }
            self.waiting = None;
            self.pump();
        }
        self.repeating = None;

        self.effect_wait = None;
        while !self.pending.is_empty() {
            let effect = self.pending.remove(0);
            // Sound is reported rather than played: the walkthrough has no
            // device, and what a route triggers is exactly what is worth
            // seeing when a route sounds wrong.
            match &effect {
                Effect::PlaySound { name, loudness } => audio.push(match loudness {
                    Some(l) => format!("play {name} ({l})"),
                    None => format!("play {name}"),
                }),
                Effect::StartLoop { name, volume } => {
                    audio.push(format!("loop {name} at {}", volume.unwrap_or(255)))
                }
                Effect::StopLoop { name, .. } => audio.push(format!("stop {name}")),
                _ => {}
            }
            self.apply_puppet(&effect);
        }
        audio
    }

    /// Whether the effect queue still has work, including a wait in progress.
    pub fn effects_busy(&self) -> bool {
        !self.pending.is_empty() || self.effect_wait.is_some()
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

        // Everything on the stage is one ordered set of sprite channels, and
        // the numbers come from the game rather than from a guess about which
        // kind of element belongs on top:
        //
        //   * A room writes `#channel: N`, which is an offset from
        //     `lastScoreSprite`. `setUpGame` sets that to 12, so a room's
        //     channels 1-10 are really 13-22.
        //   * Movies live on 44 and 45 -- `refreshVidSprites` forces a redraw
        //     by flickering the visibility of exactly those two.
        //   * A script takes a channel with `puppetSprite` and drives it
        //     directly, at whatever number it names: 30, 39 and 44 are the
        //     ones the game claims.
        //
        // Drawing these as three fixed layers -- plates, then the movie, then
        // the puppets -- happens to be right until a script claims a channel
        // below the movie, and then the puppet lands on top of a film it
        // belongs behind.
        const SCORE_BASE: u16 = 12;
        const MOVIE_CHANNEL: u16 = 44;

        enum Layer {
            /// A cast member from the room or from a puppet channel.
            Art { cast: u32, at: Option<(i32, i32)> },
            Movie,
        }

        let mut stage: Vec<(u16, Layer)> = Vec::new();
        for (ch, cast, center) in self.visible() {
            stage.push((SCORE_BASE + ch as u16, Layer::Art { cast, at: center }));
        }
        if self.player.is_some() {
            stage.push((MOVIE_CHANNEL, Layer::Movie));
        }
        for (ch, puppet) in self.puppets.iter() {
            if puppet.cast == 0 || puppet.hidden {
                continue;
            }
            stage.push((
                *ch as u16,
                Layer::Art {
                    cast: puppet.cast,
                    at: puppet.loc,
                },
            ));
        }
        // A stable sort keeps the room's own order within a channel, which is
        // how two plates sharing one channel stack.
        stage.sort_by_key(|(ch, _)| *ch);

        let domain = self.node().domain.clone();
        let video_centre = self
            .world
            .nodes[self.room]
            .sprites
            .iter()
            .find(|s| matches!(s.channel, Channel::Video))
            .and_then(|s| s.center);

        for (channel, layer) in stage {
            match layer {
                Layer::Art { cast, at } => {
                    let Some(art) = self.art(&domain, cast) else {
                        continue;
                    };
                    // `#coords` gives where the sprite's registration point
                    // lands on the stage. Without one there is no anchor and
                    // the registration point alone says nothing, so the image
                    // is centred instead.
                    let (ox, oy) = match at {
                        Some((cx, cy)) => (
                            cx - if art.reg_x != 0 { art.reg_x as i32 } else { art.width as i32 / 2 },
                            cy - if art.reg_y != 0 { art.reg_y as i32 } else { art.height as i32 / 2 },
                        ),
                        None => (
                            (width as i32 - art.width as i32) / 2,
                            (height as i32 - art.height as i32) / 2,
                        ),
                    };
                    trace!(
                        crate::trace::Topic::Sprite,
                        "draw ch{channel} cast {cast} {}x{} at ({ox},{oy})",
                        art.width,
                        art.height
                    );
                    blit(frame, width, height, &art.rgba, art.width, art.height, ox, oy);
                }
                Layer::Movie => {
                    let Some(player) = &self.player else { continue };
                    // The decoder is authoritative: a frame header can
                    // disagree with the container, and it is the decoder that
                    // resized its buffer.
                    let (w, h) = player.frame_size();
                    let centre =
                        video_centre.unwrap_or((width as i32 / 2, height as i32 / 2));
                    trace!(
                        crate::trace::Topic::Sprite,
                        "draw ch{channel} movie {w}x{h} at {centre:?}"
                    );
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
            }
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
                // The current chapter first, because that is where a room's
                // own sounds are, then the others: a cast number is per movie,
                // and a chapter can name a sound another one carries.
                let domain = self.node().domain.clone();
                let mut order = vec![domain.clone()];
                order.extend(world_domains(&self.world).into_iter().filter(|d| *d != domain));
                for d in order {
                    let Some(chapter) = self.chapter(&d) else { continue };
                    if let Ok(s) = chapter.movie.sound(number) {
                        if !s.samples.is_empty() {
                            return Some(sound::Pcm {
                                samples: s.samples,
                                rate: s.sample_rate,
                                channels: s.channels,
                            });
                        }
                    }
                }
                None
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
                // A sound sprite's `#earShot` is its level, and a negative one
                // is an instruction to stop rather than a level to play at:
                //
                //   sndVolume = getProp(sprite, #earShot)
                //   if sndVolume < 0 then endLoop( value(castName) )
                //   else setLoop( value(castName), sndVolume )
                //
                // Fifty-six sprites use it that way, to silence a loop in a
                // room it should not carry into. Passed through as a level it
                // is a negative gain, which inverts the waveform.
                let level = s.volume.unwrap_or(255);
                if level < 0 {
                    return None;
                }
                Some((name, level as f32 / 255.0))
            })
            .collect();
        for (key, level) in &node.ambience {
            // The mix keys are the loop names with a volume suffix; the house
            // hum is the one that is always present.
            if key == "househum" && *level > 0 {
                out.push(("houseHum".into(), *level as f32 / 255.0));
            }
        }

        // `#earShot` is a balance, not a set of absolute levels: it says how
        // prominent each source is from where the player is standing. The
        // living room asks for a clock at 47%, a radio at 47%, a fire at 100%
        // and the house hum at 88%, and those sources are real recordings --
        // the hum alone peaks at 96% of full scale. Summed as written that is
        // nearly three times full scale, and a hundred and one rooms ask for
        // more than one. Everything below the ceiling is left exactly as the
        // room asked for it; only a bed that would not fit is scaled, which
        // keeps the balance and removes the clipping.
        //
        // The ceiling sits below unity because speech and sound effects play
        // over this, and they are what the player is meant to be listening to.
        const BED_CEILING: f32 = 0.7;
        let total: f32 = out.iter().map(|(_, g)| *g).sum();
        if total > BED_CEILING {
            let scale = BED_CEILING / total;
            for (_, gain) in out.iter_mut() {
                *gain *= scale;
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
        let holding = self.state.item_in_use().is_some();
        self.node()
            .hit_test(x, y, holding, |c| state.test(c))
            .map(|h| (h.verb, h.bounds))
    }

    /// Handles a click, moving the player if the hotspot says to.
    pub fn click(&mut self, x: i32, y: i32) -> Option<Outcome> {
        // Handlers such as `stashClick` want the click position, which the
        // scripts read from the mouse rather than being passed.
        self.state.set("gMouseLoc", lingo::Value::Point(x, y));
        let actions = {
            let state = &self.state;
            let holding = self.state.item_in_use().is_some();
            self.node()
                .hit_test(x, y, holding, |c| state.test(c))?
                .actions
                .clone()
        };
        // A click abandons whatever the previous one was still waiting on.
        self.script = actions.clone();
        self.waiting = None;
        let outcome = self.pump();
        // A dial asks to keep turning; the first repeat waits out the lag the
        // original spends before it starts spinning.
        self.repeating = outcome
            .repeat_while_held
            .then(|| (actions, Instant::now() + Duration::from_millis(400)));
        Some(outcome)
    }

    /// Notes whether the button is still down, and re-runs the held action
    /// when its interval comes round.
    ///
    /// The first repeat waits longer than the rest, which is what the original
    /// does with its lag timer: a single click turns one notch, and holding
    /// spins.
    pub fn tick_held(&mut self, held: bool) -> Option<Outcome> {
        if !held {
            self.repeating = None;
            return None;
        }
        let (actions, due) = self.repeating.as_ref()?;
        if Instant::now() < *due {
            return None;
        }
        let actions = actions.clone();
        let outcome = script::run(&actions, &mut self.state);
        self.apply(&outcome);
        self.repeating = Some((actions, Instant::now() + Duration::from_millis(120)));
        Some(outcome)
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
            let hold = outcome.effects.iter().find_map(wait_for);
            // Each action's own outcome is applied, not the running total.
            // Applying the accumulation instead re-queued every effect the
            // sequence had produced so far, once more per action, so a list of
            // n effect-producing actions queued n(n+1)/2 effects: the same
            // sound started several times over itself, and identical copies of
            // one waveform sum coherently, which is far louder and harsher
            // than the same number of unrelated sounds.
            self.apply(&outcome);
            merge(&mut combined, outcome);
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
                self.move_to(prev);
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
                self.move_to(next);
            }
        }
        // A move changes which movie is on screen, so reload it either way.
        if outcome.destination.is_some() || outcome.go_back {
            self.start_room_video();
        }
        if !outcome.effects.is_empty() {
            trace!(
                crate::trace::Topic::Script,
                "queue {} effect(s), {} pending",
                outcome.effects.len(),
                self.pending.len() + outcome.effects.len()
            );
        }
        self.pending.extend(outcome.effects.iter().cloned());
    }
}

fn world_domains(world: &World) -> Vec<String> {
    let mut names: Vec<String> = world.domains.keys().cloned().collect();
    names.sort();
    names
}

/// The wait an effect asks for, if it is one.
///
/// Named so the set stays in one place: `pump` holds between a sequence's
/// actions and `drain_ready` holds within one action's effects, and the two
/// disagreeing about what counts as a wait would be invisible until a cutscene
/// ran at the wrong speed.
fn wait_for(effect: &Effect) -> Option<Wait> {
    match effect {
        Effect::WaitTicks(t) => Some(Wait::Until(
            Instant::now() + Duration::from_secs_f64(*t as f64 / 60.0),
        )),
        Effect::WaitForVideo => Some(Wait::Video),
        // A sound's real length is not known here, so this is a short hold
        // rather than a promise.
        Effect::WaitForSound(_) => {
            Some(Wait::Until(Instant::now() + Duration::from_millis(250)))
        }
        _ => None,
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exactly_the_three_wait_effects_hold_the_queue() {
        assert!(wait_for(&Effect::WaitTicks(120)).is_some());
        assert!(wait_for(&Effect::WaitForVideo).is_some());
        assert!(wait_for(&Effect::WaitForSound("tumbler".into())).is_some());
    }

    #[test]
    fn the_effects_a_cutscene_is_made_of_do_not_hold() {
        // The mirror message is cursorOff, suspendSounds, pushVideo, wait,
        // restoreSounds, trimState. Only the wait may stop the queue; if any
        // of the others did, the sequence would stall part way and the
        // ambience would stay suspended.
        for effect in [
            Effect::CursorOff,
            Effect::SuspendSounds { fade: true },
            Effect::PlayVideo(None),
            Effect::RestoreSounds { fade: true },
            Effect::StopVideo,
            Effect::PlaySound {
                name: "MCALL7".into(),
                loudness: None,
            },
            Effect::StartLoop {
                name: "houseHum".into(),
                volume: Some(224),
            },
        ] {
            assert!(wait_for(&effect).is_none(), "{effect:?} should not hold");
        }
    }
}

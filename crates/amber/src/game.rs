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
#[derive(Clone)]
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
    /// Decoded plates, keyed by cast member and by whether its
    /// background is painted -- the same member is used both ways.
    art: HashMap<(u32, bool), Option<CachedArt>>,
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
    /// True while the opening film is playing, because that one film -- and
    /// only that one -- can be cut short by a click.
    intro_running: bool,
    /// A transition the scripts have armed for the next stage change.
    transition: Option<String>,
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
    /// The film currently loaded for the room, by name. A room's film can be
    /// conditional, so this is what a state change has to be compared against.
    playing: Option<String>,
    /// The rect that film's cast member says it occupies, which is not always
    /// the size the film is stored at.
    playing_size: Option<(u32, u32)>,
    /// Whether a sequence has taken the pointer away. Every set piece opens
    /// with `cursorOff` and the player is meant to watch it, not click through
    /// it.
    pub cursor_hidden: bool,
    pub player: Option<VideoPlayer>,
}

impl Game {
    /// The chapter the game opens in. Roxy's is the present-day frame story
    /// that the other three are reached from.
    const FIRST_CHAPTER: &'static str = "ROXY";

    /// A game over an empty world, for tests that exercise the parts of
    /// `apply` that do not depend on there being rooms to move between.
    #[cfg(test)]
    fn for_test() -> Game {
        // One empty room, so `node()` has something to return. These tests
        // are about what a move arms, not about where it goes.
        let world = World {
            nodes: vec![crate::world::Node::default()],
            ..World::default()
        };
        Game::over(world, Path::new("."))
    }

    pub fn new(root: &Path) -> std::io::Result<Game> {
        Ok(Game::over(World::load(root)?, root))
    }

    fn over(world: World, root: &Path) -> Game {
        let mut game = Game {
            world,
            state: State::new(),
            room: 0,
            history: Vec::new(),
            root: root.to_path_buf(),
            chapters: HashMap::new(),
            pending: Vec::new(),
            playing: None,
            playing_size: None,
            cursor_hidden: false,
            effect_wait: None,
            intro_running: false,
            transition: None,
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
        game.play_intro();
        game
    }

    /// The room the game opens in, whose only hotspot does nothing at all.
    const INTRO_ROOM: &'static str = "Gbhs_playIntro";

    /// Plays the opening film and moves into the game behind it.
    ///
    /// `Gbhs_playIntro` has one hotspot and its action is the string
    /// `"nothing"`, so there is no way out of the opening by clicking on the
    /// room. The way out is in `initInventory`, which runs at startup and ends
    /// with a special case for this one room:
    ///
    /// ```text
    /// if getState( #currentLocation ) = #Gbhs_playIntro then
    ///   cursorOff
    ///   suspendSounds
    ///   pushVideo
    ///   repeat while the movieRate of sprite 44 <> 0 and not the mouseDown
    ///     updateStage
    ///   end repeat
    ///   killVideo
    ///   goTo #Gbhs_gameEntry, #fadeIn
    /// end if
    /// ```
    ///
    /// Without it the engine sits on `intro.mov` for ever, which is where the
    /// game actually began for anyone starting a new one.
    ///
    /// `suspendSounds` is left out because the intro room declares no
    /// ambience: there is nothing playing for it to suspend, and suspending
    /// with nothing to restore afterwards would only risk silence later.
    fn play_intro(&mut self) {
        if !self.node().name.as_deref().is_some_and(|n| n == Self::INTRO_ROOM) {
            return;
        }
        self.intro_running = true;
        self.pending.extend([
            Effect::CursorOff,
            Effect::PlayVideo(None),
            Effect::WaitForVideo,
            Effect::StopVideo,
            Effect::GoToRoom {
                room: "Gbhs_gameEntry".into(),
                transition: Some("fadeIn".into()),
            },
        ]);
    }

    /// Cuts the opening film short, which is the one film a click can skip.
    ///
    /// Every other wait in the game is `wait #videoStop`, and that handler
    /// loops on the movie rate alone with no test on the mouse. The opening
    /// is the only place the original watches for a click, so this is not a
    /// general skip and should not become one.
    fn skip_intro(&mut self) -> bool {
        if !self.intro_running {
            return false;
        }
        self.intro_running = false;
        self.effect_wait = None;
        self.player = None;
        self.playing = None;
        true
    }
}

impl Game {


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

        // The declared start, but only if it can actually show something.
        //
        // Margaret's chapter declares `#bedrm_fadeIn`, which is a scaffolding
        // record: its only sprite is a seventeen by three palette holder, its
        // exits go to a literal `#destination`, and the film it plays --
        // `40sINTRO.mov` -- is one of seven digital video members on the disc
        // with no file behind it. Entering there is a black screen with no way
        // out. Roxy's opening also draws nothing, but its film is there, so
        // the test is whether the room has *either*.
        let declared = start.and_then(|(name, _)| self.world.resolve(&name, Some(domain)));
        let usable = declared.filter(|&i| {
            let here = self.room;
            self.room = i;
            let plays = self
                .video()
                .is_some_and(|name| self.movies.find(&name).is_some());
            // A way out, rather than anything about what is drawn. The
            // template's only sprite is a seventeen by three palette holder,
            // which is drawing by any measure and a scene by none; what marks
            // it as scaffolding is that both its exits go to a literal
            // `#destination` that resolves to no room at all.
            //
            // The guards are deliberately ignored. Brice's chapter opens on a
            // montage whose every exit is gated until it has played, so asking
            // which exits are live right now would reject a perfectly good
            // opening. Asking whether they *name* a room separates a scene
            // from a template: the template's exits go to `#destination`,
            // which is a word rather than a place.
            let leads_somewhere = self.world.nodes[i]
                .hotspots
                .iter()
                .filter(|h| !h.actions.is_empty())
                .filter_map(|h| {
                    let mut probe = self.state.clone();
                    crate::script::run(&h.actions, &mut probe).destination
                })
                .any(|dest| self.world.resolve(&dest, Some(domain)).is_some());
            self.room = here;
            if !plays && !leads_somewhere {
                trace!(
                    crate::trace::Topic::Room,
                    "{domain} declares a start with no film and no exit; \
                     going to its first room with art instead"
                );
            }
            plays || leads_somewhere
        });

        let target = usable
            .or_else(|| self.first_room_with_art(domain))
            .or_else(|| self.world.domains.get(domain).map(|(s, _)| *s));
        if let Some(t) = target {
            self.move_to(t);
            // Director runs `exitFrame` as each frame ends, and a chapter's
            // opening sequence lives inside it. This engine has no score, so
            // the chapter's own frame script is offered the room once, here.
            // Only handlers that recognise the room do anything.
            let mut outcome = crate::script::Outcome::default();
            if crate::natives::call("exitframe", &[], &mut self.state, &mut outcome) {
                self.apply(&outcome);
            }
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
        // The opening is skippable in the game itself, so skipping it here is
        // the same act, and it knows where to go next. Falling through to
        // `first_playable` instead landed in whatever room happened to have
        // art, which is not a place the player could have reached.
        if self.skip_intro() {
            return true;
        }
        if self.player.is_none() {
            return false;
        }
        self.player = None;
        if !self.draws_nothing() {
            return true;
        }
        // The opening used to be jumped past from here, because nothing in
        // the engine knew how to end it. `play_intro` does that now, and
        // doing it twice was worse than not doing it at all: this moved the
        // room without draining the queue, so the intro's own `goTo` stayed
        // pending and fired under the player's next click, sending them back
        // to the entry they had just left.
        if let Some(i) = self.first_playable() {
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
        // Going somewhere deliberately abandons the opening rather than
        // skipping it. Skipping keeps the `goTo #Gbhs_gameEntry` the original
        // runs afterwards, which would otherwise fire from the queue a moment
        // later and drag the player back out of wherever they had jumped to.
        self.cancel_intro();
        self.move_to(room);
    }

    /// Throws the opening away, film and destination together.
    fn cancel_intro(&mut self) {
        if !self.intro_running {
            return;
        }
        self.intro_running = false;
        self.effect_wait = None;
        self.player = None;
        self.playing = None;
        self.pending.clear();
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
        // The room's own mix, which handlers read off the puppeteer: the
        // ghostly telephone rings at whatever level the room says the phone
        // carries from here, so it is loud in the living room and faint
        // upstairs.
        let mix: Vec<(String, i32)> = self.world.nodes[room]
            .ambience
            .iter()
            .map(|(k, v)| (k.clone(), *v))
            .collect();
        for (key, level) in mix {
            self.state
                .set_all(&format!("gEarShot_{key}"), vec![lingo::Value::Int(level)]);
        }
    }

    fn chapter(&mut self, domain: &str) -> Option<&mut Chapter> {
        if !self.chapters.contains_key(domain) {
            let path =
                crate::world::find_ci(&self.root.join(domain), &format!("{domain}.DXR"))?;
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
    fn art(&mut self, domain: &str, cast: u32, matte: bool) -> Option<&CachedArt> {
        let chapter = self.chapter(domain)?;
        let key = (cast, matte);
        if !chapter.art.contains_key(&key) {
            let decoded = chapter.movie.bitmap(cast).ok().map(|b: Bitmap| {
                // The member names the palette cast it was authored against.
                let palette = chapter
                    .movie
                    .palette_for_cast(b.palette_ref)
                    .or_else(|| chapter.palettes.first().cloned())
                    .unwrap_or_default();
                // The whole game uses two inks: 0 for the 2345 sprites that
                // are a room's own plates, and 36 for the fifteen that are
                // something held up in front of one -- a phone lifted to the
                // ear, a bottle turned over, a newspaper being read. Those
                // fifteen are drawn on a white field that must not be painted,
                // and index zero is white in every one of this game's
                // palettes.
                //
                // Not painting it is the whole of what ink means here, so
                // rather than model Director's ink table this asks the one
                // question the data actually poses.
                let transparent = matte.then_some(0u8);
                CachedArt {
                    rgba: b.to_rgba(&palette, transparent),
                    width: b.width as u32,
                    height: b.height as u32,
                    reg_x: b.reg_x,
                    reg_y: b.reg_y,
                }
            });
            chapter.art.insert(key, decoded);
        }
        chapter.art.get(&key).and_then(Option::as_ref)
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
                // The rect this one occupies, not whatever the last film left
                // behind. Forgetting to move this with the player drew a new
                // film squeezed into the previous film's shape.
                let domain = self.node().domain.clone();
                self.playing = Some(n.clone());
                self.playing_size = self
                    .chapter(&domain)
                    .and_then(|c| c.movie.member_by_name(&n))
                    .map(|m| (m.width as u32, m.height as u32))
                    .filter(|(w, h)| *w > 0 && *h > 0);
            }
            None => self.start_room_video(),
        }
    }

    /// Loads and starts the current room's movie, if it has one.
    pub fn start_room_video(&mut self) {
        self.player = None;
        let Some(name) = self.video() else {
            self.playing = None;
            self.playing_size = None;
            return;
        };
        self.playing = Some(name.clone());
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
        let member = self
            .chapter(&domain)
            .and_then(|c| c.movie.member_by_name(&name))
            .map(|m| (m.loops, m.width, m.height));
        let loops = member.map(|(l, ..)| l).unwrap_or(false);
        self.playing_size = member
            .map(|(_, w, h)| (w as u32, h as u32))
            .filter(|(w, h)| *w > 0 && *h > 0);
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
    pub fn visible(&mut self) -> Vec<StageElement> {
        let domain = self.node().domain.clone();
        self.chapter(&domain);
        let tables = self.chapters.get(&domain).map(|c| &c.tables);

        let mut out: Vec<StageElement> = self
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
                Some((ch, cast, s.center, s.ink))
            })
            .collect();
        out.sort_by_key(|(ch, ..)| *ch);
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
        let mut report = Vec::new();
        // A sequence interleaves: the script runs until it waits, the effects
        // queued so far play, and then the script goes on. Running the whole
        // script and *then* draining -- which is what this used to do -- shows
        // every state write before the first film, which is not the order
        // anything happens in. It made the portal into Margaret's chapter look
        // broken when what was broken was the report.
        //
        // The bound guards against a handler that asks to repeat while a
        // button is held, which in a window is the player's finger and here is
        // nothing at all.
        for _ in 0..256 {
            // Whatever is due now, in order. The waits are the timing and
            // there is no clock here, so they are named and stepped over.
            while !self.pending.is_empty() {
                let effect = self.pending.remove(0);
                match &effect {
                    // Sound is reported rather than played: the walkthrough
                    // has no device, and what a route triggers is exactly what
                    // is worth seeing when a route sounds wrong.
                    Effect::PlaySound { name, loudness } => report.push(match loudness {
                        Some(l) => format!("play {name} ({l})"),
                        None => format!("play {name}"),
                    }),
                    Effect::StartLoop { name, volume } => {
                        report.push(format!("loop {name} at {}", volume.unwrap_or(255)))
                    }
                    Effect::StopLoop { name, .. } => report.push(format!("stop {name}")),
                    // Which film a room plays depends on state, so a sequence
                    // that steps a flag between two `pushVideo`s plays two
                    // different films -- and that is the whole of what these
                    // sequences are for. Resolved here, not decoded.
                    Effect::PlayVideo(which) => {
                        let named = which.clone().or_else(|| self.video());
                        report.push(match named {
                            Some(n) => format!("film {n}"),
                            None => "film (none)".to_string(),
                        });
                    }
                    Effect::PlayVideoSegment { from, to } => {
                        report.push(format!("film {from}..{to}"))
                    }
                    Effect::StopVideo => report.push("film stops".into()),
                    Effect::WaitForVideo => report.push("wait for the film".into()),
                    Effect::WaitForSound(n) => report.push(format!("wait for {n}")),
                    Effect::WaitTicks(t) if *t > 0 => report.push(format!("wait {t}")),
                    Effect::SetState { key, value } => report.push(format!("{key} = {value:?}")),
                    Effect::FadeToMontage(n) => report.push(format!("montage {n}")),
                    _ => {}
                }
                self.apply_puppet(&effect);
            }
            self.effect_wait = None;
            if self.script.is_empty() {
                break;
            }
            self.waiting = None;
            self.pump();
        }
        self.repeating = None;
        report
    }

    /// Takes an armed transition, ready to run.
    ///
    /// A transition is armed once and spent on the next change of the stage,
    /// which is what `setTransition` means: it does not persist.
    pub fn take_transition(&mut self) -> Option<Transition> {
        let kind = self.transition.take()?;
        Some(transition_for(&kind))
    }

    /// Drops a wait in progress, for a tool with no clock to wait against.
    pub fn clear_effect_wait(&mut self) {
        self.effect_wait = None;
    }

    /// Whether the effect queue still has work, including a wait in progress.
    /// Whether the action list has run out, as distinct from its effects.
    pub fn script_idle(&self) -> bool {
        self.script.is_empty() && self.waiting.is_none()
    }

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
            Effect::GoToRoom { room, transition } => {
                self.intro_running = false;
                if let Some(kind) = transition {
                    self.transition = Some(kind.clone());
                }
                let from = self.node().domain.clone();
                match self.world.resolve(room, Some(&from)) {
                    Some(next) => {
                        trace!(crate::trace::Topic::Room, "scripted move to {room}");
                        if next != self.room {
                            self.history.push(self.room);
                        }
                        self.move_to(next);
                        self.start_room_video();
                    }
                    None => trace!(
                        crate::trace::Topic::Room,
                        "scripted move names {room}, which is not a room"
                    ),
                }
            }
            // Every set piece opens with this and the original hides the
            // pointer for its duration. It was emitted in a hundred and four
            // places and acted on in none -- the fifth effect in this engine
            // to be produced and dropped, and the one my coverage test could
            // not see, because the test asks whether a variant is *mentioned*
            // in a file that applies effects and this one was mentioned in a
            // list of effects to emit.
            Effect::CursorOff => self.cursor_hidden = true,
            Effect::SetTransition { kind } => {
                trace!(crate::trace::Topic::Room, "transition armed: {kind}");
                self.transition = Some(kind.clone());
            }
            Effect::FadeToMontage(step) => {
                trace!(crate::trace::Topic::Room, "montage step {step}");
                self.state.set("showMontage", lingo::Value::Int(*step));
                self.transition = Some("fadeIn".into());
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
            Art {
                cast: u32,
                at: Option<(i32, i32)>,
                /// Whether the member's background colour is painted.
                matte: bool,
            },
            Movie,
        }

        let mut stage: Vec<(u16, Layer)> = Vec::new();
        for (ch, cast, center, ink) in self.visible() {
            stage.push((
                SCORE_BASE + ch as u16,
                Layer::Art {
                    cast,
                    at: center,
                    matte: ink != 0,
                },
            ));
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
                    // A script-driven channel does not carry an ink of its
                    // own; the game only ever puts full plates on one.
                    matte: false,
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
                Layer::Art { cast, at, matte } => {
                    let Some(art) = self.art(&domain, cast, matte) else {
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
                    // The decoder is authoritative for what was decoded: a
                    // frame header can disagree with the container, and it is
                    // the decoder that resized its buffer.
                    let (w, h) = player.frame_size();
                    // But not for how big it is drawn. A film's cast member
                    // carries the rect it occupies, and that is not always the
                    // size it is stored at -- `MEmrloop.mov`, the loop behind
                    // the portal into Margaret's chapter, is a 160 by 120 film
                    // in a 320 by 240 member. Drawn at its stored size it is a
                    // small patch in the middle of a black screen, which is
                    // exactly what that room looked like.
                    let (dw, dh) = self.playing_size.filter(|(a, b)| *a > 0 && *b > 0).unwrap_or((w, h));
                    let centre =
                        video_centre.unwrap_or((width as i32 / 2, height as i32 / 2));
                    trace!(
                        crate::trace::Topic::Sprite,
                        "draw ch{channel} movie {w}x{h} as {dw}x{dh} at {centre:?}"
                    );
                    blit_scaled(
                        frame,
                        width,
                        height,
                        player.frame(),
                        w,
                        h,
                        dw,
                        dh,
                        centre.0 - dw as i32 / 2,
                        centre.1 - dh as i32 / 2,
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

    /// Returns the next item to play when the current one has run its course.
    ///
    /// The caller plays it and the programme schedules the following item from
    /// the length of what was just handed over, which keeps the sequence
    /// running without the mixer having to report completions.
    pub fn tick_program(&mut self) -> Option<Cue> {
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
        Some((group, samples, rate, channels, gain))
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

        // Each source plays at the level its room asks for and nothing else.
        //
        // Entry 57 scaled the whole bed down when it summed past a ceiling,
        // which was measured before the game's own `soundVolTweaks` trim was
        // applied and so was measuring numbers that never reach the mixer.
        // With the trim, twenty-eight rooms sum above full scale rather than a
        // hundred and one, and the most is 1.83 rather than 2.82 -- which the
        // saturator handles as gentle compression.
        //
        // The scaling also had a fault worse than the clipping it prevented.
        // It divided by how many sources a room had, so the house hum -- which
        // every one of these rooms declares at 224 -- played at 21% in one
        // room and 11% in the next. The hum is the one sound that has to be
        // steady as the player walks through the house, and a bed that dips
        // whenever a clock comes into earshot is more obviously wrong than a
        // peak that compresses.
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
        self.art(&domain, cast, false).is_some()
    }

    /// Draws the game's own cursor for `verb` at `(x, y)`.
    ///
    /// Returns false when there is no art to draw -- a system cursor, or a
    /// chapter whose movie does not carry the pair -- so the caller can fall
    /// back to a drawn shape rather than leaving the player with no pointer.
    pub fn draw_cursor(
        &mut self,
        frame: &mut [u32],
        width: u32,
        height: u32,
        verb: Option<crate::world::Verb>,
        x: i32,
        y: i32,
    ) -> bool {
        let holding = self.state.item_in_use().map(str::to_string);
        let Some(id) = crate::cursor::id_for(verb, holding.as_deref()) else {
            return false;
        };
        let Some((image, mask)) = crate::cursor::casts_for(id) else {
            return false;
        };
        let domain = self.node().domain.clone();
        // Both halves, and both have to be there: a cursor drawn without its
        // mask is a black square.
        let Some(art) = self.art(&domain, image, false).cloned() else {
            return false;
        };
        let Some(shape) = self.art(&domain, mask, false).cloned() else {
            return false;
        };

        // The hot spot is the middle. Director stores one per cursor and this
        // does not read it yet; for a diamond, an arrow and a lens the centre
        // is close enough to click with, and being wrong by a few pixels is
        // visible only if you look for it.
        // A Macintosh cursor is sixteen by sixteen. These members are stored
        // eighteen or nineteen square and the pair does not even agree with
        // itself -- the forward cursor is a 19 image against an 18 mask -- so
        // the last rows and columns are something other than the picture,
        // most likely the hot spot. Cropping to sixteen leaves an arrow, a
        // viewfinder and a diamond; not cropping leaves those with a fringe of
        // speckle down two sides.
        const CURSOR: u32 = 16;
        let (w, h) = (
            art.width.min(shape.width).min(CURSOR),
            art.height.min(shape.height).min(CURSOR),
        );
        let (ox, oy) = (x - w as i32 / 2, y - h as i32 / 2);
        for row in 0..h {
            for col in 0..w {
                let (px, py) = (ox + col as i32, oy + row as i32);
                if px < 0 || py < 0 || px >= width as i32 || py >= height as i32 {
                    continue;
                }
                let i = ((row * shape.width + col) * 4) as usize;
                // The mask says which pixels exist; the image says their
                // colour. A one-bit member decodes to black and white, so the
                // test is simply which of the two it landed on.
                if shape.rgba.get(i).copied().unwrap_or(0) < 128 {
                    continue;
                }
                let j = ((row * art.width + col) * 4) as usize;
                let lit = art.rgba.get(j).copied().unwrap_or(0) >= 128;
                frame[(py as u32 * width + px as u32) as usize] =
                    if lit { 0xffff_ffff } else { 0xff00_0000 };
            }
        }
        true
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
            let Some(art) = self.art(&domain, cast, false) else { continue };
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
        // The opening film watches for a click and stops early if it gets
        // one. Its room's own hotspot does nothing, so this has to come first
        // or the click is swallowed by it.
        if self.skip_intro() {
            return Some(Outcome::default());
        }
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
            Wait::Video => self.player.as_ref().is_none_or(|p| p.finished),
        }
    }

    /// Applies an outcome from outside the click path, used by the
    /// walkthrough so it exercises the same movement code as the game.
    pub fn apply_outcome(&mut self, outcome: &Outcome) {
        self.apply(outcome);
    }

    fn apply(&mut self, outcome: &Outcome) {
        // Crossing into another chapter. The transition rooms end on
        // `enterNewDomain`, and until this was acted on the player watched the
        // whole sequence and stayed where they were.
        if let Some(domain) = &outcome.new_domain {
            let target = self
                .world
                .domains
                .keys()
                .find(|d| d.eq_ignore_ascii_case(domain))
                .cloned();
            match target {
                Some(d) => {
                    trace!(crate::trace::Topic::Room, "entering {d}");
                    self.enter_chapter(&d);
                    self.start_room_video();
                    return;
                }
                None => trace!(
                    crate::trace::Topic::Room,
                    "enterNewDomain names {domain}, which is not a chapter"
                ),
            }
        }
        // `goTo destination, transition` hands its second argument straight
        // to `setTransition` before it moves, so the flavour on a move is the
        // transition for that move. Every one of the game's three thousand
        // eight hundred moves names one and this engine dropped all of them,
        // which made every turn of the head a hard cut.
        //
        // Armed here rather than through the effect queue because the original
        // arms it inline, in the statement before `moveToLocation`: it has to
        // be set before the stage changes, and the queue drains afterwards.
        if outcome.destination.is_some() || outcome.go_back {
            if let Some(kind) = &outcome.transition {
                self.transition = Some(kind.clone());
            }
        }
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
        //
        // So can standing still. Which film a room plays is guarded like
        // anything else -- the psionic bar's waveform is behind
        // `[#equals: [#BarOnline, 1]]` -- so solving a puzzle can make a film
        // eligible where a moment ago there was none. Reloading only on a move
        // meant the bar came online and went on showing nothing, which is
        // indistinguishable from the puzzle not having worked.
        if outcome.destination.is_some() || outcome.go_back {
            self.start_room_video();
        } else if outcome.redraw && !self.effects_busy() && self.video() != self.playing {
            // Not while a scripted sequence is running: `pushVideo` puts a
            // film on the same player, and reloading the room's own film
            // underneath it would cut the sequence off part way.
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

/// One thing to draw on the stage: its channel, cast member, an override
/// position when a script has moved it, and its ink.
pub type StageElement = (u8, u32, Option<(i32, i32)>, i32);

/// A sound a programme wants played: name, samples, rate, channels, gain.
pub type Cue = (String, Arc<Vec<i16>>, u32, u16, f32);

/// Blits RGBA source pixels onto a BGRA framebuffer, clipped to its bounds.
/// Draws `src` scaled to `dst_w_out` by `dst_h_out`.
///
/// Nearest neighbour, which is what Director did and what the source material
/// wants: these are 160 by 120 films doubled to 320 by 240, and smoothing them
/// would invent detail the original never showed.
#[allow(clippy::too_many_arguments)]
fn blit_scaled(
    dst: &mut [u32],
    dst_w: u32,
    dst_h: u32,
    src: &[u8],
    src_w: u32,
    src_h: u32,
    out_w: u32,
    out_h: u32,
    at_x: i32,
    at_y: i32,
) {
    if src_w == 0 || src_h == 0 || out_w == 0 || out_h == 0 {
        return;
    }
    // The common case is no scaling at all, and it is worth not paying for.
    if src_w == out_w && src_h == out_h {
        blit(dst, dst_w, dst_h, src, src_w, src_h, at_x, at_y);
        return;
    }
    for y in 0..out_h {
        let ty = at_y + y as i32;
        if ty < 0 || ty >= dst_h as i32 {
            continue;
        }
        let sy = (y as u64 * src_h as u64 / out_h as u64).min(src_h as u64 - 1) as u32;
        for x in 0..out_w {
            let tx = at_x + x as i32;
            if tx < 0 || tx >= dst_w as i32 {
                continue;
            }
            let sx = (x as u64 * src_w as u64 / out_w as u64).min(src_w as u64 - 1) as u32;
            let si = ((sy * src_w + sx) * 4) as usize;
            let Some(px) = src.get(si..si + 4) else { continue };
            if px[3] == 0 {
                continue;
            }
            dst[(ty as u32 * dst_w + tx as u32) as usize] =
                u32::from(px[0]) << 16 | u32::from(px[1]) << 8 | u32::from(px[2]);
        }
    }
}

#[allow(clippy::too_many_arguments)]
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
    fn going_somewhere_deliberately_abandons_the_opening() {
        // The opening queues its own `goTo #Gbhs_gameEntry` behind the film.
        // Jumping straight to a room left that queued, and it fired a moment
        // later and pulled the player back out again -- which is what broke
        // the recorded walkthrough, whose first step is a jump.
        let mut game = Game::for_test();
        game.intro_running = true;
        game.pending.push(Effect::GoToRoom {
            room: "Gbhs_gameEntry".into(),
            transition: Some("fadeIn".into()),
        });
        game.jump_to(0);
        assert!(game.pending.is_empty(), "the opening was still queued");
        assert!(!game.intro_running);
    }

    #[test]
    fn but_skipping_it_still_goes_where_the_opening_was_going() {
        // Skipping is what a click does in the original, and the `goTo` after
        // the film runs either way.
        let mut game = Game::for_test();
        game.intro_running = true;
        game.pending.push(Effect::GoToRoom {
            room: "Gbhs_gameEntry".into(),
            transition: Some("fadeIn".into()),
        });
        assert!(game.skip_intro());
        assert_eq!(game.pending.len(), 1, "the destination was thrown away");
    }

    #[test]
    fn the_flavour_on_a_move_is_the_transition_for_that_move() {
        // `goTo destination, transition` calls `setTransition( oPuppeteer,
        // transition )` in the statement before `moveToLocation`, so the
        // second argument of a move *is* its transition. Every real move in
        // the game names one and this engine armed none of them.
        for (flavour, want) in [
            ("turnLeft", Wipe::Right),
            ("turnRight", Wipe::Left),
            ("lookUp", Wipe::Down),
            ("lookDown", Wipe::Up),
            ("forward", Wipe::Dissolve),
        ] {
            let outcome = Outcome {
                destination: Some("anywhere".into()),
                transition: Some(flavour.to_string()),
                ..Outcome::default()
            };
            let mut game = Game::for_test();
            game.apply(&outcome);
            let armed = game.take_transition().expect("{flavour} armed nothing");
            assert_eq!(armed.kind, want, "{flavour}");
        }
    }

    #[test]
    fn standing_still_arms_nothing() {
        // A transition is spent on a change of the stage. An outcome that
        // only redraws must not leave one armed, or the next move would use
        // a transition that belongs to something else.
        let outcome = Outcome {
            redraw: true,
            transition: Some("turnLeft".into()),
            ..Outcome::default()
        };
        let mut game = Game::for_test();
        game.apply(&outcome);
        assert!(game.take_transition().is_none());
    }

    #[test]
    fn a_turn_is_a_quarter_of_a_second_and_a_dissolve_is_longer() {
        // The times come from the puppeteer's table, in quarter-seconds, at
        // the sixty frames a second this loop runs at.
        assert!((transition_for("turnLeft").step - 1.0 / 15.0).abs() < 1e-6);
        assert!((transition_for("forward").step - 1.0 / 30.0).abs() < 1e-6);
        assert!((transition_for("slowMontage").step - 1.0 / 45.0).abs() < 1e-6);
        // And a wipe is chunky where a dissolve is not.
        assert_eq!(transition_for("turnLeft").chunk, 16);
        assert_eq!(transition_for("forward").chunk, 0);
    }

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

/// One of Director's stage transitions, as the game's puppeteer names them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Wipe {
    /// Director's code 26, and by far the commonest: a plain crossfade.
    Dissolve,
    /// Code 1. A hard edge travelling right, so the new view enters at the
    /// left. This is a turn to the left: the world swings the other way.
    Right,
    /// Code 2. The same edge travelling left -- a turn to the right.
    Left,
    /// Code 3, looking up.
    Down,
    /// Code 4, looking down.
    Up,
}

/// An armed transition: how to make the change, and how fast.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Transition {
    pub kind: Wipe,
    /// Fraction of the change to advance each frame.
    pub step: f32,
    /// Director's chunk size, in pixels. A wipe advances in steps this wide
    /// rather than a pixel at a time, which is visible and is meant to be.
    pub chunk: u32,
}

/// The puppeteer's transition table, verbatim.
///
/// `birth` builds it as a property list of `puppetTransition` argument
/// strings -- `whichTransition, time in quarter-seconds, chunkSize,
/// changeArea` -- and `setTransition` looks a flavour up in it:
///
/// ```text
/// #turnRight   02,1,16,TRUE      #lookUp      03,1,16,TRUE
/// #turnLeft    01,1,16,TRUE      #lookDown    04,1,16,TRUE
/// #forward     26,2,0,TRUE       #fadeIn      26,2,0,TRUE
/// #lookAt      26,2,0,TRUE       #slowMontage 26,3,0,TRUE
/// #backOff     26,2,0,TRUE       #nextPage    2,2,16,TRUE
///                                #prevPage    1,2,16,TRUE
/// ```
///
/// Worth reading twice: turning is a chunky quarter-second wipe, not a
/// dissolve. Only moving forward, looking at something and backing off
/// dissolve. Rendering the turns as crossfades -- which is what this engine
/// did, because it kept the speed and threw the code away -- loses the one
/// cue that tells the player they have turned rather than teleported.
fn transition_for(name: &str) -> Transition {
    // Director's time is in quarter-seconds and the loop runs at 60.
    let at = |kind: Wipe, quarters: f32, chunk: u32| Transition {
        kind,
        step: 1.0 / (quarters * 15.0),
        chunk,
    };
    match name.to_ascii_lowercase().as_str() {
        "turnright" => at(Wipe::Left, 1.0, 16),
        "turnleft" => at(Wipe::Right, 1.0, 16),
        "lookup" => at(Wipe::Down, 1.0, 16),
        "lookdown" => at(Wipe::Up, 1.0, 16),
        "nextpage" => at(Wipe::Left, 2.0, 16),
        "prevpage" => at(Wipe::Right, 2.0, 16),
        "slowmontage" => at(Wipe::Dissolve, 3.0, 0),
        // `#forward`, `#lookAt`, `#backOff`, `#fadeIn`, and anything the
        // table does not name.
        _ => at(Wipe::Dissolve, 2.0, 0),
    }
}

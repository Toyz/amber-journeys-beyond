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
    art: HashMap<(u32, i32), Option<CachedArt>>,
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
    /// True between `suspendSounds` and `restoreSounds`, which a cutscene
    /// brackets itself with. The ghosts hold their tongues in between.
    sounds_suspended: bool,
    /// When the ghost call now sounding -- or the pause standing in for one --
    /// is over, and the next turn of the rota may take its place.
    ghost_call_until: Option<Instant>,
    /// How far through each ghost's own calls the game has got. The original
    /// keeps this in `gCurrentEntrySounds` and never resets it, so a ghost
    /// works through its recordings in order and starts again at the top.
    ghost_call_at: HashMap<String, usize>,
    /// A transition the scripts have armed for the next stage change.
    transition: Option<String>,
    /// Sprite channels a script has taken over, keyed by channel so they
    /// composite in the same back-to-front order as the room's own sprites.
    puppets: BTreeMap<u8, Puppet>,
    /// An inventory item drawn with one of its other icons, while it lasts.
    icon_override: Option<(String, usize)>,
    /// A film playing on a script-driven channel, over the room's own.
    ///
    /// Director makes no distinction: a sprite points at a cast member, and if
    /// that member happens to be a digital video the sprite plays it. The PeeK
    /// unit is built out of that -- it rolls up on one channel and then shows
    /// its recordings on the same one -- and every clip it has to show is a
    /// 128 by 96 film.
    overlay: Option<Overlay>,
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
    /// The chapter whose cast numbers the icon table is written in.
    icons_from: String,
    /// Chapters whose starting state has already been written.
    seeded: std::collections::BTreeSet<String>,
    /// The house's flags while the player is inside a chapter, which is
    /// `#StateOnIce` -- put away on the way in and taken out on the way home.
    on_ice: Option<State>,
    /// Where a chapter puts the player when it hands them back.
    reentry: Option<String>,
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
    /// Where the film was placed when it was opened.
    playing_at: Option<(i32, i32)>,
    /// Whether a sequence has taken the pointer away. Every set piece opens
    /// with `cursorOff` and the player is meant to watch it, not click through
    /// it.
    pub cursor_hidden: bool,
    pub player: Option<VideoPlayer>,
}

/// The channel the room's own sprite list starts just above.
///
/// `updateDisplay` addresses a room sprite as `lastScoreSprite + #channel`,
/// which leaves 1..12 to the frame's own furniture.
const SCORE_BASE: u16 = 12;

/// The score channel the room's `#video` sprite occupies.
///
/// Not a made-up number: `pushQT` and every handler that swaps a film write
/// `the castNum of sprite 44`, so a film a script puts on a channel and the
/// film the room declares on `#video` are the *same sprite*. Drawing both --
/// which this engine did -- puts two films on screen at once, one over the
/// other, and the one underneath shows as a sliver round the edge of the one
/// on top. Edwin's car is where it shows: `setCarLocation` loads the junction
/// film onto 44 and `driveTheCar` pushes the stretch of track over the room's
/// video channel, and both were drawn.
const MOVIE_CHANNEL: u8 = 44;

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
            playing_at: None,
            cursor_hidden: false,
            effect_wait: None,
            intro_running: false,
            sounds_suspended: false,
            ghost_call_until: None,
            ghost_call_at: HashMap::new(),
            transition: None,
            puppets: BTreeMap::new(),
            icon_override: None,
            overlay: None,
            repeating: None,
            script: Vec::new(),
            waiting: None,
            movies: MovieIndex::build(root),
            sounds: SoundBank::new(root),
            inventory: Inventory::from_texts(&[]),
            icons_from: String::new(),
            seeded: std::collections::BTreeSet::new(),
            on_ice: None,
            reentry: None,
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
                // The icon table is the same in every chapter; take the
                // first that yields one, and remember which chapter it came
                // from. The table is written as cast *numbers*, and a number
                // means something different in every movie -- 951 is "PeeK
                // color" in ROXY and "MDR-CLOCK-4.45" in MARGARET. Drawing
                // the bar out of whichever chapter the player is standing in
                // turned the inventory into a row of clock faces the moment
                // they stepped into Margaret's house.
                if game.inventory.is_empty() {
                    game.inventory = Inventory::from_texts(&texts);
                    if !game.inventory.is_empty() {
                        game.icons_from = domain.clone();
                    }
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
        // Being sent to a chapter abandons the opening, for the same reason a
        // jump does: `play <dir> MARGARET` seeds Margaret's chapter and lands
        // in her first room, and then the opening's queued `goTo` fired on the
        // second frame and pulled the player back out to the boathouse. The
        // chapter was unreachable from the command line entirely.
        //
        // Harmless later on: nothing is running to cancel once the game has
        // started, and this is called before the opening is armed at startup.
        self.cancel_intro();
        let start = self.chapter(domain).and_then(|c| {
            let schema = c.schema.as_ref()?;
            Some((schema.start_location()?.to_string(), ()))
        });

        // `enterNewDomain` does not just change rooms; it swaps the whole
        // state list, and the outer one goes in the freezer:
        //
        // ```text
        // if destination = "ROXY" then
        //   if count( states ) > 0 then
        //     lastOuterDomain = the domain being left
        //     savedRoxy = getProp( states, #StateOnIce )
        //     states <- savedRoxy
        //     quietly( me, #lastDomainVisited, lastOuterDomain )
        //     if lastOuterDomain = "MARGARET" then
        //       setProp( states, #currentLocation, [#DarkUp_40sReentry] )
        //     ... and a re-entry room for each of the other two ...
        // else
        //   storedState = states
        //   states <- value( the text of cast 'stateData' )   -- fresh
        //   addProp( states, #StateOnIce, storedState )
        // ```
        //
        // So a chapter always starts from its own declarations, and the house
        // is put back exactly as it was left. This engine had one flat state
        // and no freezer, which is what entry 154 ran into from the other
        // side: seeding on the way home wrote the declarations over the game.
        // Seeding once patched that symptom; this is the shape.
        let leaving = self.state.get("currentDomain");
        let leaving = leaving.as_str().unwrap_or_default().to_string();
        let coming_home = domain.eq_ignore_ascii_case(Self::FIRST_CHAPTER);
        match (coming_home, self.on_ice.take()) {
            (true, Some(thawed)) => {
                trace!(crate::trace::Topic::State, "thawing {domain} from the freezer");
                self.state = thawed;
                self.state
                    .set_all("lastDomainVisited", vec![lingo::Value::String(leaving.clone())]);
                // Each chapter has its own way back into the house.
                let reentry = match leaving.to_ascii_uppercase().as_str() {
                    "MARGARET" => Some("DarkUp_40sReentry"),
                    "BRICE" => Some("Ggaz_Reentry"),
                    "EDWIN" => Some("Gbhs_Reentry1"),
                    _ => None,
                };
                if let Some(room) = reentry {
                    self.state
                        .set_all("currentLocation", vec![lingo::Value::Symbol(room.into())]);
                    // And that is where the player arrives, rather than the
                    // room index the call carries: `enterNewDomain( #ROXY,
                    // 12 )` names the hall by her living room, and the house
                    // puts you back through the dark upstairs instead.
                    self.reentry = Some(room.to_string());
                }
            }
            (false, _) if !leaving.is_empty() && !leaving.eq_ignore_ascii_case(domain) => {
                trace!(crate::trace::Topic::State, "freezing {leaving} to enter {domain}");
                self.on_ice = Some(self.state.clone());
                // `states <- value( the text of cast 'stateData' )` replaces
                // the list; it does not write over it. A chapter starts with
                // its own declarations and nothing else -- which means the
                // player walks into it carrying nothing, because Roxy's tools
                // are Roxy's and they went into the freezer with her house.
                // Seeding on top of the old state left the PeeK unit, the
                // videotape and the headgear sitting in the bar in Margaret's
                // bedroom in 1943.
                self.state = State::new();
                self.seeded.remove(domain);
                self.seed_chapter(domain);
            }
            (true, None) => {
                // Home, with nothing in the freezer: a recording that starts
                // inside a chapter rather than walking to it. There is still a
                // way back in -- each chapter has its own -- and using it
                // beats landing on the opening film, which is where the
                // chapter's declared start sends you.
                self.seed_chapter(domain);
                self.reentry = match leaving.to_ascii_uppercase().as_str() {
                    "MARGARET" => Some("DarkUp_40sReentry".into()),
                    "BRICE" => Some("Ggaz_Reentry".into()),
                    "EDWIN" => Some("Gbhs_Reentry1".into()),
                    _ => None,
                };
            }
            _ => self.seed_chapter(domain),
        }

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
        // Once each. A schema is the chapter's *starting* state, and coming
        // back to a chapter already visited is not starting it again: the
        // original stashes what a domain's flags were and puts them back --
        // "Just stashed Roxy's state-data into #stateOnIce" -- rather than
        // writing the declarations over them.
        //
        // Re-seeding was quietly catastrophic in a way nothing tested for.
        // Margaret's chapter ends with `enterNewDomain( #ROXY, 12 )`, which
        // came back through here and reset every flag ROXY declares, so the
        // player arrived home having never pulled the breaker, never built
        // the BAR, and -- the one that showed -- no longer holding the
        // headgear. `#playerHasHeadgear` went back to 0, the Amber vision
        // could not be turned on again, and the portal into the second
        // chapter is only there with the vision on. The game ended, silently,
        // at the end of its first chapter.
        if self.seeded.insert(domain.to_string()) {
            if let Some(chapter) = self.chapters.get(domain) {
                if let Some(schema) = &chapter.schema {
                    schema.seed(&mut self.state);
                }
            }
        }
        // The chapter's own name for itself, which its schema declares and
        // which therefore stops being written once a chapter is only seeded
        // once. Room guards and handlers both read it, and after a chapter
        // handed the player back it still named the chapter they had left.
        self.state.set_all(
            "currentDomain",
            vec![lingo::Value::String(domain.to_string())],
        );
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

    /// Goes straight to where the opening was heading, without playing it.
    ///
    /// Distinct from `skip_intro`, which cuts short a film already running.
    /// At startup nothing has drained yet: the whole opening is still sitting
    /// in the queue, so clearing the wait only lets it play from the top and
    /// hold on `WaitForVideo` for the length of the film. The terminal hid
    /// that, because its `settle` runs the queue out ignoring waits; the
    /// window honours them, and sat on the intro for a minute and a half.
    pub fn skip_opening(&mut self) {
        if !self.intro_running {
            return;
        }
        self.cancel_intro();
        let from = self.node().domain.clone();
        if let Some(i) = self.world.resolve("Gbhs_gameEntry", Some(&from)) {
            self.move_to(i);
            self.start_room_video();
        }
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
    fn art(&mut self, domain: &str, cast: u32, ink: i32) -> Option<&CachedArt> {
        let chapter = self.chapter(domain)?;
        let key = (cast, ink);
        if !chapter.art.contains_key(&key) {
            let decoded = chapter.movie.bitmap(cast).ok().map(|b: Bitmap| {
                // The member names the palette cast it was authored against.
                let palette = chapter
                    .movie
                    .palette_for_cast(b.palette_ref)
                    .or_else(|| chapter.palettes.first().cloned())
                    .unwrap_or_default();
                // A room's own plates are ink 0 and painted whole. The
                // fifteen sprites held up in front of one -- a phone lifted to
                // the ear, a bottle turned over, a newspaper being read -- are
                // ink 36, Director's background-transparent, which keys out
                // every pixel of the background colour; index zero is white in
                // every one of this game's palettes and that is the field they
                // are drawn on.
                //
                // Ink 8 is `#matte`, and it is not the same thing. Matte keys
                // out the background *outside the shape*, from a mask derived
                // from the member's outline, so a slab of the background
                // colour in the middle of the art stays painted. Treating the
                // two alike was fine until the PeeK unit, whose body is
                // exactly such a slab: keying on the colour punched its middle
                // out and left a frame with the room showing through it.
                let background = b.background();
                let rgba = match ink {
                    0 => b.to_rgba(&palette, None),
                    36 => b.to_rgba(&palette, Some(background)),
                    _ => b.to_rgba_matte(&palette, background),
                };
                CachedArt {
                    rgba,
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
        // `pushVideo` writes the video channel, so whatever a script had put
        // on that channel is replaced -- they are one sprite. Without this the
        // junction film Edwin's car parks at a hub went on covering the
        // stretch of track being driven.
        self.release_overlay(MOVIE_CHANNEL);
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
                // And where it sits. A pushed film goes on the room's video
                // channel, and in Director a channel's *position* is a score
                // property that does not care whether the sprite's `#showIF`
                // holds -- the guard decides which film is on it, not where it
                // is. So the room's declared coords are taken with the guard
                // preferred and without it as a fallback.
                //
                // The car is where this shows. `car_inside` declares
                // `carBack.mov` at (322, 204) behind `[#showMontage, 3]`, and
                // once the drive starts the montage is 0: the guard stops
                // holding, the coords went with it, and the track films drew
                // centred on the stage instead of in the windscreen.
                self.playing_at = self.video_channel_centre();
            }
            // `pushVideo` with nothing named plays whatever the room has on
            // its video channel. It is almost always preceded by a `setState`
            // and an `updateDisplay` that choose *which* film that is -- and
            // the redraw has already started it by the time this runs, so
            // starting it again played it twice. The weathervane is where it
            // shows: three pulls on the rope, and each one ran its film
            // through and then ran it through again.
            None => {
                let already = self.video() == self.playing;
                let running = self.player.as_ref().is_some_and(|p| !p.finished);
                if !already || !running {
                    self.start_room_video();
                }
            }
        }
    }

    /// Loads and starts the current room's movie, if it has one.
    pub fn start_room_video(&mut self) {
        self.release_overlay(MOVIE_CHANNEL);
        self.player = None;
        let Some(name) = self.video() else {
            self.playing = None;
            self.playing_size = None;
            self.playing_at = None;
            return;
        };
        // The sprite whose guard holds now, kept for as long as the film runs.
        // A room can declare several films on the video channel, each gated on
        // a different state and each with its own `#coords`: the study puts
        // the headgear films at (303, 220) and the film of the oscillator
        // being fitted at (317, 185).
        let state = &self.state;
        self.playing_at = self.world.nodes[self.room]
            .sprites
            .iter()
            .find(|s| matches!(s.channel, Channel::Video) && state.test(&s.condition))
            .and_then(|s| s.center);
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
            if !self.wait_satisfied(wait, true) {
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
        // Nothing can proceed while the queue is holding for a click, and
        // clearing it here would take the click out of the player's hands --
        // which is the whole of what entry 150 was about.
        if self.waiting_for_click() {
            return report;
        }
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
            // there is no clock here, so they are named and stepped over --
            // all but one.
            //
            // A click wait is not the clock's, it is the player's, and
            // stepping over it here is what made a recording written in the
            // terminal wrong in the window. The terminal needed no click to
            // dismiss a playback and the window did, so from the first one
            // onwards every step landed one out of phase: `full.walk` opened
            // the PeeK and then spent the next step putting it away rather
            // than walking, and the route came apart from there. Both front
            // ends now want the same clicks.
            while !self.pending.is_empty() {
                if matches!(self.pending.first().and_then(wait_for), Some(Wait::Click)) {
                    self.pending.remove(0);
                    self.effect_wait = Some(Wait::Click);
                    report.push("wait for a click".into());
                    return report;
                }
                let effect = self.pending.remove(0);
                // One formatter for both front ends, so the timeline the
                // walkthrough prints and the timeline `--strict` prints cannot
                // say different things about the same queue. The room's own
                // film is asked for per effect rather than once: a sequence
                // that steps a flag between two `pushVideo`s plays two
                // different films, and reading it up front named the first one
                // twice -- or, at the end of the game, neither.
                let room_film = self.video();
                if let Some(line) = describe(&effect, room_film.as_deref()) {
                    report.push(line);
                }
                self.apply_puppet(&effect);
            }
            self.effect_wait = None;
            if self.script.is_empty() {
                break;
            }
            // The other half of the same rule. A sequence can hold for a click
            // between two of its actions rather than between two of its
            // effects, and `waiting_for_click` has always had to look in both
            // places; so does this.
            if matches!(self.waiting, Some(Wait::Click)) {
                report.push("wait for a click".into());
                return report;
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

    /// Each ghost's calls, in the order the game lists them.
    ///
    /// String order, not numeric -- the authors built these from a directory
    /// listing, so `Mcall1` is followed by `Mcall10` and then `Mcall2`. A
    /// ghost works through its own list in that order, which is why the calls
    /// come in a fixed sequence rather than at random.
    const GHOST_CALLS: [(&'static str, &'static [&'static str]); 3] = [
        (
            "Margaret",
            &[
                "Mcall1", "Mcall10", "Mcall2", "Mcall3", "Mcall4", "Mcall5", "Mcall6", "Mcall7",
                "Mcall8", "Mcall9",
            ],
        ),
        (
            "Brice",
            &[
                "Bcall1", "Bcall10", "Bcall11", "Bcall2", "Bcall3", "Bcall4", "Bcall5", "Bcall6",
                "Bcall7", "Bcall8", "Bcall9",
            ],
        ),
        (
            "Edwin",
            &[
                "Ecall1", "Ecall10", "Ecall11", "Ecall12", "Ecall2", "Ecall3", "Ecall4", "Ecall5",
                "Ecall6", "Ecall7", "Ecall8", "Ecall9",
            ],
        ),
    ];

    /// Lets whichever ghost's turn it is call, once a frame.
    ///
    /// This is `playDomainEntrySound`, which the original runs from `idle`:
    ///
    /// ```text
    /// on playDomainEntrySound
    ///   if gSoundsSuspended = 1 then return
    ///   batterUp = getState( #ghostsCalling )
    ///   ... if a call is still sounding on its channel then return ...
    ///   soundList = gEntrySoundFiles[ batterUp ]
    ///   newSound  = ( gCurrentEntrySounds[ batterUp ] mod count(soundList) ) + 1
    ///   if batterUp = #nobody then waitaSec( #start )
    ///   else soundEffect( getAt(soundList, newSound), getState(#ghostCallVol) )
    ///   gCurrentEntrySounds[ batterUp ] = newSound
    ///   -- rotate: the last of the list becomes the first
    ///   if count( #ghostsCalling ) > 1 then
    ///     addAt( stateList, 1, getLast(stateList) )
    ///     deleteAt( stateList, count(stateList) )
    /// ```
    ///
    /// The ghosts telephone the player, and it is the whole of the game's
    /// signposting -- how anyone learns there is somewhere to go. Fifty-seven
    /// room scripts call `ghostCalls` to say who is calling from where and how
    /// loudly, and nothing in this engine ever ran the other half. The ghosts
    /// have been silent for the entire project.
    ///
    /// `#nobody` in the rota is a one-second pause rather than a sound, which
    /// is what the padding in `ghostCalls` is: an entry call lands every turn,
    /// a warm one one turn in three, a cool one one in four. The rotation
    /// moves the *last* entry to the front rather than stepping forward, so a
    /// list of three ghosts and three pauses gives one call, three seconds of
    /// quiet, then two calls together.
    /// Counts a move and lets the house haunt the player, as `goTo` does.
    ///
    /// ```text
    /// lsMoveCounter = getProp( oStoryteller.states, #moveCount )
    /// setAt( lsMoveCounter, 1, getAt( lsMoveCounter, 1 ) + 1 )
    /// ...
    /// if getState( #BarOnline ) and getState( #PeekDisplay ) = #None then
    ///   showTime = getState( #hauntDelay )
    ///   if getAt( lsMoveCounter, 1 ) > showTime
    ///      and destination <> #LivingRmBarCU2 then
    ///     spawnGhostlyEvent()
    ///     setProp( oStoryteller.states, #hauntDelay, list( max( 0, showTime - 4 ) ) )
    /// ```
    ///
    /// This is the clock the six camera haunts run on, and it is the whole
    /// second act's gate: the telephone only rings once every haunt has been
    /// caught and watched back, and until the telephone rings there is no
    /// headgear, no Amber vision, and no way into any of the three chapters.
    /// None of it was ported, so the house was silent however far it was
    /// walked, and I had put the missing piece down to the haunts arriving
    /// "on their own clock". They arrive on this one.
    ///
    /// `#hauntDelay` opens at 60 and comes down by four each time, so the
    /// first haunt is a long way into the house and they quicken after that.
    /// The bar's own close-up is excluded because that is where the player
    /// goes to watch one.
    fn count_move(&mut self, destination: &str) {
        let count = self.state.get("moveCount").as_int().unwrap_or(0) + 1;
        self.state.set_all("moveCount", vec![lingo::Value::Int(count)]);

        if !self.state.get("BarOnline").truthy() {
            return;
        }
        if !self.state.get("PeekDisplay").is_symbol("None") {
            return;
        }
        let delay = self.state.get("hauntDelay").as_int().unwrap_or(0);
        if count <= delay || destination.eq_ignore_ascii_case("LivingRmBarCU2") {
            return;
        }
        if self.spawn_ghostly_event() {
            self.state
                .set_all("hauntDelay", vec![lingo::Value::Int((delay - 4).max(0))]);
        }
    }

    /// Puts the first haunt the player is not standing in front of onto the
    /// PeeK unit.
    ///
    /// Each of the six names the area it happens in, and some of them name
    /// doorways as well -- a haunt in the kitchen is not offered from the
    /// dining room's entry to it either, because the point of the recording
    /// is that it happened where nobody was looking. The living room's is
    /// held back from the study too, which is the room its camera is watched
    /// from.
    ///
    /// The list is walked in order and the first eligible one wins, so which
    /// haunt arrives depends on where the player is when the counter comes
    /// round.
    fn spawn_ghostly_event(&mut self) -> bool {
        // `#ghostKnife` and `#KdKnob` are both in the kitchen and share a
        // list of doorways it can be seen from.
        const KITCHEN_DOORS: [&str; 7] = [
            "DiningRmKitchenEntry2",
            "HallKitchenEntryOpen",
            "Ghse_D_S",
            "Ghse_D_W",
            "Ghse_E_W",
            "Ghse_P_KitchenDoorCU",
            "Ghse_P_KitchenEntry",
        ];
        const LIVING_ROOM_DOORS: [&str; 4] =
            ["HallLivingRmEntry", "HallNwall", "HallExit", "PorchDoorCU"];

        let here = self.node().name.clone().unwrap_or_default();
        let zone = self.node().zone.clone().unwrap_or_default();
        let elsewhere = |area: &str| !zone.eq_ignore_ascii_case(area);
        let not_at = |rooms: &[&str]| !rooms.iter().any(|r| r.eq_ignore_ascii_case(&here));

        let waiting: Vec<String> = self
            .state
            .get_all("cameraFeedbackRemaining")
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.trim_start_matches('#').to_string()))
            .collect();

        for haunt in waiting {
            let eligible = match haunt.to_ascii_lowercase().as_str() {
                "ghostknife" | "kdknob" => elsewhere("kitchen") && not_at(&KITCHEN_DOORS),
                "crazylr" => {
                    elsewhere("livingRm") && elsewhere("Study") && not_at(&LIVING_ROOM_DOORS)
                }
                "crazydr" => {
                    elsewhere("diningRm")
                        && elsewhere("Study")
                        && not_at(&["KitchenDiningRmEntry"])
                }
                "ghostlykey" => elsewhere("MBR") && not_at(&["UHallMasterBedrmEntry"]),
                "bloodbath" => elsewhere("Marg") && not_at(&["UHallMargRoomEntry"]),
                _ => false,
            };
            if eligible {
                trace!(
                    crate::trace::Topic::Script,
                    "a camera caught {haunt}; the PeeK has it"
                );
                self.state
                    .set("PeekDisplay", lingo::Value::Symbol(haunt.clone()));
                return true;
            }
        }
        false
    }

    pub fn tick_ghost_call(&mut self) {
        // `idle` puts this behind `getState( #AMBERVISION ) = #on`, and the
        // hint book agrees: the calls begin once the headgear is on and
        // calibrated, and they are what lead the player to the domain entry
        // tunnels. Ungated, the ghosts start telephoning from the boathouse
        // path before there is anything to be led to.
        if !self.state.get("AMBERVISION").is_symbol("on") {
            return;
        }
        // `if gSoundsSuspended = 1 then return`, so a cutscene is not talked
        // over by a ghost part way through.
        if self.sounds_suspended {
            return;
        }
        if self.ghost_call_until.is_some_and(|t| Instant::now() < t) {
            return;
        }
        let rota = self.state.get_all("ghostsCalling").to_vec();
        let Some(batter) = rota.first().and_then(|v| v.as_str()).map(|s| {
            s.trim_start_matches('#').to_string()
        }) else {
            return;
        };

        // The last becomes the first, which is how the original steps on.
        if rota.len() > 1 {
            let mut next = rota;
            if let Some(last) = next.pop() {
                next.insert(0, last);
            }
            self.state.set_all("ghostsCalling", next);
        }

        if batter.eq_ignore_ascii_case("nobody") {
            self.ghost_call_until = Some(Instant::now() + Duration::from_secs(1));
            return;
        }

        let Some((_, files)) = Self::GHOST_CALLS
            .iter()
            .find(|(who, _)| who.eq_ignore_ascii_case(&batter))
        else {
            return;
        };
        let cursor = self.ghost_call_at.entry(batter).or_insert(0);
        *cursor = (*cursor + 1) % files.len();
        let name = files[*cursor].to_string();

        // Hold the rota until this call has finished, which is the original's
        // `soundBusy` check on the channel it went out on.
        let secs = self
            .sound(&name)
            .map(|(pcm, rate, ch)| pcm.len() as f64 / (rate.max(1) * ch.max(1) as u32) as f64)
            .unwrap_or(0.0);
        self.ghost_call_until = Some(Instant::now() + Duration::from_secs_f64(secs));

        let loudness = match self.state.get("ghostCallVol").as_int().unwrap_or(180) {
            v if v <= 90 => "low",
            v if v <= 180 => "medium",
            _ => "high",
        };
        trace!(
            crate::trace::Topic::Audio,
            "ghost call {name} at {loudness} ({secs:.1}s)"
        );
        // `gLastCall` is how the original finds the channel again, both to
        // ask whether the call is still going and to take it down.
        self.state.set("gLastCall", lingo::Value::String(name.clone()));
        self.pending.push(Effect::PlaySound {
            name,
            loudness: Some(loudness.into()),
        });
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

    /// Everything that would be drawn, bottom to top, as text.
    ///
    /// The terminal cannot see the stage, and a fault that is only visible --
    /// a film at the wrong size, a puppet left over from a sequence that has
    /// finished -- has until now had to be diagnosed from a photograph. This
    /// is the compositor's own layer list, in the order it paints.
    pub fn stage_report(&mut self) -> Vec<String> {
        let mut out = Vec::new();
        let mut rows: Vec<(u16, String)> = Vec::new();
        for (ch, cast, center, ink) in self.visible() {
            rows.push((
                12 + ch as u16,
                format!("room sprite ch{ch} cast {cast} ink {ink} at {center:?}"),
            ));
        }
        let channel_centre = self.video_channel_centre();
        let overlay_owns_video = self
            .overlay
            .as_ref()
            .is_some_and(|o| o.channel == MOVIE_CHANNEL);
        if let Some(p) = &self.player {
            let (w, h) = p.frame_size();
            let (dw, dh) = self.playing_size.unwrap_or((w, h));
            rows.push((
                MOVIE_CHANNEL as u16,
                format!(
                    "film {} {w}x{h} drawn {dw}x{dh} at {:?}{}{}",
                    self.playing.clone().unwrap_or_else(|| "(none)".into()),
                    self.playing_at,
                    if p.finished { " (ended)" } else { "" },
                    if overlay_owns_video {
                        "  -- not drawn: a script has the video channel"
                    } else {
                        ""
                    }
                ),
            ));
        }
        if let Some(o) = &self.overlay {
            let (w, h) = o.player.frame_size();
            let at = self
                .puppets
                .get(&o.channel)
                .and_then(|p| p.loc)
                .or(if o.channel == MOVIE_CHANNEL {
                    self.playing_at.or(channel_centre)
                } else {
                    None
                });
            rows.push((
                o.channel as u16,
                format!(
                    "overlay film on ch{} {w}x{h} drawn {:?} at {at:?}{}",
                    o.channel,
                    o.size,
                    if o.parked {
                        " (a still, not playing)"
                    } else if o.player.finished {
                        " (ended)"
                    } else {
                        ""
                    }
                ),
            ));
        }
        for (ch, puppet) in self.puppets.iter() {
            if puppet.cast == 0 || puppet.hidden {
                continue;
            }
            rows.push((
                *ch as u16,
                format!(
                    "puppet ch{ch} cast {} ink {} at {:?}",
                    puppet.cast, puppet.ink, puppet.loc
                ),
            ));
        }
        rows.sort_by_key(|(ch, _)| *ch);
        for (_, line) in rows {
            out.push(line);
        }
        out
    }

    /// What the queue and the running script look like, for the strict
    /// replay's report when a recording stops making progress.
    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    /// The wait the queue itself is holding on, named.
    pub fn armed_wait(&self) -> String {
        name_wait(self.effect_wait.as_ref())
    }

    /// How many lines of the current hotspot script are still to run.
    pub fn script_len(&self) -> usize {
        self.script.len()
    }

    /// The wait a part-run script is holding on, between two of its actions.
    pub fn held_wait(&self) -> String {
        name_wait(self.waiting.as_ref())
    }

    /// Runs the current film out, for a front end with no clock of its own.
    ///
    /// A film that loops is left alone: most of them are scenery -- the mirror
    /// in Margaret's doorway turns for as long as the player stands there --
    /// and one that something is waiting on has its loop taken off by
    /// `drain_ready` on the pass after the wait is armed. So this says nothing
    /// about deadlock; a queue that will not move is caught by the caller
    /// running out of turns, which is the only honest test.
    pub fn end_film(&mut self) {
        if let Some(p) = &mut self.player {
            if !p.loops() {
                p.finished = true;
            }
        }
        if let Some(o) = &mut self.overlay {
            if !o.player.loops() {
                o.player.finished = true;
            }
        }
    }

    /// Brings a tick wait forward to now.
    ///
    /// The strict replay has no clock: a `wait 30` is thirty sixtieths of a
    /// second the window really spends and this front end cannot. Only the
    /// clock is skipped -- a film wait, a sound wait and a click wait are all
    /// left exactly as they are, because those are the ones that deadlock.
    pub fn fast_forward_ticks(&mut self) {
        let now = Instant::now();
        for wait in [&mut self.effect_wait, &mut self.waiting] {
            if let Some(Wait::Until(t)) = wait {
                *t = now;
            }
        }
    }

    /// Drains whatever is due now and says what it carried.
    ///
    /// `settle` steps over waits; this does not. It is the window's drain with
    /// the walkthrough's report on top, so a strict replay can show the same
    /// timeline the terminal prints without also stepping over the thing that
    /// is stuck.
    pub fn drain_ready_report(&mut self) -> Vec<String> {
        let effects = self.drain_ready();
        let mut report = Vec::new();
        for effect in &effects {
            let room_film = self.video();
            if let Some(line) = describe(effect, room_film.as_deref()) {
                report.push(line);
            }
            // `drain_ready` hands the effects back for the caller to act on --
            // the window's `apply_effect` is what plays them. Reporting them
            // without applying them was a front end that watched its own queue
            // go past: every state write in a sequence was reported and none
            // of them happened.
            self.apply_puppet(effect);
        }
        report
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
            Effect::ReplaceState { key, value } => {
                self.state.set_all(key, vec![value.clone()]);
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
                        self.count_move(room);
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
            // Leaving the spot a ghost calls from stops it mid-call, and
            // frees the rota so the next room's ghosts can start at once.
            Effect::StopGhostCall => self.ghost_call_until = None,
            Effect::SuspendSounds { .. } => self.sounds_suspended = true,
            Effect::RestoreSounds { .. } => self.sounds_suspended = false,
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
                    // Releasing a channel has to stop whatever it was
                    // playing. The PeeK unit ends on `PkBlank.mov` in its
                    // screen, so letting the film outlive the channel left
                    // the unit's screen hanging in the middle of the room
                    // after the unit itself had gone.
                    self.release_overlay(*channel);
                }
            }
            Effect::SpriteCast { channel, cast } => self.point_channel(*channel, *cast),
            Effect::SpriteLoc { channel, x, y } => {
                let p = self.puppets.entry(*channel).or_default();
                p.loc = Some((*x, *y));
            }
            Effect::PlayOverlay { channel } => {
                if let Some(o) = &mut self.overlay {
                    if o.channel == *channel {
                        o.player.restart();
                        o.started = false;
                        o.parked = false;
                    }
                }
            }
            Effect::SpriteCastNamed { channel, name } => {
                if let Some(cast) = self.presentation_cast(name) {
                    self.point_channel(*channel, cast);
                }
            }
            Effect::SpriteCastFromTable { channel, table, key } => {
                if let Some(cast) = self.cast_lookup(table, &lingo::Value::Symbol(key.clone())) {
                    self.point_channel(*channel, cast);
                }
            }
            Effect::SpriteVisible { channel, visible } => {
                self.puppets.entry(*channel).or_default().hidden = !*visible;
            }
            Effect::SpriteInk { channel, ink } => {
                self.puppets.entry(*channel).or_default().ink = *ink;
            }
            Effect::InventoryIcon { item, index } => {
                self.icon_override = index.map(|i| (item.clone(), i));
            }
            Effect::ParkSpareSprites => self.park_spare_sprites(),
            Effect::EnterDomain { domain, room } => {
                let outcome = Outcome {
                    new_domain: Some(domain.clone()),
                    new_domain_room: *room,
                    ..Outcome::default()
                };
                self.apply(&outcome);
            }
            _ => return false,
        }
        true
    }

    /// Points a script-driven channel at a cast member.
    ///
    /// Director draws whatever the member is, and a digital video member
    /// plays. The PeeK unit is built out of exactly that: sprite 44 first
    /// holds `PeeKup.mov`, the unit sliding up into view, and then becomes the
    /// little screen the recordings play in. Setting the number and leaving it
    /// to the bitmap decoder drew nothing at all, because a film has no
    /// bitmap -- so the unit opened, said what it had, and showed a blank.
    /// Runs `initInventory`'s homecoming branch for the room just arrived in.
    fn close_chapter(&mut self, room: &str) {
        let mut out = Outcome::default();
        let done = crate::natives::call(
            "closechapter",
            &[lingo::Value::Symbol(room.to_string())],
            &mut self.state,
            &mut out,
        );
        if done {
            trace!(crate::trace::Topic::Room, "closing the chapter at {room}");
            self.apply(&out);
        }
    }

    /// Where the room puts its `#video` channel.
    ///
    /// In Director a channel's position is a score property: the `#showIF`
    /// decides which film is on the channel, not where the channel is. So the
    /// sprite whose guard holds is preferred and the first video sprite stands
    /// in when none of them holds -- which is the usual case while a script is
    /// playing a film of its own over the room's.
    fn video_channel_centre(&self) -> Option<(i32, i32)> {
        let state = &self.state;
        let video: Vec<_> = self.world.nodes[self.room]
            .sprites
            .iter()
            .filter(|s| matches!(s.channel, Channel::Video))
            .collect();
        video
            .iter()
            .find(|s| state.test(&s.condition))
            .or_else(|| video.first())
            .and_then(|s| s.center)
    }

    fn point_channel(&mut self, channel: u8, cast: u32) {
        let domain = self.node().domain.clone();
        let film = self
            .chapter(&domain)
            .and_then(|c| c.movie.member(cast))
            .filter(|m| m.kind == director::CastKind::DigitalVideo)
            .map(|m| (m.name.clone().unwrap_or_default(), m.width, m.height, m.loops));

        let Some((name, width, height, loops)) = film else {
            // An ordinary member: the channel draws it, and anything the
            // channel was playing stops.
            if self.overlay.as_ref().is_some_and(|o| o.channel == channel) {
                self.overlay = None;
            }
            self.puppets.entry(channel).or_default().cast = cast;
            return;
        };

        self.puppets.entry(channel).or_default().cast = 0;
        match self.movies.find(&name) {
            Some(path) => {
                trace!(
                    crate::trace::Topic::Video,
                    "channel {channel} plays {name} -> {}",
                    path.display()
                );
                let mut player = VideoPlayer::open(path);
                if let Some(p) = &mut player {
                    p.set_looping(loops);
                }
                self.overlay = player.map(|mut player| {
                    // Pointing a channel at a film shows a frame. Playing it
                    // is a separate thing a handler asks for, and one of them
                    // deliberately does not: `setCarLocation` puts the
                    // junction film on the car's channel and leaves it
                    // standing, waiting for `chooseTrack` to scrub a third of
                    // it. Starting it here played the drive again every time
                    // the car reached a hub, on a loop.
                    player.park();
                    player
                })
                .map(|player| Overlay {
                    channel,
                    parked: true,
                    player,
                    size: Some((width as u32, height as u32)).filter(|(w, h)| *w > 0 && *h > 0),
                    started: false,
                });
            }
            None => {
                trace!(crate::trace::Topic::Video, "no file for movie {name}");
                self.overlay = None;
            }
        }
    }

    /// Advances a film on a script-driven channel, as the window does for the
    /// room's own. Answers whether the stage needs redrawing.
    pub fn tick_overlay(&mut self) -> bool {
        self.overlay.as_mut().is_some_and(|o| o.player.tick())
    }

    /// The soundtrack of a film on a script-driven channel, once.
    ///
    /// The PeeK's recordings have sound on them, and `usePeekUnit` reads and
    /// restores the video sprite's volume around the roll-up rather than
    /// muting it, so they are meant to be heard.
    pub fn take_overlay_audio(&mut self) -> Option<(std::sync::Arc<Vec<i16>>, u32, u16)> {
        let o = self.overlay.as_mut()?;
        if o.started {
            return None;
        }
        o.started = true;
        Some((
            o.player.audio_for_segment(),
            o.player.audio_rate,
            o.player.audio_channels,
        ))
    }

    /// Stops a film on a channel, if that is what is on it.
    fn release_overlay(&mut self, channel: u8) {
        if self.overlay.as_ref().is_some_and(|o| o.channel == channel) {
            self.overlay = None;
        }
    }

    /// Releases every claimed channel, which a room change does.
    pub fn clear_puppets(&mut self) {
        self.puppets.clear();
        self.overlay = None;
    }

    /// Clears the channels the room did not place, as `updateDisplay` does.
    ///
    /// The game never un-puppets a puzzle's pieces. It composes the stage from
    /// the room's own sprite list and then walks from just past the last one
    /// it placed up to sprite 37, blanking each and parking it off to one
    /// side. So a piece stays on the stage exactly as long as no other
    /// composition happens, and the next `updateDisplay` sweeps it away.
    ///
    /// The telegram is where this shows: solving it sets `#showMontage` and
    /// calls `updateDisplay`, which is what takes the twelve tiles down and
    /// leaves the whole telegram behind them. Without the sweep the tiles
    /// stayed up, a couple of pixels off the picture they were laid over, and
    /// every line of the message read twice.
    fn park_spare_sprites(&mut self) {
        const LAST_SWEPT: u16 = 37;
        let last_placed = self.world.nodes[self.room]
            .sprites
            .iter()
            .filter_map(|s| match s.channel {
                Channel::Sprite(n) => Some(SCORE_BASE + u16::from(n)),
                _ => None,
            })
            .max()
            .unwrap_or(SCORE_BASE);
        self.puppets
            .retain(|ch, _| u16::from(*ch) <= last_placed || u16::from(*ch) > LAST_SWEPT);
        if let Some(o) = &self.overlay {
            let ch = u16::from(o.channel);
            if ch > last_placed && ch <= LAST_SWEPT {
                self.overlay = None;
            }
        }
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

        enum Layer {
            /// A cast member from the room or from a puppet channel.
            Art {
                cast: u32,
                at: Option<(i32, i32)>,
                /// Director's ink, which decides how the background is
                /// treated: 0 paints it, 36 keys it out, 8 mattes it.
                ink: i32,
            },
            Movie,
            /// A film a script put on its own channel, drawn in that
            /// channel's place rather than with the room's film.
            Overlay,
        }

        let mut stage: Vec<(u16, Layer)> = Vec::new();
        for (ch, cast, center, ink) in self.visible() {
            stage.push((
                SCORE_BASE + ch as u16,
                Layer::Art { cast, at: center, ink },
            ));
        }
        // One sprite, one film. When a script has put a film on the video
        // channel it has replaced whatever the room had there, so only the
        // overlay is drawn.
        let overlay_owns_video = self
            .overlay
            .as_ref()
            .is_some_and(|o| o.channel == MOVIE_CHANNEL);
        if self.player.is_some() && !overlay_owns_video {
            stage.push((MOVIE_CHANNEL as u16, Layer::Movie));
        }
        if let Some(o) = &self.overlay {
            stage.push((o.channel as u16, Layer::Overlay));
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
                    ink: puppet.ink,
                },
            ));
        }
        // A stable sort keeps the room's own order within a channel, which is
        // how two plates sharing one channel stack.
        stage.sort_by_key(|(ch, _)| *ch);

        let domain = self.node().domain.clone();
        // The video sprite whose guard holds, not merely the first one. A
        // room can declare several films on the video channel, each gated on
        // a different state and each with its own `#coords`, and `video()`
        // already picks between them by exactly this test -- so taking the
        // first one's position put the film that was playing wherever a
        // different film would have gone.
        //
        // The study is the case that shows it: `HGup.mov` and `HGdown.mov`
        // sit at (303, 220) and `oslator1.mov`, the film of the oscillator
        // being fitted, at (317, 185). Placing the oscillator played the
        // right film in the headgear's place.
        // Where it was placed when it was opened, not where the guards say it
        // would go now. A sequence sets the flag its film is gated on and
        // *then* waits for the film -- fitting the oscillator is exactly that:
        //
        // ```text
        // setState( #oscillatorInPlace, #placingNow )  -- the film's guard
        // pushVideo : wait #videoStop
        // setState( #oscillatorInPlace, TRUE )         -- and it is gone
        // ```
        //
        // so re-deriving the position every frame meant the film played in the
        // AMBER device's slot and then, on the last frame or two, jumped to
        // the middle of the stage when no sprite's guard held any longer.
        let video_centre = self.playing_at;
        // And where the video channel sits when no film of the room's is open,
        // which is what a film a script put there has to fall back on.
        let channel_centre = self.video_channel_centre();

        for (channel, layer) in stage {
            match layer {
                Layer::Art { cast, at, ink } => {
                    let Some(art) = self.art(&domain, cast, ink) else {
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
                Layer::Overlay => {
                    let Some(o) = &self.overlay else { continue };
                    let (w, h) = o.player.frame_size();
                    let (dw, dh) = o.size.filter(|(a, b)| *a > 0 && *b > 0).unwrap_or((w, h));
                    // Where the script put the channel, by registration point.
                    // A film member's registration point is its centre, which
                    // is what `set the loc of sprite 44 = point(317, 132)`
                    // means: the middle of the PeeK unit's little screen.
                    // Where the channel is. A script that swaps the film on
                    // a channel does not move it, so a channel with no loc of
                    // its own keeps the place the room gave it -- and for the
                    // video channel that is the room's `#video` coords, not
                    // the middle of the stage. The car's junction film drew
                    // centred on the windscreen's plate instead of in the
                    // windscreen because of this.
                    let centre = self
                        .puppets
                        .get(&o.channel)
                        .and_then(|p| p.loc)
                        .or(if o.channel == MOVIE_CHANNEL {
                            video_centre.or(channel_centre)
                        } else {
                            None
                        })
                        .unwrap_or((width as i32 / 2, height as i32 / 2));
                    trace!(
                        crate::trace::Topic::Sprite,
                        "draw ch{channel} overlay {w}x{h} as {dw}x{dh} at {centre:?}"
                    );
                    blit_scaled(
                        frame,
                        width,
                        height,
                        o.player.frame(),
                        w,
                        h,
                        dw,
                        dh,
                        centre.0 - dw as i32 / 2,
                        centre.1 - dh as i32 / 2,
                    );
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
            playing: None,
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
        let (group, item, gain, finished) = {
            let p = self.program.as_mut()?;
            let item = p.order[p.next % p.order.len()].clone();
            let finished = p.playing.replace(item.clone());
            p.next = p.next.wrapping_add(1);
            (p.group.clone(), item, p.gain, finished)
        };

        // Hearing one of the dining room's announcements out is what starts
        // Margaret's clock puzzle. `prodVLoops` watches the elapsed time of
        // the sound against a hard-coded length -- 707 ticks for `#news`, 946
        // for `#buster` -- and fires once it is within a second of the end:
        //
        //   if sndElapsedTime > sndLength - 60
        //      and sndElapsedTime < sndLength + 300 then
        //     setState( #clockPuzzleActivated, 1 )
        //     if getState( #clockTime ) = #t7 then
        //       addState( #tunedIn, #livingRm )
        //
        // Here the programme already knows when an item ends, because that is
        // what schedules the next one, so this fires as the announcement
        // gives way rather than on a measured deadline. She tells you there
        // is something wrong with the clocks, and from then on the clocks
        // will listen.
        if group.eq_ignore_ascii_case("DRradio") {
            if let Some(heard) = finished {
                if ["news", "buster"].iter().any(|a| heard.eq_ignore_ascii_case(a)) {
                    trace!(
                        crate::trace::Topic::Script,
                        "heard {heard} out: the clock puzzle is live"
                    );
                    self.state.set("clockPuzzleActivated", lingo::Value::Int(1));
                    if self.state.get("clockTime").is_symbol("t7") {
                        self.state
                            .add_item("tunedIn", lingo::Value::Symbol("livingRm".into()));
                    }
                }
            }
        }

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
        self.art(&domain, cast, 0).is_some()
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
        let Some(art) = self.art(&domain, image, 0).cloned() else {
            return false;
        };
        let Some(shape) = self.art(&domain, mask, 0).cloned() else {
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
    /// Draws the inventory bar.
    ///
    /// `hot` says whether the cursor is over the bar. `updateInventory` takes
    /// the first of an item's two icons when it is and the second when it is
    /// not -- full colour under the cursor, a glowing outline away from it,
    /// which is the first thing the hint book teaches a new player about the
    /// interface.
    ///
    /// This engine had the pair standing for something else entirely: the
    /// second icon marked the item in hand and the first everything else, so
    /// the bar never changed as the cursor moved and the outline art appeared
    /// on exactly the one item that should not have been in the bar at all.
    /// The item in hand is on the cursor; `updateInventory` moves its sprite
    /// off the stage.
    pub fn draw_inventory(&mut self, frame: &mut [u32], width: u32, height: u32, hot: bool) {
        let slots: Vec<(usize, String)> = self
            .state
            .slots()
            .map(|(n, item)| (n, item.to_string()))
            .collect();
        let in_use = self.state.item_in_use().map(|s| s.to_ascii_lowercase());
        let placed = self
            .inventory
            .layout(slots.into_iter(), width as i32, height as i32);
        // Not the room's chapter: the icons are cast numbers in the chapter
        // the table was read from.
        let domain = self.icons_from.clone();

        for (item, x, y) in placed {
            if in_use.as_deref() == Some(item.to_ascii_lowercase().as_str()) {
                continue;
            }
            let Some(icons) = self.inventory.icons(&item) else { continue };
            // `peekAlert` pulses the middle slot between the two glows, so an
            // override beats the cursor's own hot/cool choice while it runs.
            let swapped = self
                .icon_override
                .as_ref()
                .filter(|(name, _)| name.eq_ignore_ascii_case(&item))
                .and_then(|(_, i)| self.inventory.icon_at(&item, *i));
            let cast = swapped.unwrap_or(if hot { icons.hot } else { icons.cool });
            // Keyed on the field the icon lays around itself, which is index
            // 255 rather than the 0 a room's plates use. Painted whole, the
            // bar was three black boxes sitting over the bottom of the room.
            let Some(art) = self.art(&domain, cast, 36) else { continue };
            let (w, h) = (art.width, art.height);
            blit(frame, width, height, &art.rgba, w, h, x, y);
        }
    }

    /// Handles a click on the inventory bar, returning true if it was one.
    ///
    /// Clicking an item takes it in hand; clicking the item already in hand
    /// puts it back, which is what `stowInventory` does from script.
    pub fn click_inventory(&mut self, x: i32, y: i32, width: i32, height: i32) -> bool {
        let slots: Vec<(usize, String)> = self
            .state
            .slots()
            .map(|(n, item)| (n, item.to_string()))
            .collect();
        let Some(item) = self.inventory.hit(slots.into_iter(), width, height, x, y) else {
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
            // Through `useInventory`, so the PeeK unit opens when it is
            // picked out of the bar. That is what the hint book means by
            // "whenever the PeeK flashes, click on it" -- the click is on the
            // bar, and every machine in the house reports through what it
            // then shows.
            let mut out = Outcome::default();
            crate::script::run(
                &[format!("useInventory( #{item} )")],
                &mut self.state,
            )
            .effects
            .into_iter()
            .for_each(|e| out.effects.push(e));
            self.apply(&out);
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
    /// Whether the queue is holding for a click, so one dismisses it rather
    /// than reaching the room underneath.
    pub fn waiting_for_click(&self) -> bool {
        // Either side can be holding for one. The queue holds when the wait
        // arrives as an effect; the script holds when `pump` sees it in a
        // sequence and stops there. Clearing only the queue's left the script
        // waiting for a click that had already happened, which is the PeeK
        // unit refusing to close.
        matches!(self.effect_wait, Some(Wait::Click)) || matches!(self.waiting, Some(Wait::Click))
    }

    pub fn click(&mut self, x: i32, y: i32) -> Option<Outcome> {
        // A modal screen takes the click that dismisses it. Letting it
        // through would work the room behind the PeeK unit while the unit is
        // still on top of it.
        if self.waiting_for_click() {
            if matches!(self.effect_wait, Some(Wait::Click)) {
                self.effect_wait = None;
            }
            if matches!(self.waiting, Some(Wait::Click)) {
                self.waiting = None;
            }
            return Some(Outcome::default());
        }
        // The opening film watches for a click and stops early if it gets
        // one. Its room's own hotspot does nothing, so this has to come first
        // or the click is swallowed by it.
        if self.skip_intro() {
            return Some(Outcome::default());
        }
        // Handlers such as `stashClick` want the click position, which the
        // scripts read from the mouse rather than being passed.
        self.state.set("gMouseLoc", lingo::Value::Point(x, y));

        // A sprite a script is driving takes the click before the room does.
        // It has no rectangle in the room data, so the room's own hotspots
        // know nothing about it -- the telegram's twelve tiles sit on top of a
        // `#browse` region that would otherwise swallow every one of them.
        //
        // Only the tiles for now. The game has twenty-seven of these scripts
        // and this is the first, so the dispatch is by which puzzle is on the
        // stage rather than by a member's own script, which is not read yet.
        if let Some(channel) = self.sprite_at(x, y) {
            // ...and only while the puzzle is still a puzzle. The tiles stay
            // on the stage once it is solved, and a solved tile that still
            // swallowed clicks left the player looking at a finished telegram
            // with no way to go on from it.
            let order: Vec<i32> = self
                .state
                .get_all("telegramGuess")
                .iter()
                .filter_map(lingo::Value::as_int)
                .collect();
            let unsolved = order.len() == 12 && order != (1..=12).collect::<Vec<i32>>();
            if (25..=36).contains(&channel) && unsolved {
                let mut out = Outcome::default();
                if crate::natives::call(
                    "moveme",
                    &[lingo::Value::Int(channel as i32)],
                    &mut self.state,
                    &mut out,
                ) {
                    self.apply(&out);
                    return Some(out);
                }
            }
        }
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
            if !self.wait_satisfied(wait, false) {
                return combined;
            }
            self.waiting = None;
        }

        while !self.script.is_empty() {
            let action = self.script.remove(0);
            let mut outcome = script::run(std::slice::from_ref(&action), &mut self.state);
            // A `setState` writes as the action is read, which is right when
            // nothing is outstanding and wrong the moment something is: the
            // effects queued by the actions above have not been applied yet,
            // and this write would beat them to it. The weathervane is the
            // case that showed it. Its trellis hotspot fades through montages
            // 3, 2 and 1 and then puts the montage away, and with the write
            // arriving first the chapter came to rest on montage 1 -- where
            // the boat's sail, and everything else guarded on the montage
            // being down, is not there to be clicked. So when the queue is
            // still holding, the write is made again in its own place.
            if !self.pending.is_empty() || self.effect_wait.is_some() {
                let repeats = outcome
                    .writes
                    .drain(..)
                    .map(|(key, value)| Effect::SetState { key, value });
                outcome.effects.extend(repeats);
            }
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

    fn wait_satisfied(&self, wait: &Wait, armed_from_queue: bool) -> bool {
        match wait {
            Wait::Until(t) => Instant::now() >= *t,
            // A room with no movie has nothing to wait for; treating that as
            // satisfied stops a missing video from stalling the sequence.
            // The rest of the reasoning is on `film_wait_satisfied`.
            Wait::Video => film_wait_satisfied(
                &self.pending,
                self.player.as_ref().map(|p| p.finished),
                armed_from_queue,
            ),
            // The same question about the other film. A channel with nothing
            // on it has nothing to wait for.
            Wait::Overlay => self.overlay.as_ref().is_none_or(|o| o.player.finished),
            // Only a click clears this, so it is never satisfied by waiting.
            Wait::Click => false,
        }
    }


    /// Which script-driven sprite is under a point, topmost first.
    ///
    /// Director's `the clickOn`. A sprite a script has taken over is not a
    /// hotspot -- it has no rectangle in the room data -- so the only way to
    /// know it was clicked is to ask where its art actually landed, which is
    /// the same sum the renderer does.
    ///
    /// This is how the telegram is played: its twelve tiles are sprites 25 to
    /// 36, moved about the stage by `initTelegramPuzzle`, and `moveMe` reads
    /// `the clickOn` to learn which one was picked up.
    pub fn sprite_at(&mut self, x: i32, y: i32) -> Option<u8> {
        let domain = self.node().domain.clone();
        let placed: Vec<PlacedSprite> = self
            .puppets
            .iter()
            .filter(|(_, p)| !p.hidden && p.cast != 0)
            .map(|(ch, p)| (*ch, p.cast, p.loc))
            .collect();
        // Highest channel first: later channels draw over earlier ones.
        for (channel, cast, loc) in placed.into_iter().rev() {
            // Keyed, because a click lands on the art and not on the field
            // around it: the telegram's tiles are ragged and the gaps between
            // them belong to whatever is underneath.
            let Some(art) = self.art(&domain, cast, 36) else { continue };
            let (w, h) = (art.width as i32, art.height as i32);
            let (rx, ry) = (art.reg_x as i32, art.reg_y as i32);
            let (ox, oy) = match loc {
                Some((cx, cy)) => (
                    cx - if rx != 0 { rx } else { w / 2 },
                    cy - if ry != 0 { ry } else { h / 2 },
                ),
                None => continue,
            };
            if x >= ox && x < ox + w && y >= oy && y < oy + h {
                return Some(channel);
            }
        }
        None
    }

    /// Starts a hotspot's actions, the way a click on it would.
    ///
    /// The difference from running the list outright is the timeline: a
    /// sequence that plays a film, waits for it, moves, and plays another has
    /// to stop at each wait and let the queue catch up. Running the list in
    /// one go writes every flag and queues every film in the same instant, so
    /// the room has already changed by the time the first film is asked for
    /// and it is never the one the script meant.
    pub fn begin(&mut self, actions: &[String]) -> Outcome {
        self.script = actions.to_vec();
        self.waiting = None;
        self.pump()
    }

    fn apply(&mut self, outcome: &Outcome) {
        // Crossing into another chapter. The transition rooms end on
        // `enterNewDomain`, and until this was acted on the player watched the
        // whole sequence and stayed where they were.
        if let Some(domain) = &outcome.new_domain {
            // Not yet, if there is a sequence still to play. A handler that
            // ends a chapter queues the whole of its ending and *then* asks
            // for the domain change -- `goodbyeMandy` is the closet opening,
            // the drips, three montage steps, Mandy, two films and the lights
            // going out, and `enterNewDomain` is its last line. Acting on the
            // flag the moment it appears threw all of that away: the click on
            // the closet went straight from the basement to the gazebo, and
            // the ending helba could feel was missing was missing entirely.
            if !self.pending.is_empty()
                || self.effect_wait.is_some()
                || !outcome.effects.is_empty()
            {
                trace!(
                    crate::trace::Topic::Room,
                    "holding the move to {domain} until the queue is done"
                );
                self.pending.extend(outcome.effects.iter().cloned());
                self.pending.push(Effect::EnterDomain {
                    domain: domain.clone(),
                    room: outcome.new_domain_room,
                });
                return;
            }
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
                    // The call names the room to arrive in, by its index
                    // within the chapter. Without it the player lands on
                    // whatever the chapter's schema calls its start, which is
                    // where a *new game* would begin and not where the story
                    // has just put them.
                    // The chapter's own way back into the house wins over
                    // the index the call carries.
                    if let Some(name) = self.reentry.take() {
                        if let Some(room) = self.world.resolve(&name, Some(&d)) {
                            trace!(crate::trace::Topic::Room, "re-entering at {name}");
                            self.move_to(room);
                            // Arriving in the re-entry room is what closes the
                            // chapter: the ghost comes off `#ghostsRemaining`,
                            // its haunts come off `#hauntsRemaining`, and the
                            // PeeK unit reports a psionic fragment. The
                            // original hangs it off the inventory refresh,
                            // which runs constantly; here it runs once, at the
                            // moment it means.
                            self.close_chapter(&name);
                            self.start_room_video();
                            return;
                        }
                    }
                    if let Some(index) = outcome.new_domain_room {
                        let start = self.world.domains.get(&d).map(|(s, _)| *s);
                        if let Some(room) = start.map(|s| s + index.max(0) as usize) {
                            if room < self.world.nodes.len() {
                                trace!(
                                    crate::trace::Topic::Room,
                                    "arriving at {} (index {index})",
                                    self.world.nodes[room].name.clone().unwrap_or_default()
                                );
                                self.move_to(room);
                            }
                        }
                    }
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
                // `goBack` is a `goTo` with the destination looked up, so it
                // counts towards the haunts like any other move.
                let back = self.node().name.clone().unwrap_or_default();
                self.count_move(&back);
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
                let dest = dest.clone();
                self.count_move(&dest);
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
        Effect::WaitForOverlay => Some(Wait::Overlay),
        Effect::WaitForClick => Some(Wait::Click),
        // A sound's real length is not known here, so this is a short hold
        // rather than a promise.
        Effect::WaitForSound(_) => {
            Some(Wait::Until(Instant::now() + Duration::from_millis(250)))
        }
        _ => None,
    }
}

/// One line of the walkthrough's report for an effect, if it is worth one.
///
/// `room_film` is what the room would play of its own accord, which is what an
/// unnamed `pushVideo` resolves to.
fn describe(effect: &Effect, room_film: Option<&str>) -> Option<String> {
    Some(match effect {
        Effect::PlaySound { name, loudness } => match loudness {
            Some(l) => format!("play {name} ({l})"),
            None => format!("play {name}"),
        },
        Effect::StartLoop { name, volume } => {
            format!("loop {name} at {}", volume.unwrap_or(255))
        }
        Effect::StopLoop { name, .. } => format!("stop {name}"),
        Effect::PlayVideo(which) => match which.as_deref().or(room_film) {
            Some(n) => format!("film {n}"),
            None => "film (none)".to_string(),
        },
        Effect::PlayVideoSegment { from, to } => format!("film {from}..{to}"),
        Effect::StopVideo => "film stops".into(),
        Effect::WaitForVideo => "wait for the film".into(),
        Effect::WaitForOverlay => "wait for the unit".into(),
        Effect::WaitForSound(n) => format!("wait for {n}"),
        Effect::WaitTicks(t) if *t > 0 => format!("wait {t}"),
        Effect::SetState { key, value } | Effect::ReplaceState { key, value } => {
            format!("{key} = {value:?}")
        }
        Effect::FadeToMontage(n) => format!("montage {n}"),
        _ => return None,
    })
}

/// Names a wait for the strict replay's report.
fn name_wait(wait: Option<&Wait>) -> String {
    match wait {
        None => "none".into(),
        Some(Wait::Until(_)) => "a tick count".into(),
        Some(Wait::Video) => "a film".into(),
        Some(Wait::Overlay) => "a film on a channel".into(),
        Some(Wait::Click) => "a click".into(),
    }
}

/// What a part-run script is waiting on.
enum Wait {
    Until(Instant),
    Video,
    /// Held until a film on a script-driven channel ends -- the PeeK unit
    /// sliding up, which the original runs out with its own loop.
    Overlay,
    /// Held until the player clicks, for a modal screen.
    Click,
}

/// Folds one action's outcome into the running total for a sequence.
fn merge(into: &mut Outcome, from: Outcome) {
    into.destination = from.destination.or(into.destination.take());
    into.transition = from.transition.or(into.transition.take());
    into.new_domain = from.new_domain.or(into.new_domain.take());
    into.new_domain_room = from.new_domain_room.or(into.new_domain_room.take());
    into.go_back |= from.go_back;
    into.redraw |= from.redraw;
    into.credits |= from.credits;
    into.effects.extend(from.effects);
    into.writes.extend(from.writes);
    into.unhandled.extend(from.unhandled);
}

/// Whether a `wait #videoStop` has been met.
///
/// It is waiting for the film the `pushVideo` above it starts, and at the
/// moment the wait is armed that `pushVideo` may still be sitting in the
/// queue unapplied -- `pump` stops at the wait with the action's other
/// effects still pending. Asking "is a film playing yet" answers no, the
/// wait clears on the spot, and the script runs straight past its own
/// cutscene. The breaker in Roxy's office is the clearest case: throw it and
/// neither the film of the switch nor the film of the lights coming up was
/// ever opened.
///
/// So it asks whether a film is still waiting to be *started*, rather than
/// whether the queue is empty. Asking for an empty queue was a deadlock
/// waiting to happen, because `drain_ready` will not drain while a wait is
/// armed: anything queued after the wait meant the queue could never empty
/// and the wait could never clear. Turning `peekAlert` on found it, since
/// fitting the oscillator plays a film and then tells the PeeK about it.
fn film_wait_satisfied(
    pending: &[Effect],
    player_finished: Option<bool>,
    armed_from_queue: bool,
) -> bool {
    if !player_finished.unwrap_or(true) {
        return false;
    }
    // A wait armed out of the effect queue has already had everything before
    // it handed over, so the film it is waiting for is running and whatever is
    // left in the queue is *after* the wait. Looking for a film in there is
    // looking at the wrong ones: the heart box queues three films with a wait
    // between each, so the first wait saw the second and third films pending
    // and never cleared. That deadlock only showed in the window, because
    // `settle` steps over film waits and the terminal never reached it.
    //
    // A wait the script arms is different. `pump` stops at it with the rest of
    // that action's effects still queued, and the `pushVideo` on the line
    // above may be among them.
    if armed_from_queue {
        return true;
    }
    !pending
        .iter()
        .any(|e| matches!(e, Effect::PlayVideo(_) | Effect::PlayVideoSegment { .. }))
}

/// One sprite channel under script control.
/// A film a script has put on one of its own channels.
struct Overlay {
    channel: u8,
    player: VideoPlayer,
    /// The rect the member declares, which is not always the stored size.
    size: Option<(u32, u32)>,
    /// Whether its soundtrack has been handed to the mixer.
    started: bool,
    /// Held on its first frame, which is what a channel pointed at a film
    /// shows until a handler asks for it to play.
    parked: bool,
}

#[derive(Copy, Clone, Default)]
struct Puppet {
    /// Cast member to draw; zero means the channel is claimed but empty.
    cast: u32,
    /// A claimed channel can be prepared while hidden and shown later.
    hidden: bool,
    /// Where the sprite's registration point sits, if the script set it.
    loc: Option<(i32, i32)>,
    /// Director's ink. Anything but 0 means the background is not painted.
    ink: i32,
}

/// A running radio or clock programme.
struct Program {
    group: String,
    order: Vec<String>,
    /// Index of the next item to play, wrapping so the programme cycles.
    next: usize,
    /// The item on the air now, so the one it replaces is known to have been
    /// heard out. Margaret's clock puzzle starts on hearing an announcement
    /// to its end.
    playing: Option<String>,
    /// When the current item is expected to finish.
    due: Instant,
    gain: f32,
    /// Consecutive items that failed to resolve, so a wholly unresolvable
    /// programme stops instead of polling.
    misses: usize,
}

/// A script-driven sprite as it currently stands: channel, cast, and where
/// the script put it.
type PlacedSprite = (u8, u32, Option<(i32, i32)>);

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

    /// What `pump` hands back is a *report*, not a work list: every effect in
    /// it has already been queued on the way past. A caller that also acts on
    /// them acts on them twice -- which is what two of the window's paths were
    /// doing, so a resumed sequence sounded each of its cues once early and
    /// once in its place. The car's drive is where helba could hear it.
    #[test]
    fn pump_reports_the_effects_it_has_already_queued() {
        let mut game = Game::for_test();
        game.script = vec![
            "startSound #carDoorOpen".into(),
            "startSound #carDoorClose".into(),
        ];
        let outcome = game.pump();
        assert!(!outcome.effects.is_empty());
        for effect in &outcome.effects {
            assert!(
                game.pending.contains(effect),
                "{effect:?} was reported but not queued"
            );
        }
    }

    /// Pointing a channel at a film shows a frame; it does not play it.
    ///
    /// `setCarLocation` is the case that proves it: it sets `the castNum of
    /// sprite 44` to the junction film and calls `updateDisplay`, and that is
    /// all -- the film stands on its first frame until `chooseTrack` scrubs a
    /// third of it. Starting it here played the whole drive again, on a loop,
    /// every time the car reached a hub.
    #[test]
    fn a_film_on_a_channel_is_a_still_until_something_plays_it() {
        // Nothing to point at in the test harness, so the rule is checked on
        // the player itself, which is what `point_channel` parks.
        let Some(mut player) = crate::player::VideoPlayer::open(std::path::Path::new(
            "extract/EDWIN/MOVIES_E/CARBACK.MOV",
        )) else {
            // No game data here; the rule is still stated above.
            return;
        };
        player.park();
        assert!(player.finished, "a parked film does not advance");
        player.restart();
        assert!(!player.finished, "and playing it starts it again");
    }

    /// A channel's position is a score property, and the room's `#showIF`
    /// says which film is on the video channel rather than where it is. So a
    /// film a script pushes over the room's keeps the room's coords even
    /// though no guard holds any more.
    #[test]
    fn the_video_channel_keeps_its_place_when_no_guard_holds() {
        use crate::world::{Channel, Cond, Node, Sprite};

        let mut game = Game::for_test();
        game.world.nodes.push(Node {
            sprites: vec![Sprite {
                cast_name: Some("carBack.mov".into()),
                cast_number: 0,
                cast_lookup: None,
                channel: Channel::Video,
                condition: Cond::Equals {
                    key: "showMontage".into(),
                    value: lingo::Value::Int(3),
                },
                center: Some((322, 204)),
                ink: 0,
                volume: None,
            }],
            ..Node::default()
        });
        game.room = game.world.nodes.len() - 1;

        // The guard holds: the room's own film, in its place.
        game.state.set_all("showMontage", vec![lingo::Value::Int(3)]);
        assert_eq!(game.video_channel_centre(), Some((322, 204)));

        // The car sets off and the montage goes to 0. The guard stops holding
        // and the channel does not move; before this the track films drew
        // centred on the stage instead of in the windscreen.
        game.state.set_all("showMontage", vec![lingo::Value::Int(0)]);
        assert_eq!(game.video_channel_centre(), Some((322, 204)));
    }

    #[test]
    fn a_state_write_lands_after_the_effects_queued_above_it() {
        // The weathervane's trellis, in miniature: fade through three montages
        // and then take the montage down. The writes happen as the list is
        // read and the fades happen as the queue drains, so without the queued
        // copy the flag ends on the last fade rather than on 0 -- and every
        // hotspot in Edwin's chapter guarded on the montage being down stays
        // dead, which is how the chapter came to be unfinishable.
        let mut game = Game::for_test();
        game.state
            .set_all("showMontage", vec![lingo::Value::Int(1), lingo::Value::Int(0)]);
        game.script = vec![
            "fadeToMontage 3".into(),
            "fadeToMontage 2".into(),
            "fadeToMontage 1".into(),
            "setState( oStoryteller, #showMontage, 0 )".into(),
        ];
        game.pump();
        // The write is the last thing in the queue, behind all three fades.
        assert!(
            matches!(
                game.pending.as_slice(),
                [
                    Effect::FadeToMontage(3),
                    Effect::FadeToMontage(2),
                    Effect::FadeToMontage(1),
                    Effect::SetState { key, value: lingo::Value::Int(0) },
                ] if key == "showMontage"
            ),
            "{:?}",
            game.pending
        );
        for _ in 0..64 {
            if game.pending.is_empty() && game.effect_wait.is_none() {
                break;
            }
            game.drain_ready();
        }
        assert_eq!(game.state.get("showMontage"), lingo::Value::Int(0));
    }

    #[test]
    fn a_queue_of_films_drains_to_the_end() {
        // The shape the heart box queues: three films, each with a wait and a
        // stop after it. Draining it the way the window does has to reach the
        // end. `settle` steps over film waits, so the terminal replays every
        // recording without ever asking whether such a queue terminates --
        // which is how a deadlock here shipped twice.
        let mut game = Game::for_test();
        for step in 1..=3 {
            game.pending.push(Effect::FadeToMontage(step));
            game.pending.push(Effect::PlayVideo(None));
            game.pending.push(Effect::WaitForVideo);
            game.pending.push(Effect::StopVideo);
        }
        game.pending.push(Effect::SetState {
            key: "heartBox".into(),
            value: lingo::Value::Symbol("open".into()),
        });

        // No decoder here, so every film is finished the moment it starts,
        // which is exactly the question being asked: does the queue advance.
        for _ in 0..64 {
            if game.pending.is_empty() && game.effect_wait.is_none() {
                break;
            }
            game.drain_ready();
        }
        assert!(
            game.pending.is_empty() && game.effect_wait.is_none(),
            "the queue stalled with {} effect(s) left",
            game.pending.len()
        );
    }

    #[test]
    fn a_film_wait_clears_even_with_work_queued_behind_it() {
        use super::film_wait_satisfied;

        // `drain_ready` will not drain while a wait is armed, so a wait that
        // asks for an empty queue can never be satisfied once anything has
        // been queued after it. Fitting the oscillator does exactly that: it
        // plays a film, tells the PeeK unit about it, and the PeeK's pulse
        // queues thirteen waits of its own behind the film's. The window
        // stopped there for good.
        //
        // What the wait is for is the film having been started, so that is
        // what it asks about.
        // Armed by the script, with the `pushVideo` above it still queued.
        assert!(
            !film_wait_satisfied(&[Effect::PlayVideo(None), Effect::WaitTicks(5)], Some(true), false),
            "a film still waiting to be started holds a script's wait"
        );
        assert!(
            film_wait_satisfied(&[Effect::WaitTicks(5)], Some(true), false),
            "and nothing left to start does not"
        );
        // Armed out of the queue: everything before it has been handed over,
        // so a film further down the queue belongs to a *later* wait. The
        // heart box queues three of them and the first wait never cleared.
        assert!(
            film_wait_satisfied(&[Effect::PlayVideo(None)], Some(true), true),
            "a later film in the queue does not hold this wait"
        );
        assert!(
            !film_wait_satisfied(&[], Some(false), true),
            "and a film still running holds it either way"
        );
    }

    #[test]
    fn a_film_is_placed_by_the_sprite_that_is_actually_showing() {
        // A room can declare several films on the video channel, each gated
        // on a different state and each with its own `#coords`. The study
        // puts the headgear films at (303, 220) and the film of the
        // oscillator being fitted at (317, 185), so taking the first sprite's
        // position played the right film in the headgear's place. Forty rooms
        // declare more than one film and twenty-six of them at differing
        // coordinates.
        use crate::world::{Channel, Cond, Node, Sprite};

        let film = |flag: &str, value: &str, centre: (i32, i32)| Sprite {
            cast_name: None,
            cast_number: 0,
            cast_lookup: None,
            channel: Channel::Video,
            condition: Cond::Equals {
                key: flag.into(),
                value: lingo::Value::Symbol(value.into()),
            },
            center: Some(centre),
            ink: 0,
            volume: None,
        };
        let mut game = Game::for_test();
        game.world.nodes[0] = Node {
            sprites: vec![
                film("AMBERVISION", "waitingForPlayer", (303, 220)),
                film("oscillatorInPlace", "placingNow", (317, 185)),
            ],
            ..Node::default()
        };

        let centre_now = |g: &Game| {
            let state = &g.state;
            g.world.nodes[g.room]
                .sprites
                .iter()
                .find(|s| matches!(s.channel, Channel::Video) && state.test(&s.condition))
                .and_then(|s| s.center)
        };

        game.state.set_all("oscillatorInPlace", vec![lingo::Value::Symbol("placingNow".into())]);
        assert_eq!(centre_now(&game), Some((317, 185)), "the film that is playing");

        game.state.set_all("oscillatorInPlace", vec![lingo::Value::Int(0)]);
        game.state.set_all("AMBERVISION", vec![lingo::Value::Symbol("waitingForPlayer".into())]);
        assert_eq!(centre_now(&game), Some((303, 220)), "the other one");
    }

    #[test]
    fn a_film_stays_where_it_was_placed_when_its_guard_stops_holding() {
        // The sequence that fits the oscillator sets the flag its film is
        // gated on, plays the film, waits for it, and then sets the flag on
        // again -- so for the last frames of the film no video sprite's guard
        // holds at all. Deriving the position every frame moved the film out
        // of the AMBER device's slot and into the middle of the stage just
        // before it ended, which is the jump helba photographed.
        use crate::world::{Channel, Cond, Node, Sprite};

        let mut game = Game::for_test();
        game.world.nodes.push(Node {
            sprites: vec![Sprite {
                cast_name: Some("oslator1.mov".into()),
                cast_number: 0,
                cast_lookup: None,
                channel: Channel::Video,
                condition: Cond::Equals {
                    key: "oscillatorInPlace".into(),
                    value: lingo::Value::Symbol("placingNow".into()),
                },
                center: Some((317, 185)),
                ink: 0,
                volume: None,
            }],
            ..Node::default()
        });
        game.room = game.world.nodes.len() - 1;

        game.state
            .set_all("oscillatorInPlace", vec![lingo::Value::Symbol("placingNow".into())]);
        game.start_room_video();
        assert_eq!(game.playing_at, Some((317, 185)));

        // The sequence moves on while the film is still running.
        game.state.set_all("oscillatorInPlace", vec![lingo::Value::Int(1)]);
        assert_eq!(
            game.playing_at,
            Some((317, 185)),
            "it stays in the slot until the film is replaced"
        );
    }

    #[test]
    fn a_ghost_works_through_its_calls_in_the_order_the_game_lists_them() {
        // `newSound = ( gCurrentEntrySounds[who] mod count(list) ) + 1`, and
        // the cursor starts at 1, so the *second* file is the first one
        // heard. The lists are in string order because the authors built them
        // from a directory listing, so Margaret's second file is `Mcall10`.
        let mut game = Game::for_test();
        game.state.set_all("AMBERVISION", vec![lingo::Value::Symbol("on".into())]);
        game.state.set_all("ghostsCalling", vec![lingo::Value::Symbol("Margaret".into())]);

        let mut heard = Vec::new();
        for _ in 0..4 {
            game.ghost_call_until = None;
            game.tick_ghost_call();
            heard.extend(game.pending.drain(..).filter_map(|e| match e {
                Effect::PlaySound { name, .. } => Some(name),
                _ => None,
            }));
        }
        assert_eq!(heard, ["Mcall10", "Mcall2", "Mcall3", "Mcall4"]);
    }

    #[test]
    fn nobody_in_the_rota_is_a_pause_rather_than_a_call() {
        // The padding `ghostCalls` adds is what spaces the calls out: an
        // entry call lands every turn, a warm one one turn in three.
        let mut game = Game::for_test();
        game.state.set_all("AMBERVISION", vec![lingo::Value::Symbol("on".into())]);
        game.state.set_all(
            "ghostsCalling",
            ["Margaret", "nobody", "nobody"]
                .iter()
                .map(|w| lingo::Value::Symbol((*w).into()))
                .collect(),
        );

        let mut turns = Vec::new();
        for _ in 0..6 {
            game.ghost_call_until = None;
            game.tick_ghost_call();
            let played = game.pending.drain(..).find_map(|e| match e {
                Effect::PlaySound { name, .. } => Some(name),
                _ => None,
            });
            turns.push(played);
        }
        // The rotation moves the *last* entry to the front rather than
        // stepping forward, so one call is followed by both pauses.
        assert_eq!(
            turns,
            [
                Some("Mcall10".into()),
                None,
                None,
                Some("Mcall2".into()),
                None,
                None
            ]
        );
    }

    #[test]
    fn a_call_holds_the_rota_until_it_has_finished() {
        // The original asks `soundBusy` on the channel the last call went out
        // on and gives up if it is still speaking, so two ghosts never talk
        // over each other.
        let mut game = Game::for_test();
        game.state.set_all("AMBERVISION", vec![lingo::Value::Symbol("on".into())]);
        game.state.set_all("ghostsCalling", vec![lingo::Value::Symbol("Margaret".into())]);
        game.ghost_call_until = Some(Instant::now() + Duration::from_secs(30));
        game.tick_ghost_call();
        assert!(game.pending.is_empty(), "called over the top of a call");
    }

    #[test]
    fn nobody_calls_until_the_headgear_is_on() {
        // `idle` runs `playDomainEntrySound` only when `#AMBERVISION` is
        // `#on`. The hint book says the same: the calls begin once the
        // headgear is calibrated and they are what lead the player to the
        // domain entry tunnels. Ungated, the ghosts telephone from the
        // boathouse path before there is anywhere to be led.
        let mut game = Game::for_test();
        game.state.set_all("ghostsCalling", vec![lingo::Value::Symbol("Margaret".into())]);

        for state in ["off", "startingUp", "readyToGo"] {
            game.state.set_all("AMBERVISION", vec![lingo::Value::Symbol(state.into())]);
            game.ghost_call_until = None;
            game.tick_ghost_call();
            assert!(game.pending.is_empty(), "called with the vision {state}");
        }

        game.state.set_all("AMBERVISION", vec![lingo::Value::Symbol("on".into())]);
        game.ghost_call_until = None;
        game.tick_ghost_call();
        assert!(!game.pending.is_empty(), "silent with the vision on");
    }

    #[test]
    fn a_cutscene_is_not_talked_over() {
        // `if gSoundsSuspended = 1 then return`.
        let mut game = Game::for_test();
        game.state.set_all("AMBERVISION", vec![lingo::Value::Symbol("on".into())]);
        game.state.set_all("ghostsCalling", vec![lingo::Value::Symbol("Margaret".into())]);
        game.apply_puppet(&Effect::SuspendSounds { fade: true });
        game.tick_ghost_call();
        assert!(game.pending.iter().all(|e| !matches!(e, Effect::PlaySound { .. })));

        game.apply_puppet(&Effect::RestoreSounds { fade: true });
        game.ghost_call_until = None;
        game.tick_ghost_call();
        assert!(game.pending.iter().any(|e| matches!(e, Effect::PlaySound { .. })));
    }

    #[test]
    fn silencing_the_ghosts_frees_the_rota_at_once() {
        // `ghostCalls #None` is on the way out of a room, and the next room's
        // ghosts should be able to start straight away rather than waiting
        // out a call that has just been cut off.
        let mut game = Game::for_test();
        game.ghost_call_until = Some(Instant::now() + Duration::from_secs(30));
        game.apply_puppet(&Effect::StopGhostCall);
        assert!(game.ghost_call_until.is_none());
    }

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

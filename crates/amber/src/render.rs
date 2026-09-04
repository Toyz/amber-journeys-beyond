//! Window, input and the main loop.

#[cfg(feature = "desktop")]
use std::path::Path;


use crate::audio::Audio;
use crate::cursor;
use crate::game::Game;
use crate::script::Effect;
use crate::world::Verb;

const STAGE_W: usize = 640;
const STAGE_H: usize = 480;

/// The pointer shape a verb implies. Amber swaps the cursor to tell the player
/// what a region will do, which is the whole of its interaction vocabulary.
fn cursor_hint(verb: Verb) -> &'static str {
    match verb {
        Verb::Left => "turn left",
        Verb::Right => "turn right",
        Verb::Forward => "forward",
        Verb::Up => "look up",
        Verb::Down => "look down",
        Verb::Examine => "examine",
        Verb::Pointer => "operate",
        Verb::ItemInUse => "use item",
        Verb::NextPage => "next page",
        Verb::RotateLeft => "rotate left",
        Verb::RotateRight => "rotate right",
        Verb::Browse => "",
    }
}

/// Opens the window and plays a recording into it before handing over.
///
/// The steps go through `walk::command`, the same dispatcher the terminal
/// uses, so watching a `.walk` file and replaying it in the terminal cannot
/// diverge. Control passes to the player when the recording runs out, which
/// makes a recording a way of setting the game up as much as of watching it.
#[cfg(feature = "desktop")]
pub fn play_with(
    root: &Path,
    start: Option<&str>,
    steps: Vec<String>,
    muted: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut game = Game::new(root)?;
    if let Some(name) = start {
        // A chapter name goes to that chapter's own opening, which is what
        // anyone typing `play <dir> MARGARET` means. A room name goes to the
        // room. Either way the chapter is seeded first: jumping straight to a
        // room in another chapter used to leave its flags unwritten, so every
        // guard there read against a void and the room came up wrong.
        let chapter = game
            .world
            .domains
            .keys()
            .find(|d| d.eq_ignore_ascii_case(name))
            .cloned();
        match chapter {
            Some(domain) => game.enter_chapter(&domain),
            None => match game.world.resolve(name, None) {
                Some(i) => {
                    let domain = game.world.nodes[i].domain.clone();
                    game.seed_chapter(&domain);
                    game.jump_to(i);
                }
                None => eprintln!(
                    "warning: {name} is neither a room nor a chapter ({}), \
                     starting at the default",
                    game.world.domains.keys().cloned().collect::<Vec<_>>().join(", ")
                ),
            },
        }
        game.start_room_video();
    }

    // A machine with no audio device is not an error; the game is playable
    // silently and this is the normal case over a remote session.
    //
    // `--mute` is not the same thing as having no device. It runs the whole
    // mixer -- gains, the four channels, the groups that refuse to talk over
    // each other -- into nowhere, and turns the audio log on, so what the game
    // asked to hear can be read instead of heard. That is the only way I can
    // check the sound of a game I cannot listen to.
    let audio = if muted { Some(Audio::silent()) } else { Audio::open() };
    match (&audio, muted) {
        (Some(_), true) => eprintln!("muted; sound is logged, not played"),
        (Some(a), false) => eprintln!("audio out at {} Hz", a.rate()),
        (None, _) => eprintln!("no audio device; running silently"),
    }
    eprintln!("space skips a movie, tab outlines live hotspots, escape quits");
    let opening_movie = game.video();
    eprintln!(
        "starting in {} / {}{}",
        game.node().domain,
        game.node().name.clone().unwrap_or_default(),
        match &opening_movie {
            Some(m) => format!(" (playing {m})"),
            None => String::new(),
        }
    );
    // `--scale` and `--filter`. The stage stays 640 by 480 and the window is
    // whatever that grows to: the pointer is mapped against the stage, so the
    // game never knows how big it is being shown.
    let factor = std::env::var("AMBER_SCALE")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(1)
        .clamp(1, 4);
    let filter = std::env::var("AMBER_FILTER")
        .ok()
        .and_then(|v| crate::scale::Filter::parse(&v))
        .unwrap_or_default();
    let shown = (STAGE_W * factor, STAGE_H * factor);
    let mut host = crate::host_desktop::Desktop::open("Amber: Journeys Beyond", shown)?;
    // Where a save goes. `AMBER_SAVE` overrides it; otherwise it sits in the
    // working directory rather than beside the game, because the game may well
    // be a read-only disc image.
    let saves = std::env::var_os("AMBER_SAVE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("amber.save"));

    // The loop itself is host-agnostic, so it is `run`: the desktop opens a
    // window and hands it over, and so does every other front end.
    run(&mut game, &mut host, audio, steps, muted, factor, filter, Some(saves))
}

/// The main loop, over any [`Host`](crate::host::Host).
///
/// This is the whole game running: input, the effect queue, the waits, the
/// compositor, the transitions and the mixer. It names no platform -- the
/// desktop, and anything else with a window, differ only in what they hand in.
/// Keeping one loop is deliberate: two front ends with a loop each is how this
/// engine shipped a dozen faults that were live in one and invisible in the
/// other.
#[allow(clippy::too_many_arguments)]
pub fn run(
    game: &mut Game,
    host: &mut dyn crate::host::Host,
    audio: Option<Audio>,
    steps: Vec<String>,
    muted: bool,
    factor: usize,
    filter: crate::scale::Filter,
    saves: Option<std::path::PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    const STAGE: (usize, usize) = (STAGE_W, STAGE_H);
    let shown = (STAGE_W * factor, STAGE_H * factor);
    let mut playing_soundtrack = false;
    let mut ambience_room = usize::MAX;
    // The route has to start somewhere known, so the first line of a
    // recording is the room the game opened in.
    if crate::record::active() {
        if let Some(name) = game.node().name.clone() {
            crate::record::step(&name);
        }
    }
    let mut input = crate::host::Input {
        hover: true,
        pointer: None,
        down: false,
        pressed: Vec::new(),
        open: true,
    };

    // `frame` holds the composed scene, redrawn only when it changes;
    // `out` adds the cursor, which follows the mouse every frame.
    let mut frame = vec![0u32; STAGE_W * STAGE_H];
    let mut out = vec![0u32; STAGE_W * STAGE_H];
    let mut show_hotspots = false;
    let mut dirty = true;
    // The recording is played a step at a time rather than all at once, so it
    // can be watched. A step waits for the one before it to finish -- films
    // and their waits included -- and then for a breath, so a sequence of
    // moves does not become a blur.
    let mut replay: std::collections::VecDeque<String> = steps.into();
    if !replay.is_empty() {
        // Clicking through the opening, which is what a viewer would do: a
        // recording is not usually a recording of the intro, and waiting out
        // the film before the first step is a minute and a half of nothing.
        // Going where it was going, so a recording that starts by walking from
        // the entry still works and one that starts by jumping somewhere
        // cancels it on the jump.
        game.skip_opening();
    }
    let mut next_step = std::time::Instant::now();
    let mut was_down = false;
    let mut last_title = String::new();

    let mut frames: u64 = 0;
    let started = std::time::Instant::now();
    // The stage as it looked before the change a transition is covering, and
    // how far through that transition we are.
    let mut outgoing: Vec<u32> = vec![0; STAGE_W * STAGE_H];
    let mut dissolve: Option<(f32, crate::game::Transition)> = None;
    // Whether the cursor is over the inventory bar, which decides whether
    // its icons are drawn in full colour or as outlines.
    let mut inventory_hot = false;
    // Which piece of cut content the C key shows next.
    let mut cut_next = 0usize;
    let mut last_frame = std::time::Instant::now();
    let mut menu: Option<crate::menu::Menu> = None;
    // The picture filter starts where the command line put it and is the
    // player's from then on.
    let mut settings = crate::menu::Settings { filter, ..Default::default() };
    let mut menu_was_down = false;
    let mut hud_was_down = false;
    let mut quit = false;
    while input.open && !quit {
        input = host.poll(STAGE);

        // The pause menu sits over everything. While it is up the game sees no
        // input at all, which is the whole of what "paused" means here -- and
        // it is drawn by the engine rather than by a front end, because two
        // front ends with a menu each is two menus that will disagree.
        if input.pressed.contains(&crate::host::Key::Menu) {
            menu = match menu {
                Some(_) => None,
                None => Some(crate::menu::Menu::new(
                    settings,
                    saves.as_deref().map(crate::save::slots).unwrap_or_default(),
                )),
            };
            dirty = true;
        }
        // Acting on the release edge, which is what the game does everywhere
        // else, so a press that started outside a row does not pick it.
        let action = match (&mut menu, menu_was_down && !input.down) {
            (Some(open), true) => match input.pointer {
                Some((mx, my)) => open.click(mx, my, STAGE_W, STAGE_H),
                None => crate::menu::Action::None,
            },
            _ => crate::menu::Action::None,
        };
        menu_was_down = input.down;
        match action {
            crate::menu::Action::Resume => menu = None,
            crate::menu::Action::Quit => quit = true,
            crate::menu::Action::Save(slot) => {
                let note = match saves.as_deref() {
                    Some(base) => {
                        let path = crate::save::slot_path(base, slot + 1);
                        match std::fs::write(&path, crate::save::write(game)) {
                            Ok(()) => "SAVED",
                            Err(_) => "COULD NOT WRITE",
                        }
                    }
                    None => "NOWHERE TO SAVE",
                };
                if let Some(open) = &mut menu {
                    // The slot list is re-read so the row it was just written
                    // to stops saying EMPTY.
                    open.slots = saves.as_deref().map(crate::save::slots).unwrap_or_default();
                    open.note = Some(note.into());
                }
            }
            crate::menu::Action::Load(slot) => {
                let loaded = saves
                    .as_deref()
                    .map(|base| crate::save::slot_path(base, slot + 1))
                    .and_then(|path| std::fs::read_to_string(path).ok())
                    .map(|text| crate::save::read(game, &text));
                match loaded {
                    Some(Ok(())) => {
                        // The queue and the film belong to the room being
                        // left, so they go with it.
                        playing_soundtrack = false;
                        ambience_room = usize::MAX;
                        menu = None;
                    }
                    _ => {
                        if let Some(open) = &mut menu {
                            open.note = Some("COULD NOT LOAD".into());
                        }
                    }
                }
            }
            crate::menu::Action::None => {}
        }
        // Settings follow the menu rather than the menu reaching out: the
        // volume is the only one that needs telling, and the picture and the
        // pad are read where they are used.
        if let Some(open) = &menu {
            if open.settings != settings {
                settings = open.settings;
                if let Some(a) = &audio {
                    a.set_master(settings.volume);
                }
                dirty = true;
            }
        }

        if menu.is_some() {
            input.pointer = None;
            input.down = false;
            input.pressed.clear();
            dirty = true;
        }

        // The on-screen buttons, which a phone needs because it has no keys.
        // Checked before the game sees the click and swallowing it either way,
        // so a press on a button never also works the room underneath.
        let film_playing = game.can_skip();
        // Where to click to put down whatever is being held up. Also the only
        // sign the game is holding for a click: after the PeeK comes down, or
        // a switch is thrown, the queue waits and there is otherwise nothing
        // on screen that says so.
        let way_out = game.way_out();
        // Set when CLOSE is pressed, and spent by the dispatch below -- so the
        // button produces the same click a player would have made rather than
        // a second way of doing it.
        let mut close_at: Option<(i32, i32)> = None;
        if menu.is_none() {
            if let Some((mx, my)) = input.pointer {
                if let Some(tap) =
                    crate::menu::hud_hit(mx, my, STAGE_W, film_playing, way_out.is_some())
                {
                    if hud_was_down && !input.down {
                        match tap {
                            crate::menu::Tap::Menu => {
                                menu = Some(crate::menu::Menu::new(
                                    settings,
                                    saves.as_deref().map(crate::save::slots).unwrap_or_default(),
                                ));
                            }
                            // Closing is the click a player would have made
                            // on the part of the screen that is not the thing.
                            crate::menu::Tap::Close => {
                                if let Some((cx, cy)) = way_out {
                                    close_at = Some((cx, cy));
                                }
                            }
                            crate::menu::Tap::Skip => {
                                if game.skip_video() {
                                    crate::record::step("skip");
                                    if let Some(a) = &audio {
                                        a.stop_oneshots();
                                    }
                                    playing_soundtrack = false;
                                }
                            }
                        }
                    }
                    hud_was_down = input.down;
                    // The position is taken away so the room underneath does
                    // not also get the click. The *button* is left alone:
                    // zeroing it meant `was_down` never became true, so the
                    // release edge the dispatch below waits for never
                    // happened -- and CLOSE, which needs that dispatch to
                    // deliver its click, did nothing at all.
                    input.pointer = None;
                    dirty = true;
                } else {
                    hud_was_down = false;
                }
            }
        }
        frames += 1;
        // Nothing pulls samples through a silent mixer, so without this every
        // sound would run for ever, the four channels would fill, and the log
        // would report each later sound as dropped -- which is exactly the
        // false picture `mix` gave of the music boxes.
        if muted {
            if let Some(a) = &audio {
                a.settle(last_frame.elapsed().as_secs_f32());
            }
        }
        last_frame = std::time::Instant::now();
        crate::trace::frame(frames);

        // Director's `the ticks` is sixtieths since startup, and the scan
        // unit's timer is an absolute deadline in them. Never advancing it
        // left every such deadline in the past.
        game.state.set(
            "gTicks",
            lingo::Value::Int((started.elapsed().as_secs_f64() * 60.0) as i32),
        );
        // And the scan unit's countdown reads that clock. The original walks
        // it back when the player looks at the unit, on a sprite `mouseDown`
        // this engine has no equivalent of; nothing but the unit itself reads
        // the status, so keeping it current costs nothing and a scan that is
        // never looked at still finishes.
        {
            let mut out = crate::script::Outcome::default();
            if crate::natives::call("resetpeekdisplay", &[], &mut game.state, &mut out) && out.redraw
            {
                dirty = true;
            }
        }

        // Whichever ghost's turn it is calls. The original runs this from
        // `idle`, so it is a frame concern rather than a click one -- the
        // calls carry on while the player stands still, which is the point of
        // them.
        game.tick_ghost_call();
        // And a carol, on Edwin's ice, every three and a half minutes.
        if let Some(song) = game.tick_carol() {
            if let Some(a) = &audio {
                let gain = game.sounds.gain(&song);
                if let Some((pcm, rate, ch)) = game.sound(&song) {
                    a.play(Some(&song), None, pcm, rate, ch, gain, false, true);
                }
            }
        }

        // The recording takes its next step once the game has gone quiet: a
        // step that starts a film has to be allowed to finish it, or the
        // replay races ahead of what it is meant to show.
        // Not on `player.is_none()`: a room's own film loops for as long as
        // the player stands there, so waiting for the picture to stop would
        // wait for ever. A film a script is waiting on holds the effect queue,
        // and that is what has to be quiet.
        //
        // A queue waiting for a *click* is the exception, and it has to be:
        // the PeeK unit holds there until it is dismissed, and the only thing
        // that can dismiss it during a replay is the replay's own next step.
        // Waiting for the queue to go quiet first is a deadlock, and it is
        // one the terminal cannot see -- its `settle` steps over every wait,
        // so the same recording ran there and hung in the window.
        if !replay.is_empty()
            && (!game.effects_busy() || game.waiting_for_click())
            && (!game.script_running() || game.waiting_for_click())
            && std::time::Instant::now() >= next_step
        {
            if let Some(cmd) = replay.pop_front() {
                println!("> {cmd}");
                // `wait <ticks>` is the recording asking to be left alone for
                // a moment, so that whatever is on screen can be watched. It
                // paces the replay rather than the game: the queue carries on,
                // the film keeps playing, and only the next step is held back.
                let beat = cmd
                    .strip_prefix("wait ")
                    .and_then(|n| n.trim().parse::<u64>().ok())
                    .map(|ticks| std::time::Duration::from_millis(ticks * 1000 / 60));
                if beat.is_none() {
                    let _ = crate::walk::command(&mut *game, &cmd, false);
                    dirty = true;
                }
                next_step = std::time::Instant::now()
                    + beat.unwrap_or(std::time::Duration::from_millis(600));
                if replay.is_empty() {
                    println!("-- recording finished, the game is yours --");
                }
            }
        }

        // A handler's sequence can hold part way through -- suspend, play a
        // film, wait for it, restore -- so the queue is pumped every frame and
        // not only in the frame a click arrives.
        if game.effects_busy() {
            apply_effects(&mut *game, audio.as_ref(), &mut dirty, &mut playing_soundtrack);
        }
        if game.room != ambience_room {
            ambience_room = game.room;
            crate::game::update_ambience(&mut *game, audio.as_ref());
        }

        // Space skips whatever movie is playing. The opening is two minutes
        // long and the original had no way past it either, so this is the one
        // deliberate departure from the game's behaviour.
        if input.pressed.contains(&crate::host::Key::Space) && game.skip_video() {
            crate::record::step("skip");
            if let Some(a) = &audio {
                a.stop_oneshots();
            }
            playing_soundtrack = false;
            dirty = true;
            eprintln!(
                "skipped to {} / {}",
                game.node().domain,
                game.node().name.clone().unwrap_or_default()
            );
        }

        // A part-run hotspot script resumes here once whatever it was waiting
        // on has finished. This is what lets an in-world animation be seen:
        // the switch sets its flag, the stage recomposes so the movie appears,
        // and only when the movie ends does the script clear the flag again.
        if game.script_running() {
            // Only what the resumed actions ask of the *frame*. Their effects
            // are not played here: `pump` has already queued every one of them
            // on the way past, and `apply_effects` above is what plays a queue
            // -- in order, and only once whatever wait stands in front of them
            // has cleared. Playing them here as well sounded each of them
            // twice, once early and once in its place, which is what made a
            // long sequence like the car's drive stutter and echo.
            let outcome = game.pump();
            if outcome.destination.is_some()
                || outcome.go_back
                || outcome.redraw
                || !outcome.effects.is_empty()
            {
                dirty = true;
            }
        }

        // Advance any running programme, queuing its next item as the current
        // one runs out.
        if let Some((group, pcm, rate, channels, gain)) = game.tick_program() {
            if let Some(a) = &audio {
                // A take is keyed by the programme it belongs to, not by its
                // own name: the takes are distinct recordings played in turn,
                // and what must not happen is two of them at once. The key
                // also lets a room that no longer wants the radio retire it,
                // and stops the programme being started again on arrival --
                // without it, every room change queued another two minute take
                // until all four channels were radio and nothing else could be
                // heard.
                a.play(Some(&group), Some(group.clone()), pcm, rate, channels, gain, false, true);
            }
        }

        // The cues a drive carries, against the film's own clock. They are
        // not queued effects: a queue is sequential and would have to wait for
        // the film it is meant to play over.
        for effect in game.due_cues() {
            apply_effect(&mut *game, audio.as_ref(), effect, &mut dirty, &mut playing_soundtrack);
        }

        // A playing movie supplies its own redraws; a static room only needs
        // one after a click.
        if let Some(player) = &mut game.player {
            if player.tick() {
                dirty = true;
            }
            // Start the soundtrack once, when the movie is first shown.
            if !playing_soundtrack {
                if let Some(a) = &audio {
                    a.play(
                        None,
                        None,
                        player.audio_for_segment(),
                        player.audio_rate,
                        player.audio_channels,
                        1.0,
                        false,
                        // QuickTime plays a movie's soundtrack outside the
                        // four channels, so it takes none of them.
                        false,
                    );
                }
                playing_soundtrack = true;
            }
        } else {
            playing_soundtrack = false;
        }

        // A film a script put on its own channel runs alongside the room's,
        // which is how the PeeK unit slides up and then plays its recordings
        // over whatever room the player happens to be standing in.
        if game.tick_overlay() {
            dirty = true;
        }
        if let Some((pcm, rate, channels)) = game.take_overlay_audio() {
            if let Some(a) = &audio {
                a.play(None, None, pcm, rate, channels, 1.0, false, false);
            }
        }
        if dirty {
            // A transition covers the change from what is on the stage now to
            // what is about to be, so the outgoing image has to be kept before
            // the new one is composed over it.
            if let Some(t) = game.take_transition() {
                outgoing.copy_from_slice(&frame);
                dissolve = Some((0.0, t));
            }
            game.inventory_hot = inventory_hot;
            game.draw(&mut frame, STAGE_W as u32, STAGE_H as u32);
            dirty = false;
        }

        // Director's `#fadeIn` is a dissolve, not a cut. The game asks for one
        // a hundred and six times -- every door, every close-up, every step of
        // a montage -- and without it each of those is a hard jump.
        if let Some((progress, t)) = dissolve {
            let progress = progress + t.step;
            if progress >= 1.0 {
                dissolve = None;
            } else {
                dissolve = Some((progress, t));
            }
        }
        // Tab shows where the live hotspots actually are, which is the quickest
        // way to tell a missing exit from one that is merely hard to find.
        if input.pressed.contains(&crate::host::Key::Hotspots) {
            show_hotspots = !show_hotspots;
        }
        // S prints what is on the stage, bottom to top, into the log. A fault
        // that is only visible -- a film at the wrong size, a film drawn twice
        // because something is running one on a channel as well -- has had to
        // be diagnosed from a photograph until now. This is the compositor
        // saying what it is about to paint, at the moment it looks wrong.
        // C plays the next thing the chapter carries and never shows: three
        // finished handlers nothing in the game calls. See entry 185.
        if input.pressed.contains(&crate::host::Key::Cut) {
            let domain = game.node().domain.clone();
            let carried = crate::natives::cut::in_chapter(&domain);
            if carried.is_empty() {
                println!("-- nothing cut in {domain} --");
            } else {
                let cut = carried[cut_next % carried.len()];
                cut_next += 1;
                println!("-- {}: {} --", cut.name, cut.about);
                let mut out = crate::script::Outcome::default();
                crate::natives::cut::call(cut.name, &[], &mut game.state, &mut out);
                // A handler with a guard does nothing out of context, and a
                // key that silently does nothing is worse than no key.
                if out.effects.is_empty() {
                    match cut.needs {
                        Some(needs) => println!("   nothing to show: it wants {needs}"),
                        None => println!("   nothing to show"),
                    }
                }
                game.play_outcome(out);
                dirty = true;
            }
        }
        if input.pressed.contains(&crate::host::Key::Stage) {
            println!("-- stage, bottom to top --");
            for line in game.stage_report() {
                println!("   {line}");
            }
        }

        // Already in stage coordinates: the host maps them back, because it
        // is the only thing that knows how it scaled the frame.
        let pos = input.pointer;
        // Which directions the room offers now. Cheap -- a dozen guards -- and
        // it has to be current, because the whole point of the pad is that it
        // shows only what is actually there.
        let dirs = if menu.is_none() && settings.pad && !game.waiting_for_click() {
            game.live_directions()
        } else {
            Vec::new()
        };
        // A tap on the pad *becomes* the click a player would have made on the
        // scene, rather than dispatching a move of its own. So it takes the
        // same path, honours the same guards, and records the same step -- a
        // recording made with a thumb replays identically under a mouse.
        let pad_target = pos.and_then(|(x, y)| crate::menu::pad_hit(x, y, &dirs, STAGE_W));
        // `if the mouseV > gInventoryTopY` -- the bar lights up under the
        // cursor and goes back to outlines when it leaves, and the original
        // redraws the stage on each crossing.
        let over_bar = pos.is_some_and(|(_, y)| y > crate::inventory::Inventory::top_y(STAGE_H as i32));
        if over_bar != inventory_hot {
            inventory_hot = over_bar;
            dirty = true;
        }

        // Report the room and the affordance under the cursor in the title bar,
        // which stands in for the cursor art until that is wired up.
        let room = game.node();
        let name = room.name.clone().unwrap_or_else(|| format!("#{}", room.index));
        let hint = pos
            .and_then(|(x, y)| game.hotspot_at(x, y))
            .map(|(v, _)| cursor_hint(v))
            .unwrap_or("");
        let title = if hint.is_empty() {
            format!("Amber - {} / {name}", room.domain)
        } else {
            format!("Amber - {} / {name} - {hint}", room.domain)
        };
        if title != last_title {
            host.set_title(&title);
            last_title = title.clone();
        }

        // Act on the release edge, so a click cannot fire twice.
        let down = input.down;
        // A dial that asked to keep turning does so while the button is held.
        if let Some(outcome) = game.tick_held(down) {
            // Same again: a held dial's repeat goes through `pump` like any
            // other action, so its effects are already queued and applying
            // them here would apply them twice.
            if outcome.redraw || outcome.destination.is_some() || !outcome.effects.is_empty() {
                dirty = true;
            }
        }
        // A sequence that is still running takes the click with it.
        //
        // These sequences open with `cursorOff` and the original hides the
        // pointer for their duration: the player watches a door swing or a
        // chapter change and cannot walk out of it half way. Letting a click
        // through moved the room while the film was still playing, which left
        // the film running over the room it had moved to.
        // (`was_down` is carried at the foot of the loop, so the busy
        // branch only has to decline the click.)
        // A queue waiting for a *click* is the exception, and it has to be the
        // same exception the replay gate above makes: the PeeK unit holds there
        // until it is dismissed, and the only thing that can dismiss it is a
        // click. Without this the live player is stuck the moment they open it
        // -- which the replay path could not show, because it had already been
        // taught this and the live path had not. Third time a wait has lived in
        // two places and only one of them learned something.
        let modal = game.waiting_for_click();
        if (!game.effects_busy() || modal) && was_down && !down {
            if let Some((x, y)) = close_at.take().or(pad_target).or(pos) {
                // The bar sits over the stage, so it gets first refusal on a
                // click; otherwise picking an item would also walk the player
                // through whatever hotspot lies beneath it.
                // While something is modal the click belongs to it, so the
                // bar does not get first refusal: stowing an item instead of
                // dismissing the unit is how it stays open for ever.
                if !modal && game.click_inventory(x, y, STAGE_W as i32, STAGE_H as i32) {
                    crate::record::step(&format!("inv {x} {y}"));
                    dirty = true;
                    was_down = down;
                    continue;
                }
                let had_movie = game.player.is_some();
                // Recorded before the click is taken, so a click that turns
                // out to crash is still in the file.
                if crate::record::active() {
                    let here = game.node().name.clone().unwrap_or_default();
                    crate::record::note(&format!("in {here}"));
                    crate::record::step(&format!("click {x} {y}"));
                }
                if let Some(outcome) = game.click(x, y) {
                    // A move cuts whatever the previous scene was playing.
                    if outcome.destination.is_some() || outcome.go_back {
                        game.clear_puppets();
                        if had_movie {
                            if let Some(a) = &audio {
                                // Keep the house ambience across a move; only
                                // the previous scene's own audio stops.
                                a.stop_oneshots();
                            }
                        }
                        playing_soundtrack = false;
                    }
                    // Any action can change state, and sprite visibility is
                    // driven by state, so the stage has to recompose. The room
                    // scripts rely on this: opening the front door is a bare
                    // setState with no updateDisplay, because Director
                    // refreshed the stage on its own. Recomposing is cheap.
                    dirty = true;
                    apply_effects(&mut *game, audio.as_ref(), &mut dirty, &mut playing_soundtrack);
                }
            }
        }
        was_down = down;

        match dissolve {
            Some((progress, t)) => blend(&mut out, &outgoing, &frame, progress, t),
            None => out.copy_from_slice(&frame),
        }
        if show_hotspots {
            let state = game.state.clone();
            for h in &game.node().hotspots {
                if h.actions.is_empty() || !state.test(&h.condition) {
                    continue;
                }
                cursor::outline(
                    &mut out,
                    STAGE_W as i32,
                    STAGE_H as i32,
                    h.bounds,
                    0xff00_ff00,
                );
            }
        }
        // A sequence takes the pointer away until it is done: `cursorOff` at
        // the top of a set piece, and the queue running dry is the `cursorOn`
        // this engine does not otherwise get.
        if game.cursor_hidden && !game.effects_busy() && game.script_idle() {
            game.cursor_hidden = false;
        }
        if let Some((mx, my)) = pos.filter(|_| !game.cursor_hidden && (input.hover || down)) {
            let verb = game.hotspot_at(mx, my).map(|(v, _)| v);
            // The game's own art first; the drawn shapes are what is left when
            // a cursor is a system one -- `#back` and `#noCursor` have no cast
            // behind them -- so the player is never without a pointer.
            if !game.draw_cursor(&mut out, STAGE_W as u32, STAGE_H as u32, verb, mx, my) {
                cursor::draw(&mut out, STAGE_W as i32, STAGE_H as i32, mx, my, verb);
            }
        }
        if menu.is_none() {
            crate::menu::draw_pad(&mut out, STAGE_W, STAGE_H, &dirs, pos);
        }
        crate::menu::draw_hud(&mut out, STAGE_W, STAGE_H, game.can_skip(), way_out.is_some(), pos);
        if let Some(open) = &menu {
            open.draw(&mut out, STAGE_W, STAGE_H, pos);
        }
        if factor == 1 && settings.filter == crate::scale::Filter::Nearest {
            host.present(&out, STAGE)?;
        } else {
            let grown = crate::scale::up(&out, STAGE_W, STAGE_H, factor, settings.filter);
            host.present(&grown, shown)?;
        }
    }
    Ok(())
}

/// Applies the effects that are due, honouring the waits between them.
///
/// Called once a frame rather than only after a click, because a handler's
/// sequence can hold: the mirror message suspends the ambience, plays a film,
/// waits for it to end, and only then restores. Draining the whole queue in
/// the frame the click arrived ran all of that in one instant.
/// Mixes two composed stages, `progress` of the way from `from` to `to`.
fn blend(out: &mut [u32], from: &[u32], to: &[u32], progress: f32, t: crate::game::Transition) {
    use crate::game::Wipe;
    let p = progress.clamp(0.0, 1.0);
    if t.kind == Wipe::Dissolve {
        for ((o, a), b) in out.iter_mut().zip(from).zip(to) {
            let mix = |shift: u32| {
                let a = ((a >> shift) & 0xff) as f32;
                let b = ((b >> shift) & 0xff) as f32;
                ((a + (b - a) * p) as u32) & 0xff
            };
            *o = 0xff00_0000 | (mix(16) << 16) | (mix(8) << 8) | mix(0);
        }
        return;
    }
    // A wipe has no blending in it at all: a hard edge crosses the stage and
    // the new image is simply on one side of it. Director advances that edge
    // in chunks rather than a pixel at a time, which is why a turn in this
    // game has a texture to it instead of looking smooth.
    let span = match t.kind {
        Wipe::Right | Wipe::Left => STAGE_W,
        _ => STAGE_H,
    };
    let chunk = t.chunk.max(1) as usize;
    let edge = ((p * span as f32) as usize / chunk) * chunk;
    for y in 0..STAGE_H {
        for x in 0..STAGE_W {
            let i = y * STAGE_W + x;
            let new = match t.kind {
                Wipe::Right => x < edge,
                Wipe::Left => x >= STAGE_W - edge,
                Wipe::Down => y < edge,
                Wipe::Up => y >= STAGE_H - edge,
                Wipe::Dissolve => unreachable!(),
            };
            out[i] = if new { to[i] } else { from[i] };
        }
    }
}

/// Applies one effect, for the `mix` command.
pub fn apply_one(game: &mut Game, audio: Option<&Audio>, effect: &Effect) {
    let (mut dirty, mut playing) = (false, false);
    apply_effect(game, audio, effect.clone(), &mut dirty, &mut playing);
}

fn apply_effects(
    game: &mut Game,
    audio: Option<&Audio>,
    dirty: &mut bool,
    playing_soundtrack: &mut bool,
) {
    let effects: Vec<Effect> = game.drain_ready();
    for effect in effects {
        apply_effect(game, audio, effect, dirty, playing_soundtrack);
    }
}

/// Applies one effect: what is drawn, then what is heard.
fn apply_effect(
    game: &mut Game,
    audio: Option<&Audio>,
    effect: Effect,
    dirty: &mut bool,
    playing_soundtrack: &mut bool,
) {
    trace!(crate::trace::Topic::Script, "effect {effect:?}");
    {
    // Channel effects change what is drawn, not what is
    // heard, so they are applied before the audio match.
    if game.apply_puppet(&effect) {
        *dirty = true;
        return;
    }
    let Some(a) = audio else { return };
    match effect {
        Effect::StopGhostCall => {
            if let Some(n) = game.state.get("gLastCall").as_str() {
                a.stop_oneshot(n);
            }
        }
        Effect::PlaySound { name, loudness } => {
            // `ghostCalls` names the levels: [#low: 90, #medium: 180,
            // #high: 255], out of Director's 255.
            let scale = match loudness.as_deref() {
                Some("low") => 90.0 / 255.0,
                Some("medium") => 180.0 / 255.0,
                _ => 1.0,
            };
            let gain = game.sounds.gain(&name) * scale;
            if let Some((pcm, rate, ch)) = game.sound(&name) {
                a.play(Some(&name), None, pcm, rate, ch, gain, false, true);
            }
        }
        Effect::StartLoop { name, volume } => {
            let level = volume.unwrap_or(255) as f32 / 255.0;
            let gain = level * game.sounds.gain(&name);
            if let Some((pcm, rate, ch)) = game.sound(&name) {
                a.play(Some(&name), Some(name.clone()), pcm, rate, ch, gain, true, true);
            }
        }
        // `pushVideo` was reaching here and being
        // discarded, so every montage that plays through it
        // showed nothing and the `wait #videoStop` after it
        // resolved against whatever movie happened to be
        // loaded.
        Effect::PlayVideo(ref which) => {
            game.play_movie(which.as_deref());
            *playing_soundtrack = false;
            *dirty = true;
        }
        Effect::StopLoop { name, .. } => {
            a.stop(&name);
            game.stop_program(&name);
        }
        // Fades are not modelled yet; the duck itself is
        // what the scripts rely on.
        Effect::SuspendSounds { .. } => a.set_suspended(true),
        Effect::RestoreSounds { .. } => a.set_suspended(false),
        Effect::StopVideo => {
            game.player = None;
            *playing_soundtrack = false;
        }
        // Declared, emitted in three places, and applied in none of them
        // until now -- the fourth time this has happened, after PlayVideo,
        // new_domain and FadeToMontage. The catch-all arm below is what makes
        // it silent, so this one is worth stating plainly: the radio dial and
        // the weather vane are both movies scrubbed by hand, and without this
        // they moved their state and showed nothing.
        //
        // A zero-length segment parks on a frame rather than playing, which
        // is what entering a room with the vane already turned needs.
        Effect::PlayVideoSegment { from, to } => {
            if game.player.is_none() {
                game.start_room_video();
            }
            if let Some(player) = &mut game.player {
                player.play_segment(from, to);
            }
            *dirty = true;
        }
        _ => {}
    }
}
}


#[cfg(test)]
mod tests {
    use super::{blend, STAGE_H, STAGE_W};
    use crate::game::{Transition, Wipe};

    const FADE: Transition = Transition { kind: Wipe::Dissolve, step: 1.0 / 30.0, chunk: 0 };

    #[test]
    fn a_dissolve_starts_on_the_old_stage_and_ends_on_the_new() {
        let from = vec![0xff00_0000u32; 4];
        let to = vec![0xffff_ffffu32; 4];
        let mut out = vec![0u32; 4];
        blend(&mut out, &from, &to, 0.0, FADE);
        assert_eq!(out[0], 0xff00_0000);
        blend(&mut out, &from, &to, 1.0, FADE);
        assert_eq!(out[0], 0xffff_ffff);
    }

    #[test]
    fn half_way_is_half_way_in_every_channel() {
        let from = vec![0xff00_0000u32];
        let to = vec![0xffff_ffffu32];
        let mut out = vec![0u32; 1];
        blend(&mut out, &from, &to, 0.5, FADE);
        for shift in [0, 8, 16] {
            let v = (out[0] >> shift) & 0xff;
            assert!((126..=128).contains(&v), "channel {shift} was {v}");
        }
    }

    #[test]
    fn progress_outside_the_range_does_not_wrap_a_channel() {
        // A frame that overruns must clamp rather than roll over, which would
        // flash the wrong colour on the last frame of every transition.
        let from = vec![0xff00_0000u32];
        let to = vec![0xffff_ffffu32];
        let mut out = vec![0u32; 1];
        for p in [-1.0, 1.5, 99.0] {
            blend(&mut out, &from, &to, p, FADE);
            let v = out[0] & 0xff;
            assert!(v == 0 || v == 255, "clamped to {v} at {p}");
        }
    }

    #[test]
    fn a_turn_wipes_rather_than_fading() {
        // `#turnLeft` is Director's code 1, a hard edge travelling right, so
        // the new view enters at the left. Nothing is ever blended: every
        // pixel is one image or the other. Fading these instead -- which this
        // engine did for every one of the game's three thousand eight hundred
        // moves -- loses the cue that says the camera turned.
        let from = vec![0xff00_0000u32; STAGE_W * STAGE_H];
        let to = vec![0xffff_ffffu32; STAGE_W * STAGE_H];
        let mut out = vec![0u32; STAGE_W * STAGE_H];
        let turn = Transition { kind: Wipe::Right, step: 1.0 / 15.0, chunk: 16 };
        blend(&mut out, &from, &to, 0.5, turn);

        assert!(out.iter().all(|&p| p == 0xff00_0000 || p == 0xffff_ffff));
        assert_eq!(out[0], 0xffff_ffff, "the left of the stage has changed");
        assert_eq!(out[STAGE_W - 1], 0xff00_0000, "the right has not yet");
        // And the edge sits on a chunk boundary, which is what makes a
        // Director wipe look like one.
        let row = &out[..STAGE_W];
        let edge = row.iter().position(|&p| p == 0xff00_0000).unwrap();
        assert_eq!(edge % 16, 0, "edge at {edge} is not on a 16 pixel chunk");
    }

    #[test]
    fn turning_the_other_way_wipes_the_other_way() {
        let from = vec![0xff00_0000u32; STAGE_W * STAGE_H];
        let to = vec![0xffff_ffffu32; STAGE_W * STAGE_H];
        let mut out = vec![0u32; STAGE_W * STAGE_H];
        let turn = Transition { kind: Wipe::Left, step: 1.0 / 15.0, chunk: 16 };
        blend(&mut out, &from, &to, 0.5, turn);
        assert_eq!(out[0], 0xff00_0000);
        assert_eq!(out[STAGE_W - 1], 0xffff_ffff);
    }

    #[test]
    fn looking_up_and_down_wipe_vertically() {
        let from = vec![0xff00_0000u32; STAGE_W * STAGE_H];
        let to = vec![0xffff_ffffu32; STAGE_W * STAGE_H];
        let mut out = vec![0u32; STAGE_W * STAGE_H];
        let up = Transition { kind: Wipe::Down, step: 1.0 / 15.0, chunk: 16 };
        blend(&mut out, &from, &to, 0.5, up);
        assert_eq!(out[0], 0xffff_ffff, "the top has changed");
        assert_eq!(out[(STAGE_H - 1) * STAGE_W], 0xff00_0000, "the bottom has not");

        let down = Transition { kind: Wipe::Up, step: 1.0 / 15.0, chunk: 16 };
        blend(&mut out, &from, &to, 0.5, down);
        assert_eq!(out[0], 0xff00_0000);
        assert_eq!(out[(STAGE_H - 1) * STAGE_W], 0xffff_ffff);
    }
}

#[cfg(test)]
mod effect_coverage {
    /// Every `Effect` variant must be acted on somewhere.
    ///
    /// Four variants have now been declared, emitted by handlers, carried
    /// through the queue and then dropped on the floor: `PlayVideo`,
    /// `FadeToMontage`, `PlayVideoSegment`, and `Outcome::new_domain` before
    /// them. Each one looked like working code from the handler's side and
    /// each was found by reading rather than by playing, because a catch-all
    /// match arm cannot fail.
    ///
    /// This reads the sources as text rather than reflecting, which is crude,
    /// but it fails the moment somebody adds a fifth variant without an arm --
    /// and that is the whole job.
    #[test]
    fn every_effect_variant_is_applied_somewhere() {
        const SCRIPT: &str = include_str!("script.rs");
        const APPLIERS: [&str; 3] = [
            include_str!("render.rs"),
            include_str!("game.rs"),
            include_str!("audio.rs"),
        ];

        let body = SCRIPT
            .split_once("pub enum Effect {")
            .expect("Effect enum moved")
            .1;
        let body = body.split_once("\n}").expect("Effect enum unterminated").0;

        let variants: Vec<&str> = body
            .lines()
            .map(str::trim)
            .filter(|line| {
                line.starts_with(|c: char| c.is_ascii_uppercase()) && !line.starts_with("///")
            })
            .filter_map(|line| line.split(['(', '{', ',', ' ']).next())
            .filter(|name| !name.is_empty())
            .collect();
        assert!(variants.len() > 15, "only found {variants:?}");

        // `Native` is the one variant that is deliberately never applied.
        // `natives::call` runs first and only pushes it when no handler took
        // the verb, so it is a record that something is unported -- read by
        // `verify` and nothing else. If it ever does get applied, that is the
        // bug.
        let missing: Vec<&str> = variants
            .iter()
            .copied()
            .filter(|v| *v != "Native")
            .filter(|v| {
                // A *match arm*, not a mention. The first version of this test
                // asked whether the name appeared anywhere in a file that
                // applies effects, and `Effect::CursorOff` appeared in a list
                // of effects to emit -- so the test passed for a hundred and
                // four call sites that were dropped on the floor.
                let used = format!("Effect::{v}");
                !APPLIERS.iter().any(|src| {
                    src.match_indices(&used).any(|(at, _)| {
                        let rest = &src[at + used.len()..];
                        let head: String =
                            rest.chars().take(80).collect::<String>().replace('\n', " ");
                        // `Effect::X =>`, or a pattern binding before the arrow.
                        let arm = head.trim_start();
                        arm.starts_with("=>")
                            || (arm.starts_with('{') || arm.starts_with('('))
                                && head.split("=>").next().is_some_and(|before| {
                                    !before.contains(';') && !before.contains(',')
                                        || before.matches('{').count() > 0
                                }) && head.contains("=>")
                    })
                })
            })
            .collect();
        assert!(missing.is_empty(), "Effect variants never applied: {missing:?}");
    }
}


#[cfg(test)]
mod loop_tests {
    use super::*;
    use crate::host::{Host, Input};

    /// A host that plays a fixed script of inputs and then closes.
    ///
    /// The loop had never been driven by a test at all -- only the replay path
    /// had, through `walk` -- which is exactly how a click wait came to be
    /// handled in the replay gate and not in the live one.
    struct Scripted {
        frames: Vec<Input>,
        at: usize,
    }

    impl Host for Scripted {
        fn poll(&mut self, _stage: (usize, usize)) -> Input {
            let frame = self.frames.get(self.at);
            self.at += 1;
            match frame {
                Some(i) => Input {
                    pointer: i.pointer,
                    down: i.down,
                    pressed: i.pressed.clone(),
                    open: true,
                    hover: true,
                },
                None => Input {
                    pointer: None,
                    down: false,
                    pressed: Vec::new(),
                    open: false,
                    hover: true,
                },
            }
        }
        fn present(&mut self, _frame: &[u32], _stage: (usize, usize)) -> std::io::Result<()> {
            Ok(())
        }
        fn set_title(&mut self, _title: &str) {}
    }

    fn at(x: i32, y: i32, down: bool) -> Input {
        Input { pointer: Some((x, y)), down, pressed: Vec::new(), open: true, hover: true }
    }

    /// Pressing CLOSE actually delivers a click.
    ///
    /// It computed the right point and never spent it: the button block zeroed
    /// `input.down`, so the release edge the dispatch waits on never happened
    /// and the button did nothing at all.
    #[test]
    fn the_close_button_delivers_its_click() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../extract");
        if !root.is_dir() {
            return;
        }
        let mut game = Game::new(&root).expect("extract/ is not a game");
        game.skip_opening();
        game.pending.clear();
        game.pending.push(Effect::WaitForClick);

        // Somewhere on the CLOSE button, found the way the player finds it.
        let spot = (0..STAGE_W as i32)
            .step_by(4)
            .flat_map(|x| (0..60).step_by(4).map(move |y| (x, y)))
            .find(|(x, y)| {
                crate::menu::hud_hit(*x, *y, STAGE_W, false, true)
                    == Some(crate::menu::Tap::Close)
            })
            .expect("no CLOSE button to press");

        let mut host = Scripted {
            frames: vec![
                at(spot.0, spot.1, false),
                at(spot.0, spot.1, true),
                at(spot.0, spot.1, false),
                at(spot.0, spot.1, false),
            ],
            at: 0,
        };
        run(&mut game, &mut host, None, Vec::new(), true, 1, crate::scale::Filter::Nearest, None)
            .expect("the loop failed");

        assert!(!game.waiting_for_click(), "CLOSE was pressed and nothing happened");
    }

    /// A click gets through to a queue that is holding for one.
    ///
    /// Opening the PeeK unit holds the queue on `Wait::Click` until it is
    /// dismissed. The live dispatch used to be gated on the queue being idle,
    /// so the only click that could clear it never arrived and the player was
    /// stuck the moment they picked the unit up.
    #[test]
    fn a_click_reaches_a_queue_that_is_waiting_for_one() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../extract");
        if !root.is_dir() {
            return;
        }
        let mut game = Game::new(&root).expect("extract/ is not a game");
        // The opening film is already queued and holds on its own video wait,
        // so anything pushed behind it is never reached.
        game.skip_opening();
        game.pending.clear();
        game.pending.push(Effect::WaitForClick);

        // First: prove the wait actually arms. Without this the assert below
        // is vacuous -- it would pass against a game that was never waiting,
        // which is how two earlier tests in this project protected nothing.
        let mut idle = Scripted { frames: (0..8).map(|_| at(0, 0, false)).collect(), at: 0 };
        run(&mut game, &mut idle, None, Vec::new(), true, 1, crate::scale::Filter::Nearest, None)
            .expect("the loop failed");
        assert!(game.waiting_for_click(), "the click wait never armed; the test proves nothing");

        let mut host = Scripted {
            frames: vec![
                at(320, 240, false),
                at(320, 240, true),
                at(320, 240, false),
                at(320, 240, false),
            ],
            at: 0,
        };
        run(
            &mut game,
            &mut host,
            None,
            Vec::new(),
            true,
            1,
            crate::scale::Filter::Nearest,
            None,
        )
        .expect("the loop failed");

        assert!(
            !game.waiting_for_click(),
            "the queue is still holding for a click that was made"
        );
    }
}

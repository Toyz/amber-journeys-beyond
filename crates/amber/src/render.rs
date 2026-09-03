//! Window, input and the main loop.

use std::path::Path;

use minifb::{Key, MouseButton, MouseMode, Window, WindowOptions};

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
    let mut playing_soundtrack = false;
    let mut ambience_room = usize::MAX;
    eprintln!("space skips a movie, tab outlines live hotspots, escape quits");
    // The route has to start somewhere known, so the first line of a
    // recording is the room the game opened in.
    if crate::record::active() {
        if let Some(name) = game.node().name.clone() {
            crate::record::step(&name);
        }
    }
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

    let mut window = Window::new(
        "Amber: Journeys Beyond",
        STAGE_W,
        STAGE_H,
        WindowOptions {
            scale: minifb::Scale::X1,
            resize: true,
            scale_mode: minifb::ScaleMode::AspectRatioStretch,
            ..WindowOptions::default()
        },
    )?;
    // The original runs at a nominal 15 fps; this only caps the loop, and the
    // room is static between clicks anyway.
    window.set_target_fps(60);
    // The game draws its own pointer into the frame, so the desktop's would be
    // a second one sitting on top of it.
    window.set_cursor_visibility(false);

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
    let mut last_frame = std::time::Instant::now();
    while window.is_open() && !window.is_key_down(Key::Escape) {
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
                    let _ = crate::walk::command(&mut game, &cmd, false);
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
            apply_effects(&mut game, audio.as_ref(), &mut dirty, &mut playing_soundtrack);
        }
        if game.room != ambience_room {
            ambience_room = game.room;
            update_ambience(&mut game, audio.as_ref());
        }

        // Space skips whatever movie is playing. The opening is two minutes
        // long and the original had no way past it either, so this is the one
        // deliberate departure from the game's behaviour.
        if window.is_key_pressed(Key::Space, minifb::KeyRepeat::No) && game.skip_video() {
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
            apply_effect(&mut game, audio.as_ref(), effect, &mut dirty, &mut playing_soundtrack);
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
            game.draw(&mut frame, STAGE_W as u32, STAGE_H as u32);
            game.draw_inventory(&mut frame, STAGE_W as u32, STAGE_H as u32, inventory_hot);
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
        if window.is_key_pressed(Key::Tab, minifb::KeyRepeat::No) {
            show_hotspots = !show_hotspots;
        }
        // S prints what is on the stage, bottom to top, into the log. A fault
        // that is only visible -- a film at the wrong size, a film drawn twice
        // because something is running one on a channel as well -- has had to
        // be diagnosed from a photograph until now. This is the compositor
        // saying what it is about to paint, at the moment it looks wrong.
        if window.is_key_pressed(Key::S, minifb::KeyRepeat::No) {
            println!("-- stage, bottom to top --");
            for line in game.stage_report() {
                println!("   {line}");
            }
        }

        // The window may be resized, but the stage is always 640x480 and the
        // scale mode letterboxes it, so mouse coordinates need mapping back.
        let (win_w, win_h) = window.get_size();
        let map = |x: f32, y: f32| -> (i32, i32) {
            let scale = (win_w as f32 / STAGE_W as f32).min(win_h as f32 / STAGE_H as f32);
            let (ox, oy) = (
                (win_w as f32 - STAGE_W as f32 * scale) / 2.0,
                (win_h as f32 - STAGE_H as f32 * scale) / 2.0,
            );
            (((x - ox) / scale) as i32, ((y - oy) / scale) as i32)
        };

        let pos = window.get_mouse_pos(MouseMode::Pass).map(|(x, y)| map(x, y));
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
            window.set_title(&title);
            last_title = title.clone();
        }

        // Act on the release edge, so a click cannot fire twice.
        let down = window.get_mouse_down(MouseButton::Left);
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
        if !game.effects_busy() && was_down && !down {
            if let Some((x, y)) = pos {
                // The bar sits over the stage, so it gets first refusal on a
                // click; otherwise picking an item would also walk the player
                // through whatever hotspot lies beneath it.
                if game.click_inventory(x, y, STAGE_W as i32, STAGE_H as i32) {
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
                    apply_effects(&mut game, audio.as_ref(), &mut dirty, &mut playing_soundtrack);
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
        if let Some((mx, my)) = pos.filter(|_| !game.cursor_hidden) {
            let verb = game.hotspot_at(mx, my).map(|(v, _)| v);
            // The game's own art first; the drawn shapes are what is left when
            // a cursor is a system one -- `#back` and `#noCursor` have no cast
            // behind them -- so the player is never without a pointer.
            if !game.draw_cursor(&mut out, STAGE_W as u32, STAGE_H as u32, verb, mx, my) {
                cursor::draw(&mut out, STAGE_W as i32, STAGE_H as i32, mx, my, verb);
            }
        }
        window.update_with_buffer(&out, STAGE_W, STAGE_H)?;
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

/// Makes the mixer match what the current room asks to hear.
///
/// Shared with the `mix` command, which runs it against a silent mixer so the
/// result can be printed. A room's audio is decided in one place either way.
pub fn update_ambience(game: &mut Game, audio: Option<&Audio>) {
// Ambient loops belong to the room, so they start on arrival and are
// left running until a room wants something different.
    if let Some(a) = audio {
        // Retire loops this room does not want and re-level the ones
        // it does, before starting anything new. Without this the
        // house hum follows the player out onto the grounds, where
        // the room's own mix asks for silence.
        let wanted: Vec<(String, f32)> = game
            .ambience()
            .into_iter()
            .map(|(name, level)| {
                let gain = level * game.sounds.gain(&name);
                (name, gain)
            })
            .collect();
        a.set_loops(&wanted);
        let already = a.playing_loops();

        for (name, level) in game.ambience() {
            if already.contains(&name) {
                continue;
            }
            let gain = level * game.sounds.gain(&name);
            // A radio or clock is a programme of takes played in order,
            // not one looping voice, so it goes to the sequencer. Some
            // groups hold a single take and no running order; those are
            // ordinary loops and the mixer can hold them gaplessly.
            if game.sounds.is_group(&name) {
                if game.start_program(&name, gain) {
                    continue;
                }
                let single = game.sounds.group_items(&name).first().map(|s| s.to_string());
                if let Some(item) = single {
                    if let Some((pcm, rate, ch)) = game.group_sound_public(&name, &item) {
                        a.play(Some(&name), Some(name.clone()), pcm, rate, ch, gain, true, true);
                    }
                }
                continue;
            }
            if let Some((pcm, rate, channels)) = game.sound(&name) {
                a.play(Some(&name), Some(name.clone()), pcm, rate, channels, gain, true, true);
            }
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


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

pub fn play(root: &Path, start: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
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
    let audio = Audio::open();
    match &audio {
        Some(a) => eprintln!("audio out at {} Hz", a.rate()),
        None => eprintln!("no audio device; running silently"),
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

    // `frame` holds the composed scene, redrawn only when it changes;
    // `out` adds the cursor, which follows the mouse every frame.
    let mut frame = vec![0u32; STAGE_W * STAGE_H];
    let mut out = vec![0u32; STAGE_W * STAGE_H];
    let mut show_hotspots = false;
    let mut dirty = true;
    let mut was_down = false;
    let mut last_title = String::new();

    let mut frames: u64 = 0;
    let started = std::time::Instant::now();
    // The stage as it looked before the change a transition is covering, and
    // how far through that transition we are.
    let mut outgoing: Vec<u32> = vec![0; STAGE_W * STAGE_H];
    let mut dissolve: Option<(f32, f32)> = None;
    while window.is_open() && !window.is_key_down(Key::Escape) {
        frames += 1;
        crate::trace::frame(frames);

        // Director's `the ticks` is sixtieths since startup, and the scan
        // unit's timer is an absolute deadline in them. Never advancing it
        // left every such deadline in the past.
        game.state.set(
            "gTicks",
            lingo::Value::Int((started.elapsed().as_secs_f64() * 60.0) as i32),
        );

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
            let outcome = game.pump();
            if outcome.destination.is_some() || outcome.go_back || outcome.redraw {
                dirty = true;
            }
            if !outcome.effects.is_empty() {
                dirty = true;
            }
            for effect in outcome.effects {
                let Some(a) = &audio else { continue };
                match effect {
                    Effect::PlaySound { name, loudness } => {
                        let scale = match loudness.as_deref() {
                            Some("low") => 0.5,
                            Some("medium") => 0.75,
                            _ => 1.0,
                        };
                        let gain = game.sounds.gain(&name) * scale;
                        if let Some((pcm, rate, ch)) = game.sound(&name) {
                            a.play(Some(&name), None, pcm, rate, ch, gain, false, true);
                        }
                    }
                    Effect::StopVideo => {
                        game.player = None;
                        playing_soundtrack = false;
                    }
                    _ => {}
                }
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
        if dirty {
            // A transition covers the change from what is on the stage now to
            // what is about to be, so the outgoing image has to be kept before
            // the new one is composed over it.
            if let Some(step) = game.take_transition() {
                outgoing.copy_from_slice(&frame);
                dissolve = Some((0.0, step));
            }
            game.draw(&mut frame, STAGE_W as u32, STAGE_H as u32);
            game.draw_inventory(&mut frame, STAGE_W as u32, STAGE_H as u32);
            dirty = false;
        }

        // Director's `#fadeIn` is a dissolve, not a cut. The game asks for one
        // a hundred and six times -- every door, every close-up, every step of
        // a montage -- and without it each of those is a hard jump.
        if let Some((progress, step)) = dissolve {
            let progress = progress + step;
            if progress >= 1.0 {
                dissolve = None;
            } else {
                dissolve = Some((progress, step));
            }
        }
        // Tab shows where the live hotspots actually are, which is the quickest
        // way to tell a missing exit from one that is merely hard to find.
        if window.is_key_pressed(Key::Tab, minifb::KeyRepeat::No) {
            show_hotspots = !show_hotspots;
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
            if outcome.redraw || outcome.destination.is_some() {
                dirty = true;
            }
            for effect in outcome.effects {
                if game.apply_puppet(&effect) {
                    dirty = true;
                    continue;
                }
                if let (Some(a), Effect::PlaySound { name, .. }) = (&audio, &effect) {
                    let gain = game.sounds.gain(name);
                    if let Some((pcm, rate, ch)) = game.sound(name) {
                        a.play(Some(&name), None, pcm, rate, ch, gain, false, true);
                    }
                }
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
            Some((progress, _)) => blend(&mut out, &outgoing, &frame, progress),
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
        if let Some((mx, my)) = pos {
            let verb = game.hotspot_at(mx, my).map(|(v, _)| v);
            cursor::draw(&mut out, STAGE_W as i32, STAGE_H as i32, mx, my, verb);
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
fn blend(out: &mut [u32], from: &[u32], to: &[u32], progress: f32) {
    let t = progress.clamp(0.0, 1.0);
    for ((o, a), b) in out.iter_mut().zip(from).zip(to) {
        let mix = |shift: u32| {
            let a = ((a >> shift) & 0xff) as f32;
            let b = ((b >> shift) & 0xff) as f32;
            ((a + (b - a) * t) as u32) & 0xff
        };
        *o = 0xff00_0000 | (mix(16) << 16) | (mix(8) << 8) | mix(0);
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
        Effect::PlaySound { name, loudness } => {
            // The loudness word is a coarse mix hint, not a
            // level: quiet lines sit under the ambience.
            let scale = match loudness.as_deref() {
                Some("low") => 0.5,
                Some("medium") => 0.75,
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
            if already.iter().any(|k| *k == name) {
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
    use super::blend;

    #[test]
    fn a_dissolve_starts_on_the_old_stage_and_ends_on_the_new() {
        let from = vec![0xff00_0000u32; 4];
        let to = vec![0xffff_ffffu32; 4];
        let mut out = vec![0u32; 4];
        blend(&mut out, &from, &to, 0.0);
        assert_eq!(out[0], 0xff00_0000);
        blend(&mut out, &from, &to, 1.0);
        assert_eq!(out[0], 0xffff_ffff);
    }

    #[test]
    fn half_way_is_half_way_in_every_channel() {
        let from = vec![0xff00_0000u32];
        let to = vec![0xffff_ffffu32];
        let mut out = vec![0u32; 1];
        blend(&mut out, &from, &to, 0.5);
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
            blend(&mut out, &from, &to, p);
            let v = out[0] & 0xff;
            assert!(v == 0 || v == 255, "clamped to {v} at {p}");
        }
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
                let used = format!("Effect::{v}");
                !APPLIERS.iter().any(|src| {
                    // The declaration site does not count, only a use in a
                    // file that acts on effects.
                    src.matches(&used).count() > 0
                })
            })
            .collect();
        assert!(missing.is_empty(), "Effect variants never applied: {missing:?}");
    }
}


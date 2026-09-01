//! Window, input and the main loop.

use std::path::Path;
use std::sync::Arc;

use minifb::{Key, MouseButton, MouseMode, Window, WindowOptions};

use crate::audio::Audio;
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
        match game.world.resolve(name, None) {
            Some(i) => game.room = i,
            None => eprintln!("warning: no room named {name}, starting at the default"),
        }
    }

    if start.is_some() {
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
    eprintln!(
        "starting in {} / {}{}",
        game.node().domain,
        game.node().name.clone().unwrap_or_default(),
        match game.video() {
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

    let mut frame = vec![0u32; STAGE_W * STAGE_H];
    let mut dirty = true;
    let mut was_down = false;
    let mut last_title = String::new();

    while window.is_open() && !window.is_key_down(Key::Escape) {
        // Ambient loops belong to the room, so they start on arrival and are
        // left running until a room wants something different.
        if game.room != ambience_room {
            ambience_room = game.room;
            if let Some(a) = &audio {
                for (name, level) in game.ambience() {
                    let gain = level * game.sounds.gain(&name);
                    if let Some((pcm, rate, channels)) = game.sound(&name) {
                        a.play(Some(name), pcm, rate, channels, gain, true);
                    }
                }
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
                        Arc::clone(&player.audio),
                        player.audio_rate,
                        player.audio_channels,
                        1.0,
                        false,
                    );
                }
                playing_soundtrack = true;
            }
        } else {
            playing_soundtrack = false;
        }
        if dirty {
            game.draw(&mut frame, STAGE_W as u32, STAGE_H as u32);
            dirty = false;
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
        if was_down && !down {
            if let Some((x, y)) = pos {
                let had_movie = game.player.is_some();
                if let Some(outcome) = game.click(x, y) {
                    // A move cuts whatever the previous scene was playing.
                    if outcome.destination.is_some() || outcome.go_back {
                        if had_movie {
                            if let Some(a) = &audio {
                                // Keep the house ambience across a move; only
                                // the previous scene's own audio stops.
                                a.stop_oneshots();
                            }
                        }
                        playing_soundtrack = false;
                    }
                    if outcome.destination.is_some() || outcome.go_back || outcome.redraw {
                        dirty = true;
                    }
                    let effects: Vec<Effect> = game.pending.drain(..).collect();
                    for effect in effects {
                        if std::env::var_os("AMBER_TRACE").is_some() {
                            eprintln!("  effect: {effect:?}");
                        }
                        let Some(a) = &audio else { continue };
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
                                    a.play(None, pcm, rate, ch, gain, false);
                                }
                            }
                            Effect::StartLoop { name, volume } => {
                                let level = volume.unwrap_or(255) as f32 / 255.0;
                                let gain = level * game.sounds.gain(&name);
                                if let Some((pcm, rate, ch)) = game.sound(&name) {
                                    a.play(Some(name), pcm, rate, ch, gain, true);
                                }
                            }
                            Effect::StopLoop { name, .. } => a.stop(&name),
                            // Fades are not modelled yet; the duck itself is
                            // what the scripts rely on.
                            Effect::SuspendSounds { .. } => a.set_master(0.25),
                            Effect::RestoreSounds { .. } => a.set_master(1.0),
                            Effect::StopVideo => {
                                game.player = None;
                                playing_soundtrack = false;
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        was_down = down;

        window.update_with_buffer(&frame, STAGE_W, STAGE_H)?;
    }
    Ok(())
}

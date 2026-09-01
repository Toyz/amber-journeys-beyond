//! Window, input and the main loop.

use std::path::Path;

use minifb::{Key, MouseButton, MouseMode, Window, WindowOptions};

use crate::game::Game;
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

    // The game opens on its intro movie, and a room whose only element is a
    // movie renders as black while playback is unimplemented. Say so and move
    // to the first room that has art, rather than presenting a blank window.
    if start.is_none() && game.draws_nothing() {
        let room = game.node().name.clone().unwrap_or_default();
        let movie = game.video().unwrap_or("a movie").to_string();
        eprintln!("note: the game opens at {room}, which plays {movie} and draws nothing else.");
        eprintln!("      Video playback is not implemented yet, so starting at the first room");
        eprintln!("      with art instead. Pass a room name to override.");
        if let Some(i) = game.first_playable() {
            game.room = i;
        }
    }
    eprintln!(
        "starting in {} / {}",
        game.node().domain,
        game.node().name.clone().unwrap_or_default()
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
                if let Some(outcome) = game.click(x, y) {
                    if outcome.destination.is_some() || outcome.go_back || outcome.redraw {
                        dirty = true;
                    }
                    // Effects are collected but not yet played; drain them so
                    // the queue cannot grow without bound.
                    for effect in game.pending.drain(..) {
                        if std::env::var_os("AMBER_TRACE").is_some() {
                            eprintln!("  effect: {effect:?}");
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

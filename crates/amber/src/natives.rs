//! Set-piece handlers ported from the movies' compiled Lingo.
//!
//! The room scripts call these by name and the engine records any it cannot
//! perform as [`Effect::Native`]. Each one implemented here was read from the
//! disassembled bytecode of the movie that defines it; the comment above each
//! gives that reading so the port can be checked against the original.
//!
//! Handlers are added as they are decoded. Anything still unported keeps
//! falling through to `Effect::Native`, so the engine's own report stays an
//! honest measure of what is left.

use lingo::Value;

use crate::script::{Effect, Outcome};
use crate::state::State;

/// Runs a named handler, returning false when it is not implemented yet.
///
/// `args` are already evaluated. Effects the handler produces are appended to
/// `out` so they interleave with the calling script's own timeline.
pub fn call(name: &str, args: &[Value], state: &mut State, out: &mut Outcome) -> bool {
    match name {
        // on enableGust
        //   setState(oStoryteller, #gustEnabled, 1)
        "enablegust" => state.set("gustEnabled", Value::Int(1)),
        // on disableGust
        //   setState(oStoryteller, #gustEnabled, 0)
        "disablegust" => state.set("gustEnabled", Value::Int(0)),

        // on enableSongs
        //   setState(oStoryteller, #carolsEnabled, 1)
        "enablesongs" => state.set("carolsEnabled", Value::Int(1)),
        // on disableSongs
        //   setState(oStoryteller, #carolsEnabled, 0)
        //   killSongs()
        "disablesongs" => {
            state.set("carolsEnabled", Value::Int(0));
            out.effects.push(Effect::StopLoop {
                name: "carols".into(),
                fade: false,
            });
        }

        // on freezeInventory
        //   oPuppeteer cursor #cool
        //   setState(oStoryteller, #inventoryStatus, #cool)
        //   gFreezeInventory = 1
        "freezeinventory" => {
            state.set("inventoryStatus", Value::Symbol("cool".into()));
            state.set("gFreezeInventory", Value::Int(1));
        }

        // on beeSwarm
        //   if random(3) = 3 then soundEffect #beeSwarm
        //
        // The original rolls a die so the swarm is only sometimes heard. The
        // roll is reproduced rather than firing every time, because the
        // intermittency is the effect.
        "beeswarm" => {
            if roll(state, 3) == 3 {
                out.effects.push(Effect::PlaySound {
                    name: "beeSwarm".into(),
                    loudness: None,
                });
            }
        }

        // These are `nothing` in the shipped movies: hooks the authors left
        // wired up but empty. Implemented as no-ops so they stop being
        // reported as missing.
        "disablepeekalert" | "enablepeekalert" | "initboxpuzzle" | "idle" | "nothing" => {}

        _ => return false,
    }
    true
}

/// A small deterministic die roll, seeded from a counter kept in game state.
///
/// The original calls Lingo's `random`. Using a state-backed counter rather
/// than a system source keeps a save reproducible, which matters because these
/// rolls gate audible events and a replayed save should sound the same.
fn roll(state: &mut State, sides: i32) -> i32 {
    let seed = state.get("gRandomSeed").as_int().unwrap_or(1) as u32;
    // A small linear congruential step; the exact sequence does not need to
    // match the original, only to be varied and repeatable.
    let next = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    state.set("gRandomSeed", Value::Int(next as i32));
    ((next >> 16) % sides.max(1) as u32) as i32 + 1
}

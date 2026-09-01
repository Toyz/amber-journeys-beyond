//! Handlers the chapters share, and the hooks that ship empty.
//!
//! Each handler carries the disassembly it was read from, so the port can be
//! checked against the original without going back to the bytecode.

use lingo::Value;

use crate::script::{Effect, Outcome};
use crate::state::State;


/// Runs a handler from this chapter, or reports that it is not one of ours.
pub fn call(name: &str, args: &[Value], state: &mut State, out: &mut Outcome) -> bool {
    // Arguments and effects are unused by some chapters until more handlers
    // land here; the signature is uniform so the dispatcher stays simple.
    let _ = (args, &out, &state);
    match name {
        // on puppetSprite channel, on
        //   Takes a sprite channel away from the score so a script can drive
        //   it, or hands it back. The channels the game claims are 30, 39, 44
        //   and 45, which carry the animated parts of the puzzles.
        "puppetsprite" => {
            let channel = args.first().and_then(Value::as_int).unwrap_or(0);
            let on = args.get(1).map_or(true, |v| v.truthy());
            if channel > 0 {
                out.effects.push(Effect::PuppetSprite {
                    channel: channel as u8,
                    on,
                });
            }
        }

        // Preload hints for the laptop's animated controls; the engine decodes
        // on demand, so there is nothing to prepare.
        "loadmultiframes" | "purgemultiframes" => {}


        // These are `nothing` in the shipped movies: hooks the authors left
        // wired up but empty. Implemented as no-ops so they stop being
        // reported as missing.
        "disablepeekalert" | "enablepeekalert" | "initboxpuzzle" | "idle" | "nothing" => {}

        _ => return false,
    }
    true
}

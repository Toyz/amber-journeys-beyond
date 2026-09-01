//! Margaret's chapter: the house as it was, the radio and the clocks.
//!
//! Each handler carries the disassembly it was read from, so the port can be
//! checked against the original without going back to the bytecode.
//!
//! Most of Margaret's set pieces drive sprite channels directly, through
//! `puppetSprite` and by assigning a sprite's cast member and position. The
//! engine draws the sprites a room declares in its `#onStage` list and has no
//! path for that, so the radio dial, the door static and the telegram wait on
//! the renderer rather than on being decoded.

use lingo::Value;

use crate::script::{Effect, Outcome};
use crate::state::State;

/// Runs a handler from this chapter, or reports that it is not one of ours.
pub fn call(name: &str, _args: &[Value], state: &mut State, out: &mut Outcome) -> bool {
    match name {
        // on resetBoxPuzzle
        //   killVideo
        //   setProp(oStoryteller, #boxList, [])
        //
        // Clears the boxes the player has opened, so the puzzle can be worked
        // through again from the start.
        "resetboxpuzzle" => {
            out.effects.push(Effect::StopVideo);
            state.set("boxList", Value::List(Vec::new()));
        }
        _ => return false,
    }
    true
}

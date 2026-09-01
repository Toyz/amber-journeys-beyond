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
        // on newDoorStatic
        //   puppetSprite 45, 1
        //   loopClip = getProp(oPuppeteer, #doorStatic)
        //   preLoadCast loopClip
        //   set the castNum of sprite 45 = loopClip
        //   set the visible of sprite 45 = 0
        //   set the loc of sprite 45 = point(...) + gOriginPoint
        //   if gCPU <> #Mac then setLoop #loopingStatic
        //   if gCPU <> #Mac then suspendSounds
        //   pushVideo
        //   repeat until the movie ends or the mouse is pressed
        //
        // The static that plays over a doorway between the house's two
        // periods. The channel is claimed and prepared hidden, then the movie
        // runs over it. This port behaves as the Windows build, whose branches
        // are the ones the data fills in.
        "newdoorstatic" => {
            out.effects.push(Effect::PuppetSprite {
                channel: 45,
                on: true,
            });
            out.effects.push(Effect::SpriteCastNamed {
                channel: 45,
                name: "doorStatic".into(),
            });
            out.effects.push(Effect::SpriteVisible {
                channel: 45,
                visible: false,
            });
            out.effects.push(Effect::StartLoop {
                name: "loopingStatic".into(),
                volume: None,
            });
            out.effects.push(Effect::SuspendSounds { fade: false });
            out.effects.push(Effect::PlayVideo(None));
            out.effects.push(Effect::WaitForVideo);
            out.effects.push(Effect::PuppetSprite {
                channel: 45,
                on: false,
            });
        }

        _ => return false,
    }
    true
}

//! Brice's chapter: the grounds, the shed and the bees.
//!
//! Each handler carries the disassembly it was read from, so the port can be
//! checked against the original without going back to the bytecode.

use lingo::Value;

use crate::script::{Effect, Outcome};
use crate::state::State;

use super::roll;

/// Runs a handler from this chapter, or reports that it is not one of ours.
pub fn call(name: &str, args: &[Value], state: &mut State, out: &mut Outcome) -> bool {
    // Arguments and effects are unused by some chapters until more handlers
    // land here; the signature is uniform so the dispatcher stays simple.
    let _ = (args, &out, &state);
    match name {
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


        // on shedAutoSlam
        //   if getState(oStoryteller, #shedDoorIsOpen) = 1 then
        //     setState(oStoryteller, #shedDoorIsOpen, 0)
        //     setLoop #Shed
        //
        // The door swings shut by itself, and the ambience changes with it.
        "shedautoslam" => {
            if state.get("shedDoorIsOpen").as_int() == Some(1) {
                state.set("shedDoorIsOpen", Value::Int(0));
                out.effects.push(Effect::StartLoop {
                    name: "Shed".into(),
                    volume: None,
                });
            }
        }


        // on curseWeeds howLikely
        //   clamps howLikely into 1..6, defaulting to 3 when not a number,
        //   then draws a curse from a list.
        //
        // Only the clamp is ported; the drawn line is a spoken cue and goes
        // out as a sound so the pacing is right even before the list is read.
        "curseweeds" => {
            let likely = args
                .first()
                .and_then(Value::as_int)
                .unwrap_or(3)
                .clamp(1, 6);
            if roll(state, 6) <= likely {
                out.effects.push(Effect::PlaySound {
                    name: "damnWeeds".into(),
                    loudness: None,
                });
            }
        }

        // on toggleTrapDoor
        //   doorIsOpen = getState(oStoryteller, #trapDoorIsOpen)
        //   if doorIsOpen then
        //     setState(oStoryteller, #trapDoorIsOpen, 0) : soundEffect #closeTrap
        //   else
        //     setState(oStoryteller, #trapDoorIsOpen, 1) : soundEffect #openTrap
        "toggletrapdoor" => {
            let open = state.get("trapDoorIsOpen").truthy();
            state.set("trapDoorIsOpen", Value::Int(!open as i32));
            out.effects.push(Effect::PlaySound {
                name: if open { "closeTrap" } else { "openTrap" }.into(),
                loudness: None,
            });
        }

        // on testClosetLock
        //   if getState(#gazFlag) = #flying then
        //     if getState(#closetDoorIsOpen) = #ajar then
        //       goTo #Basement_closet, #backOff
        //
        // The closet only opens once the weathervane is flying and the door
        // has been worked loose; otherwise the click does nothing.
        "testclosetlock" => {
            let flying = state
                .get("gazFlag")
                .as_str()
                .is_some_and(|v| v.eq_ignore_ascii_case("flying"));
            let ajar = state
                .get("closetDoorIsOpen")
                .as_str()
                .is_some_and(|v| v.eq_ignore_ascii_case("ajar"));
            if flying && ajar {
                out.destination = Some("Basement_closet".into());
                out.transition = Some("backOff".into());
            }
        }

        _ => return false,
    }
    true
}

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

        // on panelButton whichButton
        //   if inState(#panelGuess, whichButton) then
        //     trimState #panelGuess, whichButton : cue #CPbuttonUp
        //   else
        //     addState #panelGuess, whichButton : cue #CPbuttonDn
        //   updateDisplay
        //   repeat over [#A1, #A2, #B2, #B3]
        //     if not inState(#panelGuess, i) then exit
        //   repeat over [#A3, #B1]
        //     if inState(#panelGuess, j) then exit
        //   setState(oStoryteller, #controlPanel, #closed)
        //   updateDisplay
        //   goTo #basement_doorGadgets, #backOff
        //
        // Each press toggles a button in or out of the set. The panel opens
        // only when all four of the first list are down and neither of the
        // second is: pressing a wrong button does not reset anything, it just
        // keeps the check from passing until it is pressed again.
        "panelbutton" => {
            const REQUIRED: [&str; 4] = ["A1", "A2", "B2", "B3"];
            const FORBIDDEN: [&str; 2] = ["A3", "B1"];

            let Some(button) = args
                .first()
                .and_then(Value::as_str)
                .map(|b| b.trim_start_matches('#').to_string())
            else {
                return true;
            };
            let down = |st: &State, b: &str| match st.get("panelGuess") {
                Value::List(items) => items
                    .iter()
                    .any(|i| i.as_str().is_some_and(|s| s.eq_ignore_ascii_case(b))),
                _ => false,
            };

            if down(state, &button) {
                state.trim_item("panelGuess", &Value::Symbol(button.clone()));
                out.effects.push(Effect::PlaySound {
                    name: "CPbuttonUp".into(),
                    loudness: None,
                });
            } else {
                state.add_item("panelGuess", Value::Symbol(button.clone()));
                out.effects.push(Effect::PlaySound {
                    name: "CPbuttonDn".into(),
                    loudness: None,
                });
            }
            out.redraw = true;

            let solved = REQUIRED.iter().all(|b| down(state, b))
                && !FORBIDDEN.iter().any(|b| down(state, b));
            if solved {
                state.set("controlPanel", Value::Symbol("closed".into()));
                out.destination = Some("basement_doorGadgets".into());
                out.transition = Some("backOff".into());
            }
        }

        _ => return false,
    }
    true
}

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
            let down = |st: &State, b: &str| {
                st.get_all("panelGuess")
                    .iter()
                    .any(|i| i.as_str().is_some_and(|s| s.eq_ignore_ascii_case(b)))
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

        // on adjustLockSettings whichDigit, upOrDown
        //   cursorOff
        //   digitStack = getProp( oPuppeteer.frames, #lock_<X>_digits )
        //   ... find the sprite showing one of those casts ...
        //   startTimer
        //   repeat while stillDown() and the ticks exceed lagTime
        //     wait #soundStop, #tumbler : soundEffect #tumbler
        //     set the castNum of sprite digitSprite to getProp(digitStack, #spin)
        //     updateStage : wait 4 ticks
        //     currentSetting = getState( #lock_<X> )
        //     if upOrDown = #up   then newSetting = (currentSetting + 11) mod 10
        //     if upOrDown = #down then newSetting = (currentSetting + 9)  mod 10
        //     setProp( oStoryteller.states, #lock_<X>, list(newSetting) )
        //     wait #soundStop, #tumbler
        //     set the castNum of sprite digitSprite to getProp(digitStack, newSetting)
        //     updateStage : lagTime = lagTime + 40
        //   if getState(#lock_A) <> 3 then return
        //   if getState(#lock_B) <> 2 then return
        //   if getState(#lock_C) <> 1 then return
        //   soundEffect #grateUnlock
        //
        // The wheels run 0-9 and wrap, which the `+11` and `+9` before the
        // `mod 10` say without ever naming a range: the schema declares each
        // wheel with a single value, so the range is not stated anywhere else.
        //
        // The sprite is not driven here. Each wheel's `#castNum` is
        // `[#lock_A, #lock_A_digits]`, so writing the flag is what changes the
        // art; the original only touches the sprite directly to show the
        // motion-blur frame between two settings.
        "adjustlocksettings" => {
            let Some(wheel) = args.first().and_then(Value::as_str).and_then(|d| {
                match d.trim_start_matches('#').to_ascii_lowercase().as_str() {
                    "a_digit" => Some("lock_A"),
                    "b_digit" => Some("lock_B"),
                    "c_digit" => Some("lock_C"),
                    _ => None,
                }
            }) else {
                return true;
            };
            let up = args
                .get(1)
                .and_then(Value::as_str)
                .is_some_and(|d| d.trim_start_matches('#').eq_ignore_ascii_case("up"));

            let current = state.get(wheel).as_int().unwrap_or(0);
            let stepped = (current + if up { 11 } else { 9 }).rem_euclid(10);
            // `setProp( oStoryteller.states, #lock_A, list(newSetting) )`: the
            // original replaces the wheel's whole value list rather than going
            // through `setState`, so the wheel holds exactly one digit.
            state.set_all(wheel, vec![Value::Int(stepped)]);

            out.effects.push(Effect::PlaySound {
                name: "tumbler".into(),
                loudness: None,
            });
            out.redraw = true;
            // A click turns one notch; holding the button spins the wheel.
            out.repeat_while_held = true;

            // The combination, checked here as well as in `tryToOpenGrate`, so
            // the lock answers as soon as the last wheel lands on it.
            if [("lock_A", 3), ("lock_B", 2), ("lock_C", 1)]
                .iter()
                .all(|(k, want)| state.get(k).as_int() == Some(*want))
            {
                out.effects.push(Effect::PlaySound {
                    name: "grateUnlock".into(),
                    loudness: None,
                });
            }
        }

        // on tryToOpenGrate
        //   currentCombination = list( getState(#lock_A), getState(#lock_B), getState(#lock_C) )
        //   if currentCombination = list(3, 2, 1) then
        //     if getState(#currentLocation) <> #gaz_trapdoorCU then
        //       goTo( #gaz_trapdoorCU, #backOff )
        //     setState( #grateIsOpen, 1 )
        //   else
        //     failureSounds = [#dammit, #dammit, #dammit]
        //     soundEffect getAt( failureSounds, random(count(failureSounds)) )
        //
        // The combination is 3-2-1. The failure list holds the same symbol
        // three times, so the roll picks between three identical sounds: the
        // authors left room for variants they never recorded, and reproducing
        // the roll costs nothing.
        "trytoopengrate" => {
            let solved = [("lock_A", 3), ("lock_B", 2), ("lock_C", 1)]
                .iter()
                .all(|(k, want)| state.get(k).as_int() == Some(*want));

            if solved {
                state.set("grateIsOpen", Value::Int(1));
                out.destination = Some("gaz_trapdoorCU".into());
                out.transition = Some("backOff".into());
            } else {
                let _ = roll(state, 3);
                out.effects.push(Effect::PlaySound {
                    name: "dammit".into(),
                    loudness: None,
                });
            }
        }

        _ => return false,
    }
    true
}

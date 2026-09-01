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
        // on pushNail targetNail
        //   shadowNail = the next nail round: nail_1 -> nail_2 -> nail_3 -> nail_1
        //   targetCurrentState = getState( targetNail )
        //   targetMovement = #inward : nailSound = #nailHeadIn
        //   if targetCurrentState = #out     then setState( targetNail, #halfway )
        //   if targetCurrentState = #halfway then setState( targetNail, #in )
        //   if targetCurrentState = #in      then
        //     targetMovement = #outward : nailSound = #nailHeadOut
        //     setState( targetNail, #out )
        //   soundEffect nailSound : updateDisplay : wait 15
        //   shadowCurrentState = getState( shadowNail )
        //   if targetMovement = #inward then
        //     if shadowCurrentState = #out     then setState( shadowNail, #in )
        //     if shadowCurrentState = #halfway then setState( shadowNail, #out )
        //     if shadowCurrentState = #in      then setState( shadowNail, #halfway )
        //   else
        //     if shadowCurrentState = #out     then setState( shadowNail, #halfway )
        //     if shadowCurrentState = #halfway then setState( shadowNail, #in )
        //     if shadowCurrentState = #in      then setState( shadowNail, #out )
        //   if getState(#nail_1) = #out and getState(#nail_2) = #out
        //                              and getState(#nail_3) = #out then
        //     ... showMontage 1, 2, 3 with a film each ...
        //     setState( #heartBox, #open ) : setState( #showMontage, 4 )
        //
        // Three nails, each `#out`, `#halfway` or `#in`, and pushing one drags
        // the next one round with it: pressing a nail deeper pulls its
        // neighbour back a step, and letting one pop out pushes its neighbour
        // in a step. All three out opens the heart box.
        //
        // The chain of `if`s reads the *saved* state rather than re-reading
        // the flag, which is why setting `#halfway` in the first test does not
        // then fall into the second and skip straight to `#in`. Writing them
        // as a match on the saved value keeps that property instead of relying
        // on it.
        "pushnail" => {
            const NAILS: [&str; 3] = ["nail_1", "nail_2", "nail_3"];
            // The nail's three depths, in the order pushing takes them.
            const DEPTHS: [&str; 3] = ["out", "halfway", "in"];

            let Some(target) = args
                .first()
                .and_then(Value::as_str)
                .map(|n| n.trim_start_matches('#').to_string())
                .filter(|n| NAILS.contains(&n.as_str()))
            else {
                return true;
            };
            let shadow = NAILS[(NAILS.iter().position(|n| *n == target).unwrap() + 1) % 3];

            let depth = |state: &State, nail: &str| {
                DEPTHS
                    .iter()
                    .position(|d| state.get(nail).as_symbol() == Some(d))
                    .unwrap_or(0)
            };

            let was = depth(state, &target);
            // Out and halfway go deeper; in pops all the way back out.
            let inward = was < 2;
            let moved = if inward { was + 1 } else { 0 };
            state.set(&target, Value::Symbol(DEPTHS[moved].into()));
            out.effects.push(Effect::PlaySound {
                name: if inward { "nailHeadIn" } else { "nailHeadOut" }.into(),
                loudness: None,
            });
            out.effects.push(Effect::WaitTicks(15));

            // The neighbour goes the other way, one step round the same ring.
            let shadow_was = depth(state, shadow);
            let shadow_now = if inward {
                (shadow_was + 2) % 3
            } else {
                (shadow_was + 1) % 3
            };
            state.set(shadow, Value::Symbol(DEPTHS[shadow_now].into()));
            out.effects.push(Effect::PlaySound {
                name: if inward { "nailHeadIn" } else { "nailHeadOut" }.into(),
                loudness: None,
            });
            out.redraw = true;

            if NAILS.iter().any(|n| state.get(n).as_symbol() != Some("out")) {
                return true;
            }
            // All three out. Three films in a row, then the box.
            //
            // The win sounds are behind `if gCPU = #PC`, and this port takes
            // the Mac arm throughout: on the Mac the films carry their own
            // audio, which is why the PC build has to play it separately. This
            // engine decodes film soundtracks, so playing them again here
            // would double them.
            out.effects.push(Effect::CursorOff);
            for step in 1..=3 {
                out.effects.push(Effect::FadeToMontage(step));
                out.effects.push(Effect::PlayVideo(None));
                out.effects.push(Effect::WaitForVideo);
                out.effects.push(Effect::StopVideo);
            }
            out.effects.push(Effect::SetState {
                key: "heartBox".into(),
                value: Value::Symbol("open".into()),
            });
            out.effects.push(Effect::FadeToMontage(4));
            out.effects.push(Effect::PlaySound {
                name: "heartOpen".into(),
                loudness: None,
            });
            out.effects.push(Effect::SetTransition {
                kind: "fadeIn".into(),
            });
        }

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
                // The original only moves the player when they are not already
                // at the trapdoor: the lock can be worked from a close-up of
                // it, and re-entering the room would restart its scene.
                let here = state.get("currentLocation");
                let at_door = here
                    .as_str()
                    .is_some_and(|r| r.eq_ignore_ascii_case("gaz_trapdoorCU"));
                if !at_door {
                    out.destination = Some("gaz_trapdoorCU".into());
                    out.transition = Some("backOff".into());
                }
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

#[cfg(test)]
mod tests {
    use super::*;

    // -- the three nails ----------------------------------------------------

    fn nails() -> State {
        let mut s = State::new();
        s.set_all("gChapter", vec![Value::Symbol("BRICE".into())]);
        // The schema starts all three halfway in.
        for nail in ["nail_1", "nail_2", "nail_3"] {
            s.set_all(nail, vec![Value::Symbol("halfway".into())]);
        }
        s
    }

    fn push(state: &mut State, nail: u8) -> Outcome {
        let mut out = Outcome::default();
        assert!(call(
            "pushnail",
            &[Value::Symbol(format!("nail_{nail}"))],
            state,
            &mut out
        ));
        out
    }

    fn depths(state: &State) -> Vec<String> {
        ["nail_1", "nail_2", "nail_3"]
            .iter()
            .map(|n| state.get(n).as_symbol().unwrap_or("").to_string())
            .collect()
    }

    #[test]
    fn a_nail_goes_out_halfway_in_and_back_out() {
        let mut s = nails();
        s.set("nail_1", Value::Symbol("out".into()));
        for want in ["halfway", "in", "out"] {
            push(&mut s, 1);
            assert_eq!(depths(&s)[0], want);
        }
    }

    #[test]
    fn pushing_one_in_drags_its_neighbour_back() {
        let mut s = nails();
        push(&mut s, 1);
        // nail_1 halfway -> in, so nail_2 halfway -> out.
        assert_eq!(depths(&s), ["in", "out", "halfway"]);
    }

    #[test]
    fn and_letting_one_out_pushes_its_neighbour_in() {
        let mut s = nails();
        s.set("nail_1", Value::Symbol("in".into()));
        push(&mut s, 1);
        // nail_1 pops out, so nail_2 halfway -> in.
        assert_eq!(depths(&s), ["out", "in", "halfway"]);
    }

    #[test]
    fn each_push_moves_the_nail_exactly_one_step() {
        // The original saves the state into a local before its chain of ifs.
        // Re-reading the flag would carry #out straight through to #in.
        let mut s = nails();
        s.set("nail_1", Value::Symbol("out".into()));
        push(&mut s, 1);
        assert_eq!(depths(&s)[0], "halfway");
    }

    #[test]
    fn the_puzzle_can_be_solved_from_where_it_starts() {
        // Breadth-first over all 27 positions says five pushes is the
        // shortest way out, and that every position is reachable from every
        // other -- so the puzzle cannot be locked up.
        let mut s = nails();
        for nail in [1, 1, 3, 1, 2] {
            push(&mut s, nail);
        }
        assert_eq!(depths(&s), ["out", "out", "out"]);
    }

    #[test]
    fn and_that_opens_the_heart_box() {
        let mut s = nails();
        for nail in [1, 1, 3, 1] {
            let out = push(&mut s, nail);
            assert!(
                !out.effects.iter().any(|e| matches!(e, Effect::SetState { key, .. } if key == "heartBox")),
                "the box opened early"
            );
        }
        let out = push(&mut s, 2);
        let opened = out.effects.iter().any(|e| {
            matches!(e, Effect::SetState { key, value }
                if key == "heartBox" && value.as_symbol() == Some("open"))
        });
        assert!(opened);
        // Four montage steps, three of them with a film.
        let montage: Vec<i32> = out
            .effects
            .iter()
            .filter_map(|e| match e {
                Effect::FadeToMontage(n) => Some(*n),
                _ => None,
            })
            .collect();
        assert_eq!(montage, [1, 2, 3, 4]);
    }
}

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
        // on goodbyeMandy
        //   cursorOff : setState( #showMontage, 0 )
        //   soundEffect #solidDoorOpen
        //   goTo( #basement_closetOpen, #lookAt )
        //   endLoop( #Basement, #fadeOut )
        //   soundEffect #drips : wait 60
        //   setState( #showMontage, 1 ) : setTransition #slowMontage : updateDisplay : wait 60
        //   setState( #showMontage, 2 ) : setTransition #slowMontage : updateDisplay : wait 120
        //   assertSound #atMandy : wait #soundStop, #atMandy
        //   suspendSounds : pushVideo : wait #videoStop : killVideo : wait 15
        //   setState( #showMontage, 3 ) : ... #lightsOut ... : updateDisplay : wait 60
        //   setState( #showMontage, 4 ) : updateDisplay : pushVideo : wait #videoStop
        //   setState( #showMontage, 5 ) : setTransition #fadeIn : updateDisplay : killVideo
        //   soundEffect #toRoxy
        //   enterNewDomain( oStoryteller, string(#Roxy), ... )
        //
        // The end of Brice's chapter. Six montage steps, two films and one
        // remark, and then the game moves to Roxy.
        //
        // `#slowMontage` rather than `#fadeIn` for the middle of it, which is
        // the one transition in the game with its own speed -- a third of the
        // rate, from entry 79. This is what it is for.
        "goodbyemandy" => {
            out.effects.push(Effect::CursorOff);
            out.effects.push(Effect::SetState {
                key: "showMontage".into(),
                value: Value::Int(0),
            });
            out.effects.push(Effect::PlaySound {
                name: "solidDoorOpen".into(),
                loudness: None,
            });
            out.effects.push(Effect::GoToRoom {
                room: "basement_closetOpen".into(),
                transition: Some("lookAt".into()),
            });
            out.effects.push(Effect::StopLoop {
                name: "Basement".into(),
                fade: true,
            });
            out.effects.push(Effect::PlaySound {
                name: "drips".into(),
                loudness: None,
            });
            out.effects.push(Effect::WaitTicks(60));

            // The slow half.
            for (step, hold) in [(1, 60), (2, 120)] {
                out.effects.push(Effect::SetTransition {
                    kind: "slowMontage".into(),
                });
                out.effects.push(Effect::SetState {
                    key: "showMontage".into(),
                    value: Value::Int(step),
                });
                out.effects.push(Effect::WaitTicks(hold));
            }

            super::assert_sound("atMandy", None, state, out);
            out.effects.push(Effect::WaitForSound("atMandy".into()));
            out.effects.push(Effect::SuspendSounds { fade: false });
            out.effects.push(Effect::PlayVideo(None));
            out.effects.push(Effect::WaitForVideo);
            out.effects.push(Effect::StopVideo);
            out.effects.push(Effect::WaitTicks(15));

            // `setState( #showMontage, 3 )` and then
            // `set the queuedSound of oPuppeteer = #lightsOut`. There is no
            // plate declared for step 3 and no film either: the screen goes
            // black, and the sound of the lights going out is the whole beat.
            // Without it the black is a gap rather than a moment, which is
            // what helba could feel was missing.
            out.effects.push(Effect::SetState {
                key: "showMontage".into(),
                value: Value::Int(3),
            });
            out.effects.push(Effect::PlaySound {
                name: "lightsOut".into(),
                loudness: None,
            });
            out.effects.push(Effect::WaitTicks(60));
            // Step 4 is `Bexit.mov`, and neither 3 nor 4 arms a transition --
            // only 5 does, on the way to the re-entry picture.
            out.effects.push(Effect::SetState {
                key: "showMontage".into(),
                value: Value::Int(4),
            });
            out.effects.push(Effect::PlayVideo(None));
            out.effects.push(Effect::WaitForVideo);
            out.effects.push(Effect::FadeToMontage(5));
            out.effects.push(Effect::StopVideo);
            out.effects.push(Effect::PlaySound {
                name: "toRoxy".into(),
                loudness: None,
            });
            // And into Roxy's chapter.
            out.new_domain = Some("ROXY".into());
        }

        // on keyholeComments
        //   if inState( #utterancesRemaining, #someTrouble )
        //     then assertSound #someTrouble
        //     else assertSound #concernedCitizen
        //
        // Looking through the keyhole. The first time he suspects trouble;
        // after that he is only a concerned citizen. The fallback is itself an
        // `assertSound`, so it too is said once and the third look is silent.
        "keyholecomments" => {
            let first = state
                .get_all("utterancesRemaining")
                .iter()
                .any(|v| v.is_symbol("someTrouble"));
            let line = if first { "someTrouble" } else { "concernedCitizen" };
            super::assert_sound(line, None, state, out);
        }

        // on windowHints
        //   if inState( #utterancesRemaining, #herWindow ) then
        //     assertSound #herWindow
        //     wait #soundStop, #herWindow
        //     wait 60
        //     assertSound #tellMeSomething
        //   else
        //     assertSound #nicePattern
        //
        // Two remarks in a row the first time -- he notices the window is
        // hers, pauses a second, and then asks it to tell him something --
        // and one about the pattern thereafter.
        "windowhints" => {
            let first = state
                .get_all("utterancesRemaining")
                .iter()
                .any(|v| v.is_symbol("herWindow"));
            if first {
                super::assert_sound("herWindow", None, state, out);
                out.effects.push(Effect::WaitForSound("herWindow".into()));
                out.effects.push(Effect::WaitTicks(60));
                super::assert_sound("tellMeSomething", None, state, out);
            } else {
                super::assert_sound("nicePattern", None, state, out);
            }
        }

        // on setConservatoryDoorIsOpen suggestion
        //   currentState = getState( #conservatoryDoorIsOpen )
        //   cursorOff : currentLoc = getState( #currentLocation )
        //   if suggestion = 0 and currentState = 1 then
        //     soundEffect #solidDoorClose
        //     setProp( states, #conservatoryDoorIsOpen, list(0) )
        //     ... if currentLoc = #Cons_p1_s, a horsepower-gated film ...
        //     endLoop #win_hangingLoop
        //     updateDisplay( oPuppeteer )
        //     if currentLoc = #Cons_CenterS or currentLoc = #Cons_Exit then
        //       endLoop #outsideLoop
        //   if suggestion = 1 and currentState = 0 then
        //     soundEffect #solidDoorOpen
        //     setProp( states, #conservatoryDoorIsOpen, list(1) )
        //     ... the same film ...
        //     updateDisplay( oPuppeteer )
        //     if currentLoc = #Cons_Exit then setLoop( #outsideLoop, 120 )
        //
        // Another bleeding door, and not a symmetrical one: closing it stops
        // the outside in **two** rooms and opening it starts the outside in
        // **one**. Standing at `#Cons_CenterS` you can hear the outside die
        // when the door shuts and not hear it return when it opens -- which
        // looks like a bug in the original and is faithfully reproduced,
        // because there is no reading of those two branches that makes them
        // agree.
        //
        // Not modelled: on a fast machine the open and close each hold until
        // the door's film passes movieTime 220, behind `if gHorsepower =
        // #high`. This engine has no equivalent of waiting on part of an
        // already-running film, and the wait is a pause rather than a
        // behaviour -- what it gates is `killVideo`, which happens either way.
        "setconservatorydoorisopen" => {
            let asked = args.first().and_then(Value::as_int).unwrap_or(0);
            let held = state.get("conservatoryDoorIsOpen").as_int().unwrap_or(0);
            let opening = match (asked, held) {
                (1, 0) => true,
                (0, 1) => false,
                _ => return true,
            };
            let at = state.get("currentLocation");

            out.effects.push(Effect::CursorOff);
            out.effects.push(Effect::PlaySound {
                name: if opening { "solidDoorOpen" } else { "solidDoorClose" }.into(),
                loudness: None,
            });
            state.set_all("conservatoryDoorIsOpen", vec![Value::Int(asked)]);

            if opening {
                if at.is_symbol("Cons_Exit") {
                    out.effects.push(Effect::StartLoop {
                        name: "outsideLoop".into(),
                        volume: Some(120),
                    });
                }
            } else {
                out.effects.push(Effect::StopLoop {
                    name: "win_hangingLoop".into(),
                    fade: false,
                });
                if at.is_symbol("Cons_CenterS") || at.is_symbol("Cons_Exit") {
                    out.effects.push(Effect::StopLoop {
                        name: "outsideLoop".into(),
                        fade: false,
                    });
                }
            }
            out.redraw = true;
        }

        // on setShedDoorIsOpen suggestion
        //   currentState = getState( #shedDoorIsOpen )
        //   if suggestion = 0 and currentState = 1 then
        //     soundEffect #shedDoorClose
        //     setProp( oStoryteller.states, #shedDoorIsOpen, list(0) )
        //     updateDisplay( oPuppeteer )
        //     if getState( #currentLocation ) = #Shed_Door_NW then endLoop #outsideLoop
        //   if suggestion = 1 and currentState = 0 then
        //     soundEffect #shedDoorOpen
        //     setProp( oStoryteller.states, #shedDoorIsOpen, list(1) )
        //     updateDisplay( oPuppeteer )
        //     if getState( #currentLocation ) = #Shed_Door_NW then setLoop( #outsideLoop, 90 )
        //
        // A bleeding door, the same shape as Roxy's front door in entry 63:
        // the shed's own doorway is the one place the outside is audible
        // through it, so the loop is started and stopped only while standing
        // there. Open it from anywhere else and the sound is somebody else's
        // problem -- the room you walk into declares its own mix.
        //
        // Guarded on the flag changing, so a door already open neither sounds
        // nor restarts the loop.
        "setsheddoorisopen" => {
            let asked = args.first().and_then(Value::as_int).unwrap_or(0);
            let held = state.get("shedDoorIsOpen").as_int().unwrap_or(0);
            let opening = match (asked, held) {
                (1, 0) => true,
                (0, 1) => false,
                _ => return true,
            };

            out.effects.push(Effect::PlaySound {
                name: if opening { "shedDoorOpen" } else { "shedDoorClose" }.into(),
                loudness: None,
            });
            state.set_all("shedDoorIsOpen", vec![Value::Int(asked)]);
            if state.get("currentLocation").is_symbol("Shed_Door_NW") {
                out.effects.push(if opening {
                    Effect::StartLoop {
                        name: "outsideLoop".into(),
                        volume: Some(90),
                    }
                } else {
                    Effect::StopLoop {
                        name: "outsideLoop".into(),
                        fade: false,
                    }
                });
            }
            out.redraw = true;
        }

        // on resetHeartBox
        //   if getState( #heartBox ) = #open then return
        //   setState( #nail_1, #halfway )
        //   setState( #nail_2, #halfway )
        //   setState( #nail_3, #halfway )
        //
        // Walking away puts the nails back where they started, so the puzzle
        // has to be solved in one visit -- unless it already has been, in
        // which case the open box is left alone.
        "resetheartbox" => {
            if state.get("heartBox").is_symbol("open") {
                return true;
            }
            for nail in ["nail_1", "nail_2", "nail_3"] {
                state.set(nail, Value::Symbol("halfway".into()));
            }
            out.redraw = true;
        }

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

            if NAILS.iter().any(|n| !state.get(n).is_symbol("out")) {
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
        //   setState(oStoryteller, #closetDoorIsOpen, #ajar)
        //   set the queuedSound of oPuppeteer = #solidDoorOpen
        //   updateDisplay : killVideo
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
                // What the panel is for. The closet is what the whole chapter
                // is walking towards, and its door coming ajar is the last
                // line of this handler -- which I had left off, so the panel
                // opened, the player was carried back to the door, and the
                // door was still shut. `testClosetLock` needs `#ajar` and the
                // weathervane flying, and only this writes the first.
                state.set("closetDoorIsOpen", Value::Symbol("ajar".into()));
                out.effects.push(Effect::PlaySound {
                    name: "solidDoorOpen".into(),
                    loudness: None,
                });
                out.effects.push(Effect::StopVideo);
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

    #[test]
    fn walking_away_puts_the_nails_back() {
        let mut s = nails();
        push(&mut s, 1);
        let mut out = Outcome::default();
        assert!(call("resetheartbox", &[], &mut s, &mut out));
        assert_eq!(depths(&s), ["halfway", "halfway", "halfway"]);
    }

    #[test]
    fn but_leaves_a_box_that_is_already_open() {
        let mut s = nails();
        for nail in [1, 1, 3, 1, 2] {
            push(&mut s, nail);
        }
        s.set("heartBox", Value::Symbol("open".into()));
        let mut out = Outcome::default();
        assert!(call("resetheartbox", &[], &mut s, &mut out));
        assert_eq!(depths(&s), ["out", "out", "out"]);
    }

    #[test]
    fn the_shed_door_is_only_audible_from_its_own_doorway() {
        let loops = |at: &str, to: i32, from: i32| {
            let mut s = State::new();
            s.set_all("gChapter", vec![Value::Symbol("BRICE".into())]);
            s.set_all("shedDoorIsOpen", vec![Value::Int(from)]);
            s.set_all("currentLocation", vec![Value::Symbol(at.into())]);
            let mut out = Outcome::default();
            assert!(call("setsheddoorisopen", &[Value::Int(to)], &mut s, &mut out));
            out.effects
                .iter()
                .filter_map(|e| match e {
                    Effect::StartLoop { name, volume } => Some((name.clone(), *volume)),
                    Effect::StopLoop { name, .. } => Some((name.clone(), None)),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(
            loops("Shed_Door_NW", 1, 0),
            [("outsideLoop".to_string(), Some(90))]
        );
        assert_eq!(loops("Shed_Door_NW", 0, 1), [("outsideLoop".to_string(), None)]);
        // Opened from elsewhere it makes its noise and leaves the mix alone.
        assert!(loops("Shed_Interior", 1, 0).is_empty());
    }

    #[test]
    fn and_a_door_already_open_does_nothing() {
        let mut s = State::new();
        s.set_all("gChapter", vec![Value::Symbol("BRICE".into())]);
        s.set_all("shedDoorIsOpen", vec![Value::Int(1)]);
        s.set_all("currentLocation", vec![Value::Symbol("Shed_Door_NW".into())]);
        let mut out = Outcome::default();
        assert!(call("setsheddoorisopen", &[Value::Int(1)], &mut s, &mut out));
        assert!(out.effects.is_empty());
    }

    #[test]
    fn the_conservatory_door_is_not_symmetrical() {
        let loops = |at: &str, to: i32, from: i32| {
            let mut s = State::new();
            s.set_all("gChapter", vec![Value::Symbol("BRICE".into())]);
            s.set_all("conservatoryDoorIsOpen", vec![Value::Int(from)]);
            s.set_all("currentLocation", vec![Value::Symbol(at.into())]);
            let mut out = Outcome::default();
            assert!(call(
                "setconservatorydoorisopen",
                &[Value::Int(to)],
                &mut s,
                &mut out
            ));
            out.effects
                .iter()
                .filter_map(|e| match e {
                    Effect::StartLoop { name, .. } => Some(format!("start {name}")),
                    Effect::StopLoop { name, .. } => Some(format!("stop {name}")),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };

        // At the exit the outside comes and goes with the door.
        assert!(loops("Cons_Exit", 1, 0).contains(&"start outsideLoop".to_string()));
        assert!(loops("Cons_Exit", 0, 1).contains(&"stop outsideLoop".to_string()));

        // At the centre it only ever goes. Closing the door kills the outside
        // and opening it does not bring it back -- the two branches of the
        // original disagree and there is no reading that makes them agree, so
        // this is faithful rather than fixed.
        assert!(loops("Cons_CenterS", 0, 1).contains(&"stop outsideLoop".to_string()));
        assert!(!loops("Cons_CenterS", 1, 0).contains(&"start outsideLoop".to_string()));
    }

    // -- what he says at the window and the keyhole -------------------------

    fn brice_with(lines: &[&str]) -> State {
        let mut s = State::new();
        s.set_all("gChapter", vec![Value::Symbol("BRICE".into())]);
        s.set_all(
            "utterancesRemaining",
            lines.iter().map(|l| Value::Symbol((*l).into())).collect(),
        );
        s
    }

    fn said(state: &mut State, verb: &str) -> Vec<String> {
        let mut out = Outcome::default();
        assert!(call(verb, &[], state, &mut out));
        out.effects
            .iter()
            .filter_map(|e| match e {
                Effect::PlaySound { name, .. } => Some(name.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_keyhole_has_a_first_look_and_a_second() {
        let mut s = brice_with(&["someTrouble", "concernedCitizen"]);
        assert_eq!(said(&mut s, "keyholecomments"), ["someTrouble"]);
        assert_eq!(said(&mut s, "keyholecomments"), ["concernedCitizen"]);
        // And a third, which is silence: the fallback is an utterance too.
        assert!(said(&mut s, "keyholecomments").is_empty());
    }

    #[test]
    fn the_window_gets_two_remarks_the_first_time() {
        let mut s = brice_with(&["herWindow", "tellMeSomething", "nicePattern"]);
        assert_eq!(
            said(&mut s, "windowhints"),
            ["herWindow", "tellMeSomething"]
        );
        assert_eq!(said(&mut s, "windowhints"), ["nicePattern"]);
    }
}

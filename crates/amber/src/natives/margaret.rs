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
pub fn call(name: &str, args: &[Value], state: &mut State, out: &mut Outcome) -> bool {
    match name {
        // on setDoorIsOpen suggestion
        //   valid = [#None, #DRtoKitchen, #DRtoStudy, #KitchenToOutside,
        //            #KitchenToHall, #KitchenToDR, #Bedrm, #livingRm]
        //   if not getPos(valid, suggestion) then put "..." : return
        //   if suggestion <> #None and not inState(#tunedIn, #livingRm) then
        //     if suggestion = #DRtoKitchen      then goBack( #add_15min )
        //     if suggestion = #DRtoStudy        then goBack( #add_30min )
        //     if suggestion = #KitchenToOutside then goBack( #add_3hr )
        //     if suggestion = #KitchenToHall    then goBack( #add_15min )
        //     if suggestion = #KitchenToDR      then goBack( #add_15min )
        //     if suggestion = #Bedrm            then goBack( #reset_4pm )
        //   previousState = getState( #doorIsOpen )
        //   setProp( oStoryteller.states, #doorIsOpen, list(suggestion) )
        //   updateDisplay( oPuppeteer )
        //   if suggestion = #None and previousState <> #None then
        //     if getState(#clockPuzzleActivated) = 1 or inState(#tunedIn, #diningRm) then
        //       assertSound #Iwonder
        //     else
        //       if inState(#utterancesRemaining, #wasteOfTime) then wait 40
        //       assertSound #wasteOfTime
        //
        // Opening a door and then shutting it again is how Margaret talks to
        // herself. Closing one she has just opened draws a line: `#Iwonder`
        // once the clock puzzle is under way or the radio is tuned to the
        // dining room, and `#wasteOfTime` otherwise. The wait before the
        // second is the beat before she says it, and it is only taken when the
        // line has not been used up.
        //
        // The `goBack` arguments name times -- fifteen minutes for a door
        // between rooms, three hours for the one outside, and the bedroom
        // resetting to four o'clock. `goBack` in this chapter takes no
        // arguments at all, so the original discards them: they read as a
        // clock this door was once meant to move and no longer does. They are
        // carried through as the transition, which is what a second argument
        // to a move means everywhere else.
        "setdoorisopen" => {
            const DOORS: [(&str, &str); 6] = [
                ("DRtoKitchen", "add_15min"),
                ("DRtoStudy", "add_30min"),
                ("KitchenToOutside", "add_3hr"),
                ("KitchenToHall", "add_15min"),
                ("KitchenToDR", "add_15min"),
                ("Bedrm", "reset_4pm"),
            ];
            let Some(asked) = args
                .first()
                .and_then(Value::as_str)
                .map(|v| v.trim_start_matches('#').to_string())
            else {
                return true;
            };
            let known = ["None", "livingRm"]
                .iter()
                .map(|s| (*s, ""))
                .chain(DOORS.iter().map(|(d, t)| (*d, *t)))
                .find(|(d, _)| d.eq_ignore_ascii_case(&asked));
            let Some((door, flavour)) = known else {
                trace!(crate::trace::Topic::Script, "setDoorIsOpen: no door {asked}");
                return true;
            };

            let in_living_room = state
                .get_all("tunedIn")
                .iter()
                .any(|v| v.as_str().is_some_and(|s| s.eq_ignore_ascii_case("livingRm")));
            if !door.eq_ignore_ascii_case("None") && !in_living_room && !flavour.is_empty() {
                out.go_back = true;
                out.transition = Some(flavour.to_string());
            }

            let previous = state.get("doorIsOpen");
            let was_open = previous
                .as_str()
                .is_some_and(|p| !p.eq_ignore_ascii_case("None"));
            state.set_all("doorIsOpen", vec![Value::Symbol(door.to_string())]);
            out.redraw = true;

            if door.eq_ignore_ascii_case("None") && was_open {
                let puzzle_on = state.get("clockPuzzleActivated").as_int().unwrap_or(0) == 1;
                let dining = state
                    .get_all("tunedIn")
                    .iter()
                    .any(|v| v.as_str().is_some_and(|s| s.eq_ignore_ascii_case("diningRm")));
                if puzzle_on || dining {
                    out.effects.push(Effect::PlaySound {
                        name: "Iwonder".into(),
                        loudness: None,
                    });
                } else {
                    let unused = state
                        .get_all("utterancesRemaining")
                        .iter()
                        .any(|v| v.as_str().is_some_and(|s| s.eq_ignore_ascii_case("wasteOfTime")));
                    if unused {
                        out.effects.push(Effect::WaitTicks(40));
                    }
                    out.effects.push(Effect::PlaySound {
                        name: "wasteOfTime".into(),
                        loudness: None,
                    });
                }
            }
        }

        // on setOpenBox suggestion
        //   validSuggestions = [#None: 0, #all: #allboxes, #moot: 0,
        //                       #snd1: #snd1box, ... #snd5: #snd5box]
        //   whichBox = getaProp(validSuggestions, suggestion)
        //   if voidp(whichBox) then put "..." : return
        //   cursorOff
        //   setProp( oStoryteller.states, #openBox, list(suggestion) )
        //   if suggestion <> #all then updateDisplay
        //   if suggestion = #None or suggestion = #moot then return
        //   if suggestion = #all then
        //     killVideo : updateDisplay : startSound #allboxes : wait 30
        //     pushVideo : wait #videoStop : killVideo
        //     setState( oStoryteller, #openBox, #moot )
        //     setTransition #fadeIn : updateDisplay : assertSound #thatVoice
        //   boxTimes = [#snd1: [0, 32], #snd2: [36, 60], #snd3: [68, 92],
        //               #snd4: [100, 124], #snd5: [#flipper, #hGap]]
        //   newBoxTimes = getaProp(boxTimes, suggestion)
        //   if not voidp(newBoxTimes) then
        //     prerollQT( startTime, stopTime, 4 )
        //     startSound whichBox
        //     if gHorsepower <> #low then wait 30
        //     pushQTcarefully( startTime, stopTime, 4 )
        //     boxesSoFar = getProp( oStoryteller.states, #boxList )
        //     if count(boxesSoFar) > 4 then deleteAt(boxesSoFar, 1)
        //     append(boxesSoFar, suggestion)
        //     if boxesSoFar = [#snd1, #snd2, #snd3, #snd4, #snd5] then
        //       setState( oStoryteller, #openBox, #all )
        //
        // Five music boxes on a dresser, and one film holding all five
        // performances: each box plays its own stretch of it, named in ticks,
        // landing four ticks inside the keyframes at every thirty-two so the
        // seams do not show.
        //
        // The puzzle is the order. `#boxList` keeps the last five boxes opened
        // and nothing more -- the count is trimmed from the front before each
        // append -- so there is no wrong move to undo and no need to start
        // again. Play them in order and the fifth press completes the sequence
        // whatever came before it.
        "setopenbox" => {
            const ORDER: [&str; 5] = ["snd1", "snd2", "snd3", "snd4", "snd5"];
            // Where each box's performance sits in the film, in ticks. The
            // fifth is written as two symbols rather than numbers, so it is
            // left to the room's own film rather than played as a stretch.
            const TIMES: [(&str, u32, u32); 4] = [
                ("snd1", 0, 32),
                ("snd2", 36, 60),
                ("snd3", 68, 92),
                ("snd4", 100, 124),
            ];

            let Some(asked) = args
                .first()
                .and_then(Value::as_str)
                .map(|v| v.trim_start_matches('#').to_string())
            else {
                return true;
            };
            let known = ["None", "all", "moot"]
                .iter()
                .chain(ORDER.iter())
                .any(|v| v.eq_ignore_ascii_case(&asked));
            if !known {
                trace!(crate::trace::Topic::Script, "setOpenBox: no box {asked}");
                return true;
            }

            out.effects.push(Effect::CursorOff);
            state.set_all("openBox", vec![Value::Symbol(asked.clone())]);
            out.redraw = true;

            if asked.eq_ignore_ascii_case("None") || asked.eq_ignore_ascii_case("moot") {
                return true;
            }

            if asked.eq_ignore_ascii_case("all") {
                out.effects.push(Effect::StopVideo);
                out.effects.push(Effect::PlaySound {
                    name: "allboxes".into(),
                    loudness: None,
                });
                out.effects.push(Effect::WaitTicks(30));
                out.effects.push(Effect::PlayVideo(None));
                out.effects.push(Effect::WaitForVideo);
                out.effects.push(Effect::StopVideo);
                out.effects.push(Effect::SetState {
                    key: "openBox".into(),
                    value: Value::Symbol("moot".into()),
                });
                out.effects.push(Effect::PlaySound {
                    name: "thatVoice".into(),
                    loudness: None,
                });
                return true;
            }

            // Every box sounds; only four have a stretch of film. The fifth
            // is written as `[#flipper, #hGap]` -- two symbols where the other
            // four have numbers -- so it plays its sound and whatever the room
            // already has on its video channel.
            out.effects.push(Effect::PlaySound {
                name: format!("{asked}box"),
                loudness: None,
            });
            if let Some(&(_, from, to)) = TIMES.iter().find(|(b, _, _)| b.eq_ignore_ascii_case(&asked)) {
                out.effects.push(Effect::PlayVideoSegment { from, to });
                out.effects.push(Effect::WaitForVideo);
            }

            // The rolling window of the last five.
            let mut opened: Vec<Value> = state.get_all("boxList").to_vec();
            if opened.len() > 4 {
                opened.remove(0);
            }
            opened.push(Value::Symbol(asked.clone()));
            let in_order = opened.len() == ORDER.len()
                && opened.iter().zip(ORDER).all(|(v, want)| {
                    v.as_str().is_some_and(|s| s.eq_ignore_ascii_case(want))
                });
            state.set_all("boxList", opened);

            if in_order {
                call("setopenbox", &[Value::Symbol("all".into())], state, out);
            }
        }

        // on resetBoxPuzzle
        //   killVideo
        //   setProp( oStoryteller.states, #boxList, [] )
        //
        // Leaving the dresser forgets the order, so the sequence has to be
        // played in one visit.
        "resetboxpuzzle" => {
            out.effects.push(Effect::StopVideo);
            state.set_all("boxList", Vec::new());
        }

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

#[cfg(test)]
mod box_tests {
    use super::*;

    fn dresser() -> State {
        let mut state = State::new();
        state.set_all("gChapter", vec![Value::Symbol("MARGARET".into())]);
        state.set_all("boxList", Vec::new());
        state
    }

    fn open(state: &mut State, box_name: &str) -> Outcome {
        let mut out = Outcome::default();
        assert!(call(
            "setopenbox",
            &[Value::Symbol(box_name.into())],
            state,
            &mut out
        ));
        out
    }

    fn sounds(out: &Outcome) -> Vec<String> {
        out.effects
            .iter()
            .filter_map(|e| match e {
                Effect::PlaySound { name, .. } => Some(name.clone()),
                _ => None,
            })
            .collect()
    }

    fn opened(state: &State) -> Vec<String> {
        state
            .get_all("boxList")
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect()
    }

    #[test]
    fn in_order_the_boxes_all_open() {
        let mut s = dresser();
        for b in ["snd1", "snd2", "snd3", "snd4"] {
            let out = open(&mut s, b);
            assert!(!sounds(&out).contains(&"allboxes".to_string()), "too early at {b}");
        }
        let out = open(&mut s, "snd5");
        assert!(sounds(&out).contains(&"allboxes".to_string()));
        assert!(sounds(&out).contains(&"thatVoice".to_string()));
    }

    #[test]
    fn out_of_order_they_do_not() {
        let mut s = dresser();
        for b in ["snd1", "snd3", "snd2", "snd4", "snd5"] {
            let out = open(&mut s, b);
            assert!(!sounds(&out).contains(&"allboxes".to_string()));
        }
    }

    #[test]
    fn only_the_last_five_are_remembered_so_a_wrong_start_costs_nothing() {
        // The count is trimmed from the front before each append, so there is
        // no wrong move to undo: play them in order and the fifth press
        // completes the sequence whatever came before it.
        let mut s = dresser();
        for b in ["snd4", "snd4", "snd2"] {
            open(&mut s, b);
        }
        for b in ["snd1", "snd2", "snd3", "snd4"] {
            open(&mut s, b);
        }
        let out = open(&mut s, "snd5");
        assert_eq!(opened(&s).len(), 5);
        assert!(sounds(&out).contains(&"allboxes".to_string()));
    }

    #[test]
    fn every_box_sounds_even_the_one_with_no_stretch_of_film() {
        // The fifth is written as two symbols where the others have numbers,
        // so it has no segment -- but it still plays.
        for b in ["snd1", "snd5"] {
            let mut s = dresser();
            let out = open(&mut s, b);
            assert!(sounds(&out).contains(&format!("{b}box")), "{b}");
        }
    }

    #[test]
    fn only_four_boxes_play_a_stretch_of_the_film() {
        let segments = |b: &str| {
            let mut s = dresser();
            open(&mut s, b)
                .effects
                .iter()
                .filter(|e| matches!(e, Effect::PlayVideoSegment { .. }))
                .count()
        };
        for b in ["snd1", "snd2", "snd3", "snd4"] {
            assert_eq!(segments(b), 1, "{b}");
        }
        assert_eq!(segments("snd5"), 0);
    }

    #[test]
    fn leaving_the_dresser_forgets_the_order() {
        let mut s = dresser();
        open(&mut s, "snd1");
        open(&mut s, "snd2");
        let mut out = Outcome::default();
        call("resetboxpuzzle", &[], &mut s, &mut out);
        assert!(opened(&s).is_empty());
    }

    #[test]
    fn a_box_that_is_not_one_of_the_five_is_ignored() {
        let mut s = dresser();
        open(&mut s, "snd9");
        assert!(opened(&s).is_empty());
    }
}

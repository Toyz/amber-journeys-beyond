//! Edwin's chapter: the frozen lake, the boat and Chippy.
//!
//! Each handler carries the disassembly it was read from, so the port can be
//! checked against the original without going back to the bytecode.

use lingo::Value;

use crate::script::{Effect, Outcome};
use crate::state::State;

use super::roll;

/// Runs a handler from this chapter, or reports that it is not one of ours.
/// Where the vane movie rests for each direction. The movie starts pointing
/// East, so East is frame zero and the rest follow a quarter-turn apart.
fn vane_rest(direction: &str) -> u32 {
    match direction {
        "E" => 0,
        "S" => 256,
        "W" => 384,
        _ => 128,
    }
}

/// The segment of the vane movie for one turn, named `#<from>to<to>` in the
/// original and built there by string concatenation.
///
/// Written out rather than derived. The obvious derivation -- clockwise turn
/// on the resting frame, counter-clockwise 64 ticks later -- fits `#n` and
/// `#W` and is wrong for `#E` and `#S`. What actually decides it is the
/// destination: a turn ending at `#n` or `#E` sits on the resting frame and
/// one ending at `#S` or `#W` sits 64 later. Eight entries is cheaper than a
/// rule that has to be believed.
fn vane_turn(from: &str, to: &str) -> Option<(u32, u32)> {
    let seg = match (from, to) {
        ("n", "E") => (128, 188),
        ("n", "W") => (192, 252),
        ("E", "n") => (0, 60),
        ("E", "S") => (64, 124),
        ("S", "E") => (256, 316),
        ("S", "W") => (320, 380),
        ("W", "n") => (384, 444),
        ("W", "S") => (448, 512),
        _ => return None,
    };
    Some(seg)
}

pub fn call(name: &str, args: &[Value], state: &mut State, out: &mut Outcome) -> bool {
    // Arguments and effects are unused by some chapters until more handlers
    // land here; the signature is uniform so the dispatcher stays simple.
    let _ = (args, &out, &state);
    match name {
        // on enableGust
        //   setState(oStoryteller, #gustEnabled, 1)
        // on setWeatherVane whichWay
        //   currentDirection = getState( #weatherVane )
        //   if whichWay = #clockwise then deltaList = [#n, #E, #S, #W, #n]
        //   else                          deltaList = [#n, #W, #S, #E, #n]
        //   newDirection = getAt( deltaList, getPos( deltaList, currentDirection ) + 1 )
        //   cursorOff
        //   squeakList  = getProp( oStoryteller.states, #vaneSqueaks )
        //   newSqueak   = getAt( squeakList, 1 )
        //   nextSqueak  = getLast( squeakList )
        //   setState( #vaneSqueaks, nextSqueak )
        //   setProp( oStoryteller.states, #weatherVane, list(newDirection) )
        //   if getState( #Wind ) <> #None then setState( #Wind, newDirection )
        //   vaneTurn = value( "#" & currentDirection & "to" & newDirection )
        //   turnTimes = [ #NtoE: [128, 188], #NtoW: [192, 252],
        //                 #EtoN: [0, 60],    #EtoS: [64, 124],
        //                 #StoE: [256, 316], #StoW: [320, 380],
        //                 #WtoN: [384, 444], #WtoS: [448, 512] ]
        //   startTime = getAt( newTurnTimes, 1 ) : stopTime = getAt( newTurnTimes, 2 )
        //   startSound newSqueak
        //   set the visible of sprite 44 = 0 : ... = 1
        //   pushQT( ..., 4 ) : updateDisplay
        //   if inState( #utterancesRemaining, #windControl ) then
        //     assertSound #windControl : wait #soundStop
        //
        // The vane's movie is 512 ticks laid out as eight 64-tick segments,
        // two for each direction it can be sitting in, grouped by that
        // direction in the order E, n, S, W. Which is why `initWeatherVane`
        // rests East at zero rather than North -- the movie simply starts
        // there -- and why every clockwise turn begins on a multiple of 128
        // and every counter-clockwise one 64 later.
        //
        // The wind only follows the vane once it is already blowing. Turning
        // the vane in still air moves the vane and nothing else.
        "setweathervane" => {
            const CLOCKWISE: [&str; 5] = ["n", "E", "S", "W", "n"];
            const COUNTER: [&str; 5] = ["n", "W", "S", "E", "n"];

            let clockwise = args
                .first()
                .and_then(Value::as_str)
                .is_some_and(|w| w.trim_start_matches('#').eq_ignore_ascii_case("clockwise"));
            let order = if clockwise { CLOCKWISE } else { COUNTER };

            let current = state
                .get("weatherVane")
                .as_symbol()
                .unwrap_or("n")
                .to_string();
            // getPos finds the first match, so #n is found at one and the
            // repeated #n at the end is only ever the wrap target.
            let at = order.iter().position(|d| *d == current).unwrap_or(0);
            let new = order[(at + 1).min(order.len() - 1)];

            out.effects.push(Effect::CursorOff);
            // The squeaks rotate: the first is used and sent to the back.
            let mut squeaks = state.get_all("vaneSqueaks").to_vec();
            if !squeaks.is_empty() {
                let squeak = squeaks.remove(0);
                if let Some(name) = squeak.as_str() {
                    out.effects.push(Effect::PlaySound {
                        name: name.trim_start_matches('#').into(),
                        loudness: None,
                    });
                }
                squeaks.push(squeak);
                state.set_all("vaneSqueaks", squeaks);
            }

            state.set_all("weatherVane", vec![Value::Symbol(new.into())]);
            if state.get("Wind").as_symbol() != Some("None") {
                state.set("Wind", Value::Symbol(new.into()));
            }

            if let Some((from, to)) = vane_turn(&current, new) {
                out.effects.push(Effect::PlayVideoSegment { from, to });
            }
            out.redraw = true;
        }

        // on initWeatherVane
        //   prerollQT( 0, 512, 4 )
        //   currentDirection = getState( #weatherVane )
        //   startTimes = [#n: 128, #E: 0, #S: 256, #W: 384]
        //   set the visible of sprite 44 = 0
        //   set the loc of sprite 44 = point(319, 234) ...
        //
        // Parks the vane movie on the frame for whichever way it is already
        // pointing, so entering the room does not snap it back to East.
        "initweathervane" => {
            let at = vane_rest(state.get("weatherVane").as_symbol().unwrap_or("n"));
            out.effects.push(Effect::PlayVideoSegment { from: at, to: at });
            out.redraw = true;
        }

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


        // on chippyCries howLoud
        //   if getState(oStoryteller, #chippyFreed) = 1 then exit
        //   volDesired = 90, or louder when howLoud is #loud
        //   highRoll = 2, or 6 when howLoud is #loud
        //   if random(6) <= highRoll then
        //     pleaList = getProp(oStoryteller, #distantPleas)
        //     newPlea = getAt(pleaList, 1)
        //     soundEffect(newPlea, volDesired)
        //     wait #soundStop, newPlea
        //     nextPlea = getLast(pleaList)
        //     setState(oStoryteller, #distantPleas, nextPlea)
        //
        // Chippy calls for help from somewhere out of sight until freed, and
        // the roll makes it occasional except when the script asks for #loud,
        // where 6 of 6 means it always sounds.
        //
        // The last two lines are ambiguous and this port does not follow them
        // literally. `getLast` is not defined as a handler in any of the five
        // movies, so it is Lingo's built-in, which returns the last element
        // rather than a list. Taken at face value the pool of eight pleas is
        // replaced by a single symbol after the first cry and nothing can be
        // indexed from it again, leaving Chippy silent for the rest of the
        // chapter. The pool is rotated instead, which is what a pool of eight
        // consumed one at a time is evidently for. Flagged because it is a
        // judgement, not a reading.
        "chippycries" => {
            if state.get("chippyFreed").as_int() == Some(1) {
                return true;
            }
            let loud = args
                .first()
                .and_then(Value::as_str)
                .is_some_and(|s| s.trim_start_matches('#').eq_ignore_ascii_case("loud"));
            let (volume, threshold) = if loud { (255, 6) } else { (90, 2) };
            if roll(state, 6) > threshold {
                return true;
            }

            let pool = match state.get_all("distantPleas") {
                items if !items.is_empty() => items.to_vec(),
                // Before the pool is seeded, the chapter's own list applies.
                _ => (1..=8)
                    .map(|n| Value::Symbol(format!("help{n}")))
                    .collect(),
            };
            let Some(plea) = pool.first().and_then(Value::as_str).map(str::to_owned) else {
                return true;
            };

            out.effects.push(Effect::PlaySound {
                name: plea.clone(),
                loudness: Some(if loud { "high".into() } else { "low".into() }),
            });
            out.effects.push(Effect::WaitForSound(plea));
            let _ = volume;

            let mut rotated = pool;
            rotated.rotate_left(1);
            state.set("distantPleas", Value::List(rotated));
        }


        // on snowBlind
        //   startSound #borderGust
        //   fadeToMontage 1 / 2 / 1 / 0
        //   nearTheHouse = (getState(#currentLocation) = #ice_border_N1)
        //   goBack
        //   if nearTheHouse then assertSound #cantSeeTheHouse
        //
        // Walking out onto the ice whites out, turns the player round, and
        // draws a remark when it happens within sight of the house. The four
        // montage steps are the white-out and its recovery.
        "snowblind" => {
            out.effects.push(Effect::PlaySound {
                name: "borderGust".into(),
                loudness: None,
            });
            for step in [1, 2, 1, 0] {
                out.effects.push(Effect::FadeToMontage(step));
            }
            let near = state
                .get("currentLocation")
                .as_str()
                .is_some_and(|l| l.eq_ignore_ascii_case("ice_border_N1"));
            out.go_back = true;
            if near {
                out.effects.push(Effect::PlaySound {
                    name: "cantSeeTheHouse".into(),
                    loudness: None,
                });
            }
        }


        // on iceAnchorComments
        //   if getState(#boatPosition) = #backward then
        //     if getState(#teddyLocation) = #onAnchor then #meTeddy
        //     else #iSeeAnchor
        //   assertSound thisComment : wait #soundStop
        //
        // With the boat facing the wrong way there is nothing to say, and the
        // handler falls through to sounding an unset comment, which is
        // silence.
        "iceanchorcomments" => {
            let facing_back = state
                .get("boatPosition")
                .as_str()
                .is_some_and(|p| p.eq_ignore_ascii_case("backward"));
            if !facing_back {
                return true;
            }
            let on_anchor = state
                .get("teddyLocation")
                .as_str()
                .is_some_and(|l| l.eq_ignore_ascii_case("onAnchor"));
            let line = if on_anchor { "meTeddy" } else { "iSeeAnchor" };
            out.effects.push(Effect::PlaySound {
                name: line.into(),
                loudness: None,
            });
            out.effects.push(Effect::WaitForSound(line.into()));
        }


        // on waterAnchorComments
        //   if getState(#teddyLocation) = #waiting then #iSeeAnchor
        //   else #meTeddy
        //   assertSound thisComment : wait #soundStop
        "wateranchorcomments" => {
            let waiting = state
                .get("teddyLocation")
                .as_str()
                .is_some_and(|l| l.eq_ignore_ascii_case("waiting"));
            let line = if waiting { "iSeeAnchor" } else { "meTeddy" };
            out.effects.push(Effect::PlaySound {
                name: line.into(),
                loudness: None,
            });
            out.effects.push(Effect::WaitForSound(line.into()));
        }

        // on listenToBees
        //   cursorOff
        //   if gCPU = #PC then suspendSounds #fadeOut
        //   else setLoop #outsideLoop, #howManybits
        //   pushVideo
        //   wait #videoStop
        //   if gCPU = #PC then restoreSounds #fadeIn
        //   else setLoop #outsideLoop, #startT
        //   assertSound #youBees
        //
        // Standing and listening to the hive. The two platforms handle the
        // ambience differently, the Windows build ducking it and the Mac one
        // swapping the outdoor loop; this port behaves as the Windows build,
        // whose branches the data fills in. The remark afterwards is common to
        // both.
        "listentobees" => {
            out.effects.push(Effect::CursorOff);
            out.effects.push(Effect::SuspendSounds { fade: true });
            out.effects.push(Effect::PlayVideo(None));
            out.effects.push(Effect::WaitForVideo);
            out.effects.push(Effect::RestoreSounds { fade: true });
            out.effects.push(Effect::PlaySound {
                name: "youBees".into(),
                loudness: None,
            });
        }

        // on leaveWhirligig
        //   set the visible of sprite 44 = 1
        //   puppetSprite 45, 0
        //   puppetSprite 44, 0
        //   enableGust
        //   enableSongs
        //
        // Stepping away from the whirligig: show the plate again, hand both
        // channels back to the score, and let the wind and the carols resume.
        "leavewhirligig" => {
            out.effects.push(Effect::SpriteVisible {
                channel: 44,
                visible: true,
            });
            for channel in [45, 44] {
                out.effects.push(Effect::PuppetSprite {
                    channel,
                    on: false,
                });
            }
            state.set("gustEnabled", Value::Int(1));
            state.set("carolsEnabled", Value::Int(1));
        }

        // on enterBubbleChamber
        //   cursorOff
        //   if gCPU = #PC then setLoop #underWater, <loud>
        //   fadeToMontage 1
        //   if gCPU = #PC then setLoop #underWater, 120
        //   fadeToMontage 2
        //   if gCPU = #PC then setLoop #underWater, 80
        //   setState(oStoryteller, #showMontage, ...)
        //   goTo #te_bubbleChamber, #forward
        //   if gCPU = #PC then endLoop #underWater, #fadeOut
        //   set a flag on sprite 44 from #horsePower
        //   hold until the mouse is pressed, then wait 120
        //
        // Going down into the bubble chamber. The descent is two montage steps
        // with the underwater loop brought up between them and faded out on
        // arrival, so the sound carries the movement rather than the picture.
        "enterbubblechamber" => {
            out.effects.push(Effect::CursorOff);
            // The loop is brought up, then eased back as the descent settles.
            for (step, volume) in [(1, 160), (2, 120)] {
                out.effects.push(Effect::StartLoop {
                    name: "underWater".into(),
                    volume: Some(volume),
                });
                out.effects.push(Effect::FadeToMontage(step));
            }
            out.effects.push(Effect::StartLoop {
                name: "underWater".into(),
                volume: Some(80),
            });
            out.destination = Some("te_bubbleChamber".into());
            out.transition = Some("forward".into());
            out.effects.push(Effect::StopLoop {
                name: "underWater".into(),
                fade: true,
            });
            out.effects.push(Effect::WaitTicks(120));
        }

        _ => return false,
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- the weather vane ---------------------------------------------------

    fn vane(facing: &str) -> State {
        let mut s = State::new();
        s.set_all("gChapter", vec![Value::Symbol("EDWIN".into())]);
        s.set_all("weatherVane", vec![Value::Symbol(facing.into())]);
        s.set_all(
            "vaneSqueaks",
            vec![
                Value::Symbol("squeak1".into()),
                Value::Symbol("squeak2".into()),
                Value::Symbol("squeak3".into()),
            ],
        );
        s
    }

    fn turn(state: &mut State, clockwise: bool) -> Outcome {
        let mut out = Outcome::default();
        let way = if clockwise { "clockwise" } else { "counter" };
        assert!(call(
            "setweathervane",
            &[Value::Symbol(way.into())],
            state,
            &mut out
        ));
        out
    }

    fn facing(state: &State) -> String {
        state.get("weatherVane").as_symbol().unwrap_or("").to_string()
    }

    fn segment(out: &Outcome) -> Option<(u32, u32)> {
        out.effects.iter().find_map(|e| match e {
            Effect::PlayVideoSegment { from, to } => Some((*from, *to)),
            _ => None,
        })
    }

    #[test]
    fn clockwise_goes_north_east_south_west() {
        let mut s = vane("n");
        for want in ["E", "S", "W", "n"] {
            turn(&mut s, true);
            assert_eq!(facing(&s), want);
        }
    }

    #[test]
    fn counter_clockwise_goes_the_other_way() {
        let mut s = vane("n");
        for want in ["W", "S", "E", "n"] {
            turn(&mut s, false);
            assert_eq!(facing(&s), want);
        }
    }

    #[test]
    fn every_turn_has_its_own_segment_of_the_movie() {
        // The eight of them, as the original names them.
        let want = [
            ("n", true, (128, 188)),
            ("n", false, (192, 252)),
            ("E", false, (0, 60)),
            ("E", true, (64, 124)),
            ("S", false, (256, 316)),
            ("S", true, (320, 380)),
            ("W", true, (384, 444)),
            ("W", false, (448, 512)),
        ];
        for (from, clockwise, seg) in want {
            let mut s = vane(from);
            let out = turn(&mut s, clockwise);
            assert_eq!(segment(&out), Some(seg), "turning from {from}");
        }
    }

    #[test]
    fn the_wind_follows_the_vane_once_it_is_blowing() {
        let mut s = vane("n");
        s.set("Wind", Value::Symbol("n".into()));
        turn(&mut s, true);
        assert_eq!(s.get("Wind"), Value::Symbol("E".into()));
    }

    #[test]
    fn but_turning_it_in_still_air_moves_only_the_vane() {
        let mut s = vane("n");
        s.set("Wind", Value::Symbol("None".into()));
        turn(&mut s, true);
        assert_eq!(facing(&s), "E");
        assert_eq!(s.get("Wind"), Value::Symbol("None".into()));
    }

    #[test]
    fn the_squeaks_take_it_in_turns() {
        let mut s = vane("n");
        let heard: Vec<String> = (0..4)
            .map(|_| {
                let out = turn(&mut s, true);
                out.effects
                    .iter()
                    .find_map(|e| match e {
                        Effect::PlaySound { name, .. } => Some(name.clone()),
                        _ => None,
                    })
                    .unwrap_or_default()
            })
            .collect();
        assert_eq!(heard, ["squeak1", "squeak2", "squeak3", "squeak1"]);
    }

    #[test]
    fn entering_the_room_parks_the_vane_where_it_already_points() {
        for (facing, at) in [("E", 0), ("n", 128), ("S", 256), ("W", 384)] {
            let mut s = vane(facing);
            let mut out = Outcome::default();
            assert!(call("initweathervane", &[], &mut s, &mut out));
            assert_eq!(segment(&out), Some((at, at)), "facing {facing}");
        }
    }
}

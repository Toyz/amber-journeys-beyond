//! Edwin's chapter: the frozen lake, the boat and Chippy.
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
        // on enableGust
        //   setState(oStoryteller, #gustEnabled, 1)
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

            let pool = match state.get("distantPleas") {
                Value::List(items) => items,
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

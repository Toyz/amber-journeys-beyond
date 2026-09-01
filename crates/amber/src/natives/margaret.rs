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

/// Where each station sits on the dial, from `checkRadioStations`. The dial
/// runs 0 to 240 in steps of four, so a station is four or eight either side
/// of its own position before the signal is lost.
const RADIO_STATIONS: [(&str, i32); 4] = [
    ("bedroom", 36),
    ("diningRm", 56),
    ("kitchen", 88),
    ("livingRm", 196),
];

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

        // on exitFrame                                    -- the frame script
        //   repeat with i = 1 to 48: puppetSprite i, 1
        //   moveToLocation( oPuppeteer )
        //   gStaticFrame = 0
        //   gStaticWhere = getState( #tunedIn )
        //   if getState( #currentLocation ) = #bedrm_fadeIn then
        //     cursorOff
        //     fadeOutTransit
        //     setaProp( oStoryteller.states, #soundChannels,
        //       [ 1: [#sndType: #virtualLoop, #sndName: #BRradio, #volume: 0],
        //         2: [#sndType: #loop,        #sndName: #BRclock, #volume: 0],
        //         3: [#sndType: #None,        #sndName: #None,    #volume: 0],
        //         4: [#sndType: #None,        #sndName: #None,    #volume: 0] ] )
        //     restoreSounds
        //     setState( #showMontage, 1 )
        //     goTo( #bedrm_margaret, #fadeIn )
        //     setState( #showMontage, 2 ) : setTransition( #fadeIn ) : updateDisplay : wait 45
        //     setState( #showMontage, 3 ) : setTransition( #fadeIn ) : updateDisplay : wait 45
        //     setState( #showMontage, 4 ) : setTransition( #fadeIn ) : updateDisplay : wait 60
        //     setState( #showMontage, 0 ) : setTransition( #fadeIn ) : updateDisplay
        //     fadeUpRadio( #None, 1 )
        //     wait 20
        //     assertSound #awful
        //
        // Margaret's chapter opening, and the thing that was missing.
        //
        // It is a *frame script* -- Director runs `exitFrame` as each frame
        // ends -- which is why searching the verbs for an opening handler
        // found nothing. Entry 78 looked at `startMovie` and `enterFrame` and
        // concluded there was none. `exitFrame` is the one that matters, and
        // every chapter has one: Roxy's carries the scan unit's countdown.
        //
        // This engine has no score and no frames, so only the `#bedrm_fadeIn`
        // branch is ported and it is run once, when the chapter is entered,
        // rather than every frame. The rest of the handler is `moveToLocation`
        // and static bookkeeping that this engine's own loop already does.
        //
        // The sequence: the 1940s film plays out, the stage fades, the bedroom
        // radio and clock are set up silent, and the player is put in
        // `bedrm_margaret` -- the room with her body -- while the montage
        // steps 1, 2, 3, 4 and back to 0 over the top of it. Then the radio
        // comes up and she says `#awful`.
        //
        // The `#volume: 0` on both loops is not a mistake to be corrected:
        // `fadeUpRadio` at the end is what brings them in.
        "exitframe" => {
            if state.get("currentLocation").as_symbol() != Some("bedrm_fadeIn") {
                return true;
            }
            // The opening film first; everything below is what happens after.
            out.effects.push(Effect::WaitForVideo);
            out.effects.push(Effect::CursorOff);

            out.effects.push(Effect::StartLoop {
                name: "BRradio".into(),
                volume: Some(0),
            });
            out.effects.push(Effect::StartLoop {
                name: "BRclock".into(),
                volume: Some(0),
            });
            out.effects.push(Effect::RestoreSounds { fade: false });

            out.effects.push(Effect::SetState {
                key: "showMontage".into(),
                value: Value::Int(1),
            });
            out.effects.push(Effect::GoToRoom {
                room: "bedrm_margaret".into(),
                transition: Some("fadeIn".into()),
            });
            // Each step fades in and holds; the last drops the montage and
            // leaves the room underneath it on screen.
            for (step, hold) in [(2, 45), (3, 45), (4, 60)] {
                out.effects.push(Effect::FadeToMontage(step));
                out.effects.push(Effect::WaitTicks(hold));
            }
            out.effects.push(Effect::FadeToMontage(0));

            out.effects.push(Effect::StartLoop {
                name: "BRradio".into(),
                volume: Some(255),
            });
            out.effects.push(Effect::WaitTicks(20));
            out.effects.push(Effect::PlaySound {
                name: "awful".into(),
                loudness: None,
            });
        }

        // on initRadioDial
        //   gStaticMarkers = [ #bedroomWarm: [0, 4, 8], #bedroomCool: [12, 16, 20],
        //                      #diningRmWarm: [48, 52, 56], #diningRmCool: [60, 64, 68],
        //                      #livingRmWarm: [72, 76, 80], #livingRmCool: [84, 88, 92],
        //                      #kitchenWarm: [96, 100, 104], #kitchenCool: [108, 112, 116],
        //                      #bedroom: [228], #diningRm: [236], #livingRm: [240],
        //                      #kitchen: [244], #inBetween: [216, 220, 224] ]
        //   if inState( #tunedIn, #diningRm ) then
        //     gStaticMarkers[#inBetween]   = [264, 268, 272]
        //     gStaticMarkers[#bedroom]     = [232]
        //     gStaticMarkers[#bedroomWarm] = [24, 28, 32]
        //     gStaticMarkers[#bedroomCool] = [36, 40, 44]
        //     if getState( #dumbWaiter ) = #kitchen then
        //       gStaticMarkers[#kitchen]     = [256]
        //       gStaticMarkers[#kitchenWarm] = [168, 172, 176]
        //       gStaticMarkers[#kitchenCool] = [180, 184, 188]
        //     else
        //       gStaticMarkers[#kitchen]     = [248]
        //       gStaticMarkers[#kitchenWarm] = [120, 124, 128]
        //       gStaticMarkers[#kitchenCool] = [132, 136, 140]
        //
        // The frames of `radio.mov` to show for each band of the dial: the
        // station itself, and the warm and cool bands either side of it where
        // the signal is coming in or going out.
        //
        // The branch worth keeping: **where the kitchen station sits depends
        // on where the dumb waiter is**. The kitchen radio is heard up the
        // dumb waiter shaft, so moving the shaft moves the station. Nothing
        // else in the chapter ties two puzzles together this quietly, and it
        // was invisible until entry 82 -- every one of these numbers was being
        // printed as an unrelated symbol.
        "initradiodial" => {
            const BASE: [(&str, &[i32]); 13] = [
                ("bedroomWarm", &[0, 4, 8]),
                ("bedroomCool", &[12, 16, 20]),
                ("diningRmWarm", &[48, 52, 56]),
                ("diningRmCool", &[60, 64, 68]),
                ("livingRmWarm", &[72, 76, 80]),
                ("livingRmCool", &[84, 88, 92]),
                ("kitchenWarm", &[96, 100, 104]),
                ("kitchenCool", &[108, 112, 116]),
                ("bedroom", &[228]),
                ("diningRm", &[236]),
                ("livingRm", &[240]),
                ("kitchen", &[244]),
                ("inBetween", &[216, 220, 224]),
            ];
            let mark = |state: &mut State, band: &str, frames: &[i32]| {
                state.set_all(
                    &format!("gStaticMarker_{band}"),
                    frames.iter().map(|f| Value::Int(*f)).collect(),
                );
            };
            for (band, frames) in BASE {
                mark(state, band, frames);
            }
            // The original seeds the station positions only `if voidp`, but
            // they are constants, so writing them every time is the same
            // thing without a guard that has to be believed.
            for (station, at) in RADIO_STATIONS {
                state.set_all(&format!("gRadioStation_{station}"), vec![Value::Int(at)]);
            }
            if state.get_all("tunedIn").iter().any(|v| v.as_symbol() == Some("diningRm")) {
                mark(state, "inBetween", &[264, 268, 272]);
                mark(state, "bedroom", &[232]);
                mark(state, "bedroomWarm", &[24, 28, 32]);
                mark(state, "bedroomCool", &[36, 40, 44]);
                if state.get("dumbWaiter").as_symbol() == Some("kitchen") {
                    mark(state, "kitchen", &[256]);
                    mark(state, "kitchenWarm", &[168, 172, 176]);
                    mark(state, "kitchenCool", &[180, 184, 188]);
                } else {
                    mark(state, "kitchen", &[248]);
                    mark(state, "kitchenWarm", &[120, 124, 128]);
                    mark(state, "kitchenCool", &[132, 136, 140]);
                }
            }
        }

        // on radioDial upOrDown
        //   if getState( #tunedIn ) <> #inBetween then
        //     if not voidp( gStaticWhere ) then gStaticWhere = getState( #tunedIn )
        //     setState( #tunedIn, #inBetween )
        //     endLoop #BRclock : endLoop #Kclock : endLoop #DRclock
        //     endLoop #LRclock : endLoop #roaringFire
        //     idle
        //     set the visible of sprite 44 = 1
        //   if upOrDown = #up then
        //     if gRadioDial < 240 then gRadioDial = gRadioDial + 4
        //   else
        //     if gRadioDial > 3   then gRadioDial = gRadioDial - 4
        //   set the movieTime of sprite 45 = gRadioDial
        //   patchPalette
        //   repeat while mouseDown ...
        //
        // The dial is a movie scrubbed by hand: sixty-one positions four ticks
        // apart. Turning it off a station stops the room's clock and its fire
        // with it, which is the point -- those loops are the station, heard
        // through the house rather than out of the radio.
        "radiodial" => {
            let up = args
                .first()
                .and_then(Value::as_str)
                .is_some_and(|d| d.trim_start_matches('#').eq_ignore_ascii_case("up"));

            let tuned = state.get("tunedIn");
            if tuned.as_symbol() != Some("inBetween") {
                state.set_all("gStaticWhere", vec![tuned]);
                state.set("tunedIn", Value::Symbol("inBetween".into()));
                for loop_name in ["BRclock", "Kclock", "DRclock", "LRclock", "roaringFire"] {
                    out.effects.push(Effect::StopLoop {
                        name: loop_name.into(),
                        fade: false,
                    });
                }
            }

            let at = state.get("gRadioDial").as_int().unwrap_or(0);
            // Both limits are one-sided in the original: it steps up while
            // below 240 and down while above 3, so the ends are 240 and 0.
            let moved = if up {
                if at < 240 { at + 4 } else { at }
            } else if at > 3 {
                at - 4
            } else {
                at
            };
            state.set_all("gRadioDial", vec![Value::Int(moved)]);
            // The needle is a frame of the dial movie, not a sprite.
            out.effects.push(Effect::PlayVideoSegment {
                from: moved as u32,
                to: moved as u32 + 4,
            });
            out.repeat_while_held = true;
            out.redraw = true;

            call("checkradiostations", &[], state, out);
        }

        // on checkRadioStations
        //   oldStaticWhere = gStaticWhere
        //   gStaticWhere = #inBetween
        //   if voidp( gRadioStations ) then
        //     gRadioStations = [#bedroom: 36, #diningRm: 56, #kitchen: 88, #livingRm: 196]
        //   repeat with i in gRadioStations
        //     if gRadioDial = i then gStaticWhere = getOne( gRadioStations, i ) : exit
        //     if abs( gRadioDial - i ) = 4 then
        //       gStaticWhere = value( "#" & getOne( gRadioStations, i ) & "Warm" ) : exit
        //     if abs( gRadioDial - i ) = 8 then
        //       gStaticWhere = value( "#" & getOne( gRadioStations, i ) & "Cool" ) : exit
        //   nearbyRoom = getOne( gRadioStations, i )
        //   onTheAir = getProp( oStoryteller.states, #tunedIn )
        //   if getPos( onTheAir, nearbyRoom ) = 0 then return
        //   if nearbyRoom = #diningRm then
        //     ... demote gStaticWhere one band per pass ...
        //     if ticks > gDR_timer + 120 then
        //       cursorOff
        //       setLoop( #radioTuner, 120 ) : setLoop( #DRradio, 90 )
        //       assertSound #thatStation
        //       ... wait for #thatStation to finish ...
        //       cursorOn
        //       setLoop( #radioTuner, 230 ) : setLoop( #DRradio, 120 )
        //       gDR_timer = ticks
        //   else
        //     gStaticWhere = #inBetween
        //
        // The band names are built by string concatenation -- `"#" & station &
        // "Warm"` -- which is why they never appeared in the name table and
        // why `gStaticMarkers` looked like it was keyed on nothing.
        //
        // `onTheAir` is `getProp( states, #tunedIn )`, the whole list rather
        // than its head, so it falls out of the list-valued state model for
        // free: a station is on the air if it is one of the values #tunedIn
        // is allowed to take.
        //
        // Only the dining room gets the full fade-in here. The other three
        // stations set the band and stop, and their programmes are started
        // elsewhere; that is the original's shape, not an omission of mine.
        "checkradiostations" => {
            let at = state.get("gRadioDial").as_int().unwrap_or(0);
            let mut band = "inBetween".to_string();
            let mut nearby = None;
            for (station, default) in RADIO_STATIONS {
                let pos = state
                    .get(&format!("gRadioStation_{station}"))
                    .as_int()
                    .unwrap_or(default);
                let away = (at - pos).abs();
                let found = match away {
                    0 => Some(station.to_string()),
                    4 => Some(format!("{station}Warm")),
                    8 => Some(format!("{station}Cool")),
                    _ => None,
                };
                if let Some(found) = found {
                    band = found;
                    nearby = Some(station);
                    break;
                }
            }
            state.set_all("gStaticWhere", vec![Value::Symbol(band.clone())]);
            out.redraw = true;

            let Some(nearby) = nearby else { return true };
            let on_the_air = state
                .get_all("tunedIn")
                .iter()
                .any(|v| v.as_symbol() == Some(nearby));
            if !on_the_air || nearby != "diningRm" || band != "diningRm" {
                return true;
            }
            state.set("tunedIn", Value::Symbol("diningRm".into()));
            out.effects.push(Effect::CursorOff);
            // Under the announcement, then up once it has finished.
            out.effects.push(Effect::StartLoop { name: "radioTuner".into(), volume: Some(120) });
            out.effects.push(Effect::StartLoop { name: "DRradio".into(), volume: Some(90) });
            out.effects.push(Effect::PlaySound { name: "thatStation".into(), loudness: None });
            out.effects.push(Effect::WaitForSound("thatStation".into()));
            out.effects.push(Effect::StartLoop { name: "radioTuner".into(), volume: Some(230) });
            out.effects.push(Effect::StartLoop { name: "DRradio".into(), volume: Some(120) });
        }

        // on backAwayFromRadio
        //   currentRoom = getState( #tunedIn )
        //   if currentRoom <> #inBetween then
        //     cursorOff : updateDisplay( oPuppeteer ) : trimState
        //     if currentRoom <> #bedroom then
        //       endLoop #DRradio : endLoop #LRradio : endLoop #Kradio ...
        //
        // Walking away leaves the station you tuned playing and stops the
        // other three, so the house keeps the radio on behind you.
        "backawayfromradio" => {
            let tuned = state.get("tunedIn");
            let Some(room) = tuned.as_symbol().map(str::to_string) else {
                return true;
            };
            if room == "inBetween" {
                return true;
            }
            out.effects.push(Effect::CursorOff);
            for (station, _) in RADIO_STATIONS {
                let loop_name = match station {
                    "bedroom" => "BRradio",
                    "diningRm" => "DRradio",
                    "livingRm" => "LRradio",
                    _ => "Kradio",
                };
                if station != room {
                    out.effects.push(Effect::StopLoop {
                        name: loop_name.into(),
                        fade: false,
                    });
                }
            }
            out.redraw = true;
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
            // Both of these are behind a platform test, and the two arms are
            // opposites:
            //
            //   if gCPU = #Mac  then setLoop( #loopingStatic, 0 )
            //   if gCPU <> #Mac then suspendSounds
            //
            // The static is started at volume *zero* -- it is a placeholder
            // the Mac build keeps silent -- and the PC build ducks the bed
            // instead. This disc's movies are `RIFX`, which is the Mac
            // ordering, so the Mac arm is the one that applies. Playing the
            // static at full volume, as this did, put a constant hiss over
            // every door in the chapter.
            out.effects.push(Effect::StartLoop {
                name: "loopingStatic".into(),
                volume: Some(0),
            });
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


    // -- the radio ----------------------------------------------------------

    fn radio() -> State {
        let mut s = State::new();
        s.set_all("gChapter", vec![Value::Symbol("MARGARET".into())]);
        s.set_all("tunedIn", vec![Value::Symbol("inBetween".into())]);
        let mut out = Outcome::default();
        assert!(call("initradiodial", &[], &mut s, &mut out));
        s
    }

    fn turn(state: &mut State, up: bool) -> Outcome {
        let mut out = Outcome::default();
        let dir = if up { "up" } else { "down" };
        assert!(call("radiodial", &[Value::Symbol(dir.into())], state, &mut out));
        out
    }

    fn dial(state: &State) -> i32 {
        state.get("gRadioDial").as_int().unwrap_or(-1)
    }

    fn where_at(state: &mut State, at: i32) -> String {
        state.set_all("gRadioDial", vec![Value::Int(at)]);
        let mut out = Outcome::default();
        assert!(call("checkradiostations", &[], state, &mut out));
        state.get("gStaticWhere").as_symbol().unwrap_or("").to_string()
    }

    fn stopped(out: &Outcome) -> Vec<String> {
        out.effects
            .iter()
            .filter_map(|e| match e {
                Effect::StopLoop { name, .. } => Some(name.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_dial_stops_at_both_ends() {
        let mut s = radio();
        for _ in 0..70 {
            turn(&mut s, false);
        }
        assert_eq!(dial(&s), 0);
        for _ in 0..70 {
            turn(&mut s, true);
        }
        assert_eq!(dial(&s), 240);
        // And does not run past it.
        turn(&mut s, true);
        assert_eq!(dial(&s), 240);
    }

    #[test]
    fn the_dial_moves_four_at_a_time() {
        let mut s = radio();
        turn(&mut s, true);
        assert_eq!(dial(&s), 4);
        turn(&mut s, true);
        assert_eq!(dial(&s), 8);
        turn(&mut s, false);
        assert_eq!(dial(&s), 4);
    }

    #[test]
    fn turning_off_a_station_stops_the_room_with_it() {
        let mut s = radio();
        s.set("tunedIn", Value::Symbol("diningRm".into()));
        let out = turn(&mut s, true);
        assert_eq!(s.get("tunedIn"), Value::Symbol("inBetween".into()));
        assert_eq!(
            stopped(&out),
            ["BRclock", "Kclock", "DRclock", "LRclock", "roaringFire"]
        );
    }

    #[test]
    fn and_only_does_that_once() {
        let mut s = radio();
        s.set("tunedIn", Value::Symbol("diningRm".into()));
        turn(&mut s, true);
        // Already between stations: nothing left to stop.
        let out = turn(&mut s, true);
        assert!(stopped(&out).is_empty());
    }

    #[test]
    fn a_station_has_a_warm_and_a_cool_band_either_side() {
        let mut s = radio();
        assert_eq!(where_at(&mut s, 56), "diningRm");
        assert_eq!(where_at(&mut s, 52), "diningRmWarm");
        assert_eq!(where_at(&mut s, 60), "diningRmWarm");
        assert_eq!(where_at(&mut s, 48), "diningRmCool");
        assert_eq!(where_at(&mut s, 64), "diningRmCool");
    }

    #[test]
    fn and_nothing_at_all_between_them() {
        let mut s = radio();
        // Far from all four of 36, 56, 88 and 196.
        assert_eq!(where_at(&mut s, 150), "inBetween");
    }

    #[test]
    fn the_dumb_waiter_moves_the_kitchen_station() {
        let frames = |s: &State| {
            s.get_all("gStaticMarker_kitchenWarm")
                .iter()
                .filter_map(Value::as_int)
                .collect::<Vec<_>>()
        };
        // The rewrite only happens once the dining room is on the air.
        let mut down = State::new();
        down.set_all("gChapter", vec![Value::Symbol("MARGARET".into())]);
        down.set_all("tunedIn", vec![Value::Symbol("diningRm".into())]);
        let mut up = down.clone();
        down.set("dumbWaiter", Value::Symbol("kitchen".into()));
        up.set("dumbWaiter", Value::Symbol("attic".into()));
        let mut out = Outcome::default();
        assert!(call("initradiodial", &[], &mut down, &mut out));
        assert!(call("initradiodial", &[], &mut up, &mut out));
        assert_eq!(frames(&down), [168, 172, 176]);
        assert_eq!(frames(&up), [120, 124, 128]);
    }

    #[test]
    fn walking_away_leaves_your_station_playing() {
        let mut s = radio();
        s.set("tunedIn", Value::Symbol("diningRm".into()));
        let mut out = Outcome::default();
        assert!(call("backawayfromradio", &[], &mut s, &mut out));
        assert_eq!(stopped(&out), ["BRradio", "Kradio", "LRradio"]);
    }

    #[test]
    fn and_stops_nothing_if_you_never_tuned_one() {
        let mut s = radio();
        let mut out = Outcome::default();
        assert!(call("backawayfromradio", &[], &mut s, &mut out));
        assert!(stopped(&out).is_empty());
    }

    // -- the chapter opening ------------------------------------------------

    fn opening() -> (State, Outcome) {
        let mut s = State::new();
        s.set_all("gChapter", vec![Value::Symbol("MARGARET".into())]);
        s.set_all("currentLocation", vec![Value::Symbol("bedrm_fadeIn".into())]);
        let mut out = Outcome::default();
        assert!(call("exitframe", &[], &mut s, &mut out));
        (s, out)
    }

    #[test]
    fn the_opening_waits_for_its_film_before_anything_else() {
        let (_, out) = opening();
        assert!(matches!(out.effects.first(), Some(Effect::WaitForVideo)));
    }

    #[test]
    fn and_then_puts_the_player_where_the_body_is() {
        let (_, out) = opening();
        let moved = out.effects.iter().find_map(|e| match e {
            Effect::GoToRoom { room, transition } => Some((room.clone(), transition.clone())),
            _ => None,
        });
        assert_eq!(
            moved,
            Some(("bedrm_margaret".to_string(), Some("fadeIn".to_string())))
        );
    }

    #[test]
    fn the_montage_steps_up_and_then_clears() {
        let (_, out) = opening();
        let steps: Vec<i32> = out
            .effects
            .iter()
            .filter_map(|e| match e {
                Effect::FadeToMontage(n) => Some(*n),
                _ => None,
            })
            .collect();
        // One is set before the move, so it arrives as a plain state write.
        assert_eq!(steps, [2, 3, 4, 0]);
    }

    #[test]
    fn the_bedroom_loops_start_silent_and_are_brought_up_after() {
        let (_, out) = opening();
        let radio: Vec<Option<i32>> = out
            .effects
            .iter()
            .filter_map(|e| match e {
                Effect::StartLoop { name, volume } if name == "BRradio" => Some(*volume),
                _ => None,
            })
            .collect();
        assert_eq!(radio, [Some(0), Some(255)]);
    }

    #[test]
    fn and_nothing_happens_anywhere_else() {
        let mut s = State::new();
        s.set_all("gChapter", vec![Value::Symbol("MARGARET".into())]);
        s.set_all("currentLocation", vec![Value::Symbol("bedrm_A1".into())]);
        let mut out = Outcome::default();
        assert!(call("exitframe", &[], &mut s, &mut out));
        assert!(out.effects.is_empty());
    }
}

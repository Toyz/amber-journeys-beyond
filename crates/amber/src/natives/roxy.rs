//! Roxy's chapter: the present-day house, the office laptop and the
//! ghost telephone.
//!
//! Each handler carries the disassembly it was read from, so the port can be
//! checked against the original without going back to the bytecode.

use lingo::Value;

use crate::script::{Effect, Outcome};
use crate::state::State;

use super::roll;

/// One ambience loop a door lets through, and where it can be heard from.
struct Bleed {
    /// Areas the loop is audible in -- `#Hall`, `#Porch` -- rather than
    /// rooms. Anywhere else, the door makes no difference to what the player
    /// hears.
    rooms: &'static [&'static str],
    /// Extra conditions, as `(flag, value)`, all of which must hold.
    guards: &'static [(&'static str, &'static str)],
    loop_name: &'static str,
    /// Volume when starting. `None` is the original's `#disablePeekAlert`,
    /// which is a flag rather than a level.
    volume: Option<i32>,
}

/// Doors whose opening and shutting is heard beyond the door itself.
///
/// The plain openable setters in `shared` write a flag and play a cue. These
/// three do that and then start or stop an ambience loop, but only when the
/// player is somewhere the loop would carry to: the grounds become audible
/// from the hall when the front door opens, and the house hum reaches the
/// porch. Standing anywhere else, the door is only a sound.
const BLEED_DOORS: &[(&str, &str, &str, &[Bleed])] = &[
    (
        "setfrontdoorisopen",
        "frontDoorOpen",
        "frontDoorClose",
        &[
            Bleed { rooms: &["DarkDn", "Hall"], guards: &[], loop_name: "grounds", volume: None },
            Bleed { rooms: &["Porch"], guards: &[], loop_name: "houseHum", volume: Some(80) },
        ],
    ),
    (
        "setkitchenreardoorisopen",
        "kitchenExitOpen",
        "kitchenExitClose",
        &[
            Bleed { rooms: &["DarkDn", "kitchen"], guards: &[], loop_name: "grounds", volume: None },
            Bleed { rooms: &["Ghse"], guards: &[], loop_name: "houseHum", volume: None },
            // The scanner on the kitchen door is only heard through it when
            // the unit is actually mounted there and switched on.
            Bleed {
                rooms: &["kitchen"],
                guards: &[("DoorWithScanUnit", "kitchenOutside"), ("scanUnitIsActive", "1")],
                loop_name: "scanLoop",
                volume: Some(120),
            },
        ],
    ),
    (
        "setbalconydoorisopen",
        "doorOpen",
        "doorClose",
        &[
            Bleed {
                rooms: &["UHallBalconyEntry", "UHallNwall2", "DarkUp_UHallNwall2", "DarkUp_BalcEntry"],
                guards: &[],
                loop_name: "grounds",
                volume: None,
            },
            Bleed {
                rooms: &["UHallBalconyN", "UHallBalconyS", "DarkUp_BalcNorth", "DarkUp_BalcSouth"],
                guards: &[],
                loop_name: "houseHum",
                volume: Some(80),
            },
        ],
    ),
];

/// The shared body of the three doors above.
///
///   on set<X>IsOpen suggestion
///     currentState = getState( #X )
///     currentRoom  = <where the player is>
///     if suggestion = 0 and currentState = 1 then
///       cue( #<x>Close ) : setProp( #X, list(0) ) : updateDisplay
///       if currentRoom = ... then endLoop( #grounds )
///       if currentRoom = ... then endLoop( #houseHum )
///     if suggestion = 1 and currentState = 0 then
///       cue( #<x>Open ) : setProp( #X, list(1) ) : updateDisplay
///       if currentRoom = ... then setLoop( #grounds, #disablePeekAlert )
///       if currentRoom = ... then setLoop( #houseHum, 80 )
///
/// Guarded on the flag changing, like the plain setters, so a door already
/// open neither sounds nor disturbs the loops.
fn bleed_door(name: &str, args: &[Value], state: &mut State, out: &mut Outcome) -> bool {
    // Margaret has a balcony door and a front door of her own, handled by the
    // plain setters. These rules are Roxy's rooms and Roxy's loops, so the
    // chapter is checked here rather than left to the order handlers run in.
    if state
        .get("gChapter")
        .as_str()
        .is_none_or(|c| !c.eq_ignore_ascii_case("ROXY"))
    {
        return false;
    }
    let Some(&(_, open_cue, close_cue, bleeds)) =
        BLEED_DOORS.iter().find(|(h, _, _, _)| *h == name)
    else {
        return false;
    };

    let flag = &name[3..];
    let current = state.get(flag).as_int().unwrap_or(0);
    let Some(suggestion) = args.first().and_then(Value::as_int) else {
        return true;
    };
    let opening = match (suggestion, current) {
        (1, 0) => true,
        (0, 1) => false,
        _ => return true,
    };

    out.effects.push(Effect::PlaySound {
        name: if opening { open_cue } else { close_cue }.into(),
        loudness: None,
    });
    state.set_all(flag, vec![Value::Int(suggestion)]);
    out.redraw = true;

    let here = state.get("gZone");
    let here = here.as_str().unwrap_or_default().trim_start_matches('#').to_string();
    for bleed in bleeds {
        if !bleed.rooms.iter().any(|r| r.eq_ignore_ascii_case(&here)) {
            continue;
        }
        if !bleed.guards.iter().all(|(flag, want)| {
            let held = state.get(flag);
            held.as_str().is_some_and(|s| s.eq_ignore_ascii_case(want))
                || held.as_int().map(|n| n.to_string()).as_deref() == Some(*want)
        }) {
            continue;
        }
        out.effects.push(if opening {
            Effect::StartLoop {
                name: bleed.loop_name.into(),
                volume: bleed.volume,
            }
        } else {
            Effect::StopLoop {
                name: bleed.loop_name.into(),
                fade: false,
            }
        });
    }
    true
}

/// Runs a handler from this chapter, or reports that it is not one of ours.
pub fn call(name: &str, args: &[Value], state: &mut State, out: &mut Outcome) -> bool {
    if bleed_door(name, args, state, out) {
        return true;
    }
    // Arguments and effects are unused by some chapters until more handlers
    // land here; the signature is uniform so the dispatcher stays simple.
    let _ = (args, &out, &state);
    match name {
        // on freezeInventory
        //   oPuppeteer cursor #cool
        //   setState(oStoryteller, #inventoryStatus, #cool)
        //   gFreezeInventory = 1
        "freezeinventory" => {
            state.set("inventoryStatus", Value::Symbol("cool".into()));
            state.set("gFreezeInventory", Value::Int(1));
        }


        // on stashClick
        //   gClickLoc = point(the mouseH, the mouseV)
        //
        // Records where the last click landed, for handlers that need the
        // position rather than just the fact of a click. The engine writes the
        // live position into state on every click, so this copies it across.
        "stashclick" => {
            let point = state.get("gMouseLoc");
            state.set("gClickLoc", point);
        }


        // on approachOfficeLaptop
        //   currentScreen = getState(oStoryteller, #BT_fragStatus)
        //   if currentScreen = #None then goTo #OfficeMonitorCU, #lookAt : exit
        //   setState(oStoryteller, #showMontage, 2)
        //   if currentScreen = #crisisPrompt then goTo #OfficeMonitor_PTsuite
        //   if currentScreen = #alignment or #spinningNow then
        //     goTo #OfficeMonitor_alignment
        //   if currentScreen = #algoPrompt or #algorithm then
        //     goTo #OfficeMonitor_algorithm, #fadeIn
        //   if currentScreen = #allDone then goTo #OfficeMonitor_algorithm
        //
        // The laptop shows whichever screen the fragment puzzle has reached,
        // so focusing it is a branch on that progress rather than one fixed
        // close-up. `loadMultiframes` calls between the branches are preload
        // hints and need nothing here.
        "approachofficelaptop" => {
            let screen = state
                .get("BT_fragStatus")
                .as_str()
                .unwrap_or("None")
                .to_ascii_lowercase();
            let (room, transition) = match screen.as_str() {
                "none" => ("OfficeMonitorCU", "lookAt"),
                "crisisprompt" => ("OfficeMonitor_PTsuite", "lookAt"),
                "alignment" | "spinningnow" => ("OfficeMonitor_alignment", "lookAt"),
                "algoprompt" | "algorithm" | "alldone" => {
                    ("OfficeMonitor_algorithm", "fadeIn")
                }
                // An unrecognised screen still gets the plain close-up rather
                // than leaving the click dead.
                _ => ("OfficeMonitorCU", "lookAt"),
            };
            if screen != "none" {
                state.set("showMontage", Value::Int(2));
            }
            out.destination = Some(room.to_string());
            out.transition = Some(transition.to_string());
        }


        // on backAwayFromLaptop
        //   currentScreen = getState(oStoryteller, #BT_fragStatus)
        //   if currentScreen = #None then exit
        //   cursorOff
        //   if currentScreen = #spinningNow then
        //     setState(oStoryteller, #BT_fragStatus, #alignment)
        //   if getState(oStoryteller, #showMontage) <> 2 then
        //     setState(oStoryteller, #showMontage, 3)
        //     updateDisplay(oPuppeteer)
        //     resumeTime = the ticks + 60
        //   else
        //     resumeTime = the ticks
        //   purgeMultiframes(...)
        //   repeat while the ticks < resumeTime : updateStage
        //   setState(oStoryteller, #showMontage, 2)
        //   updateDisplay(oPuppeteer)
        //
        // The busy loop is a one-second hold on the intermediate montage
        // before settling back, which the engine expresses as a wait rather
        // than by spinning.
        "backawayfromlaptop" => {
            let screen = state
                .get("BT_fragStatus")
                .as_str()
                .unwrap_or("None")
                .to_ascii_lowercase();
            if screen == "none" {
                return true;
            }
            out.effects.push(Effect::CursorOff);
            if screen == "spinningnow" {
                state.set("BT_fragStatus", Value::Symbol("alignment".into()));
            }
            // The intermediate montage has to be on screen across the hold,
            // so both writes are queued. Written directly they would flip
            // through 3 to 2 before the wait ever ran, and the second of the
            // two would be all anyone saw.
            if state.get("showMontage").as_int() != Some(2) {
                out.effects.push(Effect::SetState {
                    key: "showMontage".into(),
                    value: Value::Int(3),
                });
                out.redraw = true;
                out.effects.push(Effect::WaitTicks(60));
            }
            out.effects.push(Effect::SetState {
                key: "showMontage".into(),
                value: Value::Int(2),
            });
            out.redraw = true;
        }


        // on ghostCalls suggestion, howLoud
        //   possibleCallLists = [#allGhosts, #Brice_entry, #Margaret_entry,
        //                        #Edwin_entry, #Brice_warm, ..., #None]
        //   if getPos(possibleCallLists, suggestion) = 0 then exit
        //   suggestedCalls = []
        //   if suggestion = #allGhosts then
        //     repeat over [#Margaret, #Brice, #Edwin]
        //       if inState(#ghostsRemaining, theGhost) then append it
        //     append #nobody three times
        //   if suggestion = #Brice_entry then
        //     if inState(#ghostsRemaining, #Brice) then [#Brice]
        //   if suggestion = #Brice_warm then  [#Brice, #nobody, #nobody]
        //   if suggestion = #Brice_cool then  [#Brice, #nobody, #nobody, #nobody]
        //   ... and the same for Margaret and Edwin
        //
        // The ghosts telephone the player, and the padding is the weighting:
        // an entry call always lands, a warm one lands once in three and a
        // cool one once in four. A ghost already dealt with is not a
        // candidate, so `#ghostsRemaining` both gates and thins the calls as
        // the game is solved.
        "ghostcalls" => {
            let suggestion = args
                .first()
                .and_then(Value::as_str)
                .unwrap_or("None")
                .trim_start_matches('#')
                .to_string();
            let loudness = args.get(1).and_then(Value::as_str).unwrap_or("medium");

            // Volume by loudness, stored where the mixer can read it.
            let volume = match loudness.trim_start_matches('#') {
                "low" => 90,
                "high" => 255,
                _ => 160,
            };
            state.set("ghostCallVol", Value::Int(volume));

            let remaining = state.get_all("ghostsRemaining").to_vec();
            let present = |who: &str| match remaining.as_slice() {
                items if !items.is_empty() => items.iter().any(|i| {
                    i.as_str().is_some_and(|s| s.eq_ignore_ascii_case(who))
                }),
                // Before the list is seeded every ghost is still to be dealt
                // with, which is the state the game opens in.
                _ => true,
            };

            // Build the weighted candidate list exactly as the original does.
            let mut candidates: Vec<Option<&str>> = Vec::new();
            let lower = suggestion.to_ascii_lowercase();
            if lower == "allghosts" {
                for who in ["Margaret", "Brice", "Edwin"] {
                    if present(who) {
                        candidates.push(Some(who));
                    }
                }
                candidates.extend([None, None, None]);
            } else if let Some((who, kind)) = lower.split_once('_') {
                let who = match who {
                    "brice" => "Brice",
                    "margaret" => "Margaret",
                    "edwin" => "Edwin",
                    _ => return true,
                };
                if present(who) {
                    candidates.push(Some(who));
                    let padding = match kind {
                        "entry" => 0,
                        "warm" => 2,
                        "cool" => 3,
                        _ => return true,
                    };
                    candidates.extend(std::iter::repeat(None).take(padding));
                }
            } else {
                // #None and anything unrecognised place no call.
                return true;
            }

            if candidates.is_empty() {
                return true;
            }
            let pick = roll(state, candidates.len() as i32) as usize - 1;
            let Some(Some(who)) = candidates.get(pick) else {
                return true;
            };

            // Each ghost's calls are external files named by initial: Brice
            // has eleven, Edwin twelve, Margaret ten.
            let (prefix, count) = match *who {
                "Brice" => ("BCALL", 11),
                "Edwin" => ("ECALL", 12),
                _ => ("MCALL", 10),
            };
            let n = roll(state, count);
            out.effects.push(Effect::PlaySound {
                name: format!("{prefix}{n}"),
                loudness: Some(loudness.trim_start_matches('#').to_string()),
            });
        }

        // on peekAlert
        //   if gPeekAlertEnabled = 0 or getState(#playerHasPeekUnit) = 0 then exit
        //   colorGraphic    = getAt(getProp(oPuppeteer, #PeekUnit), 1)
        //   highGlowGraphic = getAt(..., 3)
        //   lowGlowGraphic  = getAt(..., 2)
        //   oldPeekGraphic  = the castNum of sprite 7
        //   if oldPeekGraphic <> colorGraphic then lowGlowGraphic = colorGraphic
        //   repeat with i = 1 to 12
        //     hold five ticks
        //     alternate sprite 7 between the high and low glow
        //   end repeat
        //
        // The peek unit pulses in the inventory bar to say it has something to
        // show. Its three icons are the plain one and two glows, which is why
        // the inventory table lists three casts for this item and two for
        // every other.
        "peekalert" => {
            let enabled = state.get("gPeekAlertEnabled").as_int().unwrap_or(0) != 0;
            let carried = state.get("playerHasPeekUnit").as_int().unwrap_or(0) != 0;
            if !enabled || !carried {
                return true;
            }
            // The icons are the second and third of the item's three, which
            // the inventory table already names.
            for i in 0..12 {
                out.effects.push(Effect::WaitTicks(5));
                // The third icon is the bright glow and the second the dim
                // one, which is how the item comes to list three where every
                // other lists two.
                out.effects.push(Effect::SpriteCastIcon {
                    channel: 7,
                    item: "PeekUnit".into(),
                    index: if i % 2 == 0 { 3 } else { 2 },
                });
            }
        }

        // on testForMargGhost  /  on testForMirrorMsg
        //   activeHaunts = getProp(oStoryteller, #hauntsRemaining)
        //   if getPos(activeHaunts, #ghostBrushingHair) then
        //     cursorOff
        //     if gCPU = #PC then suspendSounds #fadeOut
        //     pushVideo
        //     wait #videoStop
        //     if gCPU = #PC then restoreSounds #fadeIn
        //     trimState #hauntsRemaining, #ghostBrushingHair
        //
        // A haunt plays only while it is still in the pool and trims itself
        // once it has, so the house runs out of things to do as the player
        // sees them. The two differ only in which haunt they are.
        "testformargghost" | "testformirrormsg" => {
            let haunt = if name == "testformargghost" {
                "ghostBrushingHair"
            } else {
                "mirrorMessage"
            };
            let pending = state
                .get_all("hauntsRemaining")
                .iter()
                .any(|i| i.as_str().is_some_and(|s| s.eq_ignore_ascii_case(haunt)));
            if !pending {
                return true;
            }
            out.effects.push(Effect::CursorOff);
            out.effects.push(Effect::SuspendSounds { fade: true });
            out.effects.push(Effect::PlayVideo(None));
            out.effects.push(Effect::WaitForVideo);
            out.effects.push(Effect::RestoreSounds { fade: true });
            // Queued rather than written here: the movie is gated on the
            // haunt still being in the pool, so trimming it now would consume
            // the haunt before the movie it belongs to has played.
            out.effects.push(Effect::TrimState {
                key: "hauntsRemaining".into(),
                item: Value::Symbol(haunt.to_string()),
            });
        }

        // on setDoorWithScanUnit suggestion
        //   validKnobs = [#None, #kitchenOutside, #kitchenInside,
        //                 #margaretRmInside, #margaretRmOutside,
        //                 #bathroomInside, #bathroomOutside, #garageInside,
        //                 #garageOutside, #boathouseInside, #boatHouseOutside]
        //   if getPos(validKnobs, suggestion) <> 0 then
        //     oldValue = getState(#DoorWithScanUnit)
        //     if suggestion = #None and oldValue <> #None then cue #scanOffKnob
        //     if suggestion <> #None and oldValue = #None then cue #scanOntoKnob
        //     setProp(oStoryteller.states, #DoorWithScanUnit, list(suggestion))
        //
        // Where the scan unit is clipped. Only the knobs in the list are
        // accepted; anything else is a mistake the original prints and
        // ignores.
        //
        // The two cues are guarded on crossing to or from #None, so moving the
        // unit straight from one door to another makes no sound at all -- it
        // is one click, not an unclip and a clip.
        "setdoorwithscanunit" => {
            const KNOBS: [&str; 11] = [
                "None",
                "kitchenOutside",
                "kitchenInside",
                "margaretRmInside",
                "margaretRmOutside",
                "bathroomInside",
                "bathroomOutside",
                "garageInside",
                "garageOutside",
                "boathouseInside",
                "boatHouseOutside",
            ];
            let Some(knob) = args
                .first()
                .and_then(Value::as_str)
                .map(|k| k.trim_start_matches('#').to_string())
            else {
                return true;
            };
            let Some(valid) = KNOBS.iter().find(|k| k.eq_ignore_ascii_case(&knob)) else {
                trace!(
                    crate::trace::Topic::Script,
                    "setDoorWithScanUnit: {knob} is not a knob"
                );
                return true;
            };
            let is_none = |s: &str| s.eq_ignore_ascii_case("None");
            let old = state.get("DoorWithScanUnit");
            let old = old.as_str().unwrap_or("None");

            if is_none(valid) && !is_none(old) {
                out.effects.push(Effect::PlaySound {
                    name: "scanOffKnob".into(),
                    loudness: None,
                });
            } else if !is_none(valid) && is_none(old) {
                out.effects.push(Effect::PlaySound {
                    name: "scanOntoKnob".into(),
                    loudness: None,
                });
            }
            state.set_all("DoorWithScanUnit", vec![Value::Symbol((*valid).into())]);
            out.redraw = true;
        }

        // on setPKScanStatus suggestion
        //   validList = [#Offline, #CantAttach, #Online, #NoResidue, #Wait5min,
        //                #Wait4min, #Wait3min, #Wait2min, #Wait1min,
        //                #ReadyForPlayback, #Interrupted, #Preamble]
        //   if not getPos(validList, suggestion) then alert(...) : return
        //   currentStatus = getState(#PKscanStatus)
        //   if suggestion = #Online then
        //     if getPos([#Wait1min..#Wait5min], currentStatus) then
        //       gScanFinish = 0 : suggestion = #Interrupted
        //     if currentStatus = #ReadyForPlayback then suggestion = #ReadyForPlayback
        //   if suggestion = #Offline then gScanFinish = 0
        //   if currentStatus = #ReadyForPlayback then
        //     if suggestion <> #ReadyForPlayback then setState(#PeekDisplay, #None)
        //     if getPos([#Wait5min..#Wait1min], suggestion) then
        //       setState(#PeekDisplay, #goodScan5min)
        //   setProp(oStoryteller.states, #PKscanStatus, list(suggestion))
        //   if suggestion = #Interrupted and getState(#playerHasPeekUnit) = #carrying then
        //     setState(#PeekDisplay, #interruptedScan) : peekAlert
        //
        // The scan unit's state machine. The two rewrites of `suggestion` are
        // the whole of it: asking to go online while a scan is counting down
        // interrupts that scan rather than restarting it, and asking for
        // anything at all while a result is waiting keeps the result. Between
        // them the player cannot lose a finished scan by fiddling with the
        // unit, which is the only thing that would make the puzzle unfair.
        "setpkscanstatus" => {
            const VALID: [&str; 12] = [
                "Offline",
                "CantAttach",
                "Online",
                "NoResidue",
                "Wait5min",
                "Wait4min",
                "Wait3min",
                "Wait2min",
                "Wait1min",
                "ReadyForPlayback",
                "Interrupted",
                "Preamble",
            ];
            const COUNTING_DOWN: [&str; 5] = [
                "Wait1min", "Wait2min", "Wait3min", "Wait4min", "Wait5min",
            ];

            let Some(asked) = args
                .first()
                .and_then(Value::as_str)
                .map(|k| k.trim_start_matches('#').to_string())
            else {
                return true;
            };
            let Some(&wanted) = VALID.iter().find(|v| v.eq_ignore_ascii_case(&asked)) else {
                // The original raises an alert, which is a message to its
                // authors rather than to the player.
                trace!(
                    crate::trace::Topic::Script,
                    "setPKScanStatus: {asked} is not a status"
                );
                return true;
            };

            let current = state.get("PKscanStatus");
            let current = current.as_str().unwrap_or("Offline").to_string();
            let is = |a: &str, b: &str| a.eq_ignore_ascii_case(b);
            let counting = COUNTING_DOWN.iter().any(|w| is(&current, w));

            let mut settle = wanted;
            if is(wanted, "Online") {
                if counting {
                    state.set("gScanFinish", Value::Int(0));
                    settle = "Interrupted";
                }
                if is(&current, "ReadyForPlayback") {
                    settle = "ReadyForPlayback";
                }
            }
            if is(settle, "Offline") {
                state.set("gScanFinish", Value::Int(0));
            }

            if is(&current, "ReadyForPlayback") {
                if !is(settle, "ReadyForPlayback") {
                    state.set("PeekDisplay", Value::Symbol("None".into()));
                }
                if COUNTING_DOWN.iter().any(|w| is(settle, w)) {
                    state.set("PeekDisplay", Value::Symbol("goodScan5min".into()));
                }
            }

            state.set_all("PKscanStatus", vec![Value::Symbol(settle.into())]);

            let carrying = state
                .get("playerHasPeekUnit")
                .as_str()
                .is_some_and(|v| v.eq_ignore_ascii_case("carrying"));
            if is(settle, "Interrupted") && carrying {
                state.set("PeekDisplay", Value::Symbol("interruptedScan".into()));
                call("peekalert", &[], state, out);
            }
            out.redraw = true;
        }

        // on setPlayerIsExaminingPhone suggestion
        //   if suggestion = 0 then
        //     endLoop #phoneRoxy : endLoop #phoneDead
        //   else
        //     endLoop #phoneRinging
        //     setProp( oStoryteller.states, #ghostlyPhoneCall, list(#dontRingPlease) )
        //   setProp( oStoryteller.states, #playerIsExaminingPhone, list(suggestion) )
        //
        // Lifting the receiver stops the ringing and parks the call at
        // `#dontRingPlease`, which is not one of the four settings the call's
        // own setter accepts -- it is written straight to the flag, so the
        // phone cannot start ringing again while it is in the player's hand.
        "setplayerisexaminingphone" => {
            let lifted = args.first().and_then(Value::as_int).unwrap_or(0) != 0;
            let stop = |out: &mut Outcome, name: &str| {
                out.effects.push(Effect::StopLoop {
                    name: name.into(),
                    fade: false,
                });
            };
            if lifted {
                stop(out, "phoneRinging");
                state.set_all(
                    "ghostlyPhoneCall",
                    vec![Value::Symbol("dontRingPlease".into())],
                );
            } else {
                stop(out, "phoneRoxy");
                stop(out, "phoneDead");
            }
            state.set_all(
                "playerIsExaminingPhone",
                vec![Value::Int(lifted as i32)],
            );
            out.redraw = true;
        }

        // on putDownThePhone
        //   if getState(#phoneButtonsPressed) > 0 and inState(#hauntsRemaining, #spookyOperator) then
        //     setProp( ..., #phoneButtonsPressed, list(7) )
        //     setState( oStoryteller, #ghostlyPhoneCall, #speaking )
        //     setProp( ..., #phoneButtonsPressed, list(0) )
        //   else
        //     setState( oStoryteller, #playerIsExaminingPhone, 0 )
        //     setState( oStoryteller, #ghostlyPhoneCall, #done )
        //     setProp( ..., #phoneButtonsPressed, list(0) )
        //     setTransition( oPuppeteer, #fadeIn )
        //   updateDisplay( oPuppeteer )
        //
        // This is the puzzle. Pressing the buttons at all and then hanging up
        // forces the count to seven, which is the one number above the
        // threshold the speaking branch tests, so the operator answers. The
        // count is cleared either way, so a failed attempt costs nothing but
        // has to be made again from the beginning.
        "putdownthephone" => {
            let pressed = state.get("phoneButtonsPressed").as_int().unwrap_or(0);
            let operator_pending = state
                .get_all("hauntsRemaining")
                .iter()
                .any(|h| h.as_str().is_some_and(|s| s.eq_ignore_ascii_case("spookyOperator")));

            if pressed > 0 && operator_pending {
                state.set_all("phoneButtonsPressed", vec![Value::Int(7)]);
                call("setghostlyphonecall", &[Value::Symbol("speaking".into())], state, out);
                state.set_all("phoneButtonsPressed", vec![Value::Int(0)]);
            } else {
                call("setplayerisexaminingphone", &[Value::Int(0)], state, out);
                call("setghostlyphonecall", &[Value::Symbol("done".into())], state, out);
                state.set_all("phoneButtonsPressed", vec![Value::Int(0)]);
            }
            out.redraw = true;
        }

        // on setGhostlyPhoneCall suggestion
        //   if not getPos([#notyet, #ringingNow, #speaking, #done], suggestion) then alert : return
        //   if suggestion = #ringingNow then
        //     setLoop #phoneRinging, oPuppeteer.earShot[#phoneVol]
        //   if suggestion = #speaking then
        //     cursorOff : endLoop #phoneRinging
        //     if getState(#psionicWavesPresent) = 1 and inState(#hauntsRemaining, #phoneMessage) then
        //       soundEffect #phoneRoxy : wait #soundStop, #phoneRoxy
        //       suggestion = #done : setLoop #roxyCallDone
        //       trimState #hauntsRemaining, #phoneMessage
        //       trimState #hauntsRemaining, #spookyOperator
        //       setState #AMBERVISION, #waitingForPlayer
        //       setState #PKamberStatus, #WaveActivated
        //       setState #PeekDisplay, #amberStatus
        //     else if inState(#hauntsRemaining, #spookyOperator) then
        //       if getState(#phoneButtonsPressed) > 6 then
        //         soundEffect #spookyOperator : wait #soundStop, #spookyOperator
        //         trimState #hauntsRemaining, #spookyOperator
        //         setProp( ..., #phoneButtonsPressed, list(0) )
        //         setLoop #phoneDead
        //     else if inState(#hauntsRemaining, #phoneMessage) then setLoop #phoneDead
        //     else setLoop #roxyCallDone
        //   if suggestion = #done then
        //     cursorOn
        //     endLoop #phoneRinging : endLoop #spookyOperator : endLoop #phoneRoxy
        //   setProp( oStoryteller.states, #ghostlyPhoneCall, list(suggestion) )
        //
        // What the player hears when they lift the receiver depends on where
        // they are in the chapter, and the three answers are the phone being
        // dead, an operator who should not exist, and Roxy's own message. The
        // last of those is the one that matters: it consumes both phone haunts
        // at once and switches the monitor on, so it is the call that moves the
        // chapter forward rather than a piece of atmosphere.
        //
        // The branch rewrites `suggestion` to `#done`, which is why the call
        // ends by itself: the flag written at the end is the rewritten one.
        "setghostlyphonecall" => {
            const VALID: [&str; 4] = ["notyet", "ringingNow", "speaking", "done"];
            let Some(asked) = args
                .first()
                .and_then(Value::as_str)
                .map(|v| v.trim_start_matches('#').to_string())
            else {
                return true;
            };
            let Some(&wanted) = VALID.iter().find(|v| v.eq_ignore_ascii_case(&asked)) else {
                trace!(
                    crate::trace::Topic::Script,
                    "setGhostlyPhoneCall: {asked} is not a call state"
                );
                return true;
            };

            let pending = |st: &State, haunt: &str| {
                st.get_all("hauntsRemaining")
                    .iter()
                    .any(|h| h.as_str().is_some_and(|s| s.eq_ignore_ascii_case(haunt)))
            };
            let stop = |out: &mut Outcome, name: &str| {
                out.effects.push(Effect::StopLoop {
                    name: name.into(),
                    fade: false,
                })
            };
            let loop_at = |out: &mut Outcome, name: &str, volume: Option<i32>| {
                out.effects.push(Effect::StartLoop {
                    name: name.into(),
                    volume,
                })
            };

            let mut settle = wanted;

            if wanted.eq_ignore_ascii_case("ringingNow") {
                // How loud the phone carries from this room, which the room
                // states as part of its own mix.
                let level = state.get("gEarShot_phonevol").as_int();
                loop_at(out, "phoneRinging", level);
            }

            if wanted.eq_ignore_ascii_case("speaking") {
                out.effects.push(Effect::CursorOff);
                stop(out, "phoneRinging");

                let waves = state.get("psionicWavesPresent").as_int().unwrap_or(0) == 1;
                if waves && pending(state, "phoneMessage") {
                    out.effects.push(Effect::PlaySound {
                        name: "phoneRoxy".into(),
                        loudness: None,
                    });
                    out.effects.push(Effect::WaitForSound("phoneRoxy".into()));
                    settle = "done";
                    loop_at(out, "roxyCallDone", None);
                    out.effects.push(Effect::TrimState {
                        key: "hauntsRemaining".into(),
                        item: Value::Symbol("phoneMessage".into()),
                    });
                    out.effects.push(Effect::TrimState {
                        key: "hauntsRemaining".into(),
                        item: Value::Symbol("spookyOperator".into()),
                    });
                    for (key, value) in [
                        ("AMBERVISION", "waitingForPlayer"),
                        ("PKamberStatus", "WaveActivated"),
                        ("PeekDisplay", "amberStatus"),
                    ] {
                        out.effects.push(Effect::SetState {
                            key: key.into(),
                            value: Value::Symbol(value.into()),
                        });
                    }
                } else if pending(state, "spookyOperator") {
                    if state.get("phoneButtonsPressed").as_int().unwrap_or(0) > 6 {
                        out.effects.push(Effect::PlaySound {
                            name: "spookyOperator".into(),
                            loudness: None,
                        });
                        out.effects.push(Effect::WaitForSound("spookyOperator".into()));
                        out.effects.push(Effect::TrimState {
                            key: "hauntsRemaining".into(),
                            item: Value::Symbol("spookyOperator".into()),
                        });
                        state.set_all("phoneButtonsPressed", vec![Value::Int(0)]);
                        loop_at(out, "phoneDead", None);
                    }
                } else if pending(state, "phoneMessage") {
                    loop_at(out, "phoneDead", None);
                } else {
                    loop_at(out, "roxyCallDone", None);
                }
            }

            if settle.eq_ignore_ascii_case("done") {
                for name in ["phoneRinging", "spookyOperator", "phoneRoxy"] {
                    stop(out, name);
                }
            }

            state.set_all("ghostlyPhoneCall", vec![Value::Symbol(settle.into())]);
            out.redraw = true;
        }

        // on camControl whichBtn
        //   storedPosition = getState( #videoTapePosition )
        //   if integerp( storedPosition ) or gHorsepower = #low then
        //     set the movieRate of sprite 44 = 0 : updateStage
        //   buttonStack  = getProp( oPuppeteer.frames, #camButtons )
        //   buttonSprite = the channel in 10..48 showing one of buttonStack
        //   markerList   = [44, 2152, 4432, 7898, 12474, 14984]
        //   ... light the pressed button for 8 ticks, then unlight it ...
        //   currentPosition = the movieTime of sprite 44
        //   currentSegment  = the last marker at or before currentPosition
        //   if whichBtn = #prevMarker or whichBtn = #nextMarker then
        //     if #prevMarker and currentSegment > 1              then currentSegment = currentSegment - 1
        //     if #nextMarker and currentSegment < count(markerList) then currentSegment = currentSegment + 1
        //     newPosition  = getAt( markerList, currentSegment )
        //     trueLength   = abs( the movieTime of sprite 44 - newPosition ) / 20
        //     rewindLength = min( trueLength, 300 )
        //     ... swap in the rewind static on 45 for rewindLength ticks ...
        //   if whichBtn = #pause then set the movieRate of sprite 44 = 0
        //   if whichBtn = #play then
        //     if the movieTime of sprite 44 > 15000 then set the movieTime of sprite 44 = 15000
        //     set the movieRate of sprite 44 = 1
        //
        // The security tape, and a real VCR: six markers, a shuttle between
        // them whose length is the distance travelled over twenty and capped
        // at three hundred ticks, and a play button that clamps to the end of
        // the tape at 15000. That clamp is where the tape's length comes from
        // -- it is not written down anywhere else.
        //
        // Not modelled: the pressed button lights for eight ticks and the
        // rewind static is a second film swapped onto channel 45. The eight
        // ticks are kept because they are a beat the player feels; the static
        // needs a sprite the original finds by scanning channels 10 to 48 for
        // whichever one is showing a `#camButtons` cast, and this engine
        // resolves those from state instead.
        "camcontrol" => {
            const MARKERS: [i32; 6] = [44, 2152, 4432, 7898, 12474, 14984];
            const TAPE_END: i32 = 15000;

            let Some(button) = args
                .first()
                .and_then(Value::as_str)
                .map(|b| b.trim_start_matches('#').to_ascii_lowercase())
            else {
                return true;
            };

            let position = state.get("videoTapePosition").as_int().unwrap_or(MARKERS[0]);
            // The button stays lit for eight ticks whichever one it is.
            out.effects.push(Effect::WaitTicks(8));

            match button.as_str() {
                "prevmarker" | "nextmarker" => {
                    // The segment is the last marker at or before where the
                    // tape is sitting, counting from one.
                    let mut segment = MARKERS
                        .iter()
                        .rposition(|m| *m <= position)
                        .map_or(0, |i| i as i32);
                    if button == "prevmarker" && segment > 0 {
                        segment -= 1;
                    } else if button == "nextmarker" && segment < MARKERS.len() as i32 - 1 {
                        segment += 1;
                    }
                    let moved = MARKERS[segment as usize];
                    let shuttle = ((moved - position).abs() / 20).min(300);
                    state.set_all("videoTapePosition", vec![Value::Int(moved)]);
                    if shuttle > 0 {
                        out.effects.push(Effect::WaitTicks(shuttle as u32));
                    }
                    out.effects.push(Effect::PlayVideoSegment {
                        from: moved as u32,
                        to: moved as u32,
                    });
                }
                "play" => {
                    let from = position.min(TAPE_END);
                    state.set_all("videoTapePosition", vec![Value::Int(from)]);
                    out.effects.push(Effect::PlayVideoSegment {
                        from: from as u32,
                        to: TAPE_END as u32,
                    });
                }
                "pause" => out.effects.push(Effect::StopVideo),
                _ => return true,
            }
            out.redraw = true;
        }

        // on camLogInit
        //   killVideo
        //   disablePeekAlert
        //   if getState( #playerHasVideotape ) = #usedUp then ...
        //   puppetSprite 45, 1 : updateStage
        //   set the castNum of sprite 45 = getProp( oPuppeteer.frames, #camRewind )
        //   set the visible of sprite 45 = 0
        //
        // Sitting down at the monitor turns the peek alert off, so the ghost
        // cannot interrupt while the player is working through the tape.
        "camloginit" => {
            out.effects.push(Effect::StopVideo);
            call("disablepeekalert", &[], state, out);
            out.effects.push(Effect::PuppetSprite { channel: 45, on: true });
            out.effects.push(Effect::SpriteCastNamed {
                channel: 45,
                name: "camRewind".into(),
            });
            out.effects.push(Effect::SpriteVisible { channel: 45, visible: false });
            out.redraw = true;
        }

        // on camLogShutdown
        //   pushVideo
        //   enablePeekAlert
        //   storedPosition = getState( #videoTapePosition )
        //   if storedPosition <> #None then
        //     setState( #videoTapePosition, the movieTime of sprite 44 )
        //   set the movieRate of sprite 44 = 0
        //   puppetSprite 44, 0
        //
        // And standing up turns it back on, remembering where the tape was
        // left -- but only if it had been started. A tape never played keeps
        // its `#None` rather than being pinned to the beginning.
        // The original stops the tape and starts the room's film, which are
        // two movies on two channels. This engine has one player, so stopping
        // the tape *is* starting the room's film and a `StopVideo` after the
        // `PlayVideo` would only cancel it.
        //
        // The position is not written back here either. The original reads the
        // live movieTime off the sprite; this port tracks it in
        // `#videoTapePosition` as `camControl` moves it, which agrees except
        // while the tape is rolling -- leave mid-play and it remembers where
        // play began rather than where it had got to.
        "camlogshutdown" => {
            call("enablepeekalert", &[], state, out);
            out.effects.push(Effect::PuppetSprite { channel: 44, on: false });
            out.effects.push(Effect::PlayVideo(None));
            out.redraw = true;
        }

        // on setFragmentBias option
        //   checkFrames = getProp( oPuppeteer.frames, #BT_checkBox )
        //   bias1Frames..bias3Frames likewise
        //   currentState = getState( #BT_bias )
        //   checkSprite = 22 : bias1Sprite = 23 : bias2Sprite = 24 : bias3Sprite = 25
        //   if option = #toggle then
        //     if currentState = #off then
        //       newState = getState( #BT_storedBias )
        //       setState( #BT_bias, newState )
        //       ... four sprites to their #on cast
        //     else
        //       setState( #BT_bias, #off )
        //       ... four sprites to their #off cast
        //   else
        //     if currentState <> #off then
        //       ... step through list( 1, 2, 3, #None )
        //       setState( #BT_bias, next ) : setState( #BT_storedBias, next )
        //
        // The check box remembers: turning the section off parks the value in
        // #BT_storedBias and turning it back on restores it, so a player who
        // switches the bias off mid-thought does not lose their place. The
        // schema ships #BT_storedBias as [2, 1, 3, #None], already holding two.
        //
        // No sprite effects here. #BT_checkBox is keyed [#on, 1, 2, 3, #off]
        // and #BT_bias1..3 are keyed by the bias value itself, so writing the
        // flag and asking for a redraw puts every one of the four sprites on
        // the cast the original set by hand. That one table answering to both
        // #on and a number is why the same check box art serves this section
        // and the alignment section above it, which key on different flags.
        "setfragmentbias" => {
            let cycle = [
                Value::Int(1),
                Value::Int(2),
                Value::Int(3),
                Value::Symbol("None".into()),
            ];
            let toggle = args
                .first()
                .and_then(Value::as_str)
                .is_some_and(|o| o.trim_start_matches('#').eq_ignore_ascii_case("toggle"));
            let current = state.get("BT_bias");
            let off = current.as_symbol() == Some("off");

            if toggle {
                if off {
                    let restored = state.get("BT_storedBias");
                    state.set("BT_bias", restored);
                } else {
                    state.set("BT_bias", Value::Symbol("off".into()));
                }
            } else if !off {
                // The bias only steps while its section is switched on.
                let at = cycle.iter().position(|v| *v == current).unwrap_or(0);
                let next = cycle[(at + 1) % cycle.len()].clone();
                state.set("BT_bias", next.clone());
                state.set("BT_storedBias", next);
            }
            out.redraw = true;
        }

        // on setFragmentAlignment option
        //   currentPsionOrder = getProp( oStoryteller.states, #BT_psionOrder )
        //   currentState = getState( #BT_alignmentLeft )
        //   if currentState <> #off then currentState = #on
        //   if option = #toggle then
        //     if currentState = #on then
        //       setProp( states, #BT_alignmentLeft,  list(#off) )
        //       setProp( states, #BT_alignmentRight, list(#off) )
        //     else
        //       ... both to list(#on)
        //     ... sprites 19, 20, 21 to the matching cast
        //   else
        //     if currentState <> #on then return
        //     if option = #clockwise then
        //       newFirst = getAt( currentPsionOrder, 3 )
        //       deleteAt( currentPsionOrder, 3 ) : addAt( currentPsionOrder, 1, newFirst )
        //       goTo #clockwise : castCursor : killVideo
        //     if option = #counter then
        //       newLast = getAt( currentPsionOrder, 1 )
        //       deleteAt( currentPsionOrder, 1 ) : addAt( currentPsionOrder, 3, newLast )
        //       goTo #counter : castCursor : killVideo
        //
        // The alignment control spins the three psions rather than setting a
        // number: clockwise carries the last to the front and counter carries
        // the first to the back, both on #BT_psionOrder, which the schema
        // ships as [1, 2, 3]. This is the one place a flag's whole list is the
        // value and not a history, which is why it is written with set_all.
        //
        // Both check boxes move together -- the left one is read for the state
        // and the right one only ever follows it -- and neither spins while
        // the section is off.
        "setfragmentalignment" => {
            let option = args
                .first()
                .and_then(Value::as_str)
                .map(|o| o.trim_start_matches('#').to_ascii_lowercase())
                .unwrap_or_default();
            let on = state.get("BT_alignmentLeft").as_symbol() != Some("off");

            if option == "toggle" {
                let now = Value::Symbol(if on { "off" } else { "on" }.into());
                state.set_all("BT_alignmentLeft", vec![now.clone()]);
                state.set_all("BT_alignmentRight", vec![now]);
                out.redraw = true;
                return true;
            }
            if !on {
                return true;
            }
            let mut order = state.get_all("BT_psionOrder").to_vec();
            if order.len() == 3 {
                match option.as_str() {
                    "clockwise" => order.rotate_right(1),
                    "counter" => order.rotate_left(1),
                    _ => return true,
                }
                state.set_all("BT_psionOrder", order);
                out.destination = Some(option);
                out.effects.push(Effect::StopVideo);
                out.redraw = true;
            }
        }

        // on adjustAlgorithm whichColumn, upOrDown
        //   cursorOff
        //   columnStack = getProp( oPuppeteer.frames, #BT_algo<Column> )
        //   currentSetting = getState( #BT_algorithm<Column> )
        //   if upOrDown = #down then newSetting = currentSetting - 1
        //   if upOrDown = #up   then newSetting = currentSetting + 1
        //   if newSetting < 1 or newSetting > 8 then
        //     soundEffect #algorithmNotAvail
        //   else
        //     setProp( oStoryteller.states, #BT_algorithm<Column>, list(newSetting) )
        //     set the castNum of the column's sprite from columnStack[newSetting]
        //     lagTime = lagTime + 40 ... repeat while stillDown
        //   if getState(#BT_algorithmLeft)   <> 5 then return
        //   if getState(#BT_algorithmMiddle) <> 2 then return
        //   if getState(#BT_algorithmRight)  <> 8 then return
        //   cursorOff : wait 60 : soundEffect #happyBeep
        //   pushVideo : wait #videoStop
        //   setState( #BT_fragStatus, #allDone )
        //   setState( #endGame, 1 )
        //
        // The three columns of the psionic bar, each a digit from one to
        // eight, and setting them to five, two and eight is how the game ends.
        // The schema starts them at two, three and five, so none of the three
        // begins on its answer.
        //
        // A column at its limit refuses with a sound rather than wrapping,
        // which is the opposite of the lock in Brice's chapter -- those wheels
        // wrap through zero. Worth not assuming one from the other.
        "adjustalgorithm" => {
            const COLUMNS: [(&str, &str); 3] = [
                ("left", "BT_algorithmLeft"),
                ("middle", "BT_algorithmMiddle"),
                ("right", "BT_algorithmRight"),
            ];
            const ANSWER: [(&str, i32); 3] = [
                ("BT_algorithmLeft", 5),
                ("BT_algorithmMiddle", 2),
                ("BT_algorithmRight", 8),
            ];

            let Some(column) = args.first().and_then(Value::as_str).and_then(|c| {
                let c = c.trim_start_matches('#');
                COLUMNS
                    .iter()
                    .find(|(name, _)| name.eq_ignore_ascii_case(c))
                    .map(|(_, flag)| *flag)
            }) else {
                return true;
            };
            let up = args
                .get(1)
                .and_then(Value::as_str)
                .is_some_and(|d| d.trim_start_matches('#').eq_ignore_ascii_case("up"));

            let current = state.get(column).as_int().unwrap_or(0);
            let wanted = current + if up { 1 } else { -1 };
            if !(1..=8).contains(&wanted) {
                out.effects.push(Effect::PlaySound {
                    name: "algorithmNotAvail".into(),
                    loudness: None,
                });
                return true;
            }
            state.set_all(column, vec![Value::Int(wanted)]);
            out.redraw = true;
            // A click moves one step and a hold runs the column up or down.
            out.repeat_while_held = true;

            if ANSWER
                .iter()
                .all(|(flag, want)| state.get(flag).as_int() == Some(*want))
            {
                out.effects.push(Effect::CursorOff);
                out.effects.push(Effect::WaitTicks(60));
                out.effects.push(Effect::PlaySound {
                    name: "happyBeep".into(),
                    loudness: None,
                });
                out.effects.push(Effect::PlayVideo(None));
                out.effects.push(Effect::WaitForVideo);
                out.effects.push(Effect::SetState {
                    key: "BT_fragStatus".into(),
                    value: Value::Symbol("allDone".into()),
                });
                out.effects.push(Effect::SetState {
                    key: "endGame".into(),
                    value: Value::Int(1),
                });
            }
        }

        // on setScanTime howManyMinutes
        //   gScanFinish = the ticks + howManyMinutes * 3600
        //   setState(oStoryteller, #PeekDisplay, ...)
        //   setState(oStoryteller, #PKscanStatus, ...)
        //   goBack
        //
        // Starts the scan unit running for a number of minutes and steps back
        // out of the close-up. Ticks are sixtieths, so 3600 of them is the
        // minute. The two display strings are built from the argument and are
        // text this port does not render, so only the timer and the state are
        // carried over.
        "setscantime" => {
            let minutes = args.first().and_then(Value::as_int).unwrap_or(0).max(0);
            let finish = state.get("gTicks").as_int().unwrap_or(0) + minutes * 3600;
            state.set("gScanFinish", Value::Int(finish));
            // The two strings build symbols from the argument: `#goodScan5min`
            // and `#Wait5min`. I had written `#Scanning` here, which is not one
            // of the twelve statuses the unit accepts, so `setPKScanStatus`
            // would have refused it outright once that handler existed.
            state.set(
                "PeekDisplay",
                Value::Symbol(format!("goodScan{minutes}min")),
            );
            call(
                "setpkscanstatus",
                &[Value::Symbol(format!("Wait{minutes}min"))],
                state,
                out,
            );
            out.go_back = true;
        }

        // on assertEdwinGhost
        //   if not inState(#hauntsRemaining, #lakeGhost2) then exit
        //   if playerHasCrowbar = 0 or inState(#hauntsRemaining, #lakeGhost) = 0
        //     then exit
        //   if gCPU = #PC then suspendSounds #fadeOut
        //   pushVideo
        //   wait #videoStop
        //   if gCPU = #PC then restoreSounds #fadeIn
        //   trimState #hauntsRemaining, #lakeGhost2
        //
        // The second lake ghost, which only appears while the player is
        // carrying the crowbar and the first has not been seen yet. That is
        // the exact complement of the guard on the room's own lakegst2 sprite,
        // which shows when either of those does not hold: between them the two
        // paths cover every case and never both fire.
        "assertedwinghost" => {
            let pending = |key: &str, item: &str| {
                state
                    .get_all(key)
                    .iter()
                    .any(|i| i.as_str().is_some_and(|s| s.eq_ignore_ascii_case(item)))
            };
            let carrying = state.get("playerHasCrowbar").as_int().unwrap_or(0) != 0;
            if !pending("hauntsRemaining", "lakeGhost2")
                || !carrying
                || !pending("hauntsRemaining", "lakeGhost")
            {
                return true;
            }
            out.effects.push(Effect::SuspendSounds { fade: true });
            out.effects.push(Effect::PlayVideo(None));
            out.effects.push(Effect::WaitForVideo);
            out.effects.push(Effect::RestoreSounds { fade: true });
            // Queued, so the haunt is still in the pool while its movie plays.
            out.effects.push(Effect::TrimState {
                key: "hauntsRemaining".into(),
                item: Value::Symbol("lakeGhost2".into()),
            });
        }

        _ => return false,
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn door(zone: &str, handler: &str, to: i32, from: i32) -> (State, Outcome) {
        let mut state = State::new();
        state.set_all("gChapter", vec![Value::Symbol("ROXY".into())]);
        state.set_all("gZone", vec![Value::Symbol(zone.into())]);
        state.set_all(&handler[3..], vec![Value::Int(from)]);
        let mut out = Outcome::default();
        assert!(bleed_door(handler, &[Value::Int(to)], &mut state, &mut out));
        (state, out)
    }

    fn loops(out: &Outcome) -> Vec<(String, bool, Option<i32>)> {
        out.effects
            .iter()
            .filter_map(|e| match e {
                Effect::StartLoop { name, volume } => Some((name.clone(), true, *volume)),
                Effect::StopLoop { name, .. } => Some((name.clone(), false, None)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn an_open_door_lets_the_grounds_into_the_hall() {
        let (_, out) = door("Hall", "setfrontdoorisopen", 1, 0);
        assert_eq!(loops(&out), [("grounds".to_string(), true, None)]);
    }

    #[test]
    fn and_lets_the_house_out_onto_the_porch() {
        let (_, out) = door("Porch", "setfrontdoorisopen", 1, 0);
        assert_eq!(loops(&out), [("houseHum".to_string(), true, Some(80))]);
    }

    #[test]
    fn shutting_it_again_ends_that_loop() {
        let (state, out) = door("Porch", "setfrontdoorisopen", 0, 1);
        assert_eq!(loops(&out), [("houseHum".to_string(), false, None)]);
        assert_eq!(state.get_all("frontdoorisopen"), &[Value::Int(0)]);
    }

    #[test]
    fn standing_somewhere_it_cannot_be_heard_the_door_is_only_a_sound() {
        // The office is nowhere near the front door. The cue still plays --
        // this is a setter, not a room -- but nothing about the ambience
        // changes.
        let (_, out) = door("office", "setfrontdoorisopen", 1, 0);
        assert!(loops(&out).is_empty());
        assert!(matches!(out.effects.as_slice(), [Effect::PlaySound { .. }]));
    }

    #[test]
    fn a_door_already_open_does_nothing_at_all() {
        let (_, out) = door("Hall", "setfrontdoorisopen", 1, 1);
        assert!(out.effects.is_empty());
        assert!(!out.redraw);
    }

    #[test]
    fn the_scanner_is_heard_through_the_kitchen_door_only_when_it_is_mounted_there() {
        // rooms: #kitchen, and the unit has to be on that door and switched on.
        let mut state = State::new();
        state.set_all("gChapter", vec![Value::Symbol("ROXY".into())]);
        state.set_all("gZone", vec![Value::Symbol("kitchen".into())]);
        state.set_all("kitchenReardoorisopen", vec![Value::Int(0)]);
        let mut out = Outcome::default();
        bleed_door("setkitchenreardoorisopen", &[Value::Int(1)], &mut state, &mut out);
        assert!(
            !loops(&out).iter().any(|(n, _, _)| n == "scanLoop"),
            "no scanner mounted, so no scanner heard"
        );

        let mut state = State::new();
        state.set_all("gChapter", vec![Value::Symbol("ROXY".into())]);
        state.set_all("gZone", vec![Value::Symbol("kitchen".into())]);
        state.set_all("kitchenReardoorisopen", vec![Value::Int(0)]);
        state.set_all("DoorWithScanUnit", vec![Value::Symbol("kitchenOutside".into())]);
        state.set_all("scanUnitIsActive", vec![Value::Int(1)]);
        let mut out = Outcome::default();
        bleed_door("setkitchenreardoorisopen", &[Value::Int(1)], &mut state, &mut out);
        assert!(loops(&out)
            .iter()
            .any(|(n, on, v)| n == "scanLoop" && *on && *v == Some(120)));
    }

    #[test]
    fn margaret_has_her_own_doors_and_these_rules_are_not_them() {
        let mut state = State::new();
        state.set_all("gChapter", vec![Value::Symbol("MARGARET".into())]);
        state.set_all("gZone", vec![Value::Symbol("Hall".into())]);
        let mut out = Outcome::default();
        assert!(!bleed_door(
            "setbalconydoorisopen",
            &[Value::Int(1)],
            &mut state,
            &mut out
        ));
    }
}

#[cfg(test)]
mod scan_tests {
    use super::*;

    fn run(handler: &str, arg: &str, setup: &[(&str, &str)]) -> (State, Outcome) {
        let mut state = State::new();
        state.set_all("gChapter", vec![Value::Symbol("ROXY".into())]);
        for (k, v) in setup {
            state.set_all(k, vec![Value::Symbol((*v).into())]);
        }
        let mut out = Outcome::default();
        assert!(call(handler, &[Value::Symbol(arg.into())], &mut state, &mut out));
        (state, out)
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

    #[test]
    fn clipping_the_unit_onto_a_knob_sounds_once() {
        let (state, out) = run(
            "setdoorwithscanunit",
            "kitchenOutside",
            &[("DoorWithScanUnit", "None")],
        );
        assert_eq!(sounds(&out), ["scanOntoKnob"]);
        assert!(state
            .get("DoorWithScanUnit")
            .as_str()
            .is_some_and(|k| k == "kitchenOutside"));
    }

    #[test]
    fn taking_it_off_sounds_the_other_way() {
        let (_, out) = run(
            "setdoorwithscanunit",
            "None",
            &[("DoorWithScanUnit", "kitchenOutside")],
        );
        assert_eq!(sounds(&out), ["scanOffKnob"]);
    }

    #[test]
    fn moving_it_between_doors_is_one_click_and_makes_no_sound() {
        // Both cues are guarded on crossing to or from #None, so a move from
        // one knob straight to another is silent.
        let (state, out) = run(
            "setdoorwithscanunit",
            "bathroomInside",
            &[("DoorWithScanUnit", "kitchenOutside")],
        );
        assert!(sounds(&out).is_empty());
        assert!(state
            .get("DoorWithScanUnit")
            .as_str()
            .is_some_and(|k| k == "bathroomInside"));
    }

    #[test]
    fn somewhere_that_is_not_a_knob_is_refused() {
        let (state, _) = run(
            "setdoorwithscanunit",
            "theCeiling",
            &[("DoorWithScanUnit", "None")],
        );
        assert!(state
            .get("DoorWithScanUnit")
            .as_str()
            .is_some_and(|k| k == "None"));
    }

    #[test]
    fn going_online_during_a_countdown_interrupts_that_scan() {
        let (state, _) = run("setpkscanstatus", "Online", &[("PKscanStatus", "Wait3min")]);
        assert!(state
            .get("PKscanStatus")
            .as_str()
            .is_some_and(|s| s == "Interrupted"));
        assert_eq!(state.get("gScanFinish").as_int(), Some(0));
    }

    #[test]
    fn a_finished_scan_survives_being_fiddled_with() {
        // Asking to go online while a result is waiting keeps the result.
        // Losing a finished scan by touching the unit is the one thing that
        // would make the puzzle unfair.
        let (state, _) = run(
            "setpkscanstatus",
            "Online",
            &[("PKscanStatus", "ReadyForPlayback")],
        );
        assert!(state
            .get("PKscanStatus")
            .as_str()
            .is_some_and(|s| s == "ReadyForPlayback"));
    }

    #[test]
    fn an_unknown_status_leaves_the_unit_alone() {
        let (state, _) = run("setpkscanstatus", "Bananas", &[("PKscanStatus", "Offline")]);
        assert!(state
            .get("PKscanStatus")
            .as_str()
            .is_some_and(|s| s == "Offline"));
    }

    #[test]
    fn going_offline_clears_the_deadline() {
        let mut state = State::new();
        state.set_all("gChapter", vec![Value::Symbol("ROXY".into())]);
        state.set("gScanFinish", Value::Int(99_000));
        state.set_all("PKscanStatus", vec![Value::Symbol("Wait2min".into())]);
        let mut out = Outcome::default();
        call(
            "setpkscanstatus",
            &[Value::Symbol("Offline".into())],
            &mut state,
            &mut out,
        );
        assert_eq!(state.get("gScanFinish").as_int(), Some(0));
    }

    #[test]
    fn setting_a_scan_time_asks_for_a_status_the_unit_accepts() {
        // `#Scanning` is not one of the twelve, and the setter refuses it.
        let mut state = State::new();
        state.set_all("gChapter", vec![Value::Symbol("ROXY".into())]);
        let mut out = Outcome::default();
        call("setscantime", &[Value::Int(5)], &mut state, &mut out);
        assert!(state
            .get("PKscanStatus")
            .as_str()
            .is_some_and(|s| s == "Wait5min"));
        assert!(state
            .get("PeekDisplay")
            .as_str()
            .is_some_and(|s| s == "goodScan5min"));
        assert!(out.go_back);
    }
}

#[cfg(test)]
mod phone_tests {
    use super::*;

    fn phone(haunts: &[&str]) -> State {
        let mut state = State::new();
        state.set_all("gChapter", vec![Value::Symbol("ROXY".into())]);
        state.set_all(
            "hauntsRemaining",
            haunts.iter().map(|h| Value::Symbol((*h).into())).collect(),
        );
        state.set_all("ghostlyPhoneCall", vec![Value::Symbol("notyet".into())]);
        state
    }

    fn run(state: &mut State, handler: &str, args: &[Value]) -> Outcome {
        let mut out = Outcome::default();
        assert!(call(handler, args, state, &mut out), "{handler} unhandled");
        out
    }

    fn sounds(out: &Outcome) -> Vec<String> {
        out.effects
            .iter()
            .filter_map(|e| match e {
                Effect::PlaySound { name, .. } => Some(name.clone()),
                Effect::StartLoop { name, .. } => Some(format!("loop {name}")),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_operator_answers_only_after_the_buttons_and_hanging_up() {
        // Lifting the receiver alone gives nothing: the count is zero and the
        // speaking branch tests for more than six.
        let mut state = phone(&["spookyOperator", "phoneMessage"]);
        let out = run(&mut state, "setghostlyphonecall", &[Value::Symbol("speaking".into())]);
        assert!(!sounds(&out).iter().any(|s| s.contains("spookyOperator")));

        // Pressing at all and then hanging up forces the count past it.
        state.set_all("phoneButtonsPressed", vec![Value::Int(2)]);
        let out = run(&mut state, "putdownthephone", &[]);
        assert!(sounds(&out).iter().any(|s| s == "spookyOperator"));
        assert_eq!(state.get("phoneButtonsPressed").as_int(), Some(0));
    }

    #[test]
    fn the_operator_is_used_up_once_heard() {
        let mut state = phone(&["spookyOperator", "phoneMessage"]);
        state.set_all("phoneButtonsPressed", vec![Value::Int(2)]);
        let out = run(&mut state, "putdownthephone", &[]);
        assert!(out.effects.iter().any(|e| matches!(
            e,
            Effect::TrimState { key, item }
                if key == "hauntsRemaining"
                    && item.as_str() == Some("spookyOperator")
        )));
    }

    #[test]
    fn roxys_message_needs_the_waves_and_ends_the_call_itself() {
        // The branch rewrites its own argument to #done, which is why the
        // call hangs up on its own rather than waiting to be put down.
        let mut state = phone(&["phoneMessage", "spookyOperator"]);
        state.set("psionicWavesPresent", Value::Int(1));
        let out = run(&mut state, "setghostlyphonecall", &[Value::Symbol("speaking".into())]);
        assert!(sounds(&out).iter().any(|s| s == "phoneRoxy"));
        assert!(state
            .get("ghostlyPhoneCall")
            .as_str()
            .is_some_and(|s| s == "done"));
    }

    #[test]
    fn roxys_message_switches_the_monitor_on() {
        // This is the call that moves the chapter forward rather than being
        // atmosphere: it consumes both phone haunts and arms the monitor.
        let mut state = phone(&["phoneMessage", "spookyOperator"]);
        state.set("psionicWavesPresent", Value::Int(1));
        let out = run(&mut state, "setghostlyphonecall", &[Value::Symbol("speaking".into())]);
        let written: Vec<&str> = out
            .effects
            .iter()
            .filter_map(|e| match e {
                Effect::SetState { key, .. } => Some(key.as_str()),
                _ => None,
            })
            .collect();
        assert!(written.contains(&"AMBERVISION"));
        assert!(written.contains(&"PKamberStatus"));
    }

    #[test]
    fn without_the_waves_a_pending_message_leaves_the_line_dead() {
        let mut state = phone(&["phoneMessage"]);
        let out = run(&mut state, "setghostlyphonecall", &[Value::Symbol("speaking".into())]);
        assert!(sounds(&out).iter().any(|s| s == "loop phoneDead"));
    }

    #[test]
    fn with_nothing_left_to_hear_the_call_is_over() {
        let mut state = phone(&[]);
        let out = run(&mut state, "setghostlyphonecall", &[Value::Symbol("speaking".into())]);
        assert!(sounds(&out).iter().any(|s| s == "loop roxyCallDone"));
    }

    #[test]
    fn a_state_the_call_does_not_have_is_refused() {
        let mut state = phone(&[]);
        run(&mut state, "setghostlyphonecall", &[Value::Symbol("dialling".into())]);
        assert!(state
            .get("ghostlyPhoneCall")
            .as_str()
            .is_some_and(|s| s == "notyet"));
    }

    #[test]
    fn lifting_the_receiver_stops_it_ringing_again() {
        let mut state = phone(&[]);
        run(&mut state, "setplayerisexaminingphone", &[Value::Int(1)]);
        assert!(state
            .get("ghostlyPhoneCall")
            .as_str()
            .is_some_and(|s| s == "dontRingPlease"));
    }

    // -- the psionic bar ----------------------------------------------------

    /// The bar as the schema ships it: the columns on two, three and five, the
    /// bias off with two remembered, the alignment off, the psions in order.
    fn bar() -> State {
        let mut s = State::new();
        s.set_all("gChapter", vec![Value::Symbol("ROXY".into())]);
        s.set_all("BT_algorithmLeft", vec![Value::Int(2)]);
        s.set_all("BT_algorithmMiddle", vec![Value::Int(3)]);
        s.set_all("BT_algorithmRight", vec![Value::Int(5)]);
        s.set_all(
            "BT_bias",
            vec![
                Value::Symbol("off".into()),
                Value::Int(1),
                Value::Int(2),
                Value::Int(3),
                Value::Symbol("None".into()),
            ],
        );
        s.set_all(
            "BT_storedBias",
            vec![
                Value::Int(2),
                Value::Int(1),
                Value::Int(3),
                Value::Symbol("None".into()),
            ],
        );
        s.set_all("BT_alignmentLeft", vec![Value::Symbol("off".into())]);
        s.set_all("BT_alignmentRight", vec![Value::Symbol("off".into())]);
        s.set_all(
            "BT_psionOrder",
            vec![Value::Int(1), Value::Int(2), Value::Int(3)],
        );
        s
    }

    fn step(state: &mut State, column: &str, up: bool) -> Outcome {
        let mut out = Outcome::default();
        let dir = if up { "up" } else { "down" };
        assert!(call(
            "adjustalgorithm",
            &[
                Value::Symbol(column.into()),
                Value::Symbol(dir.into()),
            ],
            state,
            &mut out,
        ));
        out
    }

    fn drive(state: &mut State, column: &str, to: i32) -> Outcome {
        let flag = match column {
            "left" => "BT_algorithmLeft",
            "middle" => "BT_algorithmMiddle",
            _ => "BT_algorithmRight",
        };
        let mut last = Outcome::default();
        for _ in 0..16 {
            let at = state.get(flag).as_int().unwrap_or(0);
            if at == to {
                return last;
            }
            last = step(state, column, at < to);
        }
        panic!("{column} never reached {to}");
    }

    /// The flags a handler writes through the effect list rather than
    /// directly, which is where anything that has to land after a wait goes.
    fn written(out: &Outcome) -> Vec<(String, Value)> {
        out.effects
            .iter()
            .filter_map(|e| match e {
                Effect::SetState { key, value } => Some((key.clone(), value.clone())),
                _ => None,
            })
            .collect()
    }

    // #endGame and #BT_fragStatus are written through the effect list, not
    // straight onto state, because the original sets them after `wait
    // #videoStop` -- the ending has to have played before the game is over.
    #[test]
    fn five_two_eight_ends_the_game() {
        let mut s = bar();
        // Nothing has ended on the way: the last column is still on its five.
        let out = drive(&mut s, "left", 5);
        assert!(written(&out).is_empty());
        drive(&mut s, "middle", 2);
        let out = drive(&mut s, "right", 8);
        assert_eq!(
            written(&out),
            [
                ("BT_fragStatus".to_string(), Value::Symbol("allDone".into())),
                ("endGame".to_string(), Value::Int(1)),
            ]
        );
        assert!(out.effects.iter().any(|e| matches!(e, Effect::PlayVideo(_))));
    }

    #[test]
    fn no_other_combination_does() {
        let mut s = bar();
        drive(&mut s, "left", 5);
        drive(&mut s, "middle", 2);
        let out = drive(&mut s, "right", 7);
        assert!(written(&out).is_empty());
    }

    #[test]
    fn a_column_at_its_limit_refuses_rather_than_wrapping() {
        // The lock in Brice's chapter wraps through zero; this does not.
        let mut s = bar();
        drive(&mut s, "left", 1);
        let out = step(&mut s, "left", false);
        assert_eq!(s.get("BT_algorithmLeft"), Value::Int(1));
        assert_eq!(sounds(&out), ["algorithmNotAvail"]);

        drive(&mut s, "left", 8);
        let out = step(&mut s, "left", true);
        assert_eq!(s.get("BT_algorithmLeft"), Value::Int(8));
        assert_eq!(sounds(&out), ["algorithmNotAvail"]);
    }

    fn bias(state: &mut State, option: &str) {
        let mut out = Outcome::default();
        assert!(call(
            "setfragmentbias",
            &[Value::Symbol(option.into())],
            state,
            &mut out,
        ));
    }

    #[test]
    fn the_check_box_gives_back_the_bias_it_was_switched_off_on() {
        let mut s = bar();
        // Switching the section on restores the remembered two, so a single
        // step from there reaches three.
        bias(&mut s, "toggle");
        assert_eq!(s.get("BT_bias"), Value::Int(2));
        bias(&mut s, "step");
        assert_eq!(s.get("BT_bias"), Value::Int(3));
        bias(&mut s, "toggle");
        assert_eq!(s.get("BT_bias"), Value::Symbol("off".into()));
        bias(&mut s, "toggle");
        assert_eq!(s.get("BT_bias"), Value::Int(3));
    }

    #[test]
    fn the_bias_will_not_step_while_its_section_is_off() {
        let mut s = bar();
        bias(&mut s, "step");
        assert_eq!(s.get("BT_bias"), Value::Symbol("off".into()));
    }

    #[test]
    fn the_bias_cycles_back_round_through_none() {
        let mut s = bar();
        bias(&mut s, "toggle");
        let seen: Vec<Value> = (0..5)
            .map(|_| {
                bias(&mut s, "step");
                s.get("BT_bias")
            })
            .collect();
        assert_eq!(
            seen,
            [
                Value::Int(3),
                Value::Symbol("None".into()),
                Value::Int(1),
                Value::Int(2),
                Value::Int(3),
            ]
        );
    }

    fn align(state: &mut State, option: &str) -> Outcome {
        let mut out = Outcome::default();
        assert!(call(
            "setfragmentalignment",
            &[Value::Symbol(option.into())],
            state,
            &mut out,
        ));
        out
    }

    fn order(state: &State) -> Vec<i32> {
        state
            .get_all("BT_psionOrder")
            .iter()
            .filter_map(Value::as_int)
            .collect()
    }

    #[test]
    fn spinning_the_psions_is_a_rotation_not_a_swap() {
        let mut s = bar();
        align(&mut s, "toggle");
        align(&mut s, "clockwise");
        assert_eq!(order(&s), [3, 1, 2]);
        align(&mut s, "clockwise");
        assert_eq!(order(&s), [2, 3, 1]);
        align(&mut s, "counter");
        assert_eq!(order(&s), [3, 1, 2]);
    }

    #[test]
    fn three_turns_the_same_way_come_back_round() {
        let mut s = bar();
        align(&mut s, "toggle");
        for _ in 0..3 {
            align(&mut s, "counter");
        }
        assert_eq!(order(&s), [1, 2, 3]);
    }

    #[test]
    fn the_psions_will_not_spin_while_the_alignment_is_off() {
        let mut s = bar();
        align(&mut s, "clockwise");
        assert_eq!(order(&s), [1, 2, 3]);
    }

    #[test]
    fn both_alignment_boxes_move_together() {
        let mut s = bar();
        align(&mut s, "toggle");
        assert_eq!(s.get("BT_alignmentLeft"), Value::Symbol("on".into()));
        assert_eq!(s.get("BT_alignmentRight"), Value::Symbol("on".into()));
    }


    // -- the security tape --------------------------------------------------

    fn tape(at: i32) -> State {
        let mut s = State::new();
        s.set_all("gChapter", vec![Value::Symbol("ROXY".into())]);
        s.set_all("videoTapePosition", vec![Value::Int(at)]);
        s
    }

    fn press(state: &mut State, button: &str) -> Outcome {
        let mut out = Outcome::default();
        assert!(call(
            "camcontrol",
            &[Value::Symbol(button.into())],
            state,
            &mut out
        ));
        out
    }

    fn at(state: &State) -> i32 {
        state.get("videoTapePosition").as_int().unwrap_or(-1)
    }

    fn waits(out: &Outcome) -> Vec<u32> {
        out.effects
            .iter()
            .filter_map(|e| match e {
                Effect::WaitTicks(t) => Some(*t),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_tape_steps_between_its_six_markers() {
        let mut s = tape(44);
        for want in [2152, 4432, 7898, 12474, 14984] {
            press(&mut s, "nextMarker");
            assert_eq!(at(&s), want);
        }
        for want in [12474, 7898, 4432, 2152, 44] {
            press(&mut s, "prevMarker");
            assert_eq!(at(&s), want);
        }
    }

    #[test]
    fn and_stops_at_either_end_of_the_tape() {
        let mut s = tape(44);
        press(&mut s, "prevMarker");
        assert_eq!(at(&s), 44);

        let mut s = tape(14984);
        press(&mut s, "nextMarker");
        assert_eq!(at(&s), 14984);
    }

    #[test]
    fn the_shuttle_is_as_long_as_the_distance_travelled() {
        // 2152 back to 44 is 2108 ticks of tape, and a twentieth of that.
        let mut s = tape(2152);
        let out = press(&mut s, "prevMarker");
        // The first wait is the button lighting up.
        assert_eq!(waits(&out), [8, 105]);
    }

    #[test]
    fn but_never_longer_than_three_hundred_ticks() {
        // Left mid-tape at 14000, the previous marker is 6102 ticks back,
        // which is 305 -- over the cap.
        let mut s = tape(14000);
        let out = press(&mut s, "prevMarker");
        assert_eq!(at(&s), 7898);
        assert_eq!(waits(&out), [8, 300]);
    }

    #[test]
    fn play_runs_to_the_end_of_the_tape() {
        let mut s = tape(4432);
        let out = press(&mut s, "play");
        let played = out.effects.iter().find_map(|e| match e {
            Effect::PlayVideoSegment { from, to } => Some((*from, *to)),
            _ => None,
        });
        assert_eq!(played, Some((4432, 15000)));
    }

    #[test]
    fn and_will_not_run_past_it() {
        // The clamp in the original is where the tape's length is written
        // down at all.
        let mut s = tape(15600);
        press(&mut s, "play");
        assert_eq!(at(&s), 15000);
    }

    #[test]
    fn pause_stops_the_tape() {
        let mut s = tape(4432);
        let out = press(&mut s, "pause");
        assert!(out.effects.iter().any(|e| matches!(e, Effect::StopVideo)));
        assert_eq!(at(&s), 4432);
    }
}

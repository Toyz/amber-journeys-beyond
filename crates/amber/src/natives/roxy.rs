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

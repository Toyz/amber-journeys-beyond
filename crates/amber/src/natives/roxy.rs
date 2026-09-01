//! Roxy's chapter: the present-day house, the office laptop and the
//! ghost telephone.
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

            let remaining = state.get("ghostsRemaining");
            let present = |who: &str| match &remaining {
                Value::List(items) => items.iter().any(|i| {
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
            let pending = match state.get("hauntsRemaining") {
                Value::List(items) => items
                    .iter()
                    .any(|i| i.as_str().is_some_and(|s| s.eq_ignore_ascii_case(haunt))),
                _ => false,
            };
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

        _ => return false,
    }
    true
}

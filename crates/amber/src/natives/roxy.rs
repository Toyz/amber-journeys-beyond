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
/// The three books, by the verb that acts on them.
///
/// Each entry is the name the flags are built from, the flag that says the
/// book is open, and the frames its pages live on.
fn book_for(verb: &str) -> Option<(&'static str, &'static str, &'static [i32])> {
    const BOOKS: [(&str, &str, &[i32]); 3] = [
        ("Diary", "playerIsReadingDreamDiary", &[1, 2, 3, 5, 6]),
        (
            "Realms",
            "playerIsReadingRealms",
            &[0, 1, 3, 5, 7, 19, 21, 35, 37, 51, 53],
        ),
        ("BarManual", "playerIsReadingBarManual", &[0, 1, 2, 3, 4, 5]),
    ];
    let verb = verb.to_ascii_lowercase();
    BOOKS.into_iter().find(|(book, reading, _)| {
        verb.ends_with(&book.to_ascii_lowercase())
            || verb == format!("set{}", reading.to_ascii_lowercase())
    })
}

/// Points the unit's text channel at one of `#peekText`'s pages.
fn text_page(channel: u8, page: &str) -> Effect {
    Effect::SpriteCastFromTable {
        channel,
        table: "peekText".into(),
        key: page.into(),
    }
}

/// Which page of `#peekText` a status readout should be showing.
///
/// The three machines each keep their own status flag and the pages are named
/// for the machine and the status together -- `#PKbarStatus` of `#Online` is
/// the page `#BarOnline`, `#PKscanStatus` of `#Wait3min` is `#scanWait3min`.
/// So the page is the prefix and the status run together, which is why the
/// table has twenty-six entries and no dispatch table is needed.
fn status_page(display: &str, state: &State) -> Option<String> {
    let (flag, prefix) = match display.to_ascii_lowercase().as_str() {
        "barstatus" => ("PKbarStatus", "bar"),
        "scanstatus" => ("PKscanStatus", "scan"),
        "amberstatus" => ("PKamberStatus", "amber"),
        _ => return None,
    };
    let status = state.get(flag);
    let status = status.as_str()?.trim_start_matches('#').to_string();
    Some(format!("{prefix}{status}"))
}

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


        // on unFreezeInventory
        //   setState( oStoryteller, #inventoryStatus, #hot )
        //   gFreezeInventory = 0
        //
        // The mirror of `freezeInventory` above, and missing until now -- so
        // the bar froze and never thawed.
        "unfreezeinventory" => {
            state.set("inventoryStatus", Value::Symbol("hot".into()));
            state.set("gFreezeInventory", Value::Int(0));
        }

        // on setPlayerIsUsingLaptop suggestion
        //   newValue = #none
        //   if suggestion = 0 then
        //     purgeMultiframes #passwordEntry : unFreezeInventory : killVideo
        //     newValue = 0 : setProp( states, #passwordAttempt, list() )
        //   if suggestion = #prompting then
        //     loadMultiFrames #passwordEntry : newValue = #prompting
        //   if suggestion = #crashing then newValue = #crashing
        //   if suggestion = #crashed  then newValue = #crashed
        //   if suggestion = #restart  then newValue = #restart
        //   if suggestion = #off then
        //     newValue = #off : setProp( states, #passwordAttempt, list() )
        //     soundEffect #computerOff
        //   if suggestion = #warmingUp then
        //     newValue = #warmingUp : soundEffect #computerStart
        //   if suggestion = #password then
        //     freezeInventory : newValue = #password
        //   if suggestion = #startUp then
        //     purgeMultiframes #passwordEntry : unFreezeInventory
        //     newValue = #startUp
        //
        // The laptop's eight states, each with what it does on the way in.
        // Two of them are worth naming:
        //
        // `#password` **freezes the inventory** -- while the cursor is in the
        // password field the player cannot pick something up and use it, which
        // is why the bar goes cold rather than simply being ignored. `#startUp`
        // and switching off thaw it again.
        //
        // And `#off` clears `#passwordAttempt`, as does 0. So a wrong password
        // is not remembered: switching the machine off and on is a real reset
        // and not a way to keep guessing from where you left off.
        "setplayerisusinglaptop" => {
            let asked = args.first().cloned().unwrap_or(Value::Void);
            let is = |name: &str| asked.is_symbol(name);
            let switched_off = asked.as_int() == Some(0);

            if switched_off || is("off") {
                // Nothing typed survives the machine going off.
                state.set_all("passwordAttempt", Vec::new());
            }
            if switched_off || is("startUp") {
                call("unfreezeinventory", &[], state, out);
            }
            if switched_off {
                out.effects.push(Effect::StopVideo);
            }
            if is("off") {
                out.effects.push(Effect::PlaySound {
                    name: "computerOff".into(),
                    loudness: None,
                });
            }
            if is("warmingUp") {
                out.effects.push(Effect::PlaySound {
                    name: "computerStart".into(),
                    loudness: None,
                });
            }
            if is("password") {
                call("freezeinventory", &[], state, out);
            }

            // Only a state it recognises is written; anything else leaves the
            // machine where it was.
            const STATES: [&str; 7] = [
                "prompting",
                "crashing",
                "crashed",
                "restart",
                "off",
                "warmingUp",
                "startUp",
            ];
            if switched_off {
                state.set_all("playerIsUsingLaptop", vec![Value::Int(0)]);
            } else if let Some(&settled) = STATES.iter().find(|v| asked.is_symbol(v)) {
                state.set_all("playerIsUsingLaptop", vec![Value::Symbol(settled.into())]);
            } else {
                return true;
            }
            out.redraw = true;
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

            // `[#low: 90, #medium: 180, #high: 255]`, out of Director's 255.
            let volume = match loudness.trim_start_matches('#') {
                "low" => 90,
                "high" => 255,
                _ => 180,
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
                    candidates.extend(std::iter::repeat_n(None, padding));
                }
            } else if lower == "none" {
                // No ghost calls from here. The original walks the sound
                // channels and takes down any call still running, so leaving
                // the room a ghost calls from stops it calling.
                state.set_all("ghostsCalling", Vec::new());
                out.effects.push(Effect::StopGhostCall);
                return true;
            } else {
                // Anything unrecognised places no call.
                return true;
            }

            // The list is stored, not played. `ghostCalls` only says who is
            // calling from here and how loudly; `playDomainEntrySound` runs
            // off the front of it once a frame and rotates it.
            //
            // This used to pick one candidate at random and play a random
            // call file on the spot. That is a different game: the rota is
            // what spaces the calls out and what the `#nobody` padding is
            // for, and a room that sets the rota was making a single noise
            // and then falling silent for ever.
            state.set_all(
                "ghostsCalling",
                candidates
                    .iter()
                    .map(|c| Value::Symbol(c.unwrap_or("nobody").to_string()))
                    .collect(),
            );
        }

        // on usePeekUnit
        //   setState( #playerHasPeekUnit, #inUse )
        //   killVideo : updateStage : idle : cursorOff
        //   peekBody = 38 : peekAntenna = 46 : peekRollUp = 44 : peekText = 40
        //   pkScanIcon = 41 : pkBarIcon = 42 : pkAmberIcon = 43
        //   ... #PeekDown, #peekAntenna, #PeekUpAnim, then #PeekUp ...
        //   display = getState( #PeekDisplay )
        //   setState( #PeekDisplay, #None )
        //   if display = #ghostKnife then
        //     gPeekPlayList = [#PkFadeIn, #PkKitchenGhost, #PkFadeOut]
        //     setState( #PKbarStatus, #ActivityDetected )
        //     trimState( #cameraFeedbackRemaining, #ghostKnife )
        //   ... and the same shape for the other five camera haunts ...
        //   if display = #BARstartup then
        //     setState( #PKbarStatus, #Online ) ... wait ...
        //     setState( #PKbarStatus, #noActivity )
        //   if display = #amberStartup then
        //     setState( #PKamberStatus, #ModulatingEEG ) ... wait ...
        //     setState( #PKamberStatus, #OneMoment ) ... wait ...
        //     setState( #PKamberStatus, #surfsUp )
        //
        // The hand-held unit, and the only feedback the game gives. Every
        // machine in the house reports through it: the hint book's first
        // instruction is to pick it up and press play, and its standing
        // advice is "whenever the PeeK flashes, click on it".
        //
        // `peekAlert` was ported long ago and makes it flash. Nothing was
        // behind the flash, because this is reached from the inventory bar
        // rather than from a room script and so no tally counted it. Every
        // puzzle solved so far has been reporting into nothing: the BAR comes
        // online and says so here, the scanner finishes and says so here, and
        // the cameras catch each haunt and play it back here.
        //
        // What the unit shows is whatever `#PeekDisplay` was last set to, and
        // reading it clears it -- so an alert is consumed by being looked at.
        "usepeekunit" => {
            // The unit's own sprite channels, and where each one sits.
            //
            // ```text
            // peekBody = 38 : peekAntenna = 46 : peekRollUp = 44 : peekText = 40
            // set the castNum of sprite peekBody    = #PeekDown
            // set the loc     of sprite peekBody    = point( 320, 200 )
            // set the castNum of sprite peekAntenna = #peekAntenna
            // set the loc     of sprite peekAntenna = point( 320, 200 )
            // set the castNum of sprite peekRollUp  = #PeekUpAnim
            // set the loc     of sprite peekRollUp  = point( 317, 189 )
            // ... play it out ...
            // set the castNum of sprite peekBody    = #PeekUp
            // camSprite = peekRollUp
            // set the loc     of sprite camSprite   = point( 317, 132 )
            // set the castNum of sprite camSprite   = PkVideoNormal[#PkNone]
            // ```
            //
            // So the unit is drawn, its aerial goes up, `PeeKup.mov` plays it
            // sliding into view, and the channel the animation was on becomes
            // the little screen the recordings play in. None of that was here:
            // the unit reported what it had as a line of text over the room,
            // with no unit and no picture, which is what helba was looking at.
            const BODY: u8 = 38;
            const ANTENNA: u8 = 46;
            const SCREEN: u8 = 44;
            const TEXT: u8 = 40;
            const SCAN_LIGHT: u8 = 41;
            const BAR_LIGHT: u8 = 42;
            const AMBER_LIGHT: u8 = 43;
            const UNIT_AT: (i32, i32) = (320, 200);
            const ROLL_UP_AT: (i32, i32) = (317, 189);
            const SCREEN_AT: (i32, i32) = (317, 132);
            const TEXT_AT: (i32, i32) = (317, 226);
            const BUTTONS_AT: (i32, i32) = (247, 270);

            let display = state.get("PeekDisplay");
            let display = display.as_str().unwrap_or("None").trim_start_matches('#').to_string();
            state.set("PeekDisplay", Value::Symbol("None".into()));
            state.set("playerHasPeekUnit", Value::Symbol("inUse".into()));
            out.effects.push(Effect::CursorOff);

            // `set the ink of sprite peekBody = 8` (matte) and
            // `... peekAntenna = 36` (background transparent). Both mean the
            // background colour is not painted, which is the only distinction
            // this engine draws -- and without it the unit arrives as a white
            // rectangle with the unit inside it, covering the room.
            let mut place = |channel: u8, name: &str, at: (i32, i32), ink: i32| {
                out.effects.push(Effect::PuppetSprite { channel, on: true });
                out.effects.push(Effect::SpriteInk { channel, ink });
                out.effects.push(Effect::SpriteCastNamed {
                    channel,
                    name: name.into(),
                });
                out.effects.push(Effect::SpriteLoc {
                    channel,
                    x: at.0,
                    y: at.1,
                });
            };
            place(BODY, "PeekDown", UNIT_AT, 8);
            place(ANTENNA, "peekAntenna", UNIT_AT, 36);
            // The roll-up is a film, and the channel holds it until it ends.
            place(SCREEN, "PeekUpAnim", ROLL_UP_AT, 0);
            out.effects.push(Effect::PlayOverlay { channel: SCREEN });
            out.effects.push(Effect::WaitForOverlay);
            out.effects.push(Effect::SpriteCastNamed {
                channel: BODY,
                name: "PeekUp".into(),
            });
            // And then the same channel is the screen, showing the blank
            // frame until something is put in it.
            out.effects.push(Effect::SpriteLoc {
                channel: SCREEN,
                x: SCREEN_AT.0,
                y: SCREEN_AT.1,
            });
            out.effects.push(Effect::SpriteCastFromTable {
                channel: SCREEN,
                table: "PkVideoNormal".into(),
                key: "PkNone".into(),
            });

            // The three status lights along the bottom of the unit, one per
            // machine. All three are placed at the same point --
            //
            // ```text
            // buttonCoords = point( 247, 270 ) + gOriginPoint
            // set the loc of sprite pkScanIcon  = buttonCoords
            // set the loc of sprite pkBarIcon   = buttonCoords
            // set the loc of sprite pkAmberIcon = buttonCoords
            // ```
            //
            // -- and land side by side anyway, because each one's registration
            // point sits a different distance outside its own rectangle: the
            // three casts are 32 by 29 with origins at 342, 389 and 437.
            //
            // Each light has three frames in a list read by position,
            // `[channel, offline, online, active]`, and which one it shows is
            // its machine's own flag.
            let scan = state.get("PKscanStatus");
            let scan_on = !(scan.is_symbol("Offline") || scan.is_symbol("CantAttach"));
            let lights = [
                (SCAN_LIGHT, "scanIcon", scan_on),
                (BAR_LIGHT, "barIcon", state.get("BarOnline").as_int() == Some(1)),
                (AMBER_LIGHT, "amberIcon", state.get("AMBERisOnline").as_int() == Some(1)),
            ];
            // The readout, which every branch below points at a page of
            // `#peekText`. It was never claimed or placed, so the pages were
            // drawn wherever a sprite with no location goes -- the middle of
            // the stage, which happens to be close enough to the middle of
            // the unit that the mistake did not show.
            // No ink: `usePeekUnit` sets the castNum and the location on the
            // readout and on each of the three lights and nothing else, so
            // they are drawn whole. Only the body and the aerial carry an ink
            // -- 8 and 36 -- and they are the two that have to let the room
            // through around their edges.
            out.effects.push(Effect::PuppetSprite { channel: TEXT, on: true });
            out.effects.push(Effect::SpriteLoc {
                channel: TEXT,
                x: TEXT_AT.0,
                y: TEXT_AT.1,
            });
            out.effects.push(text_page(TEXT, "None"));

            for (channel, table, online) in lights {
                out.effects.push(Effect::PuppetSprite { channel, on: true });
                out.effects.push(Effect::SpriteCastFromTable {
                    channel,
                    table: table.into(),
                    // `getAt( list, 3 )` when the machine is up, 2 when it is
                    // not; the first entry is the channel, not a frame.
                    key: if online { "3" } else { "2" }.into(),
                });
                out.effects.push(Effect::SpriteLoc {
                    channel,
                    x: BUTTONS_AT.0,
                    y: BUTTONS_AT.1,
                });
            }

            // A camera haunt: the unit plays back what was caught, the bar
            // records that it caught something, and the haunt comes off the
            // list of what is still to be seen.
            const HAUNTS: [(&str, &str); 6] = [
                ("ghostKnife", "PkKitchenGhost"),
                ("ghostlyKey", "PkBedroomGhost"),
                ("crazyLR", "PkCrazyLR"),
                ("crazyDR", "PkCrazyDR"),
                ("KdKnob", "PkKdKnob"),
                ("bloodBath", "PkBloodBath"),
            ];

            // `camSprite` is the roll-up's channel once the unit is up, so
            // every clip plays in the little screen rather than over the whole
            // body of the unit.
            // A clip in the unit's little screen: the channel is pointed at
            // it and then told to run. Pointing alone is a still -- which is
            // right for the blank frame the screen rests on and wrong for
            // every one of these, and getting that wrong left the unit
            // showing a frozen first frame with the room visible through it.
            let frame = |out: &mut Outcome, name: &str| {
                out.effects.push(Effect::SpriteCastFromTable {
                    channel: SCREEN,
                    table: "PkVideoNormal".into(),
                    key: name.into(),
                });
                out.effects.push(Effect::PlayOverlay { channel: SCREEN });
            };

            if let Some((_, clip)) = HAUNTS
                .iter()
                .find(|(d, _)| d.eq_ignore_ascii_case(&display))
            {
                frame(out, "PkFadeIn");
                out.effects.push(Effect::WaitTicks(15));
                frame(out, clip);
                out.effects.push(Effect::WaitForClick);
                frame(out, "PkFadeOut");
                out.effects.push(Effect::WaitTicks(15));
                out.effects.push(Effect::SetState {
                    key: "PKbarStatus".into(),
                    value: Value::Symbol("ActivityDetected".into()),
                });
                out.effects.push(Effect::TrimState {
                    key: "cameraFeedbackRemaining".into(),
                    item: Value::Symbol(display.clone()),
                });
            } else if display.eq_ignore_ascii_case("psionicFragment") {
                // The fragment the pyramid offers, which is picked up by
                // looking at it.
                frame(out, "PkFragment");
                out.effects.push(Effect::WaitForClick);
            } else if display.eq_ignore_ascii_case("BARstartup") {
                // The machine reporting for duty, and then reporting that it
                // has nothing yet.
                out.effects.push(Effect::SetState {
                    key: "PKbarStatus".into(),
                    value: Value::Symbol("Online".into()),
                });
                out.effects.push(text_page(TEXT, "BarOnline"));
                out.effects.push(Effect::WaitForClick);
                out.effects.push(Effect::SetState {
                    key: "PKbarStatus".into(),
                    value: Value::Symbol("noActivity".into()),
                });
            } else if display.eq_ignore_ascii_case("amberStartup") {
                // Calibrating itself, in three stages with a wait between.
                for (status, page) in [
                    ("ModulatingEEG", "amberModulatingEEG"),
                    ("OneMoment", "amberOneMoment"),
                    ("surfsUp", "amberSurfsUp"),
                ] {
                    out.effects.push(Effect::SetState {
                        key: "PKamberStatus".into(),
                        value: Value::Symbol(status.into()),
                    });
                    out.effects.push(text_page(TEXT, page));
                    out.effects.push(Effect::WaitTicks(120));
                }
            } else if let Some(page) = status_page(&display, state) {
                // The three status readouts, each showing the text for
                // whatever its machine last reported.
                out.effects.push(text_page(TEXT, &page));
                out.effects.push(Effect::WaitForClick);
            } else {
                // Nothing to report: the blank page, dismissed by a click.
                out.effects.push(text_page(TEXT, "None"));
                out.effects.push(Effect::WaitForClick);
            }

            for channel in [AMBER_LIGHT, BAR_LIGHT, SCAN_LIGHT, TEXT, SCREEN, ANTENNA, BODY] {
                out.effects.push(Effect::PuppetSprite { channel, on: false });
            }
            // The unit stays in the hand. Nothing here puts it back, and that
            // is deliberate: the room-sized `#itemInUse` catcher stows it on
            // the next click, which is what the office table means by "don't
            // worry, it'll be added automatically when user is finished".
            // Stowing is also what restores the flag from `#inUse` to
            // `#carrying`, so writing that here would be inventing a step.
            out.redraw = true;
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
        // on initInventory
        //   ... rebuilds the bar ...
        //   if getState( #currentLocation ) = #DarkUp_40sReentry then
        //     fadeOutTransit
        //     trimState( #ghostsRemaining, #Margaret ) : ghostCalls #None
        //     trimState( #hauntsRemaining, #ghostBrushingHair )
        //     trimState( #hauntsRemaining, #stairsGhost )
        //     setLoop #houseHum, 96
        //     setState( #showMontage, 1 ) : setTransition #slowMontage
        //     updateDisplay
        //     setState( #PeekDisplay, #psionicFragment ) : peekAlert
        //   ... and the same for #Ggaz_Reentry and #Gbhs_Reentry1 ...
        //
        // The handler that closes a chapter. Coming home is not just arriving
        // in a room: the ghost is taken off `#ghostsRemaining`, the haunts
        // that were that ghost's are taken off `#hauntsRemaining` so they stop
        // being drawn, and the PeeK unit reports a psionic fragment -- which
        // is the game telling the player one of the three is done.
        //
        // None of it was ported, and the cost was the whole ending. The
        // headgear on Roxy in the garage is the last click in the game, and
        // the chain that gets there is counted in ghosts. Three chapters
        // played and `#ghostsRemaining` still held all three.
        //
        // The original hangs this off the inventory refresh, which runs
        // constantly; here it runs once, when the chapter's own way home puts
        // the player in the re-entry room, which is the moment it means.
        "closechapter" => {
            const HOMECOMINGS: [(&str, &str, &[&str]); 3] = [
                (
                    "DarkUp_40sReentry",
                    "Margaret",
                    &["ghostBrushingHair", "stairsGhost"],
                ),
                (
                    "Ggaz_Reentry",
                    "Brice",
                    &["knifeShadow", "hungMan", "TVghost", "Gazebo1", "gazebo2", "gazebo3"],
                ),
                ("Gbhs_Reentry1", "Edwin", &["lakeGhost", "lakeGhost2"]),
            ];
            let Some(where_now) = args.first().and_then(|v| v.as_str().map(str::to_string)) else {
                return true;
            };
            let Some((_, ghost, haunts)) = HOMECOMINGS
                .iter()
                .find(|(room, ..)| room.eq_ignore_ascii_case(where_now.trim_start_matches('#')))
            else {
                return true;
            };

            state.trim_item("ghostsRemaining", &Value::Symbol((*ghost).into()));
            for haunt in *haunts {
                state.trim_item("hauntsRemaining", &Value::Symbol((*haunt).into()));
            }
            // Margaret's is the one that also drops the house hum, because
            // hers is the chapter that leaves the player upstairs in the dark.
            if ghost.eq_ignore_ascii_case("Margaret") {
                out.effects.push(Effect::StartLoop {
                    name: "houseHum".into(),
                    volume: Some(96),
                });
            }
            state.set("showMontage", Value::Int(1));
            out.effects.push(Effect::SetTransition { kind: "slowMontage".into() });
            state.set("PeekDisplay", Value::Symbol("psionicFragment".into()));
            call("peekalert", &[], state, out);

            // The Amber vision comes back on, which the shipped data never
            // does. This is a departure and it is here because without it the
            // game cannot be finished.
            //
            // `enterNewDomain` sets `#AMBERVISION` to `#off` and ends the
            // `#amberHum` loop on the way *out* of Roxy's house, before it
            // stashes her state -- so the state that comes back from the
            // freezer has the vision off. Nothing turns it on again: the
            // eleven room writes, both branches of `enterNewDomain` and this
            // handler's own original have been read, and `#off` has no exit
            // from the state machine at all. The way in is
            // `#waitingForPlayer`, and the only thing that sets that is the
            // telephone, which rings once in the whole game.
            //
            // So by the disc, entering one chapter spends the vision and the
            // other two portals never open. That cannot be what was meant --
            // the hint book says "you may enter these domains in any order" --
            // and it leaves the player stranded in a house with nothing left
            // to do. Mirroring the way out is the smallest restore that gets
            // the stated design back: the headgear is still worn, so the
            // vision it grants is still on.
            if state.get("playerHasHeadgear").as_int() != Some(0) {
                state.set("AMBERVISION", Value::Symbol("on".into()));
                out.effects.push(Effect::StartLoop {
                    name: "amberHum".into(),
                    volume: Some(255),
                });
            }
            out.redraw = true;
        }

        "peekalert" => {
            // `enablePeekAlert` and `disablePeekAlert` are one line each and
            // only the camcorder log turns it off, so an unset flag means on.
            let enabled = state
                .get("gPeekAlertEnabled")
                .as_int()
                .unwrap_or(1)
                != 0;
            let carried = state.carrying("PeekUnit");
            if !enabled || !carried {
                return true;
            }
            // Sprite 7 is the bar's middle slot and the PeeK always sits
            // there, which is why the original can name the channel and be
            // sure of what it is holding. This engine draws the bar from what
            // is carried, so it asks the bar for the other icon instead --
            // puppeting channel 7 put a 67-pixel icon in the middle of the
            // room, underneath the room's own plates, where nobody saw it.
            for i in 0..12 {
                out.effects.push(Effect::WaitTicks(5));
                // The third icon is the bright glow and the second the dim
                // one, which is how the item comes to list three where every
                // other lists two.
                out.effects.push(Effect::InventoryIcon {
                    item: "PeekUnit".into(),
                    index: Some(if i % 2 == 0 { 3 } else { 2 }),
                });
            }
            // `set the castNum of sprite 7 = oldPeekGraphic` -- the pulse puts
            // back whatever the bar was showing before it started.
            out.effects.push(Effect::WaitTicks(5));
            out.effects.push(Effect::InventoryIcon {
                item: "PeekUnit".into(),
                index: None,
            });
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
        // on testForPsionicWaves
        //   cameraFeedbackRemaining = count( #cameraFeedbackRemaining )
        //   oscillatorInPlace       = getState( #oscillatorInPlace )
        //   tonalResidueRemaining   = count( #tonalResidueRemaining )
        //   if cameraFeedbackRemaining < 1 and oscillatorInPlace
        //      and tonalResidueRemaining < 4 then
        //     setState( #psionicWavesPresent, 1 )
        //     if inState( #hauntsRemaining, #phoneMessage ) then
        //       setState( #ghostlyPhoneCall, #ringingNow )
        //
        // The gate to the second half of the game, and the whole of it is
        // these three counts. The house has to have shown everything its
        // cameras caught, the oscillator has to be in the AMBER device, and
        // at least one of the four door residues has to have been listened
        // to. Then the telephone rings in the living room, and answering it
        // is what activates the headgear -- which is what turns the Amber
        // vision on, which is what makes the ghosts call, which is what leads
        // the player to the portals.
        //
        // Everything after the first hour of this game is behind that
        // telephone, and the telephone is behind this handler, and this
        // handler was not ported. The three counts were all being kept
        // correctly and nothing ever read them.
        //
        // It is called from `stowInventory`, and only when what is being
        // stowed is the PeeK unit -- which is exactly the right moment, since
        // the PeeK is how all three of those things are seen. You look at
        // what the house has to show you, put the unit away, and the game
        // asks whether that was the last of it.
        "testforpsionicwaves" => {
            let cameras = state.get_all("cameraFeedbackRemaining").len();
            let residues = state.get_all("tonalResidueRemaining").len();
            let oscillator = state.get("oscillatorInPlace").truthy();
            if cameras >= 1 || !oscillator || residues >= 4 {
                return true;
            }
            state.set("psionicWavesPresent", Value::Int(1));

            let still_to_come = state
                .get_all("hauntsRemaining")
                .iter()
                .any(|h| h.as_str().is_some_and(|s| s.eq_ignore_ascii_case("phoneMessage")));
            if still_to_come {
                call(
                    "setghostlyphonecall",
                    &[Value::Symbol("ringingNow".into())],
                    state,
                    out,
                );
            }
        }

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

        // on exitFrame                     -- Roxy's frame script, on entry
        //   repeat with i = 1 to 48: puppetSprite i, 1
        //   scanStatus = getState( #PKscanStatus )
        //   if getPos( [#Wait1min .. #Wait5min], scanStatus ) <> 0 then
        //     minutesRemaining = getPos( [#Wait1min .. #Wait5min], scanStatus )
        //     gScanFinish = the ticks + minutesRemaining * 3600
        //   initInventory : domainIsReady = 1 : cursorOff
        //   restoreSounds #fadeIn : closePatchFile : go "bigLoop"
        //
        // Coming back into Roxy's chapter with a scan already running restarts
        // its clock from however many minutes the status says are left, rather
        // than carrying a deadline across from whenever the chapter was left.
        // A scan does not run while you are somewhere else.
        //
        // Only this branch is ported; the rest is loading and hand-off that
        // this engine does its own way.
        //
        // Guarded on the chapter. Every chapter has an `exitFrame` and two of
        // them do real work, so without this the first arm in the dispatch
        // chain answers for all four -- which is exactly what happened:
        // adding this one stopped Margaret's opening running at all.
        "exitframe" if state.get("gChapter").is_symbol("ROXY") => {
            const COUNTDOWN: [&str; 5] = [
                "Wait1min", "Wait2min", "Wait3min", "Wait4min", "Wait5min",
            ];
            let status = state.get("PKscanStatus");
            let Some(status) = status.as_str().map(|s| s.trim_start_matches('#')) else {
                return true;
            };
            if let Some(minutes) = COUNTDOWN
                .iter()
                .position(|w| w.eq_ignore_ascii_case(status))
                .map(|i| i as i32 + 1)
            {
                let now = state.get("gTicks").as_int().unwrap_or(0);
                state.set("gScanFinish", Value::Int(now + minutes * 3600));
            }
            out.effects.push(Effect::RestoreSounds { fade: true });
        }

        // From the peek unit's own `on mouseDown`, which recomputes the
        // countdown every time the display is looked at:
        //
        //   currentStatus = getState( #PKscanStatus )
        //   if currentStatus = #ReadyForPlayback and getState( #scanUnitIsActive ) = 0 then
        //     setProp( states, #PKscanStatus, list(#Online) )
        //   if getPos( [#Wait1min, #Wait2min, #Wait3min, #Wait4min, #Wait5min],
        //              currentStatus ) <> 0 then
        //     if voidp( gScanFinish ) then
        //       put ">¥> There is no set 'gScanFinish' time.. this can only lead to trouble..."
        //     else
        //       minutesRemaining = (gScanFinish - the ticks) / 3600 + 1
        //       if minutesRemaining > 0
        //         then currentStatus = getAt( [#Wait1min .. #Wait5min], minutesRemaining )
        //         else currentStatus = #ReadyForPlayback
        //   if currentStatus <> #Offline and currentStatus <> #CantAttach then
        //     setState( #PKscanStatus, currentStatus )
        //
        // The scan unit counts down in real time. `setScanTime` parks a
        // deadline in `gScanFinish` and this is what walks the status back
        // through `#Wait5min`, `#Wait4min` ... to `#ReadyForPlayback`. Without
        // it a scan started never finishes, which is what this port did: the
        // deadline was written and never read.
        //
        // The original recomputes when the player looks at the unit, on a
        // sprite `mouseDown` this engine has no equivalent of. Here it is
        // recomputed each frame instead. Only the unit's own display and its
        // setter ever read `#PKscanStatus` -- five scripts touch it and three
        // of them are the unit itself -- so a status that is always current
        // rather than current-when-looked-at is not a difference anything can
        // see.
        "resetpeekdisplay" => {
            const COUNTDOWN: [&str; 5] = [
                "Wait1min", "Wait2min", "Wait3min", "Wait4min", "Wait5min",
            ];
            let status = state.get("PKscanStatus");
            let Some(status) = status.as_str().map(|s| s.trim_start_matches('#')) else {
                return true;
            };

            // A unit that finished its scan and was then switched off goes
            // back to simply being on.
            if status.eq_ignore_ascii_case("ReadyForPlayback")
                && state.get("scanUnitIsActive").as_int() == Some(0)
            {
                state.set("PKscanStatus", Value::Symbol("Online".into()));
                return true;
            }
            if !COUNTDOWN.iter().any(|w| w.eq_ignore_ascii_case(status)) {
                return true;
            }
            let Some(finish) = state.get("gScanFinish").as_int() else {
                trace!(
                    crate::trace::Topic::Script,
                    "scan unit is counting down with no deadline set"
                );
                return true;
            };
            let now = state.get("gTicks").as_int().unwrap_or(0);
            // Ticks are sixtieths, so 3600 of them is the minute. The plus one
            // is what makes a scan with any time left at all still read as a
            // whole minute remaining.
            let remaining = (finish - now) / 3600 + 1;
            // The plus one compensates for the truncating division, so any
            // time left at all still reads as a whole minute remaining. It
            // also means that evaluated on the very tick a five-minute scan is
            // started, this comes out as six. The original never sees that --
            // it only recomputes when the player looks at the unit, by which
            // point the clock has moved -- but this runs every frame, so the
            // clamp is written down rather than left to an index that happens
            // to fall off the end of the list.
            let settled = if remaining > 0 {
                COUNTDOWN[(remaining as usize).clamp(1, COUNTDOWN.len()) - 1]
            } else {
                "ReadyForPlayback"
            };
            if !settled.eq_ignore_ascii_case(status) {
                state.set("PKscanStatus", Value::Symbol(settled.into()));
                out.redraw = true;
            }
        }

        // on setcurrentPageIn<Book> suggestion
        //   cursorOff
        //   frameStack = getProp( oPuppeteer.frames, #<book> )
        //   pageList = [ ... ]
        //   currentPage = getPos( pageList, getState( #currentPageIn<Book> ) )
        //   if suggestion = #next then
        //     if currentPage = count(pageList) then
        //       setState( #playerIsReading<Book>, 0 ) : updateDisplay
        //     else ... step forward, and slide the page sprite across ...
        //   if suggestion = #previous then
        //     if currentPage = 1 then
        //       setState( #playerIsReading<Book>, 0 ) : updateDisplay
        //     else ... step back ...
        //
        // on setplayerIsReading<Book> suggestion
        //   setProp( oStoryteller.states, #currentPageIn<Book>, list(<first page>) )
        //   setProp( oStoryteller.states, #playerIsReading<Book>, list(suggestion) )
        //   setTransition( oPuppeteer, #fadeIn )
        //
        // Three books, one shape. Turning past either end closes the book
        // rather than stopping at it, and opening one always starts at its
        // first page -- there is no bookmark, which is why the diary can be
        // read straight through and not resumed.
        //
        // The page lists are not ranges:
        //
        //     dream diary   [1, 2, 3, 5, 6]
        //     realms        [0, 1, 3, 5, 7, 19, 21, 35, 37, 51, 53]
        //     bar manual    [0, 1, 2, 3, 4, 5]
        //
        // They are the frames each page lives on, so the gaps are spreads and
        // not missing pages. Treating any of them as "first to last" would
        // turn to a frame that is not a page.
        "setcurrentpageindiary" | "setcurrentpageinrealms" | "setcurrentpageinbarmanual" => {
            let Some((book, reading, pages)) = book_for(name) else {
                return true;
            };
            let forward = args
                .first()
                .and_then(Value::as_str)
                .is_some_and(|d| d.trim_start_matches('#').eq_ignore_ascii_case("next"));

            let flag = format!("currentPageIn{book}");
            let held = state.get(&flag).as_int().unwrap_or(pages[0]);
            let at = pages.iter().position(|p| *p == held).unwrap_or(0);

            out.effects.push(Effect::CursorOff);
            let past_the_end = if forward { at + 1 >= pages.len() } else { at == 0 };
            if past_the_end {
                state.set_all(reading, vec![Value::Int(0)]);
            } else {
                let moved = if forward { at + 1 } else { at - 1 };
                state.set_all(&flag, vec![Value::Int(pages[moved])]);
            }
            out.redraw = true;
        }

        "setplayerisreadingdreamdiary"
        | "setplayerisreadingrealms"
        | "setplayerisreadingbarmanual" => {
            let Some((book, reading, pages)) = book_for(name) else {
                return true;
            };
            let open = args.first().and_then(Value::as_int).unwrap_or(0);
            // Always at the first page: opening a book is not resuming it.
            state.set_all(&format!("currentPageIn{book}"), vec![Value::Int(pages[0])]);
            state.set_all(reading, vec![Value::Int(open)]);
            out.effects.push(Effect::SetTransition {
                kind: "fadeIn".into(),
            });
            out.redraw = true;
        }

        // on setPKamberStatus suggestion
        //   validList = [#Incomplete, #Online, #WaveButIncomplete, #WaveActivated,
        //                #ModulatingEEG, #oneMoment, #surfsUp]
        //   if getPos( validList, suggestion ) = 0 then alert : return
        //   if suggestion = #Online then
        //     if getState( #psionicWavesPresent ) then suggestion = #WaveActivated
        //                                          else suggestion = #Online
        //   if suggestion = #WaveActivated then
        //     if getState( #oscillatorInPlace ) then suggestion = #WaveActivated
        //                                        else suggestion = #WaveButIncomplete
        //   setProp( oStoryteller.states, #PKamberStatus, list(suggestion) )
        //   ... sprite 43's cast follows the status ...
        //
        // The peek unit's amber display corrects what it is asked for. Ask for
        // `#Online` while the waves are present and it shows `#WaveActivated`;
        // ask for `#WaveActivated` without the oscillator in place and it
        // shows `#WaveButIncomplete`. So the display never claims more than
        // the equipment can do, and the two corrections chain: `#Online` with
        // waves but no oscillator lands on `#WaveButIncomplete`.
        "setpkamberstatus" => {
            const VALID: [&str; 7] = [
                "Incomplete",
                "Online",
                "WaveButIncomplete",
                "WaveActivated",
                "ModulatingEEG",
                "oneMoment",
                "surfsUp",
            ];
            let Some(asked) = args.first().and_then(Value::as_str).map(|s| s.trim_start_matches('#'))
            else {
                return true;
            };
            let Some(&wanted) = VALID.iter().find(|v| v.eq_ignore_ascii_case(asked)) else {
                trace!(
                    crate::trace::Topic::Script,
                    "setPKamberStatus: {asked} is not a status"
                );
                return true;
            };

            let mut settled = wanted;
            if settled.eq_ignore_ascii_case("Online") && state.get("psionicWavesPresent").truthy() {
                settled = "WaveActivated";
            }
            if settled.eq_ignore_ascii_case("WaveActivated")
                && !state.get("oscillatorInPlace").truthy()
            {
                settled = "WaveButIncomplete";
            }
            state.set_all("PKamberStatus", vec![Value::Symbol(settled.into())]);
            out.redraw = true;
        }

        // on setPKbarStatus suggestion
        //   validList = [#Offline, #Online, #noActivity, #activityDetected]
        //   if getPos( validList, suggestion ) = 0 then alert : return
        //   setProp( oStoryteller.states, #PKbarStatus, list(suggestion) )
        //   ... sprite 42's cast follows it ...
        //
        // No correction on this one, only the refusal. Worth having anyway:
        // without it a mistyped status would be written and the sprite keyed
        // on it would find no cast and draw nothing.
        "setpkbarstatus" => {
            const VALID: [&str; 4] = ["Offline", "Online", "noActivity", "activityDetected"];
            let Some(asked) = args.first().and_then(Value::as_str).map(|s| s.trim_start_matches('#'))
            else {
                return true;
            };
            match VALID.iter().find(|v| v.eq_ignore_ascii_case(asked)) {
                Some(&wanted) => {
                    state.set_all("PKbarStatus", vec![Value::Symbol(wanted.into())]);
                    out.redraw = true;
                }
                None => trace!(
                    crate::trace::Topic::Script,
                    "setPKbarStatus: {asked} is not a status"
                ),
            }
        }

        // on setvideoTapePosition suggestion
        //   setProp( oStoryteller.states, #videoTapePosition, list(suggestion) )
        //
        // The whole handler. It exists so that `setState` has something to
        // dispatch to for a flag whose value list holds one entry, and it does
        // exactly what the direct write would have done -- which is worth
        // having ported rather than left to the fallback, because the fallback
        // is the thing that quietly happens when a setter is *missing*.
        "setvideotapeposition" => {
            if let Some(v) = args.first() {
                state.set_all("videoTapePosition", vec![v.clone()]);
            }
        }

        // on pyramidSpeaks
        //   cursorOff
        //   remainingMessages = getProp( oStoryteller.states, #pyramidMessagesRemaining )
        //   messagesStack = getProp( oPuppeteer.frames, #PyramidMsg )
        //   if count( remainingMessages ) > 0 then
        //     helpTest = getAt( remainingMessages, 1 )
        //     if helpTest = #helpMe then
        //       myAnswer = 6 : deleteAt( remainingMessages, 1 )
        //     else
        //       msgPosition = random( count( remainingMessages ) )
        //       myAnswer = getAt( remainingMessages, msgPosition )
        //       ...
        //
        // The pyramid answers. `#helpMe` is always first and always taken from
        // the front -- it is the one thing it says before it will say anything
        // else -- and after that it picks from what is left at random, so two
        // players get the same first answer and a different second.
        //
        // Nothing is said once the list is empty.
        "pyramidspeaks" => {
            out.effects.push(Effect::CursorOff);
            let left = state.get_all("pyramidMessagesRemaining").to_vec();
            if left.is_empty() {
                return true;
            }
            // The first message is not drawn from the pile; it is the pile's lid.
            let at = if left[0].is_symbol("helpMe") {
                0
            } else {
                (roll(state, left.len() as i32) - 1).max(0) as usize
            };
            let Some(said) = left.get(at).cloned() else {
                return true;
            };
            state.trim_item("pyramidMessagesRemaining", &said);
            if let Some(name) = said.as_str() {
                out.effects.push(Effect::PlaySound {
                    name: name.trim_start_matches('#').into(),
                    loudness: None,
                });
            }
            out.redraw = true;
        }

        // on transitToEdwin
        //   cursorOff
        //   setState( #AMBERVISION, #off )
        //   ... fadeOut, killVideo, sprite 39 off stage ...
        //   setState( #showMontage, 1 ) : updateDisplay #fastVideo
        //   castCursor #toEdwin
        //   pushVideo : wait #videoStop
        //   setState( #showMontage, 2 ) : killVideo : updateDisplay #fastVideo
        //   pushVideo : wait #videoStop : killVideo
        //   setState( #showMontage, 3 ) : setTransition #fadeIn : updateDisplay
        //   setState( #showMontage, 0 )
        //   enterNewDomain( oStoryteller, string(#Edwin), 15 )
        //
        // Roxy's chapter into Edwin's, and the same shape as `goodbyeMandy`
        // ending Brice's: montage steps with a film on each, then the domain
        // changes. The monitor goes off first -- `#AMBERVISION` to `#off` --
        // because the next chapter is not somewhere it works.
        //
        // `castCursor #toEdwin` is a cursor label rather than a number, and
        // `castCursor` prints "wow, a cursor label" and returns without doing
        // anything. It is dead in the original too, so nothing is missing by
        // leaving it out.
        "transittoedwin" => {
            out.effects.push(Effect::CursorOff);
            state.set("AMBERVISION", Value::Symbol("off".into()));
            out.effects.push(Effect::StopVideo);

            for step in [1, 2] {
                out.effects.push(Effect::SetState {
                    key: "showMontage".into(),
                    value: Value::Int(step),
                });
                out.effects.push(Effect::PlayVideo(None));
                out.effects.push(Effect::WaitForVideo);
                out.effects.push(Effect::StopVideo);
            }
            out.effects.push(Effect::FadeToMontage(3));
            out.effects.push(Effect::SetState {
                key: "showMontage".into(),
                value: Value::Int(0),
            });
            out.new_domain = Some("EDWIN".into());
        }

        // on forcePalette palName
        //   puppetPalette palName, 60
        //   setProp( ..., #changeMe, ... )
        //   cursorOn
        //
        // Director has one palette for the whole stage, so a room whose art
        // was authored against another has to force it before drawing. This
        // engine resolves a palette per cast member -- each plate carries the
        // number of the one it was drawn against, from entry 94 -- so there is
        // no stage palette to force and nothing to do.
        //
        // Ported as the no-op it is here rather than left unported, so the
        // count stops reporting it as work outstanding when the work is
        // already done a different way.
        "forcepalette" => {}

        // on setBarMode whichOne
        //   cursorOff
        //   oldMode = getState( #BarMode )
        //   if whichOne = #power then
        //     ... light the button from #barButtons ...
        //     if oldMode = #runOFF then i = #runON
        //     if oldMode = #runON  then i = #runOFF
        //     if oldMode = #setOFF then i = #setON
        //     if oldMode = #setON  then i = #setOFF
        //   else                                        -- #mode
        //     ... light the switch from #barSwitch ...
        //     if oldMode = #runOFF then i = #setOFF
        //     if oldMode = #runON  then i = #setON
        //     if oldMode = #setOFF then i = #runOFF
        //     if oldMode = #setON  then i = #runON
        //   if i = #runON then
        //     if getState( #BarLevel ) = 6 and getState( #BarGain ) = 5
        //                                 and getState( #BarFM )    = 8 then
        //       setState( #BarOnline, 1 )
        //       go ...
        //   setProp( oStoryteller.states, #BarMode, list(i) )
        //
        // The panel has two buttons and four modes. Power switches on and off
        // without changing which of the two modes it is in; mode switches
        // between running and setting without changing whether it is on. So
        // `#runOFF`, `#runON`, `#setOFF`, `#setON`, and the schema starts at
        // `#setOFF`.
        //
        // **Level six, gain five, FM eight**, then set it running with the
        // power on, and `#BarOnline` comes up. That is the whole puzzle, and
        // it is the only place those three numbers appear together.
        //
        // Neither `#power` nor `#mode` is a mode: they are what the buttons
        // ask for. Without this handler the flag was being set to the request
        // itself, so the panel sat in a state that is not one of its four and
        // nothing worked -- which is exactly what helba found.
        "setbarmode" => {
            let asked = args
                .first()
                .and_then(Value::as_str)
                .map(|w| w.trim_start_matches('#').to_ascii_lowercase())
                .unwrap_or_default();
            let old = state.get("BarMode");
            let old = old.as_str().unwrap_or("setOFF").trim_start_matches('#');

            let is = |m: &str| old.eq_ignore_ascii_case(m);
            let moved = match asked.as_str() {
                // On and off, staying in whichever mode it is in.
                "power" => {
                    if is("runOFF") {
                        "runON"
                    } else if is("runON") {
                        "runOFF"
                    } else if is("setOFF") {
                        "setON"
                    } else {
                        "setOFF"
                    }
                }
                // Running and setting, staying on or off as it was.
                "mode" => {
                    if is("runOFF") {
                        "setOFF"
                    } else if is("runON") {
                        "setON"
                    } else if is("setOFF") {
                        "runOFF"
                    } else {
                        "runON"
                    }
                }
                _ => return true,
            };

            out.effects.push(Effect::CursorOff);
            if moved == "runON"
                && state.get("BarLevel").as_int() == Some(6)
                && state.get("BarGain").as_int() == Some(5)
                && state.get("BarFM").as_int() == Some(8)
            {
                state.set("BarOnline", Value::Int(1));
                // ... setState( #PeekDisplay, #BARstartup )
                //     unFreezeInventory
                //     peekAlert
                //
                // The machine tells the player it is running by flashing the
                // PeeK unit, and the unit is where it then says so. Without
                // these two lines the puzzle was solvable and silent: the
                // right numbers brought it online and nothing anywhere
                // acknowledged it.
                state.set("PeekDisplay", Value::Symbol("BARstartup".into()));
                call("peekalert", &[], state, out);
            }
            state.set_all("BarMode", vec![Value::Symbol(moved.into())]);
            out.redraw = true;
        }

        // on setBarSelection
        //   cursorOff
        //   ... light the button ...
        //   if getState( #BarMode ) = #setON then
        //     i = [#level, #gain, #FM]
        //     pos = getPos( i, getState( #BarSelection ) )
        //     ... step it, wrapping at three ...
        //     setProp( oStoryteller.states, #BarSelection, list(getAt(i, pos)) )
        //     ... move the dash to the new column ...
        //   else
        //     wait 2 : ... unlight the button ...
        //
        // Which of the three digits the up and down buttons act on, and only
        // while the panel is in setting mode and switched on. Pressing it with
        // the bar running does nothing but light the button for two ticks.
        "setbarselection" => {
            const COLUMNS: [&str; 3] = ["level", "gain", "FM"];
            out.effects.push(Effect::CursorOff);
            if !state.get("BarMode").is_symbol("setON") {
                out.effects.push(Effect::WaitTicks(2));
                return true;
            }
            let held = state.get("BarSelection");
            let held = held.as_str().unwrap_or("level");
            let at = COLUMNS
                .iter()
                .position(|c| c.eq_ignore_ascii_case(held.trim_start_matches('#')))
                .unwrap_or(0);
            let moved = COLUMNS[(at + 1) % COLUMNS.len()];
            state.set_all("BarSelection", vec![Value::Symbol(moved.into())]);
            out.redraw = true;
        }

        // on adjustBarSettings upOrDown
        //   cursorOff
        //   whichSetting = getState( #BarSelection )
        //   if whichSetting = #level then digitStack = getProp( frames, #levelDigits )
        //   if whichSetting = #gain  then digitStack = getProp( frames, #gainDigits )
        //   if whichSetting = #FM    then digitStack = getProp( frames, #FMdigits )
        //   ... find the digit and button sprites in channels 10..48 ...
        //   startTimer
        //   ... light the pressed button ...
        //   repeat while stillDown and lagTime ... and getState( #BarMode ) = #setON
        //     if whichSetting = #level then
        //       currentLevel = getState( #BarLevel )
        //       if upOrDown = #up   then newLevel = (currentLevel + 11) mod 10
        //       if upOrDown = #down then newLevel = (currentLevel + 9)  mod 10
        //       setProp( oStoryteller.states, #BarLevel, list(newLevel) )
        //     ... the same for #gain and #FM ...
        //
        // Three digits on the psionic bar -- level, gain and FM -- one of them
        // selected by `#BarSelection`, each running 0 to 9 and wrapping.
        //
        // `(x + 11) mod 10` and `(x + 9) mod 10` is the same trick the lock in
        // Brice's chapter uses: adding rather than subtracting keeps the value
        // positive so the modulo behaves. Worth noticing that these wrap while
        // the algorithm columns on the same machine refuse at their limits --
        // one panel, two deliberately different dials, and no way to guess
        // which is which without reading both.
        //
        // Nothing moves unless the bar is switched on.
        "adjustbarsettings" => {
            const SETTINGS: [(&str, &str); 3] = [
                ("level", "BarLevel"),
                ("gain", "BarGain"),
                ("FM", "BarFM"),
            ];
            if !state.get("BarMode").is_symbol("setON") {
                return true;
            }
            let Some(flag) = state.get("BarSelection").as_symbol().and_then(|sel| {
                let sel = sel.trim_start_matches('#');
                SETTINGS
                    .iter()
                    .find(|(name, _)| name.eq_ignore_ascii_case(sel))
                    .map(|(_, flag)| *flag)
            }) else {
                return true;
            };
            let up = args
                .first()
                .and_then(Value::as_str)
                .is_some_and(|d| d.trim_start_matches('#').eq_ignore_ascii_case("up"));

            out.effects.push(Effect::CursorOff);
            let current = state.get(flag).as_int().unwrap_or(0);
            let moved = (current + if up { 11 } else { 9 }).rem_euclid(10);
            state.set_all(flag, vec![Value::Int(moved)]);
            out.repeat_while_held = true;
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
            let off = current.is_symbol("off");

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
            let on = !state.get("BT_alignmentLeft").is_symbol("off");

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
            let carrying = state.carrying("Crowbar");
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

    // -- the bar's three digits ---------------------------------------------

    fn bar_digits() -> State {
        let mut s = State::new();
        s.set_all("gChapter", vec![Value::Symbol("ROXY".into())]);
        s.set_all("BarMode", vec![Value::Symbol("setON".into())]);
        s.set_all("BarSelection", vec![Value::Symbol("level".into())]);
        s.set_all("BarLevel", vec![Value::Int(4)]);
        s.set_all("BarGain", vec![Value::Int(2)]);
        s.set_all("BarFM", vec![Value::Int(0)]);
        s
    }

    fn nudge(state: &mut State, up: bool) {
        let mut out = Outcome::default();
        let dir = if up { "up" } else { "down" };
        assert!(call(
            "adjustbarsettings",
            &[Value::Symbol(dir.into())],
            state,
            &mut out
        ));
    }

    #[test]
    fn the_bars_digits_wrap_where_its_columns_refuse() {
        let mut s = bar_digits();
        s.set_all("BarLevel", vec![Value::Int(9)]);
        nudge(&mut s, true);
        assert_eq!(s.get("BarLevel"), Value::Int(0));
        nudge(&mut s, false);
        assert_eq!(s.get("BarLevel"), Value::Int(9));
    }

    #[test]
    fn the_selection_decides_which_digit_moves() {
        let mut s = bar_digits();
        s.set_all("BarSelection", vec![Value::Symbol("gain".into())]);
        nudge(&mut s, true);
        assert_eq!(s.get("BarGain"), Value::Int(3));
        // The others are untouched.
        assert_eq!(s.get("BarLevel"), Value::Int(4));
        assert_eq!(s.get("BarFM"), Value::Int(0));
    }

    #[test]
    fn and_nothing_moves_while_the_bar_is_switched_off() {
        let mut s = bar_digits();
        s.set_all("BarMode", vec![Value::Symbol("setOFF".into())]);
        nudge(&mut s, true);
        assert_eq!(s.get("BarLevel"), Value::Int(4));
    }

    // -- the scan unit's countdown -----------------------------------------

    fn scanning(minutes: i32) -> State {
        let mut s = State::new();
        s.set_all("gChapter", vec![Value::Symbol("ROXY".into())]);
        s.set_all("gTicks", vec![Value::Int(0)]);
        s.set_all("scanUnitIsActive", vec![Value::Int(1)]);
        let mut out = Outcome::default();
        assert!(call("setscantime", &[Value::Int(minutes)], &mut s, &mut out));
        s
    }

    fn at_tick(state: &mut State, ticks: i32) -> String {
        state.set_all("gTicks", vec![Value::Int(ticks)]);
        let mut out = Outcome::default();
        assert!(call("resetpeekdisplay", &[], state, &mut out));
        state.get("PKscanStatus").as_str().unwrap_or("").to_string()
    }

    /// The original's arithmetic, worked through for a five-minute scan set
    /// at tick zero, so `gScanFinish` is 18000:
    ///
    /// | tick | `(finish - now) / 3600 + 1` | shows |
    /// |---|---|---|
    /// | 0 | 6, clamped | `Wait5min` |
    /// | 3600 | 5 | `Wait5min` |
    /// | 7200 | 4 | `Wait4min` |
    /// | 14400 | 2 | `Wait2min` |
    /// | 18000 | 1 | `Wait1min` |
    /// | 21600 | 0 | `ReadyForPlayback` |
    ///
    /// So the unit rounds a part-minute up and finishes one minute after its
    /// nominal deadline -- which is self-consistent, because the number it
    /// shows is the number of minutes still to run. I had assumed a plain
    /// countdown and written the test to that; the handler was right and the
    /// test was wrong.
    #[test]
    fn a_scan_counts_itself_down_and_finishes() {
        let mut s = scanning(5);
        assert_eq!(at_tick(&mut s, 0), "Wait5min");
        assert_eq!(at_tick(&mut s, 3600), "Wait5min");
        assert_eq!(at_tick(&mut s, 7200), "Wait4min");
        assert_eq!(at_tick(&mut s, 10800), "Wait3min");
        assert_eq!(at_tick(&mut s, 14400), "Wait2min");
        assert_eq!(at_tick(&mut s, 18000), "Wait1min");
        assert_eq!(at_tick(&mut s, 21600), "ReadyForPlayback");
        // And stays finished.
        assert_eq!(at_tick(&mut s, 30000), "ReadyForPlayback");
    }

    #[test]
    fn any_part_of_a_minute_left_counts_as_a_whole_one() {
        let mut s = scanning(5);
        assert_eq!(at_tick(&mut s, 3600), "Wait5min");
        // One tick past the boundary and the fifth minute is gone.
        assert_eq!(at_tick(&mut s, 3601), "Wait4min");
    }

    #[test]
    fn a_finished_unit_that_is_switched_off_goes_back_to_online() {
        let mut s = scanning(1);
        at_tick(&mut s, 7200);
        assert_eq!(
            s.get("PKscanStatus"),
            Value::Symbol("ReadyForPlayback".into())
        );
        s.set("scanUnitIsActive", Value::Int(0));
        let mut out = Outcome::default();
        assert!(call("resetpeekdisplay", &[], &mut s, &mut out));
        assert_eq!(s.get("PKscanStatus"), Value::Symbol("Online".into()));
    }

    #[test]
    fn coming_back_to_the_chapter_restarts_the_clock() {
        // A scan does not run while the player is in another chapter: the
        // deadline is rebuilt from however many minutes the status says.
        let mut s = scanning(3);
        s.set_all("gTicks", vec![Value::Int(100_000)]);
        let mut out = Outcome::default();
        assert!(call("exitframe", &[], &mut s, &mut out));
        assert_eq!(s.get("gScanFinish"), Value::Int(100_000 + 3 * 3600));
    }

    #[test]
    fn each_chapters_frame_script_answers_only_for_its_own() {
        // Every chapter has an exitFrame and the dispatch chain is
        // first-match-wins, so without a guard Roxy's answers for all four.
        let mut s = scanning(3);
        s.set_all("gChapter", vec![Value::Symbol("MARGARET".into())]);
        s.set_all("currentLocation", vec![Value::Symbol("bedrm_fadeIn".into())]);
        let mut out = Outcome::default();
        // Through the real dispatch chain, which tries Roxy before Margaret.
        assert!(crate::natives::call("exitframe", &[], &mut s, &mut out));
        assert!(
            out.effects.iter().any(|e| matches!(
                e,
                Effect::GoToRoom { room, .. } if room == "bedrm_margaret"
            )),
            "Roxy's frame script answered for Margaret's chapter"
        );
    }

    // -- the bar panel ------------------------------------------------------

    fn panel() -> State {
        let mut s = State::new();
        s.set_all("gChapter", vec![Value::Symbol("ROXY".into())]);
        s.set_all("BarMode", vec![Value::Symbol("setOFF".into())]);
        s.set_all("BarSelection", vec![Value::Symbol("level".into())]);
        s.set_all("BarLevel", vec![Value::Int(4)]);
        s.set_all("BarGain", vec![Value::Int(2)]);
        s.set_all("BarFM", vec![Value::Int(0)]);
        s
    }

    fn panel_press(state: &mut State, button: &str) {
        let mut out = Outcome::default();
        assert!(call(
            &format!("setbar{}", if button == "select" { "selection" } else { "mode" }),
            &[Value::Symbol(button.into())],
            state,
            &mut out
        ));
    }

    fn mode(state: &State) -> String {
        state.get("BarMode").as_str().unwrap_or("").to_string()
    }

    #[test]
    fn power_switches_on_without_leaving_the_mode_it_is_in() {
        let mut s = panel();
        panel_press(&mut s, "power");
        assert_eq!(mode(&s), "setON");
        panel_press(&mut s, "power");
        assert_eq!(mode(&s), "setOFF");
    }

    #[test]
    fn and_mode_switches_between_setting_and_running() {
        let mut s = panel();
        panel_press(&mut s, "mode");
        assert_eq!(mode(&s), "runOFF");
        panel_press(&mut s, "power");
        assert_eq!(mode(&s), "runON");
        panel_press(&mut s, "mode");
        assert_eq!(mode(&s), "setON");
    }

    #[test]
    fn six_five_eight_brings_the_bar_online() {
        let mut s = panel();
        s.set_all("BarLevel", vec![Value::Int(6)]);
        s.set_all("BarGain", vec![Value::Int(5)]);
        s.set_all("BarFM", vec![Value::Int(8)]);
        panel_press(&mut s, "power"); // setOFF -> setON
        panel_press(&mut s, "mode"); // setON  -> runON
        assert_eq!(mode(&s), "runON");
        assert_eq!(s.get("BarOnline"), Value::Int(1));
    }

    #[test]
    fn and_no_other_setting_does() {
        let mut s = panel();
        s.set_all("BarLevel", vec![Value::Int(6)]);
        s.set_all("BarGain", vec![Value::Int(5)]);
        s.set_all("BarFM", vec![Value::Int(7)]);
        panel_press(&mut s, "power");
        panel_press(&mut s, "mode");
        assert_eq!(mode(&s), "runON");
        assert_eq!(s.get("BarOnline"), Value::Void);
    }

    #[test]
    fn the_selection_only_moves_while_the_panel_is_set_and_on() {
        let mut s = panel();
        // Off: nothing moves.
        panel_press(&mut s, "select");
        assert_eq!(s.get("BarSelection"), Value::Symbol("level".into()));

        panel_press(&mut s, "power");
        for want in ["gain", "FM", "level"] {
            panel_press(&mut s, "select");
            assert_eq!(s.get("BarSelection"), Value::Symbol(want.into()));
        }
    }

    // -- the three books ----------------------------------------------------

    fn reading(book: &str, page: i32) -> State {
        let mut s = State::new();
        s.set_all("gChapter", vec![Value::Symbol("ROXY".into())]);
        s.set_all(&format!("currentPageIn{book}"), vec![Value::Int(page)]);
        s
    }

    fn turn(state: &mut State, verb: &str, way: &str) {
        let mut out = Outcome::default();
        assert!(call(verb, &[Value::Symbol(way.into())], state, &mut out));
    }

    #[test]
    fn a_books_pages_are_frames_and_not_a_range() {
        // The diary lives on frames 1, 2, 3, 5 and 6, so the page after three
        // is five. Stepping the number would turn to a frame that is not a page.
        let mut s = reading("Diary", 3);
        turn(&mut s, "setcurrentpageindiary", "next");
        assert_eq!(s.get("currentPageInDiary"), Value::Int(5));
        turn(&mut s, "setcurrentpageindiary", "previous");
        assert_eq!(s.get("currentPageInDiary"), Value::Int(3));
    }

    #[test]
    fn turning_past_either_end_closes_the_book() {
        let mut s = reading("Diary", 6);
        s.set_all("playerIsReadingDreamDiary", vec![Value::Int(1)]);
        turn(&mut s, "setcurrentpageindiary", "next");
        assert_eq!(s.get("playerIsReadingDreamDiary"), Value::Int(0));

        let mut s = reading("Diary", 1);
        s.set_all("playerIsReadingDreamDiary", vec![Value::Int(1)]);
        turn(&mut s, "setcurrentpageindiary", "previous");
        assert_eq!(s.get("playerIsReadingDreamDiary"), Value::Int(0));
    }

    #[test]
    fn and_opening_one_is_not_resuming_it() {
        // Realms starts at frame 0 whatever page it was left on.
        let mut s = reading("Realms", 35);
        let mut out = Outcome::default();
        assert!(call(
            "setplayerisreadingrealms",
            &[Value::Int(1)],
            &mut s,
            &mut out
        ));
        assert_eq!(s.get("currentPageInRealms"), Value::Int(0));
        assert_eq!(s.get("playerIsReadingRealms"), Value::Int(1));
    }

    #[test]
    fn each_book_has_its_own_pages() {
        let mut s = reading("Realms", 7);
        turn(&mut s, "setcurrentpageinrealms", "next");
        assert_eq!(s.get("currentPageInRealms"), Value::Int(19));

        let mut s = reading("BarManual", 3);
        turn(&mut s, "setcurrentpageinbarmanual", "next");
        assert_eq!(s.get("currentPageInBarManual"), Value::Int(4));
    }

    // -- the peek unit's displays -------------------------------------------

    fn amber_status(waves: bool, oscillator: bool, ask: &str) -> String {
        let mut s = State::new();
        s.set_all("gChapter", vec![Value::Symbol("ROXY".into())]);
        s.set_all("psionicWavesPresent", vec![Value::Int(waves as i32)]);
        s.set_all("oscillatorInPlace", vec![Value::Int(oscillator as i32)]);
        let mut out = Outcome::default();
        assert!(call(
            "setpkamberstatus",
            &[Value::Symbol(ask.into())],
            &mut s,
            &mut out
        ));
        s.get("PKamberStatus").as_str().unwrap_or("").to_string()
    }

    #[test]
    fn the_amber_display_never_claims_more_than_the_kit_can_do() {
        // Asking for Online with no waves is Online.
        assert_eq!(amber_status(false, false, "Online"), "Online");
        // With waves it upgrades itself.
        assert_eq!(amber_status(true, true, "Online"), "WaveActivated");
        // But not past the missing oscillator -- the two corrections chain.
        assert_eq!(amber_status(true, false, "Online"), "WaveButIncomplete");
        assert_eq!(amber_status(false, false, "WaveActivated"), "WaveButIncomplete");
    }

    #[test]
    fn and_a_status_it_does_not_know_is_refused() {
        let mut s = State::new();
        s.set_all("gChapter", vec![Value::Symbol("ROXY".into())]);
        s.set_all("PKbarStatus", vec![Value::Symbol("Offline".into())]);
        let mut out = Outcome::default();
        assert!(call(
            "setpkbarstatus",
            &[Value::Symbol("Scanning".into())],
            &mut s,
            &mut out
        ));
        // Left alone, rather than written and then drawn as nothing.
        assert_eq!(s.get("PKbarStatus"), Value::Symbol("Offline".into()));

        assert!(call(
            "setpkbarstatus",
            &[Value::Symbol("activityDetected".into())],
            &mut s,
            &mut out
        ));
        assert_eq!(s.get("PKbarStatus"), Value::Symbol("activityDetected".into()));
    }

    // -- the laptop ---------------------------------------------------------

    fn laptop(to: &str) -> (State, Outcome) {
        let mut s = State::new();
        s.set_all("gChapter", vec![Value::Symbol("ROXY".into())]);
        s.set_all("passwordAttempt", vec![Value::Int(4), Value::Int(2)]);
        let mut out = Outcome::default();
        let arg = match to.parse::<i32>() {
            Ok(n) => Value::Int(n),
            Err(_) => Value::Symbol(to.into()),
        };
        assert!(call("setplayerisusinglaptop", &[arg], &mut s, &mut out));
        (s, out)
    }

    #[test]
    fn typing_a_password_freezes_the_inventory() {
        let (s, _) = laptop("password");
        assert_eq!(s.get("inventoryStatus"), Value::Symbol("cool".into()));
        // And starting up thaws it again.
        let (s, _) = laptop("startUp");
        assert_eq!(s.get("inventoryStatus"), Value::Symbol("hot".into()));
    }

    #[test]
    fn switching_it_off_forgets_what_was_typed() {
        let (s, out) = laptop("off");
        assert!(s.get_all("passwordAttempt").is_empty());
        assert!(out.effects.iter().any(|e| matches!(
            e,
            Effect::PlaySound { name, .. } if name == "computerOff"
        )));

        // Closing the lid does the same, without the sound.
        let (s, _) = laptop("0");
        assert!(s.get_all("passwordAttempt").is_empty());
        assert_eq!(s.get("playerIsUsingLaptop"), Value::Int(0));
    }

    #[test]
    fn and_a_state_it_does_not_know_leaves_it_alone() {
        let mut s = State::new();
        s.set_all("gChapter", vec![Value::Symbol("ROXY".into())]);
        s.set_all("playerIsUsingLaptop", vec![Value::Symbol("warmingUp".into())]);
        let mut out = Outcome::default();
        assert!(call(
            "setplayerisusinglaptop",
            &[Value::Symbol("rebooting".into())],
            &mut s,
            &mut out
        ));
        assert_eq!(s.get("playerIsUsingLaptop"), Value::Symbol("warmingUp".into()));
    }

    #[test]
    fn the_pyramid_says_help_me_before_it_says_anything_else() {
        let mut s = State::new();
        s.set_all("gChapter", vec![Value::Symbol("ROXY".into())]);
        s.set_all("gRandomSeed", vec![Value::Int(7)]);
        s.set_all(
            "pyramidMessagesRemaining",
            vec![
                Value::Symbol("helpMe".into()),
                Value::Symbol("aMessage".into()),
                Value::Symbol("another".into()),
            ],
        );
        let said = |s: &mut State| {
            let mut out = Outcome::default();
            assert!(call("pyramidspeaks", &[], s, &mut out));
            out.effects.iter().find_map(|e| match e {
                Effect::PlaySound { name, .. } => Some(name.clone()),
                _ => None,
            })
        };
        assert_eq!(said(&mut s), Some("helpMe".to_string()));
        // And what it says next is one of the rest, taken out as it goes.
        let second = said(&mut s).expect("a second message");
        assert!(["aMessage", "another"].contains(&second.as_str()));
        let third = said(&mut s).expect("a third message");
        assert_ne!(second, third);
        // Then it has nothing left to say.
        assert_eq!(said(&mut s), None);
    }

    #[test]
    fn ghost_calls_sets_the_rota_rather_than_making_a_noise() {
        // `setProp( oStoryteller.states, #ghostsCalling, suggestedCalls )` --
        // the whole weighted list, which `playDomainEntrySound` then works
        // off the front of. Playing a call here instead meant a room made one
        // noise and fell silent for ever.
        let mut s = State::new();
        s.set_all("gChapter", vec![Value::Symbol("ROXY".into())]);
        let mut out = Outcome::default();
        assert!(call(
            "ghostcalls",
            &[Value::Symbol("Margaret_warm".into()), Value::Symbol("medium".into())],
            &mut s,
            &mut out
        ));

        assert!(out.effects.is_empty(), "ghostCalls played something itself");
        let rota: Vec<String> = s
            .get_all("ghostsCalling")
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect();
        // A warm call lands one turn in three: the ghost and two pauses.
        assert_eq!(rota, ["Margaret", "nobody", "nobody"]);
        assert_eq!(s.get("ghostCallVol").as_int(), Some(180));
    }

    #[test]
    fn an_entry_call_lands_every_turn_and_a_cool_one_less_often() {
        let rota = |kind: &str| {
            let mut s = State::new();
            s.set_all("gChapter", vec![Value::Symbol("ROXY".into())]);
            let mut out = Outcome::default();
            call(
                "ghostcalls",
                &[Value::Symbol(format!("Brice_{kind}")), Value::Symbol("high".into())],
                &mut s,
                &mut out,
            );
            s.get_all("ghostsCalling").len()
        };
        assert_eq!(rota("entry"), 1, "an entry call has no padding");
        assert_eq!(rota("warm"), 3);
        assert_eq!(rota("cool"), 4);
    }

    #[test]
    fn none_empties_the_rota_and_takes_down_a_call_in_progress() {
        let mut s = State::new();
        s.set_all("gChapter", vec![Value::Symbol("ROXY".into())]);
        s.set_all("ghostsCalling", vec![Value::Symbol("Margaret".into())]);
        let mut out = Outcome::default();
        call("ghostcalls", &[Value::Symbol("None".into())], &mut s, &mut out);
        assert!(s.get_all("ghostsCalling").is_empty());
        assert!(out.effects.iter().any(|e| matches!(e, Effect::StopGhostCall)));
    }

    #[test]
    fn the_peek_unit_reports_what_it_was_last_told_and_forgets_it() {
        // `display = getState( #PeekDisplay ) : setState( #PeekDisplay, #None )`
        // -- an alert is consumed by being looked at, so the unit does not
        // keep showing the same thing.
        let mut s = State::new();
        s.set_all("gChapter", vec![Value::Symbol("ROXY".into())]);
        s.set_all("PeekDisplay", vec![Value::Symbol("BARstartup".into())]);
        let mut out = Outcome::default();
        assert!(call("usepeekunit", &[], &mut s, &mut out));
        assert!(s.get("PeekDisplay").is_symbol("None"));

        // It says the machine is running, waits to be dismissed, and then
        // settles to having nothing to report.
        let states: Vec<String> = out
            .effects
            .iter()
            .filter_map(|e| match e {
                Effect::SetState { key, value } if key == "PKbarStatus" => {
                    value.as_str().map(str::to_string)
                }
                _ => None,
            })
            .collect();
        assert_eq!(states, ["Online", "noActivity"]);
        assert!(out.effects.iter().any(|e| matches!(e, Effect::WaitForClick)));
    }

    #[test]
    fn a_caught_haunt_plays_back_and_comes_off_the_list() {
        // Every camera haunt has the same shape: the clip between a fade in
        // and a fade out, the bar recording that it caught something, and the
        // haunt struck off what is still to be seen.
        let mut s = State::new();
        s.set_all("gChapter", vec![Value::Symbol("ROXY".into())]);
        s.set_all("PeekDisplay", vec![Value::Symbol("ghostKnife".into())]);
        let mut out = Outcome::default();
        call("usepeekunit", &[], &mut s, &mut out);

        let frames: Vec<String> = out
            .effects
            .iter()
            .filter_map(|e| match e {
                Effect::SpriteCastFromTable { key, .. } => Some(key.clone()),
                _ => None,
            })
            .collect();
        // The screen starts blank -- `set the castNum of camSprite =
        // PkVideoNormal[#PkNone]` once the unit is up -- then the readout's
        // blank page and the three status lights are placed, and then the
        // recording plays between its two fades.
        assert_eq!(
            frames,
            [
                "PkNone", "None", "3", "2", "2",
                "PkFadeIn", "PkKitchenGhost", "PkFadeOut"
            ]
        );

        assert!(out.effects.iter().any(|e| matches!(
            e,
            Effect::SetState { key, value } if key == "PKbarStatus" && value.is_symbol("ActivityDetected")
        )));
        assert!(out.effects.iter().any(|e| matches!(
            e,
            Effect::TrimState { key, item } if key == "cameraFeedbackRemaining" && item.is_symbol("ghostKnife")
        )));
    }

    #[test]
    fn a_status_page_is_named_for_its_machine_and_its_reading() {
        // `#PKscanStatus` of `#Wait3min` is the page `#scanWait3min`, which is
        // why `#peekText` has twenty-six entries and no dispatch table.
        let mut s = State::new();
        s.set_all("gChapter", vec![Value::Symbol("ROXY".into())]);
        s.set_all("PKscanStatus", vec![Value::Symbol("Wait3min".into())]);
        s.set_all("PeekDisplay", vec![Value::Symbol("scanStatus".into())]);
        let mut out = Outcome::default();
        call("usepeekunit", &[], &mut s, &mut out);
        assert!(out.effects.iter().any(|e| matches!(
            e,
            Effect::SpriteCastFromTable { table, key, .. }
                if table == "peekText" && key == "scanWait3min"
        )));
    }

    #[test]
    fn taking_the_peek_out_of_the_bar_opens_it() {
        // `useInventory` ends with `if whichItem = #PeekUnit then usePeekUnit`.
        // It is the one item that does something when it is taken up rather
        // than waiting to be used on the scene.
        let mut s = State::new();
        s.set_all("gChapter", vec![Value::Symbol("ROXY".into())]);
        let out = crate::script::run(&["useInventory( #PeekUnit )".to_string()], &mut s);
        assert_eq!(s.item_in_use(), Some("PeekUnit"));
        assert!(out.effects.iter().any(|e| matches!(e, Effect::WaitForClick)));

        // Anything else just goes on the cursor.
        let mut s = State::new();
        s.set_all("gChapter", vec![Value::Symbol("ROXY".into())]);
        let out = crate::script::run(&["useInventory( #Crowbar )".to_string()], &mut s);
        assert!(!out.effects.iter().any(|e| matches!(e, Effect::WaitForClick)));
    }

    #[test]
    fn the_telephone_rings_when_the_house_has_shown_everything_it_has() {
        // `testForPsionicWaves` is the gate to the second half of the game:
        // every camera haunt seen, the oscillator in the AMBER device, and at
        // least one door residue listened to. Then the phone rings, and
        // answering it is what activates the headgear.
        let mut s = State::new();
        s.set_all("gChapter", vec![Value::Symbol("ROXY".into())]);
        s.set_all("ghostlyPhoneCall", vec![Value::Symbol("notyet".into())]);
        s.set_all("hauntsRemaining", vec![Value::Symbol("phoneMessage".into())]);
        s.set_all(
            "cameraFeedbackRemaining",
            ["KdKnob", "crazyDR"].iter().map(|h| Value::Symbol((*h).into())).collect(),
        );
        s.set_all(
            "tonalResidueRemaining",
            ["PkPatioScan", "PkBathroomScan", "Pk40sScan", "PkBoathouseScan"]
                .iter()
                .map(|r| Value::Symbol((*r).into()))
                .collect(),
        );
        s.set_all("oscillatorInPlace", vec![Value::Int(0)]);

        let ring = |s: &mut State| {
            let mut out = Outcome::default();
            call("testforpsionicwaves", &[], s, &mut out);
            s.get("ghostlyPhoneCall").is_symbol("ringingNow")
        };
        assert!(!ring(&mut s), "rang with two haunts still to see");

        // Watch the last two haunts back on the PeeK.
        for haunt in ["KdKnob", "crazyDR"] {
            s.set_all("PeekDisplay", vec![Value::Symbol(haunt.into())]);
            let mut out = Outcome::default();
            call("usepeekunit", &[], &mut s, &mut out);
            for e in &out.effects {
                if let Effect::TrimState { key, item } = e {
                    s.trim_item(key, item);
                }
            }
        }
        assert!(s.get_all("cameraFeedbackRemaining").is_empty());
        assert!(!ring(&mut s), "rang without the oscillator");

        s.set_all("oscillatorInPlace", vec![Value::Int(1)]);
        assert!(!ring(&mut s), "rang without a residue played");

        // And one residue listened to.
        s.trim_item("tonalResidueRemaining", &Value::Symbol("PkPatioScan".into()));
        assert!(ring(&mut s), "everything done and the phone stayed silent");
        assert_eq!(s.get("psionicWavesPresent").as_int(), Some(1));
    }

    #[test]
    fn putting_the_peek_away_is_what_asks() {
        // `stowInventory` runs the test, and only for the PeeK unit -- which
        // is the right moment, because the PeeK is how all three of those
        // things are seen.
        let mut s = State::new();
        s.set_all("gChapter", vec![Value::Symbol("ROXY".into())]);
        s.set_all("ghostlyPhoneCall", vec![Value::Symbol("notyet".into())]);
        s.set_all("hauntsRemaining", vec![Value::Symbol("phoneMessage".into())]);
        s.set_all("cameraFeedbackRemaining", Vec::new());
        s.set_all("tonalResidueRemaining", vec![Value::Symbol("Pk40sScan".into())]);
        s.set_all("oscillatorInPlace", vec![Value::Int(1)]);

        // Stowing something else asks nothing.
        s.add_inventory("Crowbar");
        s.set("itemInUse", Value::Symbol("Crowbar".into()));
        crate::script::run(&["stowInventory( #Crowbar )".to_string()], &mut s);
        assert!(s.get("ghostlyPhoneCall").is_symbol("notyet"));

        s.add_inventory("PeekUnit");
        s.set("itemInUse", Value::Symbol("PeekUnit".into()));
        crate::script::run(&["stowInventory( #PeekUnit )".to_string()], &mut s);
        assert!(s.get("ghostlyPhoneCall").is_symbol("ringingNow"));
    }

    #[test]
    fn a_chapter_names_the_room_to_arrive_in() {
        // `enterNewDomain( oStoryteller, string(#Margaret), 15 )`. The index
        // is within the chapter, so Margaret's is entered at `bedrm_C4` and
        // the way back arrives at `HallLivingRmEntry` -- not at whatever each
        // chapter's schema calls its start, which is where a new game begins.
        let mut s = State::new();
        let out = crate::script::run(
            &["enterNewDomain( oStoryteller, string(#Margaret), 15 )".to_string()],
            &mut s,
        );
        assert_eq!(out.new_domain.as_deref(), Some("Margaret"));
        assert_eq!(out.new_domain_room, Some(15));

        let mut s = State::new();
        let out = crate::script::run(
            &["enterNewDomain( oStoryteller, string(#ROXY), 12 )".to_string()],
            &mut s,
        );
        assert_eq!(out.new_domain.as_deref(), Some("ROXY"));
        assert_eq!(out.new_domain_room, Some(12));
    }
}

#[cfg(test)]
mod homecoming_tests {
    use super::*;

    fn coming_home(has_headgear: bool) -> State {
        let mut state = State::new();
        state.set_all("ghostsRemaining", vec![
            Value::Symbol("Margaret".into()),
            Value::Symbol("Brice".into()),
            Value::Symbol("Edwin".into()),
        ]);
        state.set_all("hauntsRemaining", vec![Value::Symbol("stairsGhost".into())]);
        // The freezer hands back what the portal stashed, and the portal turns
        // the vision off on the way through.
        state.set_all("AMBERVISION", vec![
            Value::Symbol("off".into()),
            Value::Symbol("on".into()),
        ]);
        state.set(
            "playerHasHeadgear",
            if has_headgear { Value::Symbol("inUse".into()) } else { Value::Int(0) },
        );
        let mut out = Outcome::default();
        call("closechapter", &[Value::Symbol("DarkUp_40sReentry".into())], &mut state, &mut out);
        state
    }

    /// Coming home from a chapter leaves the vision on, so the other portals
    /// can still be opened.
    ///
    /// A departure from the disc, which never turns it back on -- and without
    /// which the game is unfinishable after the first chapter, because the
    /// portals are guarded on the vision and the only thing that switches it
    /// on is a telephone that rings once.
    #[test]
    fn the_vision_survives_a_chapter() {
        let state = coming_home(true);
        assert_eq!(
            state.get("AMBERVISION"),
            Value::Symbol("on".into()),
            "came home from a chapter with the vision off and no way to turn it on"
        );
        // And the chapter really was closed out, so this is not passing on a
        // handler that did nothing at all.
        assert!(
            !state.get_all("ghostsRemaining").contains(&Value::Symbol("Margaret".into())),
            "the ghost was not retired; the handler did not run"
        );
    }

    /// Without the headgear there is no vision to restore.
    #[test]
    fn the_vision_needs_the_headgear() {
        let state = coming_home(false);
        assert_eq!(
            state.get("AMBERVISION"),
            Value::Symbol("off".into()),
            "the vision came on without the headgear"
        );
    }
}

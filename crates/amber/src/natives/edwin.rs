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

/// The whirligig's spin-up and steady-loop movies for a wind direction.
///
/// `#gigStartMovies: [#n: 966, #s: 967, #e: 965, #W: 968]`
/// `#gigLoopMovies:  [#n: 970, #s: 971, #e: 969, #W: 972]`
///
/// Keyed without regard to case, because the tables spell three of the four
/// directions in lower case and the weather vane spells three of them in
/// upper. Lingo does not care and neither can this.
fn whirligig_movies(facing: &str) -> (u32, u32) {
    match facing.trim_start_matches('#').to_ascii_lowercase().as_str() {
        "s" => (967, 971),
        "e" => (965, 969),
        "w" => (968, 972),
        _ => (966, 970),
    }
}

pub fn call(name: &str, args: &[Value], state: &mut State, out: &mut Outcome) -> bool {
    // Arguments and effects are unused by some chapters until more handlers
    // land here; the signature is uniform so the dispatcher stays simple.
    let _ = (args, &out, &state);
    match name {
        // on enableGust
        //   setState(oStoryteller, #gustEnabled, 1)
        // on initWhirligig
        //   if getState( #Wind ) <> #None then return
        //   disableGust : disableSongs
        //   windDirection = getState( #weatherVane )
        //   gigLoopMovies = getProp( oPuppeteer.frames, #gigLoopMovies )
        //   puppetSprite 44, 1
        //   set the visible   of sprite 44 = 0
        //   set the castNum   of sprite 44 = getProp( gigLoopMovies, windDirection )
        //   set the movieRate of sprite 44 = 0
        //   updateStage
        //   set the loc of sprite 44 = point(336, 184) + gOriginPoint
        //   puppetSprite 45, 1
        //   gigStartMovies = getProp( oPuppeteer.frames, #gigStartMovies )
        //   set the castNum of sprite 45 = getProp( gigStartMovies, windDirection )
        //   set the loc     of sprite 45 = point(336, 184) + gOriginPoint
        //   updateStage
        //
        // The whirligig is two movies stacked on the two film channels: the
        // spin-up on 45 and the steady loop on 44, each with a version for
        // every wind direction. Nothing is shown yet -- 44 is hidden and
        // stopped -- because `startWhirligig` runs the pair in order.
        //
        // Note the keys. The movie tables are `[#n, #s, #e, #W]` while the
        // vane holds `#n`, `#E`, `#S`, `#W`. Lingo compares symbols without
        // regard to case, so the mismatch never mattered there and would have
        // silently found nothing here.
        "initwhirligig" => {
            // `if getState( #Wind ) <> #None then return` -- the jump is taken
            // when the comparison is *false*, so this runs while the air is
            // still and does nothing once the wind is up. I had read it the
            // other way round, which made the whirligig refuse to work until
            // there was already a wind, and nothing else in the chapter starts
            // one: the vane only steers a wind that is blowing, `setSail` only
            // reads it. So Edwin's chapter could not be started at all.
            if !state.get("Wind").is_symbol("None") {
                return true;
            }
            call("disablegust", &[], state, out);
            call("disablesongs", &[], state, out);

            let facing = state.get("weatherVane").as_symbol().unwrap_or("n").to_string();
            let (start, spin) = whirligig_movies(&facing);

            out.effects.push(Effect::PuppetSprite { channel: 44, on: true });
            out.effects.push(Effect::SpriteCast { channel: 44, cast: spin });
            out.effects.push(Effect::SpriteVisible { channel: 44, visible: false });
            out.effects.push(Effect::SpriteLoc { channel: 44, x: 336, y: 184 });
            out.effects.push(Effect::PuppetSprite { channel: 45, on: true });
            out.effects.push(Effect::SpriteCast { channel: 45, cast: start });
            out.effects.push(Effect::SpriteLoc { channel: 45, x: 336, y: 184 });
            out.redraw = true;
        }

        // on startWhirligig
        //   if getState( #Wind ) <> #None then return
        //   setState( #showMontage, 1 )
        //   cursorOff : killSongs
        //   setLoop( #steadyWind, 0 )
        //   setState( #Wind, getState( #weatherVane ) )
        //   startSound #WGstart
        //   set the movieRate of sprite 45 = 1
        //   repeat while the movieRate of sprite 45 <> 1: updateStage
        //   repeat while the movieRate of sprite 45 <> 0: updateStage
        //   set the loc     of sprite 45 = point(-1000, -1000)
        //   set the visible of sprite 44 = 1
        //
        // Runs the spin-up once, waits for it to stop of its own accord, then
        // throws that sprite a thousand pixels off stage and shows the loop
        // underneath. The two waits are the original's way of blocking on a
        // movie: one until it has started, one until it has finished.
        //
        // The wind is set from the vane here rather than when the vane turns,
        // so a vane turned while the whirligig is still gives the direction
        // the wind picks up in.
        "startwhirligig" => {
            // The same guard as `initWhirligig`, read the same way: it runs in
            // still air and returns once the wind is already up.
            if !state.get("Wind").is_symbol("None") {
                return true;
            }
            state.set("showMontage", Value::Int(1));
            out.effects.push(Effect::CursorOff);
            call("killsongs", &[], state, out);
            out.effects.push(Effect::StartLoop {
                name: "steadyWind".into(),
                volume: Some(0),
            });
            let facing = state.get("weatherVane").as_symbol().unwrap_or("n").to_string();
            state.set("Wind", Value::Symbol(facing));

            out.effects.push(Effect::PlaySound {
                name: "WGstart".into(),
                loudness: None,
            });
            out.effects.push(Effect::PlayVideo(None));
            out.effects.push(Effect::WaitForVideo);
            out.effects.push(Effect::SpriteLoc { channel: 45, x: -1000, y: -1000 });
            out.effects.push(Effect::SpriteVisible { channel: 44, visible: true });
            out.redraw = true;

            // The tail. The wind comes up under the film -- the loop is faded
            // in behind it and left running at full -- and the last thing the
            // handler does is take the montage down again. Every hotspot in
            // the chapter that is worth clicking is guarded on `showMontage`
            // being 0, so without this line the whirligig leaves the world
            // read-only: you can walk, and nothing else.
            out.effects.push(Effect::StartLoop {
                name: "steadyWind".into(),
                volume: Some(200),
            });
            out.effects.push(Effect::StartLoop {
                name: "steadyWind".into(),
                volume: Some(255),
            });
            out.effects.push(Effect::WaitTicks(30));
            out.effects.push(Effect::PlaySound {
                name: "inYourSails".into(),
                loudness: None,
            });
            out.effects.push(Effect::SetState {
                key: "showMontage".into(),
                value: Value::Int(0),
            });
        }

        // on setSail
        //   boatPos = getState( #boatPosition )
        //   windDirection = getState( #Wind )
        //   if boatPos = #forward and windDirection = #E then
        //     setState( #boatPosition, #backward ) : startSound #boatMove
        //   if boatPos = #backward and windDirection = #W then
        //     setState( #boatPosition, #forward )
        //     if getState( #teddyLocation ) = #waiting then
        //       setState( #teddyLocation, #onAnchor )
        //     startSound #boatMove
        //   pushVideo : wait #videoStop
        //
        // The boat goes where the wind sends it, and only where the wind sends
        // it: an east wind pushes it back, a west wind brings it forward, and
        // any other wind does nothing. So this is the weather vane's puzzle
        // seen from the other end -- `setWeatherVane` decides which way this
        // works, two rooms away.
        //
        // And bringing the boat forward while Teddy is waiting puts Teddy on
        // the anchor, which is the only place that transition happens.
        "setsail" => {
            let forward = state.get("boatPosition").is_symbol("forward");
            let wind = state.get("Wind");

            if forward && wind.is_symbol("E") {
                state.set_all("boatPosition", vec![Value::Symbol("backward".into())]);
            } else if !forward && wind.is_symbol("W") {
                state.set_all("boatPosition", vec![Value::Symbol("forward".into())]);
                if state.get("teddyLocation").is_symbol("waiting") {
                    state.set_all("teddyLocation", vec![Value::Symbol("onAnchor".into())]);
                }
            } else {
                // Becalmed, or the wind is against it.
                return true;
            }
            out.effects.push(Effect::PlaySound {
                name: "boatMove".into(),
                loudness: None,
            });
            out.effects.push(Effect::PlayVideo(None));
            out.effects.push(Effect::WaitForVideo);
            out.redraw = true;
        }

        // on driveTheCar
        //   cursorOff
        //   setWaffleTracks #None
        //   mList = #alone
        //   if getState( #chippyLocation ) = #inCar then
        //     mList = #chippy : loadMultiframes #chipHead
        //     ... a head clip on sprite 39 at point(454, 365) ...
        //   currentTrack = getState( #currentTrack )
        //   if currentTrack = #BM then
        //     if chippyLocation = #inCar then film = #BM_withChippy else film = #BM
        //   if currentTrack = #CM then
        //     if chippyLocation = #inCar   then film = #CM_missRamp
        //     if boatPosition   = #forward then film = #CM_anchorDown
        //     if teddyLocation  = #onAnchor then film = #CM_teddyRescue
        //     else                              film = #CM_emptyAnchor
        //   ... trackLoop under it, then the film ...
        //
        // Setting the car going, and the one place the chapter's puzzles meet.
        // On the middle track of C the film depends on where the boat is and
        // whether Teddy is on the anchor -- which is the weather vane's wind,
        // through `setSail`, arriving three handlers later.
        //
        // The tests are sequential `if`s assigning one local, so the **last**
        // match wins rather than the first: the boat being forward and Teddy
        // being on the anchor are both true after a successful rescue, and it
        // is the rescue that plays.
        //
        // Driving also clears `#waffleTracks`, so the record of where the car
        // has been starts again with the journey.
        "drivethecar" => {
            out.effects.push(Effect::CursorOff);
            call("setwaffletracks", &[Value::Symbol("None".into())], state, out);

            let chippy = state.get("chippyLocation").is_symbol("inCar");
            let track = state.get("currentTrack");
            let track = track.as_str().unwrap_or_default().trim_start_matches('#');

            let film = if track.eq_ignore_ascii_case("BM") {
                if chippy { "BM_withChippy" } else { "BM" }
            } else if track.eq_ignore_ascii_case("CM") {
                let mut which = "CM_emptyAnchor";
                if chippy {
                    which = "CM_missRamp";
                }
                if state.get("boatPosition").is_symbol("forward") {
                    which = "CM_anchorDown";
                }
                if state.get("teddyLocation").is_symbol("onAnchor") {
                    which = "CM_teddyRescue";
                }
                which
            } else {
                // Every other track has one film, named after itself.
                track
            };

            out.effects.push(Effect::StartLoop {
                name: "trackLoop".into(),
                volume: Some(120),
            });
            out.effects.push(Effect::PlayVideo(Some(film.to_string())));
            out.effects.push(Effect::WaitForVideo);
            out.effects.push(Effect::StopLoop {
                name: "trackLoop".into(),
                fade: false,
            });

            // And then the car is somewhere else. Each stretch of track ends
            // at one of the four tunnel mouths --
            //
            // ```text
            // if getPos( [#BL, #BR],           whichTrack ) then goTo #teN_fwd
            // if getPos( [#CL, #CM_missRamp],  whichTrack ) then goTo #teS_fwd
            // if getPos( [#AR, #CR],           whichTrack ) then goTo #teE_fwd
            // if getPos( [#AM, #AL],           whichTrack ) then goTo #teW_fwd
            // ```
            //
            // -- except the one that ends the chapter. Driving the middle of
            // the C track with the teddy hanging on the anchor is the rescue:
            // the film plays, `teddyGetsOut` puts him in the car, and it
            // drives out through `#car_domainExit` and home.
            //
            // None of this was here. The port chose the film and stopped, so
            // every drive played its stretch of track and left the car where
            // it started, and the chapter could not be finished or even moved
            // around in.
            const MOUTHS: [(&str, &[&str]); 4] = [
                ("teN_fwd", &["BL", "BR"]),
                ("teS_fwd", &["CL", "CM_missRamp"]),
                ("teE_fwd", &["AR", "CR"]),
                ("teW_fwd", &["AM", "AL"]),
            ];
            // The rescue, and the end of the chapter. `teddyGetsIn` opens
            // the door and puts him aboard; then the car drives out through
            // `#car_domainExit`, plays that room's film, and hands back to
            // Roxy's house at `Edwin_reentry`.
            if film.eq_ignore_ascii_case("CM_teddyRescue") {
                state.set("teddyLocation", Value::Symbol("inCar".into()));
                for beat in ["carDoorOpen", "carDoorOpen"] {
                    out.effects.push(Effect::PlaySound {
                        name: beat.into(),
                        loudness: None,
                    });
                }
                out.effects.push(Effect::GoToRoom {
                    room: "car_domainExit".into(),
                    transition: Some("fadeIn".into()),
                });
                out.effects.push(Effect::PlaySound {
                    name: "carDoorClose".into(),
                    loudness: None,
                });
                out.effects.push(Effect::WaitTicks(30));
                out.effects.push(Effect::WaitForSound("carDoorClose".into()));
                out.effects.push(Effect::SuspendSounds { fade: false });
                out.effects.push(Effect::PlayVideo(None));
                out.effects.push(Effect::WaitForVideo);
                out.effects.push(Effect::SetState {
                    key: "showMontage".into(),
                    value: Value::Int(1),
                });
                out.effects.push(Effect::StopVideo);
                out.effects.push(Effect::PlaySound {
                    name: "toRoxy".into(),
                    loudness: None,
                });
                out.new_domain = Some("ROXY".into());
                out.new_domain_room = None;
                out.redraw = true;
                return true;
            }

            // Otherwise the car comes to rest, and where depends on what it
            // drove. The three trunk lines end at their own hub and the car
            // waits there for the next turn.
            if let Some(hub) = ["A", "B", "c"]
                .iter()
                .find(|t| t.eq_ignore_ascii_case(film))
                .map(|t| format!("hub_{t}"))
            {
                state.set_all("carLocation", vec![Value::Symbol(hub)]);
                state.set_all("currentTrack", vec![Value::Symbol("main".into())]);
                state.set("showMontage", Value::Int(1));
                out.redraw = true;
                return true;
            }
            state.set_all("carLocation", vec![Value::Symbol("standingBy".into())]);
            state.set_all("currentTrack", vec![Value::Symbol("main".into())]);

            // Four of the spurs go nowhere: the film runs out and the car is
            // back where it started, on montage 3 -- which is the state the
            // `pointer` hotspot in `car_inside` is guarded on, so the only
            // thing left to do is set off again.
            const DEAD_ENDS: [&str; 4] = [
                "CM_anchorDown",
                "CM_emptyAnchor",
                "BM_withChippy",
                "BM_noChippy",
            ];
            if DEAD_ENDS.iter().any(|t| t.eq_ignore_ascii_case(film)) {
                out.effects.push(Effect::StopVideo);
                state.set("showMontage", Value::Int(3));
                out.redraw = true;
                return true;
            }

            if !film.eq_ignore_ascii_case("main") {
                out.effects.push(Effect::StartLoop {
                    name: "underWater".into(),
                    volume: None,
                });
            }
            if let Some((mouth, _)) = MOUTHS
                .iter()
                .find(|(_, tracks)| tracks.iter().any(|t| t.eq_ignore_ascii_case(film)))
            {
                out.destination = Some((*mouth).to_string());
                out.transition = Some("fadeIn".into());
            }
            out.redraw = true;
        }

        // on pullOnChippy
        //   cursorOff
        //   lsChippyPleas = getProp( oStoryteller.states, #chippyPleas )
        //   if getPos( lsChippyPleas, #pullMyFinger ) = 2 then
        //     ... a grunt clip from #chippyGrunts on sprite 44 ...
        //   else
        //     trimState( #chippyPleas, #pullMyFinger )
        //     startSound #puppetFart : wait #soundStop, #puppetFart
        //     startSound #edwinLaugh : wait #soundStop, #edwinLaugh
        //
        // The gag, once. Chippy asks you to pull his finger; you do; he does
        // the obvious and Edwin laughs. Pulling again gets a grunt, because
        // `#pullMyFinger` has been taken off his list of things to ask for.
        //
        // The guard is not "is it in the list" but "is it *second* in it" --
        // he only has the joke ready once he has got past whatever he wanted
        // first, and once it has been used the list shortens and the test
        // stops matching. Two different ways of saying "already done", in one
        // handler.
        "pullonchippy" => {
            out.effects.push(Effect::CursorOff);
            let at = state
                .get_all("chippyPleas")
                .iter()
                .position(|p| p.is_symbol("pullMyFinger"));
            if at == Some(1) {
                // Second in the list: the joke is ready.
                state.trim_item("chippyPleas", &Value::Symbol("pullMyFinger".into()));
                for line in ["puppetFart", "edwinLaugh"] {
                    out.effects.push(Effect::PlaySound {
                        name: line.into(),
                        loudness: None,
                    });
                    out.effects.push(Effect::WaitForSound(line.into()));
                }
                out.redraw = true;
            } else {
                // Anything else and he just grunts.
                out.effects.push(Effect::PlayVideo(None));
                out.effects.push(Effect::WaitForVideo);
            }
        }

        // on chooseTrack whichDirection
        //   cursorOff
        //   currentLocation = getState( #carLocation )
        //   if currentLocation <> #enRoute then
        //     setLoop( #trackLoop, 120 )
        //     if #left   then hub_main -> #c, pushQT(0, 223)   else <hub>L, pushQT(0, 178)
        //     if #middle then hub_main -> #B, pushQT(225, 448) else <hub>M, pushQT(180, 358)
        //     if #right  then hub_main -> #A, pushQT(450, 675) else <hub>R, pushQT(360, 540)
        //     wait #videoStop : setLoop( #trackLoop, 0 )
        //     setState( #currentTrack, newTrack )
        //     setState( #carLocation, #enRoute )
        //     setState( #showMontage, 0 )
        //   else
        //     currentTrack = getState( #currentTrack )
        //     destinations = [#c: #B, #B: #A, #AL: #AM, #AM: #AR, ...]   -- for #right
        //                    [#B: #c, #A: #B, #AM: #AL, #AR: #AM, ...]   -- for #left
        //     myDestination = getaProp( destinations, currentTrack )
        //     if voidp( myDestination ) then return
        //     segments = [#B: [0, 448], #BM: [0, 358], ...]              -- for #right
        //                [#B: [450, 900], #BM: [360, 720], ...]          -- for #left
        //     ... play that stretch, or the whole film if it has no stretch ...
        //     newTrack = myDestination
        //
        // Edwin's car and its tracks, which work two ways depending on where
        // the car is.
        //
        // **At a hub** the three directions each lead somewhere and the film
        // is a third of `waffle.mov` -- the main hub gets 0-223, 225-448 and
        // 450-675, and the three lettered hubs share a shorter set at 0-178,
        // 180-358 and 360-540. Six stretches of one film, which is the same
        // trick the music boxes use in entry 70.
        //
        // **Already on a track** it is a lookup instead, and the two tables
        // are each other backwards: right takes `#c` to `#B` and left takes
        // `#B` to `#c`. A direction with no entry for the track you are on
        // does nothing at all, which is how the dead ends are expressed --
        // there is no "you cannot go that way", only an absence.
        //
        // The left film is the same stretches with 450 and 360 added, so the
        // one film holds the journey both ways round and left is simply the
        // back half.
        "choosetrack" => {
            const HUBS: [(&str, [&str; 3]); 4] = [
                ("hub_main", ["c", "B", "A"]),
                ("hub_A", ["AL", "AM", "AR"]),
                ("hub_B", ["BL", "BM", "BR"]),
                ("hub_C", ["CL", "CM", "CR"]),
            ];
            const MAIN_SEGMENTS: [(u32, u32); 3] = [(0, 223), (225, 448), (450, 675)];
            const SPUR_SEGMENTS: [(u32, u32); 3] = [(0, 178), (180, 358), (360, 540)];
            // Each other backwards, and read as (from, to) for a right turn.
            const RIGHT: [(&str, &str); 8] = [
                ("c", "B"),
                ("B", "A"),
                ("AL", "AM"),
                ("AM", "AR"),
                ("BL", "BM"),
                ("BM", "BR"),
                ("CL", "CM"),
                ("CM", "CR"),
            ];
            // The stretch played on arriving at a track, when it has one.
            // Both written out: the left set is *nearly* the right set with
            // 450 and 360 added, and `#B` ends at 900 rather than the 898 that
            // would give. Deriving one from the other is exactly the mistake
            // the weather vane's table invited in entry 84, and this time the
            // test caught it.
            const RIGHT_SEGMENTS: [(&str, (u32, u32)); 4] = [
                ("B", (0, 448)),
                ("BM", (0, 358)),
                ("AM", (0, 358)),
                ("CM", (0, 358)),
            ];
            const LEFT_SEGMENTS: [(&str, (u32, u32)); 4] = [
                ("B", (450, 900)),
                ("BM", (360, 720)),
                ("AM", (360, 720)),
                ("CM", (360, 720)),
            ];

            let Some(way) = args
                .first()
                .and_then(Value::as_str)
                .map(|d| d.trim_start_matches('#').to_ascii_lowercase())
            else {
                return true;
            };
            let Some(turn) = ["left", "middle", "right"]
                .iter()
                .position(|d| *d == way)
            else {
                return true;
            };

            out.effects.push(Effect::CursorOff);
            let at = state.get("carLocation");

            if !at.is_symbol("enRoute") {
                let Some((hub, tracks)) = HUBS.iter().find(|(h, _)| at.is_symbol(h)) else {
                    return true;
                };
                let (from, to) = if *hub == "hub_main" {
                    MAIN_SEGMENTS[turn]
                } else {
                    SPUR_SEGMENTS[turn]
                };
                out.effects.push(Effect::StartLoop {
                    name: "trackLoop".into(),
                    volume: Some(120),
                });
                out.effects.push(Effect::PlayVideoSegment { from, to });
                out.effects.push(Effect::WaitForVideo);
                out.effects.push(Effect::StopLoop {
                    name: "trackLoop".into(),
                    fade: false,
                });
                state.set_all("currentTrack", vec![Value::Symbol(tracks[turn].into())]);
                state.set_all("carLocation", vec![Value::Symbol("enRoute".into())]);
                state.set("showMontage", Value::Int(0));
                out.redraw = true;
                return true;
            }

            // Already rolling. The middle is not a turn, so there is no table
            // for it and nothing happens.
            if turn == 1 {
                return true;
            }
            let held = state.get("currentTrack");
            let held = held.as_str().unwrap_or_default().trim_start_matches('#');
            let going_right = turn == 2;
            let moved = RIGHT.iter().find_map(|(a, b)| {
                let (from, to) = if going_right { (a, b) } else { (b, a) };
                from.eq_ignore_ascii_case(held).then_some(*to)
            });
            // No entry is how a dead end is written: nothing happens.
            let Some(moved) = moved else { return true };

            out.effects.push(Effect::StartLoop {
                name: "trackLoop".into(),
                volume: Some(120),
            });
            let table = if going_right { RIGHT_SEGMENTS } else { LEFT_SEGMENTS };
            if let Some((_, (from, to))) =
                table.iter().find(|(t, _)| t.eq_ignore_ascii_case(moved))
            {
                out.effects.push(Effect::PlayVideoSegment {
                    from: *from,
                    to: *to,
                });
            } else {
                out.effects.push(Effect::PlayVideo(None));
            }
            out.effects.push(Effect::WaitForVideo);
            out.effects.push(Effect::StopLoop {
                name: "trackLoop".into(),
                fade: false,
            });
            state.set_all("currentTrack", vec![Value::Symbol(moved.into())]);
            out.redraw = true;
        }

        // on chippySpeaks howLikely
        //   if integerp( howLikely ) then
        //     highRoll = howLikely, clamped to 1..6
        //   else
        //     highRoll = 6
        //   if random(6) <= highRoll then
        //     cursorOff
        //     pleaList = getProp( oStoryteller.states, #chippyPleas )
        //     newPlea = getAt( pleaList, 1 )
        //     ... put the matching clip on sprite 44 and play it ...
        //
        // Chippy pipes up, sometimes. The argument is how likely out of six,
        // clamped, and defaulting to six -- certain -- when it is not a number
        // at all. So `chippySpeaks 2` is a one-in-three chance and
        // `chippySpeaks` on its own always speaks.
        //
        // He works through `#chippyPleas` from the front rather than at
        // random, so the order he asks for things in is fixed even though
        // whether he asks at all is not.
        "chippyspeaks" => {
            let likely = args
                .first()
                .and_then(Value::as_int)
                .map_or(6, |n| n.clamp(1, 6));
            if roll(state, 6) > likely {
                return true;
            }
            let pleas = state.get_all("chippyPleas").to_vec();
            let Some(next) = pleas.first().cloned() else {
                return true;
            };
            out.effects.push(Effect::CursorOff);
            if let Some(name) = next.as_str() {
                out.effects.push(Effect::PlaySound {
                    name: name.trim_start_matches('#').into(),
                    loudness: None,
                });
            }
            out.effects.push(Effect::WaitForVideo);

            // And then the list turns over: the one at the back comes to the
            // front, so the next click gets the next thing he has to say.
            // It is the rotation that makes the finger joke reachable --
            // `pullOnChippy` wants `#pullMyFinger` lying *second*, and three
            // turns of this list is what puts it there.
            if let Some(last) = state.get_all("chippyPleas").last().cloned() {
                state.set("chippyPleas", last);
            }
            // Anything but the first plea comes with the finger wiggle, which
            // is a second film on the same channel, lower down the frame.
            if !next.is_symbol("helpMe") {
                out.effects.push(Effect::PuppetSprite {
                    channel: 44,
                    on: true,
                });
                out.effects.push(Effect::SpriteLoc {
                    channel: 44,
                    x: 305,
                    y: 336,
                });
                out.effects.push(Effect::PlayVideo(Some("fingerWiggle".into())));
                out.effects.push(Effect::WaitForVideo);
            }
            out.redraw = true;
        }

        // on carComments
        //   if getState( #chippyLocation ) <> #inCar then return
        //   if inState( #utterancesRemaining, #homeEdwin ) then thisComment = #homeEdwin
        //   else if inState( #utterancesRemaining, #letsGo ) then thisComment = #letsGo
        //   else thisComment = #joyRide
        //   assertSound thisComment
        //   wait #soundStop
        //   if inState( #utterancesRemaining, #iCantSee ) then assertSound #iCantSee
        //
        // Chippy in the car, working down a list: first he wants to go home,
        // then he settles for going anywhere, and after that it is a joy ride.
        // Then, separately, he mentions that he cannot see out.
        //
        // Nothing is said at all unless Chippy is actually in the car, which
        // is the one guard: he does not narrate a journey he is not on.
        "carcomments" => {
            if !state.get("chippyLocation").is_symbol("inCar") {
                return true;
            }
            let pending = |state: &State, want: &str| {
                state
                    .get_all("utterancesRemaining")
                    .iter()
                    .any(|v| v.is_symbol(want))
            };
            let line = if pending(state, "homeEdwin") {
                "homeEdwin"
            } else if pending(state, "letsGo") {
                "letsGo"
            } else {
                "joyRide"
            };
            super::assert_sound(line, None, state, out);
            out.effects.push(Effect::WaitForSound(line.into()));
            if pending(state, "iCantSee") {
                super::assert_sound("iCantSee", None, state, out);
            }
        }

        // on setCarLocation suggestion
        //   validSuggestions = [#inStorage, #standingBy, #enRoute,
        //                       #hub_main, #hub_A, #hub_B, #hub_C]
        //   if getPos( validSuggestions, suggestion ) = 0 then
        //     beep : put "<!> Sorry, " & suggestion & " isn't a valid suggestion..."
        //     return
        //   setProp( oStoryteller.states, #carLocation, list(suggestion) )
        //   if getPos( validSuggestions, suggestion ) > 3 then
        //     set the castNum of sprite 44 = getProp( <hub frames>, suggestion )
        //   updateDisplay( oPuppeteer )
        //
        // Seven places the car can be, in two groups. The first three --
        // stored, waiting, on its way -- are states of the car; the last four
        // are positions on the hub, and only those redraw the hub display.
        // The split is done by position in the list rather than by name, which
        // is why the order of that list is not arbitrary.
        "setcarlocation" => {
            const PLACES: [&str; 7] = [
                "inStorage",
                "standingBy",
                "enRoute",
                "hub_main",
                "hub_A",
                "hub_B",
                "hub_C",
            ];
            let Some(asked) = args.first() else { return true };
            let Some(at) = PLACES.iter().position(|p| asked.is_symbol(p)) else {
                trace!(
                    crate::trace::Topic::Script,
                    "setCarLocation: {asked:?} is not a place the car can be"
                );
                return true;
            };
            state.set_all("carLocation", vec![Value::Symbol(PLACES[at].into())]);
            // The last four are hub positions and redraw the hub display.
            if at >= 3 {
                out.effects.push(Effect::SpriteCastNamed {
                    channel: 44,
                    name: PLACES[at].into(),
                });
            }
            out.redraw = true;
        }

        // on setWaffleTracks suggestion
        //   lsWaffleTracks = getProp( oStoryteller.states, #waffleTracks )
        //   if suggestion = #None then
        //     setProp( oStoryteller.states, #waffleTracks, list(#None) )
        //   else
        //     if getPos( lsWaffleTracks, suggestion ) = 0
        //       then append( lsWaffleTracks, suggestion )
        //       else cursorDance
        //
        // Not a flag but a set: the tracks the car has been down, accumulated.
        // Asking for one already in the list is not an error and not a
        // repeat -- the cursor twitches and nothing else happens, which is the
        // game's way of saying "you have already done that".
        //
        // `#None` empties it.
        "setwaffletracks" => {
            let Some(asked) = args.first() else { return true };
            if asked.is_symbol("None") {
                state.set_all("waffleTracks", vec![Value::Symbol("None".into())]);
                return true;
            }
            let already = state
                .get_all("waffleTracks")
                .iter()
                .any(|t| t.loosely_eq(asked));
            if !already {
                state.add_item("waffleTracks", asked.clone());
                out.redraw = true;
            }
        }

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
            if !state.get("Wind").is_symbol("None") {
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

        // on chippyHopsIn
        //   wait 30 : startSound #carDoorOpen
        //   setState( oStoryteller, #chippyLocation, #inCar )
        //   wait 30
        //   passengerSprite = 45
        //   puppetSprite passengerSprite, 1
        //   set the castNum of sprite passengerSprite = 1183   -- chpenter.mov
        //   set the loc     of sprite passengerSprite = point( 454, 365 )
        //   ... run the film out ...
        //   puppetSprite passengerSprite, 0 : updateDisplay
        //   startSound #carDoorClose
        //
        // Squeezing the duck on the car's wing calls the chipmunk over, and
        // this is him getting in: a 172 by 80 film of him climbing through the
        // window, parked at the passenger side. The room only asks for it --
        // `if getState( #chippyLocation ) = #waiting then chippyHopsIn` -- so
        // with the handler missing the duck honked and nothing happened, and
        // he never rode along. Two of the car's films are the ones with him in
        // it, and one of those two is the only way the chapter ends.
        "chippyhopsin" => {
            const PASSENGER: u8 = 45;
            const CHPENTER: u32 = 1183;

            out.effects.push(Effect::WaitTicks(30));
            out.effects.push(Effect::PlaySound {
                name: "carDoorOpen".into(),
                loudness: None,
            });
            state.set("chippyLocation", Value::Symbol("inCar".into()));
            out.effects.push(Effect::WaitTicks(30));

            out.effects.push(Effect::PuppetSprite {
                channel: PASSENGER,
                on: true,
            });
            out.effects.push(Effect::SpriteCast {
                channel: PASSENGER,
                cast: CHPENTER,
            });
            out.effects.push(Effect::SpriteLoc {
                channel: PASSENGER,
                x: 454,
                y: 365,
            });
            out.effects.push(Effect::WaitForOverlay);
            out.effects.push(Effect::PuppetSprite {
                channel: PASSENGER,
                on: false,
            });
            out.effects.push(Effect::PlaySound {
                name: "carDoorClose".into(),
                loudness: None,
            });
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
            call("killsongs", &[], state, out);
        }

        // on killSongs optionSwitch
        //   if gSoundsSuspended then return
        //   if optionSwitch = #PConly and gCPU <> #PC then return
        //   lsCarols      = getProp( oStoryteller.states, #windSongs )
        //   soundChannels = getProp( oStoryteller.states, #soundChannels )
        //   repeat with each channel whose #sndName is in lsCarols
        //     ... ramp its volume 255 down to 0, waiting 4 ticks a step ...
        //   puppetSound 0
        //
        // `#windSongs: [#threeKings, #silentNight, #godRestYe, #goodKing]` --
        // the four carols the wind carries. This used to stop a loop called
        // `carols`, which is a name I made up and which nothing in the game
        // answers to, so for eight call sites the carols simply never stopped.
        //
        // It went unnoticed because `verify` checked the names on `PlaySound`
        // and `StartLoop` and not on `StopLoop`. It checks all three now, and
        // that is what turned this up.
        //
        // `#PConly` is honoured the way the rest of the port honours a
        // platform test: this disc's movies are RIFX, so the Mac arm is the
        // one that applies and a `#PConly` call does nothing.
        "killsongs" => {
            const WIND_SONGS: [&str; 4] = ["threeKings", "silentNight", "godRestYe", "goodKing"];
            let pc_only = args
                .first()
                .and_then(Value::as_str)
                .is_some_and(|o| o.trim_start_matches('#').eq_ignore_ascii_case("PConly"));
            if pc_only || state.get("gSoundsSuspended").as_int() == Some(1) {
                return true;
            }
            for song in WIND_SONGS {
                out.effects.push(Effect::StopLoop {
                    name: song.into(),
                    fade: true,
                });
            }
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


    // -- the whirligig ------------------------------------------------------

    /// The vane pointing somewhere and the air still, which is the state the
    /// whirligig is worked in.
    fn windy(facing: &str) -> State {
        let mut s = State::new();
        s.set_all("gChapter", vec![Value::Symbol("EDWIN".into())]);
        s.set_all("weatherVane", vec![Value::Symbol(facing.into())]);
        s.set_all("Wind", vec![Value::Symbol("None".into())]);
        s
    }

    fn casts(out: &Outcome) -> Vec<(u8, u32)> {
        out.effects
            .iter()
            .filter_map(|e| match e {
                Effect::SpriteCast { channel, cast } => Some((*channel, *cast)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_whirligig_has_a_pair_of_films_for_each_wind() {
        for (facing, start, spin) in [
            ("n", 966, 970),
            ("S", 967, 971),
            ("E", 965, 969),
            ("W", 968, 972),
        ] {
            let mut s = windy(facing);
            let mut out = Outcome::default();
            assert!(call("initwhirligig", &[], &mut s, &mut out));
            assert_eq!(casts(&out), [(44, spin), (45, start)], "wind from {facing}");
        }
    }

    #[test]
    fn and_finds_them_whichever_way_the_case_falls() {
        // The film tables spell three directions in lower case and the vane
        // spells three in upper; Lingo does not care and neither can this.
        let mut lower = windy("s");
        let mut upper = windy("S");
        let (mut a, mut b) = (Outcome::default(), Outcome::default());
        assert!(call("initwhirligig", &[], &mut lower, &mut a));
        assert!(call("initwhirligig", &[], &mut upper, &mut b));
        assert_eq!(casts(&a), casts(&b));
        assert!(!casts(&a).is_empty());
    }

    #[test]
    fn nothing_happens_once_the_wind_is_up() {
        // `if getState( #Wind ) <> #None then return`. The whirligig is what
        // starts the wind, so it works in still air and has nothing to do once
        // the air is moving -- the way round I had it, it would only work once
        // there was already a wind, and nothing in the chapter makes one.
        let mut s = windy("n");
        s.set("Wind", Value::Symbol("W".into()));
        let mut out = Outcome::default();
        assert!(call("initwhirligig", &[], &mut s, &mut out));
        assert!(out.effects.is_empty());

        let mut out = Outcome::default();
        assert!(call("startwhirligig", &[], &mut s, &mut out));
        assert!(out.effects.is_empty());
    }

    #[test]
    fn starting_it_takes_the_wind_from_the_vane() {
        let mut s = windy("n");
        // The vane has been turned since the wind picked up.
        s.set("weatherVane", Value::Symbol("W".into()));
        let mut out = Outcome::default();
        assert!(call("startwhirligig", &[], &mut s, &mut out));
        assert_eq!(s.get("Wind"), Value::Symbol("W".into()));
    }

    #[test]
    fn killing_the_songs_stops_the_four_carols() {
        let mut s = windy("n");
        let mut out = Outcome::default();
        assert!(call("killsongs", &[], &mut s, &mut out));
        let stopped: Vec<String> = out
            .effects
            .iter()
            .filter_map(|e| match e {
                Effect::StopLoop { name, .. } => Some(name.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            stopped,
            ["threeKings", "silentNight", "godRestYe", "goodKing"]
        );
    }

    #[test]
    fn but_a_pc_only_call_does_nothing_here() {
        let mut s = windy("n");
        let mut out = Outcome::default();
        assert!(call(
            "killsongs",
            &[Value::Symbol("PConly".into())],
            &mut s,
            &mut out
        ));
        assert!(out.effects.is_empty());
    }

    #[test]
    fn the_waffle_tracks_accumulate_and_do_not_repeat() {
        let mut s = State::new();
        s.set_all("gChapter", vec![Value::Symbol("EDWIN".into())]);
        let mut add = |t: &str| {
            let mut out = Outcome::default();
            assert!(call(
                "setwaffletracks",
                &[Value::Symbol(t.into())],
                &mut s,
                &mut out
            ));
        };
        add("a");
        add("b");
        add("a");
        let held: Vec<String> = s
            .get_all("waffleTracks")
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect();
        assert_eq!(held, ["a", "b"]);

        // #None empties it.
        let mut out = Outcome::default();
        assert!(call(
            "setwaffletracks",
            &[Value::Symbol("None".into())],
            &mut s,
            &mut out
        ));
        assert_eq!(s.get("waffleTracks"), Value::Symbol("None".into()));
    }

    #[test]
    fn only_the_hub_positions_redraw_the_hub() {
        let drive = |to: &str| {
            let mut s = State::new();
            s.set_all("gChapter", vec![Value::Symbol("EDWIN".into())]);
            let mut out = Outcome::default();
            assert!(call(
                "setcarlocation",
                &[Value::Symbol(to.into())],
                &mut s,
                &mut out
            ));
            let drew = out
                .effects
                .iter()
                .any(|e| matches!(e, Effect::SpriteCastNamed { channel: 44, .. }));
            (s.get("carLocation"), drew)
        };
        // The first three are states of the car, not places on the hub.
        assert_eq!(drive("enRoute"), (Value::Symbol("enRoute".into()), false));
        assert_eq!(drive("hub_B"), (Value::Symbol("hub_B".into()), true));
    }

    #[test]
    fn and_a_place_the_car_cannot_be_is_refused() {
        let mut s = State::new();
        s.set_all("gChapter", vec![Value::Symbol("EDWIN".into())]);
        s.set_all("carLocation", vec![Value::Symbol("standingBy".into())]);
        let mut out = Outcome::default();
        assert!(call(
            "setcarlocation",
            &[Value::Symbol("hub_Z".into())],
            &mut s,
            &mut out
        ));
        assert_eq!(s.get("carLocation"), Value::Symbol("standingBy".into()));
    }

    #[test]
    fn chippy_only_narrates_a_journey_he_is_on() {
        let make = |where_: &str, lines: &[&str]| {
            let mut s = State::new();
            s.set_all("gChapter", vec![Value::Symbol("EDWIN".into())]);
            s.set_all("chippyLocation", vec![Value::Symbol(where_.into())]);
            s.set_all(
                "utterancesRemaining",
                lines.iter().map(|l| Value::Symbol((*l).into())).collect(),
            );
            s
        };
        let said = |s: &mut State| {
            let mut out = Outcome::default();
            assert!(call("carcomments", &[], s, &mut out));
            out.effects
                .iter()
                .filter_map(|e| match e {
                    Effect::PlaySound { name, .. } => Some(name.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };

        // Not in the car: nothing at all.
        let mut out_of_car = make("onShelf", &["homeEdwin", "letsGo"]);
        assert!(said(&mut out_of_car).is_empty());

        // In the car, he works down the list and mentions the view once.
        let mut s = make("inCar", &["homeEdwin", "letsGo", "iCantSee"]);
        assert_eq!(said(&mut s), ["homeEdwin", "iCantSee"]);
        assert_eq!(said(&mut s), ["letsGo"]);
        // Everything spent, and he falls through to the joy ride -- which he
        // does not have, so he says nothing.
        assert!(said(&mut s).is_empty());
    }

    #[test]
    fn how_likely_chippy_is_to_speak_is_out_of_six() {
        let attempt = |likely: Option<i32>, seed: i32| {
            let mut s = State::new();
            s.set_all("gChapter", vec![Value::Symbol("EDWIN".into())]);
            s.set_all("gRandomSeed", vec![Value::Int(seed)]);
            s.set_all("chippyPleas", vec![Value::Symbol("pullMyFinger".into())]);
            let mut out = Outcome::default();
            let args: Vec<Value> = likely.map(Value::Int).into_iter().collect();
            assert!(call("chippyspeaks", &args, &mut s, &mut out));
            out.effects
                .iter()
                .any(|e| matches!(e, Effect::PlaySound { .. }))
        };

        // No argument at all means certain, whatever the roll comes out as.
        for seed in 1..40 {
            assert!(attempt(None, seed), "silent with no argument, seed {seed}");
        }
        // A one-in-six is not certain, and is not never either.
        let spoke = (1..200).filter(|s| attempt(Some(1), *s)).count();
        assert!(spoke > 0 && spoke < 199, "one-in-six spoke {spoke} of 199");
    }

    #[test]
    fn and_he_asks_for_things_in_order() {
        let mut s = State::new();
        s.set_all("gChapter", vec![Value::Symbol("EDWIN".into())]);
        s.set_all(
            "chippyPleas",
            vec![
                Value::Symbol("pullMyFinger".into()),
                Value::Symbol("pokeHim".into()),
            ],
        );
        let mut out = Outcome::default();
        assert!(call("chippyspeaks", &[], &mut s, &mut out));
        let said: Vec<String> = out
            .effects
            .iter()
            .filter_map(|e| match e {
                Effect::PlaySound { name, .. } => Some(name.clone()),
                _ => None,
            })
            .collect();
        // The front of the list, not a pick from it.
        assert_eq!(said, ["pullMyFinger"]);
    }

    // -- the car and its tracks ---------------------------------------------

    fn car_at(where_: &str, track: &str) -> State {
        let mut s = State::new();
        s.set_all("gChapter", vec![Value::Symbol("EDWIN".into())]);
        s.set_all("carLocation", vec![Value::Symbol(where_.into())]);
        if !track.is_empty() {
            s.set_all("currentTrack", vec![Value::Symbol(track.into())]);
        }
        s
    }

    fn steer(state: &mut State, way: &str) -> Outcome {
        let mut out = Outcome::default();
        assert!(call(
            "choosetrack",
            &[Value::Symbol(way.into())],
            state,
            &mut out
        ));
        out
    }

    #[test]
    fn leaving_a_hub_picks_a_track_and_a_stretch_of_film() {
        // The main hub has the long stretches.
        let mut s = car_at("hub_main", "");
        let out = steer(&mut s, "middle");
        assert_eq!(s.get("currentTrack"), Value::Symbol("B".into()));
        assert_eq!(s.get("carLocation"), Value::Symbol("enRoute".into()));
        assert_eq!(segment(&out), Some((225, 448)));

        // The lettered hubs share a shorter set.
        let mut s = car_at("hub_C", "");
        let out = steer(&mut s, "right");
        assert_eq!(s.get("currentTrack"), Value::Symbol("CR".into()));
        assert_eq!(segment(&out), Some((360, 540)));
    }

    #[test]
    fn the_two_turn_tables_are_each_other_backwards() {
        let mut s = car_at("enRoute", "c");
        steer(&mut s, "right");
        assert_eq!(s.get("currentTrack"), Value::Symbol("B".into()));
        steer(&mut s, "left");
        assert_eq!(s.get("currentTrack"), Value::Symbol("c".into()));

        let mut s = car_at("enRoute", "AL");
        steer(&mut s, "right");
        assert_eq!(s.get("currentTrack"), Value::Symbol("AM".into()));
        steer(&mut s, "right");
        assert_eq!(s.get("currentTrack"), Value::Symbol("AR".into()));
    }

    #[test]
    fn a_dead_end_is_an_absence_rather_than_a_refusal() {
        // #AR has no right turn, so nothing at all happens -- no film, no
        // sound, and the car stays where it is.
        let mut s = car_at("enRoute", "AR");
        let out = steer(&mut s, "right");
        assert_eq!(s.get("currentTrack"), Value::Symbol("AR".into()));
        assert!(!out
            .effects
            .iter()
            .any(|e| matches!(e, Effect::PlayVideoSegment { .. })));

        // And the middle is not a turn at all once rolling.
        let mut s = car_at("enRoute", "c");
        steer(&mut s, "middle");
        assert_eq!(s.get("currentTrack"), Value::Symbol("c".into()));
    }

    #[test]
    fn left_is_the_back_half_of_the_same_film() {
        let mut right = car_at("enRoute", "c");
        let out = steer(&mut right, "right");
        assert_eq!(segment(&out), Some((0, 448)));

        let mut left = car_at("enRoute", "A");
        let out = steer(&mut left, "left");
        assert_eq!(segment(&out), Some((450, 900)));
    }

    #[test]
    fn the_boat_goes_where_the_wind_sends_it() {
        let sail = |boat: &str, wind: &str| {
            let mut s = State::new();
            s.set_all("gChapter", vec![Value::Symbol("EDWIN".into())]);
            s.set_all("boatPosition", vec![Value::Symbol(boat.into())]);
            s.set_all("Wind", vec![Value::Symbol(wind.into())]);
            s.set_all("teddyLocation", vec![Value::Symbol("waiting".into())]);
            let mut out = Outcome::default();
            assert!(call("setsail", &[], &mut s, &mut out));
            (
                s.get("boatPosition").as_str().unwrap_or("").to_string(),
                s.get("teddyLocation").as_str().unwrap_or("").to_string(),
            )
        };

        // East pushes it back, west brings it forward.
        assert_eq!(sail("forward", "E").0, "backward");
        assert_eq!(sail("backward", "W").0, "forward");

        // Any other wind and it stays where it is.
        assert_eq!(sail("forward", "W").0, "forward");
        assert_eq!(sail("backward", "E").0, "backward");
        assert_eq!(sail("forward", "n").0, "forward");

        // Coming forward with Teddy waiting is what puts him on the anchor.
        assert_eq!(sail("backward", "W").1, "onAnchor");
        assert_eq!(sail("forward", "E").1, "waiting");
    }

    /// Where the car comes to rest, which is the half of `driveTheCar` that
    /// was not ported at all. Without it every drive played its stretch of
    /// film and left the car exactly where it started, so the tracks could not
    /// be crossed and the chapter could not be finished.
    /// The list of things Chippy has to say turns over one place per click,
    /// and that rotation is what makes the finger joke reachable: it only
    /// lands when `#pullMyFinger` is lying second, which is three turns in.
    /// The whirligig's last line, which is the one that matters: it takes the
    /// montage down again. Every hotspot in the chapter worth clicking is
    /// guarded on `#showMontage` being 0, so without it the wind comes up and
    /// the world goes read-only.
    #[test]
    fn the_whirligig_puts_the_montage_away_after_itself() {
        let mut s = State::new();
        s.set_all("gChapter", vec![Value::Symbol("EDWIN".into())]);
        s.set_all("Wind", vec![Value::Symbol("None".into())]);
        s.set_all("weatherVane", vec![Value::Symbol("W".into())]);
        s.set_all("showMontage", vec![Value::Int(1), Value::Int(0)]);
        let mut out = Outcome::default();
        assert!(call("startwhirligig", &[], &mut s, &mut out));

        assert_eq!(s.get("Wind"), Value::Symbol("W".into()));
        let put_away = out.effects.iter().rposition(|e| {
            matches!(e, Effect::SetState { key, value } if key == "showMontage" && *value == Value::Int(0))
        });
        let film = out.effects.iter().position(|e| matches!(e, Effect::PlayVideo(_)));
        assert!(
            put_away > film && film.is_some(),
            "the montage has to come down after the film: {:?}",
            out.effects
        );
    }

    #[test]
    fn his_pleas_come_round_to_the_joke() {
        let mut s = State::new();
        s.set_all("gChapter", vec![Value::Symbol("EDWIN".into())]);
        s.set_all(
            "chippyPleas",
            ["helpMe", "PULL_ME_OUT", "pullMyFinger", "pullMeOut"]
                .map(|p| Value::Symbol(p.into()))
                .to_vec(),
        );
        let joke_ready = |s: &State| {
            s.get_all("chippyPleas")
                .iter()
                .position(|p| p.is_symbol("pullMyFinger"))
                == Some(1)
        };

        for turn in 1..=3 {
            assert!(!joke_ready(&s), "ready after {} turns", turn - 1);
            let mut out = Outcome::default();
            assert!(call("chippyspeaks", &[], &mut s, &mut out));
        }
        assert!(joke_ready(&s), "{:?}", s.get_all("chippyPleas"));

        // And pulling then takes it off the list, so it only happens once.
        let mut out = Outcome::default();
        assert!(call("pullonchippy", &[], &mut s, &mut out));
        assert!(out.effects.iter().any(|e| matches!(
            e,
            Effect::PlaySound { name, .. } if name == "puppetFart"
        )));
        assert!(!s.get_all("chippyPleas").iter().any(|p| p.is_symbol("pullMyFinger")));
    }

    #[test]
    fn a_drive_ends_somewhere() {
        let drive = |track: &str, teddy: &str| {
            let mut s = State::new();
            s.set_all("gChapter", vec![Value::Symbol("EDWIN".into())]);
            s.set_all("currentTrack", vec![Value::Symbol(track.into())]);
            s.set_all("chippyLocation", vec![Value::Symbol("inCar".into())]);
            s.set_all("boatPosition", vec![Value::Symbol("forward".into())]);
            s.set_all("teddyLocation", vec![Value::Symbol(teddy.into())]);
            s.set_all("carLocation", vec![Value::Symbol("enRoute".into())]);
            let mut out = Outcome::default();
            assert!(call("drivethecar", &[], &mut s, &mut out));
            (s, out)
        };

        // A trunk line parks the car at its own hub, ready to be pointed down
        // one of the three spurs off it.
        let (s, _) = drive("c", "waiting");
        assert_eq!(s.get("carLocation"), Value::Symbol("hub_c".into()));
        assert_eq!(s.get("currentTrack"), Value::Symbol("main".into()));
        assert_eq!(s.get("showMontage"), Value::Int(1));

        // A middle spur is a ramp the car does not make: it comes back to
        // where it set off, on the montage the windscreen is guarded on.
        let (s, _) = drive("BM", "waiting");
        assert_eq!(s.get("carLocation"), Value::Symbol("standingBy".into()));
        assert_eq!(s.get("showMontage"), Value::Int(3));

        // A side spur comes out of a tunnel mouth back on the ice.
        let (_, out) = drive("BL", "waiting");
        assert_eq!(out.destination.as_deref(), Some("teN_fwd"));

        // And the middle of C with Teddy on the anchor is the end of the
        // chapter: he goes in the car and it drives out and home.
        let (s, out) = drive("CM", "onAnchor");
        assert_eq!(s.get("teddyLocation"), Value::Symbol("inCar".into()));
        assert_eq!(out.new_domain.as_deref(), Some("ROXY"));
        assert!(out.effects.iter().any(|e| matches!(
            e,
            Effect::GoToRoom { room, .. } if room == "car_domainExit"
        )));
    }

    #[test]
    fn the_middle_of_c_plays_whatever_the_puzzles_have_left() {
        let film = |chippy: bool, boat: &str, teddy: &str| {
            let mut s = State::new();
            s.set_all("gChapter", vec![Value::Symbol("EDWIN".into())]);
            s.set_all("currentTrack", vec![Value::Symbol("CM".into())]);
            s.set_all(
                "chippyLocation",
                vec![Value::Symbol(if chippy { "inCar" } else { "onShelf" }.into())],
            );
            s.set_all("boatPosition", vec![Value::Symbol(boat.into())]);
            s.set_all("teddyLocation", vec![Value::Symbol(teddy.into())]);
            let mut out = Outcome::default();
            assert!(call("drivethecar", &[], &mut s, &mut out));
            out.effects.iter().find_map(|e| match e {
                Effect::PlayVideo(Some(n)) => Some(n.clone()),
                _ => None,
            })
        };

        // Nothing done yet.
        assert_eq!(film(false, "backward", "waiting").as_deref(), Some("CM_emptyAnchor"));
        // Chippy aboard and the car misses the ramp.
        assert_eq!(film(true, "backward", "waiting").as_deref(), Some("CM_missRamp"));
        // The boat brought forward drops the anchor.
        assert_eq!(film(false, "forward", "waiting").as_deref(), Some("CM_anchorDown"));
        // And with Teddy on it, the rescue -- the later test wins, which is
        // what matters, because bringing the boat forward is what puts him
        // there in the first place and both are true at once.
        assert_eq!(film(false, "forward", "onAnchor").as_deref(), Some("CM_teddyRescue"));
    }

    #[test]
    fn and_every_other_track_has_one_film_named_after_it() {
        let mut s = State::new();
        s.set_all("gChapter", vec![Value::Symbol("EDWIN".into())]);
        s.set_all("currentTrack", vec![Value::Symbol("AL".into())]);
        s.set_all("waffleTracks", vec![Value::Symbol("a".into())]);
        let mut out = Outcome::default();
        assert!(call("drivethecar", &[], &mut s, &mut out));
        assert!(out
            .effects
            .iter()
            .any(|e| matches!(e, Effect::PlayVideo(Some(n)) if n == "AL")));
        // And the record of where the car has been starts again.
        assert_eq!(s.get("waffleTracks"), Value::Symbol("None".into()));
    }
}

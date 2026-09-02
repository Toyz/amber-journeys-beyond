//! Handlers the chapters share, and the hooks that ship empty.
//!
//! Each handler carries the disassembly it was read from, so the port can be
//! checked against the original without going back to the bytecode.

use lingo::Value;

use crate::script::{Effect, Outcome};
use crate::state::State;


/// The doors, cabinets and drawers that open and shut, and the sound each
/// makes doing it, as `(chapter, handler, opening cue, closing cue)`.
///
/// A flag the schema declares with a single value is not a value at all: it
/// says a handler named `set<Flag>` exists, and `setState` dispatches to it.
/// Twenty-five of those handlers are one template, differing only in the cue
/// they play, so they are a table rather than twenty-five ports.
///
/// The cues are read out of each handler rather than guessed from its name,
/// which is how the two oddities here survive: Margaret's boathouse door and
/// mailbox are cabinets, and Brice's boathouse door plays the grate, the
/// authors having copied `setGrateIsOpen` and not changed the sound.
const OPENABLE: &[(&str, &str, &str, &str)] = &[
    ("BRICE", "setboathousedoorisopen", "grateOpen", "grateClose"),
    ("BRICE", "setgrateisopen", "grateOpen", "grateClose"),
    ("EDWIN", "setgrateisopen", "grateOpen", "grateClose"),
    ("MARGARET", "setbalconydoorisopen", "doorOpen", "doorClose"),
    ("MARGARET", "setbathroomdoorisopen", "doorOpen", "doorClose"),
    ("MARGARET", "setbedrmcabinetisopen", "cabinetOpen", "cabinetClose"),
    ("MARGARET", "setboathousedoorisopen", "cabinetOpen", "cabinetClose"),
    ("MARGARET", "setdiningrmcabinetisopen", "cabinetOpen", "cabinetClose"),
    ("MARGARET", "setfortiesbedroomdoorisopen", "doorOpen", "doorClose"),
    ("MARGARET", "setfrontdoorisopen", "doorOpen", "doorClose"),
    ("MARGARET", "setgaragedoorisopen", "doorOpen", "doorClose"),
    ("MARGARET", "setkitchencabinetisopen", "cabinetOpen", "cabinetClose"),
    ("MARGARET", "setkitchenreardoorisopen", "doorOpen", "doorClose"),
    ("MARGARET", "setmailboxisopen", "cabinetOpen", "cabinetClose"),
    ("MARGARET", "setofficedrawerisopen", "drawerOpen", "drawerClose"),
    ("MARGARET", "setshowerdoorisopen", "doorOpen", "doorClose"),
    ("MARGARET", "setstudydrawerisopen", "drawerOpen", "drawerClose"),
    ("ROXY", "setbathroomdoorisopen", "doorOpen", "doorClose"),
    ("ROXY", "setboathousedoorisopen", "cabinetOpen", "cabinetClose"),
    ("ROXY", "setdiningrmcabinetisopen", "sideboardOpen", "sideboardClose"),
    ("ROXY", "setfortiesbedroomdoorisopen", "doorOpen", "doorClose"),
    ("ROXY", "setmailboxisopen", "mailboxOpen", "mailboxClose"),
    ("ROXY", "setofficedrawerisopen", "drawerOpen", "drawerClose"),
    ("ROXY", "setshowerdoorisopen", "showerOpen", "showerClose"),
    ("ROXY", "setstudydrawerisopen", "drawerOpen", "drawerClose"),
];

/// A group of things that open the same way, and the sounds they make.
struct Parts {
    members: &'static [&'static str],
    open_cue: &'static str,
    close_cue: &'static str,
    /// Whether this can be opened while something else already is. The kitchen
    /// bin can: it is not a cupboard door and does not have to wait for one to
    /// shut.
    opens_regardless: bool,
}

/// The cabinets whose flag holds *which* door is open rather than whether one
/// is.
///
/// A kitchen cabinet is not a boolean. `#kitchenCabinetIsOpen` holds
/// `#upperLeft`, `#drawer`, `#trashCan`, `#None` and so on, and the sound
/// depends on which: seven cupboard doors share one cue, the cutlery drawer
/// has its own, and the bin has its own again.
///
/// The asymmetry in the bin is real and deliberate. It *closes* with the
/// cupboard sound and *opens* with the drawer one, and it is the only member
/// that opens while another is still open. Reproducing that is the point --
/// a tidier port would sound wrong in a way nobody could name.
const ONE_OF_MANY: &[(&str, &str, &[Parts])] = &[
    (
        "ROXY",
        "setkitchencabinetisopen",
        &[
            Parts {
                members: &[
                    "upperLeft",
                    "upperMiddle",
                    "upperRight",
                    "lowerLeft",
                    "lowerMiddle",
                    "lowerRight",
                    "cupboard",
                ],
                open_cue: "cabinetOpen",
                close_cue: "cabinetClose",
                opens_regardless: false,
            },
            Parts {
                members: &["trashCan"],
                open_cue: "drawerOpen",
                close_cue: "cabinetClose",
                opens_regardless: true,
            },
            Parts {
                members: &["drawer"],
                open_cue: "drawerOpen",
                close_cue: "drawerClose",
                opens_regardless: false,
            },
            Parts {
                members: &["silverDrawer"],
                open_cue: "silverDrawerOpen",
                close_cue: "silverDrawerClose",
                opens_regardless: false,
            },
        ],
    ),
    (
        "ROXY",
        "setbedrmcabinetisopen",
        &[
            Parts {
                members: &["bureau1", "bureau2", "bureau3", "leftTable", "rightTable"],
                open_cue: "drawerOpen",
                close_cue: "drawerClose",
                opens_regardless: false,
            },
            Parts {
                members: &["armoire"],
                open_cue: "cabinetOpen",
                close_cue: "cabinetClose",
                opens_regardless: false,
            },
            Parts {
                members: &["closet"],
                open_cue: "doorOpen",
                close_cue: "doorClose",
                opens_regardless: false,
            },
        ],
    ),
];

/// The shared body of the cabinets above.
///
///   on set<X>IsOpen suggestion
///     currentState = getState( #X )
///     if suggestion = #None and currentState <> #None then
///       cue for whichever group currentState is in, closing
///       setProp( ..., #X, list(suggestion) ) : updateDisplay
///     if suggestion <> #None and currentState = #None then
///       cue for whichever group suggestion is in, opening
///       setProp( ..., #X, list(suggestion) ) : updateDisplay
///     if suggestion = #trashCan then
///       cue( #drawerOpen ) : setProp( ... ) : updateDisplay
///
/// The third arm has no guard on the current state, which is what lets the bin
/// open over an open cupboard. Everything else waits its turn.
fn one_of_many(name: &str, args: &[Value], state: &mut State, out: &mut Outcome) -> bool {
    let chapter = state.get("gChapter");
    let chapter = chapter.as_str().unwrap_or_default();
    let Some(&(_, _, groups)) = ONE_OF_MANY
        .iter()
        .find(|(c, h, _)| *h == name && c.eq_ignore_ascii_case(chapter))
    else {
        return false;
    };

    let flag = &name[3..];
    let Some(asked) = args
        .first()
        .and_then(Value::as_str)
        .map(|v| v.trim_start_matches('#').to_string())
    else {
        return true;
    };
    let held = state.get(flag);
    let current = held.as_str().unwrap_or("None").trim_start_matches('#');
    let is_none = |v: &str| v.eq_ignore_ascii_case("None");
    let group_of = |member: &str| {
        groups
            .iter()
            .find(|g| g.members.iter().any(|m| m.eq_ignore_ascii_case(member)))
    };

    let cue = if is_none(&asked) && !is_none(current) {
        group_of(current).map(|g| g.close_cue)
    } else if !is_none(&asked) && is_none(current) {
        group_of(&asked).map(|g| g.open_cue)
    } else if group_of(&asked).is_some_and(|g| g.opens_regardless) {
        group_of(&asked).map(|g| g.open_cue)
    } else {
        // Asking for a door while another is open, and this one has to wait.
        return true;
    };
    let Some(cue) = cue else {
        // Not one of this cabinet's parts, which the original treats as a
        // write it does not recognise and leaves alone.
        return true;
    };

    out.effects.push(Effect::PlaySound {
        name: cue.into(),
        loudness: None,
    });
    state.set_all(flag, vec![Value::Symbol(asked)]);
    out.redraw = true;
    true
}

/// The shared body of the twenty-five openable setters.
///
///   on set<X>IsOpen suggestion
///     currentState = getState( #X )
///     if suggestion = 0 and currentState = 1 then
///       cue( #<thing>Close )
///       setProp( oStoryteller.states, #X, list(0) )
///       updateDisplay( oPuppeteer )
///     if suggestion = 1 and currentState = 0 then
///       cue( #<thing>Open )
///       setProp( oStoryteller.states, #X, list(1) )
///       updateDisplay( oPuppeteer )
///
/// Both arms are guarded on the flag actually changing, so setting a door open
/// when it already is does nothing at all -- no sound, no redraw. That is what
/// makes these safe to call from anywhere, and it is why the write has to go
/// through here rather than straight to the flag.
fn openable(name: &str, args: &[Value], state: &mut State, out: &mut Outcome) -> bool {
    let chapter = state.get("gChapter");
    let chapter = chapter.as_str().unwrap_or_default();
    let Some(&(_, _, open_cue, close_cue)) = OPENABLE
        .iter()
        .find(|(c, h, _, _)| *h == name && c.eq_ignore_ascii_case(chapter))
    else {
        return false;
    };

    let flag = &name[3..];
    let Some(suggestion) = args.first() else {
        return true;
    };

    // Shut is `0` for the doors and `#None` for the drawers and cabinets, and
    // open is anything that is not shut. `setOfficeDrawerIsOpen` reads:
    //
    //   if suggestion = #None and currentState <> #None then close
    //   if suggestion <> #None and currentState = #None then open
    //
    // which is the same two arms as the boolean version with `#None` where
    // the `0` is -- so one predicate covers both families.
    //
    // Reading the suggestion as an integer instead swallowed every write to a
    // flag that names which part is open. `#officeDrawerIsOpen` holds `#top`
    // and `#bottom`, `as_int` gave nothing for either, and the handler
    // returned having done nothing: the desk drawer could not be opened at
    // all, which puts the BAR manual -- two of the three settings the machine
    // in the living room needs -- out of the player's reach entirely.
    let shut = |v: &Value| v.loosely_eq(&Value::Int(0)) || v.is_symbol("None");
    let cue = match (shut(suggestion), shut(&state.get(flag))) {
        (true, false) => close_cue,
        (false, true) => open_cue,
        // Already in the state asked for, so the original does nothing. This
        // is also why moving straight from one drawer to another cannot work
        // and the rooms shut the chest first: the open arm needs the flag to
        // be shut before it will take a new part.
        _ => return true,
    };
    out.effects.push(Effect::PlaySound {
        name: cue.into(),
        loudness: None,
    });
    // `setProp( states, flag, list(suggestion) )` -- the suggestion itself,
    // as the only value, so `#top` is what the room's plates then test.
    state.set_all(flag, vec![suggestion.clone()]);
    out.redraw = true;
    true
}

/// Runs a handler from this chapter, or reports that it is not one of ours.
pub fn call(name: &str, args: &[Value], state: &mut State, out: &mut Outcome) -> bool {
    if openable(name, args, state, out) || one_of_many(name, args, state, out) {
        return true;
    }
    // Arguments and effects are unused by some chapters until more handlers
    // land here; the signature is uniform so the dispatcher stays simple.
    let _ = (args, &out, &state);
    match name {
        // Every call sits at the end of a close-up's exit:
        //   stashClick / goTo( #parent, #backOff ) / idle / mouseDown
        //
        // `idle` yields to Director and `mouseDown` consumes the click that
        // is still down, so it does not fire again in the room just returned
        // to. This engine acts on the release edge and handles one click per
        // release, so there is no pending event to consume. A no-op here is
        // the same behaviour by a different route, not a gap.
        // on setCurrentLocation suggestion
        //   return
        //
        // A stub in the original, and deliberately so: the flag holds a single
        // value, which declares that a setter exists, and moving the player is
        // `moveToLocation`'s job rather than this one's. Writing the flag is
        // all that should happen, and the empty handler is how `setState` is
        // told to fall through to exactly that.
        //
        // Ported as the no-op it is, so the tally can say "read, and there was
        // nothing in it" rather than leaving it indistinguishable from a
        // handler nobody has opened.
        "setcurrentlocation" => {}

        "mousedown" => {}

        // on puppetSprite channel, on
        //   Takes a sprite channel away from the score so a script can drive
        //   it, or hands it back. The channels the game claims are 30, 39, 44
        //   and 45, which carry the animated parts of the puzzles.
        "puppetsprite" => {
            let channel = args.first().and_then(Value::as_int).unwrap_or(0);
            let on = args.get(1).is_none_or(|v| v.truthy());
            if channel > 0 {
                out.effects.push(Effect::PuppetSprite {
                    channel: channel as u8,
                    on,
                });
            }
        }

        // Preload hints for the laptop's animated controls; the engine decodes
        // on demand, so there is nothing to prepare.
        "loadmultiframes" | "purgemultiframes" => {}


        // These are `nothing` in the shipped movies: hooks the authors left
        // wired up but empty. Implemented as no-ops so they stop being
        // reported as missing.
        // Two one-line handlers, and the only thing that turns the pulse off
        // is the camcorder log -- watching Roxy's tape should not be
        // interrupted by the bar flashing at you.
        "enablepeekalert" => state.set("gPeekAlertEnabled", Value::Int(1)),
        "disablepeekalert" => state.set("gPeekAlertEnabled", Value::Int(0)),
        "initboxpuzzle" | "idle" | "nothing" => {}

        _ => return false,
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opened(chapter: &str, handler: &str, to: i32, from: i32) -> (State, Outcome) {
        let mut state = State::new();
        state.set_all("gChapter", vec![Value::Symbol(chapter.into())]);
        state.set_all(&handler[3..], vec![Value::Int(from)]);
        let mut out = Outcome::default();
        assert!(
            call(handler, &[Value::Int(to)], &mut state, &mut out),
            "{handler} should be handled"
        );
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
    fn opening_writes_the_flag_and_plays_the_cue() {
        let (state, out) = opened("ROXY", "setbathroomdoorisopen", 1, 0);
        assert_eq!(state.get_all("bathroomdoorisopen"), &[Value::Int(1)]);
        assert_eq!(sounds(&out), ["doorOpen"]);
        assert!(out.redraw);
    }

    #[test]
    fn closing_plays_the_other_cue() {
        let (state, out) = opened("ROXY", "setbathroomdoorisopen", 0, 1);
        assert_eq!(state.get_all("bathroomdoorisopen"), &[Value::Int(0)]);
        assert_eq!(sounds(&out), ["doorClose"]);
    }

    #[test]
    fn setting_a_door_to_what_it_already_is_does_nothing() {
        // Both arms of the original are guarded on the flag actually changing,
        // which is what makes these safe to call from anywhere: no sound, no
        // redraw, no write.
        let (_, out) = opened("ROXY", "setbathroomdoorisopen", 1, 1);
        assert!(sounds(&out).is_empty());
        assert!(!out.redraw);
        let (_, out) = opened("ROXY", "setbathroomdoorisopen", 0, 0);
        assert!(sounds(&out).is_empty());
    }

    #[test]
    fn the_same_flag_cues_differently_in_different_chapters() {
        // Margaret's boathouse door is a cabinet and Brice's plays the grate,
        // the authors having copied `setGrateIsOpen` without changing the
        // sound. Both are read from the handlers, not guessed from the name.
        let (_, marg) = opened("MARGARET", "setboathousedoorisopen", 1, 0);
        let (_, brice) = opened("BRICE", "setboathousedoorisopen", 1, 0);
        assert_eq!(sounds(&marg), ["cabinetOpen"]);
        assert_eq!(sounds(&brice), ["grateOpen"]);
    }

    #[test]
    fn a_chapter_that_does_not_declare_the_setter_is_not_claimed() {
        // Edwin has no bathroom door. The handler must decline rather than
        // answer with another chapter's sound.
        let mut state = State::new();
        state.set_all("gChapter", vec![Value::Symbol("EDWIN".into())]);
        let mut out = Outcome::default();
        assert!(!openable(
            "setbathroomdoorisopen",
            &[Value::Int(1)],
            &mut state,
            &mut out
        ));
    }
}

#[cfg(test)]
mod cabinet_tests {
    use super::*;

    fn kitchen(open: &str) -> State {
        let mut state = State::new();
        state.set_all("gChapter", vec![Value::Symbol("ROXY".into())]);
        state.set_all("kitchenCabinetIsOpen", vec![Value::Symbol(open.into())]);
        state
    }

    fn ask(state: &mut State, handler: &str, part: &str) -> Vec<String> {
        let mut out = Outcome::default();
        assert!(call(handler, &[Value::Symbol(part.into())], state, &mut out));
        out.effects
            .iter()
            .filter_map(|e| match e {
                Effect::PlaySound { name, .. } => Some(name.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn each_part_of_the_cabinet_sounds_like_itself() {
        for (part, cue) in [
            ("upperLeft", "cabinetOpen"),
            ("cupboard", "cabinetOpen"),
            ("drawer", "drawerOpen"),
            ("silverDrawer", "silverDrawerOpen"),
        ] {
            let mut s = kitchen("None");
            assert_eq!(ask(&mut s, "setkitchencabinetisopen", part), [cue], "{part}");
        }
    }

    #[test]
    fn the_bin_closes_like_a_cupboard_and_opens_like_a_drawer() {
        // Not tidy, and not a mistake: it is what the disc does.
        let mut s = kitchen("None");
        assert_eq!(ask(&mut s, "setkitchencabinetisopen", "trashCan"), ["drawerOpen"]);
        let mut s = kitchen("trashCan");
        assert_eq!(ask(&mut s, "setkitchencabinetisopen", "None"), ["cabinetClose"]);
    }

    #[test]
    fn the_bin_opens_over_an_open_cupboard_and_nothing_else_does() {
        let mut s = kitchen("upperLeft");
        assert_eq!(ask(&mut s, "setkitchencabinetisopen", "trashCan"), ["drawerOpen"]);
        assert!(s
            .get("kitchenCabinetIsOpen")
            .as_str()
            .is_some_and(|v| v == "trashCan"));

        // A second cupboard door has to wait for the first to shut.
        let mut s = kitchen("upperLeft");
        assert!(ask(&mut s, "setkitchencabinetisopen", "lowerRight").is_empty());
        assert!(s
            .get("kitchenCabinetIsOpen")
            .as_str()
            .is_some_and(|v| v == "upperLeft"));
    }

    #[test]
    fn closing_sounds_like_whatever_was_open() {
        let mut s = kitchen("drawer");
        assert_eq!(ask(&mut s, "setkitchencabinetisopen", "None"), ["drawerClose"]);
        let mut s = kitchen("cupboard");
        assert_eq!(ask(&mut s, "setkitchencabinetisopen", "None"), ["cabinetClose"]);
    }

    #[test]
    fn the_bedroom_has_drawers_an_armoire_and_a_closet() {
        let mut state = State::new();
        state.set_all("gChapter", vec![Value::Symbol("ROXY".into())]);
        for (part, cue) in [
            ("bureau2", "drawerOpen"),
            ("leftTable", "drawerOpen"),
            ("armoire", "cabinetOpen"),
            ("closet", "doorOpen"),
        ] {
            state.set_all("bedrmCabinetIsOpen", vec![Value::Symbol("None".into())]);
            assert_eq!(ask(&mut state, "setbedrmcabinetisopen", part), [cue], "{part}");
        }
    }

    #[test]
    fn something_that_is_not_part_of_the_cabinet_is_left_alone() {
        let mut s = kitchen("None");
        assert!(ask(&mut s, "setkitchencabinetisopen", "fridge").is_empty());
        assert!(s
            .get("kitchenCabinetIsOpen")
            .as_str()
            .is_some_and(|v| v == "None"));
    }

    #[test]
    fn a_drawer_that_names_which_one_is_open_still_opens() {
        // `#officeDrawerIsOpen` holds `#None`, `#top` and `#bottom`, not 0 and
        // 1. Reading the suggestion as an integer made every write to it a
        // no-op, so the desk drawer never opened and the BAR manual inside it
        // -- two of the three settings the machine in the living room wants --
        // could not be reached.
        let mut s = State::new();
        s.set_all("gChapter", vec![Value::Symbol("ROXY".into())]);
        s.set_all("officeDrawerIsOpen", vec![Value::Symbol("None".into())]);

        let mut out = Outcome::default();
        assert!(call(
            "setofficedrawerisopen",
            &[Value::Symbol("top".into())],
            &mut s,
            &mut out
        ));
        assert!(s.get("officeDrawerIsOpen").is_symbol("top"));
        assert!(out
            .effects
            .iter()
            .any(|e| matches!(e, Effect::PlaySound { name, .. } if name == "drawerOpen")));

        // The other drawer while this one is open does nothing, which is why
        // the room shuts the chest and waits before asking for the next one.
        let mut out = Outcome::default();
        call("setofficedrawerisopen", &[Value::Symbol("bottom".into())], &mut s, &mut out);
        assert!(s.get("officeDrawerIsOpen").is_symbol("top"));
        assert!(out.effects.is_empty());

        // Shutting it plays the close cue.
        let mut out = Outcome::default();
        call("setofficedrawerisopen", &[Value::Symbol("None".into())], &mut s, &mut out);
        assert!(s.get("officeDrawerIsOpen").is_symbol("None"));
        assert!(out
            .effects
            .iter()
            .any(|e| matches!(e, Effect::PlaySound { name, .. } if name == "drawerClose")));
    }

    #[test]
    fn and_a_door_that_is_only_ever_shut_or_open_still_works() {
        // The boolean family has to keep working: shut is 0 there, not #None.
        let mut s = State::new();
        s.set_all("gChapter", vec![Value::Symbol("ROXY".into())]);
        s.set_all("bathroomDoorIsOpen", vec![Value::Int(0)]);

        let mut out = Outcome::default();
        call("setbathroomdoorisopen", &[Value::Int(1)], &mut s, &mut out);
        assert_eq!(s.get("bathroomDoorIsOpen").as_int(), Some(1));
        assert!(out
            .effects
            .iter()
            .any(|e| matches!(e, Effect::PlaySound { name, .. } if name == "doorOpen")));

        // And opening one that is already open stays silent.
        let mut out = Outcome::default();
        call("setbathroomdoorisopen", &[Value::Int(1)], &mut s, &mut out);
        assert!(out.effects.is_empty());
    }
}

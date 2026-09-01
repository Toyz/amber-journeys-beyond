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
    let current = state.get(flag).as_int().unwrap_or(0);
    let Some(suggestion) = args.first().and_then(Value::as_int) else {
        return true;
    };

    let cue = match (suggestion, current) {
        (0, 1) => close_cue,
        (1, 0) => open_cue,
        // Already in the state asked for, so the original does nothing.
        _ => return true,
    };
    out.effects.push(Effect::PlaySound {
        name: cue.into(),
        loudness: None,
    });
    state.set_all(flag, vec![Value::Int(suggestion)]);
    out.redraw = true;
    true
}

/// Runs a handler from this chapter, or reports that it is not one of ours.
pub fn call(name: &str, args: &[Value], state: &mut State, out: &mut Outcome) -> bool {
    if openable(name, args, state, out) {
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
        "mousedown" => {}

        // on puppetSprite channel, on
        //   Takes a sprite channel away from the score so a script can drive
        //   it, or hands it back. The channels the game claims are 30, 39, 44
        //   and 45, which carry the animated parts of the puzzles.
        "puppetsprite" => {
            let channel = args.first().and_then(Value::as_int).unwrap_or(0);
            let on = args.get(1).map_or(true, |v| v.truthy());
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
        "disablepeekalert" | "enablepeekalert" | "initboxpuzzle" | "idle" | "nothing" => {}

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

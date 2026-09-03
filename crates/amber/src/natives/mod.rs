//! Set-piece handlers ported from the movies' compiled Lingo.
//!
//! The room scripts call these by name and the engine records any it cannot
//! perform as [`Effect::Native`]. Each one implemented here was read from the
//! disassembled bytecode of the movie that defines it; the comment above each
//! gives that reading so the port can be checked against the original.
//!
//! Handlers live in the module for the chapter whose movie defines them, which
//! keeps each file to the size of the chapter it describes and puts a handler
//! next to the others that share its state.
//!
//! Anything still unported keeps falling through to `Effect::Native`, so the
//! engine's own report stays an honest measure of what is left.

mod brice;
mod edwin;
mod margaret;
pub mod members;
mod roxy;
mod shared;

use lingo::Value;

use crate::script::{Effect, Outcome};
use crate::state::State;

/// Runs a named handler, returning false when it is not implemented yet.
///
/// `args` are already evaluated. Effects the handler produces are appended to
/// `out` so they interleave with the calling script's own timeline.
/// Says a line, if it has not been said before.
///
/// ```text
/// on assertSound whichSound
///   if not inState( #utterancesRemaining, whichSound ) then return
///   if whichSound = #thoseBees and inState( #utterancesRemaining, #youBees )
///     then return
///   sndDelay = getaProp( [#handwriting: 120], whichSound )
///   if voidp( sndDelay ) then sndDelay = 60
///   wait sndDelay
///   <play whichSound>
///   trimState( #utterancesRemaining, whichSound )
/// ```
///
/// Not a synonym for `soundEffect`, which is how it was read for a long time:
/// **a line is said once, ever**. One not in `#utterancesRemaining` is not
/// said at all, and saying it takes it out.
///
/// That matters because the same remark is placed in many rooms --
/// `assertSound #victoryGarden` appears in seven of Margaret's -- because it
/// is one observation the player might happen upon anywhere.
///
/// Lives here rather than in `script.rs` because the comment handlers call it
/// too, and two copies of a rule this quiet would drift.
pub(super) fn assert_sound(
    line: &str,
    loudness: Option<String>,
    state: &mut State,
    out: &mut Outcome,
) {
    let pending = |state: &State, want: &str| {
        state
            .get_all("utterancesRemaining")
            .iter()
            .any(|v| v.as_str().is_some_and(|s| s.eq_ignore_ascii_case(want)))
    };
    let line = line.trim_start_matches('#');
    if !pending(state, line) {
        return;
    }
    // Brice's bees have an order: he does not remark on whose bees they are
    // before remarking on them at all. Edwin's chapter carries the same test
    // against lines it does not have, which is a paste rather than a rule.
    if line.eq_ignore_ascii_case("thoseBees") && pending(state, "youBees") {
        return;
    }

    // The beat before speaking. Sixty ticks, with one exception per chapter --
    // and Edwin's goes the other way, because `#windControl` is a shout.
    let chapter = state.get("gChapter");
    let chapter = chapter.as_str().unwrap_or_default();
    let beat = match (chapter, line) {
        (c, l) if c.eq_ignore_ascii_case("MARGARET") && l.eq_ignore_ascii_case("victoryGarden") => 120,
        (c, l) if c.eq_ignore_ascii_case("EDWIN") && l.eq_ignore_ascii_case("windControl") => 15,
        (c, l) if c.eq_ignore_ascii_case("BRICE") && l.eq_ignore_ascii_case("handwriting") => 120,
        _ => 60,
    };
    out.effects.push(Effect::WaitTicks(beat));
    out.effects.push(Effect::PlaySound {
        name: line.to_string(),
        loudness,
    });
    // Taken out now rather than after the pause, where the original does it.
    // The original blocks on its wait, so a second call for the same line
    // later in the same list finds it already gone; deferring the trim in an
    // engine that queues its waits would let the line speak twice. The cost is
    // a guard read during the pause seeing it spent, which nothing does.
    state.trim_item("utterancesRemaining", &Value::Symbol(line.to_string()));
}

/// Whether a verb has a Rust handler in any chapter.
///
/// Worth having as its own question. An unported verb still parses, still
/// reaches the native path and still counts as an `Effect::Native`, so a
/// tally of native effects reads the same whether the handler exists or not
/// -- which is how `verify` came to report "unhandled calls: none" while
/// two dozen verbs had no arm anywhere.
///
/// Asking `call` is the only answer that cannot drift out of step with the
/// arms themselves, but it has to be asked once per chapter: the openers and
/// the bleeding doors are keyed on `(chapter, verb)` and decline outright when
/// the chapter is not theirs, so a single probe on a blank state reports every
/// one of them missing.
pub fn is_handled(name: &str) -> bool {
    ["ROXY", "MARGARET", "EDWIN", "BRICE"].iter().any(|chapter| {
        let mut state = State::new();
        state.set_all("gChapter", vec![Value::Symbol((*chapter).into())]);
        let mut out = Outcome::default();
        call(name, &[], &mut state, &mut out)
    })
}

pub fn call(name: &str, args: &[Value], state: &mut State, out: &mut Outcome) -> bool {
    shared::call(name, args, state, out)
        || members::call(name, args, state, out)
        || roxy::call(name, args, state, out)
        || edwin::call(name, args, state, out)
        || brice::call(name, args, state, out)
        || margaret::call(name, args, state, out)
}

/// A small deterministic die roll, seeded from a counter kept in game state.
///
/// The original calls Lingo's `random`. Using a state-backed counter rather
/// than a system source keeps a save reproducible, which matters because these
/// rolls gate audible events and a replayed save should sound the same.
pub(super) fn roll(state: &mut State, sides: i32) -> i32 {
    let seed = state.get("gRandomSeed").as_int().unwrap_or(1) as u32;
    // A small linear congruential step; the exact sequence does not need to
    // match the original, only to be varied and repeatable.
    let next = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    state.set("gRandomSeed", Value::Int(next as i32));
    ((next >> 16) % sides.max(1) as u32) as i32 + 1
}

#[cfg(test)]
mod handled_tests {
    use super::*;

    /// `is_handled` is only useful if an arm answers yes on empty arguments,
    /// which is not free: a handler that reads argument zero and returns false
    /// when it is missing would report itself unported.
    #[test]
    fn a_ported_verb_says_so_even_with_no_arguments() {
        for name in [
            "setfrontdoorisopen",
            "adjustalgorithm",
            "setfragmentbias",
            "setfragmentalignment",
            "resetboxpuzzle",
            "setopenbox",
        ] {
            assert!(is_handled(name), "{name} has an arm but reports unported");
        }
    }

    #[test]
    fn an_unported_handler_says_so_too() {
        // There are no unported verbs left, and the PeeK unit's own
        // interface is ported too. What remains is the rest of `idle`: the
        // menu bar, the cursor's idle animation, and the ripple that runs
        // after five seconds of no input with the Amber vision on.
        //
        // Named deliberately, so this fails loudly when one is ported and has
        // to be swapped for whatever is still outstanding. A test that could
        // not fail is what entry 81 was about.
        for name in ["cursordance", "ripple", "installmenu"] {
            assert!(!is_handled(name), "{name} reports ported but has no arm");
        }
    }
}


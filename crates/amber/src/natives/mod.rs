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
mod roxy;
mod shared;

use lingo::Value;

use crate::script::Outcome;
use crate::state::State;

/// Runs a named handler, returning false when it is not implemented yet.
///
/// `args` are already evaluated. Effects the handler produces are appended to
/// `out` so they interleave with the calling script's own timeline.
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
    fn an_unported_verb_says_so_too() {
        for name in ["camcontrol", "pushnail", "choosetrack", "drivethecar"] {
            assert!(!is_handled(name), "{name} reports ported but has no arm");
        }
    }
}


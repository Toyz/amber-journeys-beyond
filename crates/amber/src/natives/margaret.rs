//! Margaret's chapter: the house as it was, the radio and the clocks.
//!
//! Each handler carries the disassembly it was read from, so the port can be
//! checked against the original without going back to the bytecode.
//!
//! Nothing here yet. Margaret's set pieces are the clock puzzle, the radio
//! dial and the telegram, and they are still recorded as unimplemented so the
//! engine's own count stays honest.

use lingo::Value;

use crate::script::Outcome;
use crate::state::State;

/// Runs a handler from this chapter, or reports that it is not one of ours.
pub fn call(_name: &str, _args: &[Value], _state: &mut State, _out: &mut Outcome) -> bool {
    false
}

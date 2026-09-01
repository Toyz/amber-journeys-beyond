//! Parser for the Lingo literal values that make up Amber's game data.
//!
//! Every room in the game is one Lingo property list, serialised as text into the
//! `.DAT` files next to each character's movie. A node looks like:
//!
//! ```text
//! [#preLoad: [1591, 236], #onStage: [[#castName: "O_ENTRY2", #castNum: 1590,
//!  #channel: 1, #showIF: [#equals: [#always, 1]], #coords: point(320, 210),
//!  #ink: 0]], #Hotspots: [[#forward, rect(46, 64, 347, 356),
//!  ["goTo( #OfficeEwall, #forward )"], [#equals: [#always, 1]]]],
//!  #storageCast: [145, 1, 1089]]
//! ```
//!
//! Records are separated by a single 0x00 byte, and each file opens with a
//! `* date *` banner. This module turns that text into [`Value`] trees; the game
//! crate gives them meaning.

mod parser;
mod value;

pub use parser::{parse_dat, parse_value, ParseError};
pub use value::{Rect, Value};

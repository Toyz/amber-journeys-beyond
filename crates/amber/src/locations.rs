//! Resolution of the room names that `goTo` targets.
//!
//! The `.DAT` files hold rooms in file order and never name them; the hotspots
//! address destinations by symbol (`goTo( #bedrm_A1, #forward )`). The table
//! that bridges the two lives in an `STXT` chunk inside each chapter's movie,
//! grouped by area:
//!
//! ```text
//! [#DEFAULT: [#DEFAULT_LOCATION: [30, 1, 568]],
//!  #Bedrm: [#bedrm_A1: [31, 1, 784], #bedrm_A2: [32, 786, 2519], ...]]
//! ```
//!
//! Each entry's triple is exactly the `#storageCast` its room record carries, so
//! the triple is the join key between a name and a room. Several names can share
//! a triple, which is how the game gives one room more than one alias.

use std::collections::HashMap;

use lingo::{parse_value, Value};

/// Maps a room name to the `#storageCast` triple identifying its record.
pub struct LocationTable {
    by_name: HashMap<String, (u32, u32, u32)>,
}

impl LocationTable {
    /// Finds and parses the table among a movie's text chunks.
    ///
    /// The table is recognised by shape rather than by position: it is the text
    /// that parses as a property list whose leaves are three-integer lists.
    pub fn from_texts(texts: &[String]) -> LocationTable {
        let mut by_name = HashMap::new();
        for text in texts {
            let trimmed = text.trim();
            if !trimmed.starts_with("[#") || !trimmed.contains("_LOCATION") {
                continue;
            }
            let Ok(Value::Props(areas)) = parse_value(trimmed) else {
                continue;
            };
            for (_, rooms) in &areas {
                let Value::Props(rooms) = rooms else { continue };
                for (name, triple) in rooms {
                    if let Some([a, b, c]) = triple.as_list() {
                        if let (Some(a), Some(b), Some(c)) =
                            (a.as_int(), b.as_int(), c.as_int())
                        {
                            by_name.insert(name.clone(), (a as u32, b as u32, c as u32));
                        }
                    }
                }
            }
        }
        LocationTable { by_name }
    }

    pub fn all_names(&self) -> Vec<&str> {
        self.by_name.keys().map(String::as_str).collect()
    }

    pub fn triple(&self, name: &str) -> Option<(u32, u32, u32)> {
        self.by_name.get(&name.to_ascii_lowercase()).copied()
    }

    pub fn len(&self) -> usize {
        self.by_name.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }

    /// All names sharing a triple, so a room can report its aliases.
    pub fn names_for(&self, triple: (u32, u32, u32)) -> Vec<&str> {
        self.by_name
            .iter()
            .filter(|(_, t)| **t == triple)
            .map(|(n, _)| n.as_str())
            .collect()
    }
}

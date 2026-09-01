//! The declared state schema for a chapter.
//!
//! Each chapter movie carries an `STXT` chunk listing every flag the chapter
//! uses, one per line, as `#key : [values]`. The first value is the flag's
//! initial setting and the rest are the settings it may legally take:
//!
//! ```text
//! #always : [1]
//! #knittingNeedle : [#atRest, #floating, #dumbWaiter, #usedUp]
//! #currentLocation: [#bedrm_fadeIn]
//! ```
//!
//! This is effectively the save-file format, declared in the data rather than in
//! code, and it is also the only place the chapter's starting room is recorded.

use lingo::{parse_value, Value};

use crate::state::State;

pub struct Schema {
    /// Flag name to its declared values, first being the initial one.
    entries: Vec<(String, Vec<Value>)>,
}

impl Schema {
    /// Picks the schema out of a movie's text chunks and parses it.
    ///
    /// It is recognised by content rather than position: the schema is the text
    /// that declares `#always`, the flag every unconditional guard tests.
    pub fn from_texts(texts: &[String]) -> Option<Schema> {
        let text = texts.iter().find(|t| {
            t.contains("#always") && t.contains("#currentLocation") && !t.trim().starts_with("[#")
        })?;

        let mut entries = Vec::new();
        // Lines are separated by carriage returns, this being Mac-authored text.
        for line in text.split(['\r', '\n']) {
            let line = line.trim();
            let Some(rest) = line.strip_prefix('#') else { continue };
            let Some((key, values)) = rest.split_once(':') else { continue };
            let key = key.trim();
            if key.is_empty() || !key.chars().all(|c| c.is_alphanumeric() || c == '_') {
                continue;
            }
            // The value part is a Lingo list; a malformed one costs that flag
            // only, since a missing flag reads back as void.
            let Ok(parsed) = parse_value(values.trim()) else { continue };
            let list = match parsed {
                Value::List(v) => v,
                other => vec![other],
            };
            entries.push((key.to_string(), list));
        }
        (!entries.is_empty()).then_some(Schema { entries })
    }

    /// The room the chapter begins in.
    pub fn start_location(&self) -> Option<&str> {
        self.value_of("currentLocation")?.as_str()
    }

    fn value_of(&self, key: &str) -> Option<&Value> {
        self.entries
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))?
            .1
            .first()
    }

    /// Seeds a state store with every flag's initial value.
    ///
    /// Without this the guards run against an empty store, so any sprite whose
    /// `#showIF` compares a flag to its starting value stays hidden.
    pub fn seed(&self, state: &mut State) {
        for (key, values) in &self.entries {
            if let Some(initial) = values.first() {
                state.set(key, initial.clone());
            }
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

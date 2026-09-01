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

    /// The play order for a named programme, when the entry is one.
    ///
    /// A programme is declared as a list of symbols naming items inside the
    /// matching sound group, e.g.
    /// `#BRradio : [#tune2, #BRannouncer1, #tune1, #BRannouncer2]`. That is
    /// distinguishable from an ordinary flag by its values: a flag's list is
    /// its legal settings, and those are rarely all symbols and never repeat.
    pub fn playlist(&self, name: &str) -> Option<Vec<String>> {
        let (_, values) = self
            .entries
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))?;
        if values.len() < 2 {
            return None;
        }
        let items: Vec<String> = values
            .iter()
            .filter_map(|v| v.as_symbol())
            .map(str::to_owned)
            .collect();
        if items.len() != values.len() {
            return None;
        }
        // A flag lists distinct legal values; a programme repeats its takes.
        let mut unique = items.clone();
        unique.sort();
        unique.dedup();
        (unique.len() < items.len()).then_some(items)
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
    ///
    /// Most entries declare a value followed by the other settings the flag may
    /// take, so the first is the initial one. A few are pools, where the list
    /// itself is the value: the haunts still to happen, the ghosts still to be
    /// dealt with, the stations the radio can find. Those are named by
    /// `list_valued`, which the caller derives from how the game uses them
    /// rather than from anything the schema says, because the schema writes
    /// both shapes identically.
    /// Writes each flag's declared list into the state as it stands.
    ///
    /// The declared list is the flag's value: its head is the initial setting
    /// and the rest are the settings it may take, which is the same shape the
    /// game's own `getState` and `setState` work in. Nothing here needs to
    /// decide whether a flag is "a list" -- they all are.
    pub fn seed(&self, state: &mut State) {
        for (key, values) in &self.entries {
            state.seed_flag(key, values.clone());
        }
        trace!(
            crate::trace::Topic::State,
            "seeded {} flags from the schema",
            self.entries.len()
        );
    }

}

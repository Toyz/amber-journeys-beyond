//! State-indexed sprite casts.
//!
//! Most sprites name their art with a plain cast number. Fifty-eight of them
//! across three chapters instead write a two-element list:
//!
//! ```text
//! [#castName: "B_GZ_LOCK_A.frame", #castNum: [#lock_A, #lock_A_digits], ...]
//! ```
//!
//! The first element is a state flag and the second names a lookup table, so
//! the art shown is `table[state[flag]]`. That is how the lock wheels show
//! their digit, how Margaret's clocks show the time, and how Roxy's bar
//! settings show their level. Read as a single integer the list yields nothing
//! and the sprite does not draw at all, which is what happened to every one of
//! them.
//!
//! The tables themselves are `STXT` chunks in the chapter movie. Each chapter
//! ships two copies: one written against cast names, and one with those names
//! already resolved to numbers. Only the resolved copy is used here, which the
//! recognition rule below selects for free.

use std::collections::HashMap;

use lingo::{parse_value, Value};

/// Every lookup table a chapter declares, by table name.
#[derive(Default)]
pub struct CastTables {
    tables: HashMap<String, Vec<(String, u32)>>,
}

impl CastTables {
    /// Picks the lookup tables out of a movie's text chunks.
    ///
    /// A table is a property list of integers, keyed by the state value that
    /// selects it. They are collected entry by entry rather than by accepting
    /// or rejecting a whole chunk, because the chunk that holds them also
    /// holds plain frame lists that are not keyed:
    ///
    /// ```text
    /// [#levelDigits: [#ignoreMe: 1333, 0: 1271, 1: 1272, ...],
    ///  #barStartPix: [1231, 1237, 1229, ...],   -- a list, not a table
    ///  #BarSwitch:   [#setON: 1339, #setOFF: 1339, ...]]
    /// ```
    ///
    /// Two other kinds of chunk are close enough in shape to be caught by a
    /// per-entry rule alone, so they are excluded first:
    ///
    ///   - room records, which carry `#earShot: [#houseHum: 224, ...]` -- a
    ///     keyed list of integers by any structural test -- alongside
    ///     `#onStage` and the rest;
    ///   - the state schema, whose `#always: [1]` is a plain list but whose
    ///     `#soundChannels` is keyed.
    ///
    /// The copy of each table written against cast names rather than numbers
    /// fails to parse at all, so it excludes itself.
    pub fn from_texts(texts: &[String]) -> CastTables {
        const NOT_A_TABLE_CHUNK: [&str; 4] = ["onstage", "hotspots", "preload", "always"];

        let mut tables: HashMap<String, Vec<(String, u32)>> = HashMap::new();
        for text in texts {
            let trimmed = text.trim();
            if !trimmed.starts_with("[#") {
                continue;
            }
            let Ok(Value::Props(entries)) = parse_value(trimmed) else { continue };
            if entries.iter().any(|(k, _)| NOT_A_TABLE_CHUNK.contains(&k.as_str())) {
                continue;
            }
            for (name, value) in &entries {
                let rows: Vec<(String, u32)> = match value {
                    Value::Props(rows) => {
                        if rows.is_empty() || !rows.iter().all(|(_, v)| v.as_int().is_some()) {
                            continue;
                        }
                        rows.iter()
                            .filter_map(|(k, v)| {
                                Some((k.to_ascii_lowercase(), v.as_int()? as u32))
                            })
                            .collect()
                    }
                    // Some of them are written as a plain list and read with
                    // `getAt`, which is the same table addressed by position:
                    // `#scanIcon: [6, 979, 982, 985]` is the scan light's
                    // three states with the channel it belongs on in front.
                    Value::List(items) => {
                        if items.is_empty() || !items.iter().all(|v| v.as_int().is_some()) {
                            continue;
                        }
                        items
                            .iter()
                            .enumerate()
                            .filter_map(|(i, v)| Some(((i + 1).to_string(), v.as_int()? as u32)))
                            .collect()
                    }
                    _ => continue,
                };
                tables.insert(name.to_ascii_lowercase(), rows);
            }
        }
        CastTables { tables }
    }

    /// Resolves `table[key]` to a cast number.
    ///
    /// Keys are written as symbols in some tables and as bare integers in
    /// others -- the lock wheels use `1:` through `0:`, the clocks use `#t1.15`
    /// -- so the state value is matched against the key's text either way.
    pub fn lookup(&self, table: &str, key: &Value) -> Option<u32> {
        let rows = self.tables.get(&table.to_ascii_lowercase())?;
        let wanted = match key {
            Value::Int(n) => n.to_string(),
            other => other.as_str()?.trim_start_matches('#').to_string(),
        }
        .to_ascii_lowercase();
        rows.iter().find(|(k, _)| *k == wanted).map(|(_, v)| *v)
    }

    pub fn len(&self) -> usize {
        self.tables.len()
    }

}

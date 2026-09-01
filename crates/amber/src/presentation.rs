//! The chapter's presentation cast table.
//!
//! Each chapter movie's `foreground.DATA` lists the cast members its handlers
//! reach for by name rather than by number: the door static, the radio dial,
//! the headgear, the credit screen. In the original these hang off the
//! puppeteer object and are read with `getProp(oPuppeteer, #doorStatic)`.
//!
//! Only the entries naming a single cast member are kept. The same file also
//! carries the sound bank, the icon table and other nested structures, which
//! have their own readers.

use std::collections::HashMap;

use lingo::parse_value;

#[derive(Default)]
pub struct Presentation {
    casts: HashMap<String, u32>,
}

impl Presentation {
    /// Reads the table from a chapter's text chunks.
    ///
    /// Recognised by the entries every chapter carries: a credit screen and a
    /// credit movie, which are always cast numbers.
    pub fn from_texts(texts: &[String]) -> Presentation {
        let mut casts = HashMap::new();
        for text in texts {
            let trimmed = text.trim();
            if !trimmed.starts_with("[#") && !trimmed.starts_with("[ #") {
                continue;
            }
            if !trimmed.contains("creditScreen") {
                continue;
            }
            let Ok(config) = parse_value(trimmed) else { continue };
            for (name, value) in config.entries() {
                if let Some(cast) = value.as_int() {
                    if cast > 0 {
                        casts.insert(name.clone(), cast as u32);
                    }
                }
            }
        }
        Presentation { casts }
    }

    /// The cast a name refers to, as `getProp(oPuppeteer, #name)` would give.
    pub fn cast(&self, name: &str) -> Option<u32> {
        self.casts
            .get(&name.trim_start_matches('#').to_ascii_lowercase())
            .copied()
    }

    pub fn len(&self) -> usize {
        self.casts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.casts.is_empty()
    }
}

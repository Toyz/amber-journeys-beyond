//! Locating the game's QuickTime movies.
//!
//! Rooms name their movie through the cast member they place on the `#video`
//! channel, e.g. `intro.mov`. The files themselves are spread across a
//! per-chapter `MOVIES` directory plus a shared one, and the disc is
//! case-inconsistent: the scripts say `intro.mov`, the ISO says `INTRO.MOV`.
//! So names are resolved through the catalogue rather than the filesystem,
//! which is what lets the same code read a directory, an ISO or a bundle.

use crate::content::Catalogue;

pub struct MovieIndex {
    /// Upper-case file name -> the path the content spells it with.
    by_name: std::collections::HashMap<String, String>,
}

impl MovieIndex {
    pub fn build(catalogue: &Catalogue) -> MovieIndex {
        let mut by_name = std::collections::HashMap::new();
        for path in catalogue.all() {
            let Some(name) = path.rsplit('/').next() else { continue };
            let upper = name.to_ascii_uppercase();
            if upper.ends_with(".MOV") {
                // First match wins, so a chapter's own copy is not displaced
                // by an identically named one found later.
                by_name.entry(upper).or_insert_with(|| path.to_string());
            }
        }
        MovieIndex { by_name }
    }

    /// Resolves a movie name to a path.
    ///
    /// The suffix in a cast name is not always a file extension: alongside
    /// `intro.mov` the scripts use `BATHSCAN.multiframe`, which marks how the
    /// movie is played rather than what it is called. Everything after the last
    /// dot is therefore treated as advisory, and `.MOV` is what is on disc.
    pub fn find(&self, name: &str) -> Option<&str> {
        let key = name.trim().to_ascii_uppercase();
        let stem = key.rsplit_once('.').map(|(base, _)| base).unwrap_or(&key);
        self.by_name
            .get(&key)
            .or_else(|| self.by_name.get(&format!("{stem}.MOV")))
            .map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.by_name.len()
    }
}

//! Locating the game's QuickTime movies on disc.
//!
//! Rooms name their movie through the cast member they place on the `#video`
//! channel, e.g. `intro.mov`. The files themselves are spread across a
//! per-chapter `MOVIES` directory plus a shared one, and the disc is
//! case-inconsistent: the scripts say `intro.mov`, the ISO says `INTRO.MOV`.
//! So the index is built once by walking the tree and keyed case-insensitively.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub struct MovieIndex {
    by_name: HashMap<String, PathBuf>,
}

impl MovieIndex {
    pub fn build(root: &Path) -> MovieIndex {
        let mut by_name = HashMap::new();
        walk(root, 0, &mut by_name);
        MovieIndex { by_name }
    }

    /// Resolves a movie name to a file.
    ///
    /// The suffix in a cast name is not always a file extension: alongside
    /// `intro.mov` the scripts use `BATHSCAN.multiframe`, which marks how the
    /// movie is played rather than what it is called. Everything after the last
    /// dot is therefore treated as advisory, and `.MOV` is what is on disc.
    pub fn find(&self, name: &str) -> Option<&Path> {
        let key = name.trim().to_ascii_uppercase();
        let stem = key.rsplit_once('.').map(|(base, _)| base).unwrap_or(&key);
        self.by_name
            .get(&key)
            .or_else(|| self.by_name.get(&format!("{stem}.MOV")))
            .map(PathBuf::as_path)
    }

    pub fn len(&self) -> usize {
        self.by_name.len()
    }

}

fn walk(dir: &Path, depth: usize, out: &mut HashMap<String, PathBuf>) {
    // The disc is shallow; this bound just stops a symlink loop from hanging.
    if depth > 4 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, depth + 1, out);
        } else if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            let upper = name.to_ascii_uppercase();
            if upper.ends_with(".MOV") {
                // First match wins, so a chapter's own copy is not displaced by
                // an identically named one found later.
                out.entry(upper).or_insert(path);
            }
        }
    }
}

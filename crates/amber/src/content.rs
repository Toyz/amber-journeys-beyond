//! Where the game's data comes from.
//!
//! The engine reads the disc in exactly two ways: it walks the tree once to
//! learn what is there, and then reads whole files by name. Nothing streams
//! and nothing is written. Two methods is the whole of it, which is why this
//! is a trait rather than a path -- an ISO image, a zip, a bundle compiled
//! into a wasm binary and a directory all answer the same two questions.
//!
//! Paths are relative to the root and separated by `/`, whatever the host
//! uses. They are compared without case: the scripts say `intro.mov`, the ISO
//! says `INTRO.MOV`, and the Macintosh half of the hybrid disc disagrees with
//! the PC half about several directory names.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A source of game data.
pub trait Content: Send + Sync {
    /// Every file, as a path relative to the root.
    ///
    /// Called once at startup to build the indexes; order does not matter.
    fn list(&self) -> Vec<String>;

    /// The bytes of one file, addressed as `list` returned it.
    ///
    /// Returns `None` for anything that is not there, which the callers treat
    /// as "the disc does not have this" rather than as an error: the game
    /// names a handful of movies that were never shipped.
    fn read(&self, path: &str) -> Option<Vec<u8>>;

    /// Asks for a file that is not to hand yet, and says whether it is coming.
    ///
    /// A directory and an image both have everything already, so the default
    /// is "no, and it never will be". A source that fetches over a network
    /// answers `true` and starts the fetch; the engine then holds rather than
    /// carrying on without it, which is the difference between a film that
    /// arrives late and a film that is silently skipped.
    ///
    /// It must be cheap to call repeatedly with the same path: the engine asks
    /// again every frame until the bytes turn up.
    fn request(&self, path: &str) -> bool {
        let _ = path;
        false
    }
}

/// A directory on the host filesystem, plus any fallback roots.
///
/// The fallbacks exist because the game can be installed to a hard disc with
/// the bulky media left on the CD, so a name may resolve in either place. The
/// first root that has a file wins, which keeps a chapter's own copy of a
/// movie ahead of an identically named one somewhere else.
pub struct Files {
    roots: Vec<PathBuf>,
}

impl Files {
    pub fn new(root: &Path) -> Files {
        let mut roots = vec![root.to_path_buf()];
        roots.extend(crate::fallback_roots());
        Files { roots }
    }

    fn walk(dir: &Path, prefix: &str, depth: usize, out: &mut Vec<String>) {
        // The disc is shallow; this bound only stops a symlink loop hanging.
        if depth > 4 {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let here = if prefix.is_empty() {
                name.to_string()
            } else {
                format!("{prefix}/{name}")
            };
            if path.is_dir() {
                Files::walk(&path, &here, depth + 1, out);
            } else {
                out.push(here);
            }
        }
    }
}

impl Content for Files {
    fn list(&self) -> Vec<String> {
        let mut out = Vec::new();
        for root in &self.roots {
            Files::walk(root, "", 0, &mut out);
        }
        out
    }

    fn read(&self, path: &str) -> Option<Vec<u8>> {
        for root in &self.roots {
            let mut full = root.clone();
            for part in path.split('/') {
                full.push(part);
            }
            if let Ok(bytes) = std::fs::read(&full) {
                return Some(bytes);
            }
        }
        None
    }
}

/// An index of what a `Content` holds, keyed so a name can be found without
/// knowing how the disc spells it.
///
/// Built once. Everything above it -- movies, sounds, the chapter movies and
/// the room `.DAT` files -- asks this rather than the filesystem.
pub struct Catalogue {
    /// Upper-case path -> the path as the source spells it.
    by_path: HashMap<String, String>,
    paths: Vec<String>,
}

impl Catalogue {
    pub fn build(content: &dyn Content) -> Catalogue {
        let paths = content.list();
        let mut by_path = HashMap::new();
        for path in &paths {
            by_path
                .entry(path.to_ascii_uppercase())
                .or_insert_with(|| path.clone());
        }
        Catalogue { by_path, paths }
    }

    /// The path a full relative path resolves to, whatever its case.
    pub fn by_path(&self, path: &str) -> Option<&str> {
        self.by_path.get(&path.trim().to_ascii_uppercase()).map(String::as_str)
    }

    /// A file inside a directory, matched without case. This is what
    /// `find_ci` was: the Macintosh half of the disc capitalises several
    /// directory names differently from the PC half.
    pub fn in_dir(&self, dir: &str, name: &str) -> Option<&str> {
        self.by_path(&format!("{dir}/{name}"))
    }

    /// Every path directly inside a directory, matched without case.
    pub fn dir(&self, dir: &str) -> Vec<&str> {
        let want = format!("{}/", dir.trim_end_matches('/').to_ascii_uppercase());
        self.paths
            .iter()
            .filter(|p| {
                let upper = p.to_ascii_uppercase();
                upper.starts_with(&want) && !upper[want.len()..].contains('/')
            })
            .map(String::as_str)
            .collect()
    }

    /// How many files the source holds, which is the first thing to know when
    /// it holds none of the ones expected.
    pub fn file_count(&self) -> usize {
        self.paths.len()
    }

    /// Every path, for an index that wants to pick its own out.
    pub fn all(&self) -> impl Iterator<Item = &str> {
        self.paths.iter().map(String::as_str)
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `Content` that is only a list, which is what a bundle compiled into
    /// a wasm binary looks like.
    struct Bundle(Vec<(&'static str, &'static [u8])>);

    impl Content for Bundle {
        fn list(&self) -> Vec<String> {
            self.0.iter().map(|(p, _)| (*p).to_string()).collect()
        }
        fn read(&self, path: &str) -> Option<Vec<u8>> {
            self.0
                .iter()
                .find(|(p, _)| p.eq_ignore_ascii_case(path))
                .map(|(_, b)| b.to_vec())
        }
    }

    fn disc() -> Bundle {
        Bundle(vec![
            ("ROXY/ROXY.DXR", b"movie"),
            ("ROXY/MOVIES/INTRO.MOV", b"film"),
            ("ROXY/ROXY_1.DAT", b"rooms"),
            ("EDWIN/MOVIES_E/B.MOV", b"drive"),
        ])
    }

    #[test]
    fn a_film_is_found_however_the_disc_spells_it() {
        let index = crate::media::MovieIndex::build(&Catalogue::build(&disc()));
        assert_eq!(index.find("intro.mov"), Some("ROXY/MOVIES/INTRO.MOV"));
        // The suffix is advisory: `BATHSCAN.multiframe` is `BATHSCAN.MOV`.
        assert_eq!(index.find("B.multiframe"), Some("EDWIN/MOVIES_E/B.MOV"));
        assert_eq!(index.find("nothing.mov"), None);
    }

    #[test]
    fn a_chapter_movie_is_found_beside_its_chapter() {
        let cat = Catalogue::build(&disc());
        assert_eq!(cat.in_dir("roxy", "ROXY.DXR"), Some("ROXY/ROXY.DXR"));
        assert_eq!(cat.in_dir("ROXY", "roxy.dxr"), Some("ROXY/ROXY.DXR"));
        assert_eq!(cat.in_dir("EDWIN", "ROXY.DXR"), None);
    }

    #[test]
    fn a_directory_lists_what_is_directly_in_it() {
        let cat = Catalogue::build(&disc());
        let mut roxy = cat.dir("ROXY");
        roxy.sort();
        // Not `MOVIES/INTRO.MOV`, which is a directory deeper.
        assert_eq!(roxy, vec!["ROXY/ROXY.DXR", "ROXY/ROXY_1.DAT"]);
    }
}

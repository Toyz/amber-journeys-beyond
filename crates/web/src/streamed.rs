//! Content fetched from a server, a file at a time.
//!
//! The other way to run this is to hand the page a disc image, which is five
//! hundred and seventy megabytes before anything is drawn. Serving the game as
//! files instead means the first load is the room data and the chapter the
//! player starts in, and everything else arrives as it is reached.
//!
//! The engine reads synchronously and a browser fetches asynchronously, which
//! is the whole of the difficulty. It is answered by
//! [`Content::request`](amber::content::Content::request): a read that misses
//! says so, the fetch starts, and the engine holds the film's wait until the
//! bytes turn up rather than carrying on without them.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use amber::content::Content;

/// What the page has fetched, and what it has been asked for.
///
/// Shared with the page: JavaScript owns the fetching, because that is the
/// half that can await. This side only records.
#[derive(Default)]
pub struct Store {
    files: HashMap<String, Vec<u8>>,
    wanted: HashSet<String>,
    paths: Vec<String>,
}

/// A handle the page and the engine both hold.
#[derive(Clone, Default)]
pub struct Streamed(Rc<RefCell<Store>>);

// The engine's trait asks for `Send + Sync` because a desktop mixes on another
// thread. A browser tab is one thread and this never leaves it.
unsafe impl Send for Streamed {}
unsafe impl Sync for Streamed {}

impl Streamed {
    /// The manifest: every path the server will serve, which is what `list`
    /// answers and what the catalogue is built from.
    pub fn with_manifest(paths: Vec<String>) -> Streamed {
        let this = Streamed::default();
        this.0.borrow_mut().paths = paths;
        this
    }

    /// Hands over a file the page has fetched.
    pub fn put(&self, path: &str, bytes: Vec<u8>) {
        let mut store = self.0.borrow_mut();
        store.wanted.remove(&path.to_ascii_uppercase());
        store.files.insert(path.to_ascii_uppercase(), bytes);
    }

    /// What the engine has asked for and not been given, for the page to go
    /// and fetch. Taking the list clears it, so each path is fetched once.
    pub fn take_wanted(&self) -> Vec<String> {
        let mut store = self.0.borrow_mut();
        store.wanted.drain().collect()
    }
}

impl Content for Streamed {
    fn list(&self) -> Vec<String> {
        self.0.borrow().paths.clone()
    }

    fn read(&self, path: &str) -> Option<Vec<u8>> {
        self.0.borrow().files.get(&path.to_ascii_uppercase()).cloned()
    }

    fn request(&self, path: &str) -> bool {
        let key = path.to_ascii_uppercase();
        let mut store = self.0.borrow_mut();
        // Only worth waiting for if the server has it at all.
        if !store.paths.iter().any(|p| p.eq_ignore_ascii_case(path)) {
            return false;
        }
        store.wanted.insert(key);
        true
    }
}

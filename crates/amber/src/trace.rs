//! Engine event log.
//!
//! Every bug found by playing this engine so far has been a case of the engine
//! doing something reasonable for a reason that was invisible from the outside:
//! a guard that read as vacuously true, a sprite filtered out before it drew, a
//! setter that missed its dispatch and let the fallback write happen anyway. In
//! each case the symptom was one step removed from the cause, and finding it
//! meant adding a `eprintln!` and building again.
//!
//! This is that, made permanent and uniform. Nothing is logged unless asked
//! for:
//!
//! ```text
//! AMBER_TRACE=audio,script amber play extract
//! AMBER_TRACE=all AMBER_TRACE_FILE=/tmp/run.log amber play extract
//! ```
//!
//! Records carry the frame and the room, because "what was on screen when this
//! happened" is the first question every time.

use std::fmt::Arguments;
use std::fs::File;
use std::io::Write;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

/// What a record is about. Topics are selected by name in `AMBER_TRACE`.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Topic {
    /// Room changes and the state that gates them.
    Room,
    /// Handler dispatch, and handlers that fell through unported.
    Script,
    /// Flag writes.
    State,
    /// What drew, and what was asked to draw and could not.
    Sprite,
    /// Loops, one-shots and the mix.
    Audio,
    /// Movie playback.
    Video,
}

impl Topic {
    fn bit(self) -> u32 {
        1 << self as u32
    }

    fn label(self) -> &'static str {
        match self {
            Topic::Room => "room",
            Topic::Script => "script",
            Topic::State => "state",
            Topic::Sprite => "sprite",
            Topic::Audio => "audio",
            Topic::Video => "video",
        }
    }

    fn all() -> [Topic; 6] {
        [
            Topic::Room,
            Topic::Script,
            Topic::State,
            Topic::Sprite,
            Topic::Audio,
            Topic::Video,
        ]
    }
}

static MASK: OnceLock<u32> = OnceLock::new();
static SINK: OnceLock<Option<Mutex<File>>> = OnceLock::new();
static FRAME: AtomicU64 = AtomicU64::new(0);
static ROOM: OnceLock<Mutex<String>> = OnceLock::new();
static DROPPED: AtomicU32 = AtomicU32::new(0);
static PROBING: AtomicU32 = AtomicU32::new(0);

/// Marks everything recorded while it is alive as speculative.
///
/// The walkthrough lists a room's exits by running each hotspot's actions
/// against a copy of the state, and `verify` sweeps the whole game the same
/// way. Those runs call handlers and write flags exactly as a real click does,
/// on a copy that is then thrown away. Without this they read in the log as
/// things the game did.
pub struct Probe;

impl Probe {
    pub fn begin() -> Probe {
        PROBING.fetch_add(1, Ordering::Relaxed);
        Probe
    }
}

impl Drop for Probe {
    fn drop(&mut self) {
        PROBING.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Turns the named topics into a bit mask, complaining about typos.
fn bits_for(spec: &str) -> u32 {
    let spec = spec.trim();
    if spec.eq_ignore_ascii_case("all") || spec == "*" {
        return Topic::all().iter().map(|t| t.bit()).sum();
    }
    let mut bits = 0;
    for want in spec.split([',', ' ', '+']).filter(|s| !s.is_empty()) {
        match Topic::all().iter().find(|t| t.label().eq_ignore_ascii_case(want)) {
            Some(t) => bits |= t.bit(),
            // Naming a topic that does not exist is a typo in a debugging
            // session, and silently tracing nothing is the worst possible
            // answer to it.
            None => eprintln!(
                "no trace topic {want:?}; known topics are {}",
                Topic::all()
                    .iter()
                    .map(|t| t.label())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
    bits
}

/// Turns tracing on from the command line rather than the environment.
///
/// `AMBER_TRACE` and `AMBER_TRACE_FILE` still work and are still what a
/// script would use, but nobody wants to type them to answer "what did the
/// game just do". Call this before anything traces; both settings latch on
/// first use, so a later call is ignored.
pub fn configure(spec: &str, path: Option<&std::path::Path>) {
    let _ = MASK.set(bits_for(spec));
    if let Some(path) = path {
        let sink = match File::create(path) {
            Ok(f) => Some(Mutex::new(f)),
            Err(e) => {
                eprintln!("{}: {e}", path.display());
                None
            }
        };
        let _ = SINK.set(sink);
    }
}

fn mask() -> u32 {
    *MASK.get_or_init(|| match std::env::var("AMBER_TRACE") {
        Ok(spec) => bits_for(&spec),
        Err(_) => 0,
    })
}

/// Whether anything is listening for this topic.
///
/// Call this before doing work only needed for a record; the macro already
/// guards the formatting itself.
pub fn enabled(topic: Topic) -> bool {
    mask() & topic.bit() != 0
}

/// Advances the frame counter that records are stamped with.
pub fn frame(n: u64) {
    FRAME.store(n, Ordering::Relaxed);
}

/// Records which room the following events happen in.
pub fn room(name: &str) {
    if mask() == 0 {
        return;
    }
    if let Ok(mut slot) = ROOM.get_or_init(|| Mutex::new(String::new())).lock() {
        slot.clear();
        slot.push_str(name);
    }
}

#[doc(hidden)]
pub fn record(topic: Topic, args: Arguments<'_>) {
    let here = ROOM
        .get()
        .and_then(|r| r.lock().ok())
        .map(|r| r.clone())
        .unwrap_or_default();
    // A leading `~` marks a speculative run: something the engine worked out
    // on a throwaway copy of the state, not something that happened.
    let speculative = if PROBING.load(Ordering::Relaxed) > 0 { "~" } else { " " };
    let line = format!(
        "[{:>7}]{speculative}{:<6} {:<24} {args}\n",
        FRAME.load(Ordering::Relaxed),
        topic.label(),
        here
    );

    let sink = SINK.get_or_init(|| {
        let path = std::env::var("AMBER_TRACE_FILE").ok()?;
        match File::create(&path) {
            Ok(f) => Some(Mutex::new(f)),
            Err(e) => {
                eprintln!("AMBER_TRACE_FILE {path}: {e}");
                None
            }
        }
    });

    match sink {
        Some(file) => {
            // A trace that stalls the audio thread would change the behaviour
            // it is meant to observe, so a contended or failed write is counted
            // and dropped rather than waited on.
            match file.try_lock() {
                Ok(mut f) => {
                    if f.write_all(line.as_bytes()).is_err() {
                        DROPPED.fetch_add(1, Ordering::Relaxed);
                    }
                }
                Err(_) => {
                    DROPPED.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
        None => eprint!("{line}"),
    }
}

/// Writes one record, if its topic is selected.
///
/// The arguments are not evaluated when the topic is off, so a record may call
/// something expensive.
#[macro_export]
macro_rules! trace {
    ($topic:expr, $($arg:tt)*) => {
        if $crate::trace::enabled($topic) {
            $crate::trace::record($topic, format_args!($($arg)*));
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topics_have_distinct_bits() {
        let mut seen = 0u32;
        for t in Topic::all() {
            assert_eq!(seen & t.bit(), 0, "{t:?} collides");
            seen |= t.bit();
        }
    }

    #[test]
    fn every_topic_has_a_label_and_they_are_unique() {
        let mut labels: Vec<&str> = Topic::all().iter().map(|t| t.label()).collect();
        let count = labels.len();
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(labels.len(), count, "duplicate topic label");
        assert!(labels.iter().all(|l| !l.is_empty()));
    }
}

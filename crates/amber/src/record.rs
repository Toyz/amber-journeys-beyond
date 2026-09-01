//! Recording a session as a walkthrough script.
//!
//! Reproducing a fault has been the slow half of every bug this session: helba
//! plays until something goes wrong, describes where, and I try to reach the
//! same state from the terminal. A recording removes that step. `play` writes
//! every click as the command `walk` would take, so the exact route can be
//! replayed -- with any trace topic turned on -- and I can look at the state at
//! the point it went wrong rather than at a reconstruction of it.
//!
//! ```text
//! AMBER_RECORD=/tmp/run.walk amber play extract
//! amber walk extract --replay /tmp/run.walk
//! AMBER_TRACE=all amber walk extract --replay /tmp/run.walk
//! ```
//!
//! The file is plain text and can be edited: trimming the tail is how a route
//! gets shortened to the smallest one that still fails.

use std::fs::File;
use std::io::Write;
use std::sync::Mutex;
use std::sync::OnceLock;

static SINK: OnceLock<Option<Mutex<File>>> = OnceLock::new();

fn sink() -> Option<&'static Mutex<File>> {
    SINK.get_or_init(|| {
        let path = std::env::var("AMBER_RECORD").ok()?;
        match File::create(&path) {
            Ok(mut f) => {
                let _ = writeln!(
                    f,
                    "# amber session recording\n\
                     # replay with: amber walk <dir> --replay <this file>"
                );
                eprintln!("recording to {path}");
                Some(Mutex::new(f))
            }
            Err(e) => {
                eprintln!("AMBER_RECORD {path}: {e}");
                None
            }
        }
    })
    .as_ref()
}

/// Whether anything is being recorded, for callers that would otherwise do
/// work to produce a line.
pub fn active() -> bool {
    sink().is_some()
}

/// Writes one command, exactly as `walk` would read it.
pub fn step(command: &str) {
    let Some(file) = sink() else { return };
    if let Ok(mut f) = file.lock() {
        let _ = writeln!(f, "{command}");
        // Flushed each step so a recording survives the crash it was made to
        // capture.
        let _ = f.flush();
    }
}

/// Writes a comment, which `walk` ignores on replay.
pub fn note(text: &str) {
    if sink().is_some() {
        step(&format!("# {text}"));
    }
}

//! The window, the pointer and the keyboard.
//!
//! The engine composes a 640 by 480 frame of `0x00RRGGBB` pixels, asks what
//! the pointer and keyboard are doing, and hands the frame over. That is the
//! whole of what it wants from a platform, so it is a trait: a desktop window,
//! a canvas in a browser and a test harness all answer the same five
//! questions.
//!
//! Deliberately not here: audio, which has its own seam in `audio`, and the
//! game's own cursor, which is drawn into the frame rather than asked of the
//! platform -- the original draws it too, and a platform pointer would be a
//! second one sitting on top.

/// The keys the game uses. Not a general keyboard: four keys, all of them
/// conveniences this engine added rather than anything the original had.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Key {
    /// Quit.
    Escape,
    /// Skip the film that is playing.
    Space,
    /// Show where the live hotspots are.
    Hotspots,
    /// Print what is on the stage, bottom to top.
    Stage,
}

/// What the platform saw this frame.
pub struct Input {
    /// The pointer in stage coordinates, or `None` when it is outside.
    pub pointer: Option<(i32, i32)>,
    /// Whether the primary button is down now. The engine acts on the release
    /// edge, so it wants the level rather than an event.
    pub down: bool,
    /// Keys that went down this frame, without repeats.
    pub pressed: Vec<Key>,
    /// Whether the window is still there.
    pub open: bool,
}

/// Somewhere to show a frame and something to drive it.
pub trait Host {
    /// Polls the platform once. The stage is always 640 by 480; a host that
    /// scales or letterboxes maps the pointer back itself, because it is the
    /// only thing that knows how it did the scaling.
    fn poll(&mut self, stage: (usize, usize)) -> Input;

    /// Shows a composed frame, `stage.0 * stage.1` pixels of `0x00RRGGBB`.
    fn present(&mut self, frame: &[u32], stage: (usize, usize)) -> std::io::Result<()>;

    /// Names the window. Called only when the name changes.
    fn set_title(&mut self, title: &str);
}

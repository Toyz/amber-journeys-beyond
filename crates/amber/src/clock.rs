//! What time it is, for a platform that may not have a clock.
//!
//! The engine asks the time in three places -- the film that is playing, the
//! waits a sequence holds on, and the two things that happen on their own (the
//! ghost calls and Edwin's carols). All three want the same thing: how long
//! since the engine started, in seconds.
//!
//! On a desktop that is `Instant`. On `wasm32-unknown-unknown` there is no
//! clock at all and `Instant::now()` panics, so the host supplies one: it
//! knows `performance.now()` and calls [`advance`] once a frame, and
//! everything below reads what it last said.
//!
//! Both ends go through [`now`], so nothing else in the engine ever names a
//! platform's idea of time.

/// Seconds since the engine started, not counting time spent held.
pub fn now() -> f64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let wall = wall().lock().unwrap_or_else(|e| e.into_inner());
        let holding = wall.since.map_or(0.0, |at| at.elapsed().as_secs_f64());
        wall.start.elapsed().as_secs_f64() - wall.lost - holding
    }
    #[cfg(target_arch = "wasm32")]
    {
        SUPPLIED.with(|t| t.get())
    }
}

/// The wall clock, and how much of it the engine is not counting.
#[cfg(not(target_arch = "wasm32"))]
struct Wall {
    start: std::time::Instant,
    /// Seconds already spent held, which `now` has subtracted for good.
    lost: f64,
    /// When the hold that is running began, if one is.
    since: Option<std::time::Instant>,
}

#[cfg(not(target_arch = "wasm32"))]
fn wall() -> &'static std::sync::Mutex<Wall> {
    static WALL: std::sync::OnceLock<std::sync::Mutex<Wall>> = std::sync::OnceLock::new();
    WALL.get_or_init(|| {
        std::sync::Mutex::new(Wall {
            start: std::time::Instant::now(),
            lost: 0.0,
            since: None,
        })
    })
}

/// Stops the clock, and starts it again where it stopped.
///
/// For a host whose app can be taken off the screen. Android does that
/// whenever it likes, and the engine reads the time to decide how far through
/// the film that is playing it should be -- so an unheld clock means coming
/// back from a phone call to find a ninety-second film over, a wait that was
/// counting down expired, and every ghost that was due to call already gone.
///
/// Held time is *lost*, not queued: the game resumes at the moment it left,
/// which is what a player who put their phone in their pocket expects to find.
///
/// A browser needs none of this and gets none: a hidden tab stops being given
/// animation frames, so its host simply stops calling [`advance`] and the
/// clock holds itself.
#[allow(unused_variables)]
pub fn hold(held: bool) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut wall = wall().lock().unwrap_or_else(|e| e.into_inner());
        match (held, wall.since) {
            (true, None) => wall.since = Some(std::time::Instant::now()),
            (false, Some(at)) => {
                wall.lost += at.elapsed().as_secs_f64();
                wall.since = None;
            }
            // Already in the state asked for. Holding a held clock twice must
            // not stack, or releasing it once would leave it running slow for
            // ever.
            _ => {}
        }
    }
}

#[cfg(target_arch = "wasm32")]
thread_local! {
    /// Seconds since the first `advance`, not since whatever the host counts
    /// from.
    static SUPPLIED: std::cell::Cell<f64> = const { std::cell::Cell::new(0.0) };
    /// What the host's clock read the first time it said anything.
    static BASE: std::cell::Cell<Option<f64>> = const { std::cell::Cell::new(None) };
}

/// Tells the engine what time it is, for a host that has to.
///
/// The number is taken as a reading of the host's own clock, not as a time
/// since the engine started -- a browser's `performance.now()` counts from
/// when the page loaded, which by the time the game opens is however long the
/// disc took to arrive. So the first reading becomes zero and everything after
/// it is measured from there.
///
/// Getting this wrong is not subtle: a film opened before the host said
/// anything starts at zero, the next reading jumps to whatever the page's
/// clock had reached, and the film is either instantly over or -- if it loops
/// -- restarts on every frame.
///
/// Ignored where the platform has a clock of its own, so a host may call it
/// unconditionally.
#[allow(unused_variables)]
pub fn advance(seconds: f64) {
    #[cfg(target_arch = "wasm32")]
    {
        let base = BASE.with(|b| {
            if b.get().is_none() {
                b.set(Some(seconds));
            }
            b.get().unwrap_or(seconds)
        });
        SUPPLIED.with(|t| t.set((seconds - base).max(0.0)));
    }
}

/// A moment, as this engine counts them: seconds since it started.
///
/// Deliberately a plain number rather than a wrapper. Everything done with it
/// is a comparison or an addition of seconds, and `Instant` bought nothing
/// beyond a type that does not exist everywhere.
pub type Moment = f64;

#[cfg(test)]
mod tests {
    use super::*;

    /// The clock runs forward, and holding it stops it where it is.
    ///
    /// One test rather than two because there is one clock: a hold taken in
    /// another test's thread is a hold taken in this one, and split in two
    /// these raced each other into a clock that appeared to run backwards.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn the_clock_moves_forward_and_can_be_held() {
        let first = now();
        let second = now();
        assert!(second >= first, "time does not run backwards");

        // A held clock does not move, and starts again where it stopped.
        //
        // The number that matters is the second one: the engine measures a film
        // from the reading it was opened at, so a clock that came back having
        // counted the time away would put the film however long further on -- and
        // on a phone that is "I answered a call and the scene was over".

        let before = now();
        hold(true);
        let stopped = now();
        std::thread::sleep(std::time::Duration::from_millis(60));
        let still = now();
        assert!(
            (still - stopped).abs() < 0.02,
            "the clock ran while it was held: {stopped} then {still}"
        );

        hold(false);
        let after = now();
        assert!(
            after - before < 0.05,
            "the clock caught up on release: {before} then {after}"
        );

        // Releasing an already-running clock is not an error and must not
        // credit the time twice.
        hold(false);
        assert!(now() >= after, "time went backwards");
    }

    /// A host's clock counts from whenever it feels like -- a browser's from
    /// when the page loaded, which is long before the game opens. The engine's
    /// starts at zero whatever the host's reads, because a film opened before
    /// the first reading would otherwise be handed a start time of nothing and
    /// a first tick of however long the disc took to arrive.
    #[test]
    #[cfg(target_arch = "wasm32")]
    fn a_hosts_clock_is_measured_from_its_first_reading() {
        advance(1_234.5);
        assert_eq!(now(), 0.0);
        advance(1_236.0);
        assert_eq!(now(), 1.5);
    }
}

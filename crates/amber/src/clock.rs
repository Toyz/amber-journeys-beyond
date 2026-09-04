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

/// Seconds since the engine started.
pub fn now() -> f64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::sync::OnceLock;
        use std::time::Instant;
        static START: OnceLock<Instant> = OnceLock::new();
        START.get_or_init(Instant::now).elapsed().as_secs_f64()
    }
    #[cfg(target_arch = "wasm32")]
    {
        SUPPLIED.with(|t| t.get())
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

    #[test]
    fn the_clock_moves_forward() {
        let first = now();
        let second = now();
        assert!(second >= first, "time does not run backwards");
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

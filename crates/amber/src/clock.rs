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
    static SUPPLIED: std::cell::Cell<f64> = const { std::cell::Cell::new(0.0) };
}

/// Tells the engine what time it is, for a host that has to.
///
/// Ignored where the platform has a clock of its own, so a host may call it
/// unconditionally.
#[allow(unused_variables)]
pub fn advance(seconds: f64) {
    #[cfg(target_arch = "wasm32")]
    SUPPLIED.with(|t| t.set(seconds));
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
}

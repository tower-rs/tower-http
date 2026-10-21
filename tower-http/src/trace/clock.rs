use std::time::{Duration, Instant};

/// Extension trait for duration types that need to be formatted as latencies.
///
/// The [`Clock`] trait's `Duration` associated type must implement this trait,
/// which provides conversion methods used by [`Latency`](super::Latency).
pub trait DurationExt {
    /// Returns the duration in seconds as an `f64`.
    fn as_secs_f64(&self) -> f64;
    /// Returns the duration in whole milliseconds.
    fn as_millis(&self) -> u128;
    /// Returns the duration in whole microseconds.
    fn as_micros(&self) -> u128;
    /// Returns the duration in whole nanoseconds.
    fn as_nanos(&self) -> u128;
}

impl DurationExt for Duration {
    fn as_secs_f64(&self) -> f64 {
        Duration::as_secs_f64(self)
    }
    fn as_millis(&self) -> u128 {
        Duration::as_millis(self)
    }
    fn as_micros(&self) -> u128 {
        Duration::as_micros(self)
    }
    fn as_nanos(&self) -> u128 {
        Duration::as_nanos(self)
    }
}

/// An abstract clock for measuring time.
///
/// Used by the trace middleware to record latencies and stream durations.
/// The default implementation ([`DefaultClock`]) uses [`std::time::Instant`]
/// and [`std::time::Duration`], but custom clocks can be provided — for
/// example, to mock time in tests or to use a WASM-compatible time source.
pub trait Clock: Copy {
    /// An instant in time.
    type Instant: Copy + Send;

    /// A duration of time.
    type Duration: Copy + DurationExt + Send;

    /// Returns the current instant.
    fn now(&self) -> Self::Instant;

    /// Returns the duration elapsed from `instant` to now.
    fn elapsed(&self, instant: Self::Instant) -> Self::Duration;
}

/// The default [`Clock`] implementation, backed by [`std::time::Instant`].
#[derive(Debug, Clone, Copy, Default)]
pub struct DefaultClock;

impl DefaultClock {
    /// Create a new `DefaultClock`.
    pub fn new() -> Self {
        Self
    }
}

impl Clock for DefaultClock {
    type Instant = Instant;
    type Duration = Duration;

    #[inline]
    fn now(&self) -> Instant {
        Instant::now()
    }

    #[inline]
    fn elapsed(&self, instant: Instant) -> Duration {
        instant.elapsed()
    }
}

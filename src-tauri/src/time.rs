//! Clock abstraction for time-dependent logic
//!
//! This module provides a `Clock` trait that allows for easy mocking of time in tests.

use chrono::{DateTime, Utc};

/// Trait for getting the current time
pub trait Clock: Send + Sync {
    /// Returns the current time
    fn now(&self) -> DateTime<Utc>;
}

/// System clock that returns the actual current time
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

/// Fixed clock for testing - always returns the same time
#[cfg(test)]
#[derive(Debug, Clone)]
pub struct FixedClock(pub DateTime<Utc>);

#[cfg(test)]
impl Clock for FixedClock {
    fn now(&self) -> DateTime<Utc> {
        self.0
    }
}

#[cfg(test)]
impl FixedClock {
    /// Creates a new fixed clock at the given time
    pub fn new(time: DateTime<Utc>) -> Self {
        Self(time)
    }

    /// Creates a fixed clock at the current time
    pub fn now() -> Self {
        Self(Utc::now())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn system_clock_returns_current_time() {
        let clock = SystemClock;
        let before = Utc::now();
        let clock_time = clock.now();
        let after = Utc::now();

        assert!(clock_time >= before);
        assert!(clock_time <= after);
    }

    #[test]
    fn fixed_clock_returns_fixed_time() {
        let fixed_time = Utc.with_ymd_and_hms(2024, 6, 15, 12, 0, 0).unwrap();
        let clock = FixedClock::new(fixed_time);

        assert_eq!(clock.now(), fixed_time);
        assert_eq!(clock.now(), fixed_time); // Same value on repeated calls
    }
}

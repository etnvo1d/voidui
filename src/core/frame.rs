//! Event-driven frame scheduling, independent of windows and graphics devices.
use std::time::{Duration, Instant};

/// Bounded retry policy for temporarily unavailable surfaces or GPU recovery.
#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    pub initial_delay: Duration,
    pub maximum_delay: Duration,
    pub attempts: u32,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            initial_delay: Duration::from_millis(16),
            maximum_delay: Duration::from_secs(1),
            attempts: 8,
        }
    }
}

/// One pending redraw at most. Idle, hidden, and zero-sized windows have no timer.
pub(crate) struct FrameSchedule {
    pub(crate) needs_present: bool,
    pending: bool,
    available: bool,
    failures: u32,
    retry_at: Option<Instant>,
    animation_at: Option<Instant>,
    policy: RetryPolicy,
}

impl FrameSchedule {
    pub(crate) fn new(policy: RetryPolicy) -> Self {
        assert!(policy.attempts > 0, "frame retry attempts must be positive");
        assert!(
            !policy.initial_delay.is_zero() && policy.maximum_delay >= policy.initial_delay,
            "frame retry delays must be positive and ordered"
        );
        Self {
            needs_present: true,
            pending: false,
            available: true,
            failures: 0,
            retry_at: None,
            animation_at: None,
            policy,
        }
    }

    /// New external changes start a fresh opportunity to present.
    pub(crate) fn invalidate(&mut self) {
        self.needs_present = true;
        self.failures = 0;
        self.retry_at = None;
        self.animation_at = None;
    }

    pub(crate) fn set_available(&mut self, available: bool) {
        if self.available != available {
            self.available = available;
            self.pending = false;
            self.retry_at = None;
            self.animation_at = None;
            if available {
                self.invalidate();
            }
        }
    }

    pub(crate) fn request(&mut self, now: Instant) -> bool {
        if self.animation_at.is_some_and(|deadline| deadline <= now) {
            self.needs_present = true;
        }

        if !self.available
            || !self.needs_present
            || self.pending
            || self.failures >= self.policy.attempts
        {
            return false;
        }
        if self.retry_at.is_some_and(|deadline| deadline > now) {
            return false;
        }
        self.pending = true;
        self.retry_at = None;
        self.animation_at = None;
        true
    }

    /// OS exposure events can request a frame without invalidating cached content.
    pub(crate) fn begin(&mut self, now: Instant) -> bool {
        self.pending = false;
        self.available && self.retry_at.is_none_or(|deadline| deadline <= now)
    }

    pub(crate) fn presented(&mut self) {
        self.pending = false;
        self.needs_present = false;
        self.failures = 0;
        self.retry_at = None;
        self.animation_at = None;
    }

    pub(crate) fn failed(&mut self, now: Instant) {
        self.needs_present = true;
        self.pending = false;
        self.failures = self.failures.saturating_add(1);
        self.animation_at = None;
        if self.available && self.failures < self.policy.attempts {
            let multiplier = 1u32
                .checked_shl(self.failures.saturating_sub(1))
                .unwrap_or(u32::MAX);
            let delay = self
                .policy
                .initial_delay
                .saturating_mul(multiplier)
                .min(self.policy.maximum_delay);
            self.retry_at = Some(now + delay);
        } else {
            self.retry_at = None;
            self.animation_at = None;
        }
    }

    pub(crate) fn retry_exhausted(&self) -> bool {
        self.failures >= self.policy.attempts
    }

    /// Playing transitions use presentation pacing; delayed transitions install
    /// one wakeup. No animation deadline survives hiding, failure, or completion.
    pub(crate) fn animate_at(&mut self, deadline: Option<Instant>) {
        self.animation_at = deadline;
    }

    pub(crate) fn deadline(&self) -> Option<Instant> {
        if !self.available || self.pending || self.retry_exhausted() {
            return None;
        }
        self.retry_at.into_iter().chain(self.animation_at).min()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeated_invalidations_coalesce_and_idle_has_no_timer() {
        let mut state = FrameSchedule::new(RetryPolicy::default());
        let now = Instant::now();
        assert!(state.request(now));
        state.invalidate();
        assert!(!state.request(now));
        assert!(state.begin(now));
        state.presented();
        assert!(!state.request(now));
        assert!(state.deadline().is_none());
    }
    #[test]
    fn occluded_or_zero_sized_windows_park_until_restored() {
        let mut state = FrameSchedule::new(RetryPolicy::default());
        let now = Instant::now();
        state.failed(now);
        state.set_available(false);
        assert!(state.deadline().is_none());
        state.invalidate();
        assert!(!state.request(now));
        assert!(!state.begin(now));
        state.set_available(true);
        assert!(state.request(now));
    }
    #[test]
    fn failures_back_off_and_eventually_park() {
        let policy = RetryPolicy {
            attempts: 3,
            ..RetryPolicy::default()
        };
        let mut state = FrameSchedule::new(policy);
        let now = Instant::now();
        state.failed(now);
        assert_eq!(state.deadline(), Some(now + policy.initial_delay));
        assert!(!state.request(now));
        assert!(!state.begin(now));
        state.failed(now + policy.initial_delay);
        assert_eq!(state.deadline(), Some(now + policy.initial_delay * 3));
        state.failed(now + policy.initial_delay * 3);
        assert!(state.deadline().is_none());
        assert!(!state.request(now + Duration::from_secs(10)));
        state.invalidate();
        assert!(state.request(now));
    }
}

#[cfg(test)]
mod animation_tests {
    use super::*;
    #[test]
    fn delayed_animation_wakes_once_and_completion_removes_the_timer() {
        let now = Instant::now();
        let mut s = FrameSchedule::new(RetryPolicy::default());
        s.presented();
        s.animate_at(Some(now + Duration::from_secs(1)));
        assert!(!s.request(now));
        assert_eq!(s.deadline(), Some(now + Duration::from_secs(1)));
        assert!(s.request(now + Duration::from_secs(1)));
        assert!(s.deadline().is_none());
        s.presented();
        assert!(!s.request(now + Duration::from_secs(2)));
        assert!(s.deadline().is_none());
    }
    #[test]
    fn hidden_and_failed_animation_frames_do_not_spin() {
        let now = Instant::now();
        let mut s = FrameSchedule::new(RetryPolicy::default());
        s.presented();
        s.animate_at(Some(now));
        s.set_available(false);
        assert!(!s.request(now));
        assert!(s.deadline().is_none());
        s.set_available(true);
        assert!(s.request(now));
        s.failed(now);
        assert!(!s.request(now));
        assert!(s.deadline().unwrap() > now);
    }
}

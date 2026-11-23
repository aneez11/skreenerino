//! Viewer-side gap detection: decides when to send a NACK.

use skreen_transport::quality::LossTracker;
use std::time::{Duration, Instant};

/// Debounces NACKs so a single lost fragment is not requested repeatedly.
#[derive(Debug)]
pub struct NackScheduler {
    tracker: LossTracker,
    window: u64,
    last_nack: Option<Instant>,
    min_interval: Duration,
    highest_seen: u64,
}

impl NackScheduler {
    pub fn new(window: u64, min_interval: Duration) -> Self {
        Self {
            tracker: LossTracker::default(),
            window,
            last_nack: None,
            min_interval,
            highest_seen: 0,
        }
    }

    pub fn observe(&mut self, sequence: u64) {
        self.highest_seen = self.highest_seen.max(sequence);
        self.tracker.record(sequence);
    }

    pub fn highest(&self) -> u64 {
        self.highest_seen
    }

    /// Sequences worth requesting now, if the debounce interval has elapsed.
    pub fn poll(&mut self, now: Instant) -> Vec<u64> {
        if let Some(last) = self.last_nack {
            if now.duration_since(last) < self.min_interval {
                return Vec::new();
            }
        }
        // Leave a small margin so a merely reordered fragment is not NACKed.
        let settled = self.highest_seen.saturating_sub(1);
        let missing: Vec<u64> = self
            .tracker
            .missing(self.window)
            .into_iter()
            .filter(|seq| *seq < settled)
            .collect();
        if missing.is_empty() {
            return Vec::new();
        }
        self.last_nack = Some(now);
        missing
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn does_not_nack_a_merely_reordered_fragment() {
        let mut scheduler = NackScheduler::new(8, Duration::from_millis(0));
        scheduler.observe(0);
        scheduler.observe(2);
        // 1 is only one behind the highest, so it may still arrive.
        assert!(scheduler.poll(Instant::now()).is_empty());
    }

    #[test]
    fn nacks_settled_gaps() {
        let mut scheduler = NackScheduler::new(8, Duration::from_millis(0));
        for seq in [0, 1, 2, 4, 5] {
            scheduler.observe(seq);
        }
        let missing = scheduler.poll(Instant::now());
        assert_eq!(missing, vec![3]);
    }

    #[test]
    fn debounces_repeated_requests() {
        let mut scheduler = NackScheduler::new(8, Duration::from_millis(50));
        for seq in [0, 1, 3, 4] {
            scheduler.observe(seq);
        }
        let now = Instant::now();
        assert_eq!(scheduler.poll(now), vec![2]);
        assert!(scheduler.poll(now + Duration::from_millis(10)).is_empty());
        assert_eq!(scheduler.poll(now + Duration::from_millis(60)), vec![2]);
    }

    #[test]
    fn quiet_when_nothing_is_missing() {
        let mut scheduler = NackScheduler::new(8, Duration::from_millis(0));
        for seq in 0..10 {
            scheduler.observe(seq);
        }
        assert!(scheduler.poll(Instant::now()).is_empty());
    }
}

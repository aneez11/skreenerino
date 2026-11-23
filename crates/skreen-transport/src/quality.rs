use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QualityTier {
    Low,
    Medium,
    High,
}

impl QualityTier {
    pub fn bitrate_bps(self) -> u32 {
        match self {
            Self::Low => 800_000,
            Self::Medium => 2_500_000,
            Self::High => 6_000_000,
        }
    }

    pub fn scale(self) -> f32 {
        match self {
            Self::Low => 0.5,
            Self::Medium => 0.75,
            Self::High => 1.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct QualityReport {
    pub rtt_ms: u32,
    pub loss_permille: u32,
    pub decode_ms: u32,
}

#[derive(Debug)]
pub struct AdaptiveBitrate {
    current: QualityTier,
    last_change: Instant,
    min_hold: Duration,
    max_loss_permille: u32,
    min_rtt_ms: u32,
}

impl AdaptiveBitrate {
    pub fn new(start: QualityTier, max_loss_permille: u32, min_rtt_ms: u32) -> Self {
        Self {
            current: start,
            last_change: Instant::now(),
            min_hold: Duration::from_secs(2),
            max_loss_permille,
            min_rtt_ms,
        }
    }

    pub fn current(&self) -> QualityTier {
        self.current
    }

    /// Returns Some(new tier) when a change should be applied.
    pub fn observe(&mut self, report: &QualityReport, now: Instant) -> Option<QualityTier> {
        if now.duration_since(self.last_change) < self.min_hold {
            return None;
        }
        let should_down = report.loss_permille > self.max_loss_permille
            || report.rtt_ms > self.min_rtt_ms * 4
            || report.decode_ms > 50;
        let should_up = report.loss_permille < self.max_loss_permille / 8
            && report.rtt_ms < self.min_rtt_ms * 2
            && report.decode_ms < 20;

        let next = if should_down {
            match self.current {
                QualityTier::High => Some(QualityTier::Medium),
                QualityTier::Medium => Some(QualityTier::Low),
                QualityTier::Low => None,
            }
        } else if should_up {
            match self.current {
                QualityTier::Low => Some(QualityTier::Medium),
                QualityTier::Medium => Some(QualityTier::High),
                QualityTier::High => None,
            }
        } else {
            None
        };

        if let Some(next) = next {
            self.current = next;
            self.last_change = now;
        }
        next
    }
}

/// Minimal NACK window: track which sequences arrived, report holes.
#[derive(Debug)]
pub struct LossTracker {
    highest: u64,
    seen: std::collections::BTreeSet<u64>,
    max_tracked: u64,
}

impl Default for LossTracker {
    fn default() -> Self {
        Self {
            highest: 0,
            seen: Default::default(),
            max_tracked: 512,
        }
    }
}

impl LossTracker {
    pub fn record(&mut self, sequence: u64) {
        self.highest = self.highest.max(sequence);
        self.seen.insert(sequence);
        while self.seen.len() as u64 > self.max_tracked {
            if let Some(first) = self.seen.iter().next().copied() {
                if first <= self.highest {
                    self.seen.remove(&first);
                } else {
                    break;
                }
            } else {
                break;
            }
        }
    }

    /// Sequences assumed missing once a later frame has arrived.
    pub fn missing(&self, window: u64) -> Vec<u64> {
        let start = self.highest.saturating_sub(window);
        (start..self.highest)
            .filter(|seq| !self.seen.contains(seq))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_down_under_loss_then_recovers() {
        let mut abr = AdaptiveBitrate::new(QualityTier::High, 50, 30);
        let start = Instant::now();
        let bad = QualityReport {
            rtt_ms: 200,
            loss_permille: 120,
            decode_ms: 80,
        };
        assert_eq!(
            abr.observe(&bad, start + Duration::from_secs(3)),
            Some(QualityTier::Medium)
        );
        assert_eq!(
            abr.observe(&bad, start + Duration::from_secs(6)),
            Some(QualityTier::Low)
        );
        let good = QualityReport {
            rtt_ms: 5,
            loss_permille: 0,
            decode_ms: 5,
        };
        assert_eq!(
            abr.observe(&good, start + Duration::from_secs(9)),
            Some(QualityTier::Medium)
        );
    }

    #[test]
    fn holds_tier_during_min_hold() {
        let mut abr = AdaptiveBitrate::new(QualityTier::High, 50, 30);
        let bad = QualityReport {
            rtt_ms: 400,
            loss_permille: 200,
            decode_ms: 90,
        };
        assert_eq!(abr.observe(&bad, Instant::now()), None);
    }

    #[test]
    fn reports_missing_sequences() {
        let mut lt = LossTracker::default();
        for seq in [1, 2, 4, 5, 7] {
            lt.record(seq);
        }
        assert_eq!(lt.missing(5), vec![3, 6]);
    }
}

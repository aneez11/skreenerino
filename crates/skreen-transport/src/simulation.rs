//! Network-condition simulation helpers and a soak-test harness for CI.

use std::time::Duration;

/// Simple deterministic loss/jitter model for tests, so impairment is
/// reproducible without root-level `tc netem` access.
#[derive(Debug, Clone, Copy)]
pub struct NetworkProfile {
    pub loss_permille: u32,
    pub jitter: Duration,
    pub latency: Duration,
}

impl NetworkProfile {
    pub const GOOD_WIFI: Self = Self {
        loss_permille: 5,
        jitter: Duration::from_millis(8),
        latency: Duration::from_millis(10),
    };
    pub const BAD_WIFI: Self = Self {
        loss_permille: 50,
        jitter: Duration::from_millis(40),
        latency: Duration::from_millis(40),
    };
    pub const WIRED: Self = Self {
        loss_permille: 0,
        jitter: Duration::from_millis(1),
        latency: Duration::from_millis(1),
    };

    /// Should sequence `seq` be dropped under this profile?
    pub fn drops(&self, seq: u64) -> bool {
        if self.loss_permille == 0 {
            return false;
        }
        let mut x = seq
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        x ^= x >> 33;
        x = x.wrapping_mul(0xff51afd7ed558ccd);
        x ^= x >> 33;
        ((x % 1000) as u32) < self.loss_permille
    }

    /// Extra delay for a given sequence.
    pub fn delay(&self, seq: u64) -> Duration {
        if self.jitter.is_zero() {
            return self.latency;
        }
        let jitter_ns = self.jitter.as_nanos().max(1) as u64;
        self.latency + Duration::from_nanos(seq.wrapping_mul(2654435761) % jitter_ns)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use skreen_proto::fragment::{Fragmenter, Reassembler};
    use skreen_proto::media::{decode_media, encode_media, MediaDatagram};

    #[test]
    fn no_loss_drops_nothing() {
        let profile = NetworkProfile::WIRED;
        assert!((0..1000).all(|seq| !profile.drops(seq)));
    }

    #[test]
    fn loss_is_roughly_requested_rate() {
        let profile = NetworkProfile::BAD_WIFI;
        let dropped = (0..10_000).filter(|seq| profile.drops(*seq)).count();
        let rate = dropped as f64 / 10_000.0;
        assert!((rate - 0.05).abs() < 0.02, "loss rate was {rate}");
    }

    #[test]
    fn reassembly_never_yields_partial_frames_under_loss() {
        let profile = NetworkProfile::GOOD_WIFI;
        let fragmenter = Fragmenter::new(1100);
        let payload = vec![7u8; 9000];
        let frags = fragmenter.fragment("main", 1, 100, true, &payload);
        let mut reassembler = Reassembler::default();
        let mut completed = None;
        for frag in frags {
            if profile.drops(frag.fragment_index as u64) {
                continue;
            }
            let wire = encode_media(&MediaDatagram {
                stream: frag.stream.clone(),
                sequence: frag.sequence,
                timestamp_us: frag.timestamp_us,
                keyframe: frag.keyframe,
                fragment_index: frag.fragment_index,
                fragment_count: frag.fragment_count,
                frame_bytes: payload.len() as u32,
                payload: frag.payload.clone(),
            })
            .unwrap();
            let decoded = decode_media(&wire).unwrap();
            let restored = skreen_proto::fragment::FrameFragment {
                stream: decoded.stream,
                sequence: decoded.sequence,
                timestamp_us: decoded.timestamp_us,
                keyframe: decoded.keyframe,
                fragment_index: decoded.fragment_index,
                fragment_count: decoded.fragment_count,
                payload: decoded.payload,
            };
            if let Some(frame) = reassembler.push(restored) {
                completed = Some(frame);
            }
        }
        if let Some(frame) = completed {
            assert_eq!(frame, payload);
        }
    }

    #[test]
    fn jitter_stays_within_bounds() {
        let profile = NetworkProfile::BAD_WIFI;
        for seq in 0..200 {
            let delay = profile.delay(seq);
            assert!(delay >= profile.latency);
            assert!(delay < profile.latency + profile.jitter + Duration::from_nanos(1));
        }
    }
}

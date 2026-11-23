//! Fragment retransmission buffer for NACK-driven recovery.

use skreen_proto::media::MediaDatagram;
use std::collections::{BTreeMap, VecDeque};

/// Retains recently sent datagrams so a viewer's NACK can be answered without
/// re-encoding anything.
#[derive(Debug)]
pub struct RetransmitBuffer {
    capacity: usize,
    entries: BTreeMap<u64, VecDeque<MediaDatagram>>,
    order: VecDeque<u64>,
    hits: u64,
    misses: u64,
}

impl RetransmitBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            entries: BTreeMap::new(),
            order: VecDeque::new(),
            hits: 0,
            misses: 0,
        }
    }

    pub fn record(&mut self, datagram: MediaDatagram) {
        let sequence = datagram.sequence;
        self.entries
            .entry(sequence)
            .or_default()
            .push_back(datagram);
        if !self.order.contains(&sequence) {
            self.order.push_back(sequence);
        }
        while self.order.len() > self.capacity {
            if let Some(evicted) = self.order.pop_front() {
                self.entries.remove(&evicted);
            }
        }
    }

    /// Datagrams that can be resent for the requested sequences.
    pub fn recover(&mut self, sequences: &[u64]) -> Vec<MediaDatagram> {
        let mut out = Vec::new();
        for sequence in sequences {
            match self.entries.get(sequence) {
                Some(fragments) => {
                    self.hits += 1;
                    out.extend(fragments.iter().cloned());
                }
                None => self.misses += 1,
            }
        }
        out
    }

    pub fn stats(&self) -> (u64, u64) {
        (self.hits, self.misses)
    }

    pub fn len(&self) -> usize {
        self.order.len()
    }
    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn datagram(sequence: u64, fragment_index: u16) -> MediaDatagram {
        MediaDatagram {
            stream: "main".into(),
            sequence,
            timestamp_us: sequence * 1000,
            keyframe: false,
            fragment_index,
            fragment_count: 2,
            frame_bytes: 32,
            payload: vec![sequence as u8; 8],
        }
    }

    #[test]
    fn recovers_all_fragments_for_a_sequence() {
        let mut buffer = RetransmitBuffer::new(16);
        buffer.record(datagram(1, 0));
        buffer.record(datagram(1, 1));
        buffer.record(datagram(2, 0));
        let recovered = buffer.recover(&[1]);
        assert_eq!(recovered.len(), 2);
        assert_eq!(buffer.stats(), (1, 0));
    }

    #[test]
    fn reports_misses_for_evicted_or_unknown() {
        let mut buffer = RetransmitBuffer::new(2);
        for seq in 0..5 {
            buffer.record(datagram(seq, 0));
        }
        assert_eq!(buffer.len(), 2);
        assert!(buffer.recover(&[0]).is_empty());
        assert_eq!(buffer.stats(), (0, 1));
        assert_eq!(buffer.recover(&[4]).len(), 1);
    }

    #[test]
    fn adding_fragments_does_not_grow_the_window() {
        let mut buffer = RetransmitBuffer::new(4);
        for index in 0..10u16 {
            buffer.record(datagram(7, index));
        }
        assert_eq!(buffer.len(), 1);
        assert_eq!(buffer.recover(&[7]).len(), 10);
    }
}

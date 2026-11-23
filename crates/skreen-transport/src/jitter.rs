use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct JitterEntry<T> {
    pub timestamp_us: u64,
    pub value: T,
}

#[derive(Debug)]
pub struct JitterBuffer<T> {
    max_latency_us: u64,
    next_expected: u64,
    started: bool,
    entries: BTreeMap<u64, JitterEntry<T>>,
}

impl<T> JitterBuffer<T> {
    pub fn new(max_latency_us: u64) -> Self {
        Self { max_latency_us, next_expected: 0, started: false, entries: BTreeMap::new() }
    }

    pub fn push(&mut self, sequence: u64, timestamp_us: u64, value: T) {
        if self.started && sequence < self.next_expected {
            return;
        }
        self.entries.insert(sequence, JitterEntry { timestamp_us, value });
    }

    pub fn pop_ready(&mut self, now_us: u64) -> Option<JitterEntry<T>> {
        let oldest_seq = *self.entries.keys().next()?;
        if !self.started {
            self.started = true;
            self.next_expected = oldest_seq;
        }
        let expired = self
            .entries
            .get(&oldest_seq)
            .map(|e| now_us.saturating_sub(e.timestamp_us) >= self.max_latency_us)
            .unwrap_or(false);

        if oldest_seq == self.next_expected || expired {
            let entry = self.entries.remove(&oldest_seq)?;
            self.next_expected = oldest_seq + 1;
            Some(entry)
        } else {
            None
        }
    }

    pub fn len(&self) -> usize { self.entries.len() }
    pub fn is_empty(&self) -> bool { self.entries.is_empty() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waits_for_missing_sequence_within_deadline() {
        let mut buf = JitterBuffer::new(50);
        buf.push(0, 100, "a");
        buf.push(2, 100, "c");
        assert_eq!(buf.pop_ready(120).unwrap().value, "a");
        assert!(buf.pop_ready(120).is_none());
        assert_eq!(buf.pop_ready(160).unwrap().value, "c");
    }

    #[test]
    fn drops_late_and_duplicate_sequences() {
        let mut buf = JitterBuffer::new(10);
        buf.push(1, 0, "a");
        assert_eq!(buf.pop_ready(0).unwrap().value, "a");
        buf.push(1, 0, "dup");
        assert!(buf.is_empty());
    }

    #[test]
    fn expires_stalled_head() {
        let mut buf = JitterBuffer::new(10);
        buf.push(5, 0, "late");
        assert_eq!(buf.pop_ready(20).unwrap().value, "late");
    }
}

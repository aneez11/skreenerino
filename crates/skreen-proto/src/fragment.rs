use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FrameFragment {
    pub stream: String,
    pub sequence: u64,
    pub timestamp_us: u64,
    pub keyframe: bool,
    pub fragment_index: u16,
    pub fragment_count: u16,
    pub payload: Vec<u8>,
}

#[derive(Debug, Default)]
pub struct Fragmenter {
    max_payload: usize,
}

impl Fragmenter {
    pub fn new(max_payload: usize) -> Self {
        Self {
            max_payload: max_payload.max(1),
        }
    }

    pub fn fragment(
        &self,
        stream: &str,
        sequence: u64,
        timestamp_us: u64,
        keyframe: bool,
        data: &[u8],
    ) -> Vec<FrameFragment> {
        let count = data.len().max(1).div_ceil(self.max_payload);
        let mut out = Vec::with_capacity(count);
        for (i, chunk) in data.chunks(self.max_payload).enumerate() {
            out.push(FrameFragment {
                stream: stream.to_string(),
                sequence,
                timestamp_us,
                keyframe,
                fragment_index: i as u16,
                fragment_count: count as u16,
                payload: chunk.to_vec(),
            });
        }
        if data.is_empty() {
            out.push(FrameFragment {
                stream: stream.to_string(),
                sequence,
                timestamp_us,
                keyframe,
                fragment_index: 0,
                fragment_count: 1,
                payload: vec![],
            });
        }
        out
    }
}

#[derive(Debug, Default)]
pub struct Reassembler {
    pending: std::collections::BTreeMap<(String, u64), Vec<Option<Vec<u8>>>>,
}

impl Reassembler {
    pub fn push(&mut self, frag: FrameFragment) -> Option<Vec<u8>> {
        let key = (frag.stream.clone(), frag.sequence);
        let entry = self
            .pending
            .entry(key.clone())
            .or_insert_with(|| vec![None; frag.fragment_count as usize]);
        if entry.len() != frag.fragment_count as usize {
            return None;
        }
        let idx = frag.fragment_index as usize;
        if idx >= entry.len() {
            return None;
        }
        entry[idx] = Some(frag.payload);
        if entry.iter().all(|p| p.is_some()) {
            let parts = self.pending.remove(&key).unwrap();
            let mut out = Vec::new();
            for p in parts {
                out.extend_from_slice(&p.unwrap());
            }
            return Some(out);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn round_trip() {
        let f = Fragmenter::new(3);
        let frags = f.fragment("s", 1, 10, true, b"abcdefg");
        let mut r = Reassembler::default();
        let mut done = None;
        for frag in frags {
            done = r.push(frag).or(done);
        }
        assert_eq!(done.unwrap(), b"abcdefg");
    }
}

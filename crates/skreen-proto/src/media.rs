use serde::{Deserialize, Serialize};

use crate::ProtoError;

/// One fragment of an encoded frame, as sent over a QUIC datagram.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MediaDatagram {
    pub stream: String,
    pub sequence: u64,
    pub timestamp_us: u64,
    pub keyframe: bool,
    pub fragment_index: u16,
    pub fragment_count: u16,
    /// Total encoded frame size, used to size reassembly buffers.
    pub frame_bytes: u32,
    pub payload: Vec<u8>,
}

pub fn encode_media(msg: &MediaDatagram) -> Result<Vec<u8>, ProtoError> {
    Ok(postcard::to_allocvec(msg)?)
}

pub fn decode_media(bytes: &[u8]) -> Result<MediaDatagram, ProtoError> {
    Ok(postcard::from_bytes(bytes)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_round_trip() {
        let msg = MediaDatagram {
            stream: "main".into(),
            sequence: 7,
            timestamp_us: 1234,
            keyframe: true,
            fragment_index: 1,
            fragment_count: 3,
            frame_bytes: 9000,
            payload: vec![1, 2, 3],
        };
        let encoded = encode_media(&msg).unwrap();
        assert_eq!(decode_media(&encoded).unwrap(), msg);
    }
}

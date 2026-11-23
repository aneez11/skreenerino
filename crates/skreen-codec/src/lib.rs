use anyhow::Result;
use bytes::Bytes;

#[derive(Debug, Clone)]
pub struct EncodedPacket {
    pub timestamp_us: u64,
    pub keyframe: bool,
    pub bytes: Bytes,
}

pub trait Encoder {
    fn encode(&mut self, rgba: &[u8], width: u32, height: u32, timestamp_us: u64) -> Result<Vec<EncodedPacket>>;
}

pub trait Decoder {
    fn decode(&mut self, packet: &[u8]) -> Result<Option<Bytes>>;
}

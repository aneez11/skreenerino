pub mod jitter;

use anyhow::Result;
use bytes::Bytes;

pub trait Transport {
    fn send_control(&self, bytes: Bytes) -> Result<()>;
    fn send_datagram(&self, bytes: Bytes) -> Result<()>;
}

pub fn mtu_safe_payload(mtu: usize, header_len: usize) -> usize {
    mtu.saturating_sub(header_len).max(1)
}

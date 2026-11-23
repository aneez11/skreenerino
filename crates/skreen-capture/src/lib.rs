use anyhow::Result;
use bytes::Bytes;

#[derive(Debug, Clone)]
pub struct CapturedFrame {
    pub width: u32,
    pub height: u32,
    pub timestamp_us: u64,
    pub bytes: Bytes,
}

pub trait ScreenCapturer {
    fn start(&mut self) -> Result<()>;
    fn next_frame(&mut self) -> Result<Option<CapturedFrame>>;
    fn stop(&mut self) -> Result<()>;
}

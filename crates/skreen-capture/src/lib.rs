use anyhow::Result;
use bytes::Bytes;

#[derive(Debug, Clone)]
pub struct CapturedFrame {
    pub width: u32,
    pub height: u32,
    pub timestamp_us: u64,
    pub rgba: Bytes,
}

pub trait ScreenCapturer {
    fn start(&mut self) -> Result<()>;
    fn next_frame(&mut self) -> Result<Option<CapturedFrame>>;
    fn stop(&mut self) -> Result<()>;
}

#[cfg(feature = "xcap")]
pub mod xcap_backend {
    use super::*;
    use std::time::Instant;

    pub struct XcapCapturer {
        started: Instant,
        inner: xcap::Monitor,
    }

    impl XcapCapturer {
        pub fn primary() -> Result<Self> {
            let monitor = xcap::Monitor::all()?
                .into_iter()
                .next()
                .ok_or_else(|| anyhow::anyhow!("no monitors found"))?;
            Ok(Self { started: Instant::now(), inner: monitor })
        }
    }

    impl ScreenCapturer for XcapCapturer {
        fn start(&mut self) -> Result<()> {
            self.started = Instant::now();
            Ok(())
        }

        fn next_frame(&mut self) -> Result<Option<CapturedFrame>> {
            let image = self.inner.capture_image()?;
            let width = image.width();
            let height = image.height();
            let rgba = Bytes::from(image.into_raw());
            Ok(Some(CapturedFrame {
                width,
                height,
                timestamp_us: self.started.elapsed().as_micros() as u64,
                rgba,
            }))
        }

        fn stop(&mut self) -> Result<()> { Ok(()) }
    }
}

#[derive(Debug, Default)]
pub struct NullCapturer;

impl ScreenCapturer for NullCapturer {
    fn start(&mut self) -> Result<()> { Ok(()) }
    fn next_frame(&mut self) -> Result<Option<CapturedFrame>> { Ok(None) }
    fn stop(&mut self) -> Result<()> { Ok(()) }
}

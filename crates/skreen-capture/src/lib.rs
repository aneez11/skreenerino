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

/// macOS ScreenCaptureKit backend (requires Screen Recording permission).
#[cfg(target_os = "macos")]
pub mod macos_backend {
    use super::*;
    use std::time::Instant;

    /// Placeholder until the `screencapturekit` bindings are wired up.
    pub struct ScreenCaptureKitCapturer {
        started: Instant,
    }

    impl ScreenCaptureKitCapturer {
        pub fn primary() -> Result<Self> {
            anyhow::bail!(
                "macOS capture backend not yet implemented: add the `screencapturekit` dependency \
                 and request Screen Recording permission"
            )
        }
    }

    impl ScreenCapturer for ScreenCaptureKitCapturer {
        fn start(&mut self) -> Result<()> { self.started = Instant::now(); Ok(()) }
        fn next_frame(&mut self) -> Result<Option<CapturedFrame>> {
            anyhow::bail!("macOS capture backend not yet implemented")
        }
        fn stop(&mut self) -> Result<()> { Ok(()) }
    }
}

/// Linux backend: PipeWire portal on Wayland, X11 capture otherwise.
#[cfg(target_os = "linux")]
pub mod linux_backend {
    use super::*;

    pub struct LinuxCapturer {
        pub backend: &'static str,
    }

    impl LinuxCapturer {
        pub fn detect() -> Result<Self> {
            let session = std::env::var("XDG_SESSION_TYPE").unwrap_or_default();
            let backend = if session.eq_ignore_ascii_case("wayland") { "pipewire" } else { "x11" };
            Ok(Self { backend })
        }
    }

    impl ScreenCapturer for LinuxCapturer {
        fn start(&mut self) -> Result<()> {
            anyhow::bail!(
                "linux capture backend ({}) not yet implemented: wire up xdg-desktop-portal or XShm",
                self.backend
            )
        }
        fn next_frame(&mut self) -> Result<Option<CapturedFrame>> { Ok(None) }
        fn stop(&mut self) -> Result<()> { Ok(()) }
    }
}

/// Platform-appropriate capturer, or a clear error on unsupported targets.
pub fn default_capturer() -> Result<Box<dyn ScreenCapturer>> {
    #[cfg(feature = "xcap")]
    {
        Ok(Box::new(xcap_backend::XcapCapturer::primary()?))
    }
    #[cfg(not(feature = "xcap"))]
    {
        anyhow::bail!("capture backend not enabled: build with the `xcap` feature")
    }
}

#[derive(Debug, Default)]
pub struct NullCapturer;

impl ScreenCapturer for NullCapturer {
    fn start(&mut self) -> Result<()> { Ok(()) }
    fn next_frame(&mut self) -> Result<Option<CapturedFrame>> { Ok(None) }
    fn stop(&mut self) -> Result<()> { Ok(()) }
}


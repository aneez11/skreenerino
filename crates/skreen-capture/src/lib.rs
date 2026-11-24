use anyhow::Result;
use bytes::Bytes;

pub mod scale;

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

/// A display that can be shared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonitorInfo {
    /// Stable platform id, used to pick the monitor again later.
    pub id: u32,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub x: i32,
    pub y: i32,
    /// First monitor in the OS enumeration order.
    pub is_primary: bool,
}

impl MonitorInfo {
    pub fn label(&self) -> String {
        if self.name.trim().is_empty() {
            format!("Monitor {}", self.id)
        } else {
            self.name.clone()
        }
    }

    pub fn dimensions(&self) -> String {
        format!("{} x {}", self.width, self.height)
    }
}

/// Enumerate displays available for sharing.
#[cfg(feature = "xcap")]
pub fn list_monitors() -> Result<Vec<MonitorInfo>> {
    let monitors = xcap::Monitor::all()?;
    let mut out = Vec::with_capacity(monitors.len());
    for (index, monitor) in monitors.iter().enumerate() {
        out.push(MonitorInfo {
            id: monitor.id().unwrap_or(index as u32),
            name: monitor.name().unwrap_or_default(),
            width: monitor.width().unwrap_or(0),
            height: monitor.height().unwrap_or(0),
            x: monitor.x().unwrap_or(0),
            y: monitor.y().unwrap_or(0),
            is_primary: index == 0,
        });
    }
    Ok(out)
}

#[cfg(not(feature = "xcap"))]
pub fn list_monitors() -> Result<Vec<MonitorInfo>> {
    anyhow::bail!("monitor enumeration requires the `xcap` feature")
}

/// Capture a monitor and downscale it to a thumbnail for the share picker.
///
/// Returns `(rgba, width, height)`. Choosing `max_width` small keeps this cheap
/// (it is a one-off call when the picker opens, not per frame).
#[cfg(feature = "xcap")]
pub fn monitor_thumbnail(index: usize, max_width: u32) -> Result<(Bytes, u32, u32)> {
    let monitors = xcap::Monitor::all()?;
    let monitor = monitors
        .into_iter()
        .nth(index)
        .ok_or_else(|| anyhow::anyhow!("no monitor at index {index}"))?;
    let image = monitor.capture_image()?;
    let (width, height) = (image.width(), image.height());
    let raw = image.into_raw();
    if width <= max_width {
        return Ok((Bytes::from(raw), width, height));
    }
    let factor = max_width as f32 / width as f32;
    match scale::scale_rgba_nearest(&raw, width, height, factor) {
        Some((bytes, w, h)) => Ok((bytes, w, h)),
        None => Ok((Bytes::from(raw), width, height)),
    }
}

#[cfg(not(feature = "xcap"))]
pub fn monitor_thumbnail(_index: usize, _max_width: u32) -> Result<(Bytes, u32, u32)> {
    anyhow::bail!("monitor capture requires the `xcap` feature")
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
        /// Capture the first monitor.
        pub fn primary() -> Result<Self> {
            Self::by_index(0)
        }

        /// Capture the monitor at `index` in OS enumeration order.
        pub fn by_index(index: usize) -> Result<Self> {
            let monitor = xcap::Monitor::all()?
                .into_iter()
                .nth(index)
                .ok_or_else(|| anyhow::anyhow!("no monitor at index {index}"))?;
            Ok(Self {
                started: Instant::now(),
                inner: monitor,
            })
        }

        /// Capture by platform monitor id, falling back to the primary display.
        pub fn by_id(id: u32) -> Result<Self> {
            let monitors = xcap::Monitor::all()?;
            let found = monitors
                .iter()
                .find(|m| m.id().map(|mid| mid == id).unwrap_or(false))
                .or_else(|| monitors.first());
            match found {
                Some(monitor) => Ok(Self {
                    started: Instant::now(),
                    inner: monitor.clone(),
                }),
                None => anyhow::bail!("no monitors found"),
            }
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

        fn stop(&mut self) -> Result<()> {
            Ok(())
        }
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
                "macOS capture uses ScreenCaptureKit, which is not implemented yet. The `xcap` \
                 backend does support macOS if the Screen Recording permission is granted; \
                 prefer `default_capturer()`"
            )
        }
    }

    impl ScreenCapturer for ScreenCaptureKitCapturer {
        fn start(&mut self) -> Result<()> {
            self.started = Instant::now();
            Ok(())
        }
        fn next_frame(&mut self) -> Result<Option<CapturedFrame>> {
            anyhow::bail!("macOS capture backend not yet implemented")
        }
        fn stop(&mut self) -> Result<()> {
            Ok(())
        }
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
            let backend = if session.eq_ignore_ascii_case("wayland") {
                "pipewire"
            } else {
                "x11"
            };
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
        fn next_frame(&mut self) -> Result<Option<CapturedFrame>> {
            Ok(None)
        }
        fn stop(&mut self) -> Result<()> {
            Ok(())
        }
    }
}

/// Platform-appropriate capturer, or a clear error on unsupported targets.
pub fn default_capturer() -> Result<Box<dyn ScreenCapturer>> {
    #[cfg(all(target_os = "linux", feature = "xcap"))]
    {
        return linux_backend::capturer();
    }
    #[cfg(all(not(target_os = "linux"), feature = "xcap"))]
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
    fn start(&mut self) -> Result<()> {
        Ok(())
    }
    fn next_frame(&mut self) -> Result<Option<CapturedFrame>> {
        Ok(None)
    }
    fn stop(&mut self) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monitor_label_falls_back_to_id() {
        let m = MonitorInfo {
            id: 3,
            name: String::new(),
            width: 1920,
            height: 1080,
            x: 0,
            y: 0,
            is_primary: false,
        };
        assert_eq!(m.label(), "Monitor 3");
        assert_eq!(m.dimensions(), "1920 x 1080");
    }

    #[test]
    fn monitor_label_uses_name_when_present() {
        let m = MonitorInfo {
            id: 1,
            name: "DELL U2720Q".into(),
            width: 3840,
            height: 2160,
            x: 0,
            y: 0,
            is_primary: true,
        };
        assert_eq!(m.label(), "DELL U2720Q");
        assert!(m.is_primary);
    }
}

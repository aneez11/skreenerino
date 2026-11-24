//! Shared orchestration for the CLI and the GUI: everything that is not pixels.

pub mod nack;

#[cfg(feature = "audio")]
pub mod audio;
pub mod discovery;
pub mod model;
pub mod viewer;

#[cfg(all(feature = "xcap", feature = "h264", feature = "quic"))]
pub mod presenter;

use skreen_render::RgbaFrame;

/// What happened, reported to whichever front-end is running.
#[derive(Debug, Clone)]
pub enum AppEvent {
    /// A decoded video frame for a named stream.
    Frame {
        stream: String,
        frame: std::sync::Arc<RgbaFrame>,
    },
    /// The stream ended (peer closed or decode stopped).
    StreamEnded { stream: String },
    /// Transport/statistics snapshot for the stats strip.
    Stats(ViewerStats),
    /// A pairing or consent decision is needed before continuing.
    ConsentRequired {
        requester: String,
        kind: skreen_security::ConsentKind,
    },
    /// Something failed but the session may still be usable.
    Warning(String),
}

/// What a front-end asks the session to do.
#[derive(Debug, Clone)]
pub enum Command {
    StartShare(ShareOptions),
    StopShare,
    Join { target: String },
    Leave,
    SetMuted(bool),
    RequestKeyframe,
}

#[derive(Debug, Clone)]
pub struct ShareOptions {
    pub room: Option<String>,
    pub name: Option<String>,
    pub fps: u32,
    pub bitrate: u32,
    pub scale: f32,
    pub audio: bool,
    pub bind: Option<std::net::SocketAddr>,
    /// Platform monitor id to share; `None` means the primary display.
    pub monitor: Option<u32>,
}

impl Default for ShareOptions {
    fn default() -> Self {
        Self {
            room: None,
            name: None,
            fps: 30,
            bitrate: 4_000_000,
            scale: 1.0,
            audio: false,
            monitor: None,
            bind: None,
        }
    }
}

/// Counters a front-end can render without touching transport internals.
#[derive(Debug, Clone, Copy, Default)]
pub struct ViewerStats {
    pub frames: u32,
    pub datagrams: u64,
    pub bytes: usize,
    pub keyframes: u32,
    pub fps: f32,
    pub decode_ms: u32,
    pub rtt_ms: u32,
}

impl ViewerStats {
    pub fn merge(&mut self, other: &ViewerStats) {
        self.frames = other.frames;
        self.datagrams = other.datagrams;
        self.bytes = other.bytes;
        self.keyframes = other.keyframes;
        self.fps = other.fps;
        self.decode_ms = other.decode_ms;
        self.rtt_ms = other.rtt_ms;
    }
}

/// Where decoded frames go. Implemented by the CLI viewer and the GUI.
pub trait FrameSink: Send + Sync + 'static {
    fn push(&self, stream: &str, frame: RgbaFrame);
}

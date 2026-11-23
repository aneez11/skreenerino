use serde::{Deserialize, Serialize};

pub mod fragment;
pub mod latency;
pub mod media;

pub const PROTOCOL_VERSION: u16 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceId(pub String);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RoomId(pub String);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StreamId(pub String);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Role {
    Presenter,
    Viewer,
    Both,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Capabilities {
    pub codecs: Vec<String>,
    pub max_width: u32,
    pub max_height: u32,
    pub max_fps: u32,
    pub platform: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ControlMessage {
    Hello { version: u16, device: DeviceId, caps: Capabilities },
    RoomJoin { room: RoomId, passcode: Option<String> },
    RoomLeave { room: RoomId },
    PresentRequest { room: RoomId, stream: StreamId },
    ViewRequest { room: RoomId, stream: StreamId },
    Offer { stream: StreamId, codec: String, width: u32, height: u32, fps: u32 },
    Accept { stream: StreamId },
    Reject { stream: StreamId, reason: String },
    StreamStart { stream: StreamId },
    StreamStop { stream: StreamId },
    KeyframeRequest { stream: StreamId },
    /// Request retransmission of specific fragments (NACK).
    Nack { stream: StreamId, sequences: Vec<u64> },
    QualityReport { stream: StreamId, rtt_ms: u32, loss_permille: u32, decode_ms: u32 },
    RoleChange { device: DeviceId, role: Role },
    Heartbeat { ts_ms: u64 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameHeader {
    pub stream: StreamId,
    pub sequence: u64,
    pub timestamp_us: u64,
    pub keyframe: bool,
    pub fragment_index: u16,
    pub fragment_count: u16,
}

#[derive(Debug, thiserror::Error)]
pub enum ProtoError {
    #[error("postcard encode failed: {0}")]
    Encode(#[from] postcard::Error),
    #[error("json encode failed: {0}")]
    Json(#[from] serde_json::Error),
}

pub fn encode_control(msg: &ControlMessage) -> Result<Vec<u8>, ProtoError> {
    Ok(postcard::to_allocvec(msg)?)
}

pub fn decode_control(bytes: &[u8]) -> Result<ControlMessage, ProtoError> {
    Ok(postcard::from_bytes(bytes)?)
}





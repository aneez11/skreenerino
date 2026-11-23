//! Length-prefixed control-message framing over a QUIC bidirectional stream.

use anyhow::Result;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Maximum accepted control frame size, to bound memory from a hostile peer.
pub const MAX_FRAME_BYTES: usize = 64 * 1024;

pub async fn write_frame<W: AsyncWriteExt + Unpin>(writer: &mut W, payload: &[u8]) -> Result<()> {
    if payload.len() > MAX_FRAME_BYTES {
        anyhow::bail!("control frame too large: {} bytes", payload.len());
    }
    writer
        .write_all(&(payload.len() as u32).to_be_bytes())
        .await?;
    writer.write_all(payload).await?;
    writer.flush().await?;
    Ok(())
}

/// Reads one frame. Returns `Ok(None)` on a clean end of stream.
pub async fn read_frame<R: AsyncReadExt + Unpin>(reader: &mut R) -> Result<Option<Vec<u8>>> {
    let mut len_buf = [0u8; 4];
    match reader.read_exact(&mut len_buf).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e.into()),
    }
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > MAX_FRAME_BYTES {
        anyhow::bail!("control frame exceeds limit: {len} bytes");
    }
    let mut payload = vec![0u8; len];
    reader.read_exact(&mut payload).await?;
    Ok(Some(payload))
}

#[cfg(test)]
mod tests {
    use super::*;
    use skreen_proto::{ControlMessage, DeviceId, StreamId};

    #[tokio::test]
    async fn round_trips_multiple_frames() {
        let (mut a, mut b) = tokio::io::duplex(4096);
        let first = skreen_proto::encode_control(&ControlMessage::Heartbeat { ts_ms: 1 }).unwrap();
        let second = skreen_proto::encode_control(&ControlMessage::KeyframeRequest {
            stream: StreamId("main".into()),
        })
        .unwrap();
        let first_sent = first.clone();
        let second_sent = second.clone();
        let handle = tokio::spawn(async move {
            write_frame(&mut a, &first_sent).await.unwrap();
            write_frame(&mut a, &second_sent).await.unwrap();
        });
        let read_first = read_frame(&mut b).await.unwrap().unwrap();
        let read_second = read_frame(&mut b).await.unwrap().unwrap();
        handle.await.unwrap();
        assert_eq!(read_first, first);
        assert_eq!(read_second, second);
        assert!(read_frame(&mut b).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn rejects_oversized_frame() {
        let (mut a, _b) = tokio::io::duplex(64);
        let big = vec![0u8; MAX_FRAME_BYTES + 1];
        assert!(write_frame(&mut a, &big).await.is_err());
    }

    #[tokio::test]
    async fn rejects_oversized_declared_length() {
        let (mut a, mut b) = tokio::io::duplex(64);
        let declared = ((MAX_FRAME_BYTES + 1) as u32).to_be_bytes();
        tokio::spawn(async move {
            a.write_all(&declared).await.unwrap();
        });
        assert!(read_frame(&mut b).await.is_err());
    }

    #[test]
    fn decodes_legacy_hello() {
        let encoded = skreen_proto::encode_control(&ControlMessage::Hello {
            version: skreen_proto::PROTOCOL_VERSION,
            device: DeviceId("x".into()),
            caps: skreen_proto::Capabilities {
                codecs: vec!["h264".into()],
                max_width: 1920,
                max_height: 1080,
                max_fps: 30,
                platform: "linux".into(),
            },
        })
        .unwrap();
        assert!(matches!(
            skreen_proto::decode_control(&encoded).unwrap(),
            ControlMessage::Hello { .. }
        ));
    }
}

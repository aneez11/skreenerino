//! One-to-many presenter: capture and encode once on a dedicated thread, fan out
//! the encoded packets to every connected viewer.

use anyhow::Result;
use bytes::Bytes;
use skreen_capture::{xcap_backend::XcapCapturer, ScreenCapturer};
use skreen_codec::{h264::OpenH264Encoder, Encoder, EncodedPacket};
use skreen_proto::fragment::Fragmenter;
use skreen_proto::media::MediaDatagram;
use skreen_proto::{Capabilities, ControlMessage, DeviceId, PROTOCOL_VERSION};
use skreen_transport::quic;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;
use tokio::sync::broadcast;

#[derive(Debug, Clone)]
pub struct RoomConfig {
    pub bitrate: u32,
    pub fps: u32,
    pub name: String,
}

#[derive(Default)]
pub struct RoomStats {
    pub viewers: AtomicUsize,
    pub frames: AtomicU64,
    pub datagrams: AtomicU64,
}

pub struct RoomShared {
    pub stats: Arc<RoomStats>,
    pub keyframe_seq: Arc<AtomicU64>,
    pub stop: Arc<AtomicBool>,
    /// How many packets were dropped because no viewer was connected.
    pub idle_frames: Arc<AtomicU64>,
}

impl RoomShared {
    pub fn new() -> Self {
        Self {
            stats: Arc::new(RoomStats::default()),
            keyframe_seq: Arc::new(AtomicU64::new(0)),
            stop: Arc::new(AtomicBool::new(false)),
            idle_frames: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn request_keyframe(&self) {
        self.keyframe_seq.fetch_add(1, Ordering::Relaxed);
    }
}

impl Default for RoomShared {
    fn default() -> Self { Self::new() }
}

/// Accept viewers until `shutdown` resolves or the encoder thread stops.
pub async fn run_room<S>(server: quic::QuicServer, cfg: RoomConfig, shared: Arc<RoomShared>, shutdown: S) -> Result<()>
where
    S: std::future::Future<Output = ()>,
{
    let (packet_tx, _) = broadcast::channel::<Arc<EncodedPacket>>(32);

    let encoder = {
        let tx = packet_tx.clone();
        let shared = shared.clone();
        std::thread::Builder::new()
            .name("skreenerino-encode".into())
            .spawn(move || encode_thread(tx, cfg, shared))
    }?;

    let accept = {
        let shared = shared.clone();
        let server = server.clone();
        async move {
            loop {
                match server.accept().await {
                    Ok(conn) => {
                        tracing::info!(peer = %conn.remote_address(), "viewer connected");
                        shared.stats.viewers.fetch_add(1, Ordering::Relaxed);
                        shared.request_keyframe();
                        let rx = packet_tx.subscribe();
                        let shared = shared.clone();
                        tokio::spawn(async move {
                            if let Err(e) = viewer_task(conn.clone(), rx, shared.clone()).await {
                                tracing::info!("viewer {} left: {e}", conn.remote_address());
                            }
                            shared.stats.viewers.fetch_sub(1, Ordering::Relaxed);
                            conn.close(0u32.into(), b"bye");
                        });
                    }
                    Err(e) => {
                        tracing::warn!("accept failed: {e}");
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                }
            }
        }
    };

    tokio::select! {
        _ = accept => {}
        _ = shutdown => {
            tracing::info!("shutdown requested");
            shared.stop.store(true, Ordering::Relaxed);
        }
    }

    shared.stop.store(true, Ordering::Relaxed);
    let _ = tokio::task::spawn_blocking(move || encoder.join()).await;
    server.close();
    Ok(())
}

async fn viewer_task(
    conn: quinn::Connection,
    mut rx: broadcast::Receiver<Arc<EncodedPacket>>,
    shared: Arc<RoomShared>,
) -> Result<()> {
    let fragmenter = Fragmenter::new(skreen_transport::mtu_safe_payload(quic::DEFAULT_MTU, 128));
    let mut sequence = 0u64;

    if let Ok((mut control, mut remote_control)) = quic::open_control(&conn).await {
        let hello = skreen_proto::encode_control(&ControlMessage::Hello {
            version: PROTOCOL_VERSION,
            device: DeviceId("presenter".into()),
            caps: Capabilities {
                codecs: vec!["h264".into()],
                max_width: 7680,
                max_height: 4320,
                max_fps: 60,
                platform: std::env::consts::OS.into(),
            },
        })?;
        let _ = control.write_all(&hello).await;
        let _ = control.flush().await;

        let shared = shared.clone();
        tokio::spawn(async move {
            let mut buf = vec![0u8; 4096];
            loop {
                match remote_control.read(&mut buf).await {
                    Ok(Some(0)) | Ok(None) => break,
                    Ok(Some(n)) => {
                        if let Ok(ControlMessage::KeyframeRequest { .. }) =
                            skreen_proto::decode_control(&buf[..n])
                        {
                            shared.request_keyframe();
                        }
                    }
                    Err(_) => break,
                }
            }
        });
    }

    loop {
        let packet = match rx.recv().await {
            Ok(packet) => packet,
            Err(broadcast::error::RecvError::Lagged(skipped)) => {
                tracing::warn!("viewer lagged {skipped} frames");
                continue;
            }
            Err(broadcast::error::RecvError::Closed) => return Ok(()),
        };

        for frag in fragmenter.fragment(
            "main",
            sequence,
            packet.timestamp_us,
            packet.keyframe,
            &packet.bytes,
        ) {
            let datagram = MediaDatagram {
                stream: frag.stream,
                sequence: frag.sequence,
                timestamp_us: frag.timestamp_us,
                keyframe: frag.keyframe,
                fragment_index: frag.fragment_index,
                fragment_count: frag.fragment_count,
                frame_bytes: packet.bytes.len() as u32,
                payload: frag.payload,
            };
            let body = skreen_proto::media::encode_media(&datagram)?;
            quic::send_datagram(&conn, Bytes::from(body)).await?;
            shared.stats.datagrams.fetch_add(1, Ordering::Relaxed);
        }
        sequence += 1;
    }
}

/// Blocking capture/encode loop. Runs on a dedicated thread so the async
/// QUIC driver is never starved by software H.264 encoding.
fn encode_thread(
    tx: broadcast::Sender<Arc<EncodedPacket>>,
    cfg: RoomConfig,
    shared: Arc<RoomShared>,
) -> Result<()> {
    let mut capturer = XcapCapturer::primary()?;
    capturer.start()?;
    let first = capturer
        .next_frame()?
        .ok_or_else(|| anyhow::anyhow!("screen capture returned no frames"))?;
    tracing::info!(
        width = first.width,
        height = first.height,
        name = %cfg.name,
        "room presenter started"
    );

    let mut encoder = OpenH264Encoder::new(first.width, first.height, cfg.bitrate)?;
    let frame_budget = Duration::from_secs_f64(1.0 / cfg.fps.max(1) as f64);
    let mut frame = Some(first);
    let mut last = Instant::now();
    let mut seen_requests = shared.keyframe_seq.load(Ordering::Relaxed);

    while !shared.stop.load(Ordering::Relaxed) {
        let Some(f) = frame.take() else { break };

        let requests = shared.keyframe_seq.load(Ordering::Relaxed);
        if requests != seen_requests {
            encoder.force_keyframe();
            seen_requests = requests;
        }

        for packet in encoder.encode(&f.rgba, f.width, f.height, f.timestamp_us)? {
            if tx.receiver_count() > 0 {
                let _ = tx.send(Arc::new(packet));
            } else {
                shared.idle_frames.fetch_add(1, Ordering::Relaxed);
            }
        }
        shared.stats.frames.fetch_add(1, Ordering::Relaxed);

        if last.elapsed() < frame_budget {
            std::thread::sleep(frame_budget - last.elapsed());
        }
        last = Instant::now();
        frame = capturer.next_frame()?;
    }

    capturer.stop()?;
    tracing::info!("encoder thread stopped");
    Ok(())
}


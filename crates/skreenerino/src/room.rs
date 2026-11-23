//! One-to-many presenter: capture and encode once on a dedicated thread, fan out
//! the encoded packets to every connected viewer.

use anyhow::Result;
use bytes::Bytes;
use skreen_capture::{xcap_backend::XcapCapturer, ScreenCapturer};
use skreen_codec::{h264::OpenH264Encoder, Encoder, EncodedPacket};
use skreen_proto::fragment::Fragmenter;
use skreen_proto::media::MediaDatagram;
use skreen_proto::{Capabilities, ControlMessage, DeviceId, PROTOCOL_VERSION};
use skreen_transport::{control, quic, retransmit::RetransmitBuffer};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::broadcast;

#[derive(Debug, Clone)]
pub struct RoomConfig {
    pub bitrate: u32,
    pub fps: u32,
    pub name: String,
    pub audio: bool,
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
    /// Most recent receiver quality report, if any.
    pub latest_report: Arc<std::sync::RwLock<Option<skreen_transport::quality::QualityReport>>>,
}

impl RoomShared {
    pub fn new() -> Self {
        Self {
            stats: Arc::new(RoomStats::default()),
            keyframe_seq: Arc::new(AtomicU64::new(0)),
            stop: Arc::new(AtomicBool::new(false)),
            idle_frames: Arc::new(AtomicU64::new(0)),
            latest_report: Arc::new(std::sync::RwLock::new(None)),
        }
    }

    pub fn request_keyframe(&self) {
        self.keyframe_seq.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a receiver quality report; the encode thread applies ABR changes.
    pub fn report_quality(&self, report: skreen_transport::quality::QualityReport) {
        if let Ok(mut slot) = self.latest_report.write() {
            *slot = Some(report);
        }
    }

    pub fn take_report(&self) -> Option<skreen_transport::quality::QualityReport> {
        self.latest_report.write().ok().and_then(|mut slot| slot.take())
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
    #[cfg(feature = "audio")]
    let audio_tx = broadcast::channel::<Arc<Vec<u8>>>(32).0;

    let wants_audio = cfg.audio;

    let encoder = {
        let tx = packet_tx.clone();
        let shared = shared.clone();
        std::thread::Builder::new()
            .name("skreenerino-encode".into())
            .spawn(move || encode_thread(tx, cfg, shared))
    }?;

    #[cfg(feature = "audio")]
    let audio_capture = if wants_audio {
        if skreen_audio::has_input_device() {
            let tx = audio_tx.clone();
            let stop = shared.stop.clone();
            Some(
                std::thread::Builder::new()
                    .name("skreenerino-mic".into())
                    .spawn(move || crate::audio_stream::capture_into_broadcast(tx, stop))?,
            )
        } else {
            println!("--audio requested but no input device is available; streaming video only");
            None
        }
    } else {
        None
    };
    #[cfg(feature = "audio")]
    let _ = &audio_tx;

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
                        #[cfg(feature = "audio")]
                        let audio_rx = audio_tx.subscribe();
                        let shared = shared.clone();
                        tokio::spawn(async move {
                            #[cfg(feature = "audio")]
                            let audio_stop = std::sync::Arc::new(AtomicBool::new(false));
                            #[cfg(feature = "audio")]
                            let audio_handle = crate::audio_stream::spawn_sender(
                                conn.clone(),
                                "mic".into(),
                                std::sync::Arc::new(crate::audio_stream::AudioStats::default()),
                                audio_stop.clone(),
                            )
                            .ok()
                            .flatten();

                            #[cfg(feature = "audio")]
                            let result = viewer_task_with_audio(
                                conn.clone(),
                                rx,
                                audio_rx,
                                shared.clone(),
                            )
                            .await;
                            #[cfg(not(feature = "audio"))]
                            let result = viewer_task(conn.clone(), rx, shared.clone()).await;
                            if let Err(e) = result {
                                tracing::info!("viewer {} left: {e}", conn.remote_address());
                            }

                            #[cfg(feature = "audio")]
                            {
                                audio_stop.store(true, Ordering::Relaxed);
                                if let Some(handle) = audio_handle {
                                    let _ = handle.join();
                                }
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
    #[cfg(feature = "audio")]
    if let Some(handle) = audio_capture {
        let _ = tokio::task::spawn_blocking(move || handle.join()).await;
    }
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

    // The control reader hands retransmission requests to the send loop.
    let (nack_tx, mut nack_rx) =
        tokio::sync::mpsc::unbounded_channel::<Vec<u64>>();

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
        let _ = control::write_frame(&mut control, &hello).await;

        let shared_ctrl = shared.clone();
        tokio::spawn(async move {
            loop {
                match control::read_frame(&mut remote_control).await {
                    Ok(Some(frame)) => match skreen_proto::decode_control(&frame) {
                        Ok(ControlMessage::KeyframeRequest { .. }) => shared_ctrl.request_keyframe(),
                        Ok(ControlMessage::Nack { sequences, .. }) => {
                            let _ = nack_tx.send(sequences);
                        }
                        Ok(ControlMessage::QualityReport { rtt_ms, loss_permille, decode_ms, .. }) => {
                            shared_ctrl.report_quality(
                                skreen_transport::quality::QualityReport {
                                    rtt_ms,
                                    loss_permille,
                                    decode_ms,
                                },
                            );
                        }
                        Ok(_) => {}
                        Err(_) => {}
                    },
                    Ok(None) => break,
                    Err(_) => break,
                }
            }
        });
    }

    // Retain recent datagrams so a NACK from this viewer can be answered without
    // re-encoding anything.
    let mut retransmit = RetransmitBuffer::new(1024);

    loop {
        tokio::select! {
            nack = nack_rx.recv() => {
                if let Some(sequences) = nack {
                    let (hits, misses) = {
                        let recovered = retransmit.recover(&sequences);
                        for datagram in recovered {
                            let body = skreen_proto::media::encode_media(&datagram)?;
                            let _ = quic::send_datagram(&conn, Bytes::from(body)).await;
                            shared.stats.datagrams.fetch_add(1, Ordering::Relaxed);
                        }
                        retransmit.stats()
                    };
                    tracing::debug!(?hits, ?misses, "answered NACK");
                }
            }
            packet = rx.recv() => {
                let packet = match packet {
                    Ok(packet) => packet,
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        tracing::warn!("viewer lagged {skipped} frames");
                        shared.request_keyframe();
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
                    retransmit.record(datagram);
                }
                sequence += 1;
            }
        }
    }
}

/// Viewer task that also forwards broadcast audio datagrams.
#[cfg(feature = "audio")]
async fn viewer_task_with_audio(
    conn: quinn::Connection,
    rx: broadcast::Receiver<Arc<EncodedPacket>>,
    mut audio_rx: broadcast::Receiver<Arc<Vec<u8>>>,
    shared: Arc<RoomShared>,
) -> Result<()> {
    let conn_for_audio = conn.clone();
    tokio::spawn(async move {
        while let Ok(packet) = audio_rx.recv().await {
            if conn_for_audio.send_datagram(Bytes::from(packet.as_ref().clone())).is_err() {
                break;
            }
        }
    });
    viewer_task(conn, rx, shared).await
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

    if let Ok(selection) = skreen_codec::select::select_encoder() {
        tracing::info!("{}", selection.summary());
    }

    let mut encoder = OpenH264Encoder::new(first.width, first.height, cfg.bitrate)?;
    let frame_budget = Duration::from_secs_f64(1.0 / cfg.fps.max(1) as f64);
    let mut frame = Some(first);
    let mut last = Instant::now();
    let mut seen_requests = shared.keyframe_seq.load(Ordering::Relaxed);
    let mut abr = skreen_transport::quality::AdaptiveBitrate::new(
        skreen_transport::quality::QualityTier::High,
        50,
        30,
    );
    let mut applied_tier = skreen_transport::quality::QualityTier::High;

    while !shared.stop.load(Ordering::Relaxed) {
        let Some(f) = frame.take() else { break };

        let requests = shared.keyframe_seq.load(Ordering::Relaxed);
        if requests != seen_requests {
            encoder.force_keyframe();
            seen_requests = requests;
        }

        // Apply adaptive bitrate from the latest receiver report.
        if let Some(report) = shared.take_report() {
            if let Some(tier) = abr.observe(&report, Instant::now()) {
                if tier != applied_tier {
                    if let Err(e) = encoder.set_bitrate_bps(tier.bitrate_bps()) {
                        tracing::warn!("bitrate change failed: {e}");
                    } else {
                        applied_tier = tier;
                        tracing::info!(
                            bitrate = tier.bitrate_bps(),
                            "adaptive bitrate changed quality tier"
                        );
                        shared.request_keyframe();
                    }
                }
            }
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














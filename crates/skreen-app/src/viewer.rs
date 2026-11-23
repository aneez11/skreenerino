//! Viewer client: connect, decode, feed frames to a sink, and report stats.

use crate::nack::NackScheduler;
use crate::{FrameSink, ViewerStats};
use anyhow::Result;
use skreen_render::{FrameSize, RgbaFrame};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct ViewerOptions {
    pub target: std::net::SocketAddr,
    pub stream: String,
    pub max_frames: u32,
    /// Report callback invoked roughly once a second so the UI can update.
    pub report_every: Duration,
}

impl ViewerOptions {
    pub fn new(target: std::net::SocketAddr) -> Self {
        Self {
            target,
            stream: "main".into(),
            max_frames: u32::MAX,
            report_every: Duration::from_millis(500),
        }
    }
}

/// Runs until `max_frames` decoded or the stream ends. Returns final stats.
pub async fn run_viewer(
    opts: ViewerOptions,
    sink: Arc<dyn FrameSink>,
    on_stats: impl Fn(ViewerStats) + Send + 'static,
) -> Result<ViewerStats> {
    #[cfg(all(feature = "quic", feature = "h264"))]
    {
        use skreen_codec::{h264::OpenH264Decoder, Decoder};
        use skreen_proto::fragment::{FrameFragment, Reassembler};
        use skreen_proto::media::decode_media;
        use skreen_transport::{control, quic};

        let conn = crate::discovery::connect_with_tofu(opts.target).await?;
        tracing::info!(peer = %conn.remote_address(), "connected");

        let mut reassembler = Reassembler::default();
        let mut decoder = OpenH264Decoder::new()?;

        // Control stream carries NACKs, quality reports and RTT heartbeats.
        let control = quic::open_control(&conn).await.ok();
        let (maybe_writer, maybe_reader) = match control {
            Some((send, recv)) => (Some(send), Some(recv)),
            None => (None, None),
        };
        let mut control_writer = maybe_writer;
        let rtt_ms = Arc::new(AtomicU32::new(0));
        if let Some(mut recv) = maybe_reader {
            let rtt_state = rtt_ms.clone();
            tokio::spawn(async move {
                while let Ok(Some(frame)) = control::read_frame(&mut recv).await {
                    if let Ok(skreen_proto::ControlMessage::Heartbeat { ts_ms }) =
                        skreen_proto::decode_control(&frame)
                    {
                        let now_ms = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_millis() as u64)
                            .unwrap_or(0);
                        rtt_state.store(now_ms.saturating_sub(ts_ms) as u32, Ordering::Relaxed);
                    }
                }
            });
        }
        let rtt_state = rtt_ms.clone();

        let mut nack = NackScheduler::new(64, Duration::from_millis(40));
        let mut last_nack_poll = Instant::now();
        let mut last_report = Instant::now();
        let mut last_decode_ms = 0u32;
        let mut heartbeat_sent = Instant::now();

        let mut stats = ViewerStats::default();
        let started = Instant::now();
        let mut first_frame_at: Option<Instant> = None;
        let mut last_frame_at: Option<Instant> = None;
        let mut have_size = false;

        while stats.frames < opts.max_frames {
            let payload = match quic::recv_datagram(&conn).await {
                Ok(p) => p,
                Err(e) => {
                    tracing::info!("stream ended: {e}");
                    break;
                }
            };
            stats.datagrams += 1;
            stats.bytes += payload.len();

            let datagram = match decode_media(&payload) {
                Ok(d) => d,
                Err(_) => {
                    // Not video: audio is handled by the presenter-side broadcast,
                    // and this client only renders video today.
                    continue;
                }
            };
            if datagram.keyframe {
                stats.keyframes += 1;
            }
            nack.observe(datagram.sequence);

            if last_nack_poll.elapsed() >= Duration::from_millis(40) {
                last_nack_poll = Instant::now();
                let missing = nack.poll(Instant::now());
                if let Some(writer) = control_writer.as_mut() {
                    let writer = &mut *writer;
                    if !missing.is_empty() {
                        let msg = skreen_proto::ControlMessage::Nack {
                            stream: skreen_proto::StreamId(opts.stream.clone()),
                            sequences: missing.clone(),
                        };
                        if let Ok(bytes) = skreen_proto::encode_control(&msg) {
                            let _ = control::write_frame(writer, &bytes).await;
                        }
                    }
                    let expected = stats.datagrams.max(1);
                    let loss_permille = ((missing.len() as u64 * 1000) / expected).min(1000) as u32;
                    let report = skreen_proto::ControlMessage::QualityReport {
                        stream: skreen_proto::StreamId(opts.stream.clone()),
                        rtt_ms: rtt_state.load(Ordering::Relaxed),
                        loss_permille,
                        decode_ms: last_decode_ms,
                    };
                    if let Ok(bytes) = skreen_proto::encode_control(&report) {
                        let _ = control::write_frame(writer, &bytes).await;
                    }
                    if heartbeat_sent.elapsed() >= Duration::from_millis(500) {
                        heartbeat_sent = Instant::now();
                        let ts = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_millis().min(u64::MAX as u128) as u64)
                            .unwrap_or(0);
                        let hb = skreen_proto::ControlMessage::Heartbeat { ts_ms: ts };
                        if let Ok(bytes) = skreen_proto::encode_control(&hb) {
                            let _ = control::write_frame(writer, &bytes).await;
                        }
                    }
                }
            }

            let Some(frame_bytes) = reassembler.push(FrameFragment {
                stream: datagram.stream,
                sequence: datagram.sequence,
                timestamp_us: datagram.timestamp_us,
                keyframe: datagram.keyframe,
                fragment_index: datagram.fragment_index,
                fragment_count: datagram.fragment_count,
                payload: datagram.payload,
            }) else {
                continue;
            };

            let decode_started = Instant::now();
            let decoded = decoder.decode(&frame_bytes);
            last_decode_ms = decode_started.elapsed().as_millis().min(u32::MAX as u128) as u32;
            stats.decode_ms = last_decode_ms;
            match decoded {
                Ok(Some(frame)) => {
                    let now = Instant::now();
                    first_frame_at.get_or_insert(now);
                    last_frame_at = Some(now);
                    stats.frames += 1;
                    if !have_size {
                        have_size = true;
                    }
                    stats.fps = match (first_frame_at, last_frame_at) {
                        (Some(a), Some(b)) => {
                            let span = b.duration_since(a).as_secs_f32();
                            if span > 0.0 {
                                (stats.frames.saturating_sub(1)) as f32 / span
                            } else {
                                0.0
                            }
                        }
                        _ => 0.0,
                    };
                    stats.rtt_ms = rtt_state.load(Ordering::Relaxed);
                    sink.push(
                        &opts.stream,
                        RgbaFrame {
                            size: FrameSize {
                                width: frame.width,
                                height: frame.height,
                            },
                            pixels: frame.rgba,
                        },
                    );
                }
                Ok(None) => {}
                Err(e) => tracing::warn!("decode error: {e}"),
            }

            if last_report.elapsed() >= opts.report_every {
                last_report = Instant::now();
                on_stats(stats);
            }
        }

        on_stats(stats);
        tracing::info!(
            frames = stats.frames,
            datagrams = stats.datagrams,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "viewer finished"
        );
        Ok(stats)
    }
    #[cfg(not(all(feature = "quic", feature = "h264")))]
    {
        let _ = (opts, sink, on_stats);
        anyhow::bail!("viewer requires the quic and h264 features")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_options_are_sane() {
        let opts = ViewerOptions::new("127.0.0.1:7654".parse().unwrap());
        assert_eq!(opts.stream, "main");
        assert_eq!(opts.max_frames, u32::MAX);
        assert!(opts.report_every > Duration::ZERO);
    }

    #[test]
    fn stats_merge_copies_every_field() {
        let mut a = ViewerStats::default();
        let b = ViewerStats {
            frames: 3,
            datagrams: 9,
            bytes: 100,
            keyframes: 1,
            fps: 12.5,
            decode_ms: 7,
            rtt_ms: 2,
        };
        a.merge(&b);
        assert_eq!(a.frames, 3);
        assert_eq!(a.fps, 12.5);
        assert_eq!(a.rtt_ms, 2);
    }
}

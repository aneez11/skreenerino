mod spike;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::net::SocketAddr;

#[derive(Parser, Debug)]
#[command(name = "skreenerino", about = "LAN screen projection", version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Send your screen: `skreenerino send <host:port>`
    Send {
        target: String,
        #[arg(long, default_value_t = 4_000_000)]
        bitrate: u32,
        #[arg(long, default_value_t = 30)]
        fps: u32,
    },
    /// Join a room (host:port) and display the stream
    Join { room: String },
    /// Discover peers (stub until mDNS browse loop lands)
    Discover,
    /// Capture -> openh264 -> file spike (no network)
    Spike {
        #[arg(long, default_value = "capture.h264")]
        output: String,
        #[arg(long, default_value_t = 30)]
        frames: u32,
        #[arg(long, default_value_t = 4_000_000)]
        bitrate: u32,
    },
    /// Serve a screen over QUIC datagrams
    Serve {
        #[arg(long, default_value = "0.0.0.0:7654")]
        bind: SocketAddr,
        #[arg(long, default_value_t = 4_000_000)]
        bitrate: u32,
        #[arg(long, default_value_t = 30)]
        fps: u32,
    },
    /// Receive a stream over QUIC, decode it, and report statistics
    Watch {
        addr: SocketAddr,
        #[arg(long, default_value_t = 300)]
        frames: u32,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    #[cfg(feature = "quic")]
    let _ = rustls::crypto::ring::default_provider().install_default();

    match Cli::parse().command {
        Commands::Send { target, bitrate, fps } => send(&target, bitrate, fps).await,
        Commands::Join { room } => join(&room).await,
        Commands::Discover => {
            println!("discover: mDNS browse loop not yet implemented");
            Ok(())
        }
        Commands::Spike { output, frames, bitrate } => spike::run(&output, frames, bitrate),
        Commands::Serve { bind, bitrate, fps } => serve(bind, bitrate, fps).await,
        Commands::Watch { addr, frames } => watch(addr, frames).await,
    }
}

async fn send(target: &str, bitrate: u32, fps: u32) -> Result<()> {
    if target.contains(':') {
        let addr: SocketAddr = target.parse()?;
        return stream_to(addr, bitrate, fps).await;
    }
    anyhow::bail!("peer `{target}` not found: mDNS lookup is not implemented yet; pass `host:port`")
}

#[cfg(all(feature = "quic", feature = "xcap", feature = "h264"))]
mod presenter {
    use super::*;
    use bytes::Bytes;
    use skreen_capture::{xcap_backend::XcapCapturer, ScreenCapturer};
    use skreen_codec::{h264::OpenH264Encoder, Encoder};
    use skreen_proto::fragment::Fragmenter;
    use skreen_proto::media::MediaDatagram;
    use skreen_proto::{ControlMessage, DeviceId, Capabilities, PROTOCOL_VERSION};
    use skreen_transport::quic;
    use std::time::{Duration, Instant};
    use tokio::io::AsyncWriteExt;

    pub struct PresenterConfig {
        pub bitrate: u32,
        pub fps: u32,
    }

    /// Capture + encode + fragment + send until Ctrl-C (or connection close).
    pub async fn run(conn: quinn::Connection, cfg: PresenterConfig) -> Result<()> {
        let mut capturer = XcapCapturer::primary()?;
        capturer.start()?;
        let first = capturer
            .next_frame()?
            .ok_or_else(|| anyhow::anyhow!("screen capture returned no frames"))?;
        tracing::info!(width = first.width, height = first.height, "capturing primary monitor");

        let mut encoder = OpenH264Encoder::new(first.width, first.height, cfg.bitrate)?;
        let (mut control, _recv) = quic::open_control(&conn).await?;
        let hello = skreen_proto::encode_control(&ControlMessage::Hello {
            version: PROTOCOL_VERSION,
            device: DeviceId("local".into()),
            caps: Capabilities {
                codecs: vec!["h264".into()],
                max_width: first.width,
                max_height: first.height,
                max_fps: cfg.fps,
                platform: std::env::consts::OS.into(),
            },
        })?;
        control.write_all(&hello).await?;
        control.flush().await?;

        let fragmenter = Fragmenter::new(skreen_transport::mtu_safe_payload(quic::DEFAULT_MTU, 64));
        let frame_budget = Duration::from_secs_f64(1.0 / cfg.fps.max(1) as f64);
        let mut sequence = 0u64;
        let mut frame = Some(first);
        let mut last_send = Instant::now();
        let mut sent_frames = 0u64;

        'present: while let Some(f) = frame.take() {
            if last_send.elapsed() < frame_budget {
                tokio::time::sleep(frame_budget - last_send.elapsed()).await;
            }
            last_send = Instant::now();

            for packet in encoder.encode(&f.rgba, f.width, f.height, f.timestamp_us)? {
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
                    if let Err(e) = quic::send_datagram(&conn, Bytes::from(body)).await {
                        tracing::warn!("datagram send failed: {e}");
                        break 'present;
                    }
                }
                sequence += 1;
            }
            sent_frames += 1;
            if sent_frames % 30 == 0 {
                tracing::info!(frames = sent_frames, "presenting");
            }
            frame = capturer.next_frame()?;
        }

        capturer.stop()?;
        tracing::info!(frames = sent_frames, "presenter finished");
        conn.close(0u32.into(), b"done");
        Ok(())
    }
}

async fn stream_to(addr: SocketAddr, bitrate: u32, fps: u32) -> Result<()> {
    #[cfg(all(feature = "quic", feature = "xcap", feature = "h264"))]
    {
        tracing::info!(%addr, "connecting to viewer");
        let conn = skreen_transport::quic::connect(addr).await?;
        presenter::run(conn, presenter::PresenterConfig { bitrate, fps }).await
    }
    #[cfg(not(all(feature = "quic", feature = "xcap", feature = "h264")))]
    {
        let _ = (addr, bitrate, fps);
        anyhow::bail!("send requires the quic, xcap and h264 features")
    }
}

async fn serve(bind: SocketAddr, bitrate: u32, fps: u32) -> Result<()> {
    #[cfg(all(feature = "quic", feature = "xcap", feature = "h264"))]
    {
        let server = skreen_transport::quic::QuicServer::bind(bind).await?;
        println!("serving on {} (ctrl-c to stop)", server.local_addr());
        let conn = server.accept().await?;
        println!("viewer connected: {}", conn.remote_address());
        let result = presenter::run(conn, presenter::PresenterConfig { bitrate, fps }).await;
        server.close();
        result
    }
    #[cfg(not(all(feature = "quic", feature = "xcap", feature = "h264")))]
    {
        let _ = (bind, bitrate, fps);
        anyhow::bail!("serve requires the quic, xcap and h264 features")
    }
}

async fn watch(addr: SocketAddr, max_frames: u32) -> Result<()> {
    #[cfg(all(feature = "quic", feature = "h264"))]
    {
        use skreen_codec::{h264::OpenH264Decoder, Decoder};
        use skreen_proto::media::decode_media;
        use skreen_proto::fragment::Reassembler;
        use skreen_transport::quic;

        let conn = quic::connect(addr).await?;
        println!("connected to {}", conn.remote_address());
        let mut reassembler = Reassembler::default();
        let mut decoder = OpenH264Decoder::new()?;

        let mut datagrams = 0u64;
        let mut frames = 0u32;
        let mut bytes = 0usize;
        let mut keyframes = 0u32;
        let started = std::time::Instant::now();
        let mut first_frame_at: Option<std::time::Instant> = None;
        let mut last_frame_at: Option<std::time::Instant> = None;
        let mut size_check: Option<(usize, usize)> = None;

        while frames < max_frames {
            let payload = match quic::recv_datagram(&conn).await {
                Ok(p) => p,
                Err(e) => {
                    println!("stream ended: {e}");
                    break;
                }
            };
            datagrams += 1;
            bytes += payload.len();

            let Ok(datagram) = decode_media(&payload) else {
                tracing::warn!("dropping undecodable datagram");
                continue;
            };
            if datagram.keyframe {
                keyframes += 1;
            }
            let Some(frame_bytes) = reassembler.push(skreen_proto::fragment::FrameFragment {
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

            match decoder.decode(&frame_bytes) {
                Ok(Some(rgba)) => {
                    let now = std::time::Instant::now();
                    if first_frame_at.is_none() {
                        first_frame_at = Some(now);
                    }
                    last_frame_at = Some(now);
                    frames += 1;
                    let expected = size_check.get_or_insert_with(|| (0, 0));
                    if *expected == (0, 0) {
                        *expected = (rgba.len() / 4, 1);
                    }
                }
                Ok(None) => {}
                Err(e) => tracing::warn!("decode error: {e}"),
            }
        }

        let elapsed = started.elapsed();
        let stream_span = match (first_frame_at, last_frame_at) {
            (Some(a), Some(b)) => b.duration_since(a).as_secs_f64(),
            _ => 0.0,
        };
        let fps = if stream_span > 0.0 { (frames.saturating_sub(1)) as f64 / stream_span } else { 0.0 };
        println!(
            "decoded {frames} frames from {datagrams} datagrams ({keyframes} keyframe fragments), {bytes} bytes in {elapsed:?}"
        );
        println!("steady-state {fps:.1} fps");
        Ok(())
    }
    #[cfg(not(all(feature = "quic", feature = "h264")))]
    {
        let _ = (addr, max_frames);
        anyhow::bail!("watch requires the quic and h264 features")
    }
}

async fn join(room: &str) -> Result<()> {
    if let Some(peer) = skreen_discovery::parse_manual_endpoint(room) {
        let addr: SocketAddr = format!("{}:{}", peer.host, peer.port).parse()?;
        return watch(addr, 300).await;
    }
    anyhow::bail!("unknown room `{room}`: mDNS browse not implemented yet; pass `host:port`")
}


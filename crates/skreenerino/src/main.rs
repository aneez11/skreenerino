#[cfg(feature = "audio")]
mod audio_stream;
mod gateway;
mod latency;
mod nack;
mod relay;
mod room;
mod soak;
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
    /// Serve a browser-viewable page for a stream on this host
    WebView {
        #[arg(long, default_value = "0.0.0.0:7663")]
        bind: SocketAddr,
        #[arg(long)]
        stream: Option<String>,
    },
    /// Long-running churn/leak test (requires the `soak` feature)
    Soak {
        #[arg(long, default_value_t = 5)]
        minutes: u32,
    },
    /// Run relay/SFU endpoint: forwards one presenter to many viewers
    Relay {
        #[arg(long, default_value = "0.0.0.0:7662")]
        bind: SocketAddr,
    },
    /// Join a room (host:port) and display the stream
    Join {
        room: String,
        #[arg(long, default_value_t = 300)]
        frames: u32,
    },
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
        /// Name advertised over mDNS (defaults to hostname)
        #[arg(long)]
        name: Option<String>,
        /// Room name advertised alongside this presenter
        #[arg(long)]
        room: Option<String>,
        /// Keep accepting viewers until Ctrl-C instead of exiting after one
        #[arg(long, default_value_t = false)]
        multi: bool,
        /// Also stream microphone audio (requires the `audio` feature)
        #[arg(long, default_value_t = false)]
        audio: bool,
        /// Encode at a fraction of native resolution (0.1-1.0)
        #[arg(long, default_value_t = 1.0)]
        scale: f32,
    },
    /// Measure capture -> encode -> decode latency with an embedded frame token
    Latency {
        #[arg(long, default_value_t = 60)]
        frames: u32,
        #[arg(long, default_value_t = 4_000_000)]
        bitrate: u32,
    },
    /// Capture microphone audio and verify the Opus round trip
    AudioTest {
        #[arg(long, default_value_t = 50)]
        frames: u32,
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
        Commands::Send {
            target,
            bitrate,
            fps,
        } => send(&target, bitrate, fps).await,
        Commands::Join { room, frames } => join(&room, frames).await,
        Commands::Discover => discover().await,
        Commands::Spike {
            output,
            frames,
            bitrate,
        } => spike::run(&output, frames, bitrate),
        Commands::Serve {
            bind,
            bitrate,
            fps,
            name,
            room,
            multi,
            audio,
            scale,
        } => serve(bind, bitrate, fps, name, room, multi, audio, scale).await,
        Commands::Watch { addr, frames } => watch(addr, frames).await,
        Commands::WebView { bind, stream } => web_view(bind, stream).await,
        Commands::Soak { minutes } => soak::run(minutes),
        Commands::Relay { bind } => relay::run_relay(bind).await,
        Commands::Latency { frames, bitrate } => latency::run(frames, bitrate),
        Commands::AudioTest { frames } => audio_test(frames),
    }
}

async fn send(target: &str, bitrate: u32, fps: u32) -> Result<()> {
    if let Some(peer) = skreen_discovery::parse_manual_endpoint(target) {
        let addr: SocketAddr = format!("{}:{}", peer.host, peer.port).parse()?;
        return stream_to(addr, bitrate, fps).await;
    }
    let matches = browse_peers()?;
    let Some(peer) = matches.into_iter().find(|p| p.device.0 == target) else {
        anyhow::bail!("no peer named `{target}` on this network; pass `host:port` instead")
    };
    let addr: SocketAddr = format!("{}:{}", peer.host, peer.port).parse()?;
    println!("sending to {} at {}", peer.device.0, addr);
    stream_to(addr, bitrate, fps).await
}

fn browse_peers() -> Result<Vec<skreen_discovery::AdvertisedPeer>> {
    #[cfg(feature = "mdns")]
    {
        use skreen_discovery::DiscoveryService;
        let mut d = skreen_discovery::mdns::MdnsDiscovery::new()?;
        d.browse()
    }
    #[cfg(not(feature = "mdns"))]
    {
        anyhow::bail!("built without mDNS support")
    }
}

async fn discover() -> Result<()> {
    let peers = browse_peers()?;
    if peers.is_empty() {
        println!("no peers found");
        return Ok(());
    }
    for p in peers {
        let room = p.room.as_deref().unwrap_or("-");
        println!("{}  {}:{}  room={room}", p.device.0, p.host, p.port);
    }
    Ok(())
}

#[cfg(all(feature = "quic", feature = "xcap", feature = "h264"))]
mod presenter {
    use super::*;
    use bytes::Bytes;
    use skreen_capture::{xcap_backend::XcapCapturer, ScreenCapturer};
    use skreen_codec::{h264::OpenH264Encoder, Encoder};
    use skreen_proto::fragment::Fragmenter;
    use skreen_proto::media::MediaDatagram;
    use skreen_proto::{Capabilities, ControlMessage, DeviceId, PROTOCOL_VERSION};
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
        tracing::info!(
            width = first.width,
            height = first.height,
            "capturing primary monitor"
        );

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

async fn serve(
    bind: SocketAddr,
    bitrate: u32,
    fps: u32,
    name: Option<String>,
    room: Option<String>,
    multi: bool,
    room_audio: bool,
    scale: f32,
) -> Result<()> {
    #[cfg(all(feature = "quic", feature = "xcap", feature = "h264"))]
    {
        let server = skreen_transport::quic::QuicServer::bind(bind).await?;
        let local = server.local_addr();
        println!("serving on {local} (ctrl-c to stop)");

        #[cfg(feature = "mdns")]
        let mut _discovery = {
            use skreen_discovery::DiscoveryService;
            let mut d = skreen_discovery::mdns::MdnsDiscovery::new()?;
            let device = name.clone().unwrap_or_else(default_device_name);
            let advertised_host = if local.ip().is_unspecified() {
                local_display_host()
            } else {
                local.ip().to_string()
            };
            let peer = skreen_discovery::AdvertisedPeer {
                device: skreen_proto::DeviceId(device.clone()),
                host: advertised_host,
                port: local.port(),
                room: room.clone(),
            };
            match d.advertise(&peer) {
                Ok(()) => println!("advertising `{device}` via mDNS"),
                Err(e) => eprintln!("mDNS advertise failed (streaming continues): {e}"),
            }
            d
        };

        if multi {
            let cfg = room::RoomConfig {
                bitrate,
                fps,
                name: name.clone().unwrap_or_else(default_device_name),
                audio: room_audio,
                scale,
            };
            let shared = std::sync::Arc::new(room::RoomShared::new());
            let stop = async {
                let _ = tokio::signal::ctrl_c().await;
                println!("shutting down room");
            };
            let room_name = room.unwrap_or_else(|| "default".into());
            println!("room `{room_name}` accepting multiple viewers (ctrl-c to stop)");
            let result = room::run_room(server, cfg, shared, stop).await;
            #[cfg(feature = "mdns")]
            {
                use skreen_discovery::DiscoveryService;
                let _ = _discovery.shutdown();
            }
            return result;
        }

        let conn = server.accept().await?;
        println!("viewer connected: {}", conn.remote_address());
        let result = presenter::run(conn, presenter::PresenterConfig { bitrate, fps }).await;
        #[cfg(feature = "mdns")]
        {
            use skreen_discovery::DiscoveryService;
            let _ = _discovery.shutdown();
        }
        server.close();
        result
    }
    #[cfg(not(all(feature = "quic", feature = "xcap", feature = "h264")))]
    {
        let _ = (bind, bitrate, fps, name, room, multi, room_audio, scale);
        anyhow::bail!("serve requires the quic, xcap and h264 features")
    }
}

fn default_device_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "skreenerino-host".into())
        .to_lowercase()
        .replace(' ', "-")
}

fn local_display_host() -> String {
    use std::net::UdpSocket;
    UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| {
            s.connect("8.8.8.8:80")?;
            s.local_addr()
        })
        .map(|addr| addr.ip().to_string())
        .unwrap_or_else(|_| "127.0.0.1".into())
}

#[cfg(feature = "viewer")]
mod viewer {
    use anyhow::Result;
    use skreen_render::RgbaFrame;
    use std::sync::Arc;

    pub struct ViewerHandle {
        tx: std::sync::mpsc::SyncSender<Arc<RgbaFrame>>,
        thread: Option<std::thread::JoinHandle<Result<()>>>,
    }

    impl ViewerHandle {
        pub fn push(&self, frame: RgbaFrame) {
            let _ = self.tx.try_send(Arc::new(frame));
        }
    }

    impl Drop for ViewerHandle {
        fn drop(&mut self) {
            drop(self.tx.clone());
            if let Some(handle) = self.thread.take() {
                let _ = handle.join();
            }
        }
    }

    pub fn start(title: &str) -> Result<Option<ViewerHandle>> {
        let path = std::env::current_exe().unwrap_or_default();
        if std::env::var("SKREENERINO_NO_VIEWER").is_ok() || path.to_string_lossy().contains("deps")
        {
            tracing::warn!("viewer disabled");
            return Ok(None);
        }
        let (tx, rx) = skreen_render::wgpu_viewer::frame_channel(4);
        let title = title.to_string();
        let thread = std::thread::Builder::new()
            .name("skreenerino-viewer".into())
            .spawn(move || skreen_render::wgpu_viewer::run_blocking(title, rx))?;
        Ok(Some(ViewerHandle {
            tx,
            thread: Some(thread),
        }))
    }
}
#[cfg(feature = "audio")]
fn synthetic_tone(index: u32) -> Vec<i16> {
    use std::f32::consts::TAU;
    let n = skreen_audio::FRAME_SAMPLES;
    let freq = 440.0;
    let rate = skreen_audio::SAMPLE_RATE as f32;
    (0..n)
        .map(|i| {
            let t = (index as usize * n + i) as f32 / rate;
            (TAU * freq * t)
                .sin()
                .mul_add(0.3, 0.0)
                .mul_add(i16::MAX as f32, 0.0) as i16
        })
        .collect()
}
fn audio_test(frames: u32) -> Result<()> {
    #[cfg(feature = "audio")]
    {
        use skreen_audio::{
            cpal_source::CpalSource,
            opus_codec::{OpusTalkDecoder, OpusTalkEncoder},
            AudioDecoder, AudioEncoder, AudioSource,
        };

        let mut encoder = OpusTalkEncoder::new()?;
        let mut decoder = OpusTalkDecoder::new()?;
        let mut encoded_bytes = 0usize;
        let mut mismatches = 0usize;

        let mut live_source = match CpalSource::new() {
            Ok(source) => Some(source),
            Err(e) => {
                println!("no microphone available ({e}); using a synthetic tone");
                None
            }
        };
        if let Some(source) = live_source.as_mut() {
            source.start()?;
        }

        let mut captured = 0u32;
        while captured < frames {
            let pcm = match live_source.as_mut() {
                Some(source) => match source.next_frame()? {
                    Some(frame) => frame.pcm,
                    None => continue,
                },
                None => synthetic_tone(captured),
            };
            let packet = encoder.encode(&pcm)?;
            encoded_bytes += packet.len();
            let decoded = decoder.decode(&packet)?;
            if decoded.len() != pcm.len() {
                mismatches += 1;
            }
            captured += 1;
            if captured % 10 == 0 {
                println!("{captured}/{frames} audio frames, {encoded_bytes} bytes encoded");
            }
        }
        if let Some(mut source) = live_source {
            source.stop()?;
        }
        println!(
            "processed {captured} frames of {} samples; {encoded_bytes} encoded bytes; length mismatches: {mismatches}",
            skreen_audio::FRAME_SAMPLES
        );
        Ok(())
    }
    #[cfg(not(feature = "audio"))]
    {
        let _ = frames;
        anyhow::bail!("audio-test requires the `audio` feature (cpal + opus)")
    }
}
async fn web_view(bind: SocketAddr, stream: Option<String>) -> Result<()> {
    #[cfg(all(feature = "quic", feature = "h264"))]
    {
        use skreen_codec::{h264::OpenH264Decoder, Decoder};
        use skreen_proto::fragment::Reassembler;
        use skreen_proto::media::decode_media;
        use skreen_transport::quic;
        use std::sync::{Arc, Mutex};

        let target = match stream {
            Some(s) => s,
            None => anyhow::bail!("web-view needs `--stream host:port` (or a room)"),
        };
        let addr: SocketAddr = if let Some(peer) = skreen_discovery::parse_manual_endpoint(&target)
        {
            format!("{}:{}", peer.host, peer.port).parse()?
        } else {
            let peers = browse_peers()?;
            let peer = peers
                .into_iter()
                .find(|p| p.device.0 == target || p.room.as_deref() == Some(target.as_str()))
                .ok_or_else(|| anyhow::anyhow!("no stream named `{target}`"))?;
            format!("{}:{}", peer.host, peer.port).parse()?
        };

        let jpeg: gateway::SharedJpeg = Arc::new(Mutex::new(None));
        let shared = jpeg.clone();
        tokio::spawn(async move {
            let conn = match quic::connect(addr).await {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("web gateway connect failed: {e}");
                    return;
                }
            };
            let mut reassembler = Reassembler::default();
            let mut decoder = match OpenH264Decoder::new() {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("decoder init failed: {e}");
                    return;
                }
            };
            loop {
                let Ok(payload) = quic::recv_datagram(&conn).await else {
                    break;
                };
                let Ok(datagram) = decode_media(&payload) else {
                    continue;
                };
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
                    Ok(Some(frame)) => {
                        if let Ok(encoded) =
                            gateway::rgba_to_jpeg(&frame.rgba, frame.width, frame.height, 70)
                        {
                            if let Ok(mut slot) = shared.lock() {
                                *slot = Some(encoded);
                            }
                        }
                    }
                    Ok(None) => {}
                    Err(e) => tracing::warn!("gateway decode error: {e}"),
                }
            }
        });

        let stats = std::sync::Arc::new(gateway::GatewayStats::default());
        gateway::run(bind, jpeg, stats).await
    }
    #[cfg(not(all(feature = "quic", feature = "h264")))]
    {
        let _ = (bind, stream);
        anyhow::bail!("web-view requires the quic and h264 features")
    }
}
async fn watch(addr: SocketAddr, max_frames: u32) -> Result<()> {
    #[cfg(all(feature = "quic", feature = "h264"))]
    {
        use skreen_codec::{h264::OpenH264Decoder, Decoder};
        use skreen_proto::fragment::Reassembler;
        use skreen_proto::media::decode_media;
        use skreen_transport::quic;

        let conn = quic::connect(addr).await?;
        println!("connected to {}", conn.remote_address());
        let mut reassembler = Reassembler::default();
        let mut decoder = OpenH264Decoder::new()?;

        // Control stream: NACKs, quality reports, and RTT heartbeats.
        let control = quic::open_control(&conn).await.ok();
        let (maybe_writer, maybe_reader) = match control {
            Some((send, recv)) => (Some(send), Some(recv)),
            None => (None, None),
        };
        let mut control_writer = maybe_writer;
        let rtt_ms = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        if let Some(mut recv) = maybe_reader {
            let rtt_state = rtt_ms.clone();
            tokio::spawn(async move {
                while let Ok(Some(frame)) = skreen_transport::control::read_frame(&mut recv).await {
                    if let Ok(skreen_proto::ControlMessage::Heartbeat { ts_ms }) =
                        skreen_proto::decode_control(&frame)
                    {
                        let now_ms = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_millis() as u64)
                            .unwrap_or(0);
                        rtt_state.store(
                            now_ms.saturating_sub(ts_ms) as u32,
                            std::sync::atomic::Ordering::Relaxed,
                        );
                    }
                }
            });
        }
        let rtt_state = rtt_ms.clone();
        let mut nack_scheduler =
            crate::nack::NackScheduler::new(64, std::time::Duration::from_millis(40));
        let mut last_nack_poll = std::time::Instant::now();
        let mut last_decode_ms = 0u32;
        #[cfg(feature = "audio")]
        let audio_stats = std::sync::Arc::new(crate::audio_stream::AudioStats::default());
        #[cfg(feature = "audio")]
        let mut audio_player = {
            let sink = skreen_audio::cpal_sink::AudioSink::new();
            let decoder = skreen_audio::opus_codec::OpusTalkDecoder::new();
            match (sink, decoder) {
                (Ok(s), Ok(d)) => Some((s, d)),
                _ => {
                    tracing::warn!("no audio output device; audio will be ignored");
                    None
                }
            }
        };
        let mut heartbeat_sent = std::time::Instant::now();
        let _viewer = viewer::start("skreenerino viewer")?;

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

            let datagram = match decode_media(&payload) {
                Ok(d) => d,
                Err(_) => {
                    // Not a video datagram; try audio when this build supports it.
                    #[cfg(feature = "audio")]
                    if let Ok(audio) = skreen_proto::media::decode_audio(&payload) {
                        audio_stats
                            .frames
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        audio_stats.bytes.fetch_add(
                            audio.opus.len() as u64,
                            std::sync::atomic::Ordering::Relaxed,
                        );
                        if let Some((sink, decoder)) = audio_player.as_mut() {
                            if let Ok(pcm) =
                                skreen_audio::AudioDecoder::decode(decoder, &audio.opus)
                            {
                                sink.push(&pcm);
                            }
                        }
                    }
                    continue;
                }
            };
            if datagram.keyframe {
                keyframes += 1;
            }
            nack_scheduler.observe(datagram.sequence);

            // Ask the presenter to resend any settled gaps.
            if last_nack_poll.elapsed() >= std::time::Duration::from_millis(40) {
                last_nack_poll = std::time::Instant::now();
                let missing = nack_scheduler.poll(std::time::Instant::now());
                if let Some(writer) = control_writer.as_mut() {
                    let writer = &mut *writer;
                    if !missing.is_empty() {
                        let msg = skreen_proto::ControlMessage::Nack {
                            stream: skreen_proto::StreamId("main".into()),
                            sequences: missing.clone(),
                        };
                        if let Ok(bytes) = skreen_proto::encode_control(&msg) {
                            let _ = skreen_transport::control::write_frame(writer, &bytes).await;
                        }
                    }

                    // Feed the presenter's adaptive bitrate controller with the
                    // loss rate observed in this reporting window.
                    let expected = datagrams.max(1);
                    let loss_permille = ((missing.len() as u64 * 1000) / expected).min(1000) as u32;
                    let report = skreen_proto::ControlMessage::QualityReport {
                        stream: skreen_proto::StreamId("main".into()),
                        rtt_ms: rtt_state.load(std::sync::atomic::Ordering::Relaxed),
                        loss_permille,
                        decode_ms: last_decode_ms,
                    };
                    if let Ok(bytes) = skreen_proto::encode_control(&report) {
                        let _ = skreen_transport::control::write_frame(writer, &bytes).await;
                    }
                }
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

            let decode_started = std::time::Instant::now();
            let decoded = decoder.decode(&frame_bytes);
            let decode_ms = decode_started.elapsed().as_millis().min(u32::MAX as u128) as u32;
            last_decode_ms = decode_ms;
            match decoded {
                Ok(Some(frame)) => {
                    let now = std::time::Instant::now();
                    if first_frame_at.is_none() {
                        first_frame_at = Some(now);
                    }
                    last_frame_at = Some(now);
                    frames += 1;
                    let size =
                        size_check.get_or_insert((frame.width as usize, frame.height as usize));
                    if *size == (0, 0) {
                        *size = (frame.width as usize, frame.height as usize);
                    }
                    #[cfg(feature = "viewer")]
                    if let Some(ref viewer) = _viewer {
                        viewer.push(skreen_render::RgbaFrame {
                            size: skreen_render::FrameSize {
                                width: frame.width,
                                height: frame.height,
                            },
                            pixels: frame.rgba,
                        });
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
        let fps = if stream_span > 0.0 {
            (frames.saturating_sub(1)) as f64 / stream_span
        } else {
            0.0
        };
        println!(
            "decoded {frames} frames from {datagrams} datagrams ({keyframes} keyframe fragments), {bytes} bytes in {elapsed:?}"
        );
        println!("steady-state {fps:.1} fps");
        #[cfg(feature = "audio")]
        println!(
            "audio datagrams: {} ({} bytes)",
            audio_stats
                .frames
                .load(std::sync::atomic::Ordering::Relaxed),
            audio_stats.bytes.load(std::sync::atomic::Ordering::Relaxed)
        );
        Ok(())
    }
    #[cfg(not(all(feature = "quic", feature = "h264")))]
    {
        let _ = (addr, max_frames);
        anyhow::bail!("watch requires the quic and h264 features")
    }
}

async fn join(room: &str, frames: u32) -> Result<()> {
    if let Some(peer) = skreen_discovery::parse_manual_endpoint(room) {
        let addr: SocketAddr = format!("{}:{}", peer.host, peer.port).parse()?;
        return watch(addr, frames).await;
    }
    let peers = browse_peers()?;
    let Some(peer) = peers
        .into_iter()
        .find(|p| p.device.0 == room || p.room.as_deref() == Some(room))
    else {
        anyhow::bail!("no room or peer named `{room}`; pass `host:port` instead")
    };
    let addr: SocketAddr = format!("{}:{}", peer.host, peer.port).parse()?;
    println!("joining {} at {}", peer.device.0, addr);
    watch(addr, frames).await
}

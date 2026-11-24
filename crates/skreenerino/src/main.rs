#[cfg(feature = "gui")]
mod app;
#[cfg(feature = "audio")]
mod audio_stream;
mod gateway;
#[cfg(feature = "gui")]
mod icons;
mod latency;
#[cfg(feature = "gui")]
mod modals;
#[cfg(feature = "gui")]
mod runtime;
#[cfg(feature = "gui")]
mod screens;
#[cfg(feature = "gui")]
mod theme;
#[cfg(feature = "gui")]
mod tray;
#[cfg(feature = "gui")]
mod widgets;
// nack + viewer now live in skreen-app
mod relay;
use skreen_app::presenter as room;
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
    /// Open the desktop application window
    #[cfg(feature = "gui")]
    App {
        /// Join this room or host:port immediately on startup
        #[arg(long)]
        join: Option<String>,
        /// Close automatically after this many seconds (for scripted runs)
        #[arg(long)]
        exit_after: Option<u64>,
    },
    /// Show several presenters in one window as a grid wall
    Wall {
        /// Presenter addresses (`host:port`) or mDNS names
        streams: Vec<String>,
        #[arg(long, default_value_t = 300)]
        frames: u32,
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
    /// List peers advertising over mDNS
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
        /// Monitor id to share (default: primary display)
        #[arg(long)]
        monitor: Option<u32>,
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
        /// Render in the egui window instead of a bare video surface
        #[arg(long, default_value_t = false)]
        gui: bool,
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
            monitor,
        } => serve(bind, bitrate, fps, name, room, multi, audio, scale, monitor).await,
        Commands::Watch { addr, frames, gui } => watch(addr, frames, gui).await,
        Commands::Soak { minutes } => soak::run(minutes),
        #[cfg(feature = "gui")]
        Commands::App { exit_after, join } => {
            let _ = rustls::crypto::ring::default_provider().install_default();
            app::run_app(exit_after.map(std::time::Duration::from_secs), join)
        }
        Commands::WebView { bind, stream } => web_view(bind, stream).await,
        Commands::Wall { streams, frames } => wall(streams, frames).await,
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
        // Periodic keyframe so late joiners (including viewers behind a relay)
        // become decodable without waiting for a request round trip.
        let keyframe_interval = cfg.fps.max(1) as u64;

        'present: while let Some(f) = frame.take() {
            if last_send.elapsed() < frame_budget {
                tokio::time::sleep(frame_budget - last_send.elapsed()).await;
            }
            last_send = Instant::now();

            if sent_frames % keyframe_interval == 0 {
                encoder.force_keyframe();
            }

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
    monitor: Option<u32>,
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
                monitor,
                require_approval: false,
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
        let _ = (
            bind, bitrate, fps, name, room, multi, room_audio, scale, monitor,
        );
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

    /// Returns `None` when the viewer is suppressed (headless/CI).
    fn viewer_disabled() -> bool {
        let path = std::env::current_exe().unwrap_or_default();
        std::env::var("SKREENERINO_NO_VIEWER").is_ok() || path.to_string_lossy().contains("deps")
    }

    pub fn start(title: &str) -> Result<Option<ViewerHandle>> {
        if viewer_disabled() {
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

    /// egui shell: same stream, plus the Fluent window chrome and stats strip.
    #[cfg(feature = "gui")]
    pub struct GuiHandle {
        tx: std::sync::mpsc::SyncSender<skreen_render::gui::GuiInput>,
        thread: Option<std::thread::JoinHandle<Result<skreen_render::gui::GuiReport>>>,
    }

    #[cfg(feature = "gui")]
    impl GuiHandle {
        pub fn push(&self, key: &str, frame: RgbaFrame) {
            let _ = self.tx.try_send(skreen_render::gui::GuiInput::Frame {
                key: key.to_string(),
                frame: Arc::new(frame),
            });
        }
    }

    #[cfg(feature = "gui")]
    impl Drop for GuiHandle {
        fn drop(&mut self) {
            // Wake the window thread so it can observe the close request.
            let _ = self.tx.try_send(skreen_render::gui::GuiInput::End);
            if let Some(handle) = self.thread.take() {
                match handle.join() {
                    Ok(Ok(report)) => println!(
                        "gui: {:.1} fps, {} frames in {:.1}s across {} stream(s)",
                        report.fps, report.frames_shown, report.elapsed_secs, report.streams
                    ),
                    Ok(Err(e)) => eprintln!("gui: {e:#}"),
                    Err(_) => eprintln!("gui: window thread panicked"),
                }
            }
        }
    }

    #[cfg(feature = "gui")]
    pub fn start_gui(title: &str, limit: Option<u32>) -> Result<Option<GuiHandle>> {
        if viewer_disabled() {
            tracing::warn!("gui disabled");
            return Ok(None);
        }
        let (tx, rx) = skreen_render::gui::keyed_channel(4);
        let title = title.to_string();
        let thread = std::thread::Builder::new()
            .name("skreenerino-gui".into())
            .spawn(move || skreen_render::gui::run_blocking_gui(title, rx, limit))?;
        Ok(Some(GuiHandle {
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
            let conn = match connect_with_tofu(addr).await {
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
/// Connect to several presenters and render them in one grid window.
/// Connect using the pinned certificate for this host when one exists, and pin
/// the certificate on first contact (trust on first use).
#[cfg(feature = "quic")]
async fn connect_with_tofu(addr: SocketAddr) -> Result<quinn::Connection> {
    use skreen_security::TofuStore;
    use skreen_transport::identity::{check_pin, PinDecision};

    let path = skreen_security::default_tofu_path();
    let mut store = TofuStore::load(&path).unwrap_or_default();
    let host = addr.ip().to_string();
    let pinned = store.fingerprint(&host);

    match pinned {
        Some(fp) => match skreen_transport::quic::connect_pinned(addr, Some(&fp)).await {
            Ok(conn) => Ok(conn),
            Err(e) => {
                // A mismatch must stop the connection rather than silently retrust.
                anyhow::bail!(
                    "refusing to connect to {host}: certificate does not match the pinned \
                     fingerprint ({e}). Remove the entry from {} if the device was legitimately \
                     reinstalled.",
                    path.display()
                )
            }
        },
        None => {
            let conn = skreen_transport::quic::connect(addr).await?;
            if let Some(cert) = conn
                .peer_identity()
                .and_then(|id| {
                    id.downcast::<Vec<rustls::pki_types::CertificateDer<'static>>>()
                        .ok()
                })
                .and_then(|certs| certs.first().map(|c| c.as_ref().to_vec()))
            {
                if let PinDecision::FirstContact(fp) = check_pin(None, &cert) {
                    let _ = store.verify_or_pin(&host, &fp);
                    let _ = store.save(&path);
                    println!("pinned {host} certificate {fp}");
                }
            }
            Ok(conn)
        }
    }
}

async fn wall(streams: Vec<String>, max_frames: u32) -> Result<()> {
    #[cfg(all(feature = "quic", feature = "h264", feature = "viewer"))]
    {
        use skreen_codec::{h264::OpenH264Decoder, Decoder};
        use skreen_proto::fragment::Reassembler;
        use skreen_proto::media::decode_media;
        use skreen_transport::quic;
        use std::sync::Arc;

        if streams.is_empty() {
            anyhow::bail!("wall needs at least one presenter: `skreenerino wall host:port ...`");
        }

        let (tx, rx) = skreen_render::wgpu_viewer::keyed_frame_channel(64);
        let total_frames = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let limit = max_frames;

        for target in streams {
            let addr: SocketAddr = match skreen_discovery::parse_manual_endpoint(&target) {
                Some(peer) => format!("{}:{}", peer.host, peer.port).parse()?,
                None => {
                    let peers = browse_peers()?;
                    let peer = peers
                        .into_iter()
                        .find(|p| {
                            p.device.0 == target || p.room.as_deref() == Some(target.as_str())
                        })
                        .ok_or_else(|| anyhow::anyhow!("no stream named `{target}`"))?;
                    format!("{}:{}", peer.host, peer.port).parse()?
                }
            };

            let tx = tx.clone();
            let total_frames = total_frames.clone();
            let key = target.clone();
            tokio::spawn(async move {
                let conn = match connect_with_tofu(addr).await {
                    Ok(c) => c,
                    Err(e) => {
                        eprintln!("wall: {key} connect failed: {e}");
                        return;
                    }
                };
                let mut reassembler = Reassembler::default();
                let mut decoder = match OpenH264Decoder::new() {
                    Ok(d) => d,
                    Err(e) => {
                        eprintln!("wall: {key} decoder failed: {e}");
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
                    let Some(bytes) = reassembler.push(skreen_proto::fragment::FrameFragment {
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
                    if let Ok(Some(frame)) = decoder.decode(&bytes) {
                        let count = total_frames.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        if count >= limit {
                            break;
                        }
                        let _ = tx.send((
                            key.clone(),
                            Arc::new(skreen_render::RgbaFrame {
                                size: skreen_render::FrameSize {
                                    width: frame.width,
                                    height: frame.height,
                                },
                                pixels: frame.rgba,
                            }),
                        ));
                    }
                }
            });
        }

        drop(tx);
        skreen_render::wgpu_viewer::run_blocking_keyed("skreenerino wall".into(), rx)
    }
    #[cfg(not(all(feature = "quic", feature = "h264", feature = "viewer")))]
    {
        let _ = (streams, max_frames);
        anyhow::bail!("wall requires the quic, h264 and viewer features")
    }
}
/// Adapts the CLI render viewer to skreen-app's `FrameSink`.
#[cfg(feature = "viewer")]
struct CliSink {
    #[cfg(feature = "gui")]
    gui: Option<viewer::GuiHandle>,
    viewer: Option<viewer::ViewerHandle>,
}

#[cfg(feature = "viewer")]
impl skreen_app::FrameSink for CliSink {
    fn push(&self, stream: &str, frame: skreen_render::RgbaFrame) {
        #[cfg(feature = "gui")]
        if let Some(gui) = &self.gui {
            gui.push(stream, frame);
            return;
        }
        let _ = stream;
        if let Some(viewer) = &self.viewer {
            viewer.push(frame);
        }
    }
}

async fn watch(addr: SocketAddr, max_frames: u32, use_gui: bool) -> Result<()> {
    #[cfg(all(feature = "quic", feature = "h264"))]
    {
        #[cfg(feature = "viewer")]
        let sink: std::sync::Arc<dyn skreen_app::FrameSink> = {
            // egui shell when requested, otherwise the bare video window.
            #[cfg(feature = "gui")]
            {
                let gui = if use_gui {
                    viewer::start_gui("skreenerino", Some(max_frames))?
                } else {
                    None
                };
                let plain = if gui.is_none() {
                    viewer::start("skreenerino viewer")?
                } else {
                    None
                };
                std::sync::Arc::new(CliSink { gui, viewer: plain })
            }
            #[cfg(not(feature = "gui"))]
            {
                let _ = use_gui;
                std::sync::Arc::new(CliSink {
                    viewer: viewer::start("skreenerino viewer")?,
                })
            }
        };

        let opts = skreen_app::viewer::ViewerOptions {
            max_frames,
            ..skreen_app::viewer::ViewerOptions::new(addr)
        };
        let stats = skreen_app::viewer::run_viewer(opts, sink, |s| {
            tracing::debug!(frames = s.frames, fps = s.fps, "viewer progress");
        })
        .await?;
        println!(
            "decoded {} frames from {} datagrams ({} keyframe fragments), {} bytes",
            stats.frames, stats.datagrams, stats.keyframes, stats.bytes
        );
        println!("steady-state {:.1} fps", stats.fps);
        Ok(())
    }
    #[cfg(not(all(feature = "quic", feature = "h264")))]
    {
        let _ = (addr, max_frames, use_gui);
        anyhow::bail!("watch requires the quic and h264 features")
    }
}

#[cfg(not(feature = "viewer"))]
struct NullSink;

#[cfg(not(feature = "viewer"))]
impl skreen_app::FrameSink for NullSink {
    fn push(&self, _stream: &str, _frame: skreen_render::RgbaFrame) {}
}

async fn join(room: &str, frames: u32) -> Result<()> {
    if let Some(peer) = skreen_discovery::parse_manual_endpoint(room) {
        let addr: SocketAddr = format!("{}:{}", peer.host, peer.port).parse()?;
        return watch(addr, frames, false).await;
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
    watch(addr, frames, false).await
}

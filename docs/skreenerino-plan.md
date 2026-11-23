# Skreenerino: LAN Screen Projection in Rust (Multi-Platform, Multi-User)

> *Skreenerino* is a Rust app that projects your screen to everyone on the same network, and lets users project to each other. CLI: `skreenerino send bob`, `skreenerino join classroom`.

## 1. Goals and scope

- **Core:** capture a screen and project it live to one or more devices on the same network.
- **Platforms:** Windows, macOS and Linux first. Android and iOS (viewer first), plus a browser viewer, later.
- **Topologies:** one-to-many (presenter to viewers), peer-to-peer ("send my screen to Bob"), and many simultaneous presenters in a room.
- **Targets:** under 100 ms glass-to-glass latency on wired or good Wi-Fi, 1080p30 as baseline, 1080p60 and 4K as stretch.
- **Non-goals for v1:** internet/NAT traversal, remote control, recording.

## 2. Architecture (Cargo workspace)

```
skreenerino/
├─ crates/
│  ├─ skreen-proto/       # message types (serde + postcard/protobuf), versioning
│  ├─ skreen-discovery/   # mDNS/DNS-SD (mdns-sd crate) peer + room advertising
│  ├─ skreen-capture/     # trait ScreenCapturer + per-OS backends
│  ├─ skreen-codec/       # trait Encoder/Decoder + SW/HW backends
│  ├─ skreen-transport/   # QUIC (quinn), packetizer, jitter buffer, FEC/NACK
│  ├─ skreen-session/     # room state machine, roles, permissions, routing
│  ├─ skreen-security/    # certs, PIN pairing, consent
│  ├─ skreen-render/      # wgpu video surface
│  ├─ skreen-audio/       # cpal + opus (phase 2+)
│  └─ skreenerino/        # UI (egui/eframe or Tauri) + CLI + daemon
```

Every platform-specific piece (capture, encode, decode, render) sits behind a trait, so the rest of the code is platform-neutral.

## 3. Technology choices

| Concern | Choice | Notes |
|---|---|---|
| Capture, Windows | Windows.Graphics.Capture or DXGI Desktop Duplication (`windows-capture`, `windows` crate) | Prefer GPU texture output to avoid CPU copies |
| Capture, macOS | ScreenCaptureKit (`screencapturekit`) | Needs Screen Recording permission |
| Capture, Linux | PipeWire via xdg-desktop-portal (`ashpd`, `pipewire`) for Wayland; `xcap` or XShm for X11 | Wayland requires a user prompt each session |
| Capture, Android/iOS | MediaProjection via JNI / ReplayKit extension | Phase 4 |
| Encode | MVP: `openh264`. Then NVENC, QSV/AMF (Media Foundation), VideoToolbox, VAAPI. Optional: AV1 (`rav1e`/SVT-AV1) | Keep H.264 for broad hardware decode; AV1 avoids patent concerns |
| Alternative media stack | `gstreamer-rs` or `ffmpeg-next` | Faster to get hardware codecs, heavier to ship |
| Transport | QUIC with `quinn`: reliable streams for control, unreliable datagrams for video | TLS 1.3 built in, no NAT needed on LAN |
| Discovery | `mdns-sd` | Optional manual IP/QR fallback for networks that block mDNS |
| Rendering | `wgpu` + `winit` | Upload decoded frames as textures |
| UI | `egui` (simple, pure Rust) or Tauri (richer UI) | |
| Audio | `cpal` + `opus` | Phase 2 |
| Async/runtime | `tokio` | |

## 4. Protocol design

**Control plane** (QUIC reliable streams, message types in `skreen-proto`):
`Hello/Capabilities` (codecs, max resolution, platform), `RoomJoin/Leave`, `PresentRequest`, `ViewRequest`, `Offer/Accept/Reject`, `StreamStart/Stop`, `KeyframeRequest`, `QualityReport`, `RoleChange`, `Heartbeat`.

**Media plane** (QUIC datagrams or a dedicated unidirectional stream per frame):

- Frame header: stream ID, sequence, timestamp, keyframe flag, fragment index.
- Fragment frames to fit the MTU, with a small reorder/jitter buffer on the receiver.
- Loss handling: request a keyframe on loss, add NACK for fast retransmit, optional XOR FEC.
- Adaptive bitrate driven by receiver reports (RTT, loss, decode time). Drop resolution or fps first, then bitrate.
- Send a keyframe immediately when a new viewer joins.

## 5. Multi-user model

**Roles:** every node can be Presenter, Viewer, or both. A **Room** is a named group discovered via mDNS and joined with a passcode or PIN.

**Interaction modes:**

1. **Broadcast:** a presenter streams to the room, and any viewer can join.
2. **Push:** "Send my screen to Bob." Bob gets a consent prompt.
3. **Pull:** "View Alice's screen." Alice gets a consent prompt.
4. **Pass the baton:** the presenter role transfers to another user, and the old stream stops cleanly.
5. **Multi-presenter:** several streams at once, with viewers choosing a tile layout (single, grid, picture-in-picture).

**Fan-out strategy** (the main scaling decision):

- **Phase 2:** direct unicast, with the encoder running once and packets duplicated per viewer. This works well up to roughly 10 to 20 viewers.
- **Phase 3:** add an optional relay/SFU role. Any node, or a dedicated box, forwards streams without re-encoding, and the room can elect one automatically.
- **Experiment:** multicast for large rooms on wired LANs. It performs poorly on Wi-Fi, so keep it opt-in.
- **Simulcast:** the presenter encodes two or three quality tiers, and weak clients subscribe to a lower one.

**Room coordination:** start decentralized, with the room creator acting as coordinator and a deterministic handover if they drop. This avoids needing a central server.

## 6. Security

- Self-signed certificates per device, with a fingerprint pinned on first pairing (TOFU).
- PIN or QR pairing using a PAKE (e.g. SPAKE2) to prevent man-in-the-middle attacks on first contact.
- A consent prompt for every push or pull, with a persistent "this screen is being shared" indicator.
- Per-window or per-monitor sharing so users don't expose their whole desktop by accident.
- Room passcodes and an optional allowlist.

## 7. Phased roadmap

| Phase | Deliverable | Rough time |
|---|---|---|
| 0. Spikes | Capture to openh264 to a file on each OS. QUIC datagram throughput test on Wi-Fi. | 1–2 wks |
| 1. MVP | Desktop 1-to-1 stream with mDNS discovery, PIN pairing, wgpu viewer. Target 1080p30. | 4–6 wks |
| 2. One-to-many | Room join, fan-out, keyframe-on-join, adaptive bitrate, audio, basic UI. | 4–5 wks |
| 3. Multi-user | Push/pull/baton, multi-presenter tiling, relay/SFU role, hardware encoders. | 5–7 wks |
| 4. Reach | Android capture and viewer, iOS viewer, browser viewer (WebTransport or WebRTC gateway). | 6–8 wks |
| 5. Hardening | Packaging (MSI, dmg/notarization, AppImage/Flatpak), auto-update, telemetry opt-in, docs. | 3–4 wks |

## 8. Testing and CI

- **Unit/property tests:** the protocol codec, the fragmenter and reassembler, and the room state machine.
- **Network simulation:** `tc netem` or `clumsy` for loss, jitter and latency, with a CI target for graceful degradation at 5% loss.
- **Latency harness:** embed a timestamp or QR code in the captured frame and measure it at the viewer.
- **CI matrix:** GitHub Actions on Windows, macOS and Linux with cross-compilation. Run headless tests against a virtual display (Xvfb) on Linux.
- **Soak tests:** 8-hour sessions with join/leave churn and memory-leak checks.

## 9. Key risks

| Risk | Mitigation |
|---|---|
| Wayland, macOS and Windows permission flows differ | Isolate them in per-OS capture backends and build a clear first-run permission UX |
| Wi-Fi jitter and multicast unreliability | Default to unicast, add adaptive bitrate, and make multicast opt-in |
| Hardware encoder fragmentation | A trait-based codec layer with automatic software fallback |
| H.264 licensing | Ship openh264's Cisco binary or offer AV1 as an alternative |
| Mobile capture constraints (especially iOS) | Make iOS viewer-only at first |
| Enterprise networks blocking mDNS or UDP | Manual IP/QR join and a TCP/QUIC fallback |

## 10. First steps this week

1. Create the `skreenerino` workspace and the `skreen-capture`, `skreen-codec` and `skreen-transport` trait definitions.
2. Build a Windows or Linux capture-to-openh264-to-QUIC-to-wgpu pipeline end to end, even if it's ugly.
3. Measure glass-to-glass latency before adding any features.

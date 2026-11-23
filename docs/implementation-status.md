# Implementation Status

Last updated: all five roadmap phases have working, tested implementations of the
platform-neutral and Windows-verifiable parts. Platform-exclusive pieces (macOS
capture, Linux portal capture, iOS/Android) remain scaffolding behind clear errors.

## Verified working on this machine

| Capability | Evidence |
|---|---|
| Workspace builds and tests pass | `cargo test --workspace` → 61 tests; `--features audio` → 11 more |
| Screen capture (Windows) | `spike` wrote 20 H.264 frames; `serve` streams the primary monitor |
| H.264 encode (openh264, screen-content tune) | `skreen-codec::h264::OpenH264Encoder` |
| H.264 decode | `watch` decoded 150 frames in release |
| Frame fragmentation + reassembly | unit tests + live streams |
| Media datagram framing | `skreen-proto::media` round-trip test |
| QUIC transport (ALPN + self-signed certs) | `serve`/`watch` handshake over loopback and LAN |
| One-to-one stream | `serve` → `watch`, 40–150 frames decoded |
| Multi-viewer fan-out + keyframe-on-join | two concurrent viewers each decoded 30/30 |
| mDNS advertise/browse | `discover` lists peers; `join <room>` resolves automatically |
| Live wgpu viewer window | `join` opens a window rendering remote frames |
| Browser viewer | HTTP page + MJPEG stream verified (200 + JPEG multipart) |
| Relay/SFU forwarding | `relay` command; role classification tested |
| Multi-presenter session model | `active_streams`, `presenters`, `stop_presenting` tested |
| Tile layouts (single/grid/PiP) | layout + hit-test tests; wired into the viewer's grid render |
| Room coordinator handover | session tests |
| TOFU certificate pinning | Enforced on the QUIC handshake: first contact pins and saves the fingerprint; a changed certificate is refused (verified end to end) |
| Push/pull consent gate | allowlist + remember/deny tests |
| Adaptive bitrate tiers | step-down/step-up tests |
| Loss/NACK tracking | missing-sequence tests |
| Audio Opus round trip | 20 frames, 1720 bytes, 0 mismatches (synthetic tone; no mic) |
| Network impairment simulation | deterministic loss/jitter tests at 5% loss |
| Soak test | 1-minute churn run; found and fixed an unbounded stream leak |
| CI matrix | GitHub Actions: Ubuntu/Windows/macOS + fmt + clippy |
| Packaging | `scripts/package.ps1` → `dist/` with binary + MinGW DLLs |

## Known gaps

| Item | Status |
|---|---|
'| macOS capture | Uses the `xcap` ScreenCaptureKit path behind `default_capturer()`; requires Screen Recording permission (unverified: no Mac) |'
| Linux capture | X11 works through `xcap`; Wayland needs an xdg-desktop-portal ScreenCast session (unverified: no Linux host) |
| Hardware encoders | Selection layer reports candidates and falls back to software with an explicit reason; driver bindings not implemented |
| Audio over the wire | Room `--audio` streams one capture to all viewers; viewer decodes and plays through cpal |
| NACK retransmission | Implemented end to end over a duplex control stream: viewer detects gaps, presenter resends from a bounded buffer |
| Adaptive bitrate | Closed loop verified: measured decode_ms (132–175 ms, debug) drove High→Medium→Low; encoder rebuilt per tier |
| Consent UI | Gate logic exists; no on-screen prompt (planned: `docs/gui-plan.md`) |
| Android/iOS clients | Not started |
| On-device target validation | Needs a second machine and real LAN conditions |

## Build and test

```powershell
$env:CARGO_HOME=".cargo-home"
$env:PATH="$PWD\.tools\mingw\mingw64\bin;$env:PATH"
$env:CMAKE_POLICY_VERSION_MINIMUM="3.5"   # only needed for the audio feature
$env:USERPROFILE\.cargo\bin\cargo.exe test --workspace
```

See `docs/benchmarks.md` for measured latency and throughput numbers.
## GUI (docs/gui-plan.md, `--features gui`)

Verified on this machine with `skreenerino-app app --join <host:port>`: 188 frames
decoded at 13.9 fps, one live stream rendered in the Meeting view.

| Milestone | State |
|---|---|
| M0 egui shell over video | Done. egui 0.31.1 matching wgpu 24 / winit 0.30; video drawn as egui textures |
| M1 `skreen-app` extraction | Done. Presenter, viewer, discovery, NACK, audio and settings live in the library; CLI runs on it |
| M2 Home screen | Done. Sidebar nav, Share/Join/Pair actions, nearby-device list with Join buttons, manual `host:port`, recent rooms |
| M3 Meeting view | Done. Video grid, name chips, participants rail with role badges, circular control bar with red Leave, status strip |
| M4 Windows 11 look | Done. Mica backdrop, rounded corners, immersive dark mode, Segoe UI Variable, Fluent tokens, procedural icons |
| M5 Dialogs | Share picker, pair-device code, stop-sharing confirmation, Settings and About pages, persisted settings |

Threading follows the plan: the UI thread never blocks, frames cross a bounded
newest-wins channel, and commands reach tokio through a bridge thread (a blocking
`recv()` inside `block_on` starved the session tasks and was fixed during M3).

Not implemented from the mockup: the tray icon and menu, the "view request"
consent dialog (the consent gate exists in `skreen-security` but has no UI yet),
and a multi-monitor thumbnail capture (the picker shows placeholder thumbnails).

Tests: `cargo test --workspace` -> 72 passing.
# Implementation Status

Last updated: all five roadmap phases have working, tested implementations of the
platform-neutral and Windows-verifiable parts. Platform-exclusive pieces (macOS
capture, Linux portal capture, iOS/Android) remain scaffolding behind clear errors.

## Verified working on this machine

| Capability | Evidence |
|---|---|
| Workspace builds and tests pass | `cargo test --workspace` → 57 tests; `--features audio` → 11 more |
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
| Tile layouts (single/grid/PiP) | layout + hit-test tests |
| Room coordinator handover | session tests |
| TOFU pinning, PIN pairing | security tests |
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
| macOS capture (ScreenCaptureKit) | Scaffolded; returns an explicit "not yet implemented" error |
| Linux capture (PipeWire/X11) | Backend detection scaffolded; capture not implemented |
| Hardware encoders | Selection layer reports candidates and falls back to software with an explicit reason; driver bindings not implemented |
| Audio over the wire | Room `--audio` streams one capture to all viewers; viewer decodes and plays through cpal |
| NACK retransmission | Implemented end to end over a duplex control stream: viewer detects gaps, presenter resends from a bounded buffer |
| Adaptive bitrate | Closed loop verified: measured decode_ms (132–175 ms, debug) drove High→Medium→Low; encoder rebuilt per tier |
| Consent UI | Gate logic exists; no on-screen prompt |
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









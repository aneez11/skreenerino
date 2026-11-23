# Implementation Status

Last updated: Phases 0 and 1 substantially working on Windows (capture, encode, QUIC, decode, mDNS, live viewer).

## Verified working (tested on this machine)

| Capability | Evidence |
|---|---|
| Workspace builds and tests pass | `cargo test --workspace` (10+ tests) |
| Screen capture (Windows, xcap) | `skreenerino spike` wrote 20 H.264 frames |
| H.264 encode (openh264, screen-content tune) | openh264 encoder in `skreen-codec` |
| H.264 decode | `watch` decoded 60 frames from the stream |
| Frame fragmentation + reassembly | `skreen-proto::fragment` tests + live stream |
| Media datagram framing | `skreen-proto::media` round-trip test |
| QUIC transport with ALPN + self-signed certs | `serve`/`watch` handshake over loopback |
| Live end-to-end stream | `serve` → `watch`: 60 frames decoded, 350 datagrams |
| Room state machine, coordinator handover | `skreen-session` tests |
| TOFU pinning, PIN pairing | `skreen-security` tests |
| Jitter buffer | `skreen-transport` tests |
| CLI | `send`, `join`, `discover`, `spike`, `serve`, `watch` |

Loopback measurement: 60 frames in 28.3 s ≈ 2.1 fps steady state. This is a debug build with per-frame software H.264 encoding of a full desktop; it is correctness evidence, not a latency benchmark.

## Known gaps / next work

| Item | Status |
|---|---|
| `wgpu` viewer window | Implemented: `join`/`watch` open a live wgpu window |
| `send` to a named peer | Works: mDNS resolve by device or room name, plus `host:port` fallback |
| mDNS advertise/browse | Implemented with `mdns-sd`; verified by `discover` and `join` |
| Adaptive bitrate | Tier selector implemented and tested; not yet driven by live reports |
| Consent flow | `ConsentGate` push/pull logic implemented and tested; no UI prompt yet |
| Audio | cpal capture + Opus encode/decode behind `audio` feature; round trip verified (synthetic tone; no mic on dev box) |
| Multi-viewer fan-out | Implemented (`serve --multi`), verified with two concurrent viewers |
| Browser viewer | Implemented (HTTP page + MJPEG gateway), verified end to end |
| Release performance | Measured; software encode is the ceiling. See `docs/benchmarks.md` |

## Build/run notes

- Toolchain is `x86_64-pc-windows-gnu`; MinGW runtime DLLs must be on `PATH` to run the binary. See `README.md`.
- openh264 emits benign warnings for screen-content adaptive-quantization/background-detection; these are auto-disabled.





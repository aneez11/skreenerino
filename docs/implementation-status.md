# Implementation Status

Last updated: Phase 0 spike + Phase 1 transport pipeline working end to end on Windows.

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
| `wgpu` viewer window | Stub; `watch` decodes but discards pixels. Needs a real surface. |
| `send` to a named peer | Requires mDNS browse loop; only `host:port` works today |
| mDNS advertise/browse | Trait + manual endpoint only; `mdns-sd` not wired into a run loop |
| Adaptive bitrate, NACK, FEC | Not implemented |
| Consent prompts / per-window sharing | Types exist; no UI enforcement |
| Audio (cpal + opus) | Feature-gated stub |
| Multi-presenter tiling, SFU/relay | Not started; depends on transport work |
| Mobile/browser targets, packaging/CI | Not started |
| Release performance tuning | Debug build only; needs `--release` benchmark and HW encoder path |

## Build/run notes

- Toolchain is `x86_64-pc-windows-gnu`; MinGW runtime DLLs must be on `PATH` to run the binary. See `README.md`.
- openh264 emits benign warnings for screen-content adaptive-quantization/background-detection; these are auto-disabled.

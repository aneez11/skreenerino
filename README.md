# Skreenerino

LAN screen projection in Rust. See `docs/skreenerino-plan.md` for the full design and `docs/implementation-status.md` for what is currently implemented.

## Layout

```
crates/
  skreen-proto      # control messages, frame headers, fragmentation/reassembly
  skreen-discovery  # mDNS/room advertising trait + manual endpoint fallback
  skreen-capture    # ScreenCapturer trait (per-OS backends pending)
  skreen-codec      # Encoder/Decoder traits (openh264 and HW backends pending)
  skreen-transport  # Transport trait, jitter buffer, MTU helpers (QUIC pending)
  skreen-session    # room membership, roles, presenter handover
  skreen-security   # device identity, TOFU pinning, PIN pairing, consent types
  skreen-render     # wgpu surface (feature-gated, pending)
  skreen-audio      # cpal+opus (feature-gated, pending)
  skreenerino       # CLI/daemon entry point
```

## Build

The toolchain on this machine is `x86_64-pc-windows-gnu`, which needs MinGW `dlltool`/`ld`. A portable MinGW is expected at `.tools/mingw/mingw64` (gitignored). Prepend its `bin` to `PATH`.

```powershell
$env:CARGO_HOME=".cargo-home"
$env:PATH="$PWD\.tools\mingw\mingw64\bin;$env:PATH"
$env:USERPROFILE\.cargo\bin\cargo.exe test --workspace
```

Coverage today is platform-neutral: protocol codec, fragmentation/reassembly, jitter buffer, room state machine, and TOFU/PIN logic. Capture, hardware codecs, QUIC transport, and the wgpu viewer require OS SDKs and are tracked in the status doc.

## Usage

```powershell
# Phase 0 spike: capture 30 frames and write an H.264 elementary stream
skreenerino spike --frames 30 --output capture.h264

# Phase 1: serve the screen, then view it from another machine
skreenerino serve --bind 0.0.0.0:7654 --fps 30 --bitrate 4000000
skreenerino watch 192.168.1.20:7654 --frames 300
# or: skreenerino join 192.168.1.20:7654
```

## Status

Live 1080p-class capture → openh264 → QUIC datagrams → reassembly → decode is verified on Windows. mDNS lookup, an on-screen wgpu viewer, adaptive bitrate, audio, multi-presenter tiling, and mobile/browser targets are still pending. See `docs/implementation-status.md`.


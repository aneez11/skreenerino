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

## Status

- Done: workspace, core traits, protocol + fragment/reassembly, jitter buffer, room/session state, TOFU+PIN security, CLI skeleton, tests.
- Blocked: screen capture backends (Windows.Graphics.Capture/SCK/PipeWire), H.264 hardware encode, QUIC live transport, wgpu viewer, mobile and browser targets, packaging/CI.

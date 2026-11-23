# Skreenerino

LAN screen projection in Rust. Capture a screen on one machine and project it
live to one or many devices on the same network.

See `docs/skreenerino-plan.md` for the design and `docs/implementation-status.md`
for exactly what works today.

## Quick start

```powershell
# terminal 1: share this machine's screen
skreenerino serve --bind 0.0.0.0:7654 --fps 30 --bitrate 4000000 --name desk --room classroom --multi
#   add --scale 0.5 for half resolution, --audio to also send the microphone

# terminal 2 (another machine): watch the live window
skreenerino join classroom

# watch a specific host instead
skreenerino watch 192.168.1.20:7654 --frames 600

# any browser on the LAN
skreenerino web-view --bind 0.0.0.0:7663 --stream 192.168.1.20:7654
```

Other commands:

```powershell
skreenerino discover                        # list peers advertised over mDNS
skreenerino spike --frames 30               # capture -> H.264 file, no network
skreenerino latency --frames 40             # measure encode/decode/token latency
skreenerino relay --bind 0.0.0.0:7662       # forward one presenter to many viewers
skreenerino audio-test --frames 20          # Opus capture/encode/decode round trip
skreenerino soak --minutes 5                # churn/leak test (needs --features soak)
```

## Layout

```
crates/
  skreen-proto      # control messages, media datagrams, fragmentation, latency tokens
  skreen-discovery  # mDNS advertise/browse + manual host:port fallback
  skreen-capture    # ScreenCapturer trait; Windows xcap backend, macOS/Linux scaffolds
  skreen-codec      # Encoder/Decoder traits; openh264 backend
  skreen-transport  # QUIC transport, jitter buffer, ABR tiers, loss tracking, simulation
  skreen-session    # rooms, roles, presenter handover, multi-presenter state
  skreen-security   # device identity, TOFU pinning, PIN pairing, consent gate
  skreen-render     # wgpu viewer window, multi-stream tile layouts
  skreen-audio      # cpal capture + Opus encode/decode
  skreenerino       # CLI/daemon: serve, watch, join, relay, web-view, soak
```

## Building

The dev machine uses `x86_64-pc-windows-gnu`, which needs MinGW `dlltool`/`ld`.
A portable MinGW is expected at `.tools/mingw/mingw64` (gitignored).

```powershell
$env:CARGO_HOME=".cargo-home"
$env:PATH="$PWD\.tools\mingw\mingw64\bin;$env:PATH"
$env:USERPROFILE\.cargo\bin\cargo.exe test --workspace
$env:USERPROFILE\.cargo\bin\cargo.exe build --release -p skreenerino
.\scripts\package.ps1     # bundles binary + runtime DLLs into dist/
```

The audio feature additionally needs `CMAKE_POLICY_VERSION_MINIMUM=3.5` for the
bundled libopus build.

## Status

All roadmap phases have working implementations for the parts that can run on a
Windows host: capture, H.264 encode/decode, QUIC transport, mDNS discovery, live
viewer, multi-viewer fan-out, relay, browser gateway, audio codec, CI, and
packaging. macOS/Linux capture backends, hardware encoders, mobile clients, and
real two-machine LAN validation are the remaining gaps. See
`docs/implementation-status.md`.


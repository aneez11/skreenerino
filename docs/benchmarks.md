# Benchmarks

Measured on the Windows development machine, loopback/offline where noted.
These are correctness-and-baseline numbers, not production targets.

## Environment

- OS: Windows, `x86_64-pc-windows-gnu` toolchain
- MinGW runtime: portable winlibs at `.tools/mingw/mingw64`
- Capture: primary monitor via `xcap`, software H.264 via `openh264` (screen-content tune)
- Encoder bitrate: as noted; the software encoder is the bottleneck, not the network

## Phase 0: capture -> encode -> file

```
skreenerino spike --frames 20 --output spike.h264
```

Debug build: 20 frames, 110439 bytes, 8.86 s (2.3 fps). openh264 warns that
adaptive-quantization and background-detection are unsupported for screen
content and disables them; this is expected.

## Offline latency harness

```
skreenerino latency --frames 40 --bitrate 8000000   # release build
```

| Stage | mean | p95 |
|---|---|---|
| Encode | 26.60 ms | 29.46 ms |
| Decode | 7.31 ms | 7.85 ms |
| Encode+decode round trip (token measured) | 33.9 ms | 37.2 ms |

Debug build for comparison: encode ~137 ms, decode ~122 ms — 8.9 s for 20 frames.
Always benchmark with `--release`.

## End-to-end stream over QUIC

```
skreenerino serve --bind 0.0.0.0:7661 --fps 30 --bitrate 8000000 --multi
skreenerino watch 127.0.0.1:7661 --frames 150
```

Release build, loopback: 150 frames decoded, 414 datagrams, 330881 bytes, 9.79 s,
steady state 15.4 fps. Two concurrent viewers each decoded 30/30 frames in an
earlier debug-build run, confirming fan-out and keyframe-on-join.

## Interpretation

- The software H.264 encoder is the throughput ceiling (about 26 ms/frame of CPU),
  so 1080p30 needs either a faster preset, smaller resolution, or hardware encode.
- Decode is cheap (7 ms) and the QUIC path is not the limiting factor on loopback.
- The <100 ms glass-to-glass target is plausible offline; real LAN latency still
  needs a two-machine measurement with the token harness over the network.

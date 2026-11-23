# Implementation Status

Baseline: workspace scaffolding, platform-neutral logic, and tests are in place and green.

## Done

| Area | Crate | Evidence |
|---|---|---|
| Control messages, capabilities, roles | skreen-proto | `ControlMessage`, `Capabilities` |
| Fragmenter/reassembler | skreen-proto | `fragment.rs`, round-trip test |
| Room membership + coordinator handover | skreen-session | 2 tests passing |
| Presenter/viewer/transfer (baton) | skreen-session | `transfer_presenter` test |
| Jitter buffer | skreen-transport | 3 tests passing |
| TOFU device pinning | skreen-security | `verify_or_pin` test |
| PIN pairing + constant-time compare | skreen-security | 2 tests passing |
| Discovery trait + manual `host:port` fallback | skreen-discovery | parse test |
| MTU-safe payload sizing | skreen-transport | `mtu_safe_payload` |
| CLI skeleton (`send`, `join`, `discover`) | skreenerino | `--help` |

Tests: `cargo test --workspace` passes (10 tests).

## Blocked / pending

| Item | Blocker |
|---|---|
| Screen capture backends | Needs Windows.Graphics.Capture/DXGI, ScreenCaptureKit, PipeWire portals; OS permissions |
| openh264 + HW encoders | Native SDK/binary deps; licensing decision for H.264 |
| Live QUIC transport | Needs end-to-end socket/permission validation on target LANs |
| wgpu viewer | Needs a display/GPU context; currently feature-gated stub |
| Audio (cpal+opus) | Feature-gated stub; phase 2 |
| Phase 3 SFU/relay, simulcast | Depends on live transport |
| Phase 4 mobile/browser | Toolchains absent (Android NDK, Xcode, browser gateway) |
| Phase 5 packaging/CI | Needs release signing, notarization, CI runners |

## Reproduce

```powershell
$env:CARGO_HOME=".cargo-home"
$env:PATH="$PWD\.tools\mingw\mingw64\bin;$env:PATH"
$env:USERPROFILE\.cargo\bin\cargo.exe test --workspace
```

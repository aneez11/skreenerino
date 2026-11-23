# Skreenerino GUI Plan — Windows 11, Zoom-style

Goal: a desktop app that feels like a Windows 11 Fluent application (Mica backdrop,
rounded corners, Segoe UI Variable, light/dark following the system) with a Zoom-like
flow: a home screen, a meeting view with a video grid and a control bar, a share
picker, participants panel, and pairing/consent dialogs.

Status today: no application GUI. Only a bare winit+winit video window
(`skreen-render/src/wgpu_viewer.rs`) and a browser MJPEG page. All configuration is CLI flags.

## 1. Dependency decision

Use **egui 0.31.1** (immediate mode), not Tauri/eframe, because it must share the
window, the `wgpu::Device`, and the existing render loop that already draws video.

Why 0.31.1 and not latest 0.37: our lockfile pins `wgpu 24.0.5` / `winit 0.30.13`, and
`egui-wgpu 0.31.1` requires exactly `wgpu 24` / `winit 0.30.7+`. Upgrading to egui 0.37
would force a simultaneous wgpu 25 jump and re-verify the whole video pipeline. Do the
GUI first on the matched set; treat a later wgpu/egui bump as separate work.

| Crate | Version | Purpose |
|---|---|---|
| `egui` | 0.31.1 | widgets, layout, theming |
| `egui-wgpu` | 0.31.1 | tessellation → wgpu render pass; texture registry |
| `egui-winit` | 0.31.1 | input translation, clipboard, IME |
| `windows` | 0.62 | DWM backdrop, corner preference, dark mode, build check |
| `egui-phosphor` (optional) | matched | icon glyphs for the control bar |

New crate: **`crates/skreen-ui`** (feature `gui`), so the CLI binary keeps working
headless on servers and in CI.

## 2. Required refactor before UI work

`crates/skreenerino/src/main.rs` is 950 lines and owns the orchestration (presenter
thread, room accept loop, viewer client, audio, NACK/ABR control loop). A GUI cannot
call functions buried in a `main.rs`.

**Extract to `crates/skreen-app`** (library, no egui):

- `Presenter` — start/stop a room; owns the capture+encode thread and `RoomShared`.
- `Viewer` — connect, decode, emit `RgbaFrame` into a channel; owns control stream.
- `Discovery` — long-lived mDNS browse returning a peer list stream.
- `AppEvent` enum — what the UI observes: `PeerFound`, `Frame{stream}`, `PeerLeft`,
  `ConsentRequest`, `PinRequired`, `Stats{...}`, `AudioLevel(f32)`, `Error(String)`.
- `Command` enum — what the UI can ask: `StartShare{..}`, `StopShare`, `Join{..}`, `Leave`,
  `SetMuted`, `SetScale`, `AnswerConsent`, `RequestKeyframe`.

The CLI becomes a thin adapter over `skreen-app` (same flags, same behaviour), so
every command keeps working and CI keeps testing the same logic.

## 3. Threading model (non-negotiable)

Established constraint in this repo: software H.264 on the async runtime starves the
QUIC driver and connections time out. Preserve that:

```
main thread        winit event loop → egui → wgpu submit        (never blocks)
  ▲ channels/atomics
UI bridge thread   drains AppEvent → egui repaint requests
  ▲
tokio runtime      QUIC, mDNS, control streams, NACK/ABR        (background thread)
  ▲
capture thread     xcap → scale → openh264                      (blocking, dedicated)
audio thread       cpal → opus                                  (blocking, dedicated)
```

Rules: the UI thread never calls `block_on`; frames arrive as `Arc<RgbaFrame>` through
a bounded channel and are uploaded to an egui texture once per repaint, coalescing
drops (`try_send`, newest wins) so a slow UI cannot back-pressure the encoder.

## 4. Windows 11 look

Applied once at window creation (`skreen-ui/src/win11.rs`):

- `DwmSetWindowAttribute(DWMWA_SYSTEMBACKDROP_TYPE = 38, DWMSBT_MAINWINDOW = 2)` for Mica.
- `DWMWA_WINDOW_CORNER_PREFERENCE = 33, DWMWCP_ROUND = 2` for rounded corners.
- `DWMWA_USE_IMMERSIVE_DARK_MODE = 20` following the system theme
  (`AppsUseLightTheme` registry value), re-applied on theme change.
- Requires Windows 11 build ≥ 22000; backdrop needs ≥ 22621. Detect via
  `RtlGetVersion`; on failure fall back to a solid Fluent background and log why.
- Mica needs an alpha-capable swapchain, so configure `alpha_mode = PreMultiplied`
  (first supported) and clear with alpha 0. If no alpha mode is advertised, keep the
  opaque clear colour instead of showing garbage.

Fluent tokens (`skreen-ui/src/theme.rs`), light/dark pairs:

| Token | Light | Dark |
|---|---|---|
| surface / layer | `#F3F3F3` / `#FFFFFF` | `#202020` / `#2B2B2B` |
| stroke (1px) | `#E5E5E5` | `#3A3A3A` |
| accent | `#0F6CBD` | `#4CC2FF` |
| text primary / secondary | `#1A1A1A` / `#616161` | `#FFFFFF` / `#A0A0A0` |
| danger (leave) | `#C42B1C` | `#FF99A4` |

Style: Segoe UI Variable loaded from `C:\Windows\Fonts` (fallback to egui default),
corner radius 8, control height 32, 11px body / 20px title, 150 ms hover fades.

## 5. Screens

**Home** — 920×600 on launch, Zoom-like cards in a left rail plus a large hero:

- Buttons: *Share my screen* (accent), *Join a room*, *Pair a device*.
- Nearby devices list from mDNS, each row: name, room, one-click *Join*; refresh + spinner.
- Manual `host:port` entry with inline validation (mDNS-blocked networks).
- Recent rooms (last 5) persisted next to `trusted_devices.json`.

**Share picker** — modal: monitor thumbnails (live, downscaled), quality preset
(FPS 15/30/60, scale 1.0/0.75/0.5), audio toggle, then *Share*.

**Meeting** — the core Zoom-like view:

- Centre: video grid via `skreen-render::layout::TileLayout` (single / grid / PiP).
  Each stream is an egui texture; the active speaker is outlined in accent.
- Right panel (collapsible): participants with role badges (Presenter/Viewer/Baton),
  host of each stream, per-peer RTT/loss from quality reports.
- Bottom control bar, 56 px, centred pill: mic mute, camera/share toggle, participants,
  settings, and a red *Leave* that ends the share or disconnects.
- Top bar: room name, viewer count, connection quality dot, elapsed time.
- Status strip: encoder, current bitrate/tier, fps, encode ms, dropped frames.

**Dialogs** — consent (Accept / Reject / Always allow for this device), PIN pairing
(six digits, large monospace, copy + regenerate), error toast (Fluent inline, not a
native message box).

**Tray + closing** — closing the window while sharing shows *Stop sharing and exit /
Keep sharing in tray / Cancel*; the tray icon mirrors the Zoom behaviour.

## 6. Milestones

| # | Deliverable | Verification |
|---|---|---|
| M0 | `egui` running inside the existing viewer window; video drawn as an egui texture; theme skeleton | Window opens, video renders, ~no fps regression vs `watch` |
| M1 | Extract `skreen-app`; CLI reimplemented on top; `cargo test --workspace` still green | All existing CLI commands behave identically |
| M2 | Home screen: mDNS peer list, manual entry, recent rooms, Start/Join wiring | Click-to-join a real `serve --multi` on the same machine |
| M3 | Meeting view: grid, control bar, stats, right panel, Leave | Two streams tiled; mute/share/leave all act |
| M4 | Windows 11 polish: Mica/rounded/dark mode, Segoe UI Variable, Fluent tokens, icons | Visual check on Win11; graceful fallback on Win10 |
| M5 | Share picker, consent + PIN dialogs, tray-on-close, settings persisted | Consent gate and PIN flow exercised end to end |

Ship the GUI behind `--features gui`; keep the CLI-only build as the default so
headless and CI builds stay light.

## 7. Testing

- **Logic**: `skreen-app` gets unit tests for command/event transitions (join, leave,
  share start/stop, consent accept/reject) — no GPU needed.
- **UI**: `egui_kittest 0.31.1` to drive the widget tree headlessly and assert state
  (button clicked → `Command::StartShare` emitted, PIN dialog validates 6 digits).
  Optional `snapshot` feature for PNG diffs, gated to CI on Windows only.
- **Regression**: keep the existing 61 tests green; add a bench comparing fps in the
  windowed path before/after overlay (M0 exit criterion).
- **Manual matrix**: Win11 light/dark, Win10 fallback, 125%/150% DPI, single and dual
  monitor, mic absent (this dev box), mDNS blocked via manual IP.

## 8. Risks

| Risk | Mitigation |
|---|---|
| egui overlay costs fps on this software-encode path | M0 measures it; fall back to a wgpu custom callback drawing video directly, with egui drawn after |
| Mica needs an alpha swapchain that some drivers refuse | Detect supported alpha modes; fall back to opaque Fluent surface, no visual break |
| `windows` crate version skew | Confine to one small `win11.rs`; everything else stays portable |
| Refactor breaks CLI behaviour | M1 lands before any UI; CLI is the test harness for `skreen-app` |
| Threading regression (UI blocking QUIC) | UI never `block_on`; frames use bounded `try_send`; soak test re-run after M3 |
| Scope creep into Zoom's full feature set | v1 GUI exposes only what exists: share, join, grid, participants, consent, PIN |

## 9. Order of work

1. M0 spike on a branch: prove egui + video texture in one window, measure fps.
2. M1 refactor to `skreen-app`, CLI unchanged, tests green, push.
3. M2 home screen, then M3 meeting view.
4. M4 Fluent/Win11 polish, then M5 dialogs and tray.

# AGENTS

Project: Skreenerino (`D:\Jobs\PErsonal\skreenerino`)

## Intent
- Implement `docs/skreenerino-plan.md` iteratively; status lives in `docs/implementation-status.md`.
- Keep platform-specific logic behind traits in `skreen-capture`, `skreen-codec`, `skreen-render`.

## Environment
- Toolchain is `x86_64-pc-windows-gnu` at `$env:USERPROFILE\.cargo\bin\cargo.exe`; it is NOT on PATH.
- MinGW runtime/linker must be installed project-locally at `.tools/mingw/mingw64` (gitignored). Prepend its `bin` to `PATH` or the linker fails with `cannot find dllcrt2.o` / `-lkernel32`.
- Running the built `.exe` also requires those MinGW `bin` DLLs on `PATH` (exit code -1073741515 without them).

## Commands
```powershell
$env:CARGO_HOME="D:\Jobs\PErsonal\skreenerino\.cargo-home"
$env:PATH="D:\Jobs\PErsonal\skreenerino\.tools\mingw\mingw64\bin;$env:PATH"
$env:CMAKE_POLICY_VERSION_MINIMUM="3.5"   # only for the audio feature (bundled libopus)
& "$env:USERPROFILE\.cargo\bin\cargo.exe" test --workspace
& "$env:USERPROFILE\.cargo\bin\cargo.exe" build --release -p skreenerino
```
- Build target is `.cargo-target` (set in `.cargo/config.toml`).
- `scripts/package.ps1` produces `dist/` with the binary and runtime DLLs.

## Conventions
- Workspace crates live under `crates/`; heavy native deps stay behind cargo features
  (`xcap`, `h264`, `quic`, `mdns`, `viewer`, `audio`, `soak`).
- Do not commit `.cargo-home/`, `.cargo-target/`, `.tools/`, `dist/`, or `.codebase-memory/`.
- Keep the software H.264 encoder on a dedicated thread; running capture+encode on the
  async runtime starves the QUIC driver and connections time out.
- Add tests for protocol, fragmentation, room state, transport buffering, and security.

## Known constraints
- No second machine available: LAN latency and cross-host mDNS are unverified.
- No microphone on the dev box; the audio test falls back to a synthetic tone.
- macOS/Linux capture, hardware encoders, and mobile clients are unimplemented by design.
- openh264 warns that adaptive-quantization/background-detection are unsupported for
  screen content; it disables them automatically, which is expected.

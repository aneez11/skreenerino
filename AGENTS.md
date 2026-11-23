# AGENTS

Project: Skreenerino (`D:\Jobs\PErsonal\skreenerino`)

## Intent
- Implement the plan in `docs/skreenerino-plan.md` iteratively.
- Prefer small, testable Rust workspace increments. Keep platform-specific logic behind traits.

## Commands
- Use project-local Cargo dirs: `CARGO_HOME=.cargo-home`, build target in `.cargo-target` (see `.cargo/config.toml`).
- Fetch deps: `CARGO_HOME=.cargo-home cargo fetch`
- Check/build: `CARGO_HOME=.cargo-home cargo check`
- Run CLI: `CARGO_HOME=.cargo-home cargo run -p skreenerino -- --help`

## Conventions
- Workspace crates live under `crates/`.
- Do not commit secrets. Prefer explicit errors over silent fallback.
- Add tests for protocol, fragment/reassembly, room state, and transport behavior as they are implemented.

## Current blockers to record
- Rust toolchain exists on machine but was not initially on PATH; use `$env:USERPROFILE\.cargo\bin\cargo.exe` or ensure PATH.
- Full implementation depends on OS capture/HW codec SDK availability and network/device permissions; mark blocked phases clearly and continue non-blocking work.

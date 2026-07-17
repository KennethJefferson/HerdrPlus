# HerdrPlus

A Rust-based terminal workspace manager for AI coding agents — a Windows-first extension of [herdr](https://github.com/ogulcancelik/herdr).

Upstream herdr is an agent multiplexer that runs Claude Code, Codex, Copilot CLI, and 15+ other AI coding agents in a single terminal with real-time state tracking. Windows support upstream is beta; HerdrPlus exists to build, harden, and extend herdr on Windows.

## Status

- Baseline: upstream herdr v0.7.4 (`master`) builds clean on Windows 11 (x86_64-pc-windows-msvc)
- Binary output: `__solutions/target/release/herdr.exe`

## Prerequisites

| Tool | Version | Notes |
|---|---|---|
| Rust | 1.96.1 | Pinned by `rust-toolchain.toml`; rustup auto-installs |
| Zig | 0.15.2 | Compiles the vendored `libghostty-vt`; exact version matters |
| VS Build Tools | 2022+ | MSVC linker (any Visual Studio with the VC x64 workload) |

## Build

```powershell
cd __references/herdr
cargo build --release
```

That's it. `.cargo/config.toml` at the repo root handles everything else:
- Redirects all build output to `__solutions/target/`
- Sets `ZIG` and `ZIG_GLOBAL_CACHE_DIR` (the cache **must** live on the same drive as the repo — Zig panics on cross-drive relative paths on Windows)

## Layout

```
__references/herdr/   upstream herdr clone (reference base, not committed)
__solutions/          all build output (target dir lives here)
__research/           notes and investigation artifacts
.cargo/config.toml    build configuration (target-dir + Zig env)
```

## License

Upstream herdr is AGPL-3.0-or-later. Anything in this repo derived from herdr source carries the same license.

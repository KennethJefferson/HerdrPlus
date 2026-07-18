# HerdrPlus — project instructions

HerdrPlus is a Windows-first extension of upstream [herdr](https://github.com/ogulcancelik/herdr) (Rust agent terminal multiplexer, AGPL-3.0).

## Layout

- `__references/herdr/` — vendored upstream herdr source (v0.7.4 base, `master` @ `0409565`). This is the working codebase, committed to this repo. The nested `.git` was removed when vendoring; to diff against or update from upstream, clone https://github.com/ogulcancelik/herdr separately and compare.
- `__solutions/` — ALL build output goes here. Cargo's target dir is `__solutions/target/`; the release binary is `__solutions/target/release/herdrplus.exe` (bin target renamed via `[[bin]]` in Cargo.toml; package name stays `herdr`, so test commands are `cargo test --bin herdrplus`).
- `__research/` — investigation notes and artifacts.
- `.cargo/config.toml` — ancestor cargo config; applies to every cargo command run inside this tree. Do not delete it.

## Build

```powershell
cd __references/herdr
cargo build --release
```

No env setup needed — `.cargo/config.toml` sets `ZIG` (C:/Zig/zig.exe, 0.15.2) and `ZIG_GLOBAL_CACHE_DIR` and redirects `target-dir`.

### Hard-won constraints (do not regress)

- `build.rs` compiles vendored `libghostty-vt` via `zig build`; Zig 0.15.2 exactly (min pin, and 0.16 may break the 0.15-era build script).
- `ZIG_GLOBAL_CACHE_DIR` must be on the K: drive (same drive as the repo). Zig on Windows panics (`Run.zig:662 assert(!std.fs.path.isAbsolute(...))`) when it cannot relativize cross-drive paths.
- Toolchain pinned to Rust 1.96.1 via `rust-toolchain.toml` (MSVC target).

## Conventions

- `Changelog.md` is append-only, Keep a Changelog format. Add an entry for every meaningful change; never rewrite past entries.
- We only build for Windows (`x86_64-pc-windows-msvc`, the host default). No cross-compilation.
- Upstream docs live in `__references/herdr/docs/`; upstream contributor guidance in `__references/herdr/AGENTS.md`.

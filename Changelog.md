# Changelog

All notable changes to HerdrPlus will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

This file is append-only: new entries go on top, existing entries are never rewritten.

## [0.1.1] - 2026-07-17

### Changed

- Vendored the upstream herdr source (`master` @ `040956531f673c8fb7720037494ed4e61b123c6c`) into the repository at `__references/herdr/` — previously gitignored as an external clone. The nested `.git` was removed; upstream provenance is recorded here and in `CLAUDE.md`.

## [0.1.0] - 2026-07-17

### Added

- Cloned upstream [herdr](https://github.com/ogulcancelik/herdr) v0.7.4 (`master` @ `0409565`) into `__references/herdr/` as the reference codebase.
- Working Windows release build (`x86_64-pc-windows-msvc`, Rust 1.96.1, Zig 0.15.2, VS 2026 MSVC linker) producing `__solutions/target/release/herdr.exe` (~17 MB, verified `herdr 0.7.4`).
- `.cargo/config.toml` build configuration: redirects `target-dir` to `__solutions/target/` and sets `ZIG` / `ZIG_GLOBAL_CACHE_DIR` so a plain `cargo build --release` works with no shell setup.
- Project documentation: `README.md`, `CLAUDE.md`, `Usage.md`, this changelog.

### Fixed

- Zig build panic on Windows (`Run.zig:662 assert(!std.fs.path.isAbsolute(...))`) when compiling vendored `libghostty-vt`: Zig's default global cache on `C:` cannot be relativized against the repo on `K:`. Resolved by pinning `ZIG_GLOBAL_CACHE_DIR` to the repo drive.

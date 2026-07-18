# Changelog

All notable changes to HerdrPlus will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

This file is append-only: new entries go on top, existing entries are never rewritten.

## [0.3.1] - 2026-07-18

### Fixed

- Msg-bus state (inbox + group membership) is now torn down on every pane-removal path — process death (`handle_pane_died`), tab close, workspace close, and layout-apply tab replacement/rollback — not just API `pane.close`. Dead panes no longer leak inboxes or linger as `@group` members.
- `msg wait` missed-wakeup race: the CLI now subscribes to `MsgReceived` before taking the baseline `msg.list`; if unread messages already exist at baseline they are printed immediately without blocking.
- All `msg.*` handlers (`list`, `ack`, `group join/leave`, `send` sender) canonicalize the pane id via the public-id path, so alias forms (`p_N`, post-restore aliases) address the same inbox and group membership as the canonical id; stored `from_pane_id` and `@all` sender exclusion also use the canonical id.
- `msg.send` failures now return distinct wire error codes per the protocol-18 spec — `pane_not_found`, `unknown_target`, `ambiguous_target` (candidates formatted as `workspace_id/label (pane_id)`), `empty_group`, `unaddressable_label` — instead of a collapsed `invalid_target`.

## [0.3.0] - 2026-07-18

### Added

- Inter-pane messaging ("msg") bus: per-pane inboxes, label-based addressing, group support (`@devs`, `@all`), subscription-based waiting, and a CLI subcommand interface (`herdrplus msg`).
- Visible unread message indicator (✉ <count>) on the TUI pane border title when unread messages are present.
- Protocol bumped 17 → 18. Design: `docs/superpowers/specs/2026-07-18-pane-comms-design.md`.

## [0.2.1] - 2026-07-18

### Changed

- Binary renamed `herdr.exe` → `herdrplus.exe` via a `[[bin]]` target in Cargo.toml (package name stays `herdr` for upstream mergeability). Test references (`CARGO_BIN_EXE_herdr`, 43 sites) and the justfile clippy target updated to match. Cargo test invocations are now `cargo test --bin herdrplus`.

## [0.2.0] - 2026-07-17

### Added

- `herdr pane balance [--tab ID|--pane ID|--current]` — equalize all pane areas in a tab (equal ideal area, split tree preserved). New `layout.balance` socket API method and `prefix+=` TUI keybinding (Vim `Ctrl-W =` muscle memory). Protocol bumped 16 → 17. Design: `docs/superpowers/specs/2026-07-17-pane-balance-design.md`.

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

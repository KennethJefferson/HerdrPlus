# Changelog

All notable changes to HerdrPlus will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

This file is append-only: new entries go on top, existing entries are never rewritten.

## [0.4.1] - 2026-07-19

### Fixed

- Windows pane-kill was a silent no-op: `signal_processes` opened target processes with only `PROCESS_QUERY_LIMITED_INFORMATION`, so every `TerminateProcess` call failed with access denied. Pane teardown only worked when ConPTY closure happened to take the shell down; when that raced shell startup, the shell leaked and `child.wait()` blocked forever — the root cause of the long-standing flaky suite hangs (a pane-spawning test's tokio runtime drop then joined the blocking pool indefinitely). Fixed by opening with `PROCESS_TERMINATE`; regression-tested by killing a real process (`windows_signal_processes_kill_terminates_target`), and the previously ~50%-hanging `deferred_api_worktree_create_completes_after_source_workspace_changes` now passes 10/10.

## [0.4.0] - 2026-07-18

### Added

- `herdrplus team spawn <name> --agents <entry>[,...]` — one-shot team creation: workspace + N labeled agent panes + agent CLI launched in each + all panes joined to msg group `<name>`; balanced grid layout; label→pane map returned as JSON. Optional `--with-orch [cmd]` orchestrator pane, `--cwd` (defaults to caller's cwd), `--wait [--timeout secs]` readiness polling (ready = agent recognized OR agent state detected; exit 3 on timeout with team still up). Server-native wire method `team.spawn` composed from the existing workspace/pane/msg/layout handlers; rollback on partial failure closes the workspace through the hardened msg-teardown cascade and restores the caller's focus. Validation: reserved team name `all`, pane-id-shaped labels rejected, labels trimmed, duplicate labels refused, existing group names refused (`team_exists`), 24-pane cap (`team_too_large`).
- `[team.agents]` config section: agent name → launch command registry (e.g. `claude = "claude --dangerously-skip-permissions"`); unknown roster names pass through verbatim as commands. Live-reloadable.
- Roster grammar: `label=agent` only when the label is a simple identifier (`[A-Za-z0-9_-]+`); anything else is treated as a verbatim command, so `claude --model=x` launches as a command rather than misparsing as a label.

### Changed

- Protocol version 18 → 19 (strict-equality gate; schema artifact regenerated).

### Provenance

- Design + implementation reviewed externally (GPT-5.6 Sol, high effort): 5 major spec findings folded in pre-merge (readiness signal, reserved/shadowed names, cwd ownership, rollback focus restoration + documented atomicity contract, passthrough grammar).

## [0.3.2] - 2026-07-18

### Fixed

- Closing a worktree group (parent + linked worktree workspaces) now purges msg-bus state for panes in EVERY closed workspace, not just the requested one — `close_selected_workspace` owns the teardown for all close paths (TUI, API `workspace.close`, worktree removal, last-pane cascade).
- Cross-workspace `pane.move` re-keys msg-bus state to the pane's new canonical public id: the inbox travels intact (seq/ack/dropped counters preserved) and group membership follows; nothing is stranded under the old id.
- `msg wait` no longer misses messages when a large `@all`/group broadcast evicts this pane's `MsgReceived` event from the 512-entry event ring: the subscription is now only a wake hint — the CLI re-lists the inbox on every wake or 250ms poll tick, with `--timeout` still bounding total wait.
- `msg read --after SEQ` no longer over-acks: it is peek-like (never auto-acks), since acking the highest displayed seq would silently mark skipped unread messages as read. Plain `msg read` still auto-acks the highest displayed seq.
- `msg.send` with a PROVIDED sender pane id that does not resolve to a live pane (blank, malformed, closed) now returns `pane_not_found` instead of storing the bogus id verbatim; only an absent sender id means external identity.
- Msg targets containing `:` are only treated as pane ids when they match the canonical public-pane-id grammar (`w<ENC>:p<ENC>`); labels like `worker:api` now route as labels.
- `msg.send` validates the 64 KiB body cap before resolution, so an empty fan-out (e.g. `@all` excluding the only pane) can no longer "accept" an oversized body; zero-recipient sends return an empty `delivered_to` with a `null` `message` instead of a bogus `seq: 0` message object (schema: `msg_send.message` is now nullable).

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

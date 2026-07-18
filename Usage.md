# Usage

## Run

```powershell
K:\Downloads\__Projects.Mine\herdr4Windows\__solutions\target\release\herdrplus.exe
```

herdr is a full-screen TUI — run it in Windows Terminal (or any modern terminal), not through a pipe.

Common commands:

```powershell
herdrplus.exe --version    # print version
herdrplus.exe --help       # CLI options
herdrplus.exe pane balance      # equalize pane areas (ideal, pre-rounding) in the current tab (TUI: prefix+=)
```

## Inter-Pane Messaging ("msg")

herdrplus supports inter-pane messaging without corrupting terminal input.

```powershell
herdrplus.exe msg send <target> <text>          # Send a message to a pane or group
herdrplus.exe msg read [--all] [--after SEQ] [--pane ID]  # Read and acknowledge messages
herdrplus.exe msg peek [--all] [--after SEQ] [--pane ID]  # Read without acknowledging
herdrplus.exe msg ack <up-to-seq> [--pane ID]   # Acknowledge messages up to SEQ
herdrplus.exe msg wait [--timeout MS] [--pane ID] # Wait for and print new messages
herdrplus.exe msg group join|leave <name> [--pane ID]  # Join or leave a group
herdrplus.exe msg who                           # Show messaging directory
```

Notes:

- `msg read --after SEQ` is peek-like: it never auto-acknowledges, because `--after` can skip unread messages and acking past them would silently mark them read. Acknowledge explicitly with `msg ack` after processing. A plain `msg read` (no `--after`) still auto-acks the highest displayed seq.
- Targets: only the canonical public pane id form (`w1:p3`) is treated as a pane id; labels may contain `:` (e.g. `worker:api`) and are routed as labels.
- A `msg send` whose fan-out resolves to zero recipients (e.g. `@all` from the only pane) succeeds with an empty `delivered_to` and a `null` `message` — nothing is delivered and no seq is assigned.

## What it does

herdr multiplexes AI coding agents (Claude Code, Codex, Copilot CLI, Cursor Agent, and more) in one terminal:

- Every agent at a glance — blocked, working, done
- Detach and reattach from any terminal; sessions survive restarts
- tmux-style prefix keys plus first-class mouse support (click, drag, split)

See upstream docs for keybindings and configuration: `__references/herdr/docs/` or <https://herdr.dev>.

## Rebuild after changes

```powershell
cd __references/herdr
cargo build --release
```

Output lands in `__solutions/target/release/herdrplus.exe`. See `CLAUDE.md` for build constraints (Zig 0.15.2, same-drive cache).

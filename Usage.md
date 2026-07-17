# Usage

## Run

```powershell
K:\Downloads\__Projects.Mine\herdr4Windows\__solutions\target\release\herdr.exe
```

herdr is a full-screen TUI — run it in Windows Terminal (or any modern terminal), not through a pipe.

Common commands:

```powershell
herdr.exe --version    # print version
herdr.exe --help       # CLI options
```

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

Output lands in `__solutions/target/release/herdr.exe`. See `CLAUDE.md` for build constraints (Zig 0.15.2, same-drive cache).

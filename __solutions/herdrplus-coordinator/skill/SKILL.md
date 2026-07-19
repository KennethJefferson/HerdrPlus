---
name: herdrplus-coordinator
description: "Coordinate HerdrPlus (the Windows-first herdr fork, herdrplus.exe) from outside or inside the app: open/close/maximize its window, stop the lingering server and reclaim RAM, create/close workspaces, tabs, and panes, manage pane layout (split, balance, zoom, resize, swap, move), launch and stop commands in panes, drive agent CLIs in panes, use the msg bus (send/read/group), and spawn agent teams. Use this skill whenever the user mentions herdr, herdrplus, HerdrPlus panes/tabs/workspaces, spawning or messaging agents in herdr, cleaning up herdr memory/RAM/processes, or orchestrating work across herdr panes — even if they only say 'open herdr', 'kill herdr', 'spawn a team', or 'message the worker pane'."
---

# HerdrPlus Coordinator

HerdrPlus is Tony's Windows-first fork of herdr — a terminal workspace manager for AI coding agents. It has a client/server design: the TUI window is just a client; a persistent session **server keeps running (and holding RAM) after the window closes**. This skill covers driving the whole app: window lifecycle, server/RAM cleanup, workspaces/tabs/panes, running commands, agent orchestration, the msg bus, and team spawn.

Unlike upstream herdr's own skill (which refuses to run outside a herdr pane), this coordinator is *designed* to control the local session from outside — that is its job. Both modes work:

- **External coordinator** (normal): you are a terminal/agent outside herdr, managing the app and its contents.
- **Inside a pane** (`HERDR_ENV=1` is set): you are one of the panes. Use `--current` / `$env:HERDR_PANE_ID` to refer to yourself, and never stop the server you are living in.

## Locate the binary

Resolve in this order and reuse the result for every call:

1. `$env:HERDRPLUS_EXE` if set
2. `herdrplus` on PATH (`Get-Command herdrplus`)
3. `K:\Downloads\__Projects.Mine\herdr4Windows\__solutions\target\release\herdrplus.exe` (the built release binary; canonical on this machine)

The package/help text says `herdr` but the binary is `herdrplus.exe`. Nearly every control command prints JSON — parse it; never predict IDs. (Exception: `pane read` prints raw pane text, not JSON.)

Bundled helper scripts (in this skill's `scripts/` dir) wrap the fiddly Win32 parts:

- `scripts/herdr-window.ps1 -Action open|close|maximize|minimize|restore|focus|status [-Session <name>]` — window lifecycle via a named Windows Terminal window with a pinned title.
- `scripts/herdr-cleanup.ps1 [-Force] [-DryRun]` — graceful server shutdown + leftover process kill + RAM report.

Run them with `pwsh -File <script> ...`. Prefer the scripts over hand-rolling Win32 calls.

## App and window lifecycle

**Open / attach:** `herdr-window.ps1 -Action open` launches the client in a dedicated Windows Terminal window (`wt -w herdrplus-<session>`) with a suppressed tab title `HerdrPlus[<session>]`, maximized. If the window already exists it just focuses it. The client attaches to the running server or starts one.

**Maximize / minimize / restore / focus / close:** same script, respective `-Action`. `close` sends WM_CLOSE to the window — this **detaches the client only; the server and every pane process keep running**. That is by design (persistence), and it is also why RAM lingers after "closing herdr".

**Named sessions:** `herdrplus --session <name>` runs an isolated server with its own socket. To aim CLI commands at a non-default session, set `$env:HERDR_SESSION = '<name>'` (simplest — works for starting `herdrplus server` and for every subcommand), or set `$env:HERDR_SOCKET_PATH` to the session's `socket_path` from `session list --json`. Use a named session (e.g. `test`) for experiments so you never disturb the real workspace.

**Headless:** you do not need a window at all. If no server is running, `herdrplus server` (background) starts one headless; every CLI command below works against it. Open a window later to look at it.

**Status:** `herdrplus status --json` → client version/protocol + whether the server is running and on which socket.

## RAM / process cleanup

The server (plus conpty hosts and every pane's child processes) survives window close. To actually reclaim memory:

```powershell
pwsh -File scripts/herdr-cleanup.ps1 -DryRun   # inventory: processes, RAM, live agents
pwsh -File scripts/herdr-cleanup.ps1           # graceful: session stop / server stop, then kill stragglers
pwsh -File scripts/herdr-cleanup.ps1 -Force    # proceed even when live agent panes were detected
```

The script refuses (without `-Force`) when it detects agents in panes, because stopping the server kills every pane process — do not silently destroy the user's running agents. Report what it found and ask, unless the user already said "kill it all".

Manual equivalent: `herdrplus session stop <name>` per running session (`server stop` for the default), wait ~3s, then `Get-Process herdr* | Stop-Process -Force` for anything left. Note: `session stop` can report a 15s timeout error even though the stop landed — re-check the process list before concluding it failed or force-killing.

Cleanup is also the **prerequisite for rebuilding**: a running herdrplus.exe locks the binary and a "successful" rebuild silently leaves the old exe in place.

## Discovering state

```powershell
& $herdr status --json          # server up?
& $herdr workspace list         # workspaces
& $herdr tab list --workspace w1
& $herdr pane list --workspace w1
& $herdr agent list             # detected agents across panes
& $herdr api snapshot           # entire runtime state in one JSON blob
```

IDs are short opaque handles (`w1`, `w1:t2`, `w1:p3`, `term_...`). Closed IDs are never reused; a moved pane gets a **new** pane ID. Always re-read IDs from the JSON response of create/split/move/list — never construct one.

## Workspaces and tabs

```powershell
& $herdr workspace create --cwd K:\proj --label build --no-focus   # → workspace_id
& $herdr workspace focus w2
& $herdr workspace rename w2 "release"
& $herdr workspace close w2            # closes all its tabs/panes/processes

& $herdr tab create --workspace w1 --cwd K:\proj --label tests --no-focus
& $herdr tab focus w1:t2
& $herdr tab close w1:t2
```

Use `--no-focus` for background work so the user's view doesn't jump. Only close things you created, unless explicitly asked.

## Panes and layout

```powershell
& $herdr pane split w1:p1 --direction right --ratio 0.5 --no-focus  # → new pane_id in JSON
& $herdr pane balance --tab w1:t1     # HerdrPlus: equal-area layout reset (TUI: prefix+=)
& $herdr pane zoom w1:p2 --toggle     # fullscreen one pane ("maximize a pane")
& $herdr pane resize --pane w1:p2 --direction right --amount 0.1
& $herdr pane swap --source-pane w1:p1 --target-pane w1:p3
& $herdr pane move w1:p2 --new-tab --label logs --no-focus
& $herdr pane rename w1:p2 "builder"
& $herdr pane close w1:p2             # kills the pane's process tree
```

Geometry rule: split a wide pane `right`, a narrow/tall pane `down` (check `pane layout`). After spawning several panes, run `pane balance` once instead of hand-tuning ratios. For an N-pane grid: split right (N columns) then split each down, or just use `team spawn` (below) which balances for you.

## Running and stopping commands

```powershell
& $herdr pane run w1:p2 "cargo test --bin herdrplus msg"   # text + Enter, atomically
& $herdr wait output w1:p2 --match "test result" --timeout 120000   # exit 1 on timeout
& $herdr pane read w1:p2 --source recent-unwrapped --lines 120
& $herdr pane send-keys w1:p2 ctrl+c    # stop the running command, keep the pane
& $herdr pane send-keys w1:p2 Escape    # cancel/unstick a TUI agent's current action
```

- `pane run` = submit a command or chat message with Enter. `send-text` writes literal text without Enter; `send-keys` sends named keys (`Enter`, `Escape`, `ctrl+c`, arrows).
- Read sources: `visible` (viewport), `recent` (scrollback as rendered), `recent-unwrapped` (best for logs/transcripts).
- **Inspect before waiting**: `pane read` first — `wait output` can false-match text already in scrollback, and stale matches have burned us before. Prefer waiting on ground truth (a file appearing, a git commit) over screen text for anything important.

## Driving agent CLIs in panes

Launch interactively, wait for the prompt, then submit the task:

```powershell
& $herdr pane run <pane> "claude"                # or codex / omp / agy / grok / pi
& $herdr wait agent-status <pane> --status idle --timeout 30000
& $herdr pane run <pane> "Fix the failing msg tests; report in task-1-report.md"
& $herdr wait agent-status <pane> --status done --timeout 600000
```

Agent status: `idle`/`done` both mean "finished" (`done` = result unseen in a background tab), `working`, `blocked` (needs input), `unknown` (no agent detected). `herdr agent <list|get|read|send|wait|focus>` addresses agents by label instead of pane ID.

Hard-won rules for this machine — the difference between orchestration that works and one that silently stalls — are in **`references/orchestration-playbook.md`. Read it before driving more than one agent.** Highlights: `pane run` messages are LOST if the agent is mid-turn (always verify receipt and nudge with `send-keys Enter`); agents stop at turn boundaries even in auto-approve mode and must be prodded; `agy --continue` resumes its previous conversation (prefix reassignments with "IGNORE ALL PRIOR CONVERSATION CONTEXT"); monitor via report files + `git diff`, not screen-scraping.

## Msg bus (HerdrPlus)

Server-native per-pane inboxes with label/group addressing:

```powershell
& $herdr msg send builder "run the tests now"    # to pane label
& $herdr msg send "@team1" "status check"        # to group; "@all" = everyone
& $herdr msg read --pane w1:p3                   # read + auto-ack
& $herdr msg peek --all                          # look without acking
& $herdr msg wait --timeout 30000 --pane w1:p3   # block until a message arrives
& $herdr msg group join team1 --pane w1:p3
& $herdr msg who                                 # who has which labels/groups
```

Note: msg delivers to the pane's *inbox* (TUI shows an unread badge). An agent in that pane only sees it if it polls `msg read` — tell workers in their brief to poll, or use `pane run` to push text into their chat directly.

## Team spawn (HerdrPlus)

One command to stand up a whole crew — workspace + N labeled agent panes in a balanced grid, agent CLIs launched, all joined to msg group `<name>`:

```powershell
& $herdr team spawn crew --agents lead=claude,rev=codex --cwd K:\proj --with-orch --wait --timeout 120
```

- Roster entry: `label=agent` (identifier labels only) or a verbatim command (auto-labeled from the executable). Entries with commas/complex quoting go in the `[team.agents]` registry in `config.toml` (currently empty on this machine), then use the registry name.
- `--with-orch [cmd]` adds an orchestrator pane; `--wait --timeout` blocks until agents report ready; failure rolls back with focus restore.

## Safety rules

- Parse IDs from JSON; use explicit IDs (or `--current` inside a pane) — never rely on whatever pane has UI focus.
- `--no-focus` for anything the user didn't ask to look at.
- Don't close workspaces/tabs/panes/sessions you didn't create, and don't stop the server, without explicit intent — pane processes die with it. The cleanup script's agent check is the backstop, not permission.
- Experiments (including testing this skill) belong in a named session (`--session test` + `HERDR_SOCKET_PATH`), never in the default session where real work may live.
- For full command syntax, read `references/cli-reference.md`; for agent-driving lore, `references/orchestration-playbook.md`.

# HerdrPlus CLI reference

Captured from `herdrplus.exe` v0.7.4 (protocol 19). The binary is the authority — when in doubt run a command group with no subcommand (e.g. `herdrplus pane`) to print its usage. Do NOT run bare `herdrplus` for discovery: it launches/attaches the TUI. Do not probe mutating commands by omitting args — `workspace create` is valid with defaults and will execute.

## Global

```
herdrplus                          launch or attach the persistent session (TUI)
herdrplus --session <name>         use/create a named persistent session
herdrplus --no-session             monolithic mode (no server/client)
herdrplus status [server|client] [--json]
herdrplus server                   run headless server
herdrplus server stop              stop running server via socket (kills pane processes!)
herdrplus server reload-config
herdrplus api snapshot             full runtime state as JSON
herdrplus api schema [--json|--output PATH]
```

Env:
- `HERDR_SESSION` — select a named session for the current process: works for `herdrplus server` (starts that session's server headless) and for every CLI subcommand. Simplest isolation mechanism.
- `HERDR_SOCKET_PATH` — aim any CLI command at a specific session's socket (get paths from `session list --json`). Default socket: `%APPDATA%\herdr\herdr.sock`.
- `HERDR_CONFIG_PATH` — override config file (`%APPDATA%\herdr\config.toml`).
- Inside a managed pane: `HERDR_ENV=1`, `HERDR_WORKSPACE_ID`, `HERDR_TAB_ID`, `HERDR_PANE_ID`.
- Logs: `%APPDATA%\herdr\herdr.log`, `herdr-client.log`, `herdr-server.log`.

## Sessions

```
herdrplus session list [--json]        each entry: name, running, session_dir, socket_path
herdrplus session attach <name>
herdrplus session stop <name> [--json]     ('default' targets the default session)
herdrplus session delete <name> [--json]
```

## Workspaces

```
herdrplus workspace list
herdrplus workspace create [--cwd PATH] [--label TEXT] [--env KEY=VALUE] [--focus|--no-focus]
herdrplus workspace get <workspace_id>
herdrplus workspace focus <workspace_id>
herdrplus workspace rename <workspace_id> <label>
herdrplus workspace close <workspace_id>
```

## Tabs

```
herdrplus tab list [--workspace <workspace_id>]
herdrplus tab create [--workspace ID] [--cwd PATH] [--label TEXT] [--env KEY=VALUE] [--focus|--no-focus]
herdrplus tab get|focus <tab_id>
herdrplus tab rename <tab_id> <label>
herdrplus tab close <tab_id>
```

## Panes

```
herdrplus pane list [--workspace ID]
herdrplus pane current [--pane ID|--current]
herdrplus pane get <pane_id>                    agent, agent_status, geometry, process
herdrplus pane layout [--pane ID|--current]     rectangle info (wide -> split right, tall -> split down)
herdrplus pane process-info [--pane ID|--current]
herdrplus pane neighbor --direction left|right|up|down [--pane ID|--current]
herdrplus pane edges [--pane ID|--current]
herdrplus pane focus --direction left|right|up|down [--pane ID|--current]
herdrplus pane resize --direction left|right|up|down [--amount FLOAT] [--pane ID|--current]
herdrplus pane balance [--tab TAB_ID|--pane ID|--current]        HerdrPlus: equal-area reset (TUI prefix+=)
herdrplus pane zoom [<pane_id>] [--toggle|--on|--off]
herdrplus pane rename <pane_id> <label>|--clear
herdrplus pane read <pane_id> [--source visible|recent|recent-unwrapped] [--lines N] [--format text|ansi]   (prints RAW text, not JSON)
herdrplus pane split [<pane_id>|--current] --direction right|down [--ratio FLOAT] [--cwd PATH] [--env K=V] [--focus|--no-focus]
herdrplus pane swap --direction left|right|up|down [--pane ID|--current]
herdrplus pane swap --source-pane ID --target-pane ID
herdrplus pane move <pane_id> --tab <tab_id> --split right|down [--target-pane ID] [--ratio FLOAT] [--focus|--no-focus]
herdrplus pane move <pane_id> --new-tab [--workspace ID] [--label TEXT] [--focus|--no-focus]
herdrplus pane move <pane_id> --new-workspace [--label TEXT] [--tab-label TEXT] [--focus|--no-focus]
herdrplus pane close <pane_id>                  kills the pane's process tree
herdrplus pane send-text <pane_id> <text>       literal text, no Enter
herdrplus pane send-keys <pane_id> <key> [key ...]
herdrplus pane run <pane_id> <command>          text + Enter atomically (commands AND chat prompts)
```

`send-keys` grammar: named keys (`Enter`, `Escape`, `Up`, `Down`, `Left`, `Right`, `Tab`, ...), chords `ctrl+<x>` (legacy aliases `C-c`/`c-c` accepted), literal characters. Arrow keys matter: some agent TUIs (agy) have arrow-driven menus, not number-driven.

Read sources: `visible` (rendered viewport), `recent` (scrollback with soft wraps), `recent-unwrapped` (soft wraps joined — best for logs/transcripts), `detection` (agent-detection snapshot). `--format ansi` when styling is evidence.

**IDs:** workspace `w1`, tab `w1:t1`, pane `w1:p1`, terminal `term_...` — opaque strings; suffixes can grow letters. Closed IDs never reused; a moved pane gets a NEW id. Always re-read from JSON responses.

## Wait

```
herdrplus wait output <pane_id> --match <text> [--source ...] [--lines N] [--timeout MS] [--regex] [--raw]
herdrplus wait agent-status <pane_id> --status idle|working|blocked|done|unknown [--timeout MS]
```

Exit status 1 on timeout. Status waits match the current status immediately or wait for the next transition. Inspect (`pane read`, `pane get`) BEFORE waiting.

## Agents

```
herdrplus agent list
herdrplus agent get|read|send|rename|focus <target>
herdrplus agent wait <target> --status idle|working|blocked|unknown [--timeout MS]
herdrplus agent attach <target> [--takeover]
herdrplus agent start <name> [--cwd PATH] [--workspace ID] [--tab ID] [--split right|down] [--env K=V] [--focus|--no-focus] -- <argv...>
herdrplus agent explain <target> [--json]
```

Targets: terminal ids, unique agent names, detected/reported labels, legacy pane ids. `agent send` writes literal text — use `pane run` when you want text + Enter.

Status semantics: `idle` and `done` are the same finished state; `done` means the result hasn't been seen (background tab), `idle` means seen. Treat either as "completed". `blocked` = waiting on input. `unknown` = no detected agent.

## Msg bus (HerdrPlus, protocol 18)

```
herdrplus msg send <target> <text>       target = pane id | pane label | @group | @all
herdrplus msg read [--all] [--after SEQ] [--pane ID]     (--after => peek-like, never auto-acks)
herdrplus msg peek [--all] [--after SEQ] [--pane ID]
herdrplus msg ack <up-to-seq> [--pane ID]
herdrplus msg wait [--timeout MS] [--pane ID]
herdrplus msg group join|leave <name> [--pane ID]
herdrplus msg who
```

Messages land in per-pane inboxes (TUI unread badge). Agents must poll `msg read` to see them — or push into their chat with `pane run` instead.

## Team spawn (HerdrPlus, protocol 19)

```
herdrplus team spawn <name> --agents <entry>[,<entry>...] [--cwd DIR] [--with-orch [CMD]] [--wait] [--timeout SECS]
```

- entry = `<agent>` or `<label>=<agent>`; agent resolved via `[team.agents]` in config.toml; unknown names run verbatim as commands (auto-label from executable basename). `label=agent` form requires identifier labels.
- Entries containing commas or complex quoting must be registered in `[team.agents]` first.
- Spawns: new workspace named <name> + N labeled panes in a balanced grid + agent CLIs launched + every pane joined to msg group `<name>`. `--with-orch` adds an orchestrator pane. `--wait --timeout` blocks until ready. On failure: rollback with focus restore.

## Other groups

`worktree` (git worktree helpers), `notification`, `integration`, `config`, `channel`, `update`, `completion` — run the group name for usage if needed.

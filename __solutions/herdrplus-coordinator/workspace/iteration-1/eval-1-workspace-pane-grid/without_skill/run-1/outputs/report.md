# Eval 1 — Workspace Pane Grid (without_skill)

Date: 2026-07-19
Binary: `K:\Downloads\__Projects.Mine\herdr4Windows\__solutions\target\release\herdrplus.exe` (herdr 0.7.4, protocol 19)
Session isolation: all CLI commands ran with `HERDR_SESSION=grid1` against a dedicated headless server (`herdr server`) with socket `C:\Users\Tony Baloney\AppData\Roaming\herdr\sessions\grid1\herdr.sock`. The default session was never started (verified `status: not running` before and after).

## Workspace and panes

Workspace `w1` (label `grid`), tab `w1:t1`. Layout built by splitting the root pane right, then splitting the right pane down: build occupies the full-height left column; test top-right; logs bottom-right.

| Label | Pane ID | Terminal ID          | Command run       |
|-------|---------|----------------------|-------------------|
| build | w1:p1   | term_656f9addbd2e21  | `echo ready-build` |
| test  | w1:p2   | term_656f9ae4801dc2  | `echo ready-test`  |
| logs  | w1:p3   | term_656f9ae8f29303  | `echo ready-logs`  |

## Captured output

Via `herdr pane run <id> "<cmd>"` followed by `herdr pane read <id> --source recent` (p3 re-read with `--source recent-unwrapped` to undo terminal line wrap).

### build (w1:p1)
```
PS C:\Users\Tony Baloney> echo ready-build
ready-build
PS C:\Users\Tony Baloney>
```

### test (w1:p2)
```
PS C:\Users\Tony Baloney> echo ready-test
ready-test
PS C:\Users\Tony Baloney>
```

### logs (w1:p3)
```
PS C:\Users\Tony Baloney> echo ready-logs
ready-logs
PS C:\Users\Tony Baloney>
```

## Balance

`herdr pane balance --tab w1:t1` returned `"changed": true`, root split ratio adjusted 0.5 -> 0.33333334 (three panes equalized; inner down-split stayed 0.5). Post-balance layout rects:

- w1:p1 (build): x=26 y=1  w=18 h=23
- w1:p2 (test):  x=44 y=1  w=36 h=12
- w1:p3 (logs):  x=44 y=13 w=36 h=11

## Teardown evidence

1. `herdr session stop grid1 --json` -> `{"stopped": true, "session": {"name": "grid1", "running": false, ...}}`
2. `herdr session delete grid1 --json` -> `{"deleted": true, ...}`
3. `herdr session list --json` after teardown lists only the default session (`running: false`):
   `{"sessions":[{"default":true,"name":"default","running":false,"session_dir":"C:\Users\Tony Baloney\AppData\Roaming\herdr","socket_path":"C:\Users\Tony Baloney\AppData\Roaming\herdr\herdr.sock"}]}`
4. The background `herdr server` process for grid1 exited with code 0.
5. `C:\Users\Tony Baloney\AppData\Roaming\herdr\sessions\` no longer contains a `grid1` directory.
6. Default session server status after teardown: `not running` (untouched throughout).

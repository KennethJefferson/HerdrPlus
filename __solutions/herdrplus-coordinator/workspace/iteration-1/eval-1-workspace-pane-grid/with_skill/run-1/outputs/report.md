# Eval 1 — Workspace pane grid (with skill)

Date: 2026-07-19
Binary: `K:\Downloads\__Projects.Mine\herdr4Windows\__solutions\target\release\herdrplus.exe` (v0.7.4, protocol 19)
Session: isolated named session `grid1` (default session untouched; it was not running before, during, or after).

## Session setup

- Headless server started with `herdrplus --session grid1 server` (background).
- Socket confirmed via `session list --json`:
  `C:\Users\Tony Baloney\AppData\Roaming\herdr\sessions\grid1\herdr.sock`
- All CLI calls aimed at the session via `HERDR_SOCKET_PATH`.

## Workspace and pane IDs

Workspace `w1` (label `grid`), tab `w1:t1`, cwd `K:\Downloads\__Projects.Mine\herdr4Windows`.

Layout: left column + right column split in two (split `w1:p1` right, then split `w1:p2` down).

| Label | Pane ID | Terminal ID           |
|-------|---------|-----------------------|
| build | w1:p1   | term_656f971f0a8dc1   |
| test  | w1:p2   | term_656f973f60a052   |
| logs  | w1:p3   | term_656f975029ecd3   |

## Captured outputs

Commands submitted with `pane run`, confirmed with `wait output` (all exited 0, no timeout). Captured text from each pane (`recent-unwrapped` source, embedded in the wait-output match response):

**build (w1:p1)**
```
PS K:\Downloads\__Projects.Mine\herdr4Windows> echo ready-build
ready-build
PS K:\Downloads\__Projects.Mine\herdr4Windows>
```

**test (w1:p2)**
```
PS K:\Downloads\__Projects.Mine\herdr4Windows> echo ready-test
ready-test
PS K:\Downloads\__Projects.Mine\herdr4Windows>
```

**logs (w1:p3)**
```
PS K:\Downloads\__Projects.Mine\herdr4Windows> echo ready-logs
ready-logs
PS K:\Downloads\__Projects.Mine\herdr4Windows>
```

## Balance

`pane balance --tab w1:t1` returned `layout_balanced` with `"changed": true`. Resulting tree (equal-area):

```json
{"direction":"right","ratio":0.33333334,
 "first":{"pane_id":"w1:p1","label":"build"},
 "second":{"direction":"down","ratio":0.5,
   "first":{"pane_id":"w1:p2","label":"test"},
   "second":{"pane_id":"w1:p3","label":"logs"}}}
```

## Teardown evidence

1. `session stop grid1 --json` -> `{"stopped":true, "session":{"name":"grid1","running":false,...}}`
2. Background server process exited cleanly (exit code 0) after the stop.
3. `session delete grid1 --json` -> `{"deleted":true,...}`
4. Final `session list --json` shows only the default session (not running):
   ```json
   {"sessions":[{"default":true,"name":"default","running":false,
     "session_dir":"C:\Users\Tony Baloney\AppData\Roaming\herdr",
     "socket_path":"C:\Users\Tony Baloney\AppData\Roaming\herdr\herdr.sock"}]}
   ```
5. `Get-Process herdr*` returned no processes — no leftover herdrplus/server/pane processes, no lingering RAM.

# HerdrPlus Window Management — Eval 2 Report (with skill)

Date: 2026-07-19. Executed per `herdrplus-coordinator` skill (SKILL.md + `scripts/herdr-window.ps1` + `scripts/herdr-cleanup.ps1`).
Binary: `K:\Downloads\__Projects.Mine\herdr4Windows\__solutions\target\release\herdrplus.exe` (v0.7.4, protocol 19).
Session: `default`. Initial state confirmed clean: `status --json` -> `"server":{"status":"not_running"}`, zero `herdr*` processes.

## Phase 1 — Open the window, maximized

Ran the skill script:

```
pwsh -File scripts\herdr-window.ps1 -Action open
```

The script launched `wt -w herdrplus-default --maximized new-tab --title "HerdrPlus[default]" --suppressApplicationTitle -- herdrplus.exe`, but its `FindWindowW`-by-title poll timed out (`result: launched_but_window_not_found`, exit 1). Investigation showed this was a **false negative from the script's verification, not a launch failure**: the window existed and was visible.

Evidence (direct Win32 inspection of the WindowsTerminal process, PID 2124):

```json
{
  "title": "HerdrPlus[default]",
  "title_hex": "48 65 72 64 72 50 6C 75 73 5B 64 65 66 61 75 6C 74 5D",
  "hwnd": 266894,
  "visible": true,
  "find_exact": 0,
  "maximized": false
}
```

`find_exact: 0` means `FindWindowW(null, title)` returns nothing from this shell — a UIPI/window-station quirk — and wt's `--maximized` flag was also not honored.

Two fixups applied via the window handle taken from the process (fallback consistent with the script's own `Find-HerdrWindow` strategy): `ShowWindow(hwnd, SW_MAXIMIZE)` + `SetForegroundWindow(hwnd)`.

Maximization verified by two independent signals:

```json
{ "maximized": true, "rect": "left=-8 top=-8 right=1928 bottom=1040" }
```

`IsZoomed = true`, and the rect spans the full 1920x1080 desktop with the standard -8px maximized border offsets.

## Phase 2 — Verify it is actually open

Process and server evidence while the window was up:

```
   Id ProcessName RAM_MB StartTime
 8224 herdrplus     9.70 7/19/2026 12:44:00 PM   <- client (inside the wt window)
24208 herdrplus    18.90 7/19/2026 12:44:04 PM   <- server (spawned by client)
```

```
herdrplus status --json ->
"server":{"status":"running","running":true,"version":"0.7.4","protocol":19,
 "capabilities":{"live_handoff":false,"detached_server_daemon":true},
 "socket":"C:\Users\Tony Baloney\AppData\Roaming\herdr\herdr.sock"}
```

```
herdrplus workspace list ->
{"workspaces":[{"workspace_id":"w1","label":"herdr4Windows","tab_count":1,"pane_count":1,"focused":true}]}
```

Window visible + maximized, client and server processes present, server answering on the socket with a live workspace: **open verified**.

## Phase 3 — Close only the window; server must survive

Sent `WM_CLOSE` (0x0010) via `PostMessageW` to hwnd 266894 — exactly what `herdr-window.ps1 -Action close` does (its title lookup has the same FindWindowW blind spot, so the message was posted directly to the known handle).

State 4 seconds after close:

```
wt 2124 alive:      False   <- window gone
client 8224 alive:  False   <- TUI client gone
server 24208 alive: True    <- server survived

   Id RAM_MB
24208  18.70

herdrplus status --json -> "server":{"status":"running","running":true, ...}
```

The client/server design held: closing the window detached only the client; the server (PID 24208, ~18.7 MB) kept running and kept answering on the socket. **Server persistence verified**.

## Phase 4 — Full server cleanup (reclaim RAM)

Dry-run inventory first (skill safety step — checks for live agents before destroying anything):

```
pwsh -File scripts\herdr-cleanup.ps1 -DryRun ->
{ "running_sessions": ["default"], "processes": [{"pid":24208,"name":"herdrplus","ram_mb":18.4}],
  "live_agents": [], "ram_mb": 18.4, "result": "dry_run" }
```

No live agents detected, so `-Force` was unnecessary. Real run:

```
pwsh -File scripts\herdr-cleanup.ps1 ->
{ "result": "clean", "sessions_stopped": ["default"], "ram_freed_mb": 18.5,
  "force_killed": [], "remaining": [],
  "note": "The herdrplus.exe binary is now unlocked for rebuilds." }
```

Graceful `session stop` succeeded; nothing needed force-killing.

## Final verification — nothing left in RAM

Independent post-cleanup check:

```
herdr processes remaining: 0
herdrplus status --json -> "server":{"status":"not_running","running":false}
Test-Path ...\Roaming\herdr\herdr.sock -> False   (socket file removed)
```

## Outcome

| Phase | Result |
|---|---|
| 1. Open window maximized | Done (script launch + direct Win32 maximize; IsZoomed=true, full-screen rect) |
| 2. Verify open | Done (window visible, client PID 8224 + server PID 24208, server running, workspace w1) |
| 3. Close window only | Done (wt + client exited; server PID 24208 survived and answered status) |
| 4. Full cleanup | Done (`result: clean`, 18.5 MB freed, 0 processes, socket gone, server not_running) |

Noted defect for the skill: `herdr-window.ps1`'s `FindWindowW($null, $title)` verification returns 0 from this (elevated) automation shell even when the exact-title window exists, so `open` false-fails and `close`/`maximize` would miss the window. The reliable fallback is resolving the hwnd from `Get-Process WindowsTerminal | Where MainWindowTitle -eq 'HerdrPlus[<session>]'`.

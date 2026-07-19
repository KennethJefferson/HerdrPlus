# HerdrPlus Window Management — Execution Report

Date: 2026-07-19, ~13:00–13:05 local
Binary: `K:\Downloads\__Projects.Mine\herdr4Windows\__solutions\target\release\herdrplus.exe` (v0.7.4, protocol 19)
Machine: Windows 11 Pro 10.0.26200, unattended run

## Background: client/server design

Per `__references/herdr/src/server/autodetect.rs` (`auto_detect_launch`), running `herdrplus`
with no arguments spawns a fully detached `herdrplus server` daemon process, waits for its
socket, then attaches the TUI as a thin client. Closing the client window must therefore leave
the server running — that is the behavior under test.

## Phase 0 — Pre-state

```
Get-CimInstance Win32_Process -Filter "Name like 'herdr%'"   -> (no results)
```

No herdr processes existed before the test. Clean baseline.

## Phase 1 — Open the window, maximized, and verify

Launch (classic console host so `-WindowStyle Maximized` is honored):

```powershell
Start-Process -FilePath conhost.exe -ArgumentList '"...\herdrplus.exe"' -WindowStyle Maximized -PassThru
```

Process evidence (6 s after launch) — client attached in the window, detached server spawned:

```
ProcessId : 22100  ParentProcessId : 29676 (conhost)   CommandLine : ...\herdrplus.exe            <- TUI client
ProcessId : 29204  ParentProcessId : 22100             CommandLine : "...\herdrplus.exe" server   <- detached server daemon
```

Window evidence (Win32, on hwnd `1119016`, class `ConsoleWindowClass`, owned by client PID 22100):

- `IsWindowVisible` = **True**
- `IsZoomed` (maximized) = **True** (set by the launch flag; re-asserted with `ShowWindow SW_MAXIMIZE`)
- `GetWindowRect` = L=-8 T=-8 R=1928 B=1040 on a 1920x1080 primary display — the classic
  maximized geometry (work area + invisible 8 px borders)
- `DWMWA_CLOAKED` = 0 (not cloaked; genuinely on the visible desktop)
- `GetForegroundWindow` = `1119016` after activation (matches target)

Visual evidence: `phase1-window-open-maximized.png` (captured with `PrintWindow`/`PW_RENDERFULLCONTENT`
directly from the hwnd). It shows the HerdrPlus TUI fully rendered in the maximized window:
"spaces" sidebar with workspace `herdr4Windows` / branch `main`, tab strip, active pane running
PowerShell at `K:\Downloads\__Projects.Mine\herdr4Windows\__references\herdr>`, and the
`new / menu / agents / grouped` footer controls.

Server API evidence (over the herdr socket):

```
> herdrplus status server --json
{"status":"running","running":true,"version":"0.7.4","protocol":19,
 "capabilities":{"live_handoff":false,"detached_server_daemon":true},
 "compatible":true,"socket":"C:\Users\Tony Baloney\AppData\Roaming\herdr\herdr.sock",
 "session":null,"restart_needed":false}
```

**Phase 1 verdict: window open, maximized, TUI rendering, server running.**

## Phase 2 — Close only the window; verify the server survives

Close: `PostMessage(hwnd 1119016, WM_CLOSE)` — window-level close only, no process was killed
directly.

State 4 s later:

```
client PID 22100 alive: False        <- TUI client exited with its window
conhost PID 29676 alive: False       <- console host gone
server PID 29204 alive: True         <- detached server SURVIVED

ProcessId : 29204   CommandLine : "...\herdrplus.exe" server   (only herdr process remaining)

> herdrplus status server --json
{"status":"running","running":true,"version":"0.7.4","protocol":19,...}
```

**Phase 2 verdict: closing the window killed only the client; the server stayed alive and kept
answering API requests — the client/server design works as intended.**

## Phase 3 — Full cleanup; nothing left in RAM

```
> herdrplus server stop
exit code: 0
```

State 3 s later:

```
Get-CimInstance Win32_Process -Filter "Name like 'herdr%'"   -> NONE - no herdr processes remain in RAM

> herdrplus status server --json
{"status":"not_running","running":false,"version":null,"protocol":null,...}
```

**Phase 3 verdict: server stopped cleanly via its own API; zero herdr/herdrplus processes
remain. Final state identical to the Phase 0 baseline.**

## Artifacts

- `report.md` — this file
- `phase1-window-open-maximized.png` — PrintWindow capture of the maximized HerdrPlus TUI

## Notes

- A full-desktop screenshot taken mid-run showed a Chrome window in the physical foreground even
  though `GetForegroundWindow` reported the herdr hwnd; the `PrintWindow` capture of the hwnd
  itself was used as the authoritative visual evidence since it renders the window's actual
  contents regardless of z-order/monitor.
- Window title reads `C:\WINDOWS\system32\conhost.exe` because the TUI was hosted in a classic
  conhost window (chosen deliberately so the maximize flag and WM_CLOSE targeting were reliable).

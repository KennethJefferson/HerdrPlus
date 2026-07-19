# HerdrPlus session lifecycle + cleanup report (session `ram1`)

Date: 2026-07-19
Binary: `K:\Downloads\__Projects.Mine\herdr4Windows\__solutions\target\release\herdrplus.exe` (v0.7.4, protocol 19)
Method: `herdrplus-coordinator` skill — named throwaway session so the **default session was never touched** (it stayed `running: false` throughout, and only the `ram1` session I created was ever stopped).

Why RAM lingers after closing the window: herdr is client/server. Closing the TUI window only detaches the client; the session server (plus conpty hosts and pane child processes) keeps running by design. Reclaiming RAM requires stopping the server(s), which is what this run demonstrates end-to-end.

---

## 1. Baseline (before starting anything)

```powershell
$exe = 'K:\Downloads\__Projects.Mine\herdr4Windows\__solutions\target\release\herdrplus.exe'
Get-Process herdrplus, herdr -ErrorAction SilentlyContinue   # -> nothing
& $exe status --json
& $exe session list --json
```

Evidence:

- `Get-Process herdr*` returned **no processes**.
- `status --json` -> `"server":{"status":"not_running","running":false,...}`
- `session list --json` -> `default` and `cctest` both `"running":false`. No session named `ram1` existed.

## 2. Start throwaway session `ram1` (state: RUNNING)

Started a **headless** server for a named session — no window needed, default session untouched:

```powershell
Start-Process -FilePath $exe -ArgumentList '--session','ram1','server' -WindowStyle Hidden -PassThru
# launcher_pid=4192
& $exe session list --json
```

Evidence (server actually running):

- `session list --json` now includes:
  `{"default":false,"name":"ram1","running":true,"session_dir":"C:\Users\Tony Baloney\AppData\Roaming\herdr\sessions\ram1","socket_path":"C:\Users\Tony Baloney\AppData\Roaming\herdr\sessions\ram1\herdr.sock"}`
  while `default` remained `"running":false`.
- `status --json` aimed at ram1's own socket confirmed a live, compatible server:

```powershell
$env:HERDR_SOCKET_PATH = 'C:\Users\Tony Baloney\AppData\Roaming\herdr\sessions\ram1\herdr.sock'
& $exe status --json
# -> "server":{"status":"running","running":true,"version":"0.7.4","protocol":19,"compatible":true,
#              "socket":"C:\...\sessions\ram1\herdr.sock"}
Remove-Item Env:HERDR_SOCKET_PATH
```

- Process inventory: one `herdrplus` process, **PID 4192, 14.0 MB** working set.

## 3. Pre-shutdown safety inventory (dry run)

```powershell
pwsh -File K:\Downloads\__Projects.Mine\herdr4Windows\__solutions\herdrplus-coordinator\skill\scripts\herdr-cleanup.ps1 -DryRun
```

Output:

```json
{
  "result": "dry_run",
  "ram_mb": 14.0,
  "running_sessions": ["ram1"],
  "live_agents": [],
  "processes": [{ "pid": 4192, "name": "herdrplus", "ram_mb": 14.0 }],
  "would": "session stop each running session, then force-kill leftover herdr* processes"
}
```

Only `ram1` (created by this run) was running; **no live agents** would be killed, so it was safe to proceed without `-Force`.

## 4. Shutdown (state: STOPPED)

```powershell
pwsh -File K:\Downloads\__Projects.Mine\herdr4Windows\__solutions\herdrplus-coordinator\skill\scripts\herdr-cleanup.ps1
```

(Internally: `herdrplus session stop ram1 --json`, 3 s grace, then force-kill of any leftover `herdr*` — none needed.)

Output:

```json
{
  "result": "clean",
  "sessions_stopped": ["ram1"],
  "force_killed": [],
  "remaining": [],
  "ram_freed_mb": 14.1,
  "note": "The herdrplus.exe binary is now unlocked for rebuilds."
}
```

Graceful stop only — nothing had to be force-killed. **14.1 MB of RAM reclaimed.**

## 5. Final verification (state: PROCESS-FREE)

```powershell
& $exe session delete ram1 --json     # remove the throwaway session record entirely
& $exe session list --json
$procs = @(Get-Process herdrplus, herdr -ErrorAction SilentlyContinue); "herdr_process_count=$($procs.Count)"
& $exe status --json
```

Evidence:

- `session delete ram1 --json` -> `{"deleted":true, "session":{"name":"ram1","running":false,...}}`
- `session list --json` -> back to exactly the baseline: `default` and `cctest`, both `"running":false`; `ram1` gone.
- `Get-Process herdrplus, herdr` -> **herdr_process_count=0** (no herdr processes of any kind remain).
- `status --json` -> `"server":{"status":"not_running","running":false,...}`

## Conclusion

| State        | Evidence                                                                                  |
| ------------ | ----------------------------------------------------------------------------------------- |
| Running      | `session list` shows `ram1 running:true`; `status --json` on ram1 socket `running:true`; PID 4192 @ 14 MB |
| Stopped      | cleanup result `clean`, `sessions_stopped:["ram1"]`, `force_killed:[]`, 14.1 MB freed      |
| Process-free | `Get-Process herdrplus, herdr` count = 0; server `not_running`; `ram1` session deleted     |

The default session was never started, stopped, or modified. For future RAM complaints after closing the herdr window: `herdr-cleanup.ps1 -DryRun` to inventory, then `herdr-cleanup.ps1` (add `-Force` only if you intend to kill live agent panes).

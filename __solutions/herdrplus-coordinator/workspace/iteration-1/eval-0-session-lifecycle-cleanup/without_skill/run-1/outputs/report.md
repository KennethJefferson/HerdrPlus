# herdr session lifecycle cleanup — ram1

Date: 2026-07-19
Binary: `K:\Downloads\__Projects.Mine\herdr4Windows\__solutions\target\release\herdrplus.exe` (herdr 0.7.4, protocol 19)
Constraint honored: the `default` session was never started, attached, stopped, or deleted. It stayed `running: false` throughout.

## 1. Baseline (before start)

Commands:

```powershell
Get-Process | Where-Object { $_.ProcessName -match 'herdr' }
& $exe session list --json
```

Evidence:

```
(no herdr processes)

{"sessions":[{"default":true,"name":"default","running":false,
  "session_dir":"C:\Users\Tony Baloney\AppData\Roaming\herdr",
  "socket_path":"C:\Users\Tony Baloney\AppData\Roaming\herdr\herdr.sock"}]}
```

No herdr processes; only the (stopped) default session registered.

## 2. Start throwaway session `ram1` (state: RUNNING)

`herdr` selects a session via the `HERDR_SESSION` env var, and `herdr server` runs the server headless (no TUI attach needed — safe unattended). Commands:

```powershell
$env:HERDR_SESSION = 'ram1'
Start-Process -FilePath $exe -ArgumentList 'server' -WindowStyle Hidden -PassThru   # spawned pid 21928
& $exe status server
& $exe session list --json
```

Evidence (after ~5s startup; an immediate check at 3s still reported "not running" — the server needs a few seconds before its socket accepts hellos):

```
status: running
version: 0.7.4
protocol: 19
compatible: yes
socket: C:\Users\Tony Baloney\AppData\Roaming\herdr\sessions\ram1\herdr.sock

session list: ... {"default":false,"name":"ram1","running":true, ...}
              ... {"default":true,"name":"default","running":false, ...}

Get-Process -Id 21928  ->  21928 herdrplus  WorkingSet 7,483,392 bytes
```

Server confirmed running: live process (pid 21928), reachable API socket, `running: true` in the session registry. Default session untouched (`running: false`).

## 3. Shut down (state: STOPPED)

Command:

```powershell
& $exe session stop ram1 --json
```

Evidence — the stop initially reported a timeout:

```
{"error":{"code":"session_stop_failed","message":"session ram1 did not stop within 15000ms;
 sockets are still reachable at ...\sessions\ram1\herdr.sock, ...\herdr-client.sock"}}
```

The stop request did land, but the server took longer than the CLI's 15s wait to exit. Roughly 20-30s later:

```powershell
$env:HERDR_SESSION = 'ram1'
& $exe status server
```

```
status: not running
socket: C:\Users\Tony Baloney\AppData\Roaming\herdr\sessions\ram1\herdr.sock

Get-Process -Id 21928  ->  pid 21928 gone
```

Note for the RAM-hogging complaint: `session stop` can report failure while the server is actually shutting down slowly. Always re-verify with `herdr status server` + a process check after ~30s before force-killing; the immediate error is misleading.

## 4. Delete throwaway session + final verification (state: PROCESS-FREE)

Commands:

```powershell
& $exe session delete ram1 --json
& $exe session list --json
Test-Path 'C:\Users\Tony Baloney\AppData\Roaming\herdr\sessions\ram1'
Get-Process | Where-Object { $_.ProcessName -match 'herdr' }
```

Evidence:

```
{"deleted":true,"session":{"default":false,"name":"ram1","running":false, ...}}

{"sessions":[{"default":true,"name":"default","running":false, ...}]}   # only default remains

Test-Path sessions\ram1  ->  False   # session dir removed

NO herdr processes running
```

Final state: zero herdr/herdrplus processes, ram1 fully deleted (registry entry and session directory), default session left exactly as found.

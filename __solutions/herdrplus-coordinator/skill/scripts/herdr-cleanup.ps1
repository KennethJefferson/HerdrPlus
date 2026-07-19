#Requires -Version 7
<#
Reclaim RAM from HerdrPlus. The session server (and every pane's process tree)
survives closing the client window by design; this script shuts it all down.

Order of operations:
  1. Inventory herdr processes and RAM.
  2. Safety check: if any running session has detected agents, refuse unless -Force
     (stopping the server kills every pane process, including live agents).
  3. Graceful: herdrplus session stop <name> for each running session
     (covers the default session too).
  4. Kill any herdr* processes still alive after a grace period.
  5. Report RAM freed.

-DryRun reports steps 1-2 and what WOULD be stopped, changing nothing.
#>
param(
    [switch]$Force,
    [switch]$DryRun
)

$ErrorActionPreference = 'Stop'

function Resolve-HerdrExe {
    if ($env:HERDRPLUS_EXE -and (Test-Path $env:HERDRPLUS_EXE)) { return $env:HERDRPLUS_EXE }
    $cmd = Get-Command herdrplus -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd.Source }
    $known = 'K:\Downloads\__Projects.Mine\herdr4Windows\__solutions\target\release\herdrplus.exe'
    if (Test-Path $known) { return $known }
    throw 'herdrplus.exe not found: set HERDRPLUS_EXE, add it to PATH, or build the release binary.'
}

function Get-HerdrProcs {
    @(Get-Process herdrplus, herdr -ErrorAction SilentlyContinue)
}

function Emit($obj) { $obj | ConvertTo-Json -Depth 6 }

$before = Get-HerdrProcs
$beforeMb = [math]::Round((($before | Measure-Object WorkingSet64 -Sum).Sum ?? 0) / 1MB, 1)

if (-not $before) {
    Emit @{ result = 'already_clean'; processes = 0; ram_mb = 0 }
    return
}

$exe = Resolve-HerdrExe
$sessions = @()
try { $sessions = (& $exe session list --json | ConvertFrom-Json).sessions } catch { }
$running = @($sessions | Where-Object { $_.running })

# Safety: look for live agents in each running session before killing everything.
$agents = @()
foreach ($s in $running) {
    $old = $env:HERDR_SOCKET_PATH
    try {
        $env:HERDR_SOCKET_PATH = $s.socket_path
        $list = & $exe agent list 2>$null | ConvertFrom-Json
        foreach ($a in @($list.result.agents ?? $list.agents)) {
            $agents += @{ session = $s.name; agent = $a.agent ?? $a.name; pane = $a.pane_id; status = $a.agent_status ?? $a.status }
        }
    } catch { } finally {
        if ($null -ne $old) { $env:HERDR_SOCKET_PATH = $old } else { Remove-Item Env:HERDR_SOCKET_PATH -ErrorAction SilentlyContinue }
    }
}

if ($DryRun) {
    Emit @{
        result    = 'dry_run'
        processes = @($before | ForEach-Object { @{ pid = $_.Id; name = $_.ProcessName; ram_mb = [math]::Round($_.WorkingSet64 / 1MB, 1) } })
        ram_mb    = $beforeMb
        running_sessions = @($running | ForEach-Object { $_.name })
        live_agents      = $agents
        would  = 'session stop each running session, then force-kill leftover herdr* processes'
    }
    return
}

if ($agents.Count -gt 0 -and -not $Force) {
    Emit @{
        result      = 'refused_live_agents'
        live_agents = $agents
        note        = 'Stopping the server kills these agent panes. Re-run with -Force to proceed anyway.'
    }
    exit 2
}

$stopped = @()
foreach ($s in $running) {
    try {
        & $exe session stop $s.name --json | Out-Null
        $stopped += $s.name
    } catch { }
}

Start-Sleep -Seconds 3

$leftover = Get-HerdrProcs
$killed = @()
foreach ($p in $leftover) {
    try {
        Stop-Process -Id $p.Id -Force -Confirm:$false -ErrorAction Stop
        $killed += @{ pid = $p.Id; name = $p.ProcessName }
    } catch { }
}

Start-Sleep -Milliseconds 500
$after = Get-HerdrProcs
Emit @{
    result           = if ($after) { 'partial' } else { 'clean' }
    sessions_stopped = $stopped
    force_killed     = $killed
    ram_freed_mb     = $beforeMb
    remaining        = @($after | ForEach-Object { @{ pid = $_.Id; name = $_.ProcessName } })
    note             = 'The herdrplus.exe binary is now unlocked for rebuilds.'
}
if ($after) { exit 1 }

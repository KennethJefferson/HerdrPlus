#Requires -Version 7
<#
Window lifecycle for the HerdrPlus client.
Opens the client in a dedicated Windows Terminal window whose tab title is pinned
(--suppressApplicationTitle), so the window is findable by exact title later.
close detaches the client ONLY - the herdr server keeps running (use herdr-cleanup.ps1 for that).
#>
param(
    [Parameter(Mandatory)]
    [ValidateSet('open', 'close', 'maximize', 'minimize', 'restore', 'focus', 'status')]
    [string]$Action,

    [string]$Session = 'default',

    [switch]$NoMaximize
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

Add-Type -Namespace HerdrWin -Name Native -MemberDefinition @'
[DllImport("user32.dll", CharSet = CharSet.Unicode)]
public static extern IntPtr FindWindowW(string lpClassName, string lpWindowName);
[DllImport("user32.dll")]
public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
[DllImport("user32.dll")]
public static extern bool SetForegroundWindow(IntPtr hWnd);
[DllImport("user32.dll")]
public static extern bool PostMessageW(IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam);
[DllImport("user32.dll")]
public static extern bool IsZoomed(IntPtr hWnd);
[DllImport("user32.dll")]
public static extern bool IsIconic(IntPtr hWnd);
'@

$title = "HerdrPlus[$Session]"

function Find-HerdrWindow {
    # FindWindowW by exact title can false-fail from elevated/automation shells;
    # the WindowsTerminal process MainWindowTitle match is the reliable path.
    foreach ($p in (Get-Process WindowsTerminal -ErrorAction SilentlyContinue)) {
        if ($p.MainWindowTitle -eq $title -and $p.MainWindowHandle -ne 0) { return $p.MainWindowHandle }
    }
    $h = [HerdrWin.Native]::FindWindowW($null, $title)
    if ($h -ne [IntPtr]::Zero) { return $h }
    # Fallback: client launched outside wt owns its own console window
    foreach ($p in (Get-Process herdrplus, herdr -ErrorAction SilentlyContinue)) {
        if ($p.MainWindowHandle -ne 0) { return $p.MainWindowHandle }
    }
    return [IntPtr]::Zero
}

function Emit($obj) { $obj | ConvertTo-Json -Depth 4 }

switch ($Action) {
    'open' {
        $hwnd = Find-HerdrWindow
        if ($hwnd -ne [IntPtr]::Zero) {
            [void][HerdrWin.Native]::SetForegroundWindow($hwnd)
            if (-not $NoMaximize) { [void][HerdrWin.Native]::ShowWindow($hwnd, 3) }
            Emit @{ action = 'open'; result = 'already_open'; hwnd = $hwnd.ToInt64(); title = $title }
            break
        }
        $exe = Resolve-HerdrExe
        $wtArgs = @('-w', "herdrplus-$Session")
        if (-not $NoMaximize) { $wtArgs += '--maximized' }
        $wtArgs += @('new-tab', '--title', $title, '--suppressApplicationTitle', '--', $exe)
        if ($Session -ne 'default') { $wtArgs += @('--session', $Session) }
        Start-Process wt.exe -ArgumentList $wtArgs
        $deadline = (Get-Date).AddSeconds(20)
        do {
            Start-Sleep -Milliseconds 500
            $hwnd = Find-HerdrWindow
        } until ($hwnd -ne [IntPtr]::Zero -or (Get-Date) -gt $deadline)
        if ($hwnd -eq [IntPtr]::Zero) {
            Emit @{ action = 'open'; result = 'launched_but_window_not_found'; title = $title }
            exit 1
        }
        # wt's --maximized flag is unreliable; enforce via ShowWindow
        if (-not $NoMaximize) { [void][HerdrWin.Native]::ShowWindow($hwnd, 3) }
        Emit @{ action = 'open'; result = 'opened'; hwnd = $hwnd.ToInt64(); title = $title; exe = $exe; maximized = [HerdrWin.Native]::IsZoomed($hwnd) }
    }
    'close' {
        $hwnd = Find-HerdrWindow
        if ($hwnd -eq [IntPtr]::Zero) {
            Emit @{ action = 'close'; result = 'no_window'; note = 'No HerdrPlus window found. Server may still be running; use herdr-cleanup.ps1 to stop it.' }
            break
        }
        [void][HerdrWin.Native]::PostMessageW($hwnd, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero)
        Emit @{ action = 'close'; result = 'wm_close_sent'; hwnd = $hwnd.ToInt64(); note = 'Client window closed. The herdr server and pane processes KEEP RUNNING; run herdr-cleanup.ps1 to reclaim RAM.' }
    }
    { $_ -in 'maximize', 'minimize', 'restore', 'focus' } {
        $hwnd = Find-HerdrWindow
        if ($hwnd -eq [IntPtr]::Zero) {
            Emit @{ action = $Action; result = 'no_window'; title = $title }
            exit 1
        }
        switch ($Action) {
            'maximize' { [void][HerdrWin.Native]::ShowWindow($hwnd, 3) }
            'minimize' { [void][HerdrWin.Native]::ShowWindow($hwnd, 6) }
            'restore'  { [void][HerdrWin.Native]::ShowWindow($hwnd, 9) }
        }
        [void][HerdrWin.Native]::SetForegroundWindow($hwnd)
        Emit @{ action = $Action; result = 'ok'; hwnd = $hwnd.ToInt64() }
    }
    'status' {
        $hwnd = Find-HerdrWindow
        $procs = @(Get-Process herdrplus, herdr -ErrorAction SilentlyContinue | ForEach-Object {
            @{ pid = $_.Id; name = $_.ProcessName; ram_mb = [math]::Round($_.WorkingSet64 / 1MB, 1) }
        })
        $server = $null
        try {
            $exe = Resolve-HerdrExe
            $server = (& $exe status --json | ConvertFrom-Json).server
        } catch { }
        Emit @{
            action    = 'status'
            window    = if ($hwnd -ne [IntPtr]::Zero) {
                @{ found = $true; hwnd = $hwnd.ToInt64(); maximized = [HerdrWin.Native]::IsZoomed($hwnd); minimized = [HerdrWin.Native]::IsIconic($hwnd) }
            } else { @{ found = $false } }
            processes = $procs
            ram_mb    = [math]::Round(($procs | Measure-Object -Property ram_mb -Sum).Sum, 1)
            server    = $server
        }
    }
}

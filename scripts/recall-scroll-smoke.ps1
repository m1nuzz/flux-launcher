#requires -Version 5.1
<#
.SYNOPSIS
    Regression smoke for the result viewport after a history recall.

.DESCRIPTION
    Two scenarios, each in its own launcher process with its own profile:

    fit   The launcher fits its window to the number of results. Rows are 46 DIP
          high and the scroll body adds 6 DIP above and below them. Fitting with
          a 48 DIP per-row pitch left one to five row lists with a small scroll
          range, so a recalled one-row answer could still be scrolled.
          Expected: a recalled calculator answer publishes exactly one row,
          restores the remembered top row and fits 94 + 46 + 12 = 152 DIP.

    offset
          The scroll offset lives on the scroll node and survives a list swap.
          Recalling a query after scrolling a longer list handed that offset to
          the recalled rows, so the top row - the one just recalled - was clipped
          by up to a row height. Expected: after the list is scrolled and the
          query changes, the viewport reports `scroll-top changed=1`, which is the
          anchor proving it moved a stale offset back to the first row.

    Input is posted, never synthesized into the session: the launcher must stay
    in the background. That is also why the offset scenario changes the query with
    a Backspace instead of recalling it: Alt and Ctrl are read from the real
    keyboard state, which posted messages do not touch, so Alt+Up cannot be driven
    this way. The offset it leaves behind is the same either way.

.PARAMETER Executable
    Path to the flux-launcher.exe under test.

.PARAMETER OutputDirectory
    Directory for the paint traces and the isolated profiles.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Executable,
    [string]$OutputDirectory = (Join-Path $env:TEMP 'flux-recall-scroll-smoke')
)

$ErrorActionPreference = 'Stop'

$Executable = [System.IO.Path]::GetFullPath($Executable)
$OutputDirectory = [System.IO.Path]::GetFullPath($OutputDirectory)
if (-not (Test-Path $Executable)) { throw "Executable not found: $Executable" }
New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null

Add-Type -Namespace RecallSmoke -Name Native -MemberDefinition @'
[DllImport("user32.dll")] public static extern bool PostMessage(IntPtr hWnd, uint msg, UIntPtr wParam, IntPtr lParam);
[DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam);
[DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);
[DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
[DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr arg);
[DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);
[DllImport("user32.dll")] public static extern short MapVirtualKey(uint vk, uint mapType);
public delegate bool EnumProc(IntPtr hWnd, IntPtr arg);
public struct RECT { public int Left; public int Top; public int Right; public int Bottom; }

public static IntPtr FindByPid(uint targetPid) {
    IntPtr best = IntPtr.Zero;
    long bestArea = 0;
    EnumWindows((hWnd, arg) => {
        uint pid;
        GetWindowThreadProcessId(hWnd, out pid);
        if (pid != targetPid) { return true; }
        RECT r;
        if (!GetWindowRect(hWnd, out r)) { return true; }
        long area = (long)(r.Right - r.Left) * (r.Bottom - r.Top);
        if (area > bestArea) { bestArea = area; best = hWnd; }
        return true;
    }, IntPtr.Zero);
    return best;
}

public static void TypeChar(IntPtr hWnd, char value) {
    PostMessage(hWnd, 0x0102, new UIntPtr((uint)value), IntPtr.Zero);
}

public static void PostVk(IntPtr hWnd, uint vk) {
    uint scan = (uint)MapVirtualKey(vk, 0);
    PostMessage(hWnd, 0x0100, new UIntPtr(vk), (IntPtr)(long)(scan << 16 | 1u));
    PostMessage(hWnd, 0x0101, new UIntPtr(vk), (IntPtr)(long)((scan << 16) | 1u | 0x40000000u | unchecked((int)0xC0000000u)));
}

public static void PostWheel(IntPtr hWnd, int screenX, int screenY, int delta) {
    uint wParam = (uint)((delta << 16) & 0xFFFF0000);
    IntPtr lParam = (IntPtr)((screenY << 16) | (screenX & 0xFFFF));
    PostMessage(hWnd, 0x020A, new UIntPtr(wParam), lParam);
}
'@

$script:previousAppData = $env:APPDATA

function Get-TraceEvents([string]$path) {
    if (-not (Test-Path $path)) { return @() }
    $events = @()
    foreach ($line in Get-Content -Path $path) {
        if ($line -match '^t=(?<t>\d+) unix=(?<unix>\d+) seq=(?<seq>\d+) event=(?<event>\S+) ?(?<detail>.*)$') {
            $events += [pscustomobject]@{
                t = [long]$matches['t']
                unix = [long]$matches['unix']
                seq = [long]$matches['seq']
                event = $matches['event']
                detail = $matches['detail']
            }
        }
    }
    return $events
}

function Start-LauncherRun([string]$name, [string[]]$history, [hashtable]$selections) {
    $runDirectory = Join-Path $OutputDirectory ('run-' + $name + '-' + [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds())
    $settingsDirectory = Join-Path (Join-Path $runDirectory 'appdata') 'FluxLauncher'
    New-Item -ItemType Directory -Force -Path $settingsDirectory | Out-Null

    $settings = [ordered]@{
        auto_enable_everything = $false
        update_checks_enabled = $false
        start_with_windows = $false
        ignore_hotkeys_in_fullscreen = $false
        launcher_height = 382
        query_history = @($history)
        history_selections = $selections
    }
    [System.IO.File]::WriteAllText(
        (Join-Path $settingsDirectory 'settings.json'),
        ($settings | ConvertTo-Json -Depth 4),
        (New-Object System.Text.UTF8Encoding($false))
    )

    $tracePath = Join-Path $runDirectory 'paint.trace'
    $env:APPDATA = Join-Path $runDirectory 'appdata'
    $env:FLUX_DISABLE_SINGLE_INSTANCE = '1'
    $env:FLUX_DISABLE_UPDATE_CHECKS = '1'
    $env:FLUX_DISABLE_EVERYTHING_PROMPT = '1'
    $env:WINDUI_SMOKE_NO_FOREGROUND = '1'
    $env:FLUX_PAINT_TRACE_FILE = $tracePath

    $info = New-Object System.Diagnostics.ProcessStartInfo
    $info.FileName = $Executable
    $info.UseShellExecute = $false
    $info.WorkingDirectory = Split-Path -Parent $Executable
    $process = [System.Diagnostics.Process]::Start($info)

    $handle = [IntPtr]::Zero
    for ($attempt = 0; $attempt -lt 80; $attempt++) {
        $handle = [RecallSmoke.Native]::FindByPid([uint32]$process.Id)
        if ($handle -ne [IntPtr]::Zero) { break }
        Start-Sleep -Milliseconds 100
    }
    if ($handle -eq [IntPtr]::Zero) {
        $process.Kill()
        throw "launcher window never appeared ($name)"
    }

    # A hosted runner can create the window before its hotkey callback is wired.
    # Post the activation message again instead of failing on the first race.
    for ($send = 0; $send -lt 20; $send++) {
        [void][RecallSmoke.Native]::SendMessage($handle, 0x0312, [IntPtr]::Zero, [IntPtr]::Zero)
        for ($attempt = 0; $attempt -lt 10; $attempt++) {
            if ([RecallSmoke.Native]::IsWindowVisible($handle)) { break }
            Start-Sleep -Milliseconds 50
        }
        if ([RecallSmoke.Native]::IsWindowVisible($handle)) { break }
        Start-Sleep -Milliseconds 100
    }
    if (-not [RecallSmoke.Native]::IsWindowVisible($handle)) {
        $process.Kill()
        throw "launcher did not become visible after WM_HOTKEY ($name)"
    }
    Start-Sleep -Milliseconds 500

    [pscustomobject]@{
        Name = $name
        Process = $process
        Handle = $handle
        TracePath = $tracePath
        RunDirectory = $runDirectory
    }
}

function Stop-LauncherRun($run) {
    if ($null -ne $run -and $null -ne $run.Process -and -not $run.Process.HasExited) {
        $run.Process.Kill()
        $run.Process.WaitForExit(3000) | Out-Null
    }
}

function Send-Text([IntPtr]$handle, [string]$text) {
    foreach ($char in $text.ToCharArray()) {
        [RecallSmoke.Native]::TypeChar($handle, $char)
        Start-Sleep -Milliseconds 25
    }
}

function Send-Key([IntPtr]$handle, [uint32]$virtualKey, [int]$times = 1) {
    for ($i = 0; $i -lt $times; $i++) {
        [RecallSmoke.Native]::PostVk($handle, $virtualKey)
        Start-Sleep -Milliseconds 25
    }
}

function Wait-ForEvent([string]$path, [scriptblock]$predicate, [int]$attempts = 160) {
    for ($attempt = 0; $attempt -lt $attempts; $attempt++) {
        $hit = @(@(Get-TraceEvents $path) | Where-Object $predicate | Select-Object -Last 1)
        if ($hit.Count -gt 0) { return $hit[0] }
        Start-Sleep -Milliseconds 50
    }
    return $null
}

function Get-EventDetailInt([string]$detail, [string]$pattern, [string]$what) {
    # The group is named: a pattern made only of named groups gets no numeric slot,
    # so `$matches[1]` would read as an empty string here.
    if ($detail -notmatch $pattern) { throw "could not read $what from '$detail'" }
    return [int]$matches['n']
}

# --- fit: a recalled one-row answer must not keep a scroll range -------------
$fitHistory = '987654321+123456789'
$fitRun = $null
try {
    $fitRun = Start-LauncherRun 'fit' @($fitHistory) @{ $fitHistory.ToLowerInvariant() = 'builtin:calculator' }
    Send-Text $fitRun.Handle 'settings'
    Start-Sleep -Milliseconds 900
    Send-Key $fitRun.Handle 0x28 8
    Start-Sleep -Milliseconds 250
    Send-Key $fitRun.Handle 0x08 8
    Start-Sleep -Milliseconds 250

    $recallUnix = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
    Send-Key $fitRun.Handle 0x26

    $write = Wait-ForEvent $fitRun.TracePath {
        $_.event -eq 'list-write' -and $_.unix -ge $recallUnix -and
        $_.detail -like "query=$fitHistory *" -and $_.detail -like '*rows=1 *'
    }
    if ($null -eq $write) { throw "no list publish for recalled query '$fitHistory'" }

    # The first resize after the recall only accounts for the empty built-in list.
    # The provider commit that adds the calculator row triggers the fitted resize
    # this scenario needs, and it arrives a frame after the list write.
    $resize = Wait-ForEvent $fitRun.TracePath {
        $_.event -eq 'resize' -and $_.detail -like '*fitted=true*' -and
        ($_.unix -gt $write.unix -or ($_.unix -eq $write.unix -and $_.seq -gt $write.seq))
    } 80
    if ($null -eq $resize) { throw "no fitted resize after recalling '$fitHistory'" }

    $rows = Get-EventDetailInt $write.detail 'rows=(?<n>\d+)' 'the row count'
    if ($rows -ne 1) {
        throw "recalled query '$fitHistory' published $rows rows; the scenario needs exactly one"
    }
    if ($write.detail -notmatch 'head=(?<head>.*?) selected=(?<selected>.*)@(?<index>\d+)$') {
        throw "could not read the selection from '$($write.detail)'"
    }
    $head = $matches['head']
    $selected = $matches['selected']
    if ($head -ne 'builtin:calculator' -or $selected -ne 'builtin:calculator') {
        throw "the recall restored '$selected' instead of the remembered top row '$head'"
    }

    $height = 0
    if ($resize.detail -notmatch 'size=\d+x(?<height>\d+)') {
        throw "could not read the fitted size from '$($resize.detail)'"
    }
    $height = [int]$matches['height']
    if ($height -ne 152) {
        throw "one result fitted to $height DIP instead of 152 DIP; the viewport keeps a stale scroll range"
    }
    Write-Host "PASS fit: recalled '$fitHistory' selected $selected and fitted one row to $height DIP."
} finally {
    Stop-LauncherRun $fitRun
}

# --- offset: a new list must start on its first row --------------------------
# A prefix inside a big folder answers with many sibling rows, so the list is
# longer than the viewport on any Windows install and the offset left behind by
# the previous list really matters. The wheel scrolls it without touching the
# selection, which is what a recall leaves behind too.
$offsetQuery = 'C:\Windows\s'
$offsetRun = $null
try {
    $offsetRun = Start-LauncherRun 'offset' @($offsetQuery) @{}
    Send-Text $offsetRun.Handle $offsetQuery
    Start-Sleep -Milliseconds 1500

    $scrolled = @(@(Get-TraceEvents $offsetRun.TracePath) |
        Where-Object { $_.event -eq 'list-write' -and $_.detail -like "query=$offsetQuery *" } |
        Select-Object -Last 1)
    if ($scrolled.Count -eq 0) {
        throw "'$offsetQuery' never published; the scenario needs a longer list to scroll"
    }
    $rows = Get-EventDetailInt $scrolled[0].detail 'rows=(?<n>\d+)' 'the row count'
    if ($rows -le 6) {
        throw "'$offsetQuery' published $rows rows; the list has to be longer than the viewport"
    }

    $rect = New-Object RecallSmoke.Native+RECT
    [void][RecallSmoke.Native]::GetWindowRect($offsetRun.Handle, [ref]$rect)
    $wheelX = [int](($rect.Left + $rect.Right) / 2)
    $wheelY = [int]($rect.Top + 150)
    for ($i = 0; $i -lt 5; $i++) {
        [RecallSmoke.Native]::PostWheel($offsetRun.Handle, $wheelX, $wheelY, -120)
        Start-Sleep -Milliseconds 80
    }
    Start-Sleep -Milliseconds 400

    # One Backspace is enough: it is a new query, so a new list. The selection was
    # never touched by the wheel, so nothing else asks for a scroll here.
    $changeUnix = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
    Send-Key $offsetRun.Handle 0x08

    $top = Wait-ForEvent $offsetRun.TracePath {
        $_.event -eq 'scroll-top' -and $_.unix -ge $changeUnix
    } 80
    if ($null -eq $top) {
        throw 'no scroll-top for the new query; the viewport is not being reset at all'
    }
    if ($top.detail -notmatch 'changed=1') {
        throw "the new list kept the scrolled viewport ($($top.detail))"
    }
    Write-Host "PASS offset: a $rows row list was scrolled and the next query started back on its first row."
} finally {
    Stop-LauncherRun $offsetRun
    $env:APPDATA = $script:previousAppData
    foreach ($name in @('FLUX_DISABLE_SINGLE_INSTANCE', 'FLUX_DISABLE_UPDATE_CHECKS', 'FLUX_DISABLE_EVERYTHING_PROMPT', 'WINDUI_SMOKE_NO_FOREGROUND', 'FLUX_PAINT_TRACE_FILE')) {
        Remove-Item "Env:$name" -ErrorAction SilentlyContinue
    }
}

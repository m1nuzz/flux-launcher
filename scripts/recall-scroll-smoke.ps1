#requires -Version 5.1
<#
.SYNOPSIS
    Regression smoke for a history recall that clipped the top result.

.DESCRIPTION
    The launcher fits its window to the number of results. The result rows are
    46 DIP high and the scroll body adds 6 DIP above and below them. Fitting the
    window with a 48 DIP per-row pitch left one to five row lists with a small
    scroll range; a scroll offset retained from the previous query then clipped
    the restored top row after a history recall.

    This smoke drives the real window with posted input:
      1. seed history with a one-result calculator expression whose top row is
         the remembered selection,
      2. type a longer query and move the selection down so the result viewport
         is scrolled,
      3. clear the field and press Up to recall the calculator query,
      4. require the published list to restore the remembered top row and the
         fitted window to be exactly one row plus padding tall.

    The expected height is the one the fixed geometry produces: 94 DIP chrome +
    46 DIP row + 12 DIP list padding = 152 DIP. The old 48 DIP pitch produced
    142 DIP and left ten pixels of scroll range.

.PARAMETER Executable
    Path to the flux-launcher.exe under test.

.PARAMETER OutputDirectory
    Directory for the paint trace and the isolated profile.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Executable,
    [string]$RecalledQuery = '987654321+123456789',
    [string]$LongQuery = 'settings',
    [int]$DownPresses = 8,
    [string]$OutputDirectory = (Join-Path $env:TEMP 'flux-recall-scroll-smoke')
)

$ErrorActionPreference = 'Stop'

$Executable = [System.IO.Path]::GetFullPath($Executable)
$OutputDirectory = [System.IO.Path]::GetFullPath($OutputDirectory)
if (-not (Test-Path $Executable)) { throw "Executable not found: $Executable" }
New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
$runDirectory = Join-Path $OutputDirectory ('run-' + [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds())
New-Item -ItemType Directory -Force -Path $runDirectory | Out-Null

$tracePath = Join-Path $runDirectory 'paint.trace'
$scratchAppData = Join-Path $runDirectory 'appdata'
$settingsDirectory = Join-Path $scratchAppData 'FluxLauncher'
New-Item -ItemType Directory -Force -Path $settingsDirectory | Out-Null

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
'@

function Get-TraceEvents {
    if (-not (Test-Path $tracePath)) { return @() }
    $events = @()
    foreach ($line in Get-Content -Path $tracePath) {
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

function Start-FluxProcess {
    $info = New-Object System.Diagnostics.ProcessStartInfo
    $info.FileName = $Executable
    $info.UseShellExecute = $false
    $info.WorkingDirectory = Split-Path -Parent $Executable
    return [System.Diagnostics.Process]::Start($info)
}

function Wait-ForWindow([System.Diagnostics.Process]$process) {
    for ($attempt = 0; $attempt -lt 80; $attempt++) {
        $handle = [RecallSmoke.Native]::FindByPid([uint32]$process.Id)
        if ($handle -ne [IntPtr]::Zero) { return $handle }
        Start-Sleep -Milliseconds 100
    }
    throw 'launcher window never appeared'
}

function Show-Launcher([IntPtr]$handle) {
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
        throw 'launcher did not become visible after WM_HOTKEY'
    }
    Start-Sleep -Milliseconds 500
}

function Send-Text([IntPtr]$handle, [string]$text) {
    foreach ($char in $text.ToCharArray()) {
        [RecallSmoke.Native]::TypeChar($handle, $char)
        Start-Sleep -Milliseconds 25
    }
}

$settings = [ordered]@{
    auto_enable_everything = $false
    update_checks_enabled = $false
    start_with_windows = $false
    ignore_hotkeys_in_fullscreen = $false
    launcher_height = 382
    query_history = @($RecalledQuery)
    history_selections = @{ $RecalledQuery.ToLowerInvariant() = 'builtin:calculator' }
}
$settingsJson = $settings | ConvertTo-Json -Depth 4
[System.IO.File]::WriteAllText(
    (Join-Path $settingsDirectory 'settings.json'),
    $settingsJson,
    (New-Object System.Text.UTF8Encoding($false))
)

$previousAppData = $env:APPDATA
$env:APPDATA = $scratchAppData
$env:FLUX_DISABLE_SINGLE_INSTANCE = '1'
$env:FLUX_DISABLE_UPDATE_CHECKS = '1'
$env:FLUX_DISABLE_EVERYTHING_PROMPT = '1'
$env:WINDUI_SMOKE_NO_FOREGROUND = '1'
$env:FLUX_PAINT_TRACE_FILE = $tracePath

$process = $null
try {
    $process = Start-FluxProcess
    $handle = Wait-ForWindow $process
    Show-Launcher $handle

    Send-Text $handle $LongQuery
    Start-Sleep -Milliseconds 900
    for ($i = 0; $i -lt $DownPresses; $i++) {
        [RecallSmoke.Native]::PostVk($handle, 0x28)
        Start-Sleep -Milliseconds 25
    }
    Start-Sleep -Milliseconds 250

    for ($i = 0; $i -lt $LongQuery.Length; $i++) {
        [RecallSmoke.Native]::PostVk($handle, 0x08)
        Start-Sleep -Milliseconds 25
    }
    Start-Sleep -Milliseconds 250

    $recallUnix = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
    [RecallSmoke.Native]::PostVk($handle, 0x26)

    $recallWrite = @()
    for ($attempt = 0; $attempt -lt 160; $attempt++) {
        $events = @(Get-TraceEvents)
        $recallWrite = @($events | Where-Object {
            $_.event -eq 'list-write' -and $_.unix -ge $recallUnix -and
            $_.detail -like "query=$RecalledQuery *"
        } | Select-Object -Last 1)
        if ($recallWrite.Count -gt 0) { break }
        Start-Sleep -Milliseconds 50
    }
    if ($recallWrite.Count -eq 0) {
        throw "no list publish for recalled query '$RecalledQuery'"
    }

    # The first resize after recall only accounts for the empty built-in list.
    # The provider commit that adds the calculator row triggers the fitted resize
    # this smoke needs, and it arrives a frame after the list write.
    $write = $recallWrite[0]
    $recallResize = @()
    for ($attempt = 0; $attempt -lt 80; $attempt++) {
        $events = @(Get-TraceEvents)
        $recallResize = @($events | Where-Object {
            $_.event -eq 'resize' -and $_.detail -like '*fitted=true*' -and
            ($_.unix -gt $write.unix -or ($_.unix -eq $write.unix -and $_.seq -gt $write.seq))
        } | Select-Object -Last 1)
        if ($recallResize.Count -gt 0) { break }
        Start-Sleep -Milliseconds 50
    }
    if ($recallResize.Count -eq 0) {
        throw "no fitted resize after recalling '$RecalledQuery'"
    }

    $detail = $recallWrite[0].detail
    if ($detail -notmatch 'rows=(?<rows>\d+)') {
        throw "could not read row count from '$detail'"
    }
    $rows = [int]$matches['rows']
    if ($rows -ne 1) {
        throw "recalled query '$RecalledQuery' published $rows rows; the smoke needs exactly one"
    }
    if ($detail -notmatch 'head=(?<head>.*?) selected=(?<selected>.*)@(?<index>\d+)$') {
        throw "could not read selection from '$detail'"
    }
    $head = $matches['head']
    $selected = $matches['selected']
    if ($head -ne 'builtin:calculator' -or $selected -ne 'builtin:calculator') {
        throw "recall restored '$selected' instead of the remembered top row '$head'"
    }

    $resizeDetail = $recallResize[0].detail
    if ($resizeDetail -notmatch 'size=(?<width>\d+)x(?<height>\d+)') {
        throw "could not read fitted size from '$resizeDetail'"
    }
    $height = [int]$matches['height']
    if ($height -ne 152) {
        throw "one result fitted to ${height} DIP instead of 152 DIP; the viewport keeps a stale scroll range"
    }

    Write-Host "PASS: recalled '$RecalledQuery' selected $selected and fitted one row to ${height} DIP."
    Write-Host "trace: $tracePath"
} finally {
    if ($null -ne $process -and -not $process.HasExited) {
        $process.Kill()
        $process.WaitForExit(3000) | Out-Null
    }
    $env:APPDATA = $previousAppData
    Remove-Item Env:FLUX_DISABLE_SINGLE_INSTANCE -ErrorAction SilentlyContinue
    Remove-Item Env:FLUX_DISABLE_UPDATE_CHECKS -ErrorAction SilentlyContinue
    Remove-Item Env:FLUX_DISABLE_EVERYTHING_PROMPT -ErrorAction SilentlyContinue
    Remove-Item Env:WINDUI_SMOKE_NO_FOREGROUND -ErrorAction SilentlyContinue
    Remove-Item Env:FLUX_PAINT_TRACE_FILE -ErrorAction SilentlyContinue
}

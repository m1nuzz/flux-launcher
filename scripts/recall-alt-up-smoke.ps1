#requires -Version 5.1
<#
.SYNOPSIS
    Drives a real Alt+Up history recall and asserts where the result viewport ends up.

.DESCRIPTION
    The reported bug: a longer list is scrolled down, then Alt+Up recalls an earlier
    query. The recalled list must start on its first row - the previous list's
    offset must not be handed to it - otherwise the row the user recalled, the top
    one, is clipped and the only way to read it is to drag the list up by hand.

    Two scenarios, because there are two ways a recall can leave the viewport
    where it was:

      other - the field holds something else, so the recall changes the query.
      same  - the field already holds the query the recall names, so the query
              text does not change at all and nothing but the recall itself can
              reset the viewport. This is the reported one: the user had picked
              the topmost result of that query before, and the list they were
              looking at was scrolled.

    Phase 1 discovers which row is first for the recalled query, so the scenarios
    can store it as that query's remembered selection - "I had picked the topmost
    result".

    Alt cannot be posted: the launcher reads it from `GetKeyState`, which only sees
    real input, and synthesized input needs the foreground, which a smoke must not
    take. So the driver points `FLUX_SMOKE_ALT_FILE` at a marker file and creates it
    for the length of one Up press: the launcher treats Alt as held for exactly that
    keystroke and for nothing else.

    The window is invisible to the user for its whole life: `WINDUI_SMOKE_PARK=1`
    makes the launcher park itself off every monitor, click-through and
    non-activating, so a run cannot take focus, cover another window, or be clicked
    by accident. The script asserts that from the window rect, so a regression in
    the hook fails the smoke instead of quietly covering the desktop.

    The launcher records `scroll-offset y=<n> max=<n>` whenever the viewport moves
    (`FLUX_PAINT_TRACE_FILE`), so the assertion is the real offset, not a guess from
    geometry: after the recall the offset must be 0 and stay 0.

.PARAMETER Executable
    Path to the flux-launcher.exe under test.

.PARAMETER OutputDirectory
    Directory for the paint traces and the isolated profile.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Executable,
    # File queries, not app queries: a bare Windows machine and a developer's
    # Start Menu answer these with the same long listing, so the scenario keeps
    # both lists longer than the viewport wherever it runs.
    [string]$RecalledQuery = 'C:\Windows\sy',
    [string]$ScrolledQuery = 'C:\Windows\s',
    [string]$OutputDirectory = (Join-Path $env:TEMP 'flux-alt-up-smoke')
)

$ErrorActionPreference = 'Stop'

$Executable = [System.IO.Path]::GetFullPath($Executable)
$OutputDirectory = [System.IO.Path]::GetFullPath($OutputDirectory)
if (-not (Test-Path $Executable)) { throw "Executable not found: $Executable" }
New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null

Add-Type -Namespace AltUpSmoke -Name Native -MemberDefinition @'
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

function Get-TraceEvents {
    param([Parameter(Mandatory = $true)][string]$Path)
    if (-not (Test-Path $Path)) { return @() }
    $events = @()
    foreach ($line in Get-Content -Path $Path) {
        if ($line -match '^t=(?<t>\d+) unix=(?<unix>\d+) seq=(?<seq>\d+) event=(?<event>\S+) ?(?<detail>.*)$') {
            $events += [pscustomobject]@{
                unix = [long]$matches['unix']
                seq = [long]$matches['seq']
                event = $matches['event']
                detail = $matches['detail']
            }
        }
    }
    return $events
}

function Show-TraceTail {
    param([Parameter(Mandatory = $true)][string]$Path)
    if (-not (Test-Path $Path)) { return }
    Write-Host '--- last trace events ---'
    Get-Content -Path $Path |
        Where-Object { $_ -match 'list-write|scroll-|query |resize' } |
        Select-Object -Last 24 |
        ForEach-Object { Write-Host $_ }
}

function Send-Text {
    param([IntPtr]$Handle, [string]$Text)
    foreach ($char in $Text.ToCharArray()) {
        [AltUpSmoke.Native]::TypeChar($Handle, $char)
        Start-Sleep -Milliseconds 25
    }
}

<#
    Starts the launcher for one phase: isolated profile, paint trace, parked window.
    Returns the process, the window handle and the trace path.
#>
function Start-Phase {
    param(
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][hashtable]$Settings
    )
    $runDirectory = Join-Path $OutputDirectory ('run-' + $Name + '-' + [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds())
    $settingsDirectory = Join-Path (Join-Path $runDirectory 'appdata') 'FluxLauncher'
    New-Item -ItemType Directory -Force -Path $settingsDirectory | Out-Null
    $tracePath = Join-Path $runDirectory 'paint.trace'
    [System.IO.File]::WriteAllText(
        (Join-Path $settingsDirectory 'settings.json'),
        ($Settings | ConvertTo-Json -Depth 4),
        (New-Object System.Text.UTF8Encoding($false))
    )

    $env:APPDATA = Join-Path $runDirectory 'appdata'
    # The child inherits the environment at spawn time, so the trace path has to be
    # in place before it starts - setting it afterwards leaves the launcher with no
    # trace at all.
    $env:FLUX_PAINT_TRACE_FILE = $tracePath
    $info = New-Object System.Diagnostics.ProcessStartInfo
    $info.FileName = $Executable
    $info.UseShellExecute = $false
    $info.WorkingDirectory = Split-Path -Parent $Executable
    $process = [System.Diagnostics.Process]::Start($info)

    $handle = [IntPtr]::Zero
    for ($attempt = 0; $attempt -lt 80; $attempt++) {
        if ($process.HasExited) { throw "the launcher exited with code $($process.ExitCode)" }
        $handle = [AltUpSmoke.Native]::FindByPid([uint32]$process.Id)
        if ($handle -ne [IntPtr]::Zero) { break }
        Start-Sleep -Milliseconds 100
    }
    if ($handle -eq [IntPtr]::Zero) { throw 'launcher window never appeared' }

    for ($send = 0; $send -lt 20; $send++) {
        [void][AltUpSmoke.Native]::SendMessage($handle, 0x0312, [IntPtr]::Zero, [IntPtr]::Zero)
        for ($attempt = 0; $attempt -lt 10; $attempt++) {
            if ([AltUpSmoke.Native]::IsWindowVisible($handle)) { break }
            Start-Sleep -Milliseconds 50
        }
        if ([AltUpSmoke.Native]::IsWindowVisible($handle)) { break }
        Start-Sleep -Milliseconds 100
    }
    if (-not [AltUpSmoke.Native]::IsWindowVisible($handle)) {
        throw 'launcher did not become visible after WM_HOTKEY'
    }

    # The user must never see the smoke: the hook parks the window off every
    # monitor, and this is the check that the hook still works.
    $rect = New-Object AltUpSmoke.Native+RECT
    [void][AltUpSmoke.Native]::GetWindowRect($handle, [ref]$rect)
    if ($rect.Left -gt -20000 -or $rect.Top -gt -20000) {
        throw "the smoke window is on screen at $($rect.Left),$($rect.Top); WINDUI_SMOKE_PARK did not take effect"
    }

    return [pscustomobject]@{
        Process = $process
        Handle  = $handle
        Trace   = $tracePath
        Rect    = $rect
    }
}

function Stop-Phase {
    param($Phase)
    if ($null -ne $Phase -and -not $Phase.Process.HasExited) {
        $Phase.Process.Kill()
        $Phase.Process.WaitForExit(3000) | Out-Null
    }
}

<#
    Waits until the trace shows a `list-write` for `Query` and returns that event.
#>
function Wait-ListWrite {
    param(
        [Parameter(Mandatory = $true)][string]$Trace,
        [Parameter(Mandatory = $true)][string]$Query,
        [long]$SinceUnix = 0,
        # The launcher publishes the built-in rows before the query's own results
        # arrive, so a publish with too few rows is not the one being waited for.
        [int]$MinRows = 1,
        [int]$TimeoutMs = 10000
    )
    $deadline = [DateTime]::UtcNow.AddMilliseconds($TimeoutMs)
    $last = $null
    while ([DateTime]::UtcNow -lt $deadline) {
        $hit = @(@(Get-TraceEvents -Path $Trace) | Where-Object {
            $_.event -eq 'list-write' -and $_.unix -ge $SinceUnix -and
            $_.detail -like "query=$Query *"
        } | Select-Object -Last 1)
        if ($hit.Count -gt 0) {
            $last = $hit[0]
            $rows = Get-DetailInt -Detail $last.detail -Name 'rows'
            if ($null -ne $rows -and $rows -ge $MinRows) { return $last }
        }
        Start-Sleep -Milliseconds 50
    }
    # Returning the best attempt lets the caller report the row count it saw.
    return $last
}

function Get-DetailInt {
    param([string]$Detail, [string]$Name)
    if ($Detail -match "$Name=(?<n>-?\d+)") { return [int]$matches['n'] }
    return $null
}

$previousAppData = $env:APPDATA
$altMarker = $null
$env:FLUX_DISABLE_SINGLE_INSTANCE = '1'
$env:FLUX_DISABLE_UPDATE_CHECKS = '1'
$env:FLUX_DISABLE_EVERYTHING_PROMPT = '1'
$env:WINDUI_SMOKE_NO_FOREGROUND = '1'
$env:WINDUI_SMOKE_PARK = '1'

$phase = $null
try {
    # ── Phase 1: which row is first for the recalled query ──────────────────────
    # The reported case is "I had picked the topmost result of that query", so the
    # recall restores a remembered selection instead of falling back to row 0.
    $phase = Start-Phase -Name 'discover' -Settings ([ordered]@{
        auto_enable_everything      = $false
        update_checks_enabled       = $false
        start_with_windows          = $false
        ignore_hotkeys_in_fullscreen = $false
        launcher_height             = 382
        query_history               = @()
        history_selections          = @{}
    })

    Send-Text $phase.Handle $RecalledQuery
    $discovered = Wait-ListWrite -Trace $phase.Trace -Query $RecalledQuery -MinRows 1
    if ($null -eq $discovered) { throw "'$RecalledQuery' never published, so its first row is unknown" }
    if ($discovered.detail -notmatch 'head=(?<id>.+?) selected=') {
        throw "could not read the first row id from '$($discovered.detail)'"
    }
    $topRowId = $matches['id']
    Stop-Phase $phase
    $phase = $null

    <#
        One scenario: type a query, scroll the list, Alt+Up, and check where the
        viewport ended up.
    #>
    function Invoke-RecallScenario {
        param(
            [Parameter(Mandatory = $true)][string]$Name,
            [Parameter(Mandatory = $true)][string]$Typed,
            [Parameter(Mandatory = $true)][string]$RememberedId
        )

        $scenarioMarker = Join-Path $OutputDirectory ('alt-held-' + $Name + '-' + [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds())
        $env:FLUX_SMOKE_ALT_FILE = $scenarioMarker
        $scenarioPhase = Start-Phase -Name $Name -Settings ([ordered]@{
            auto_enable_everything      = $false
            update_checks_enabled       = $false
            start_with_windows          = $false
            ignore_hotkeys_in_fullscreen = $false
            launcher_height             = 382
            query_history               = @($RecalledQuery)
            history_selections          = @{ $RecalledQuery.ToLowerInvariant() = $RememberedId }
        })
        try {
            Send-Text $scenarioPhase.Handle $Typed
            $published = Wait-ListWrite -Trace $scenarioPhase.Trace -Query $Typed -MinRows 7
            if ($null -eq $published) { throw "'$Typed' never published" }
            $rows = Get-DetailInt -Detail $published.detail -Name 'rows'
            if ($null -eq $rows -or $rows -le 6) {
                throw "'$Typed' published $rows rows; the list has to be longer than the viewport to scroll"
            }

            # The wheel scrolls without touching the selection, which is exactly the
            # state a recall inherits: the previous list is scrolled, the highlight
            # is unchanged.
            $wheelX = [int](($scenarioPhase.Rect.Left + $scenarioPhase.Rect.Right) / 2)
            $wheelY = [int]($scenarioPhase.Rect.Top + 150)
            for ($i = 0; $i -lt 5; $i++) {
                [AltUpSmoke.Native]::PostWheel($scenarioPhase.Handle, $wheelX, $wheelY, -120)
                Start-Sleep -Milliseconds 80
            }
            Start-Sleep -Milliseconds 300

            $before = @(@(Get-TraceEvents -Path $scenarioPhase.Trace) |
                    Where-Object { $_.event -eq 'scroll-offset' } |
                    Select-Object -Last 1)
            if ($before.Count -eq 0) { throw 'the scrolled list never reported its viewport offset' }
            $offsetBefore = Get-DetailInt -Detail $before[0].detail -Name 'y'
            if ($null -eq $offsetBefore) { throw "could not read the offset from '$($before[0].detail)'" }
            if ($offsetBefore -eq 0) {
                throw 'the scrolled list is still on its first row; the scenario has no offset to inherit'
            }

            $recallUnix = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
            New-Item -ItemType File -Force -Path $scenarioMarker | Out-Null
            Start-Sleep -Milliseconds 40
            [AltUpSmoke.Native]::PostVk($scenarioPhase.Handle, 0x26)
            Start-Sleep -Milliseconds 200
            Remove-Item -Path $scenarioMarker -Force -ErrorAction SilentlyContinue

            # The reset runs when the query changes - or, when the recall names the
            # query that is already in the field, when the recall itself asks for
            # it. Either way the trace shows it as a scroll-top for that query.
            $reset = $null
            for ($attempt = 0; $attempt -lt 120; $attempt++) {
                $hit = @(@(Get-TraceEvents -Path $scenarioPhase.Trace) | Where-Object {
                    $_.event -eq 'scroll-top' -and $_.unix -ge $recallUnix -and
                    $_.detail -like "query=$RecalledQuery *"
                } | Select-Object -Last 1)
                if ($hit.Count -gt 0) { $reset = $hit[0]; break }
                Start-Sleep -Milliseconds 50
            }
            if ($null -eq $reset) {
                throw "the Alt+Up recall of '$RecalledQuery' never reset the viewport; it did not run at all"
            }

            # Whatever the offset is now is what the user sees. It must be the first
            # row, and nothing after the reset may have moved it away again.
            Start-Sleep -Milliseconds 600
            $after = @(@(Get-TraceEvents -Path $scenarioPhase.Trace) | Where-Object {
                $_.event -eq 'scroll-offset' -and $_.unix -ge $reset.unix
            })
            if ($after.Count -eq 0) {
                throw 'the recalled list never reported a viewport offset; the reset cannot be verified'
            }
            $offsetAfter = Get-DetailInt -Detail $after[-1].detail -Name 'y'
            if ($null -eq $offsetAfter) { throw "could not read the offset from '$($after[-1].detail)'" }
            if ($offsetAfter -ne 0) {
                throw "the recalled list ended at a $offsetAfter DIP offset (was $offsetBefore before the recall); its top row is clipped"
            }

            Write-Host "PASS alt-up [$Name]: '$Typed' scrolled to $offsetBefore DIP, Alt+Up recalled '$RecalledQuery' and the viewport is back on its first row."
            Write-Host "trace: $($scenarioPhase.Trace)"
        } finally {
            Stop-Phase $scenarioPhase
            Remove-Item -Path $scenarioMarker -Force -ErrorAction SilentlyContinue
        }
    }

    # The field holds another query, so the recall changes it.
    Invoke-RecallScenario -Name 'other' -Typed $ScrolledQuery -RememberedId $topRowId
    # The field already holds the query the recall names, so the query text does
    # not change and only the recall itself can reset the viewport. This is the
    # reported case.
    Invoke-RecallScenario -Name 'same' -Typed $RecalledQuery -RememberedId $topRowId
} catch {
    Write-Host "FAIL: $($_.Exception.Message)"
    if ($null -ne $phase) { Show-TraceTail -Path $phase.Trace }
    throw
} finally {
    Stop-Phase $phase
    if ($null -ne $altMarker) { Remove-Item -Path $altMarker -Force -ErrorAction SilentlyContinue }
    $env:APPDATA = $previousAppData
    foreach ($name in @('FLUX_DISABLE_SINGLE_INSTANCE', 'FLUX_DISABLE_UPDATE_CHECKS', 'FLUX_DISABLE_EVERYTHING_PROMPT', 'FLUX_PAINT_TRACE_FILE', 'FLUX_SMOKE_ALT_FILE', 'WINDUI_SMOKE_NO_FOREGROUND', 'WINDUI_SMOKE_PARK')) {
        Remove-Item "Env:$name" -ErrorAction SilentlyContinue
    }
}

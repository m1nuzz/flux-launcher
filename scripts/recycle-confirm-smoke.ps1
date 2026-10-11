# Smoke: the Recycle Bin confirmation flow must raise exactly one native prompt per
# gesture and must never re-raise it on a later activation.
#
# Background: the launcher hides, spawns a short-lived `--empty-recycle-confirm` child
# that owns the Windows shell prompt, and reaps it on the first search tick after the
# next activation. The regression this guards is the parent emptying the bin a second
# time after the child already did: that re-pop of the dialog, with the launcher still
# visible, re-entered the interval callback through the modal message pump and spawned
# yet another child, so every later activation showed the dialog again 3-4 times.
#
# The smoke drives the REAL spawn/reap state machine directly, with no synthesised
# keystrokes or mouse clicks (those are unreliable on a runner whose active keyboard
# layout, focus or headless session differs from a developer's desktop, and a wrong
# character once turned the launcher's query into Cyrillic text). It sets three
# env hooks that interval_tick.rs honours only under FLUX_SMOKE_*:
#
#   FLUX_SMOKE_RECYCLE_NO_HIDE  - keep the interval timers alive after the child spawns
#                                 (normally the hide kills them until re-activation) so
#                                 the parent reaps the child on a later tick and the
#                                 whole flow runs headless.
#   FLUX_SMOKE_RECYCLE_ARM      - arm the confirmation at startup (one gesture).
#   FLUX_SMOKE_RECYCLE_REARM    - after each reap, re-arm this many more times, standing
#                                 in for the "user opens the launcher again" gesture.
#
# The child's outcome is driven by FLUX_SMOKE_RECYCLE_CONFIRM (already honoured in
# recycle_confirm.rs): it exits 0 for "emptied" and 1 for "declined" without touching
# the real bin, so the smoke never deletes anything.
#
# Phases:
#   1. arm once, declined: exactly one spawn, one "declined", and no further spawn.
#   2. arm once, emptied:  exactly one spawn, one "emptied".
#   3. arm once + re-arm once, declined: exactly two spawns and two "declined" - this is
#      the regression assertion. A buggy build that re-empties on reap or forgets to
#      reset the flag would cascade into many spawns here.

[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$Executable,
    [Parameter(Mandatory = $true)]
    [string]$WorkDirectory
)

$ErrorActionPreference = 'Stop'
# Resolve to an absolute path: the launcher is launched with its working directory set
# to the build output folder, so a relative trace path would be created there and the
# smoke would read an empty file at the expected location.
$WorkDirectory = [System.IO.Path]::GetFullPath($WorkDirectory)
New-Item -ItemType Directory -Force -Path $WorkDirectory | Out-Null

$trace = Join-Path $WorkDirectory 'recycle-trace.log'
$appData = Join-Path $WorkDirectory 'AppData'
$processName = [System.IO.Path]::GetFileNameWithoutExtension($Executable)

$script:Failures = New-Object System.Collections.Generic.List[string]
$script:Log = New-Object System.Collections.Generic.List[string]

function Note([string]$text) {
    $script:Log.Add($text)
    Write-Host $text
}

function Assert-True([bool]$condition, [string]$message) {
    if ($condition) {
        Note "PASS: $message"
    } else {
        Note "FAIL: $message"
        $script:Failures.Add($message)
    }
}

function Stop-FluxProcesses {
    foreach ($p in @(Get-Process -Name $processName -ErrorAction SilentlyContinue)) {
        Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
    }
    Start-Sleep -Milliseconds 400
}

function TraceCount([string]$needle) {
    if (!(Test-Path -LiteralPath $trace)) { return 0 }
    return @(Select-String -LiteralPath $trace -SimpleMatch -Pattern "event=$needle").Count
}

# The trace file accumulates across phases, so every assertion is made against the
# delta since the phase began (captured in $script:Baseline just before launch).
function Trace-Delta([string]$needle) {
    return (TraceCount $needle) - $script:Baseline[$needle]
}

function Wait-ForSpawnDelta([int]$expectedDelta, [int]$timeoutSeconds) {
    $deadline = (Get-Date).AddSeconds($timeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        if ((Trace-Delta 'recycle-confirm-spawn') -ge $expectedDelta) { return $true }
        Start-Sleep -Milliseconds 250
    }
    return (Trace-Delta 'recycle-confirm-spawn') -ge $expectedDelta
}

function Capture-Baseline {
    $script:Baseline = @{
        'recycle-confirm-armed'    = TraceCount 'recycle-confirm-armed'
        'recycle-confirm-spawn'    = TraceCount 'recycle-confirm-spawn'
        'recycle-confirm-emptied'  = TraceCount 'recycle-confirm-emptied'
        'recycle-confirm-declined' = TraceCount 'recycle-confirm-declined'
    }
}

function Start-Launcher([string]$confirm, [bool]$arm, [uint32]$rearm) {
    # Set the hooks for this phase; clearing them afterwards is done by the caller so
    # they never leak across phases.
    $env:FLUX_SMOKE_RECYCLE_ARM = if ($arm) { '1' } else { $null }
    $env:FLUX_SMOKE_RECYCLE_REARM = if ($rearm -gt 0) { [string]$rearm } else { $null }
    $env:FLUX_SMOKE_RECYCLE_CONFIRM = if ([string]::IsNullOrEmpty($confirm)) { $null } else { $confirm }
    $process = Start-Process -FilePath $Executable -WorkingDirectory (Split-Path -Parent $Executable) -PassThru
    for ($attempt = 0; $attempt -lt 120; $attempt++) {
        Start-Sleep -Milliseconds 250
        $process.Refresh()
        if ($process.HasExited) { throw "Launcher exited early with code $($process.ExitCode)" }
        if ($process.MainWindowHandle -ne [IntPtr]::Zero) { return $process }
    }
    throw 'Flux launcher window never appeared.'
}

function Clear-LauncherHooks {
    $env:FLUX_SMOKE_RECYCLE_ARM = $null
    $env:FLUX_SMOKE_RECYCLE_REARM = $null
    $env:FLUX_SMOKE_RECYCLE_CONFIRM = $null
}

$previousAppData = $env:APPDATA
$previousTrace = $env:FLUX_PAINT_TRACE_FILE
try {
    Stop-FluxProcesses
    if (Test-Path -LiteralPath $trace) { Remove-Item -LiteralPath $trace -Force }
    New-Item -ItemType Directory -Path (Join-Path $appData 'FluxLauncher') -Force | Out-Null
    @'
{
  "start_with_windows": false,
  "auto_enable_everything": false,
  "everything_install_prompt_seen": true,
  "update_checks_enabled": false,
  "clear_query_on_activation": true
}
'@ | Set-Content -LiteralPath (Join-Path $appData 'FluxLauncher\settings.json') -Encoding utf8

    $env:APPDATA = $appData
    $env:FLUX_PAINT_TRACE_FILE = $trace
    $env:FLUX_DISABLE_UPDATE_CHECKS = '1'
    $env:FLUX_DISABLE_EVERYTHING_PROMPT = '1'
    $env:FLUX_DISABLE_SINGLE_INSTANCE = '1'
    $env:WINDUI_D2D = '0'
    $env:FLUX_SMOKE_RECYCLE_NO_HIDE = '1'

    # --- phase 1: arm once, declined -----------------------------------------
    Note '=== phase 1: arm once, declined -> exactly one spawn, no cascade'
    Capture-Baseline
    $launcher = Start-Launcher 'declined' $true 0
    Clear-LauncherHooks
    $ok = Wait-ForSpawnDelta 1 20
    Assert-True $ok 'one confirmation child spawned'
    Start-Sleep -Seconds 3
    Assert-True ((Trace-Delta 'recycle-confirm-armed') -eq 1) "armed exactly once (armed=$(Trace-Delta 'recycle-confirm-armed'))"
    Assert-True ((Trace-Delta 'recycle-confirm-spawn') -eq 1) "spawned exactly once (spawn=$(Trace-Delta 'recycle-confirm-spawn'))"
    Assert-True ((Trace-Delta 'recycle-confirm-declined') -eq 1) "outcome published as declined (declined=$(Trace-Delta 'recycle-confirm-declined'))"
    Assert-True ((Trace-Delta 'recycle-confirm-emptied') -eq 0) "never reported emptied (emptied=$(Trace-Delta 'recycle-confirm-emptied'))"
    Stop-FluxProcesses

    # --- phase 2: arm once, emptied -----------------------------------------
    Note '=== phase 2: arm once, emptied -> exactly one spawn, one emptied'
    Capture-Baseline
    $launcher = Start-Launcher 'emptied' $true 0
    Clear-LauncherHooks
    $ok = Wait-ForSpawnDelta 1 20
    Assert-True $ok 'one confirmation child spawned'
    Start-Sleep -Seconds 3
    Assert-True ((Trace-Delta 'recycle-confirm-spawn') -eq 1) "spawned exactly once (spawn=$(Trace-Delta 'recycle-confirm-spawn'))"
    Assert-True ((Trace-Delta 'recycle-confirm-emptied') -eq 1) "outcome published as emptied (emptied=$(Trace-Delta 'recycle-confirm-emptied'))"
    Assert-True ((Trace-Delta 'recycle-confirm-declined') -eq 0) "never reported declined (declined=$(Trace-Delta 'recycle-confirm-declined'))"
    Stop-FluxProcesses

    # --- phase 3: arm once + re-arm once ------------------------------------
    Note '=== phase 3: arm once + re-arm once -> exactly two spawns (regression guard)'
    Capture-Baseline
    $launcher = Start-Launcher 'declined' $true 1
    Clear-LauncherHooks
    $ok = Wait-ForSpawnDelta 2 20
    Assert-True $ok 'two confirmation children spawned across the re-arm'
    Start-Sleep -Seconds 3
    Assert-True ((Trace-Delta 'recycle-confirm-armed') -eq 2) "armed exactly twice (armed=$(Trace-Delta 'recycle-confirm-armed'))"
    Assert-True ((Trace-Delta 'recycle-confirm-spawn') -eq 2) "spawned exactly twice across the re-arm (spawn=$(Trace-Delta 'recycle-confirm-spawn'))"
    Assert-True ((Trace-Delta 'recycle-confirm-declined') -eq 2) "both outcomes declined (declined=$(Trace-Delta 'recycle-confirm-declined'))"
    Assert-True ((Trace-Delta 'recycle-confirm-spawn') -le 2) "no cascade of extra spawns after re-arm (spawn=$(Trace-Delta 'recycle-confirm-spawn'))"
    Stop-FluxProcesses

    # --- outcome ------------------------------------------------------------
    $result = [ordered]@{
        Armed = TraceCount 'recycle-confirm-armed'
        Spawn = TraceCount 'recycle-confirm-spawn'
        Emptied = TraceCount 'recycle-confirm-emptied'
        Declined = TraceCount 'recycle-confirm-declined'
        Failures = $script:Failures.ToArray()
        Log = $script:Log.ToArray()
    }
    $result | ConvertTo-Json -Depth 4 | Set-Content -Encoding utf8 (Join-Path $WorkDirectory 'recycle-confirm-result.json')
    if ($script:Failures.Count -gt 0) {
        throw "Recycle confirm smoke failed: $($script:Failures.Count) assertion(s) - $($script:Failures -join '; ')"
    }
    Write-Output "Recycle confirm smoke passed: $WorkDirectory"
} finally {
    Stop-FluxProcesses
    Clear-LauncherHooks
    $env:APPDATA = $previousAppData
    $env:FLUX_PAINT_TRACE_FILE = $previousTrace
    foreach ($name in @('FLUX_DISABLE_SINGLE_INSTANCE', 'FLUX_DISABLE_UPDATE_CHECKS', 'FLUX_DISABLE_EVERYTHING_PROMPT', 'WINDUI_D2D', 'FLUX_SMOKE_RECYCLE_NO_HIDE')) {
        [System.Environment]::SetEnvironmentVariable($name, $null)
    }
}

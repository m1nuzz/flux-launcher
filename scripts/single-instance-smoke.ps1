[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$Executable,
    [Parameter(Mandatory = $true)]
    [string]$WorkDirectory
)

$ErrorActionPreference = "Stop"

function Get-FluxProcess {
    $name = [System.IO.Path]::GetFileNameWithoutExtension($Executable)
    $processes = @(Get-Process -Name $name -ErrorAction SilentlyContinue)
    @(
        foreach ($process in $processes) {
            try {
                $process.Refresh()
                $commandLine = (Get-CimInstance Win32_Process -Filter "ProcessId = $($process.Id)" -ErrorAction Stop).CommandLine
                if ($commandLine -notmatch '--plugin-host' -and
                    $commandLine -notmatch '--folder-launch-smoke') {
                    # Count hidden tray/search processes too. Hide-on-deactivation intentionally
                    # makes MainWindowHandle zero after focus moves to another application.
                    $process
                }
            } catch {
                # Ignore a process that exits while the snapshot is collected.
                Write-Verbose "Ignoring a process that exited during the snapshot: $($_.Exception.Message)"
            }
        }
    )
}

function Get-EverythingProcess {
    @(Get-Process -Name "Everything" -ErrorAction SilentlyContinue)
}

function Get-FluxProcessesStartedAtOrAfter([DateTime]$StartTime) {
    @(
        foreach ($process in (Get-FluxProcess)) {
            try {
                $process.Refresh()
                if ($process.StartTime -ge $StartTime) {
                    $process
                }
            } catch {
                # Ignore a process that exits while the snapshot is collected.
                Write-Verbose "Ignoring a process that exited during the snapshot: $($_.Exception.Message)"
            }
        }
    )
}

function Stop-SmokeProcess {
    [CmdletBinding(SupportsShouldProcess, ConfirmImpact = 'None')]
    param(
        [System.Diagnostics.Process[]]$Processes
    )
    if (!$PSCmdlet.ShouldProcess("Flux processes", "Stop flux processes")) {
        return
    }
    foreach ($process in $Processes) {
        if ($null -ne $process -and !$process.HasExited) {
            Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
        }
    }
}

function Wait-FluxProcessGone {
    $deadline = (Get-Date).AddSeconds(15)
    $emptySnapshots = 0
    while ((Get-Date) -lt $deadline) {
        $current = Get-FluxProcess
        if ($current.Count -eq 0) {
            $emptySnapshots++
            if ($emptySnapshots -ge 20) {
                return
            }
        } else {
            $emptySnapshots = 0
            Stop-SmokeProcess $current
        }
        Start-Sleep -Milliseconds 250
    }
    throw "Flux processes did not remain stopped during preflight settling"
}

function Wait-FluxReady([System.Diagnostics.Process]$Process) {
    $deadline = (Get-Date).AddSeconds(30)
    while ((Get-Date) -lt $deadline) {
        $Process.Refresh()
        if ($Process.HasExited) {
            throw "First Flux process exited unexpectedly with code $($Process.ExitCode)."
        }
        if ($Process.MainWindowHandle -ne [IntPtr]::Zero) {
            return
        }
        Start-Sleep -Milliseconds 250
    }
    throw "Timed out waiting for the first Flux window to become ready"
}

function Start-FluxAndWaitReady {
    [CmdletBinding(SupportsShouldProcess, ConfirmImpact = 'None')]
    param()
    if (!$PSCmdlet.ShouldProcess($Executable, "Start Flux and wait for readiness")) {
        throw "Start-FluxAndWaitReady was suppressed by -WhatIf; the smoke cannot continue."
    }
    $lastError = $null
    $maxAttempts = 3
    for ($attempt = 1; $attempt -le $maxAttempts; $attempt++) {
        $candidate = Start-Process -FilePath $Executable -PassThru
        try {
            Wait-FluxReady $candidate
            if ($attempt -gt 1) {
                Write-Host "First Flux launch became ready on bounded retry attempt $attempt."
            }
            return $candidate
        }
        catch {
            $lastError = $_
            $candidate.Refresh()
            $exitCode = if ($candidate.HasExited) { $candidate.ExitCode } else { "running" }
            $retryMessage = if ($attempt -lt $maxAttempts) { "Retrying on this Windows runner." } else { "No more retries remain." }
            Write-Host "First Flux launch attempt $attempt did not become ready; exit_code=$exitCode. $retryMessage"
            if (!$candidate.HasExited) {
                Stop-Process -Id $candidate.Id -Force -ErrorAction SilentlyContinue
            }
            Wait-FluxProcessGone
            if ($attempt -lt $maxAttempts) {
                Start-Sleep -Seconds 1
            }
        }
    }
    throw $lastError
}

function Wait-FluxSecondaryProcessesSettled([DateTime]$StartTime, [int]$AllowedProcessId) {
    # A runner can briefly surface a hidden process while a previous launch is
    # finishing its named-mutex/WM_COPYDATA handoff. Do not hide a real duplicate:
    # wait for that process to exit, then fail if it remains alive.
    $deadline = (Get-Date).AddSeconds(10)
    $unexpected = @()
    while ((Get-Date) -lt $deadline) {
        $unexpected = @(
            Get-FluxProcessesStartedAtOrAfter $StartTime |
                Where-Object { $_.Id -ne $AllowedProcessId }
        )
        if ($unexpected.Count -eq 0) {
            return @()
        }
        Start-Sleep -Milliseconds 250
    }
    return $unexpected
}

New-Item -ItemType Directory -Path $WorkDirectory -Force | Out-Null
$staleFlux = Get-FluxProcess
if ($staleFlux.Count -gt 0) {
    Write-Host "Stopping $($staleFlux.Count) stale Flux process(es) before single-instance smoke."
    Stop-SmokeProcess $staleFlux
}
Wait-FluxProcessGone
$appData = Join-Path $WorkDirectory "AppData"
New-Item -ItemType Directory -Path (Join-Path $appData "FluxLauncher") -Force | Out-Null
$env:APPDATA = $appData
$env:FLUX_DISABLE_UPDATE_CHECKS = "1"
$env:FLUX_DISABLE_EVERYTHING_PROMPT = "1"
# Keep this process-count smoke independent from GPU/DWM availability; the
# dedicated visual workflow exercises the D2D path separately with WINDUI_D2D=1.
$env:WINDUI_D2D = "0"

$settingsPath = Join-Path $appData "FluxLauncher\settings.json"
@'
{
  "start_with_windows": false,
  "everything_enabled": true,
  "everything_install_prompt_seen": true,
  "update_checks_enabled": false,
  "clear_query_on_activation": true
}
'@ | Set-Content -LiteralPath $settingsPath -Encoding utf8

$initialFlux = Get-FluxProcess
$initialEverything = Get-EverythingProcess
$first = $null
$second = $null
try {
    $first = Start-FluxAndWaitReady
    $firstStartTime = $first.StartTime

    # The first launch must settle to one live process. A transient hidden process
    # is allowed to finish during the bounded settle window, but a live extra
    # process is still a single-instance regression.
    $firstFlux = Wait-FluxSecondaryProcessesSettled $firstStartTime $first.Id
    if ($firstFlux.Count -gt 0) {
        Write-Host "Flux process diagnostics before first-launch assertion:"
        @(Get-Process -Name ([System.IO.Path]::GetFileNameWithoutExtension($Executable)) -ErrorAction SilentlyContinue) | ForEach-Object {
            $_.Refresh()
            $commandLine = "<unavailable>"
            try {
                $commandLine = (Get-CimInstance Win32_Process -Filter "ProcessId = $($_.Id)" -ErrorAction Stop).CommandLine
            } catch {
                # A process may exit between the snapshot and the command line query.
                Write-Verbose "Ignoring command line lookup failure: $($_.Exception.Message)"
            }
            Write-Host "Id=$($_.Id) StartTime=$($_.StartTime.ToUniversalTime().ToString('O')) MainWindowHandle=$($_.MainWindowHandle) MainWindowTitle=[$($_.MainWindowTitle)] CommandLine=[$commandLine]"
        }
        throw "First launch left an additional Flux process alive: $($firstFlux.Count)."
    }
    $everythingAfterFirst = Get-EverythingProcess
    if ($everythingAfterFirst.Count -gt ($initialEverything.Count + 1)) {
        throw "First launch created more than one additional Everything process: $($everythingAfterFirst.Count)."
    }

    $second = Start-Process -FilePath $Executable -PassThru
    # The second instance may spend up to the listener retry window plus the
    # bounded WM_COPYDATA timeout before it can exit cleanly.
    if (!$second.WaitForExit(10000)) {
        throw "Second Flux launch did not exit after handing off to the first instance."
    }
    if ($second.ExitCode -ne 0) {
        throw "Second Flux launch exited with code $($second.ExitCode)."
    }
    Start-Sleep -Seconds 2

    $finalFlux = Get-FluxProcessesStartedAtOrAfter $firstStartTime
    if ($finalFlux.Count -gt ($initialFlux.Count + 1)) {
        throw "Second launch created a duplicate Flux process: $($finalFlux.Count)."
    }
    $finalEverything = Get-EverythingProcess
    if ($finalEverything.Count -gt ($initialEverything.Count + 1)) {
        throw "Second launch created a duplicate Everything process: $($finalEverything.Count)."
    }

    Write-Host "Single-instance smoke passed: second Flux launch handed off and exited; Everything process count remained idempotent."
}
catch {
    if ($null -ne $first) {
        $first.Refresh()
        Write-Host "First process diagnostic: Id=$($first.Id) HasExited=$($first.HasExited) MainWindowHandle=$($first.MainWindowHandle)"
    }
    if ($null -ne $second) {
        $second.Refresh()
        Write-Host "Second process diagnostic: Id=$($second.Id) HasExited=$($second.HasExited) MainWindowHandle=$($second.MainWindowHandle)"
    }
    throw
}
finally {
    Stop-SmokeProcess (Get-FluxProcess)
    if ($null -ne $first -and !$first.HasExited) {
        Stop-Process -Id $first.Id -Force -ErrorAction SilentlyContinue
    }
    if ($null -ne $second -and !$second.HasExited) {
        Stop-Process -Id $second.Id -Force -ErrorAction SilentlyContinue
    }
}

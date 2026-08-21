[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$Executable,
    [Parameter(Mandatory = $true)]
    [string]$WorkDirectory
)

$ErrorActionPreference = "Stop"

function Get-FluxProcesses {
    $name = [System.IO.Path]::GetFileNameWithoutExtension($Executable)
    @(Get-Process -Name $name -ErrorAction SilentlyContinue)
}

function Get-EverythingProcesses {
    @(Get-Process -Name "Everything" -ErrorAction SilentlyContinue)
}

function Stop-Processes([System.Diagnostics.Process[]]$Processes) {
    foreach ($process in $Processes) {
        if ($null -ne $process -and !$process.HasExited) {
            Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
        }
    }
}

New-Item -ItemType Directory -Path $WorkDirectory -Force | Out-Null
$appData = Join-Path $WorkDirectory "AppData"
New-Item -ItemType Directory -Path (Join-Path $appData "FluxLauncher") -Force | Out-Null
$env:APPDATA = $appData
$env:FLUX_DISABLE_UPDATE_CHECKS = "1"
$env:FLUX_DISABLE_EVERYTHING_PROMPT = "1"

$settingsPath = Join-Path $appData "FluxLauncher\settings.json"
@'
{
  "start_with_windows": false,
  "auto_enable_everything": true,
  "everything_install_prompt_seen": true,
  "update_checks_enabled": false,
  "clear_query_on_activation": true
}
'@ | Set-Content -LiteralPath $settingsPath -Encoding utf8

$initialFlux = Get-FluxProcesses
$initialEverything = Get-EverythingProcesses
$first = $null
$second = $null
try {
    $first = Start-Process -FilePath $Executable -PassThru
    Start-Sleep -Seconds 4
    if ($first.HasExited) {
        throw "First Flux process exited unexpectedly with code $($first.ExitCode)."
    }

    $firstFlux = Get-FluxProcesses
    if ($firstFlux.Count -gt ($initialFlux.Count + 1)) {
        throw "First launch created more than one Flux process: $($firstFlux.Count)."
    }
    $everythingAfterFirst = Get-EverythingProcesses
    if ($everythingAfterFirst.Count -gt ($initialEverything.Count + 1)) {
        throw "First launch created more than one additional Everything process: $($everythingAfterFirst.Count)."
    }

    $second = Start-Process -FilePath $Executable -PassThru
    if (!$second.WaitForExit(5000)) {
        throw "Second Flux launch did not exit after handing off to the first instance."
    }
    if ($second.ExitCode -ne 0) {
        throw "Second Flux launch exited with code $($second.ExitCode)."
    }
    Start-Sleep -Seconds 2

    $finalFlux = Get-FluxProcesses
    if ($finalFlux.Count -gt ($initialFlux.Count + 1)) {
        throw "Second launch created a duplicate Flux process: $($finalFlux.Count)."
    }
    $finalEverything = Get-EverythingProcesses
    if ($finalEverything.Count -gt ($initialEverything.Count + 1)) {
        throw "Second launch created a duplicate Everything process: $($finalEverything.Count)."
    }

    Write-Host "Single-instance smoke passed: second Flux launch handed off and exited; Everything process count remained idempotent."
}
finally {
    Stop-Processes (Get-FluxProcesses)
    if ($null -ne $first -and !$first.HasExited) {
        Stop-Process -Id $first.Id -Force -ErrorAction SilentlyContinue
    }
    if ($null -ne $second -and !$second.HasExited) {
        Stop-Process -Id $second.Id -Force -ErrorAction SilentlyContinue
    }
}

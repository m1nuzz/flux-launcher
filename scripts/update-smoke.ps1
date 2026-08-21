param(
    [Parameter(Mandatory = $true)]
    [string]$Executable,
    [Parameter(Mandatory = $true)]
    [string]$Installer,
    [Parameter(Mandatory = $true)]
    [string]$WorkDirectory
)

$ErrorActionPreference = "Stop"

Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class FluxUpdateSmokeNative {
    [DllImport("user32.dll")]
    public static extern bool IsHungAppWindow(IntPtr hWnd);
}
"@

$workRoot = (New-Item -ItemType Directory -Force -Path $WorkDirectory).FullName
$fixtureRoot = Join-Path $workRoot "fixture"
$appDataRoot = Join-Path $workRoot "appdata"
$installRoot = Join-Path $workRoot "updated-install"
$tracePath = Join-Path $workRoot "update-trace.log"
$transitionPath = Join-Path $workRoot "first-release-requested.marker"
$serverScript = Join-Path $PSScriptRoot "update-fixture-server.ps1"
$fixtureInstaller = Join-Path $fixtureRoot "FluxLauncher-Setup.exe"
$firstReleasePath = Join-Path $fixtureRoot "latest.json"
$stableReleasePath = Join-Path $fixtureRoot "latest-done.json"
$server = $null
$launcher = $null

function Write-JsonFile {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path,
        [Parameter(Mandatory = $true)]
        [object]$Value
    )
    $Value | ConvertTo-Json -Depth 8 | Set-Content -Path $Path -Encoding utf8
}

function Wait-Until {
    param(
        [Parameter(Mandatory = $true)]
        [scriptblock]$Condition,
        [Parameter(Mandatory = $true)]
        [string]$Description,
        [int]$TimeoutSeconds = 120
    )
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        if (& $Condition) {
            return
        }
        Start-Sleep -Milliseconds 250
    }
    throw "Timed out waiting for $Description"
}

function Get-TraceLines {
    if (-not (Test-Path $tracePath)) {
        return @()
    }
    return @(Get-Content -Path $tracePath)
}

function Stop-ExistingFluxProcesses {
    $existing = @(Get-Process -Name "flux-launcher" -ErrorAction SilentlyContinue)
    foreach ($process in $existing) {
        if (!$process.HasExited) {
            Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
        }
    }
    $deadline = (Get-Date).AddSeconds(10)
    while ((Get-Date) -lt $deadline) {
        if (@(Get-Process -Name "flux-launcher" -ErrorAction SilentlyContinue).Count -eq 0) {
            return
        }
        Start-Sleep -Milliseconds 250
    }
    throw "A previous Flux process remained alive before update smoke"
}

function Assert-LauncherResponsive {
    param(
        [Parameter(Mandatory = $true)]
        [System.Diagnostics.Process]$Process
    )
    $Process.Refresh()
    if ($Process.HasExited) {
        throw "Launcher exited before update installation completed"
    }
    $handle = $Process.MainWindowHandle
    if ($handle -ne [IntPtr]::Zero -and [FluxUpdateSmokeNative]::IsHungAppWindow($handle)) {
        throw "Launcher UI became hung while downloading the update"
    }
}

New-Item -ItemType Directory -Force -Path $fixtureRoot, $appDataRoot | Out-Null
Stop-ExistingFluxProcesses
Copy-Item -LiteralPath $Installer -Destination $fixtureInstaller -Force
$installerHash = (Get-FileHash -Algorithm SHA256 -Path $fixtureInstaller).Hash.ToLowerInvariant()
$port = 18963
$prefix = "http://127.0.0.1:$port/"

Write-JsonFile -Path $firstReleasePath -Value @{
    tag_name = "v0.1.66"
    html_url = "https://example.test/releases/tag/v0.1.66"
    draft = $false
    prerelease = $false
    assets = @(@{
        name = "FluxLauncher-Setup.exe"
        browser_download_url = "${prefix}FluxLauncher-Setup.exe"
        digest = "sha256:$installerHash"
    })
}
Write-JsonFile -Path $stableReleasePath -Value @{
    tag_name = "v0.1.65"
    html_url = "https://example.test/releases/tag/v0.1.65"
    draft = $false
    prerelease = $false
    assets = @()
}

# Use an isolated settings file so the test exercises the real automatic update path.
$settingsDirectory = Join-Path $appDataRoot "FluxLauncher"
New-Item -ItemType Directory -Force -Path $settingsDirectory | Out-Null
Write-JsonFile -Path (Join-Path $settingsDirectory "settings.json") -Value @{
    update_checks_enabled = $true
    auto_install_updates = $true
    last_update_check_unix = 0
    update_interval_hours = 24
    start_with_windows = $false
    auto_enable_everything = $false
    everything_install_prompt_seen = $true
}

$server = Start-Process -FilePath "pwsh" -ArgumentList @(
    "-NoProfile",
    "-File", $serverScript,
    "-Prefix", $prefix,
    "-Root", $fixtureRoot,
    "-TransitionFile", $transitionPath
) -PassThru -WindowStyle Hidden

try {
    $env:APPDATA = $appDataRoot
    $env:LOCALAPPDATA = $appDataRoot
    $env:FLUX_UPDATE_API_URL = "${prefix}latest"
    $env:FLUX_FORCE_UPDATE_CHECK = "1"
    $env:FLUX_UPDATE_TRACE_FILE = $tracePath
    $env:FLUX_UPDATE_INSTALL_DIR = $installRoot
    $launcher = Start-Process -FilePath (Resolve-Path $Executable).Path -PassThru

    Wait-Until -Description "the first update progress event" -Condition {
        Assert-LauncherResponsive -Process $launcher
        (Get-TraceLines | Where-Object { $_ -like "update-progress*" }).Count -ge 2
    }
    Assert-LauncherResponsive -Process $launcher

    Wait-Until -Description "the installer handoff" -Condition {
        Assert-LauncherResponsive -Process $launcher
        (Get-TraceLines | Where-Object { $_ -like "update-installer-started*" }).Count -ge 1
    }
    Wait-Until -Description "the old launcher process to exit" -Condition {
        $launcher.Refresh()
        $launcher.HasExited
    }
    Wait-Until -Description "the updated install root" -Condition {
        Test-Path (Join-Path $installRoot "flux-launcher.exe")
    }
    Wait-Until -Description "the update launcher restart" -Condition {
        @(Get-Process -Name "flux-launcher" -ErrorAction SilentlyContinue).Count -ge 1
    }

    $traceLines = Get-TraceLines
    if ($traceLines | Where-Object { $_ -like "update-failed*" }) {
        throw "Update trace contains a failure: $($traceLines -join ' | ')"
    }
    $progressLines = Get-TraceLines | Where-Object { $_ -like "update-progress*" }
    $parsedProgress = @(
        $progressLines | ForEach-Object {
            $parts = $_ -split "`t"
            [pscustomobject]@{
                Received = [UInt64]$parts[2]
                Total = [UInt64]($parts[3] -replace '[^0-9]', '')
            }
        }
    )
    if ($parsedProgress.Count -lt 2) {
        throw "Update emitted fewer than two progress events"
    }
    for ($index = 1; $index -lt $parsedProgress.Count; $index++) {
        if ($parsedProgress[$index].Received -lt $parsedProgress[$index - 1].Received) {
            throw "Update progress moved backwards"
        }
        if ($parsedProgress[$index].Total -ne $parsedProgress[0].Total) {
            throw "Update progress total changed during download"
        }
    }
    if ($parsedProgress[-1].Received -ne $parsedProgress[-1].Total) {
        throw "Update progress did not finish at 100 percent"
    }
    if (-not (Test-Path $transitionPath)) {
        throw "Fixture server did not receive the release check"
    }
    Write-Host "Update smoke passed: real HTTP download, SHA256 verification, monotonic byte progress, non-hung UI, installer handoff, and restart were verified."
}
catch {
    Write-Host "Update smoke trace before failure:"
    Get-TraceLines | ForEach-Object { Write-Host $_ }
    throw
}
finally {
    if ($null -ne $launcher) {
        $launcher.Refresh()
        if (-not $launcher.HasExited) {
            Stop-Process -Id $launcher.Id -Force -ErrorAction SilentlyContinue
        }
    }
    Get-Process -Name "flux-launcher" -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    if ($null -ne $server -and -not $server.HasExited) {
        Stop-Process -Id $server.Id -Force -ErrorAction SilentlyContinue
    }
    Remove-Item Env:FLUX_UPDATE_API_URL -ErrorAction SilentlyContinue
    Remove-Item Env:FLUX_FORCE_UPDATE_CHECK -ErrorAction SilentlyContinue
    Remove-Item Env:FLUX_UPDATE_TRACE_FILE -ErrorAction SilentlyContinue
    Remove-Item Env:FLUX_UPDATE_INSTALL_DIR -ErrorAction SilentlyContinue
}

[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$Installer,
    [Parameter(Mandatory = $true)]
    [string]$ExpectedExe,
    [Parameter(Mandatory = $true)]
    [string]$WorkDirectory
)

$ErrorActionPreference = "Stop"
$runKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run"
$runValueName = "Flux Launcher"

Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class FluxStartupSmokeNative {
    [DllImport("user32.dll")]
    public static extern bool IsWindowVisible(IntPtr hWnd);
}
"@

function Get-StartupCommand {
    return (Get-ItemProperty -Path $runKey -Name $runValueName -ErrorAction SilentlyContinue).$runValueName
}

function Assert-NoStartupEntry {
    $startupCommand = Get-StartupCommand
    if ($null -ne $startupCommand -and $startupCommand -ne "") {
        throw "Startup registry entry unexpectedly exists: $startupCommand"
    }
}

function Invoke-SilentInstall {
    param(
        [Parameter(Mandatory = $true)]
        [string]$InstallRoot,
        [Parameter(Mandatory = $true)]
        [string]$LogPath,
        [string[]]$AdditionalArguments = @()
    )

    New-Item -ItemType Directory -Force -Path $InstallRoot | Out-Null
    $arguments = @(
        "/VERYSILENT",
        "/SUPPRESSMSGBOXES",
        "/NORESTART",
        "/LOG=$LogPath",
        "/DIR=$InstallRoot"
    ) + $AdditionalArguments
    $process = Start-Process `
        -FilePath $installerPath `
        -ArgumentList $arguments `
        -WorkingDirectory $workRoot `
        -Wait `
        -PassThru
    $exitCode = [int]$process.ExitCode
    Write-Host "Installer exit code for $InstallRoot`: $exitCode"
    if ($exitCode -ne 0) {
        if (Test-Path $LogPath) {
            Get-Content $LogPath
        }
        throw "Installer exited with code $exitCode"
    }
}

function Get-InstalledExecutable {
    param(
        [Parameter(Mandatory = $true)]
        [string]$InstallRoot
    )

    $installedExe = Join-Path $InstallRoot "flux-launcher.exe"
    if (-not (Test-Path $installedExe)) {
        throw "Installed executable was not found at $installedExe"
    }
    return $installedExe
}

function Assert-InstalledHash {
    param(
        [Parameter(Mandatory = $true)]
        [string]$InstalledExe
    )

    $installedHash = (Get-FileHash -Algorithm SHA256 $InstalledExe).Hash
    $expectedHash = (Get-FileHash -Algorithm SHA256 $expectedExePath).Hash
    if ($installedHash -ne $expectedHash) {
        throw "Installed executable hash mismatch: expected $expectedHash, got $installedHash"
    }
}

function Invoke-SilentUninstall {
    param(
        [Parameter(Mandatory = $true)]
        [string]$InstallRoot
    )

    $uninstaller = Get-ChildItem -Path $InstallRoot -Filter "unins*.exe" -File | Select-Object -First 1
    if ($null -eq $uninstaller) {
        throw "Inno Setup uninstaller was not found in $InstallRoot"
    }
    $process = Start-Process `
        -FilePath $uninstaller.FullName `
        -ArgumentList @("/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART") `
        -Wait `
        -PassThru
    $exitCode = [int]$process.ExitCode
    Write-Host "Uninstaller exit code for $InstallRoot`: $exitCode"
    if ($exitCode -ne 0) {
        throw "Uninstaller exited with code $exitCode"
    }
    if (Test-Path (Join-Path $InstallRoot "flux-launcher.exe")) {
        throw "Installed executable still exists after uninstall: $InstallRoot"
    }
}

function Assert-StartupLaunchIsHidden {
    param(
        [Parameter(Mandatory = $true)]
        [string]$InstalledExe
    )

    $process = Start-Process -FilePath $InstalledExe -ArgumentList @("--startup") -PassThru
    try {
        Start-Sleep -Seconds 3
        if ($process.HasExited) {
            throw "Startup-mode process exited before smoke verification"
        }
        $process.Refresh()
        $handle = $process.MainWindowHandle
        if ($handle -ne [IntPtr]::Zero -and [FluxStartupSmokeNative]::IsWindowVisible($handle)) {
            throw "Startup-mode process exposed a visible launcher window after --startup"
        }
        Write-Host "Startup-mode smoke passed: process is running without a visible launcher window."
    }
    finally {
        if (-not $process.HasExited) {
            Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
        }
    }
}

New-Item -ItemType Directory -Force -Path $WorkDirectory | Out-Null
$workRoot = (Resolve-Path $WorkDirectory).Path
$installerPath = (Resolve-Path $Installer).Path
$expectedExePath = (Resolve-Path $ExpectedExe).Path
$defaultInstallRoot = Join-Path $workRoot "installed-default"
$disabledInstallRoot = Join-Path $workRoot "installed-disabled"
$defaultLogPath = Join-Path $workRoot "installer-default.log"
$disabledLogPath = Join-Path $workRoot "installer-disabled.log"

Assert-NoStartupEntry

# The normal installer path must keep startup enabled by default and expose a selectable task.
Invoke-SilentInstall -InstallRoot $defaultInstallRoot -LogPath $defaultLogPath
$defaultInstalledExe = Get-InstalledExecutable -InstallRoot $defaultInstallRoot
Assert-InstalledHash -InstalledExe $defaultInstalledExe
$defaultStartupCommand = Get-StartupCommand
if ($defaultStartupCommand -notmatch [regex]::Escape($defaultInstalledExe)) {
    throw "Default installation did not create a startup value targeting the installed executable: $defaultStartupCommand"
}
if ($defaultStartupCommand -notmatch "--startup") {
    throw "Default startup registry value does not use hidden startup mode: $defaultStartupCommand"
}
Assert-StartupLaunchIsHidden -InstalledExe $defaultInstalledExe
Invoke-SilentUninstall -InstallRoot $defaultInstallRoot
Assert-NoStartupEntry

# /TASKS=!startup models the user clearing the default-checked installer checkbox.
Invoke-SilentInstall `
    -InstallRoot $disabledInstallRoot `
    -LogPath $disabledLogPath `
    -AdditionalArguments @("/TASKS=!startup")
$disabledInstalledExe = Get-InstalledExecutable -InstallRoot $disabledInstallRoot
Assert-InstalledHash -InstalledExe $disabledInstalledExe
Assert-NoStartupEntry
Invoke-SilentUninstall -InstallRoot $disabledInstallRoot
Assert-NoStartupEntry

Write-Host "Installer smoke passed: default startup enabled, startup opt-out, hidden --startup mode, hash validation, and uninstall cleanup."

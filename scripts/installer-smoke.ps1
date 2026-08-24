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

function Assert-InstallerConfiguration {
    $installerScriptPath = Join-Path $PSScriptRoot "..\packaging\installer\FluxLauncher.iss"
    $iconPath = Join-Path $PSScriptRoot "..\packaging\installer\flux-launcher.ico"
    if (-not (Test-Path $installerScriptPath)) {
        throw "Installer script was not found at $installerScriptPath"
    }
    if (-not (Test-Path $iconPath)) {
        throw "Flux Launcher icon resource was not found at $iconPath"
    }
    $installerScript = Get-Content -Path $installerScriptPath -Raw
    $requiredDirectives = @(
        '[Tasks]',
        'Name: "startup"; Description: "Start Flux Launcher automatically with Windows"; GroupDescription: "Windows startup:"',
        '[Run]',
        'Filename: "{app}\{#AppExeName}"; Description: "Launch Flux Launcher now"; Flags: nowait postinstall skipifsilent',
        '[UninstallRun]',
        'Filename: "{app}\{#AppExeName}"; Parameters: "--shutdown"; Flags: waituntilterminated skipifdoesntexist; RunOnceId: "FluxLauncherShutdown"',
        '[UninstallDelete]',
        'Type: filesandordirs; Name: "{app}"',
        'Type: filesandordirs; Name: "{group}"',
        'Type: filesandordirs; Name: "{userappdata}\FluxLauncher"',
        'IconFilename: "{app}\flux-launcher.ico"; IconIndex: 0'
    )
    foreach ($directive in $requiredDirectives) {
        if (-not $installerScript.Contains($directive)) {
            throw "Installer directive is missing: $directive"
        }
    }
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

function Assert-StartMenuShortcutIcon {
    param(
        [Parameter(Mandatory = $true)]
        [string]$InstalledExe
    )

    $startMenuRoot = Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs"
    $shortcut = Get-ChildItem -Path $startMenuRoot -Filter "Flux Launcher.lnk" -File -Recurse -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($null -eq $shortcut) {
        throw "Flux Launcher Start Menu shortcut was not created"
    }
    $shell = New-Object -ComObject WScript.Shell
    $link = $shell.CreateShortcut($shortcut.FullName)
    if ($link.TargetPath -ne $InstalledExe) {
        throw "Start Menu shortcut target mismatch: expected $InstalledExe, got $($link.TargetPath)"
    }
    if ($link.IconLocation -notmatch "flux-launcher\.ico") {
        throw "Start Menu shortcut does not reference flux-launcher.ico: $($link.IconLocation)"
    }
    Write-Host "Start Menu shortcut smoke passed: target and Flux Launcher icon are correct."
    return $shortcut.FullName
}

function Wait-PathGone {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path,
        [Parameter(Mandatory = $true)]
        [string]$Description
    )

    for ($attempt = 0; $attempt -lt 30; $attempt++) {
        if (-not (Test-Path $Path)) {
            return
        }
        Start-Sleep -Milliseconds 250
    }
    throw "$Description still exists after uninstall: $Path"
}

function Invoke-SilentUninstall {
    param(
        [Parameter(Mandatory = $true)]
        [string]$InstallRoot,
        [Parameter(Mandatory = $true)]
        [string]$ShortcutPath,
        [Parameter(Mandatory = $true)]
        [string]$UserDataRoot,
        [System.Diagnostics.Process]$LiveProcess
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
    if ($null -ne $LiveProcess) {
        $LiveProcess.Refresh()
        if (-not $LiveProcess.HasExited) {
            if (-not $LiveProcess.WaitForExit(5000)) {
                throw "Flux Launcher process was still running after uninstaller returned"
            }
        }
        Write-Host "Uninstall shutdown order passed: Flux Launcher exited before file deletion checks."
    }
    Wait-PathGone -Path $InstallRoot -Description "Install directory"
    Wait-PathGone -Path $ShortcutPath -Description "Start Menu shortcut"
    Wait-PathGone -Path $UserDataRoot -Description "Flux Launcher user data"
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
$userDataRoot = Join-Path $env:APPDATA "FluxLauncher"
$defaultLogPath = Join-Path $workRoot "installer-default.log"
$disabledLogPath = Join-Path $workRoot "installer-disabled.log"
$originalStartupCommand = Get-StartupCommand

Assert-InstallerConfiguration

try {
    # Earlier UI capture steps may launch Flux and leave a Run entry behind. Isolate this
    # installer test and restore the runner's original value in the finally block.
    Remove-ItemProperty -Path $runKey -Name $runValueName -ErrorAction SilentlyContinue
    Assert-NoStartupEntry

    # The normal installer path must keep startup enabled by default and expose a selectable task.
    Invoke-SilentInstall -InstallRoot $defaultInstallRoot -LogPath $defaultLogPath
    $defaultInstalledExe = Get-InstalledExecutable -InstallRoot $defaultInstallRoot
    Assert-InstalledHash -InstalledExe $defaultInstalledExe
    $defaultShortcutPath = Assert-StartMenuShortcutIcon -InstalledExe $defaultInstalledExe
    New-Item -ItemType Directory -Force -Path $userDataRoot | Out-Null
    Set-Content -Path (Join-Path $userDataRoot "settings.json") -Value '{"smoke":true}' -NoNewline
    $defaultStartupCommand = Get-StartupCommand
    if ($defaultStartupCommand -notmatch [regex]::Escape($defaultInstalledExe)) {
        throw "Default installation did not create a startup value targeting the installed executable: $defaultStartupCommand"
    }
    if ($defaultStartupCommand -notmatch "--startup") {
        throw "Default startup registry value does not use hidden startup mode: $defaultStartupCommand"
    }
    Assert-StartupLaunchIsHidden -InstalledExe $defaultInstalledExe
    # Keep a real installed Flux process alive while invoking the uninstaller.
    # The [UninstallRun] --shutdown entry must stop this process before the
    # uninstaller is allowed to remove its executable and install directory.
    $liveProcess = Start-Process -FilePath $defaultInstalledExe -ArgumentList @("--startup") -PassThru
    try {
        Start-Sleep -Seconds 3
        $liveProcess.Refresh()
        if ($liveProcess.HasExited) {
            throw "Live Flux Launcher process exited before uninstall-order smoke"
        }
        Invoke-SilentUninstall `
            -InstallRoot $defaultInstallRoot `
            -ShortcutPath $defaultShortcutPath `
            -UserDataRoot $userDataRoot `
            -LiveProcess $liveProcess
    }
    finally {
        if ($null -ne $liveProcess) {
            $liveProcess.Refresh()
            if (-not $liveProcess.HasExited) {
                Stop-Process -Id $liveProcess.Id -Force -ErrorAction SilentlyContinue
            }
        }
    }
    Assert-NoStartupEntry

    # /TASKS=!startup models the user clearing the default-checked installer checkbox.
    Invoke-SilentInstall `
        -InstallRoot $disabledInstallRoot `
        -LogPath $disabledLogPath `
        -AdditionalArguments @("/TASKS=!startup")
    $disabledInstalledExe = Get-InstalledExecutable -InstallRoot $disabledInstallRoot
    Assert-InstalledHash -InstalledExe $disabledInstalledExe
    $disabledShortcutPath = Assert-StartMenuShortcutIcon -InstalledExe $disabledInstalledExe
    Assert-NoStartupEntry
    Invoke-SilentUninstall `
        -InstallRoot $disabledInstallRoot `
        -ShortcutPath $disabledShortcutPath `
        -UserDataRoot $userDataRoot
    Assert-NoStartupEntry

    Write-Host "Installer smoke passed: default startup enabled, startup opt-out, hidden --startup mode, hash validation, live-process shutdown before uninstall deletion, full install-root cleanup, Start Menu cleanup, user-data cleanup, and uninstall cleanup."
}
finally {
    if ($null -eq $originalStartupCommand -or $originalStartupCommand -eq "") {
        Remove-ItemProperty -Path $runKey -Name $runValueName -ErrorAction SilentlyContinue
    }
    else {
        if (-not (Test-Path $runKey)) {
            New-Item -Path $runKey -Force | Out-Null
        }
        New-ItemProperty -Path $runKey -Name $runValueName -Value $originalStartupCommand -PropertyType String -Force | Out-Null
    }
}

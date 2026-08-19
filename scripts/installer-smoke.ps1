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
New-Item -ItemType Directory -Force -Path $WorkDirectory | Out-Null
$workRoot = (Resolve-Path $WorkDirectory).Path
$installerPath = (Resolve-Path $Installer).Path
$expectedExePath = (Resolve-Path $ExpectedExe).Path
$installRoot = Join-Path $workRoot "installed"
$logPath = Join-Path $workRoot "installer.log"
New-Item -ItemType Directory -Force -Path $installRoot | Out-Null

& $installerPath /VERYSILENT /SUPPRESSMSGBOXES /NORESTART "/LOG=$logPath" "/DIR=$installRoot"
$installerExitCode = if ($null -eq $LASTEXITCODE) { 0 } else { [int]$LASTEXITCODE }
Write-Host "Installer exit code: $installerExitCode"
if ($installerExitCode -ne 0) {
    if (Test-Path $logPath) {
        Get-Content $logPath
    }
    throw "Installer exited with code $installerExitCode"
}

$installedExe = Join-Path $installRoot "flux-launcher.exe"
if (-not (Test-Path $installedExe)) {
    throw "Installed executable was not found at $installedExe"
}

$installedHash = (Get-FileHash -Algorithm SHA256 $installedExe).Hash
$expectedHash = (Get-FileHash -Algorithm SHA256 $expectedExePath).Hash
if ($installedHash -ne $expectedHash) {
    throw "Installed executable hash mismatch: expected $expectedHash, got $installedHash"
}

$startupCommand = (Get-ItemProperty -Path $runKey -Name $runValueName -ErrorAction Stop).$runValueName
if ($startupCommand -notmatch [regex]::Escape($installedExe)) {
    throw "Startup registry value does not target the installed executable: $startupCommand"
}
if ($startupCommand -notmatch "--startup") {
    throw "Startup registry value does not use hidden startup mode: $startupCommand"
}

$uninstaller = Get-ChildItem -Path $installRoot -Filter "unins*.exe" -File | Select-Object -First 1
if ($null -eq $uninstaller) {
    throw "Inno Setup uninstaller was not found"
}
& $uninstaller.FullName /VERYSILENT /SUPPRESSMSGBOXES /NORESTART
$uninstallerExitCode = if ($null -eq $LASTEXITCODE) { 0 } else { [int]$LASTEXITCODE }
Write-Host "Uninstaller exit code: $uninstallerExitCode"
if ($uninstallerExitCode -ne 0) {
    throw "Uninstaller exited with code $uninstallerExitCode"
}

if (Test-Path $installedExe) {
    throw "Installed executable still exists after uninstall"
}
if (Get-ItemProperty -Path $runKey -Name $runValueName -ErrorAction SilentlyContinue) {
    throw "Startup registry entry still exists after uninstall"
}

Write-Host "Installer smoke passed: silent install, hash, default startup entry, hidden mode, and uninstall cleanup."

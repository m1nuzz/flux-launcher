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
$installRoot = Join-Path $WorkDirectory "installed"
$logPath = Join-Path $WorkDirectory "installer.log"
New-Item -ItemType Directory -Force -Path $installRoot | Out-Null

& $Installer /VERYSILENT /SUPPRESSMSGBOXES /NORESTART /LOG=$logPath /DIR="$installRoot"
if ($LASTEXITCODE -ne 0) {
    throw "Installer exited with code $LASTEXITCODE"
}

$installedExe = Join-Path $installRoot "flux-launcher.exe"
if (-not (Test-Path $installedExe)) {
    throw "Installed executable was not found at $installedExe"
}

$installedHash = (Get-FileHash -Algorithm SHA256 $installedExe).Hash
$expectedHash = (Get-FileHash -Algorithm SHA256 $ExpectedExe).Hash
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
if ($LASTEXITCODE -ne 0) {
    throw "Uninstaller exited with code $LASTEXITCODE"
}

if (Test-Path $installedExe) {
    throw "Installed executable still exists after uninstall"
}
if (Get-ItemProperty -Path $runKey -Name $runValueName -ErrorAction SilentlyContinue) {
    throw "Startup registry entry still exists after uninstall"
}

Write-Host "Installer smoke passed: silent install, hash, default startup entry, hidden mode, and uninstall cleanup."

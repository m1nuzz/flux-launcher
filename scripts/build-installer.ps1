[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$AppVersion,
    [string]$BuildDir = "target/x86_64-pc-windows-msvc/release",
    [string]$OutputDirectory = "artifacts/FluxLauncher-Windows11-x64",
    [string]$InnoCompiler = "C:\Program Files (x86)\Inno Setup 6\ISCC.exe"
)

$ErrorActionPreference = "Stop"
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$buildPath = (Resolve-Path (Join-Path $repoRoot $BuildDir)).Path
$outputPath = Join-Path $repoRoot $OutputDirectory
$portableName = "FluxLauncher-Portable.exe"
$installerName = "FluxLauncher-Setup.exe"

if (-not (Test-Path $InnoCompiler)) {
    throw "Inno Setup compiler was not found at $InnoCompiler"
}
$launcher = Join-Path $buildPath "flux-launcher.exe"
if (-not (Test-Path $launcher)) {
    throw "Release launcher was not found at $launcher"
}

New-Item -ItemType Directory -Force -Path $outputPath | Out-Null
& $InnoCompiler "/DAppVersion=$AppVersion" "/DBuildDir=$buildPath" (Join-Path $repoRoot "packaging/installer/FluxLauncher.iss")
if ($LASTEXITCODE -ne 0) {
    throw "Inno Setup compiler exited with code $LASTEXITCODE"
}

$builtInstaller = Join-Path $repoRoot "artifacts/installer/$installerName"
if (-not (Test-Path $builtInstaller)) {
    throw "Expected installer was not created at $builtInstaller"
}

Copy-Item $builtInstaller (Join-Path $outputPath $installerName) -Force
Copy-Item $launcher (Join-Path $outputPath $portableName) -Force
Get-ChildItem $outputPath -File | Get-FileHash -Algorithm SHA256 | Format-Table -AutoSize

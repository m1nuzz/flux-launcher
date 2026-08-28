[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$AppVersion,
    [string]$BuildDir = "target/x86_64-pc-windows-msvc/release",
    [string]$OutputDirectory = "artifacts/FluxLauncher-Windows11-x64",
    [string]$InnoCompiler = ""
)

$ErrorActionPreference = "Stop"
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$buildPath = (Resolve-Path (Join-Path $repoRoot $BuildDir)).Path
$outputPath = Join-Path $repoRoot $OutputDirectory
$portableName = "FluxLauncher-Portable.exe"
$installerName = "FluxLauncher-Setup.exe"

# Resolve the Inno Setup compiler automatically when no explicit path was given.
if (-not $InnoCompiler) {
    $candidates = @(
        "C:\Program Files (x86)\Inno Setup 6\ISCC.exe",
        "C:\Program Files\Inno Setup 6\ISCC.exe",
        (Join-Path $env:LOCALAPPDATA "Programs\Inno Setup 6\ISCC.exe")
    )
    $InnoCompiler = $candidates | Where-Object { Test-Path $_ } | Select-Object -First 1
    if (-not $InnoCompiler) {
        $command = Get-Command ISCC.exe -ErrorAction SilentlyContinue
        if ($command) {
            $InnoCompiler = $command.Source
        }
    }
}
if (-not $InnoCompiler -or -not (Test-Path $InnoCompiler)) {
    throw "Inno Setup compiler was not found. Install Inno Setup 6 (https://jrsoftware.org/isinfo.php) or pass -InnoCompiler explicitly."
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

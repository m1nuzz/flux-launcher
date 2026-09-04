[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$AppVersion,
    [string]$BuildDir = "target/x86_64-pc-windows-msvc/release",
    [string]$OutputDirectory = "artifacts/FluxLauncher-Windows11-x64",
    [string]$InstallDirectory,
    [string]$InnoCompiler
)

$ErrorActionPreference = "Stop"
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$buildPath = (Resolve-Path (Join-Path $repoRoot $BuildDir)).Path
$outputPath = Join-Path $repoRoot $OutputDirectory
$portableName = "FluxLauncher-Portable.exe"
$installerName = "FluxLauncher-Setup.exe"
$requestedInnoCompiler = $InnoCompiler

if ($InnoCompiler) {
    $innoCandidates = @($InnoCompiler)
} elseif ($InstallDirectory) {
    $innoCandidates = @((Join-Path $InstallDirectory "ISCC.exe"))
} else {
    $innoCandidates = @(
        "C:\Program Files (x86)\Inno Setup 6\ISCC.exe",
        "C:\Program Files\Inno Setup 6\ISCC.exe",
        "C:\Users\$env:USERNAME\AppData\Local\Programs\Inno Setup 6\ISCC.exe"
    )
}

$InnoCompiler = $innoCandidates | Where-Object { Test-Path -LiteralPath $_ -PathType Leaf } | Select-Object -First 1
if (-not $InnoCompiler) {
    if ($InstallDirectory) {
        throw "Inno Setup compiler was not found at $InstallDirectory. Expected ISCC.exe in that directory."
    }
    if ($PSBoundParameters.ContainsKey("InnoCompiler")) {
        throw "Inno Setup compiler was not found at $requestedInnoCompiler"
    }
    throw "Inno Setup compiler was not found in standard locations. Specify -InstallDirectory or -InnoCompiler."
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

[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^\d+\.\d+\.\d+(\.\d+)?$')]
    [string]$Version,
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^[A-Fa-f0-9]{64}$')]
    [string]$InstallerSha256,
    [string]$OutputRoot = "packaging/winget/manifests",
    [string]$ReleaseTag
)

$ErrorActionPreference = "Stop"
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
if ([string]::IsNullOrWhiteSpace($ReleaseTag)) {
    $ReleaseTag = "v$Version"
}

$packageIdentifier = "M1nuzz.FluxLauncher"
$publisher = "M1nuzz"
$packageName = "FluxLauncher"
$packageDirectory = Join-Path $repoRoot (Join-Path $OutputRoot (Join-Path "m/M1nuzz/FluxLauncher" $Version))
$installerUrl = "https://github.com/m1nuzz/flux-launcher/releases/download/$ReleaseTag/FluxLauncher-Setup.exe"
$releaseUrl = "https://github.com/m1nuzz/flux-launcher/releases/tag/$ReleaseTag"
$schemaBase = "https://aka.ms/winget-manifest"

function Write-Utf8NoBom {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path,
        [Parameter(Mandatory = $true)]
        [string]$Content
    )

    $encoding = New-Object System.Text.UTF8Encoding($false)
    [System.IO.File]::WriteAllText($Path, $Content, $encoding)
}

New-Item -ItemType Directory -Force -Path $packageDirectory | Out-Null

$versionManifest = @"
# yaml-language-server: `$schema=$schemaBase.version.1.12.0.schema.json
PackageIdentifier: $packageIdentifier
PackageVersion: $Version
DefaultLocale: en-US
ManifestType: version
ManifestVersion: 1.12.0
"@

$localeManifest = @"
# yaml-language-server: `$schema=$schemaBase.defaultLocale.1.12.0.schema.json
PackageIdentifier: $packageIdentifier
PackageVersion: $Version
PackageLocale: en-US
Publisher: m1nuzz
PublisherUrl: https://github.com/m1nuzz
PublisherSupportUrl: https://github.com/m1nuzz/flux-launcher/issues
PackageName: Flux Launcher
PackageUrl: https://github.com/m1nuzz/flux-launcher
License: MIT
LicenseUrl: https://github.com/m1nuzz/flux-launcher/blob/main/LICENSE
ShortDescription: A lightweight native Windows 11 launcher and file search tool built with Rust and windui.
Description: Flux Launcher is a keyboard-first Windows 11 launcher with a native Acrylic interface, Everything IPC file search, global hotkeys, built-in Google and Obsidian providers, legacy Flow plugin compatibility, and native Rust community plugins.
Moniker: flux
Tags:
  - launcher
  - windows-launcher
  - productivity
  - search
  - everything
  - rust
  - windows-11
  - flow-launcher-alternative
ReleaseNotesUrl: $releaseUrl
ManifestType: defaultLocale
ManifestVersion: 1.12.0
"@

$installerManifest = @"
# yaml-language-server: `$schema=$schemaBase.installer.1.12.0.schema.json
PackageIdentifier: $packageIdentifier
PackageVersion: $Version
InstallModes:
  - silent
  - silentWithProgress
Installers:
  - Architecture: x64
    InstallerType: inno
    InstallerUrl: $installerUrl
    InstallerSha256: $InstallerSha256
    Scope: user
    InstallerSwitches:
      Silent: /VERYSILENT /SUPPRESSMSGBOXES /NORESTART /SP-
      SilentWithProgress: /SILENT /SUPPRESSMSGBOXES /NORESTART /SP-
    UpgradeBehavior: install
    AppsAndFeaturesEntries:
      - DisplayName: Flux Launcher
        Publisher: m1nuzz
        DisplayVersion: $Version
        InstallerType: inno
ManifestType: installer
ManifestVersion: 1.12.0
"@

Write-Utf8NoBom (Join-Path $packageDirectory "$packageIdentifier.yaml") $versionManifest
Write-Utf8NoBom (Join-Path $packageDirectory "$packageIdentifier.locale.en-US.yaml") $localeManifest
Write-Utf8NoBom (Join-Path $packageDirectory "$packageIdentifier.installer.yaml") $installerManifest

Write-Host "Generated WinGet manifest set at $packageDirectory"
Write-Host "Installer URL: $installerUrl"
Write-Host "Installer SHA256: $InstallerSha256"

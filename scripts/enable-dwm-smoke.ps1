param(
    [Parameter(Mandatory = $true)]
    [string]$OutputDirectory
)

$ErrorActionPreference = "Continue"
New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null

# Enable per-user transparency and the best visual-effects profile.
New-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize" -Name EnableTransparency -PropertyType DWord -Value 1 -Force | Out-Null
New-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\VisualEffects" -Name VisualFXSetting -PropertyType DWord -Value 1 -Force | Out-Null

# Permit composition on Windows Server images when the runner exposes it.
New-Item -Path "HKLM:\SOFTWARE\Policies\Microsoft\Windows NT\Terminal Services" -Force | Out-Null
New-ItemProperty -Path "HKLM:\SOFTWARE\Policies\Microsoft\Windows NT\Terminal Services" -Name fAllowDesktopCompositionOnServer -PropertyType DWord -Value 1 -Force | Out-Null
Set-Service -Name Themes -StartupType Automatic
Start-Service -Name Themes -ErrorAction SilentlyContinue

# Best-effort VM workaround. Some images require a reboot before DWM honors it,
# so the resulting registry value and live DWM state are recorded below.
$forceEffectApplied = $false
try {
    New-ItemProperty -Path "HKLM:\SOFTWARE\Microsoft\Windows\Dwm" -Name ForceEffectMode -PropertyType DWord -Value 2 -Force -ErrorAction Stop | Out-Null
    $forceEffectApplied = $true
} catch {
    Write-Warning "ForceEffectMode could not be written: $($_.Exception.Message)"
}

& gpupdate.exe /target:user /force | Out-Host
& rundll32.exe user32.dll,UpdatePerUserSystemParameters
Stop-Process -Name explorer -Force -ErrorAction SilentlyContinue
Start-Process explorer.exe
Start-Sleep -Seconds 3

Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class FluxDwmPreflight {
    [DllImport("dwmapi.dll")] public static extern int DwmIsCompositionEnabled(out int enabled);
    [DllImport("user32.dll")] public static extern int GetSystemMetrics(int index);
}
'@

$composition = 0
$hr = [FluxDwmPreflight]::DwmIsCompositionEnabled([ref]$composition)
$remote = [FluxDwmPreflight]::GetSystemMetrics(0x1000)
$transparency = (Get-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize" -ErrorAction SilentlyContinue).EnableTransparency
$effectMode = (Get-ItemProperty -Path "HKLM:\SOFTWARE\Microsoft\Windows\Dwm" -ErrorAction SilentlyContinue).ForceEffectMode
$os = Get-CimInstance Win32_OperatingSystem
$computer = Get-CimInstance Win32_ComputerSystem

[ordered]@{
    Caption = $os.Caption
    Version = $os.Version
    BuildNumber = $os.BuildNumber
    ComputerModel = $computer.Model
    GitHubRunnerName = $env:RUNNER_NAME
    GitHubRunnerOS = $env:RUNNER_OS
    GitHubRunnerArchitecture = $env:RUNNER_ARCH
    GitHubImageVersion = $env:ImageVersion
    CompositionHResult = $hr
    CompositionEnabled = ($composition -ne 0)
    RemoteSession = ($remote -ne 0)
    EnableTransparency = $transparency
    ForceEffectMode = $effectMode
    ForceEffectModeWriteSucceeded = $forceEffectApplied
    CapturedAtUtc = (Get-Date).ToUniversalTime().ToString("O")
} | ConvertTo-Json | Set-Content -Encoding utf8 (Join-Path $OutputDirectory "dwm-preflight.json")

Get-Content (Join-Path $OutputDirectory "dwm-preflight.json")

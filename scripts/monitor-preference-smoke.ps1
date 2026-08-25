param(
    [Parameter(Mandatory = $true)]
    [string]$Executable,

    [Parameter(Mandatory = $true)]
    [string]$OutputDirectory
)

$ErrorActionPreference = "Stop"
New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
Add-Type -AssemblyName System.Windows.Forms

Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class FluxMonitorSmoke {
    [StructLayout(LayoutKind.Sequential)]
    public struct RECT {
        public int Left;
        public int Top;
        public int Right;
        public int Bottom;
    }
    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool GetWindowRect(IntPtr hwnd, out RECT rect);
    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool IsWindowVisible(IntPtr hwnd);
    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool EnumWindows(EnumWindowsProc callback, IntPtr lParam);
    [DllImport("user32.dll", SetLastError = true)]
    public static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint processId);
    public delegate bool EnumWindowsProc(IntPtr hwnd, IntPtr lParam);
    public static IntPtr FindWindowByProcessId(uint targetProcessId) {
        IntPtr found = IntPtr.Zero;
        EnumWindows((hwnd, lParam) => {
            uint processId;
            GetWindowThreadProcessId(hwnd, out processId);
            if (processId == targetProcessId && IsWindowVisible(hwnd)) {
                found = hwnd;
                return false;
            }
            return true;
        }, IntPtr.Zero);
        return found;
    }
}
'@

$virtual = [System.Windows.Forms.SystemInformation]::VirtualScreen
$processName = [System.IO.Path]::GetFileNameWithoutExtension($Executable)

function Request-FluxInstanceShutdown([int]$TimeoutSeconds = 10) {
    $shutdown = Start-Process -FilePath $Executable -ArgumentList "--shutdown" -WorkingDirectory (Split-Path -Parent $Executable) -PassThru -WindowStyle Hidden
    try {
        if (!$shutdown.WaitForExit($TimeoutSeconds * 1000)) {
            return $false
        }
        $deadline = (Get-Date).AddSeconds(5)
        while ((Get-Date) -lt $deadline) {
            if (@(Get-Process -Name $processName -ErrorAction SilentlyContinue).Count -eq 0) {
                return $true
            }
            Start-Sleep -Milliseconds 100
        }
        return $false
    }
    finally {
        if (!$shutdown.HasExited) {
            Stop-Process -Id $shutdown.Id -Force -ErrorAction SilentlyContinue
            try { $shutdown.WaitForExit(2000) } catch { }
        }
    }
}

function Stop-FluxProcessesAndWait {
    $existing = @(Get-Process -Name $processName -ErrorAction SilentlyContinue)
    if ($existing.Count -eq 0) { return }
    if (!(Request-FluxInstanceShutdown 10)) {
        $ids = $existing | ForEach-Object { $_.Id }
        foreach ($item in $existing) {
            if (!$item.HasExited) {
                Stop-Process -Id $item.Id -Force -ErrorAction SilentlyContinue
                try { $item.WaitForExit(5000) } catch { }
            }
        }
        throw "A previous Flux process required forced termination before monitor preference smoke: PIDs $($ids -join ', ')."
    }
    $deadline = (Get-Date).AddSeconds(10)
    while ((Get-Date) -lt $deadline) {
        if (@(Get-Process -Name $processName -ErrorAction SilentlyContinue).Count -eq 0) {
            return
        }
        Start-Sleep -Milliseconds 250
    }
    throw "A previous Flux process remained alive before monitor preference smoke."
}

$previousWinduiD2d = [Environment]::GetEnvironmentVariable(
    "WINDUI_D2D",
    [EnvironmentVariableTarget]::Process
)
$previousFluxDisableSingleInstance = [Environment]::GetEnvironmentVariable(
    "FLUX_DISABLE_SINGLE_INSTANCE",
    [EnvironmentVariableTarget]::Process
)
try {
    # D2D/compositor behavior is covered by capture-mica.ps1. This helper validates
    # monitor placement and avoids repeatedly tearing down the D2D device between
    # independent native launches on a hosted desktop.
    Remove-Item Env:WINDUI_D2D -ErrorAction SilentlyContinue
    Stop-FluxProcessesAndWait
    $modes = @("primary", "cursor", "foreground")
    $results = @()

    foreach ($mode in $modes) {
    Stop-FluxProcessesAndWait
    $env:FLUX_SMOKE_MONITOR_PREFERENCE = $mode
    # This script tests monitor placement through the normal single-instance
    # startup path; --shutdown below uses the same production tray exit route.
    $stdout = Join-Path $OutputDirectory "monitor-$mode.stdout.log"
    $stderr = Join-Path $OutputDirectory "monitor-$mode.stderr.log"
    $process = Start-Process -FilePath $Executable -WorkingDirectory (Split-Path -Parent $Executable) -PassThru -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    $modeShutdownCompleted = $false
    try {
        $handle = [IntPtr]::Zero
        for ($attempt = 0; $attempt -lt 120 -and $handle -eq [IntPtr]::Zero; $attempt++) {
            Start-Sleep -Milliseconds 100
            $process.Refresh()
            if ($process.HasExited) {
                $exitCode = $process.ExitCode
                $exitCodeHex = '0x{0:X8}' -f ([uint32]$exitCode)
                $startupStderr = if (Test-Path $stderr) { Get-Content $stderr -Raw } else { "" }
                throw "Monitor mode '$mode' launcher exited before creating a window: exit_code=$exitCode exit_code_hex=$exitCodeHex stderr=[$startupStderr]"
            }
            $handle = $process.MainWindowHandle
            if ($handle -eq [IntPtr]::Zero) {
                $handle = [FluxMonitorSmoke]::FindWindowByProcessId([uint32]$process.Id)
            }
        }
        if ($handle -eq [IntPtr]::Zero) {
            $startupStderr = if (Test-Path $stderr) { Get-Content $stderr -Raw } else { "" }
            throw "Monitor mode '$mode' did not create a native window within 12 seconds: pid=$($process.Id) stderr=[$startupStderr]"
        }
        $rect = New-Object FluxMonitorSmoke+RECT
        if (![FluxMonitorSmoke]::GetWindowRect($handle, [ref]$rect)) {
            throw "Unable to query window bounds for monitor mode '$mode'."
        }
        $insideVirtualDesktop =
            $rect.Left -ge $virtual.Left -and
            $rect.Top -ge $virtual.Top -and
            $rect.Right -le ($virtual.Left + $virtual.Width) -and
            $rect.Bottom -le ($virtual.Top + $virtual.Height)
        if (!$insideVirtualDesktop) {
            throw "Window for monitor mode '$mode' is outside the virtual desktop: $($rect.Left),$($rect.Top),$($rect.Right),$($rect.Bottom)"
        }
        $modeResult = [ordered]@{
            Mode = $mode
            WindowHandle = $handle.ToInt64()
            Left = $rect.Left
            Top = $rect.Top
            Right = $rect.Right
            Bottom = $rect.Bottom
            InsideVirtualDesktop = $insideVirtualDesktop
            GracefulShutdown = $false
        }
        $results += $modeResult
        if (!(Request-FluxInstanceShutdown 10)) {
            throw "Monitor mode '$mode' could not shut down Flux cleanly after placement check."
        }
        $modeResult.GracefulShutdown = $true
        $modeShutdownCompleted = $true
    }
    finally {
        $processStillAlive = $false
        try { $process.Refresh(); $processStillAlive = !$process.HasExited } catch { }
        if ($processStillAlive) {
            $shutdownCompleted = Request-FluxInstanceShutdown 5
            if (!$shutdownCompleted) {
                $exitCodeHex = if ($process.HasExited) { '0x{0:X8}' -f ([uint32]$process.ExitCode) } else { 'not-exited' }
                Write-Warning "Monitor mode '$mode' required forced cleanup after orderly shutdown failed: pid=$($process.Id) exit_code=$exitCodeHex"
                Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
                try { $process.WaitForExit(5000) } catch { }
            }
        }
        Remove-Item Env:FLUX_SMOKE_MONITOR_PREFERENCE -ErrorAction SilentlyContinue
        if (!$modeShutdownCompleted) {
            # The exception from the placement/startup assertion remains the
            # authoritative failure, and foreach will not start another mode.
            Write-Host "Monitor mode '$mode' ended without a graceful shutdown."
        }
    }
    }
}
finally {
    if ($null -eq $previousWinduiD2d) {
        Remove-Item Env:WINDUI_D2D -ErrorAction SilentlyContinue
    } else {
        $env:WINDUI_D2D = $previousWinduiD2d
    }
    if ($null -eq $previousFluxDisableSingleInstance) {
        Remove-Item Env:FLUX_DISABLE_SINGLE_INSTANCE -ErrorAction SilentlyContinue
    } else {
        $env:FLUX_DISABLE_SINGLE_INSTANCE = $previousFluxDisableSingleInstance
    }
}

[ordered]@{
    VirtualScreen = [ordered]@{
        Left = $virtual.Left
        Top = $virtual.Top
        Width = $virtual.Width
        Height = $virtual.Height
    }
    ModesTested = $modes
    Results = $results
    MonitorPreferenceProbe = ($results.Count -eq 3 -and ($results | Where-Object { !$_.InsideVirtualDesktop }).Count -eq 0)
} | ConvertTo-Json -Depth 6 | Set-Content -Encoding utf8 (Join-Path $OutputDirectory "monitor-preference.json")
Write-Output "Monitor preference smoke passed for Primary, Cursor, and Foreground modes."

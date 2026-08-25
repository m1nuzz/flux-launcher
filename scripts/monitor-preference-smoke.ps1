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
    public static extern bool PostMessage(IntPtr hwnd, uint message, IntPtr wParam, IntPtr lParam);
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

function Request-FluxProcessShutdown([System.Diagnostics.Process]$Process, [int]$TimeoutSeconds = 10) {
    if ($null -eq $Process) { return $true }
    try {
        $Process.Refresh()
        if ($Process.HasExited) { return $true }
    }
    catch {
        return $true
    }
    $Process.Refresh()
    $handle = $Process.MainWindowHandle
    if ($handle -eq [IntPtr]::Zero) {
        $handle = [FluxMonitorSmoke]::FindWindowByProcessId([uint32]$Process.Id)
    }
    if ($handle -eq [IntPtr]::Zero) { return $false }
    if (![FluxMonitorSmoke]::PostMessage($handle, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero)) {
        return $false
    }
    try {
        return $Process.WaitForExit($TimeoutSeconds * 1000)
    }
    catch {
        return $false
    }
}

function Stop-FluxProcessesAndWait {
    $existing = @(Get-Process -Name $processName -ErrorAction SilentlyContinue)
    $cleanupFailures = @()
    foreach ($item in $existing) {
        if (!$item.HasExited -and !(Request-FluxProcessShutdown $item 10)) {
            $cleanupFailures += $item.Id
            # This is runner cleanup only. The caller still fails immediately and
            # does not start a placement mode after an unclean predecessor exit.
            Stop-Process -Id $item.Id -Force -ErrorAction SilentlyContinue
            try { $item.WaitForExit(5000) } catch { }
        }
    }
    $deadline = (Get-Date).AddSeconds(10)
    while ((Get-Date) -lt $deadline) {
        if (@(Get-Process -Name $processName -ErrorAction SilentlyContinue).Count -eq 0) {
            if ($cleanupFailures.Count -gt 0) {
                throw "A previous Flux process required forced termination before monitor preference smoke: PIDs $($cleanupFailures -join ', ')."
            }
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
$previousSmokeExitOnClose = [Environment]::GetEnvironmentVariable(
    "FLUX_SMOKE_EXIT_ON_CLOSE",
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
    # This script tests monitor placement, not handoff behavior. Isolate each
    # process so a stale tray instance cannot make Start-Process return before
    # the new native window is created.
    $env:FLUX_DISABLE_SINGLE_INSTANCE = "1"
    $env:FLUX_SMOKE_EXIT_ON_CLOSE = "1"
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
        if (!(Request-FluxProcessShutdown $process 10)) {
            throw "Monitor mode '$mode' could not shut down Flux cleanly after placement check."
        }
        $modeResult.GracefulShutdown = $true
        $modeShutdownCompleted = $true
    }
    finally {
        $processStillAlive = $false
        try { $process.Refresh(); $processStillAlive = !$process.HasExited } catch { }
        if ($processStillAlive) {
            $shutdownCompleted = Request-FluxProcessShutdown $process 5
            if (!$shutdownCompleted) {
                $exitCodeHex = if ($process.HasExited) { '0x{0:X8}' -f ([uint32]$process.ExitCode) } else { 'not-exited' }
                Write-Warning "Monitor mode '$mode' required forced cleanup after orderly shutdown failed: pid=$($process.Id) exit_code=$exitCodeHex"
                Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
                try { $process.WaitForExit(5000) } catch { }
            }
        }
        Remove-Item Env:FLUX_SMOKE_MONITOR_PREFERENCE -ErrorAction SilentlyContinue
        Remove-Item Env:FLUX_SMOKE_EXIT_ON_CLOSE -ErrorAction SilentlyContinue
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
    if ($null -eq $previousSmokeExitOnClose) {
        Remove-Item Env:FLUX_SMOKE_EXIT_ON_CLOSE -ErrorAction SilentlyContinue
    } else {
        $env:FLUX_SMOKE_EXIT_ON_CLOSE = $previousSmokeExitOnClose
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

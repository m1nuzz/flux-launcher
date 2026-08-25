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

function Stop-FluxProcessesAndWait {
    $existing = @(Get-Process -Name $processName -ErrorAction SilentlyContinue)
    foreach ($item in $existing) {
        if (!$item.HasExited) {
            Stop-Process -Id $item.Id -Force -ErrorAction SilentlyContinue
        }
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
    $stdout = Join-Path $OutputDirectory "monitor-$mode.stdout.log"
    $stderr = Join-Path $OutputDirectory "monitor-$mode.stderr.log"
    $process = Start-Process -FilePath $Executable -WorkingDirectory (Split-Path -Parent $Executable) -PassThru -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    try {
        $handle = [IntPtr]::Zero
        for ($attempt = 0; $attempt -lt 120 -and $handle -eq [IntPtr]::Zero; $attempt++) {
            Start-Sleep -Milliseconds 100
            $process.Refresh()
            if ($process.HasExited) {
                $exitCode = $process.ExitCode
                $startupStderr = if (Test-Path $stderr) { Get-Content $stderr -Raw } else { "" }
                throw "Monitor mode '$mode' launcher exited before creating a window: exit_code=$exitCode stderr=[$startupStderr]"
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
        $results += [ordered]@{
            Mode = $mode
            WindowHandle = $handle.ToInt64()
            Left = $rect.Left
            Top = $rect.Top
            Right = $rect.Right
            Bottom = $rect.Bottom
            InsideVirtualDesktop = $insideVirtualDesktop
        }
    }
    finally {
        if (!$process.HasExited) {
            Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
        }
        try {
            Wait-Process -Id $process.Id -Timeout 10 -ErrorAction SilentlyContinue
        } catch {
            # The process may already have exited after single-instance handoff.
        }
        Stop-FluxProcessesAndWait
        Remove-Item Env:FLUX_SMOKE_MONITOR_PREFERENCE -ErrorAction SilentlyContinue
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

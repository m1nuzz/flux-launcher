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
using System.Text;
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
    public delegate bool EnumWindowsProc(IntPtr hwnd, IntPtr lParam);
    [DllImport("user32.dll")]
    public static extern bool EnumWindows(EnumWindowsProc callback, IntPtr lParam);
    [DllImport("user32.dll")]
    public static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint processId);
    [DllImport("user32.dll")]
    public static extern bool IsWindowVisible(IntPtr hwnd);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern int GetWindowText(IntPtr hwnd, StringBuilder text, int maxCount);
    [DllImport("user32.dll")]
    public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")]
    public static extern bool ShowWindowAsync(IntPtr hwnd, int cmdShow);
}
'@

function Write-ModeDiagnostics([System.Diagnostics.Process]$Process, [string]$Mode) {
    $Process.Refresh()
    Write-Output "Diagnostics for mode '$Mode': pid=$($Process.Id) responding=$($Process.Responding) threads=$($Process.Threads.Count) exited=$($Process.HasExited)"
    Write-Output "Runner foreground window: $([FluxMonitorSmoke]::GetForegroundWindow().ToInt64())"
    $pidValue = [uint32]$Process.Id
    $windows = New-Object System.Collections.Generic.List[string]
    $callback = {
        param([IntPtr]$hwnd, [IntPtr]$lParam)
        $owner = 0
        [void][FluxMonitorSmoke]::GetWindowThreadProcessId($hwnd, [ref]$owner)
        if ($owner -eq $pidValue) {
            $rect = New-Object FluxMonitorSmoke+RECT
            $hasRect = [FluxMonitorSmoke]::GetWindowRect($hwnd, [ref]$rect)
            $title = New-Object System.Text.StringBuilder 256
            [void][FluxMonitorSmoke]::GetWindowText($hwnd, $title, 256)
            $windows.Add("hwnd=$($hwnd.ToInt64()) visible=$([FluxMonitorSmoke]::IsWindowVisible($hwnd)) rect=$($rect.Left),$($rect.Top),$($rect.Right),$($rect.Bottom) hasRect=$hasRect title='$title'")
        }
        return $true
    }
    [void][FluxMonitorSmoke]::EnumWindows($callback, [IntPtr]::Zero)
    [GC]::KeepAlive($callback)
    if ($windows.Count -eq 0) {
        Write-Output "No top-level windows owned by pid $($Process.Id)."
    } else {
        $windows | ForEach-Object { Write-Output $_ }
    }
}

function Get-HiddenLauncherWindow([uint32]$PidValue) {
    $found = New-Object System.Collections.Generic.List[System.IntPtr]
    $callback = {
        param([IntPtr]$hwnd, [IntPtr]$lParam)
        $owner = 0
        [void][FluxMonitorSmoke]::GetWindowThreadProcessId($hwnd, [ref]$owner)
        if ($owner -eq $PidValue) {
            $title = New-Object System.Text.StringBuilder 256
            [void][FluxMonitorSmoke]::GetWindowText($hwnd, $title, 256)
            if ($title.ToString() -eq "Flux Launcher" -and ![FluxMonitorSmoke]::IsWindowVisible($hwnd)) {
                $found.Add($hwnd)
            }
        }
        return $true
    }
    [void][FluxMonitorSmoke]::EnumWindows($callback, [IntPtr]::Zero)
    [GC]::KeepAlive($callback)
    if ($found.Count -eq 0) {
        return [IntPtr]::Zero
    }
    return $found[0]
}

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
    $stdout = Join-Path $OutputDirectory "monitor-$mode.stdout.log"
    $stderr = Join-Path $OutputDirectory "monitor-$mode.stderr.log"
    $process = Start-Process -FilePath $Executable -PassThru -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    try {
        $handle = [IntPtr]::Zero
        # Cold start on a software-rendered runner (D2D/Acrylic init, catalog
        # and icon warm-up) exceeds 3 s: allow 12 s before declaring failure.
        $attemptsUsed = 0
        for ($attempt = 0; $attempt -lt 120 -and $handle -eq [IntPtr]::Zero; $attempt++) {
            Start-Sleep -Milliseconds 100
            $process.Refresh()
            $handle = $process.MainWindowHandle
            $attemptsUsed = $attempt + 1
            # hide_on_deactivate can conceal the freshly shown window when the
            # runner steals focus: re-show a hidden launcher window and keep
            # waiting instead of failing on an invisible-but-alive HWND.
            if ($handle -eq [IntPtr]::Zero -and ($attempt % 20) -eq 19) {
                $hidden = Get-HiddenLauncherWindow ([uint32]$process.Id)
                if ($hidden -ne [IntPtr]::Zero) {
                    [void][FluxMonitorSmoke]::ShowWindowAsync($hidden, 1)
                    Write-Output "Monitor mode '$mode': re-showed hidden launcher window."
                }
            }
        }
        if ($handle -eq [IntPtr]::Zero) {
            $process.Refresh()
            if ($process.HasExited) {
                throw "Monitor mode '$mode' exited (code $($process.ExitCode)) before creating a main window."
            }
            Write-ModeDiagnostics $process $mode
            throw "Monitor mode '$mode' did not create a main window within 12 s."
        }
        Write-Output "Monitor mode '$mode' created its main window after $attemptsUsed poll(s)."
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

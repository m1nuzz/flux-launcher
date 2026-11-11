[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$Executable,
    [Parameter(Mandatory = $true)]
    [string]$WorkDirectory,
    [int]$TimeoutMs = 2500
)

$ErrorActionPreference = "Stop"

# A launch that cannot take the foreground must not leave a visible-but-inactive
# shell: typing would go to whatever kept the focus, and no outside click can
# dismiss the window because it never received an activation to lose. The app
# may either own the foreground or sit hidden in the tray; anything else is the
# bug this smoke exists for. No synthetic input is allowed to force the focus:
# a key pressed into another app is a side effect the launcher must not have.
#
# The refusal is real: Windows denies activation to a process started by a
# background owner. Explorer, the shortcut and the tray grant it, which is why
# ordinary launches hide the defect. The second scenario sets
# WINDUI_SMOKE_DENY_FOREGROUND, so the tray fallback is verified even on a
# runner where the plain launch would have won the foreground.

if (!(Test-Path -LiteralPath $Executable)) {
    throw "Executable not found: $Executable"
}
$Executable = (Resolve-Path -LiteralPath $Executable).Path
if (!(Test-Path -LiteralPath $WorkDirectory)) {
    New-Item -ItemType Directory -Path $WorkDirectory | Out-Null
}

$name = [System.IO.Path]::GetFileNameWithoutExtension($Executable)
if (@(Get-Process -Name $name -ErrorAction SilentlyContinue).Count -gt 0) {
    throw "Another $name process is running; the single-instance handoff would hide the launch under test."
}

Add-Type @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public class FluxLaunchFocusProbe {
    public delegate bool EnumProc(IntPtr hWnd, IntPtr lParam);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr lParam);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr hWnd, StringBuilder text, int count);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();

    public static IntPtr FindFluxWindow(uint targetPid) {
        IntPtr found = IntPtr.Zero;
        EnumWindows((hWnd, lParam) => {
            uint pid;
            GetWindowThreadProcessId(hWnd, out pid);
            if (pid == targetPid) {
                var title = new StringBuilder(64);
                GetWindowTextW(hWnd, title, 64);
                if (title.ToString() == "Flux Launcher") {
                    found = hWnd;
                    return false;
                }
            }
            return true;
        }, IntPtr.Zero);
        return found;
    }
}
"@

function Invoke-LaunchProbe([bool]$DenyForeground) {
    if ($DenyForeground) {
        $env:WINDUI_SMOKE_DENY_FOREGROUND = "1"
    } else {
        Remove-Item Env:WINDUI_SMOKE_DENY_FOREGROUND -ErrorAction SilentlyContinue
    }
    $settleMs = 300
    $process = Start-Process -FilePath $Executable -WorkingDirectory $WorkDirectory -PassThru
    try {
        $deadline = (Get-Date).AddMilliseconds($TimeoutMs)
        $window = [IntPtr]::Zero
        $becameForeground = $false
        $everVisible = $false
        $foregroundSince = $null
        $hiddenSince = $null
        $outcome = ""
        while ((Get-Date) -lt $deadline) {
            Start-Sleep -Milliseconds 100
            $process.Refresh()
            if ($process.HasExited) {
                throw "Flux exited during the launch under test with code $($process.ExitCode)."
            }
            $candidate = [FluxLaunchFocusProbe]::FindFluxWindow([uint32]$process.Id)
            if ($candidate -eq [IntPtr]::Zero) {
                continue
            }
            $window = $candidate
            $now = Get-Date
            $visible = [FluxLaunchFocusProbe]::IsWindowVisible($window)
            $foreground = [FluxLaunchFocusProbe]::GetForegroundWindow() -eq $window
            if ($visible) { $everVisible = $true }
            if ($foreground) {
                $becameForeground = $true
                $hiddenSince = $null
                if ($null -eq $foregroundSince) { $foregroundSince = $now }
                if (($now - $foregroundSince).TotalMilliseconds -ge $settleMs) {
                    $outcome = "foreground"
                    break
                }
            } elseif (!$visible) {
                $foregroundSince = $null
                if ($null -eq $hiddenSince) { $hiddenSince = $now }
                if (($now - $hiddenSince).TotalMilliseconds -ge $settleMs) {
                    $outcome = "hidden"
                    break
                }
            } else {
                # Visible but inactive: allow a transient, never a steady state.
                $foregroundSince = $null
                $hiddenSince = $null
            }
        }

        if ($window -eq [IntPtr]::Zero) {
            throw "The launch created no Flux Launcher window within $TimeoutMs ms."
        }
        $process.Refresh()
        if ($process.HasExited) {
            throw "Flux exited after the launch under test with code $($process.ExitCode)."
        }
        return [pscustomobject]@{
            Denied           = $DenyForeground
            Outcome          = $outcome
            Window           = $window
            EverVisible      = $everVisible
            BecameForeground = $becameForeground
            Visible          = [FluxLaunchFocusProbe]::IsWindowVisible($window)
            Foreground       = [FluxLaunchFocusProbe]::GetForegroundWindow() -eq $window
        }
    } finally {
        if (!$process.HasExited) {
            Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
        }
        Remove-Item Env:WINDUI_SMOKE_DENY_FOREGROUND -ErrorAction SilentlyContinue
        $settleDeadline = (Get-Date).AddSeconds(10)
        while ((Get-Date) -lt $settleDeadline) {
            if (@(Get-Process -Name $name -ErrorAction SilentlyContinue).Count -eq 0) {
                break
            }
            Start-Sleep -Milliseconds 100
        }
    }
}

function Write-Probe([string]$label, $probe) {
    Write-Host ("${label}: hwnd=$($probe.Window) outcome=$($probe.Outcome) everVisible=$($probe.EverVisible) visibleNow=$($probe.Visible) becameForeground=$($probe.BecameForeground) foregroundNow=$($probe.Foreground)")
}

$normal = Invoke-LaunchProbe $false
Write-Probe "Launch focus" $normal
switch ($normal.Outcome) {
    "foreground" { Write-Host "The launch owns the foreground." }
    "hidden" { Write-Host "The launch was dismissed back to the tray; the hotkey opens it." }
    default {
        throw "Flux is visible but not the foreground window: typing goes to another app and an outside click cannot hide a window that never activated. The launch must either take the foreground or dismiss itself back to the tray."
    }
}

$denied = Invoke-LaunchProbe $true
Write-Probe "Launch focus (refused activation)" $denied
if ($denied.Outcome -ne "hidden") {
    throw "A refused activation left Flux as a visible window instead of dismissing it to the tray (outcome=$($denied.Outcome))."
}
Write-Host "A refused activation dismisses the window to the tray."

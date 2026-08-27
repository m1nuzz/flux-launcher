param(
    [Parameter(Mandatory = $true)]
    [string]$Executable,

    [Parameter(Mandatory = $true)]
    [string]$OutputDirectory
)

$ErrorActionPreference = "Stop"

Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;

public static class FluxPromptSmokeNative {
    [StructLayout(LayoutKind.Sequential)]
    public struct RECT {
        public int Left;
        public int Top;
        public int Right;
        public int Bottom;
    }

    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool EnumWindows(EnumWindowsProc callback, IntPtr lParam);

    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool IsWindowVisible(IntPtr hWnd);

    [DllImport("user32.dll", SetLastError = true)]
    public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint processId);

    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);

    [DllImport("user32.dll", SetLastError = true)]
    public static extern IntPtr GetForegroundWindow();

    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool SetForegroundWindow(IntPtr hWnd);

    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool SetCursorPos(int x, int y);

    [DllImport("user32.dll", SetLastError = true)]
    public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extraInfo);

    public delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);

    public static IntPtr FindWindowByProcessId(uint targetProcessId) {
        IntPtr found = IntPtr.Zero;
        EnumWindows((hWnd, lParam) => {
            uint processId;
            GetWindowThreadProcessId(hWnd, out processId);
            if (processId == targetProcessId) {
                found = hWnd;
                return false;
            }
            return true;
        }, IntPtr.Zero);
        return found;
    }

    public static IntPtr FindVisibleWindowByProcessId(uint targetProcessId) {
        IntPtr found = IntPtr.Zero;
        EnumWindows((hWnd, lParam) => {
            uint processId;
            GetWindowThreadProcessId(hWnd, out processId);
            if (processId == targetProcessId && IsWindowVisible(hWnd)) {
                found = hWnd;
                return false;
            }
            return true;
        }, IntPtr.Zero);
        return found;
    }

    public static int RectWidth(RECT rect) { return rect.Right - rect.Left; }
    public static int RectHeight(RECT rect) { return rect.Bottom - rect.Top; }
}
'@

function Get-WindowForProcess([System.Diagnostics.Process]$Process, [bool]$VisibleOnly = $false) {
    try {
        $Process.Refresh()
        if ($Process.MainWindowHandle -ne [IntPtr]::Zero) {
            if (!$VisibleOnly -or [FluxPromptSmokeNative]::IsWindowVisible($Process.MainWindowHandle)) {
                return $Process.MainWindowHandle
            }
        }
        if ($VisibleOnly) {
            return [FluxPromptSmokeNative]::FindVisibleWindowByProcessId([uint32]$Process.Id)
        }
        return [FluxPromptSmokeNative]::FindWindowByProcessId([uint32]$Process.Id)
    } catch {
        return [IntPtr]::Zero
    }
}

function Save-DesktopScreenshot([string]$Path) {
    $screen = [System.Windows.Forms.SystemInformation]::VirtualScreen
    $bitmap = New-Object System.Drawing.Bitmap $screen.Width, $screen.Height
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    try {
        $graphics.CopyFromScreen($screen.Left, $screen.Top, 0, 0, $screen.Size)
        $bitmap.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)
    } finally {
        $graphics.Dispose()
        $bitmap.Dispose()
    }
}

New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null
$appData = Join-Path $OutputDirectory "AppData"
$fluxConfigDirectory = Join-Path $appData "FluxLauncher"
New-Item -ItemType Directory -Path $fluxConfigDirectory -Force | Out-Null
$env:APPDATA = $appData
$env:FLUX_SMOKE_EVERYTHING_MISSING = "1"
$env:FLUX_SMOKE_EVERYTHING_PROMPT = "1"
$env:FLUX_DISABLE_UPDATE_CHECKS = "1"
$env:FLUX_DISABLE_EVERYTHING_PROMPT = "0"
$env:WINDUI_D2D = "0"

@'
{
  "start_with_windows": false,
  "auto_enable_everything": true,
  "everything_install_prompt_seen": false,
  "update_checks_enabled": false,
  "clear_query_on_activation": true
}
'@ | Set-Content -LiteralPath (Join-Path $fluxConfigDirectory "settings.json") -Encoding utf8

$stdoutPath = Join-Path $OutputDirectory "launcher.stdout.log"
$stderrPath = Join-Path $OutputDirectory "launcher.stderr.log"
$summaryPath = Join-Path $OutputDirectory "everything-install-prompt-summary.json"
$screenshotPath = Join-Path $OutputDirectory "everything-install-prompt.png"
$process = $null
$secondProcess = $null
$summary = [ordered]@{
    PromptExpected = $true
    ProcessId = 0
    WindowHandle = "0"
    WindowVisible = $false
    ForegroundWindowHandle = "0"
    ForegroundMatchesFlux = $false
    WindowWidth = 0
    WindowHeight = 0
    PromptGeometryProbe = $false
    PromptContentProbe = $false
    PromptGlassStyleProbe = $false
    PromptDismissProbe = $false
    PromptDismissedWindowWidth = 0
    PromptDismissedWindowHeight = 0
    RefusalPersistedToSettings = $false
    SecondLaunchProcessId = 0
    SecondLaunchWindowHandle = "0"
    SecondLaunchWindowVisible = $false
    SecondLaunchForegroundMatchesFlux = $false
    SecondLaunchWindowWidth = 0
    SecondLaunchWindowHeight = 0
    SecondLaunchPromptTelemetryAbsent = $false
    SecondLaunchCompactGeometryProbe = $false
    PromptSuppressedOnSecondLaunch = $false
    EverythingProcessCountBefore = 0
    EverythingProcessCountAfter = 0
    Error = $null
}

try {
    $summary.EverythingProcessCountBefore = @(Get-Process -Name "Everything" -ErrorAction SilentlyContinue).Count
    $process = Start-Process -FilePath $Executable -PassThru -RedirectStandardOutput $stdoutPath -RedirectStandardError $stderrPath
    $deadline = (Get-Date).AddSeconds(10)
    $window = [IntPtr]::Zero
    while ((Get-Date) -lt $deadline -and $window -eq [IntPtr]::Zero) {
        $window = Get-WindowForProcess $process
        if ($window -eq [IntPtr]::Zero) {
            Start-Sleep -Milliseconds 50
        }
    }
    if ($window -eq [IntPtr]::Zero) {
        throw "Everything install prompt smoke could not find the Flux window."
    }
    $summary.WindowHandle = $window.ToInt64().ToString()
    [FluxPromptSmokeNative]::SetForegroundWindow($window) | Out-Null
    Start-Sleep -Milliseconds 250
    $visibleWindow = Get-WindowForProcess $process $true
    $summary.WindowVisible = $visibleWindow -ne [IntPtr]::Zero
    $summary.ForegroundWindowHandle = [FluxPromptSmokeNative]::GetForegroundWindow().ToInt64().ToString()
    $summary.ForegroundMatchesFlux = $summary.ForegroundWindowHandle -eq $summary.WindowHandle
    $rect = New-Object FluxPromptSmokeNative+RECT
    if (![FluxPromptSmokeNative]::GetWindowRect($window, [ref]$rect)) {
        throw "Everything install prompt smoke could not measure the Flux window."
    }
    $summary.WindowWidth = [FluxPromptSmokeNative]::RectWidth($rect)
    $summary.WindowHeight = [FluxPromptSmokeNative]::RectHeight($rect)
    $summary.PromptGeometryProbe = $summary.WindowWidth -ge 400 -and $summary.WindowHeight -ge 180
    Save-DesktopScreenshot $screenshotPath
    Start-Sleep -Milliseconds 250
    $stderr = if (Test-Path $stderrPath) { Get-Content $stderrPath -Raw } else { "" }
    $summary.PromptContentProbe = $stderr -match "Everything install prompt: visible at startup"
    $summary.PromptGlassStyleProbe = $stderr -match "Everything install prompt style: glass"
    if (!$summary.PromptContentProbe) {
        throw "Everything install prompt telemetry was not observed; the screenshot must not be treated as prompt proof."
    }

    # Dismiss through the real prompt button without launching winget. The prompt's
    # panel is centered inside the fixed 440x230 window; scale the safe click for DPI.
    $windowScale = $summary.WindowWidth / 440.0
    $notNowX = $rect.Left + [int][Math]::Round(177 * $windowScale)
    $notNowY = $rect.Bottom - [int][Math]::Round(20 * $windowScale)
    [FluxPromptSmokeNative]::SetCursorPos($notNowX, $notNowY) | Out-Null
    [FluxPromptSmokeNative]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
    [FluxPromptSmokeNative]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 700
    $dismissedRect = New-Object FluxPromptSmokeNative+RECT
    if (![FluxPromptSmokeNative]::GetWindowRect($window, [ref]$dismissedRect)) {
        throw "Everything install prompt smoke could not measure the window after Not now."
    }
    $summary.PromptDismissedWindowWidth = [FluxPromptSmokeNative]::RectWidth($dismissedRect)
    $summary.PromptDismissedWindowHeight = [FluxPromptSmokeNative]::RectHeight($dismissedRect)
    $summary.PromptDismissProbe = $summary.PromptDismissedWindowWidth -ge 400 -and $summary.PromptDismissedWindowHeight -le 100
    Save-DesktopScreenshot (Join-Path $OutputDirectory "everything-install-prompt-dismissed.png")

    $settingsPath = Join-Path $fluxConfigDirectory "settings.json"
    $persistedSettings = Get-Content -LiteralPath $settingsPath -Raw | ConvertFrom-Json
    $summary.RefusalPersistedToSettings = [bool]$persistedSettings.everything_install_prompt_seen
    if (!$summary.RefusalPersistedToSettings) {
        throw "Not now did not persist everything_install_prompt_seen=true in settings.json."
    }

    # Restart the real process with the same isolated APPDATA and missing fixture.
    # The second launch must stay compact and must not emit the startup prompt probe.
    Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
    $process.WaitForExit(5000)
    if (!$process.HasExited) {
        throw "Everything install prompt smoke could not fully stop the first Flux process before relaunch."
    }
    Start-Sleep -Milliseconds 300
    $process = $null
    $secondStdoutPath = Join-Path $OutputDirectory "launcher-second.stdout.log"
    $secondStderrPath = Join-Path $OutputDirectory "launcher-second.stderr.log"
    $secondProcess = Start-Process -FilePath $Executable -PassThru -RedirectStandardOutput $secondStdoutPath -RedirectStandardError $secondStderrPath
    $summary.SecondLaunchProcessId = $secondProcess.Id
    Start-Sleep -Milliseconds 150
    if ($secondProcess.HasExited) {
        throw "Everything install prompt smoke second Flux process exited before it could create its own window."
    }
    $secondDeadline = (Get-Date).AddSeconds(10)
    $secondWindow = [IntPtr]::Zero
    while ((Get-Date) -lt $secondDeadline -and $secondWindow -eq [IntPtr]::Zero) {
        $secondWindow = Get-WindowForProcess $secondProcess
        if ($secondWindow -eq [IntPtr]::Zero) {
            Start-Sleep -Milliseconds 50
        }
    }
    if ($secondWindow -eq [IntPtr]::Zero) {
        throw "Everything install prompt smoke could not find the Flux window on the second launch."
    }
    $summary.SecondLaunchWindowHandle = $secondWindow.ToInt64().ToString()
    [FluxPromptSmokeNative]::SetForegroundWindow($secondWindow) | Out-Null
    Start-Sleep -Milliseconds 350
    $secondVisibleWindow = Get-WindowForProcess $secondProcess $true
    $summary.SecondLaunchWindowVisible = $secondVisibleWindow -ne [IntPtr]::Zero
    $summary.SecondLaunchForegroundMatchesFlux = [FluxPromptSmokeNative]::GetForegroundWindow().ToInt64().ToString() -eq $summary.SecondLaunchWindowHandle
    $secondRect = New-Object FluxPromptSmokeNative+RECT
    if (![FluxPromptSmokeNative]::GetWindowRect($secondWindow, [ref]$secondRect)) {
        throw "Everything install prompt smoke could not measure the Flux window on the second launch."
    }
    $summary.SecondLaunchWindowWidth = [FluxPromptSmokeNative]::RectWidth($secondRect)
    $summary.SecondLaunchWindowHeight = [FluxPromptSmokeNative]::RectHeight($secondRect)
    $summary.SecondLaunchCompactGeometryProbe = $summary.SecondLaunchWindowWidth -ge 400 -and $summary.SecondLaunchWindowHeight -le 100
    Save-DesktopScreenshot (Join-Path $OutputDirectory "everything-install-prompt-second-launch.png")
    Start-Sleep -Milliseconds 250
    $secondStderr = if (Test-Path $secondStderrPath) { Get-Content $secondStderrPath -Raw } else { "" }
    $summary.SecondLaunchPromptTelemetryAbsent = -not [regex]::IsMatch([string]$secondStderr, "Everything install prompt: visible at startup")
    $summary.PromptSuppressedOnSecondLaunch = $summary.RefusalPersistedToSettings -and $summary.SecondLaunchPromptTelemetryAbsent -and $summary.SecondLaunchCompactGeometryProbe
    $summary.EverythingProcessCountAfter = @(Get-Process -Name "Everything" -ErrorAction SilentlyContinue).Count
    $summary | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $summaryPath -Encoding utf8

    if (!$summary.WindowVisible) {
        throw "Everything install prompt window was not visible."
    }
    if (!$summary.PromptGeometryProbe) {
        throw "Everything install prompt window was undersized: $($summary.WindowWidth)x$($summary.WindowHeight)."
    }
    if (!$summary.PromptGlassStyleProbe) {
        throw "Everything install prompt did not report the modern glass style path."
    }
    if (!$summary.PromptDismissProbe) {
        throw "Everything install prompt did not return to compact geometry after Not now: $($summary.PromptDismissedWindowWidth)x$($summary.PromptDismissedWindowHeight)."
    }
    if (!$summary.RefusalPersistedToSettings) {
        throw "Everything install prompt refusal was not persisted."
    }
    if (!$summary.PromptSuppressedOnSecondLaunch) {
        throw "Everything install prompt reappeared or expanded on the second launch: $($summary.SecondLaunchWindowWidth)x$($summary.SecondLaunchWindowHeight), telemetry_absent=$($summary.SecondLaunchPromptTelemetryAbsent)."
    }
    if ($summary.EverythingProcessCountAfter -gt ($summary.EverythingProcessCountBefore + 1)) {
        throw "Everything install prompt created duplicate Everything processes: before=$($summary.EverythingProcessCountBefore) after=$($summary.EverythingProcessCountAfter)."
    }
    Write-Host "Everything install prompt smoke passed: first launch ${($summary.WindowWidth)}x${($summary.WindowHeight)}, refusal persisted=$($summary.RefusalPersistedToSettings), second launch ${($summary.SecondLaunchWindowWidth)}x${($summary.SecondLaunchWindowHeight)} without prompt, Everything process count remained idempotent."
} catch {
    $summary.Error = $_.Exception.Message
    $summary.EverythingProcessCountAfter = @(Get-Process -Name "Everything" -ErrorAction SilentlyContinue).Count
    $summary | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $summaryPath -Encoding utf8
    if (Test-Path $stderrPath) {
        Write-Host "Everything prompt stderr tail:"
        Get-Content $stderrPath -Tail 80
    }
    throw
} finally {
    foreach ($candidate in @($process, $secondProcess)) {
        if ($null -ne $candidate) {
            try {
                if (!$candidate.HasExited) {
                    Stop-Process -Id $candidate.Id -Force -ErrorAction SilentlyContinue
                }
            } catch {
            }
        }
    }
    Remove-Item Env:FLUX_SMOKE_EVERYTHING_MISSING -ErrorAction SilentlyContinue
    Remove-Item Env:FLUX_SMOKE_EVERYTHING_PROMPT -ErrorAction SilentlyContinue
    Remove-Item Env:FLUX_DISABLE_EVERYTHING_PROMPT -ErrorAction SilentlyContinue
}

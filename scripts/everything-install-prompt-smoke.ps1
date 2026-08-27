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
    $summary.EverythingProcessCountAfter = @(Get-Process -Name "Everything" -ErrorAction SilentlyContinue).Count
    $summary | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $summaryPath -Encoding utf8

    if (!$summary.WindowVisible) {
        throw "Everything install prompt window was not visible."
    }
    if (!$summary.PromptGeometryProbe) {
        throw "Everything install prompt window was undersized: $($summary.WindowWidth)x$($summary.WindowHeight)."
    }
    if ($summary.EverythingProcessCountAfter -gt ($summary.EverythingProcessCountBefore + 1)) {
        throw "Everything install prompt created duplicate Everything processes: before=$($summary.EverythingProcessCountBefore) after=$($summary.EverythingProcessCountAfter)."
    }
    Write-Host "Everything install prompt smoke passed: visible ${($summary.WindowWidth)}x${($summary.WindowHeight)}, foreground=$($summary.ForegroundMatchesFlux), Everything process count remained idempotent."
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
    if ($null -ne $process) {
        try {
            if (!$process.HasExited) {
                Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
            }
        } catch {
        }
    }
    Remove-Item Env:FLUX_SMOKE_EVERYTHING_MISSING -ErrorAction SilentlyContinue
    Remove-Item Env:FLUX_DISABLE_EVERYTHING_PROMPT -ErrorAction SilentlyContinue
}

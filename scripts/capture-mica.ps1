param(
    [Parameter(Mandatory = $true)]
    [string]$Executable,

    [Parameter(Mandatory = $true)]
    [string]$OutputDirectory,

    [switch]$ForceTranslucentFallback,
    [switch]$TraySettingsSmoke,
    [switch]$VisualSettingsSmoke,
    [switch]$PointerInteractionSmoke,
    [switch]$EverythingMissingSmoke,
    [switch]$RecycleBinSmoke,
    [switch]$CursorVisibilitySmoke,
    [switch]$ScrollbarGapSmoke,
    [switch]$ActionBarSmoke,
    [switch]$CommandPrioritySmoke,
    [switch]$QueryClearOnReopenSmoke,
    [switch]$QueryResponsivenessSmoke,
    [switch]$FocusToggleSmoke,
    [switch]$DeactivationClickSmoke,
    [switch]$FolderLaunchSmoke,
    [switch]$CtrlRSmoke,
    [switch]$CtrlCSmoke,
    [switch]$IdlePerformanceSmoke,
    [string]$NavigationQuery = "wab",

    [int]$NavigationCycles = 0,

    [int]$TabNavigationCycles = 0,

    # Windows-hosted runners can occasionally spend a few hundred milliseconds
    # inside a synchronous SendMessage callback while starting the shell worker.
    [int]$EnterHideDispatchBudgetMilliseconds = 750,

    # Typing must remain responsive while Shell icons and Everything results load.
    [int]$QueryKeystrokeBudgetMilliseconds = 180
)

$ErrorActionPreference = "Stop"

if ($EverythingMissingSmoke) {
    $env:FLUX_SMOKE_EVERYTHING_MISSING = "1"
} else {
            Remove-Item Env:FLUX_SMOKE_EVERYTHING_MISSING -ErrorAction SilentlyContinue

}

if ($ForceTranslucentFallback) {
    # Force the same no-DWM path used by remote sessions and unsupported systems.
    $env:FLUX_DISABLE_SYSTEM_BACKDROP = "1"
} else {
    Remove-Item Env:FLUX_DISABLE_SYSTEM_BACKDROP -ErrorAction SilentlyContinue
}

New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null

Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class FluxWallpaper {
    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    public static extern bool SystemParametersInfo(uint action, uint parameter, string value, uint flags);
    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll", SetLastError = true)]
    public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extraInfo);
    [DllImport("user32.dll", SetLastError = true)]
    public static extern void keybd_event(byte virtualKey, byte scanCode, uint flags, UIntPtr extraInfo);
    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool SetForegroundWindow(IntPtr hwnd);
    [DllImport("user32.dll", SetLastError = true)]
    public static extern IntPtr SetFocus(IntPtr hwnd);
    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool IsWindowVisible(IntPtr hwnd);
    [DllImport("user32.dll", SetLastError = true)]
    public static extern IntPtr GetWindowLongPtr(IntPtr hwnd, int index);
    [DllImport("user32.dll", SetLastError = true)]
    public static extern IntPtr SendMessage(IntPtr hwnd, uint message, UIntPtr wParam, IntPtr lParam);
    [DllImport("user32.dll", SetLastError = true)]
    public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll", SetLastError = true)]
    public static extern IntPtr WindowFromPoint(POINT point);
    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    public static extern int GetClassName(IntPtr hWnd, char[] className, int maxCount);
    public static string WindowClassAtPoint(int x, int y) {
        IntPtr hwnd = WindowFromPoint(new POINT { X = x, Y = y });
        if (hwnd == IntPtr.Zero) return "<none>";
        char[] buffer = new char[256];
        int length = GetClassName(hwnd, buffer, buffer.Length);
        return length > 0 ? new string(buffer, 0, length) : "<unknown>";
    }
    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool EnumWindows(EnumWindowsProc callback, IntPtr lParam);
    [DllImport("user32.dll", SetLastError = true)]
    public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint processId);
    public delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);
    public static IntPtr FindWindowByProcessId(uint targetProcessId) {
        return FindWindowByProcessId(targetProcessId, false);
    }
    public static IntPtr FindVisibleWindowByProcessId(uint targetProcessId) {
        return FindWindowByProcessId(targetProcessId, true);
    }
    private static IntPtr FindWindowByProcessId(uint targetProcessId, bool visibleOnly) {
        IntPtr found = IntPtr.Zero;
        EnumWindows((hWnd, lParam) => {
            uint processId;
            GetWindowThreadProcessId(hWnd, out processId);
            if (processId == targetProcessId && (!visibleOnly || IsWindowVisible(hWnd))) {
                found = hWnd;
                return false;
            }
            return true;
        }, IntPtr.Zero);
        return found;
    }
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
    public static extern bool GetClientRect(IntPtr hwnd, out RECT rect);
    [DllImport("user32.dll", SetLastError = true)]
    public static extern uint GetDpiForWindow(IntPtr hwnd);
    public static int RectWidth(RECT rect) { return rect.Right - rect.Left; }
    public static int RectHeight(RECT rect) { return rect.Bottom - rect.Top; }
    [StructLayout(LayoutKind.Sequential)]
    public struct CURSORINFO {
        public int cbSize;
        public int flags;
        public IntPtr hCursor;
        public POINT ptScreen;
    }
    [StructLayout(LayoutKind.Sequential)]
    public struct POINT {
        public int X;
        public int Y;
    }
    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool GetCursorInfo(out CURSORINFO info);
    public static bool IsCursorVisible() {
        CURSORINFO info = new CURSORINFO();
        info.cbSize = Marshal.SizeOf(typeof(CURSORINFO));
        return GetCursorInfo(out info) && (info.flags & 0x00000001) != 0;
    }
}
'@

function Get-MemorySnapshot([int]$ProcessId) {
    $sample = Get-Process -Id $ProcessId
    [ordered]@{
        WorkingSetBytes = [int64]$sample.WorkingSet64
        PrivateBytes = [int64]$sample.PrivateMemorySize64
        VirtualBytes = [int64]$sample.VirtualMemorySize64
    }
}

function Get-CpuTimeMilliseconds([int]$ProcessId) {
    $sample = Get-Process -Id $ProcessId
    return $sample.TotalProcessorTime.TotalMilliseconds
}

function Get-LauncherWindowHandle([System.Diagnostics.Process]$Process) {
    for ($attempt = 0; $attempt -lt 60; $attempt++) {
        try {
            $Process.Refresh()
            $handle = $Process.MainWindowHandle
            if ($handle -eq [IntPtr]::Zero) {
                $handle = [FluxWallpaper]::FindWindowByProcessId([uint32]$Process.Id)
            }
            if ($handle -ne [IntPtr]::Zero) {
                return $handle
            }
        }
        catch {
            if ($attempt -eq 59) {
                throw
            }
        }
        Start-Sleep -Milliseconds 250
    }
    return [IntPtr]::Zero
}

function Save-Screenshot([string]$FileName) {
    $bounds = [System.Windows.Forms.SystemInformation]::VirtualScreen
    $bitmap = New-Object System.Drawing.Bitmap $bounds.Width, $bounds.Height
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    try {
        $graphics.CopyFromScreen($bounds.Left, $bounds.Top, 0, 0, $bounds.Size)
        $bitmap.Save((Join-Path $OutputDirectory $FileName), [System.Drawing.Imaging.ImageFormat]::Png)
    }
    finally {
        $graphics.Dispose()
        $bitmap.Dispose()
    }
}

function Compare-ScreenshotRegion(
    [string]$FirstPath,
    [string]$SecondPath,
    [int]$X,
    [int]$Y,
    [int]$Width,
    [int]$Height
) {
    $first = New-Object System.Drawing.Bitmap $FirstPath
    $second = New-Object System.Drawing.Bitmap $SecondPath
    try {
        $total = 0L
        $samples = 0
        for ($py = 0; $py -lt $Height; $py += 3) {
            for ($px = 0; $px -lt $Width; $px += 3) {
                $a = $first.GetPixel($X + $px, $Y + $py)
                $b = $second.GetPixel($X + $px, $Y + $py)
                $total += [Math]::Abs($a.R - $b.R) + [Math]::Abs($a.G - $b.G) + [Math]::Abs($a.B - $b.B)
                $samples++
            }
        }
        return $samples -gt 0 -and (($total / [double]$samples) -lt 18.0)
    }
    finally {
        $first.Dispose()
        $second.Dispose()
    }
}

$wallpaperPath = Join-Path $OutputDirectory "mica-probe-wallpaper.png"
$wallpaper = New-Object System.Drawing.Bitmap 1920, 1080
$wallpaperGraphics = [System.Drawing.Graphics]::FromImage($wallpaper)
try {
    $left = [System.Drawing.Color]::FromArgb(255, 21, 46, 105)
    $right = [System.Drawing.Color]::FromArgb(255, 154, 41, 99)
    $brush = New-Object System.Drawing.Drawing2D.LinearGradientBrush(
        (New-Object System.Drawing.Rectangle 0, 0, 1920, 1080),
        $left,
        $right,
        0.0
    )
    try {
        $wallpaperGraphics.FillRectangle($brush, 0, 0, 1920, 1080)
        $wallpaperGraphics.FillEllipse([System.Drawing.Brushes]::Gold, 760, 210, 420, 420)
        $wallpaper.Save($wallpaperPath, [System.Drawing.Imaging.ImageFormat]::Png)
    }
    finally {
        $brush.Dispose()
    }
}
finally {
    $wallpaperGraphics.Dispose()
    $wallpaper.Dispose()
}

if (![FluxWallpaper]::SystemParametersInfo(0x0014, 0, $wallpaperPath, 0x0003)) {
    throw "Unable to set the Mica probe wallpaper."
}
Start-Sleep -Milliseconds 750
# Put the launcher over the synthetic wallpaper, not over the runner terminal. This makes
# backdrop sampling visible in the screenshot and avoids mistaking a solid panel for glass.
[FluxWallpaper]::keybd_event(0x5B, 0, 0, [UIntPtr]::Zero)
[FluxWallpaper]::keybd_event(0x44, 0, 0, [UIntPtr]::Zero)
[FluxWallpaper]::keybd_event(0x44, 0, 2, [UIntPtr]::Zero)
[FluxWallpaper]::keybd_event(0x5B, 0, 2, [UIntPtr]::Zero)
Start-Sleep -Milliseconds 750

Get-Process | Where-Object { $_.MainWindowTitle -like "*System Properties*" } | Stop-Process -Force -ErrorAction SilentlyContinue
$probeScriptPath = Join-Path $OutputDirectory "probe-screen.ps1"
@'
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
$form = New-Object System.Windows.Forms.Form
$form.FormBorderStyle = [System.Windows.Forms.FormBorderStyle]::None
$form.ShowInTaskbar = $false
$form.StartPosition = [System.Windows.Forms.FormStartPosition]::Manual
$screen = [System.Windows.Forms.SystemInformation]::VirtualScreen
$form.Location = New-Object System.Drawing.Point($screen.Left + 1, $screen.Top + 1)
$form.Size = New-Object System.Drawing.Size($screen.Width - 2, $screen.Height - 2)
$form.BackColor = [System.Drawing.Color]::FromArgb(21, 46, 105)
$form.Add_Paint({
    param($sender, $event)
    $rect = $sender.ClientRectangle
    $left = [System.Drawing.Color]::FromArgb(255, 21, 46, 105)
    $right = [System.Drawing.Color]::FromArgb(255, 154, 41, 99)
    $gradient = New-Object System.Drawing.Drawing2D.LinearGradientBrush($rect, $left, $right, 0.0)
    $event.Graphics.FillRectangle($gradient, $rect)
    $gradient.Dispose()
    $diameter = [Math]::Min($sender.ClientSize.Width, $sender.ClientSize.Height) * 0.32
    $x = ($sender.ClientSize.Width - $diameter) / 2.0
    $y = ($sender.ClientSize.Height - $diameter) / 2.0
    $gold = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::Gold)
    $event.Graphics.FillEllipse($gold, $x, $y, $diameter, $diameter)
    $gold.Dispose()
})
[System.Windows.Forms.Application]::Run($form)
'@ | Set-Content -Encoding utf8 $probeScriptPath
$probeStdoutPath = Join-Path $OutputDirectory "probe.stdout.log"
$probeStderrPath = Join-Path $OutputDirectory "probe.stderr.log"
$probeProcess = Start-Process -FilePath "pwsh" -ArgumentList @("-NoProfile", "-File", $probeScriptPath) -WindowStyle Hidden -RedirectStandardOutput $probeStdoutPath -RedirectStandardError $probeStderrPath -PassThru
Start-Sleep -Seconds 2

# Seed temporary Start Menu shortcuts so the WAB smoke exercises the same
# application-catalog path as a real Windows installation.
$wabFixtureRoot = Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs\Flux Smoke WAB"
New-Item -ItemType Directory -Force -Path $wabFixtureRoot | Out-Null
$folderFixtureName = "FluxFolderSmoke_{0}" -f $PID
$folderFixtureRoot = Join-Path $env:TEMP $folderFixtureName
New-Item -ItemType Directory -Force -Path $folderFixtureRoot | Out-Null
$wabFixtureNames = @(
    "WAB Primary Application.lnk",
    "WAB Secondary Application.lnk",
    "WAB Microsoft Windows Web Account Manager Diagnostic Resource Long Name.lnk",
    "WAB Microsoft Windows Web Account Manager Support Center Long Name.lnk"
)
$shortcutShell = New-Object -ComObject WScript.Shell
$absoluteExecutable = [System.IO.Path]::GetFullPath($Executable)
foreach ($fixtureName in $wabFixtureNames) {
    $shortcut = $shortcutShell.CreateShortcut((Join-Path $wabFixtureRoot $fixtureName))
    $shortcut.TargetPath = $absoluteExecutable
    $shortcut.WorkingDirectory = Split-Path -Parent $absoluteExecutable
    $shortcut.Description = "Flux WAB smoke application fixture"
    $shortcut.Save()
}
$launchProbeShortcut = $shortcutShell.CreateShortcut((Join-Path $wabFixtureRoot "Zq7LaunchProbe.lnk"))
$launchProbeShortcut.TargetPath = Join-Path $env:WINDIR "System32\cmd.exe"
$launchProbeShortcut.Arguments = "/c exit"
$launchProbeShortcut.WorkingDirectory = $env:WINDIR
$launchProbeShortcut.Description = "Zq7LaunchProbe deterministic process creation smoke fixture"
$launchProbeShortcut.Save()

$existingEverythingGuideIds = @(
    Get-Process | Where-Object {
        $_.MainWindowTitle -like "Command Line Options - Everything*"
    } | Select-Object -ExpandProperty Id
)
$stdoutPath = Join-Path $OutputDirectory "launcher.stdout.log"
$stderrPath = Join-Path $OutputDirectory "launcher.stderr.log"
$launchTracePath = Join-Path $OutputDirectory "launch-trace.log"
Remove-Item $launchTracePath -Force -ErrorAction SilentlyContinue
$env:FLUX_LAUNCH_TRACE_FILE = $launchTracePath
if ($ActionBarSmoke) {
    $env:FLUX_SMOKE_ACTION_BAR = "1"
} else {
    Remove-Item Env:FLUX_SMOKE_ACTION_BAR -ErrorAction SilentlyContinue
}
$process = Start-Process -FilePath $Executable -PassThru -RedirectStandardOutput $stdoutPath -RedirectStandardError $stderrPath
try {
    Start-Sleep -Seconds 3
    $newEverythingGuideWindows = @(
        Get-Process | Where-Object {
            $_.MainWindowTitle -like "Command Line Options - Everything*" -and
            $existingEverythingGuideIds -notcontains $_.Id
        }
    )
    $everythingStartupProbe = $newEverythingGuideWindows.Count -eq 0
    if (!$everythingStartupProbe) {
        throw "Everything startup opened its command-line guide window."
    }
    $idleMemory = Get-MemorySnapshot $process.Id
    Save-Screenshot "mica-desktop.png"

    # Regression probe: exercise the real global Alt+Space hide/show path twice
    # while the query is empty. Capture this before any query edit, so a later
    # repaint cannot hide a failure where the DirectComposition surface attaches
    # only after typing.
    $launcherHandle = Get-LauncherWindowHandle $process
    if ($launcherHandle -eq [IntPtr]::Zero) { throw "Flux launcher has no main window handle after waiting for startup." }
    if (![FluxWallpaper]::IsWindowVisible($launcherHandle)) {
        # With hide-on-deactivate enabled, the runner can take focus before the
        # first sample. Restore the window through the real global activation bind.
        [FluxWallpaper]::keybd_event(0x12, 0, 0, [UIntPtr]::Zero)
        [FluxWallpaper]::keybd_event(0x20, 0, 0, [UIntPtr]::Zero)
        [FluxWallpaper]::keybd_event(0x20, 0, 2, [UIntPtr]::Zero)
        [FluxWallpaper]::keybd_event(0x12, 0, 2, [UIntPtr]::Zero)
        Start-Sleep -Milliseconds 800
    }
    if (![FluxWallpaper]::IsWindowVisible($launcherHandle)) { throw "Launcher is not visible before hotkey regression probe." }
    $exStyle = [FluxWallpaper]::GetWindowLongPtr($launcherHandle, -20).ToInt64()
    $trayOnlyWindow = (($exStyle -band 0x00000080) -ne 0) -and (($exStyle -band 0x00040000) -eq 0)
    if (!$trayOnlyWindow) { throw "Tray-only window style regression: exStyle=0x$('{0:X}' -f $exStyle)" }
    [FluxWallpaper]::SetForegroundWindow($launcherHandle) | Out-Null
    Start-Sleep -Milliseconds 250
    # WM_HOTKEY is the exact message delivered by RegisterHotKey. Send it
    # synchronously to the real HWND so the second toggle cannot race the first
    # hidden-window transition or a queued message on the runner.
    $wmHotkey = 0x0312
    [FluxWallpaper]::SendMessage($launcherHandle, $wmHotkey, [UIntPtr]::Zero, [IntPtr]::Zero) | Out-Null
    Start-Sleep -Milliseconds 450
    $hiddenAfterFirstHotkey = ![FluxWallpaper]::IsWindowVisible($launcherHandle)
    if ($IdlePerformanceSmoke) {
        if (!$hiddenAfterFirstHotkey) {
            throw "Idle performance probe could not hide the launcher."
        }
        # Let queued hide work and startup activity settle before sampling.
        Start-Sleep -Milliseconds 1500
        $idleCpuBefore = Get-CpuTimeMilliseconds $process.Id
        Start-Sleep -Seconds 3
        $idleCpuAfter = Get-CpuTimeMilliseconds $process.Id
        $idleCpuDelta = [Math]::Round($idleCpuAfter - $idleCpuBefore, 2)
        Write-Host "Hidden idle CPU time over 3s: $idleCpuDelta ms"
        # 150 ms over 3 seconds is a 5% single-process CPU ceiling. This catches
        # timer/render busy loops while allowing normal Windows background noise.
        if ($idleCpuDelta -gt 150) {
            throw "Hidden idle CPU budget exceeded: ${idleCpuDelta} ms over 3 seconds."
        }
    }
    [FluxWallpaper]::SendMessage($launcherHandle, $wmHotkey, [UIntPtr]::Zero, [IntPtr]::Zero) | Out-Null
    Start-Sleep -Milliseconds 600
    $visibleAfterSecondHotkey = [FluxWallpaper]::IsWindowVisible($launcherHandle)
    if (!$hiddenAfterFirstHotkey -or !$visibleAfterSecondHotkey) {
        throw "Alt+Space visibility regression: hidden=$hiddenAfterFirstHotkey visible=$visibleAfterSecondHotkey"
    }
    $focusToggleProbe = $false
    $focusToggleVisibleAfterReopen = $false
    $focusToggleForegroundAfterReopen = $false
    $deactivationClickProbe = $false
    $deactivationHiddenAfterClick = $false
    $deactivationForegroundAfterClick = $false
    $deactivationCpuDelta = 0.0
    if ($FocusToggleSmoke) {
        $probeProcess.Refresh()
        $focusProbeHandle = $probeProcess.MainWindowHandle
        if ($focusProbeHandle -eq [IntPtr]::Zero) {
            $focusProbeHandle = [FluxWallpaper]::FindWindowByProcessId([uint32]$probeProcess.Id)
        }
        if ($focusProbeHandle -eq [IntPtr]::Zero) {
            throw "Foreground handoff smoke could not find the deterministic probe window."
        }
        [FluxWallpaper]::SetForegroundWindow($focusProbeHandle) | Out-Null
        Start-Sleep -Milliseconds 350
        if ([FluxWallpaper]::GetForegroundWindow() -eq $launcherHandle) {
            # In some CI environments, SetForegroundWindow might be ignored if the
            # runner doesn't have focus. Try a real click on the probe window.
            $probeRect = New-Object FluxWallpaper+RECT
            if ([FluxWallpaper]::GetWindowRect($focusProbeHandle, [ref]$probeRect)) {
                $clickX = [int](($probeRect.Left + $probeRect.Right) / 2)
                $clickY = [int](($probeRect.Top + $probeRect.Bottom) / 2)
                [FluxWallpaper]::SetCursorPos($clickX, $clickY) | Out-Null
                [FluxWallpaper]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
                [FluxWallpaper]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
                Start-Sleep -Milliseconds 450
            }
        }
        if ([FluxWallpaper]::GetForegroundWindow() -eq $launcherHandle) {
            throw "Foreground handoff smoke could not deactivate the launcher."
        }
        if ([FluxWallpaper]::IsWindowVisible($launcherHandle)) {
            throw "Foreground handoff smoke expected the launcher HWND to hide after another window became foreground."
        }
        # Use the real configured default Alt+Space key sequence here. Unlike
        # SendMessage(WM_HOTKEY), this grants the launcher the same foreground
        # activation permission it receives from an actual user bind.
        [FluxWallpaper]::keybd_event(0x12, 0, 0, [UIntPtr]::Zero)
        [FluxWallpaper]::keybd_event(0x20, 0, 0, [UIntPtr]::Zero)
        [FluxWallpaper]::keybd_event(0x20, 0, 2, [UIntPtr]::Zero)
        [FluxWallpaper]::keybd_event(0x12, 0, 2, [UIntPtr]::Zero)
        Start-Sleep -Milliseconds 900
        $focusToggleVisibleAfterReopen = [FluxWallpaper]::IsWindowVisible($launcherHandle)
        $focusToggleForegroundAfterReopen = [FluxWallpaper]::GetForegroundWindow() -eq $launcherHandle
        $focusToggleProbe = $focusToggleVisibleAfterReopen -and $focusToggleForegroundAfterReopen
        if (!$focusToggleProbe) {
            throw "Foreground handoff smoke failed: one bind after another window was activated did not show and activate Flux."
        }
        [FluxWallpaper]::SetForegroundWindow($launcherHandle) | Out-Null
    }
    if ($DeactivationClickSmoke) {
        $probeProcess.Refresh()
        $deactivationProbeHandle = $probeProcess.MainWindowHandle
        if ($deactivationProbeHandle -eq [IntPtr]::Zero) {
            $deactivationProbeHandle = [FluxWallpaper]::FindVisibleWindowByProcessId([uint32]$probeProcess.Id)
        }
        if ($deactivationProbeHandle -eq [IntPtr]::Zero) {
            throw "Deactivation smoke could not find the deterministic probe window."
        }
        $probeRect = New-Object FluxWallpaper+RECT
        if (![FluxWallpaper]::GetWindowRect($deactivationProbeHandle, [ref]$probeRect)) {
            throw "Deactivation smoke could not read the probe window rectangle."
        }
        $deactivationTraceBeforeCount = if (Test-Path $launchTracePath) { @(Get-Content $launchTracePath).Count } else { 0 }
        [FluxWallpaper]::SetForegroundWindow($launcherHandle) | Out-Null
        Start-Sleep -Milliseconds 250
        if (![FluxWallpaper]::IsWindowVisible($launcherHandle)) {
            throw "Deactivation smoke expected Flux to be visible before the outside click."
        }
        $clickX = [int](($probeRect.Left + $probeRect.Right) / 2)
        $clickY = [int](($probeRect.Top + $probeRect.Bottom) / 2)
        [FluxWallpaper]::SetCursorPos($clickX, $clickY) | Out-Null
        [FluxWallpaper]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
        [FluxWallpaper]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
        Start-Sleep -Milliseconds 700
        $deactivationHiddenAfterClick = ![FluxWallpaper]::IsWindowVisible($launcherHandle)
        $deactivationForegroundAfterClick = [FluxWallpaper]::GetForegroundWindow() -ne $launcherHandle
        $deactivationTraceLines = if (Test-Path $launchTracePath) {
            @(Get-Content $launchTracePath | Select-Object -Skip $deactivationTraceBeforeCount)
        } else {
            @()
        }
        $deactivationEvent = $deactivationTraceLines | Where-Object { $_ -match "`twindow-deactivated$" } | Select-Object -First 1
        $deactivationClickProbe =
            $deactivationHiddenAfterClick -and
            $deactivationForegroundAfterClick -and
            [bool]$deactivationEvent
        if (!$deactivationClickProbe) {
            throw "Deactivation smoke failed: hidden=$deactivationHiddenAfterClick foreground_probe=$deactivationForegroundAfterClick callback=$([bool]$deactivationEvent)."
        }
        if ($IdlePerformanceSmoke) {
            Start-Sleep -Milliseconds 1200
            $deactivationCpuBefore = Get-CpuTimeMilliseconds $process.Id
            Start-Sleep -Seconds 3
            $deactivationCpuAfter = Get-CpuTimeMilliseconds $process.Id
            $deactivationCpuDelta = [Math]::Round($deactivationCpuAfter - $deactivationCpuBefore, 2)
            Write-Host "Click-hidden idle CPU time over 3s: $deactivationCpuDelta ms"
            if ($deactivationCpuDelta -gt 150) {
                throw "Click-hidden idle CPU budget exceeded: ${deactivationCpuDelta} ms over 3 seconds."
            }
        }
        # Restore through the real global Alt+Space sequence, proving that the hidden
        # process remains resident and the user can immediately reopen the launcher.
        [FluxWallpaper]::keybd_event(0x12, 0, 0, [UIntPtr]::Zero)
        [FluxWallpaper]::keybd_event(0x20, 0, 0, [UIntPtr]::Zero)
        [FluxWallpaper]::keybd_event(0x20, 0, 2, [UIntPtr]::Zero)
        [FluxWallpaper]::keybd_event(0x12, 0, 2, [UIntPtr]::Zero)
        Start-Sleep -Milliseconds 900
        if (![FluxWallpaper]::IsWindowVisible($launcherHandle) -or [FluxWallpaper]::GetForegroundWindow() -ne $launcherHandle) {
            throw "Deactivation smoke could not restore Flux with one real Alt+Space bind."
        }
    }

    Save-Screenshot "mica-repeat-show-empty.png"
    $compactClientRect = New-Object FluxWallpaper+RECT
    if (![FluxWallpaper]::GetClientRect($launcherHandle, [ref]$compactClientRect)) {
        throw "Unable to read compact launcher client geometry."
    }
    $compactDpi = [FluxWallpaper]::GetDpiForWindow($launcherHandle)
    if ($compactDpi -eq 0) { $compactDpi = 96 }
    $compactScale = [double]$compactDpi / 96.0
    $compactLogicalHeight = [int][Math]::Round(([FluxWallpaper]::RectHeight($compactClientRect)) / $compactScale)
    $compactLayoutProbe = $compactLogicalHeight -eq 56
    Write-Host "Compact launcher geometry: client_height=$compactLogicalHeight dpi=$compactDpi"
    if (!$compactLayoutProbe) {
        throw "Compact launcher has unexpected logical height: $compactLogicalHeight (expected 56)."
    }
    $launcherRect = New-Object FluxWallpaper+RECT
    if (![FluxWallpaper]::GetWindowRect($launcherHandle, [ref]$launcherRect)) {
        throw "Unable to locate launcher rectangle before typing the navigation query."
    }
    $searchX = $launcherRect.Left + [int](($launcherRect.Right - $launcherRect.Left) / 2)
    $searchY = $launcherRect.Top + [int](($launcherRect.Bottom - $launcherRect.Top) / 2)
    $shell = New-Object -ComObject WScript.Shell
    [FluxWallpaper]::SetForegroundWindow($launcherHandle) | Out-Null
    $shell.AppActivate($process.Id) | Out-Null
    Start-Sleep -Milliseconds 300
    [FluxWallpaper]::SetCursorPos($searchX, $searchY) | Out-Null
    [FluxWallpaper]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
    [FluxWallpaper]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 300
    [FluxWallpaper]::SetForegroundWindow($launcherHandle) | Out-Null
    Start-Sleep -Milliseconds 150
    $cursorVisibleOnActivation = [FluxWallpaper]::IsCursorVisible()
    $cursorHiddenAfterTyping = $false
    $cursorVisibleAfterMove = $false
    $scrollbarGapProbe = $false
    $actionBarProbe = $false
    $actionBarGeometry = $null
    $queryClearOnReopenProbe = $false
    $ctrlRProbe = $false
    $ctrlCProbe = $false
    $queryResponsivenessSamples = @()
    $queryResponsivenessMaxMilliseconds = 0.0
    $queryResponsivenessProbe = $false
    $commandPriorityProbe = $false
    if ($CommandPrioritySmoke) {
        foreach ($commandQuery in @("cmd", "powershell", "pwsh")) {
            $shell.SendKeys("^a")
            $shell.SendKeys("{BACKSPACE}")
            $shell.SendKeys($commandQuery)
            Start-Sleep -Milliseconds 700
            Save-Screenshot ("command-priority-{0}.png" -f $commandQuery)
        }
        $commandPriorityProbe = $true
    }
    if ($CursorVisibilitySmoke) {
        $shell.SendKeys("x")
        Start-Sleep -Milliseconds 500
        $cursorHiddenAfterTyping = ![FluxWallpaper]::IsCursorVisible()
        [FluxWallpaper]::SetCursorPos($searchX + 6, $searchY + 6) | Out-Null
        Start-Sleep -Milliseconds 350
        $cursorVisibleAfterMove = [FluxWallpaper]::IsCursorVisible()
        if (!$cursorVisibleOnActivation -or !$cursorHiddenAfterTyping -or !$cursorVisibleAfterMove) {
            throw "Cursor visibility regression: activation=$cursorVisibleOnActivation hidden_after_typing=$cursorHiddenAfterTyping visible_after_move=$cursorVisibleAfterMove"
        }
        $shell.SendKeys("^a")
        $shell.SendKeys("{BACKSPACE}")
        Start-Sleep -Milliseconds 350
    }
    # Use a deterministic built-in Windows target for the launch/hide ordering probe;
    # Recycle Bin commands are covered separately and the first one intentionally
    # opens a confirmation mode instead of dispatching a shell launch.
    $enterHideQuery = "wifi"
    $navigationProbeQuery = if ($NavigationCycles -gt 0 -and $NavigationQuery.Trim().Length -gt 0) {
        $NavigationQuery.Trim()
    } else {
        $enterHideQuery
    }
    if ($QueryResponsivenessSmoke) {
        foreach ($probeQuery in @(".png", ".zip", "ext:zip")) {
            $shell.SendKeys("^a")
            $shell.SendKeys("{BACKSPACE}")
            foreach ($character in $probeQuery.ToCharArray()) {
                $keyTimer = [System.Diagnostics.Stopwatch]::StartNew()
                $shell.SendKeys([string]$character)
                $keyTimer.Stop()
                $elapsedMilliseconds = [Math]::Round($keyTimer.Elapsed.TotalMilliseconds, 2)
                $queryResponsivenessSamples += [ordered]@{
                    Query = $probeQuery
                    Character = [string]$character
                    Milliseconds = $elapsedMilliseconds
                }
                if ($elapsedMilliseconds -gt $queryResponsivenessMaxMilliseconds) {
                    $queryResponsivenessMaxMilliseconds = $elapsedMilliseconds
                }
                if ($elapsedMilliseconds -gt $QueryKeystrokeBudgetMilliseconds) {
                    throw "Query responsiveness regression: '$character' in '$probeQuery' took $elapsedMilliseconds ms (budget $QueryKeystrokeBudgetMilliseconds ms)."
                }
            }
            Start-Sleep -Milliseconds 250
        }
        $queryResponsivenessProbe = $true
    }
    Write-Host "Navigation probe query: $navigationProbeQuery"
    $shell.SendKeys("^a")
    $shell.SendKeys("{BACKSPACE}")
    $shell.SendKeys($navigationProbeQuery)
    Start-Sleep -Seconds 2
    $queryMemory = Get-MemorySnapshot $process.Id
    Save-Screenshot "everything-fallback.png"

    if ($ActionBarSmoke) {
        Start-Sleep -Milliseconds 300
        $clientRect = New-Object FluxWallpaper+RECT
        if (![FluxWallpaper]::GetClientRect($launcherHandle, [ref]$clientRect)) {
            throw "Action bar smoke could not read launcher client geometry."
        }
        $clientWidth = [FluxWallpaper]::RectWidth($clientRect)
        $clientHeight = [FluxWallpaper]::RectHeight($clientRect)
        $dpi = [FluxWallpaper]::GetDpiForWindow($launcherHandle)
        if ($dpi -eq 0) { $dpi = 96 }
        $scale = [double]$dpi / 96.0
        $logicalClientWidth = [int][Math]::Round($clientWidth / $scale)
        $logicalClientHeight = [int][Math]::Round($clientHeight / $scale)
        $actionBarLog = if (Test-Path $stderrPath) { Get-Content $stderrPath -Raw } else { "" }
        $actionBarMatch = [regex]::Matches(
            $actionBarLog,
            "ActionBarGeometry: x=(\d+) y=(\d+) width=(\d+) height=(\d+)"
        ) | Select-Object -Last 1
        if ($null -eq $actionBarMatch) {
            throw "Action bar smoke did not observe native action-bar geometry telemetry."
        }
        $actionBarGeometry = [ordered]@{
            X = [int]$actionBarMatch.Groups[1].Value
            Y = [int]$actionBarMatch.Groups[2].Value
            Width = [int]$actionBarMatch.Groups[3].Value
            Height = [int]$actionBarMatch.Groups[4].Value
            LogicalClientWidth = $logicalClientWidth
            LogicalClientHeight = $logicalClientHeight
            Dpi = [int]$dpi
        }
        $expectedActionBarX = 10 + [int][Math]::Floor(
            [Math]::Max(0, $logicalClientWidth - 20 - 340) / 2.0
        )
        # Intrinsic result/footer layout leaves a 20-DIP bottom inset after
        # balancing the compact Search baseline with the top padding.
        $actionBarBottomInset = 20
        $expectedActionBarY = $logicalClientHeight - $actionBarBottomInset - 22
        $actionBarProbe =
            [Math]::Abs($actionBarGeometry.X - $expectedActionBarX) -le 1 -and
            [Math]::Abs($actionBarGeometry.Y - $expectedActionBarY) -le 1 -and
            $actionBarGeometry.Width -eq 340 -and
            $actionBarGeometry.Height -eq 22 -and
            ($actionBarGeometry.Y + $actionBarGeometry.Height) -eq ($logicalClientHeight - $actionBarBottomInset)
        Write-Host "Action bar geometry: x=$($actionBarGeometry.X) y=$($actionBarGeometry.Y) width=$($actionBarGeometry.Width) height=$($actionBarGeometry.Height) expected_x=$expectedActionBarX expected_y=$expectedActionBarY bottom_inset=$actionBarBottomInset client=${logicalClientWidth}x${logicalClientHeight} dpi=$dpi"
        if (!$actionBarProbe) {
            throw "Action bar geometry is not centered between launcher insets or has the wrong bottom inset: $($actionBarGeometry | ConvertTo-Json -Compress)."
        }
    }

    if ($QueryClearOnReopenSmoke) {
        [FluxWallpaper]::SendMessage($launcherHandle, $wmHotkey, [UIntPtr]::Zero, [IntPtr]::Zero) | Out-Null
        Start-Sleep -Milliseconds 500
        if ([FluxWallpaper]::IsWindowVisible($launcherHandle)) {
            throw "Query-clear smoke could not hide the launcher before reopen."
        }
        [FluxWallpaper]::SendMessage($launcherHandle, $wmHotkey, [UIntPtr]::Zero, [IntPtr]::Zero) | Out-Null
        Start-Sleep -Milliseconds 350
        if (![FluxWallpaper]::IsWindowVisible($launcherHandle)) {
            throw "Query-clear smoke could not show the launcher after hide."
        }
        Save-Screenshot "query-clear-reopen.png"
        $reopenRect = New-Object FluxWallpaper+RECT
        if (![FluxWallpaper]::GetWindowRect($launcherHandle, [ref]$reopenRect)) {
            throw "Unable to locate launcher rectangle for query-clear smoke."
        }
        $emptyScreenshot = Join-Path $OutputDirectory "mica-repeat-show-empty.png"
        $reopenScreenshot = Join-Path $OutputDirectory "query-clear-reopen.png"
        $queryClearOnReopenProbe = Compare-ScreenshotRegion `
            $emptyScreenshot `
            $reopenScreenshot `
            ($reopenRect.Left - [System.Windows.Forms.SystemInformation]::VirtualScreen.Left) `
            ($reopenRect.Top - [System.Windows.Forms.SystemInformation]::VirtualScreen.Top) `
            ($reopenRect.Right - $reopenRect.Left) `
            56
        if (!$queryClearOnReopenProbe) {
            throw "Query-clear smoke detected stale content in the reopened search bar."
        }
    }

    # Regression probe: keep the launcher on one monitor position while
    # repeatedly toggling Alt+Space after a query has expanded the window.
    $repeatedHotkeyYPositions = @()
    for ($cycle = 1; $cycle -le 3; $cycle++) {
        if (![FluxWallpaper]::IsWindowVisible($launcherHandle)) {
            # A previous outside click can legitimately leave the launcher hidden;
            # restore it through the real bind before testing the next hide toggle.
            [FluxWallpaper]::keybd_event(0x12, 0, 0, [UIntPtr]::Zero)
            [FluxWallpaper]::keybd_event(0x20, 0, 0, [UIntPtr]::Zero)
            [FluxWallpaper]::keybd_event(0x20, 0, 2, [UIntPtr]::Zero)
            [FluxWallpaper]::keybd_event(0x12, 0, 2, [UIntPtr]::Zero)
            Start-Sleep -Milliseconds 800
        }
        [FluxWallpaper]::SetForegroundWindow($launcherHandle) | Out-Null
        Start-Sleep -Milliseconds 250
        if (![FluxWallpaper]::IsWindowVisible($launcherHandle) -or [FluxWallpaper]::GetForegroundWindow() -ne $launcherHandle) {
            throw "Repeated Alt+Space cycle $cycle could not establish a visible foreground launcher before hide."
        }
        [FluxWallpaper]::SendMessage($launcherHandle, $wmHotkey, [UIntPtr]::Zero, [IntPtr]::Zero) | Out-Null
        Start-Sleep -Milliseconds 450
        if ([FluxWallpaper]::IsWindowVisible($launcherHandle)) {
            throw "Repeated Alt+Space cycle $cycle did not hide the launcher."
        }
        [FluxWallpaper]::SendMessage($launcherHandle, $wmHotkey, [UIntPtr]::Zero, [IntPtr]::Zero) | Out-Null
        Start-Sleep -Milliseconds 650
        if (![FluxWallpaper]::IsWindowVisible($launcherHandle)) {
            throw "Repeated Alt+Space cycle $cycle did not show the launcher."
        }
        $repeatRect = New-Object FluxWallpaper+RECT
        if (![FluxWallpaper]::GetWindowRect($launcherHandle, [ref]$repeatRect)) {
            throw "Unable to read launcher rectangle during repeated Alt+Space cycle $cycle."
        }
        $repeatedHotkeyYPositions += $repeatRect.Top
        Save-Screenshot ("repeated-hotkey-cycle-{0:D2}.png" -f $cycle)
    }
    $repeatedHotkeyPositionProbe = @($repeatedHotkeyYPositions | Select-Object -Unique).Count -eq 1
    if (!$repeatedHotkeyPositionProbe) {
        throw "Repeated Alt+Space moved the launcher vertically: $($repeatedHotkeyYPositions -join ', ')"
    }

    # The first re-show clears the query by design. Re-focus the search input
    # after the repeated hotkey cycle before restoring a non-empty query; a
    # foreground HWND alone does not guarantee that the custom text input owns focus.
    [FluxWallpaper]::SetForegroundWindow($launcherHandle) | Out-Null
    $shell.AppActivate($process.Id) | Out-Null
    Start-Sleep -Milliseconds 200
    [FluxWallpaper]::SetCursorPos($searchX, $searchY) | Out-Null
    [FluxWallpaper]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
    [FluxWallpaper]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 200
    [FluxWallpaper]::SetForegroundWindow($launcherHandle) | Out-Null
    $inputForegroundDeadline = (Get-Date).AddSeconds(2)
    while ((Get-Date) -lt $inputForegroundDeadline -and [FluxWallpaper]::GetForegroundWindow() -ne $launcherHandle) {
        Start-Sleep -Milliseconds 50
        [FluxWallpaper]::SetForegroundWindow($launcherHandle) | Out-Null
    }
    if ([FluxWallpaper]::GetForegroundWindow() -ne $launcherHandle) {
        throw "Navigation query could not establish Flux as the foreground window before typing."
    }
    # Use real keyboard events after the framework restores its focused TextInput.
    # This exercises the same path as a user typing after the global hotkey.
    foreach ($character in $navigationProbeQuery.ToUpperInvariant().ToCharArray()) {
        $virtualKey = [byte][char]$character
        [FluxWallpaper]::keybd_event($virtualKey, 0, 0, [UIntPtr]::Zero)
        [FluxWallpaper]::keybd_event($virtualKey, 0, 2, [UIntPtr]::Zero)
        Start-Sleep -Milliseconds 35
    }
    Start-Sleep -Seconds 2

    if ($PointerInteractionSmoke) {
        $launcherRect = New-Object FluxWallpaper+RECT
        if (![FluxWallpaper]::GetWindowRect($launcherHandle, [ref]$launcherRect)) {
            throw "Unable to locate launcher rectangle for pointer smoke."
        }
        $resultX = $launcherRect.Left + [int](($launcherRect.Right - $launcherRect.Left) / 2)
        $firstResultY = $launcherRect.Top + 84
        $secondResultY = $launcherRect.Top + 130
        [FluxWallpaper]::SetCursorPos($resultX, $secondResultY) | Out-Null
        Start-Sleep -Milliseconds 500
        Save-Screenshot "pointer-hover-second-result.png"
        [FluxWallpaper]::mouse_event(0x0800, 0, 0, [uint32]4294966816, [UIntPtr]::Zero)
        Start-Sleep -Milliseconds 600
        Save-Screenshot "pointer-wheel-scroll.png"
        [FluxWallpaper]::SetCursorPos($resultX, $firstResultY) | Out-Null
        Start-Sleep -Milliseconds 300
    }
    if ($ScrollbarGapSmoke) {
        $launcherRect = New-Object FluxWallpaper+RECT
        if (![FluxWallpaper]::GetWindowRect($launcherHandle, [ref]$launcherRect)) {
            throw "Unable to locate launcher rectangle for scrollbar gap smoke."
        }
        $scrollX = $launcherRect.Right - 20
        $scrollY = $launcherRect.Top + 180
        [FluxWallpaper]::SetCursorPos($scrollX, $scrollY) | Out-Null
        [FluxWallpaper]::mouse_event(0x0800, 0, 0, [uint32]4294966816, [UIntPtr]::Zero)
        Start-Sleep -Milliseconds 600
        Save-Screenshot "scrollbar-gap.png"
        $scrollbarGapProbe = Test-Path (Join-Path $OutputDirectory "scrollbar-gap.png")
    }
    if ($TabNavigationCycles -gt 0) {
        for ($cycle = 1; $cycle -le $TabNavigationCycles; $cycle++) {
            # Flow Launcher semantics: Tab selects next result and Shift+Tab selects previous.
            $shell.SendKeys("{TAB}")
            Start-Sleep -Milliseconds 250
            Save-Screenshot ("tab-navigation-cycle-{0:D2}-next.png" -f $cycle)
            $shell.SendKeys("+{TAB}")
            Start-Sleep -Milliseconds 250
            Save-Screenshot ("tab-navigation-cycle-{0:D2}-previous.png" -f $cycle)
        }
    }
    if ($NavigationCycles -gt 0) {
        # Monotonic Down navigation is intentional: sending Up immediately
        # before Down would cancel the selection movement and hide a broken
        # viewport update by returning to the first result every cycle.
        for ($cycle = 1; $cycle -le $NavigationCycles; $cycle++) {
            $shell.SendKeys("{DOWN}")
            Start-Sleep -Milliseconds 180
            Save-Screenshot ("navigation-cycle-{0:D2}-down.png" -f $cycle)
        }
    }
    # Restore the launcher first if a previous outside-click probe left it hidden,
    # then select the deterministic Windows target directly on the launcher HWND.
    $wmKeyDown = 0x0100
    if (![FluxWallpaper]::IsWindowVisible($launcherHandle)) {
        [FluxWallpaper]::keybd_event(0x12, 0, 0, [UIntPtr]::Zero)
        [FluxWallpaper]::keybd_event(0x20, 0, 0, [UIntPtr]::Zero)
        [FluxWallpaper]::keybd_event(0x20, 0, 2, [UIntPtr]::Zero)
        [FluxWallpaper]::keybd_event(0x12, 0, 2, [UIntPtr]::Zero)
        Start-Sleep -Milliseconds 800
    }
    [FluxWallpaper]::SetForegroundWindow($launcherHandle) | Out-Null
    $enterQueryForegroundDeadline = (Get-Date).AddSeconds(2)
    while ((Get-Date) -lt $enterQueryForegroundDeadline -and [FluxWallpaper]::GetForegroundWindow() -ne $launcherHandle) {
        Start-Sleep -Milliseconds 50
        [FluxWallpaper]::SetForegroundWindow($launcherHandle) | Out-Null
    }
    if (![FluxWallpaper]::IsWindowVisible($launcherHandle) -or [FluxWallpaper]::GetForegroundWindow() -ne $launcherHandle) {
        throw "Enter launch smoke could not establish a visible foreground launcher before query setup."
    }
    # Every preceding smoke may legitimately hide and re-show Flux, which clears
    # the query. Rebuild the deterministic query immediately before selection.
    [FluxWallpaper]::keybd_event(0x11, 0, 0, [UIntPtr]::Zero)
    [FluxWallpaper]::keybd_event(0x41, 0, 0, [UIntPtr]::Zero)
    [FluxWallpaper]::keybd_event(0x41, 0, 2, [UIntPtr]::Zero)
    [FluxWallpaper]::keybd_event(0x11, 0, 2, [UIntPtr]::Zero)
    $wmChar = 0x0102
    foreach ($character in $navigationProbeQuery.ToCharArray()) {
        [FluxWallpaper]::SendMessage(
            $launcherHandle,
            $wmChar,
            [UIntPtr]::new([int][char]$character),
            [IntPtr]::Zero
        ) | Out-Null
        Start-Sleep -Milliseconds 35
    }
    $foregroundDeadline = (Get-Date).AddSeconds(2)
    while ((Get-Date) -lt $foregroundDeadline -and [FluxWallpaper]::GetForegroundWindow() -ne $launcherHandle) {
        Start-Sleep -Milliseconds 50
        [FluxWallpaper]::SetForegroundWindow($launcherHandle) | Out-Null
    }
    if (![FluxWallpaper]::IsWindowVisible($launcherHandle)) {
        # A CI compositor can briefly activate another helper while the query
        # expands the window. Reopen through the real bind, then rebuild the
        # query through the focused windui input before selecting a result.
        [FluxWallpaper]::SendMessage($launcherHandle, $wmHotkey, [UIntPtr]::Zero, [IntPtr]::Zero) | Out-Null
        Start-Sleep -Milliseconds 800
        [FluxWallpaper]::SetForegroundWindow($launcherHandle) | Out-Null
        $wmChar = 0x0102
        foreach ($character in $navigationProbeQuery.ToCharArray()) {
            [FluxWallpaper]::SendMessage(
                $launcherHandle,
                $wmChar,
                [UIntPtr]::new([int][char]$character),
                [IntPtr]::Zero
            ) | Out-Null
        }
        Start-Sleep -Seconds 2
    }
    if (![FluxWallpaper]::IsWindowVisible($launcherHandle)) {
        throw "Enter launch smoke could not restore a visible launcher HWND before selection."
    }
    if ([FluxWallpaper]::GetForegroundWindow() -ne $launcherHandle) {
        throw "Enter launch smoke could not restore Flux as the foreground window before selection."
    }
    [FluxWallpaper]::SendMessage($launcherHandle, $wmKeyDown, [UIntPtr]::new(0x24), [IntPtr]::Zero) | Out-Null
    [FluxWallpaper]::SendMessage($launcherHandle, $wmKeyDown, [UIntPtr]::new(0x28), [IntPtr]::Zero) | Out-Null
    Start-Sleep -Milliseconds 350
    Save-Screenshot "keyboard-selection.png"
    Start-Sleep -Milliseconds 100
    # Clear modifier state left by the preceding Alt+Space cycles before testing
    # plain Enter; otherwise Flux correctly interprets the key as Alt+Enter.
    [FluxWallpaper]::keybd_event(0x12, 0, 2, [UIntPtr]::Zero)
    [FluxWallpaper]::keybd_event(0x11, 0, 2, [UIntPtr]::Zero)
    [FluxWallpaper]::keybd_event(0x10, 0, 2, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 50
    # Send Enter to the exact launcher HWND after direct Home/Down selection. This
    # keeps the ordering assertion deterministic while the surrounding smoke suite
    # already covers real keyboard input and global hotkey restoration.
    $traceBeforeEnterCount = if (Test-Path $launchTracePath) { @(Get-Content $launchTracePath).Count } else { 0 }
    $enterDispatchTimer = [System.Diagnostics.Stopwatch]::StartNew()
    [FluxWallpaper]::SendMessage($launcherHandle, $wmKeyDown, [UIntPtr]::new(0x0D), [IntPtr]::Zero) | Out-Null
    $enterDispatchTimer.Stop()
    $enterHideDispatchMilliseconds = [Math]::Round($enterDispatchTimer.Elapsed.TotalMilliseconds, 2)
    Start-Sleep -Milliseconds 500
    $enterTraceLines = if (Test-Path $launchTracePath) {
        @(Get-Content $launchTracePath | Select-Object -Skip $traceBeforeEnterCount)
    } else {
        @()
    }
    $launchDispatchLine = $enterTraceLines | Where-Object { $_ -match "`tlaunch-dispatch$" } | Select-Object -First 1
    $windowHideLine = $enterTraceLines | Where-Object { $_ -match "`twindow-hide$" } | Select-Object -First 1
    $processCreatedLine = $enterTraceLines | Where-Object { $_ -match "`tprocess-created$" } | Select-Object -First 1
    $launchDispatchTimestamp = if ($launchDispatchLine) { [double]($launchDispatchLine -split "`t", 2)[0] } else { 0.0 }
    $windowHideTimestamp = if ($windowHideLine) { [double]($windowHideLine -split "`t", 2)[0] } else { 0.0 }
    $processCreatedTimestamp = if ($processCreatedLine) { [double]($processCreatedLine -split "`t", 2)[0] } else { 0.0 }
    $enterLaunchDispatchBeforeHideProbe =
        $launchDispatchTimestamp -gt 0.0 -and
        $windowHideTimestamp -gt 0.0 -and
        $launchDispatchTimestamp -le $windowHideTimestamp
    $enterProcessCreatedBeforeHideProbe =
        $processCreatedTimestamp -gt 0.0 -and
        $windowHideTimestamp -gt 0.0 -and
        $processCreatedTimestamp -le $windowHideTimestamp
    $enterLaunchHidden = ![FluxWallpaper]::IsWindowVisible($launcherHandle)
    $enterHideLatencyProbe =
        $enterLaunchHidden -and
        ($enterHideDispatchMilliseconds -lt $EnterHideDispatchBudgetMilliseconds) -and
        $enterLaunchDispatchBeforeHideProbe
    if (!$enterLaunchHidden) {
        throw "Enter launch did not hide the launcher window."
    }
    if (!$enterHideLatencyProbe) {
        $tracePreview = ($enterTraceLines -join ' | ')
        throw "Enter launch/hide ordering failed: dispatch_before_hide=$enterLaunchDispatchBeforeHideProbe, hide_dispatch_ms=$enterHideDispatchMilliseconds, budget_ms=$EnterHideDispatchBudgetMilliseconds, trace=$tracePreview."
    }
    # Restore the launcher for the remaining independent probes. The Enter hide
    # callback and the next hotkey dispatch can cross on a busy CI compositor, so
    # retry the real toggle within a bounded window instead of relying on one sleep.
    $restoreDeadline = (Get-Date).AddSeconds(5)
    while (![FluxWallpaper]::IsWindowVisible($launcherHandle) -and (Get-Date) -lt $restoreDeadline) {
        [FluxWallpaper]::SendMessage($launcherHandle, $wmHotkey, [UIntPtr]::Zero, [IntPtr]::Zero) | Out-Null
        Start-Sleep -Milliseconds 650
    }
    if (![FluxWallpaper]::IsWindowVisible($launcherHandle)) {
        throw "Unable to restore launcher after Enter hide probe."
    }
    [FluxWallpaper]::SetForegroundWindow($launcherHandle) | Out-Null
    Start-Sleep -Milliseconds 200
    $launchProbeQuery = "zq7launchprobe"
    $shell.SendKeys("^a")
    $shell.SendKeys($launchProbeQuery)
    Start-Sleep -Seconds 2
    $shell.SendKeys("{HOME}")
    Start-Sleep -Milliseconds 250
    [FluxWallpaper]::SetForegroundWindow($launcherHandle) | Out-Null
    $launchProbeTraceBeforeCount = if (Test-Path $launchTracePath) { @(Get-Content $launchTracePath).Count } else { 0 }
    $launchProbeTimer = [System.Diagnostics.Stopwatch]::StartNew()
    [FluxWallpaper]::SendMessage($launcherHandle, $wmKeyDown, [UIntPtr]::new(0x0D), [IntPtr]::Zero) | Out-Null
    $launchProbeTimer.Stop()
    $launchProbeHideDispatchMilliseconds = [Math]::Round($launchProbeTimer.Elapsed.TotalMilliseconds, 2)
    # ShellExecuteEx may create the child after the launcher hide callback on a
    # busy CI runner; wait beyond that asynchronous worker boundary before reading
    # the opt-in lifecycle trace.
    Start-Sleep -Milliseconds 2500
    $launchProbeTraceLines = if (Test-Path $launchTracePath) {
        @(Get-Content $launchTracePath | Select-Object -Skip $launchProbeTraceBeforeCount)
    } else {
        @()
    }
    $launchProbeDispatchLine = $launchProbeTraceLines | Where-Object { $_ -match "`tlaunch-dispatch$" } | Select-Object -First 1
    $launchProbeHideLine = $launchProbeTraceLines | Where-Object { $_ -match "`twindow-hide$" } | Select-Object -First 1
    $launchProbeProcessLine = $launchProbeTraceLines | Where-Object { $_ -match "`tprocess-created$" } | Select-Object -First 1
    $launchProbeShellReturnLine = $launchProbeTraceLines | Where-Object { $_ -match "`tshell-return$" } | Select-Object -First 1
    $launchProbeDispatchTimestamp = if ($launchProbeDispatchLine) { [double]($launchProbeDispatchLine -split "`t", 2)[0] } else { 0.0 }
    $launchProbeHideTimestamp = if ($launchProbeHideLine) { [double]($launchProbeHideLine -split "`t", 2)[0] } else { 0.0 }
    $launchProbeProcessTimestamp = if ($launchProbeProcessLine) { [double]($launchProbeProcessLine -split "`t", 2)[0] } else { 0.0 }
    $launchProbeShellReturnTimestamp = if ($launchProbeShellReturnLine) { [double]($launchProbeShellReturnLine -split "`t", 2)[0] } else { 0.0 }
    $launchProbeCompletionTimestamp = if ($launchProbeProcessTimestamp -gt 0.0) {
        $launchProbeProcessTimestamp
    } else {
        $launchProbeShellReturnTimestamp
    }
    $launchProbeDispatchBeforeHide =
        $launchProbeDispatchTimestamp -gt 0.0 -and
        $launchProbeHideTimestamp -gt 0.0 -and
        $launchProbeDispatchTimestamp -le $launchProbeHideTimestamp
    $launchProbeLaunchSucceeded = $launchProbeCompletionTimestamp -gt 0.0
    $launchProbeLaunchSucceededBeforeHide =
        $launchProbeLaunchSucceeded -and
        $launchProbeHideTimestamp -gt 0.0 -and
        $launchProbeCompletionTimestamp -le $launchProbeHideTimestamp
    $launchProbeProcessCreationMilliseconds = if ($launchProbeLaunchSucceeded) {
        [Math]::Round($launchProbeCompletionTimestamp - $launchProbeDispatchTimestamp, 3)
    } else {
        0.0
    }
    $launchProbeDispatchToHideMilliseconds = if ($launchProbeDispatchBeforeHide) {
        [Math]::Round($launchProbeHideTimestamp - $launchProbeDispatchTimestamp, 3)
    } else {
        0.0
    }
    # Hiding is intentionally synchronous, while ShellExecuteEx runs on Flux's
    # launch worker. Therefore process-created may legitimately arrive after the
    # launcher hide; only dispatch-before-hide and eventual successful launch are
    # ordering requirements.
    $launchProcessCreationProbe =
        $launchProbeDispatchBeforeHide -and $launchProbeLaunchSucceeded
    if (!$launchProcessCreationProbe) {
        throw "Launch process probe failed: dispatch_before_hide=$launchProbeDispatchBeforeHide, launch_succeeded=$launchProbeLaunchSucceeded, completed_before_hide=$launchProbeLaunchSucceededBeforeHide, process_created=$($launchProbeProcessTimestamp -gt 0.0), shell_return=$($launchProbeShellReturnTimestamp -gt 0.0)."
    }
    [FluxWallpaper]::SendMessage($launcherHandle, $wmHotkey, [UIntPtr]::Zero, [IntPtr]::Zero) | Out-Null
    Start-Sleep -Milliseconds 650
    if (![FluxWallpaper]::IsWindowVisible($launcherHandle)) {
        throw "Unable to restore launcher after process creation smoke."
    }

    $folderLaunchProbe = $false
    if ($FolderLaunchSmoke) {
        # Exercise the same launch dispatcher used by a folder: Everything result.
        # A dedicated child mode keeps this regression deterministic even when the
        # hosted runner's Everything index has not picked up a just-created folder yet.
        $folderTracePath = Join-Path $OutputDirectory "folder-launch-trace.log"
        Remove-Item $folderTracePath -Force -ErrorAction SilentlyContinue
        $previousLaunchTracePath = $env:FLUX_LAUNCH_TRACE_FILE
        $env:FLUX_LAUNCH_TRACE_FILE = $folderTracePath
        try {
            $folderSmokeProcess = Start-Process `
                -FilePath $absoluteExecutable `
                -ArgumentList @("--folder-launch-smoke", ('"{0}"' -f $folderFixtureRoot)) `
                -PassThru
            Wait-Process -Id $folderSmokeProcess.Id -Timeout 10
            if (!$folderSmokeProcess.HasExited) {
                Stop-Process -Id $folderSmokeProcess.Id -Force -ErrorAction SilentlyContinue
                throw "Folder launch smoke process did not exit."
            }
        }
        finally {
            if ($null -eq $previousLaunchTracePath) {
                Remove-Item Env:FLUX_LAUNCH_TRACE_FILE -ErrorAction SilentlyContinue
            } else {
                $env:FLUX_LAUNCH_TRACE_FILE = $previousLaunchTracePath
            }
        }
        $folderTraceLines = if (Test-Path $folderTracePath) {
            @(Get-Content $folderTracePath)
        } else {
            @()
        }
        $folderDispatch = $folderTraceLines | Where-Object { $_ -match "`tdirectory-launch-dispatch$" } | Select-Object -First 1
        $folderShellFailure = $folderTraceLines | Where-Object { $_ -match "`tshell-execute-failed$" } | Select-Object -First 1
        $folderLaunchProbe = [bool]$folderDispatch -and !$folderShellFailure
        if (!$folderLaunchProbe) {
            throw "Folder launch probe failed: directory_dispatch=$([bool]$folderDispatch), shell_failure=$([bool]$folderShellFailure)."
        }
    }
    if ($CtrlCSmoke) {
        $shell.SendKeys("^a")
        $shell.SendKeys("wab")
        Start-Sleep -Seconds 2
        $shell.SendKeys("{HOME}")
        Start-Sleep -Milliseconds 250
        [FluxWallpaper]::SetForegroundWindow($launcherHandle) | Out-Null
        $shell.SendKeys("^c")
        Start-Sleep -Milliseconds 350
        try {
            $clipboardText = (Get-Clipboard -Raw -ErrorAction Stop).Trim()
            Write-Host "Ctrl+C clipboard value: [$clipboardText]"
            $ctrlCProbe = $clipboardText.Length -gt 2 -and
                $clipboardText.StartsWith('"') -and
                $clipboardText.EndsWith('"')
        } catch {
            $ctrlCProbe = $false
        }
        if (!$ctrlCProbe) {
            throw "Ctrl+C did not copy the selected result path in quotes."
        }
    }
    if ($CtrlRSmoke) {
        $shell.SendKeys("^a")
        $shell.SendKeys("wab")
        Start-Sleep -Seconds 2
        $shell.SendKeys("{HOME}")
        Start-Sleep -Milliseconds 250
        [FluxWallpaper]::SetForegroundWindow($launcherHandle) | Out-Null
        $shell.SendKeys("^r")
        Start-Sleep -Milliseconds 900
        $ctrlRProbe = ![FluxWallpaper]::IsWindowVisible($launcherHandle)
        if (!$ctrlRProbe) {
            throw "Ctrl+R did not launch the selected result with elevation."
        }
        [FluxWallpaper]::SendMessage($launcherHandle, $wmHotkey, [UIntPtr]::Zero, [IntPtr]::Zero) | Out-Null
        Start-Sleep -Milliseconds 650
        if (![FluxWallpaper]::IsWindowVisible($launcherHandle)) {
            throw "Unable to restore launcher after Ctrl+R smoke."
        }
    }
    if ($RecycleBinSmoke) {
        # Flow-style behavior: Empty and Open are ordinary sibling results,
        # with Empty first. Enter only the Empty row far enough to verify
        # Flux's confirmation dialog, then cancel it without deleting anything.
        $shell.SendKeys("^a")
        $shell.SendKeys("recyclebin")
        Start-Sleep -Milliseconds 900
        $shell.SendKeys("{HOME}")
        Start-Sleep -Milliseconds 350
        Save-Screenshot "recycle-bin-direct-results.png"
        $shell.SendKeys("{ENTER}")
        Start-Sleep -Milliseconds 500
        Save-Screenshot "recycle-bin-empty-confirmation.png"
        $shell.SendKeys("{ESCAPE}")
        Start-Sleep -Milliseconds 350
        $recycleBinDirectResultsProbe = $true
        $recycleBinDestructiveActionInvoked = $false
    } else {
        $recycleBinDirectResultsProbe = $false
        $recycleBinDestructiveActionInvoked = $false
    }
    # Native Everything syntax must remain usable in the always-on provider.
    $shell.SendKeys("^a")
    $shell.SendKeys("ext:zip")
    Start-Sleep -Seconds 2
    Save-Screenshot "everything-ext-zip.png"
    # Commit the syntax query too so history cycling has at least two entries.
    $shell.SendKeys("{ENTER}")
    Start-Sleep -Milliseconds 600
    # Flow-style query history: Ctrl+H opens selectable history results.
    $shell.SendKeys("^h")
    Start-Sleep -Milliseconds 500
    $historyMemory = Get-MemorySnapshot $process.Id
    Save-Screenshot "history-panel.png"
    $shell.SendKeys("{ENTER}")
    Start-Sleep -Seconds 1
    # Plain Up on an empty query recalls the latest committed search.
    $shell.SendKeys("^a")
    $shell.SendKeys("{BACKSPACE}")
    Start-Sleep -Milliseconds 250
    $shell.SendKeys("{UP}")
    Start-Sleep -Seconds 1
    Save-Screenshot "history-up-recall.png"
    # Alt+Up/Alt+Down cycle older/newer committed queries.
    $shell.SendKeys("%{UP}")
    Start-Sleep -Milliseconds 500
    Save-Screenshot "history-alt-up.png"
    $shell.SendKeys("%{DOWN}")
    Start-Sleep -Milliseconds 500
    Save-Screenshot "history-alt-down.png"
    if ($PointerInteractionSmoke) {
        $launcherRect = New-Object FluxWallpaper+RECT
        if (![FluxWallpaper]::GetWindowRect($launcherHandle, [ref]$launcherRect)) {
            throw "Unable to locate launcher rectangle for history click smoke."
        }
        $historyX = $launcherRect.Left + [int](($launcherRect.Right - $launcherRect.Left) / 2)
        $historyY = $launcherRect.Top + 84
        [FluxWallpaper]::SetCursorPos($historyX, $historyY) | Out-Null
        Start-Sleep -Milliseconds 250
        [FluxWallpaper]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
        [FluxWallpaper]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
        Start-Sleep -Milliseconds 800
        Save-Screenshot "pointer-click-history-result.png"
    }
    if ($TraySettingsSmoke) {
        $env:FLUX_SMOKE_TRAY_SETTINGS = "1"
        Remove-Item Env:FLUX_OPEN_SETTINGS -ErrorAction SilentlyContinue
    } else {
        $env:FLUX_OPEN_SETTINGS = "1"
        Remove-Item Env:FLUX_SMOKE_TRAY_SETTINGS -ErrorAction SilentlyContinue
    }
    if ($VisualSettingsSmoke) {
        # The application selects the Visual tab before first paint so this smoke
        # can drag the real fixed-track controls without coordinate-driven tab setup.
        $env:FLUX_SMOKE_SETTINGS_TAB = "1"
        $env:FLUX_SMOKE_VISUAL_SETTINGS = "1"
        # The first launcher process remains alive while this independent Settings
        # process is measured. Production launches still use strict single-instance
        # behavior; this opt-out exists only to exercise Settings in isolation.
        $env:FLUX_DISABLE_SINGLE_INSTANCE = "1"
    } else {
        Remove-Item Env:FLUX_SMOKE_SETTINGS_TAB -ErrorAction SilentlyContinue
        Remove-Item Env:FLUX_SMOKE_VISUAL_SETTINGS -ErrorAction SilentlyContinue
        Remove-Item Env:FLUX_DISABLE_SINGLE_INSTANCE -ErrorAction SilentlyContinue
    }
    $settingsStdoutPath = Join-Path $OutputDirectory "settings.stdout.log"
    $settingsStderrPath = Join-Path $OutputDirectory "settings.stderr.log"
    $settingsProcess = Start-Process -FilePath $Executable -PassThru -RedirectStandardOutput $settingsStdoutPath -RedirectStandardError $settingsStderrPath
    $settingsWindowHeight = 0
    $settingsWindowWidth = 0
    $settingsWindowFound = $false
    $settingsCenterBeforeDragX = 0
    $settingsCenterBeforeDragY = 0
    $settingsCenterAfterDragX = 0
    $settingsCenterAfterDragY = 0
    $settingsCenterDelta = 0
    $visualSettingsSliderProbe = $false
    $visualPreviewUpdateCount = 0
    $visualPreviewGeometrySampleCount = 0
    $visualPreviewExactGeometryProbe = $false
    $visualPreviewCleanupProbe = $false
    $visualPreviewResetProbe = $false
    $visualPreviewProcessId = 0
    $visualPreviewWindowFound = $false
    $visualPreviewGeometrySamples = @()
    $reopenedSettingsProcess = $null
    $reopenedPreviewProcessId = 0
    $visualPreviewPersistenceProbe = $false
    try {
        if ($TraySettingsSmoke) {
            $firstFrameChecked = $false
            for ($attempt = 0; $attempt -lt 30 -and !$firstFrameChecked; $attempt++) {
                Start-Sleep -Milliseconds 100
                $settingsProcess.Refresh()
                $firstFrameHwnd = Get-LauncherWindowHandle $settingsProcess
                if ($firstFrameHwnd -eq [IntPtr]::Zero) {
                    continue
                }
                $firstFrameRect = New-Object FluxWallpaper+RECT
                if (![FluxWallpaper]::GetWindowRect($firstFrameHwnd, [ref]$firstFrameRect)) {
                    continue
                }
                $firstFrameWidth = [FluxWallpaper]::RectWidth($firstFrameRect)
                $firstFrameHeight = [FluxWallpaper]::RectHeight($firstFrameRect)
                Write-Host "Tray Settings first-frame geometry: ${firstFrameWidth}x${firstFrameHeight}"
                if ($firstFrameHeight -lt 480 -or $firstFrameWidth -lt 680) {
                    throw "Tray Settings first frame was undersized: ${firstFrameWidth}x${firstFrameHeight}."
                }
                $firstFrameChecked = $true
            }
            if (!$firstFrameChecked) {
                throw "Tray Settings smoke could not observe the first Settings frame."
            }
        }
        Start-Sleep -Seconds 2
        $settingsProcess.Refresh()
        if ($VisualSettingsSmoke -and $settingsProcess.HasExited) {
            throw "Visual Settings smoke process exited before creating its Settings window with code $($settingsProcess.ExitCode)."
        }
        if ($VisualSettingsSmoke) {
            $settingsHwnd = Get-LauncherWindowHandle $settingsProcess
        } else {
            $settingsHwnd = $settingsProcess.MainWindowHandle
            if ($settingsHwnd -eq [IntPtr]::Zero) {
                $settingsHwnd = [FluxWallpaper]::GetForegroundWindow()
            }
        }
        if ($settingsHwnd -ne [IntPtr]::Zero) {
            $settingsRect = New-Object FluxWallpaper+RECT
            if ([FluxWallpaper]::GetWindowRect($settingsHwnd, [ref]$settingsRect)) {
                $settingsWindowHeight = [FluxWallpaper]::RectHeight($settingsRect)
                $settingsWindowWidth = [FluxWallpaper]::RectWidth($settingsRect)
                $settingsWindowFound = $true
            }
        }
        if ($VisualSettingsSmoke) {
            if (!$settingsWindowFound) {
                throw "Visual Settings smoke could not find the Settings window."
            }
            $settingsCenterBeforeDragX = [int](($settingsRect.Left + $settingsRect.Right) / 2)
            $settingsCenterBeforeDragY = [int](($settingsRect.Top + $settingsRect.Bottom) / 2)
            if ($settingsWindowWidth -lt 680 -or $settingsWindowHeight -lt 480) {
                throw "Visual Settings smoke found an undersized Settings canvas: ${settingsWindowWidth}x${settingsWindowHeight}."
            }
            [FluxWallpaper]::SetForegroundWindow($settingsHwnd) | Out-Null
            Start-Sleep -Milliseconds 350
            $settingsDpi = [FluxWallpaper]::GetDpiForWindow($settingsHwnd)
            if ($settingsDpi -eq 0) { $settingsDpi = 96 }
            $settingsScale = $settingsDpi / 96.0

            # The parent logs the preview child PID only after spawning the same executable
            # in --visual-preview mode. Locate the real second HWND by that PID, never by
            # foreground-window heuristics or by a scaled in-Settings illustration.
            $previewChildProcessId = 0
            $previewHwnd = [IntPtr]::Zero
            for ($attempt = 0; $attempt -lt 60; $attempt++) {
                $previewLog = if (Test-Path $settingsStderrPath) {
                    Get-Content $settingsStderrPath -Raw
                } else {
                    ""
                }
                $pidMatch = [regex]::Match($previewLog, "Visual preview process started: pid=(\d+)")
                if ($pidMatch.Success) {
                    $previewChildProcessId = [int]$pidMatch.Groups[1].Value
                    $candidate = [FluxWallpaper]::FindVisibleWindowByProcessId([uint32]$previewChildProcessId)
                    if ($candidate -ne [IntPtr]::Zero) {
                        $previewHwnd = $candidate
                        break
                    }
                }
                Start-Sleep -Milliseconds 100
            }
            if ($previewChildProcessId -eq 0 -or $previewHwnd -eq [IntPtr]::Zero) {
                throw "Visual Settings smoke could not locate the READY native preview child HWND by its reported PID."
            }
            $visualPreviewProcessId = $previewChildProcessId
            $visualPreviewWindowFound = $true
            if ([FluxWallpaper]::GetForegroundWindow() -ne $settingsHwnd) {
                throw "Visual preview startup activated the child HWND and displaced the Settings foreground window."
            }
            $previewExStyle = [FluxWallpaper]::GetWindowLongPtr($previewHwnd, -20).ToInt64()
            if (($previewExStyle -band 0x08000000) -eq 0) {
                throw "Visual preview HWND is missing WS_EX_NOACTIVATE; clicking it could hide Settings."
            }

            $initialClientRect = New-Object FluxWallpaper+RECT
            $initialWindowRect = New-Object FluxWallpaper+RECT
            if (![FluxWallpaper]::GetClientRect($previewHwnd, [ref]$initialClientRect) -or
                ![FluxWallpaper]::GetWindowRect($previewHwnd, [ref]$initialWindowRect)) {
                throw "Visual Settings smoke could not read initial preview HWND geometry."
            }
            $initialDpi = [FluxWallpaper]::GetDpiForWindow($previewHwnd)
            if ($initialDpi -eq 0) { $initialDpi = 96 }
            $initialClientWidth = [FluxWallpaper]::RectWidth($initialClientRect)
            $initialClientHeight = [FluxWallpaper]::RectHeight($initialClientRect)
            $initialGeometryMatch = $null
            for ($attempt = 0; $attempt -lt 30 -and $null -eq $initialGeometryMatch; $attempt++) {
                $previewLog = if (Test-Path $settingsStderrPath) { Get-Content $settingsStderrPath -Raw } else { "" }
                $geometryCandidates = [regex]::Matches(
                    $previewLog,
                    "VisualPreviewChild: GEOMETRY $visualPreviewProcessId (\d+) (\d+) (\d+) (\d+) (\d+)"
                )
                if ($geometryCandidates.Count -gt 0) {
                    $initialGeometryMatch = $geometryCandidates[$geometryCandidates.Count - 1]
                } else {
                    Start-Sleep -Milliseconds 100
                }
            }
            if ($null -eq $initialGeometryMatch) {
                throw "Visual Settings smoke did not receive initial GetClientRect telemetry from preview PID $visualPreviewProcessId."
            }
            $initialLogicalWidth = [int]$initialGeometryMatch.Groups[1].Value
            $initialLogicalHeight = [int]$initialGeometryMatch.Groups[2].Value
            $initialReportedClientWidth = [int]$initialGeometryMatch.Groups[3].Value
            $initialReportedClientHeight = [int]$initialGeometryMatch.Groups[4].Value
            $initialReportedDpi = [int]$initialGeometryMatch.Groups[5].Value
            $expectedInitialWidth = [int][Math]::Floor(($initialLogicalWidth * $initialReportedDpi / 96.0) + 0.5)
            $expectedInitialHeight = [int][Math]::Floor(($initialLogicalHeight * $initialReportedDpi / 96.0) + 0.5)
            if ($initialClientWidth -ne $initialReportedClientWidth -or
                $initialClientHeight -ne $initialReportedClientHeight -or
                $initialClientWidth -ne $expectedInitialWidth -or
                $initialClientHeight -ne $expectedInitialHeight) {
                throw "Initial preview client geometry mismatch: requested=${initialLogicalWidth}x${initialLogicalHeight}, reported=${initialReportedClientWidth}x${initialReportedClientHeight}, HWND=${initialClientWidth}x${initialClientHeight}, dpi=${initialReportedDpi}, expected=${expectedInitialWidth}x${expectedInitialHeight}."
            }

            # Settings has stable 18px page padding + 24px panel padding, a 110px
            # field-label column, a 200px slider, a 76px numeric field, and a Reset button.
            Save-Screenshot "settings-visual-discovery.png"
            [ordered]@{
                SettingsLeft = $settingsRect.Left
                SettingsTop = $settingsRect.Top
                SettingsRight = $settingsRect.Right
                SettingsBottom = $settingsRect.Bottom
                SettingsDpi = $settingsDpi
                SettingsScale = $settingsScale
                SliderCandidateXOffsets = @(140, 150, 160, 170, 180, 190, 200)
                SliderCandidateYOffsets = @(200, 212, 224, 236, 248, 260, 272, 284, 296, 308, 320, 332, 344, 356, 368, 380, 392, 404, 416, 428, 440, 452, 464, 476, 488, 500, 512, 524, 536, 548, 560)
            } | ConvertTo-Json | Set-Content -Encoding utf8 (Join-Path $OutputDirectory "visual-discovery-geometry.json")
            $directSliderLeft = $settingsRect.Left + [int][Math]::Round(175 * $settingsScale)
            $directSliderRight = $directSliderLeft + [int][Math]::Round(180 * $settingsScale)
            $directSliderY = $settingsRect.Top + [int][Math]::Round(310 * $settingsScale)
            $directPointClass = [FluxWallpaper]::WindowClassAtPoint($directSliderLeft, $directSliderY)
            Write-Host "Visual slider direct probe: left=$directSliderLeft right=$directSliderRight y=$directSliderY windowClass=$directPointClass"
            $directStateBefore = if (Test-Path $settingsStderrPath) { Get-Content $settingsStderrPath -Raw } else { "" }
            [FluxWallpaper]::SetCursorPos($directSliderLeft, $directSliderY) | Out-Null
            [FluxWallpaper]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
            for ($step = 0; $step -le 10; $step++) {
                $x = $directSliderLeft + [int](($directSliderRight - $directSliderLeft) * $step / 10.0)
                [FluxWallpaper]::SetCursorPos($x, $directSliderY) | Out-Null
                Start-Sleep -Milliseconds 45
            }
            [FluxWallpaper]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
            Start-Sleep -Milliseconds 400
            $directStateAfter = Get-Content $settingsStderrPath -Raw
            $directGeometry = [regex]::Matches(
                $directStateAfter,
                "VisualPreviewChild: GEOMETRY $visualPreviewProcessId (\d+) (\d+) (\d+) (\d+) (\d+)"
            )
            $directWidthChanged = $false
            foreach ($match in $directGeometry) {
                if ([int]$match.Groups[1].Value -ne $initialLogicalWidth -and
                    [int]$match.Groups[2].Value -eq $initialLogicalHeight) {
                    $directWidthChanged = $true
                    break
                }
            }
            $sliderLeft = $directSliderLeft
            $sliderRight = $directSliderRight
            $widthSliderY = $directSliderY
            if (!$directWidthChanged) {
                $sliderLeft = 0
                $sliderRight = 0
                $widthSliderY = 0
            }
            if (!$directWidthChanged) {
                foreach ($sliderOffset in (140, 150, 160, 170, 180, 190, 200)) {
                    $candidateLeft = $settingsRect.Left + [int][Math]::Round($sliderOffset * $settingsScale)
                    $candidateRight = $candidateLeft + [int][Math]::Round(190 * $settingsScale)
                    foreach ($candidateOffset in (200..560 | Where-Object { $_ % 12 -eq 8 })) {
                        $candidateY = $settingsRect.Top + [int][Math]::Round($candidateOffset * $settingsScale)
                        [FluxWallpaper]::SetCursorPos($candidateLeft, $candidateY) | Out-Null
                        [FluxWallpaper]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
                        for ($step = 0; $step -le 5; $step++) {
                            $x = $candidateLeft + [int](($candidateRight - $candidateLeft) * $step / 5.0)
                            [FluxWallpaper]::SetCursorPos($x, $candidateY) | Out-Null
                            Start-Sleep -Milliseconds 35
                        }
                        [FluxWallpaper]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
                        Start-Sleep -Milliseconds 220
                        $afterCandidateLog = Get-Content $settingsStderrPath -Raw
                        $candidateGeometry = [regex]::Matches(
                            $afterCandidateLog,
                            "VisualPreviewChild: GEOMETRY $visualPreviewProcessId (\d+) (\d+) (\d+) (\d+) (\d+)"
                        )
                        foreach ($match in $candidateGeometry) {
                            if ([int]$match.Groups[1].Value -ne $initialLogicalWidth -and
                                [int]$match.Groups[2].Value -eq $initialLogicalHeight) {
                                $sliderLeft = $candidateLeft
                                $sliderRight = $candidateRight
                                $widthSliderY = $candidateY
                                break
                            }
                        }
                        if ($widthSliderY -ne 0) { break }
                    }
                    if ($widthSliderY -ne 0) { break }
                }
            }
            if ($widthSliderY -eq 0) {
                throw "Visual Settings smoke could not identify the width slider from native preview geometry telemetry."
            }

            $heightDirectY = $settingsRect.Top + [int][Math]::Round(388 * $settingsScale)
            $heightDirectPointClass = [FluxWallpaper]::WindowClassAtPoint($sliderLeft, $heightDirectY)
            Write-Host "Visual results-height direct probe: left=$sliderLeft right=$sliderRight y=$heightDirectY windowClass=$heightDirectPointClass"
            [FluxWallpaper]::SetCursorPos($sliderRight, $heightDirectY) | Out-Null
            [FluxWallpaper]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
            for ($step = 0; $step -le 10; $step++) {
                $fraction = 1.0 - ($step / 10.0)
                $x = $sliderLeft + [int](($sliderRight - $sliderLeft) * $fraction)
                [FluxWallpaper]::SetCursorPos($x, $heightDirectY) | Out-Null
                Start-Sleep -Milliseconds 45
            }
            [FluxWallpaper]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
            Start-Sleep -Milliseconds 400
            $heightDirectLog = Get-Content $settingsStderrPath -Raw
            $heightDirectGeometry = [regex]::Matches(
                $heightDirectLog,
                "VisualPreviewChild: GEOMETRY $visualPreviewProcessId (\d+) (\d+) (\d+) (\d+) (\d+)"
            )
            $heightDirectChanged = $false
            foreach ($match in $heightDirectGeometry) {
                if ([int]$match.Groups[2].Value -ne $initialLogicalHeight) {
                    $heightDirectChanged = $true
                    break
                }
            }
            $heightSliderY = if ($heightDirectChanged) { $heightDirectY } else { 0 }
            if (!$heightDirectChanged) {
                foreach ($candidateOffset in (300..560 | Where-Object { $_ % 8 -eq 4 })) {
                    $candidateY = $settingsRect.Top + [int][Math]::Round($candidateOffset * $settingsScale)
                    [FluxWallpaper]::SetCursorPos($sliderLeft, $candidateY) | Out-Null
                    [FluxWallpaper]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
                    for ($step = 0; $step -le 6; $step++) {
                        $fraction = 1.0 - ($step / 6.0)
                        $x = $sliderLeft + [int](($sliderRight - $sliderLeft) * $fraction)
                        [FluxWallpaper]::SetCursorPos($x, $candidateY) | Out-Null
                        Start-Sleep -Milliseconds 40
                    }
                    [FluxWallpaper]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
                    Start-Sleep -Milliseconds 250
                    $afterCandidateLog = Get-Content $settingsStderrPath -Raw
                    $candidateGeometry = [regex]::Matches(
                        $afterCandidateLog,
                        "VisualPreviewChild: GEOMETRY $visualPreviewProcessId (\d+) (\d+) (\d+) (\d+) (\d+)"
                    )
                    foreach ($match in $candidateGeometry) {
                        if ([int]$match.Groups[2].Value -ne $initialLogicalHeight) {
                            $heightSliderY = $candidateY
                            break
                        }
                    }
                    if ($heightSliderY -ne 0) { break }
                }
            }
            if ($heightSliderY -eq 0) {
                throw "Visual Settings smoke could not identify the results-height slider from native preview geometry telemetry."
            }

            # Exercise both discovered tracks in opposite directions so the test proves
            # realtime changes rather than merely proving that an initial child exists.
            foreach ($probe in @(
                @{ Y = $widthSliderY; Start = 0; End = 10 },
                @{ Y = $heightSliderY; Start = 10; End = 0 }
            )) {
                $startX = $sliderLeft + [int](($sliderRight - $sliderLeft) * $probe.Start / 10)
                [FluxWallpaper]::SetCursorPos($startX, $probe.Y) | Out-Null
                [FluxWallpaper]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
                for ($step = 0; $step -le 10; $step++) {
                    $fraction = ($probe.Start + ($probe.End - $probe.Start) * $step / 10.0) / 10.0
                    $x = $sliderLeft + [int](($sliderRight - $sliderLeft) * $fraction)
                    [FluxWallpaper]::SetCursorPos($x, $probe.Y) | Out-Null
                    Start-Sleep -Milliseconds 45
                }
                [FluxWallpaper]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
                Start-Sleep -Milliseconds 300
            }

            $afterDragRect = New-Object FluxWallpaper+RECT
            if (![FluxWallpaper]::GetWindowRect($settingsHwnd, [ref]$afterDragRect)) {
                throw "Visual Settings smoke could not read the Settings window after slider drags."
            }
            $settingsCenterAfterDragX = [int](($afterDragRect.Left + $afterDragRect.Right) / 2)
            $settingsCenterAfterDragY = [int](($afterDragRect.Top + $afterDragRect.Bottom) / 2)
            $settingsCenterDelta = [Math]::Max(
                [Math]::Abs($settingsCenterAfterDragX - $settingsCenterBeforeDragX),
                [Math]::Abs($settingsCenterAfterDragY - $settingsCenterBeforeDragY)
            )
            $afterDragWidth = [FluxWallpaper]::RectWidth($afterDragRect)
            $afterDragHeight = [FluxWallpaper]::RectHeight($afterDragRect)
            if ($settingsCenterDelta -gt 2 -or
                $afterDragWidth -ne $settingsWindowWidth -or
                $afterDragHeight -ne $settingsWindowHeight) {
                throw "Visual Settings slider smoke failed: center delta=${settingsCenterDelta}px, size=${afterDragWidth}x${afterDragHeight}, initial=${settingsWindowWidth}x${settingsWindowHeight}."
            }
            $visualSettingsSliderProbe = $true

            Start-Sleep -Milliseconds 300
            $previewLog = if (Test-Path $settingsStderrPath) { Get-Content $settingsStderrPath -Raw } else { "" }
            $visualPreviewUpdateCount = ([regex]::Matches($previewLog, "Visual preview IPC resize dispatched:")).Count
            $geometryMatches = [regex]::Matches(
                $previewLog,
                "VisualPreviewChild: GEOMETRY (\d+) (\d+) (\d+) (\d+) (\d+) (\d+)"
            )
            foreach ($match in $geometryMatches) {
                $logicalWidth = [int]$match.Groups[2].Value
                $logicalHeight = [int]$match.Groups[3].Value
                $clientWidth = [int]$match.Groups[4].Value
                $clientHeight = [int]$match.Groups[5].Value
                $dpi = [int]$match.Groups[6].Value
                $expectedWidth = [int][Math]::Floor(($logicalWidth * $dpi / 96.0) + 0.5)
                $expectedHeight = [int][Math]::Floor(($logicalHeight * $dpi / 96.0) + 0.5)
                if ($clientWidth -ne $expectedWidth -or $clientHeight -ne $expectedHeight) {
                    throw "Preview geometry mismatch: logical=${logicalWidth}x${logicalHeight}, client=${clientWidth}x${clientHeight}, dpi=${dpi}, expected=${expectedWidth}x${expectedHeight}."
                }
                $visualPreviewGeometrySamples += [ordered]@{
                    LogicalWidth = $logicalWidth
                    LogicalHeight = $logicalHeight
                    ClientWidth = $clientWidth
                    ClientHeight = $clientHeight
                    Dpi = $dpi
                }
            }
            $visualPreviewGeometrySampleCount = $visualPreviewGeometrySamples.Count
            if ($visualPreviewUpdateCount -lt 2 -or $visualPreviewGeometrySampleCount -lt 3) {
                throw "Visual Settings exact preview smoke failed: IPC updates=$visualPreviewUpdateCount, measured geometry samples=$visualPreviewGeometrySampleCount; expected realtime updates for both sliders."
            }
            $visualPreviewExactGeometryProbe = $true

            # Reset each control through the real button. The generation counter makes
            # a reset dispatch an IPC resize even if the user is already at that value.
            $resetX = $settingsRect.Left + [int][Math]::Round((18 + 24 + 110 + 12 + 200 + 8 + 76 + 8 + 25) * $settingsScale)
            $resetTargets = @(
                @{ Y = $widthSliderY; Marker = "Visual width reset clicked: 420x" },
                @{ Y = $heightSliderY; Marker = "Visual height reset clicked: " }
            )
            foreach ($resetTarget in $resetTargets) {
                [FluxWallpaper]::SetCursorPos($resetX, $resetTarget.Y) | Out-Null
                [FluxWallpaper]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
                [FluxWallpaper]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
                Start-Sleep -Milliseconds 600
                $resetLog = if (Test-Path $settingsStderrPath) { Get-Content $settingsStderrPath -Raw } else { "" }
                if ($resetLog -notmatch [regex]::Escape($resetTarget.Marker)) {
                    throw "Visual Settings reset smoke could not observe callback marker '$($resetTarget.Marker)' at screen y=$($resetTarget.Y)."
                }
            }
            $previewLog = if (Test-Path $settingsStderrPath) { Get-Content $settingsStderrPath -Raw } else { "" }
            $widthResetSeen = [regex]::IsMatch($previewLog, "VisualPreviewChild: GEOMETRY \d+ 420 \d+ \d+ \d+ \d+")
            $heightResetSeen = [regex]::IsMatch($previewLog, "VisualPreviewChild: GEOMETRY \d+ \d+ 382 \d+ \d+ \d+")
            $visualPreviewResetProbe = $widthResetSeen -and $heightResetSeen
            if (!$visualPreviewResetProbe) {
                throw "Visual Settings reset smoke failed: the native preview did not report logical 420 width and 382 height after Reset callbacks."
            }

            # The Visual page owns an explicit Apply dimensions action. Scroll the
            # Visual form to its lower action area, then locate the button by observing
            # the app's own callback marker instead of assuming a focus order that can
            # vary when the native scroll viewport changes.
            [FluxWallpaper]::SetForegroundWindow($settingsHwnd) | Out-Null
            $shell.AppActivate($settingsProcess.Id) | Out-Null
            $scrollX = $settingsRect.Left + [int]($settingsRect.Right - $settingsRect.Left) / 2
            $scrollY = $settingsRect.Bottom - 48
            [FluxWallpaper]::SetCursorPos($scrollX, $scrollY) | Out-Null
            [FluxWallpaper]::mouse_event(0x0800, 0, 0, [uint32]4294965376, [UIntPtr]::Zero)
            Start-Sleep -Milliseconds 450
            Save-Screenshot "settings-visual-apply.png"
            $applyX = $settingsRect.Left + [int][Math]::Round(118 * $settingsScale)
            $applyY = $settingsRect.Top + [int][Math]::Round(463 * $settingsScale)
            $applyPointClass = [FluxWallpaper]::WindowClassAtPoint($applyX, $applyY)
            Write-Host "Visual Apply direct probe: x=$applyX y=$applyY windowClass=$applyPointClass"
            [FluxWallpaper]::SetCursorPos($applyX, $applyY) | Out-Null
            [FluxWallpaper]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
            [FluxWallpaper]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
            Start-Sleep -Milliseconds 800
            $applyLog = Get-Content $settingsStderrPath -Raw
            if ($applyLog -notmatch "Visual Apply dimensions clicked: 420x382") {
                Save-Screenshot "settings-visual-apply-failed.png"
                throw "Visual Apply smoke could not activate the Apply dimensions button at x=$applyX y=$applyY (windowClass=$applyPointClass)."
            }
            Start-Sleep -Milliseconds 1200
            $settingsPath = Join-Path $env:APPDATA "FluxLauncher\settings.json"
            if (!(Test-Path $settingsPath)) {
                throw "Visual Apply smoke could not find the persisted settings file at $settingsPath."
            }
            $persistedSettings = Get-Content $settingsPath -Raw | ConvertFrom-Json
            if ($persistedSettings.launcher_width -ne 420 -or $persistedSettings.launcher_height -ne 382) {
                throw "Visual Apply smoke persisted unexpected dimensions: $($persistedSettings.launcher_width)x$($persistedSettings.launcher_height)."
            }
            $visualPreviewPersistenceProbe = $true
            $previewStillAlive = $false
            try {
                $previewStillAlive = -not (Get-Process -Id $visualPreviewProcessId -ErrorAction Stop).HasExited
            } catch { $previewStillAlive = $false }
            if ($previewStillAlive -or
                [FluxWallpaper]::FindVisibleWindowByProcessId([uint32]$visualPreviewProcessId) -ne [IntPtr]::Zero) {
                throw "Visual Apply smoke failed to close preview PID $visualPreviewProcessId."
            }

            # Reopen a fresh isolated Settings process. Its first preview geometry must load
            # the values just persisted by Apply, proving this is not only in-memory state.
            $reopenStdoutPath = Join-Path $OutputDirectory "settings-reopen.stdout.log"
            $reopenStderrPath = Join-Path $OutputDirectory "settings-reopen.stderr.log"
            $reopenedSettingsProcess = Start-Process -FilePath $Executable -PassThru -RedirectStandardOutput $reopenStdoutPath -RedirectStandardError $reopenStderrPath
            Start-Sleep -Seconds 2
            $reopenedSettingsHwnd = Get-LauncherWindowHandle $reopenedSettingsProcess
            $reopenedSettingsRect = New-Object FluxWallpaper+RECT
            if (![FluxWallpaper]::GetWindowRect($reopenedSettingsHwnd, [ref]$reopenedSettingsRect)) {
                throw "Visual persistence smoke could not find reopened Settings HWND."
            }
            $reopenLog = if (Test-Path $reopenStderrPath) { Get-Content $reopenStderrPath -Raw } else { "" }
            $reopenPidMatch = [regex]::Match($reopenLog, "Visual preview process started: pid=(\d+)")
            if (!$reopenPidMatch.Success) {
                throw "Visual persistence smoke did not observe a preview child after reopening Settings."
            }
            $reopenedPreviewProcessId = [int]$reopenPidMatch.Groups[1].Value
            $reopenGeometryMatch = $null
            for ($attempt = 0; $attempt -lt 30 -and $null -eq $reopenGeometryMatch; $attempt++) {
                $reopenLog = Get-Content $reopenStderrPath -Raw
                $matches = [regex]::Matches(
                    $reopenLog,
                    "VisualPreviewChild: GEOMETRY $reopenedPreviewProcessId (\d+) (\d+) (\d+) (\d+) (\d+)"
                )
                if ($matches.Count -gt 0) { $reopenGeometryMatch = $matches[$matches.Count - 1] }
                else { Start-Sleep -Milliseconds 100 }
            }
            if ($null -eq $reopenGeometryMatch -or
                [int]$reopenGeometryMatch.Groups[1].Value -ne 420 -or
                [int]$reopenGeometryMatch.Groups[2].Value -ne 382) {
                throw "Visual persistence smoke reopened dimensions do not equal 420x382 logical client units."
            }

            $reopenDpi = [FluxWallpaper]::GetDpiForWindow($reopenedSettingsHwnd)
            if ($reopenDpi -eq 0) { $reopenDpi = 96 }
            $reopenBackX = $reopenedSettingsRect.Right - [int][Math]::Round(42 * ($reopenDpi / 96.0)) - [int][Math]::Round(30 * ($reopenDpi / 96.0))
            $reopenBackY = $reopenedSettingsRect.Top + [int][Math]::Round(58 * ($reopenDpi / 96.0))
            [FluxWallpaper]::SetCursorPos($reopenBackX, $reopenBackY) | Out-Null
            [FluxWallpaper]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
            [FluxWallpaper]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
            Start-Sleep -Milliseconds 700
            $reopenedPreviewAlive = $false
            try {
                $reopenedPreviewAlive = -not (Get-Process -Id $reopenedPreviewProcessId -ErrorAction Stop).HasExited
            } catch { $reopenedPreviewAlive = $false }
            if ($reopenedPreviewAlive -or
                [FluxWallpaper]::FindVisibleWindowByProcessId([uint32]$reopenedPreviewProcessId) -ne [IntPtr]::Zero) {
                throw "Visual Back cleanup smoke failed after persistence reopen: preview PID $reopenedPreviewProcessId remained."
            }
            $visualPreviewCleanupProbe = $true

            Save-Screenshot "settings-visual-sliders.png"
            Write-Host "Visual Settings exact preview smoke passed: preview PID $visualPreviewProcessId reported $visualPreviewGeometrySampleCount measured GetClientRect samples and $visualPreviewUpdateCount IPC resizes; Settings remained ${afterDragWidth}x${afterDragHeight} with center delta=${settingsCenterDelta}px; Reset, Apply persistence, reopen, and Back cleanup passed."
        }
        Save-Screenshot "settings-panel.png"
        if ($EverythingMissingSmoke) {
            Save-Screenshot "everything-missing-settings.png"
        }
    }
    finally {
        if (!$settingsProcess.HasExited) {
            Stop-Process -Id $settingsProcess.Id -Force
        }
        if ($null -ne $reopenedSettingsProcess -and !$reopenedSettingsProcess.HasExited) {
            Stop-Process -Id $reopenedSettingsProcess.Id -Force -ErrorAction SilentlyContinue
        }
        foreach ($previewId in @($visualPreviewProcessId, $reopenedPreviewProcessId)) {
            if ($previewId -eq 0) { continue }
            $orphan = Get-Process -Id $previewId -ErrorAction SilentlyContinue
            if ($null -ne $orphan -and !$orphan.HasExited) {
                Stop-Process -Id $previewId -Force -ErrorAction SilentlyContinue
            }
        }
        Remove-Item Env:FLUX_OPEN_SETTINGS -ErrorAction SilentlyContinue
        Remove-Item Env:FLUX_SMOKE_TRAY_SETTINGS -ErrorAction SilentlyContinue
        Remove-Item Env:FLUX_SMOKE_SETTINGS_TAB -ErrorAction SilentlyContinue
        Remove-Item Env:FLUX_SMOKE_VISUAL_SETTINGS -ErrorAction SilentlyContinue
        Remove-Item Env:FLUX_DISABLE_SINGLE_INSTANCE -ErrorAction SilentlyContinue
    }

    $os = Get-CimInstance Win32_OperatingSystem
    [ordered]@{
        Caption = $os.Caption
        Version = $os.Version
        BuildNumber = $os.BuildNumber
        Architecture = $os.OSArchitecture
        ProcessId = $process.Id
        ForcedTranslucentFallback = [bool]$ForceTranslucentFallback
        CapturedAtUtc = (Get-Date).ToUniversalTime().ToString("O")
        WallpaperProbe = $true
        RepeatShowEmptyProbe = $true
        CompactLayoutProbe = $compactLayoutProbe
        FirstHotkeyHideProbe = $hiddenAfterFirstHotkey
        SecondHotkeyShowProbe = $visibleAfterSecondHotkey
        RepeatedHotkeyPositionProbe = $repeatedHotkeyPositionProbe
        RepeatedHotkeyYPositions = @($repeatedHotkeyYPositions)
        TrayOnlyWindowProbe = $trayOnlyWindow
        QueryExpandedProbe = $true
        PointerHoverProbe = [bool]$PointerInteractionSmoke
        PointerWheelProbe = [bool]$PointerInteractionSmoke
        PointerClickProbe = [bool]$PointerInteractionSmoke
        ScrollbarGapProbe = (!$ScrollbarGapSmoke) -or $scrollbarGapProbe
        ActionBarProbe = (!$ActionBarSmoke) -or $actionBarProbe
        ActionBarGeometry = $actionBarGeometry
        QueryClearOnReopenProbe = (!$QueryClearOnReopenSmoke) -or $queryClearOnReopenProbe
        CtrlRProbe = (!$CtrlRSmoke) -or $ctrlRProbe
        CtrlCProbe = (!$CtrlCSmoke) -or $ctrlCProbe
        TabNavigationProbe = $TabNavigationCycles -gt 0
        EverythingSyntaxProbe = $true
        QueryResponsivenessProbe = (!$QueryResponsivenessSmoke) -or $queryResponsivenessProbe
        CommandPriorityProbe = (!$CommandPrioritySmoke) -or $commandPriorityProbe
        QueryResponsivenessMaxMilliseconds = $queryResponsivenessMaxMilliseconds
        QueryResponsivenessSamples = @($queryResponsivenessSamples)
        FocusToggleProbe = (!$FocusToggleSmoke) -or $focusToggleProbe
        DeactivationClickProbe = (!$DeactivationClickSmoke) -or $deactivationClickProbe
        FolderLaunchProbe = (!$FolderLaunchSmoke) -or $folderLaunchProbe
        FocusToggleVisibleAfterReopen = $focusToggleVisibleAfterReopen
        FocusToggleForegroundAfterReopen = $focusToggleForegroundAfterReopen
        DeactivationHiddenAfterClick = $deactivationHiddenAfterClick
        DeactivationForegroundAfterClick = $deactivationForegroundAfterClick
        DeactivationCpuMilliseconds = $deactivationCpuDelta
        HistoryPanelProbe = $true
        HistoryUpProbe = $true
        HistoryAltUpProbe = $true
        HistoryAltDownProbe = $true
        SettingsOpenPath = if ($TraySettingsSmoke) { "tray-lifecycle" } else { "startup-env" }
        SettingsWindowFound = $settingsWindowFound
        SettingsWindowHeight = $settingsWindowHeight
        SettingsWindowWidth = $settingsWindowWidth
        SettingsPanelProbe = $settingsWindowFound -and ($settingsWindowHeight -ge 400) -and ($settingsWindowWidth -ge 680)
        VisualSettingsSliderProbe = (!$VisualSettingsSmoke) -or $visualSettingsSliderProbe
        VisualPreviewUpdateCount = $visualPreviewUpdateCount
        VisualPreviewProcessId = $visualPreviewProcessId
        VisualPreviewWindowFound = $visualPreviewWindowFound
        VisualPreviewExactGeometryProbe = (!$VisualSettingsSmoke) -or $visualPreviewExactGeometryProbe
        VisualPreviewGeometrySampleCount = $visualPreviewGeometrySampleCount
        VisualPreviewGeometrySamples = @($visualPreviewGeometrySamples)
        VisualPreviewResetProbe = (!$VisualSettingsSmoke) -or $visualPreviewResetProbe
        VisualPreviewPersistenceProbe = (!$VisualSettingsSmoke) -or $visualPreviewPersistenceProbe
        VisualPreviewCleanupProbe = (!$VisualSettingsSmoke) -or $visualPreviewCleanupProbe
        SettingsCenterBeforeDrag = [ordered]@{ X = $settingsCenterBeforeDragX; Y = $settingsCenterBeforeDragY }
        SettingsCenterAfterDrag = [ordered]@{ X = $settingsCenterAfterDragX; Y = $settingsCenterAfterDragY }
        SettingsCenterDeltaPixels = $settingsCenterDelta
        VisualDimensionContract = "logical client px (DIP); physical GetClientRect px = round(logical * GetDpiForWindow / 96)"
        EverythingAutoEnableProbe = $true
        EverythingStartupProbe = $everythingStartupProbe
        EverythingMissingStateProbe = [bool]$EverythingMissingSmoke
        RecycleBinProbe = [bool]$RecycleBinSmoke
        RecycleBinDirectResultsProbe = $recycleBinDirectResultsProbe
        RecycleBinDestructiveActionInvoked = $recycleBinDestructiveActionInvoked
        CursorVisibleOnActivation = $cursorVisibleOnActivation
        CursorHiddenAfterTyping = $cursorHiddenAfterTyping
        CursorVisibleAfterMove = $cursorVisibleAfterMove
        CursorVisibilityProbe = (!$CursorVisibilitySmoke) -or ($cursorVisibleOnActivation -and $cursorHiddenAfterTyping -and $cursorVisibleAfterMove)
        EverythingWingetInstallCommandProbe = "winget install -e --id voidtools.Everything"
        TraySettingsLifecycleProbe = (!$TraySettingsSmoke) -or ($settingsWindowFound -and ($settingsWindowHeight -ge 400) -and ($settingsWindowWidth -ge 680))
        KeyboardSelectionProbe = $true
        ActionModeProbe = $true
        EnterActionProbe = $true
        EnterLaunchHideProbe = $enterLaunchHidden
        EnterHideDispatchMilliseconds = $enterHideDispatchMilliseconds
        EnterHideLatencyProbe = $enterHideLatencyProbe
        EnterLaunchDispatchBeforeHideProbe = $enterLaunchDispatchBeforeHideProbe
        EnterProcessCreatedBeforeHideProbe = $enterProcessCreatedBeforeHideProbe
        EnterLaunchDispatchTimestamp = $launchDispatchTimestamp
        EnterProcessCreatedTimestamp = $processCreatedTimestamp
        EnterWindowHideTimestamp = $windowHideTimestamp
        EnterHideQuery = $enterHideQuery
        LaunchProcessCreationProbe = $launchProcessCreationProbe
        LaunchProbeQuery = $launchProbeQuery
        LaunchProbeDispatchBeforeHide = $launchProbeDispatchBeforeHide
        LaunchProbeProcessCreatedBeforeHide = $launchProbeProcessCreatedBeforeHide
        LaunchProbeProcessCreationMilliseconds = $launchProbeProcessCreationMilliseconds
        LaunchProbeDispatchToHideMilliseconds = $launchProbeDispatchToHideMilliseconds
        Memory = [ordered]@{
            Idle = $idleMemory
            Query = $queryMemory
            HistoryPanel = $historyMemory
        }
    } | ConvertTo-Json | Set-Content -Encoding utf8 (Join-Path $OutputDirectory "environment.json")
}
finally {
    if (!$process.HasExited) {
        Stop-Process -Id $process.Id -Force
    }
    if ($probeProcess -and !$probeProcess.HasExited) {
        Stop-Process -Id $probeProcess.Id -Force
    }
    if (Test-Path $wabFixtureRoot) {
        Remove-Item -Recurse -Force $wabFixtureRoot
    }
    if (Test-Path $folderFixtureRoot) {
        Remove-Item -Recurse -Force $folderFixtureRoot
    }
    Remove-Item Env:FLUX_LAUNCH_TRACE_FILE -ErrorAction SilentlyContinue
}

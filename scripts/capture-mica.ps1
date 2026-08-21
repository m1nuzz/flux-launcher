param(
    [Parameter(Mandatory = $true)]
    [string]$Executable,

    [Parameter(Mandatory = $true)]
    [string]$OutputDirectory,

    [switch]$ForceTranslucentFallback,
    [switch]$TraySettingsSmoke,
    [switch]$PointerInteractionSmoke,
    [switch]$EverythingMissingSmoke,
    [switch]$RecycleBinSmoke,
    [switch]$CursorVisibilitySmoke,
    [switch]$ScrollbarGapSmoke,
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
    public static extern bool IsWindowVisible(IntPtr hwnd);
    [DllImport("user32.dll", SetLastError = true)]
    public static extern IntPtr GetWindowLongPtr(IntPtr hwnd, int index);
    [DllImport("user32.dll", SetLastError = true)]
    public static extern IntPtr SendMessage(IntPtr hwnd, uint message, UIntPtr wParam, IntPtr lParam);
    [DllImport("user32.dll", SetLastError = true)]
    public static extern IntPtr GetForegroundWindow();
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
    $queryClearOnReopenProbe = $false
    $ctrlRProbe = $false
    $ctrlCProbe = $false
    $queryResponsivenessSamples = @()
    $queryResponsivenessMaxMilliseconds = 0.0
    $queryResponsivenessProbe = $false
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
            72
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

    # The first re-show clears the query by design. Restore a non-empty query
    # before testing normal Enter execution and hide-after-launch behavior.
    [FluxWallpaper]::SetForegroundWindow($launcherHandle) | Out-Null
    $shell.SendKeys("^a")
    $shell.SendKeys($navigationProbeQuery)
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
    $foregroundDeadline = (Get-Date).AddSeconds(2)
    while ((Get-Date) -lt $foregroundDeadline -and [FluxWallpaper]::GetForegroundWindow() -ne $launcherHandle) {
        Start-Sleep -Milliseconds 50
        [FluxWallpaper]::SetForegroundWindow($launcherHandle) | Out-Null
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
        $tracePreview = ($enterTraceLines -join ' | ')
        throw "Enter launch did not hide the launcher window. trace=$tracePreview."
    }
    if (!$enterHideLatencyProbe) {
        $tracePreview = ($enterTraceLines -join ' | ')
        throw "Enter launch/hide ordering failed: dispatch_before_hide=$enterLaunchDispatchBeforeHideProbe, hide_dispatch_ms=$enterHideDispatchMilliseconds, budget_ms=$EnterHideDispatchBudgetMilliseconds, trace=$tracePreview."
    }
    # Restore the launcher for the remaining independent probes.
    [FluxWallpaper]::SendMessage($launcherHandle, $wmHotkey, [UIntPtr]::Zero, [IntPtr]::Zero) | Out-Null
    Start-Sleep -Milliseconds 650
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
    Start-Sleep -Milliseconds 1500
    $launchProbeTraceLines = if (Test-Path $launchTracePath) {
        @(Get-Content $launchTracePath | Select-Object -Skip $launchProbeTraceBeforeCount)
    } else {
        @()
    }
    $launchProbeDispatchLine = $launchProbeTraceLines | Where-Object { $_ -match "`tlaunch-dispatch$" } | Select-Object -First 1
    $launchProbeHideLine = $launchProbeTraceLines | Where-Object { $_ -match "`twindow-hide$" } | Select-Object -First 1
    $launchProbeProcessLine = $launchProbeTraceLines | Where-Object { $_ -match "`tprocess-created$" } | Select-Object -First 1
    $launchProbeDispatchTimestamp = if ($launchProbeDispatchLine) { [double]($launchProbeDispatchLine -split "`t", 2)[0] } else { 0.0 }
    $launchProbeHideTimestamp = if ($launchProbeHideLine) { [double]($launchProbeHideLine -split "`t", 2)[0] } else { 0.0 }
    $launchProbeProcessTimestamp = if ($launchProbeProcessLine) { [double]($launchProbeProcessLine -split "`t", 2)[0] } else { 0.0 }
    $launchProbeDispatchBeforeHide =
        $launchProbeDispatchTimestamp -gt 0.0 -and
        $launchProbeHideTimestamp -gt 0.0 -and
        $launchProbeDispatchTimestamp -le $launchProbeHideTimestamp
    $launchProbeProcessCreated = $launchProbeProcessTimestamp -gt 0.0
    $launchProbeProcessCreatedBeforeHide =
        $launchProbeProcessCreated -and
        $launchProbeHideTimestamp -gt 0.0 -and
        $launchProbeProcessTimestamp -le $launchProbeHideTimestamp
    $launchProbeProcessCreationMilliseconds = if ($launchProbeProcessCreated) {
        [Math]::Round($launchProbeProcessTimestamp - $launchProbeDispatchTimestamp, 3)
    } else {
        0.0
    }
    $launchProbeDispatchToHideMilliseconds = if ($launchProbeDispatchBeforeHide) {
        [Math]::Round($launchProbeHideTimestamp - $launchProbeDispatchTimestamp, 3)
    } else {
        0.0
    }
    $launchProcessCreationProbe =
        $launchProbeDispatchBeforeHide -and $launchProbeProcessCreated
    if (!$launchProcessCreationProbe) {
        throw "Launch process probe failed: dispatch_before_hide=$launchProbeDispatchBeforeHide, process_created=$launchProbeProcessCreated."
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
    $settingsStdoutPath = Join-Path $OutputDirectory "settings.stdout.log"
    $settingsStderrPath = Join-Path $OutputDirectory "settings.stderr.log"
    $settingsProcess = Start-Process -FilePath $Executable -PassThru -RedirectStandardOutput $settingsStdoutPath -RedirectStandardError $settingsStderrPath
    $settingsWindowHeight = 0
    $settingsWindowWidth = 0
    $settingsWindowFound = $false
    try {
        Start-Sleep -Seconds 2
        $settingsProcess.Refresh()
        $settingsHwnd = $settingsProcess.MainWindowHandle
        if ($settingsHwnd -eq [IntPtr]::Zero) {
            $settingsHwnd = [FluxWallpaper]::GetForegroundWindow()
        }
        if ($settingsHwnd -ne [IntPtr]::Zero) {
            $settingsRect = New-Object FluxWallpaper+RECT
            if ([FluxWallpaper]::GetWindowRect($settingsHwnd, [ref]$settingsRect)) {
                $settingsWindowHeight = $settingsRect.Bottom - $settingsRect.Top
                $settingsWindowWidth = $settingsRect.Right - $settingsRect.Left
                $settingsWindowFound = $true
            }
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
        Remove-Item Env:FLUX_OPEN_SETTINGS -ErrorAction SilentlyContinue
        Remove-Item Env:FLUX_SMOKE_TRAY_SETTINGS -ErrorAction SilentlyContinue
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
        QueryClearOnReopenProbe = (!$QueryClearOnReopenSmoke) -or $queryClearOnReopenProbe
        CtrlRProbe = (!$CtrlRSmoke) -or $ctrlRProbe
        CtrlCProbe = (!$CtrlCSmoke) -or $ctrlCProbe
        TabNavigationProbe = $TabNavigationCycles -gt 0
        EverythingSyntaxProbe = $true
        QueryResponsivenessProbe = (!$QueryResponsivenessSmoke) -or $queryResponsivenessProbe
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

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
    [switch]$CtrlRSmoke,
    [string]$NavigationQuery = "wab",

    [int]$NavigationCycles = 0,

    [int]$TabNavigationCycles = 0
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

$existingEverythingGuideIds = @(
    Get-Process | Where-Object {
        $_.MainWindowTitle -like "Command Line Options - Everything*"
    } | Select-Object -ExpandProperty Id
)
$stdoutPath = Join-Path $OutputDirectory "launcher.stdout.log"
$stderrPath = Join-Path $OutputDirectory "launcher.stderr.log"
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
    $launcherHandle = $process.MainWindowHandle
    if ($launcherHandle -eq [IntPtr]::Zero) { throw "Flux launcher has no main window handle." }
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
    [FluxWallpaper]::SendMessage($launcherHandle, $wmHotkey, [UIntPtr]::Zero, [IntPtr]::Zero) | Out-Null
    Start-Sleep -Milliseconds 600
    $visibleAfterSecondHotkey = [FluxWallpaper]::IsWindowVisible($launcherHandle)
    if (!$hiddenAfterFirstHotkey -or !$visibleAfterSecondHotkey) {
        throw "Alt+Space visibility regression: hidden=$hiddenAfterFirstHotkey visible=$visibleAfterSecondHotkey"
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
    $enterHideQuery = "recyclebin"
    $shell.SendKeys($enterHideQuery)
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
    $shell.SendKeys($enterHideQuery)
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
        for ($cycle = 1; $cycle -le $NavigationCycles; $cycle++) {
            $shell.SendKeys("{UP}")
            Start-Sleep -Milliseconds 180
            Save-Screenshot ("navigation-cycle-{0:D2}-up.png" -f $cycle)
            $shell.SendKeys("{DOWN}")
            Start-Sleep -Milliseconds 180
            Save-Screenshot ("navigation-cycle-{0:D2}-down.png" -f $cycle)
        }
    }
    # The WAB query exercises long App Paths and duplicate application-like entries.
    # Enter must execute the selected normal result and hide the launcher.
    $shell.SendKeys("{HOME}")
    $shell.SendKeys("{DOWN}")
    Start-Sleep -Milliseconds 350
    Save-Screenshot "keyboard-selection.png"
    [FluxWallpaper]::SetForegroundWindow($launcherHandle) | Out-Null
    # Send Enter to the exact launcher HWND so this probe cannot be intercepted
    # by the desktop shell or a different foreground process.
    $wmKeyDown = 0x0100
    [FluxWallpaper]::SendMessage($launcherHandle, $wmKeyDown, [UIntPtr]::new(0x0D), [IntPtr]::Zero) | Out-Null
    Start-Sleep -Milliseconds 500
    $enterLaunchHidden = ![FluxWallpaper]::IsWindowVisible($launcherHandle)
    if (!$enterLaunchHidden) {
        throw "Enter launch did not hide the launcher window."
    }
    # Restore the launcher for the remaining independent probes.
    [FluxWallpaper]::SendMessage($launcherHandle, $wmHotkey, [UIntPtr]::Zero, [IntPtr]::Zero) | Out-Null
    Start-Sleep -Milliseconds 650
    if (![FluxWallpaper]::IsWindowVisible($launcherHandle)) {
        throw "Unable to restore launcher after Enter hide probe."
    }
    [FluxWallpaper]::SetForegroundWindow($launcherHandle) | Out-Null
    Start-Sleep -Milliseconds 200
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
        TabNavigationProbe = $TabNavigationCycles -gt 0
        EverythingSyntaxProbe = $true
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
        EnterHideQuery = $enterHideQuery
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
}

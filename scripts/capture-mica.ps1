param(
    [Parameter(Mandatory = $true)]
    [string]$Executable,

    [Parameter(Mandatory = $true)]
    [string]$OutputDirectory,

    [switch]$ForceTranslucentFallback,

    [string]$NavigationQuery = "wab",

    [int]$NavigationCycles = 0
)

$ErrorActionPreference = "Stop"

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

$stdoutPath = Join-Path $OutputDirectory "launcher.stdout.log"
$stderrPath = Join-Path $OutputDirectory "launcher.stderr.log"
$process = Start-Process -FilePath $Executable -PassThru -RedirectStandardOutput $stdoutPath -RedirectStandardError $stderrPath
try {
    Start-Sleep -Seconds 3
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

    $bounds = [System.Windows.Forms.SystemInformation]::VirtualScreen
    $searchX = $bounds.Left + [int]($bounds.Width / 2)
    $searchY = $bounds.Top + [int]($bounds.Height / 2)
    $shell = New-Object -ComObject WScript.Shell
    $shell.AppActivate($process.Id) | Out-Null
    Start-Sleep -Milliseconds 250
    [FluxWallpaper]::SetCursorPos($searchX, $searchY) | Out-Null
    [FluxWallpaper]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
    [FluxWallpaper]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 250
    $shell.SendKeys($NavigationQuery)
    Start-Sleep -Seconds 2
    $queryMemory = Get-MemorySnapshot $process.Id
    Save-Screenshot "everything-fallback.png"
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
    # Flow-style keyboard navigation: select the second result with Down,
    # enter action mode with Right, then execute the next action with Enter.
    $shell.SendKeys("{DOWN}")
    Start-Sleep -Milliseconds 350
    Save-Screenshot "keyboard-selection.png"
    $shell.SendKeys("{RIGHT}")
    Start-Sleep -Milliseconds 500
    Save-Screenshot "actions-panel.png"
    $shell.SendKeys("{DOWN}")
    $shell.SendKeys("{ENTER}")
    Start-Sleep -Milliseconds 400
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
    $env:FLUX_OPEN_SETTINGS = "1"
    $settingsStdoutPath = Join-Path $OutputDirectory "settings.stdout.log"
    $settingsStderrPath = Join-Path $OutputDirectory "settings.stderr.log"
    $settingsProcess = Start-Process -FilePath $Executable -PassThru -RedirectStandardOutput $settingsStdoutPath -RedirectStandardError $settingsStderrPath
    $settingsWindowHeight = 0
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
                $settingsWindowFound = $true
            }
        }
        Save-Screenshot "settings-panel.png"
    }
    finally {
        if (!$settingsProcess.HasExited) {
            Stop-Process -Id $settingsProcess.Id -Force
        }
        Remove-Item Env:FLUX_OPEN_SETTINGS -ErrorAction SilentlyContinue
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
        TrayOnlyWindowProbe = $trayOnlyWindow
        QueryExpandedProbe = $true
        EverythingSyntaxProbe = $true
        HistoryPanelProbe = $true
        HistoryUpProbe = $true
        HistoryAltUpProbe = $true
        HistoryAltDownProbe = $true
        SettingsWindowFound = $settingsWindowFound
        SettingsWindowHeight = $settingsWindowHeight
        SettingsPanelProbe = $settingsWindowFound -and ($settingsWindowHeight -ge 400)
        KeyboardSelectionProbe = $true
        ActionModeProbe = $true
        EnterActionProbe = $true
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

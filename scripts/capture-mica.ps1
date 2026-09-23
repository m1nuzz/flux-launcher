param(
    [Parameter(Mandatory = $true)]
    [string]$Executable,

    [Parameter(Mandatory = $true)]
    [string]$OutputDirectory,

    [switch]$ForceTranslucentFallback,
    [switch]$TraySettingsSmoke,
    [switch]$TraySettingsAfterDeactivationSmoke,
    [switch]$VisualSettingsSmoke,
    [switch]$PointerInteractionSmoke,
    [switch]$ResultMouseInteractionSmoke,
    [switch]$EverythingMissingSmoke,
    [switch]$RecycleBinSmoke,
    [switch]$CursorVisibilitySmoke,
    [switch]$ScrollbarGapSmoke,
    [switch]$ActionBarSmoke,
    [switch]$CommandPrioritySmoke,
    [switch]$PowerShellSmoke,
    [switch]$CalculatorSmoke,
    [switch]$CalculatorPolicySmoke,
    [switch]$ObsidianIconSmoke,
    [switch]$QueryClearOnReopenSmoke,
    [switch]$QueryResponsivenessSmoke,
    [switch]$FocusToggleSmoke,
    [switch]$ImeMessageSmoke,
    [switch]$DeactivationClickSmoke,
    [switch]$FolderLaunchSmoke,
    [switch]$CtrlRSmoke,
    [switch]$CtrlCSmoke,
    [switch]$IdlePerformanceSmoke,
    [switch]$ResourceProfileSmoke,
    [int]$ResourceProfileCycles = 32,
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

# 烟雾测试必须与产品处于同一坐标空间。pwsh.exe 默认 DPI 不感知：此时
# GetClientRect/GetWindowRect 返回被显示缩放折算过的虚拟坐标，而 GetDpiForWindow
# 仍返回真实 DPI。脚本里的 "client / scale"（逻辑换算）与 "* scale"（物理偏移）
# 因此会在非 100% 缩放下被重复折算，例如 150% 下紧凑高度被算成 37 而不是 56。
# 把当前线程切到 Per-Monitor v2，使所有 Win32 查询与鼠标坐标统一为物理像素。
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class FluxSmokeDpi {
    [DllImport("user32.dll", SetLastError = true)]
    public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
}
'@
# DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2 = -4
$previousDpiContext = [FluxSmokeDpi]::SetThreadDpiAwarenessContext([IntPtr](-4))
Write-Host "Smoke DPI awareness: switched to per-monitor-v2 (previous_handle=$($previousDpiContext.ToInt64()))"

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
    public static extern bool PostMessage(IntPtr hwnd, uint message, UIntPtr wParam, IntPtr lParam);
    [DllImport("user32.dll", SetLastError = true)]
    public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll", SetLastError = true)]
    public static extern IntPtr WindowFromPoint(POINT point);
    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    public static extern int GetClassName(IntPtr hWnd, char[] className, int maxCount);
    public static IntPtr WindowHandleAtPoint(int x, int y) {
        return WindowFromPoint(new POINT { X = x, Y = y });
    }
    public static string WindowClassAtPoint(int x, int y) {
        IntPtr hwnd = WindowFromPoint(new POINT { X = x, Y = y });
        if (hwnd == IntPtr.Zero) return "<none>";
        char[] buffer = new char[256];
        int length = GetClassName(hwnd, buffer, buffer.Length);
        return length > 0 ? new string(buffer, 0, length) : "<unknown>";
    }
    public static uint WindowProcessIdAtPoint(int x, int y) {
        IntPtr hwnd = WindowFromPoint(new POINT { X = x, Y = y });
        if (hwnd == IntPtr.Zero) return 0;
        uint processId;
        return GetWindowThreadProcessId(hwnd, out processId) == 0 ? 0 : processId;
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
    [StructLayout(LayoutKind.Sequential)]
    public struct GUITHREADINFO {
        public uint cbSize;
        public uint flags;
        public IntPtr hwndActive;
        public IntPtr hwndFocus;
        public IntPtr hwndCapture;
        public IntPtr hwndMenuOwner;
        public IntPtr hwndMoveSize;
        public IntPtr hwndCaret;
        public RECT rcCaret;
    }
    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool GetGUIThreadInfo(uint threadId, out GUITHREADINFO info);
    [DllImport("user32.dll", SetLastError = true)]
    public static extern IntPtr GetCursor();
    [DllImport("user32.dll", SetLastError = true)]
    public static extern IntPtr LoadCursor(IntPtr hInstance, IntPtr lpCursorName);
    public static bool IsTextCursor() {
        return GetCursor() == LoadCursor(IntPtr.Zero, new IntPtr(32513)); // IDC_IBEAM
    }
    public static bool IsArrowCursor() {
        return GetCursor() == LoadCursor(IntPtr.Zero, new IntPtr(32512)); // IDC_ARROW
    }
    [DllImport("user32.dll", SetLastError = true)]
    public static extern IntPtr GetKeyboardLayout(uint threadId);
    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    public static extern IntPtr LoadKeyboardLayout(string layoutId, uint flags);
    [DllImport("user32.dll", SetLastError = true)]
    public static extern int GetKeyboardLayoutList(int count, IntPtr[] layouts);
    public static bool IsEnglishKeyboardLayout(IntPtr layout) {
        // 主语言 ID 位于 HKL 低 10 位：0x09 为 LANG_ENGLISH。
        return (layout.ToInt64() & 0x03ff) == 0x09;
    }
    public static string DescribeKeyboardLayout(IntPtr layout) {
        return layout == IntPtr.Zero ? "0x0" : string.Format("0x{0:x}", layout.ToInt64());
    }
    public static IntPtr[] InstalledKeyboardLayouts() {
        int count = GetKeyboardLayoutList(0, null);
        if (count <= 0) {
            return new IntPtr[0];
        }
        IntPtr[] layouts = new IntPtr[count];
        int written = GetKeyboardLayoutList(count, layouts);
        if (written <= 0) {
            return new IntPtr[0];
        }
        if (written == count) {
            return layouts;
        }
        IntPtr[] trimmed = new IntPtr[written];
        Array.Copy(layouts, trimmed, written);
        return trimmed;
    }
}
'@

function Get-ProcessExitCodeText([System.Diagnostics.Process]$Process) {
    try {
        $Process.Refresh()
        if ($Process.HasExited) {
            return "exit_code=$($Process.ExitCode)"
        }
    }
    catch {
        return "exit_code=unavailable"
    }
    return "running"
}

function Assert-ProcessAlive([System.Diagnostics.Process]$Process, [string]$Context) {
    try {
        $Process.Refresh()
        if ($Process.HasExited) {
            throw "Flux process exited unexpectedly during ${Context}: pid=$($Process.Id) $(Get-ProcessExitCodeText $Process)"
        }
    }
    catch {
        if ($_.Exception.Message -like "Flux process exited unexpectedly*") { throw }
        throw "Flux process could not be inspected during ${Context}: pid=$($Process.Id) ($($_.Exception.Message))"
    }
}

function Get-MemorySnapshot([System.Diagnostics.Process]$Process, [string]$Context = "memory snapshot") {
    Assert-ProcessAlive $Process $Context
    try {
        [ordered]@{
            WorkingSetBytes = [int64]$Process.WorkingSet64
            PrivateBytes = [int64]$Process.PrivateMemorySize64
            VirtualBytes = [int64]$Process.VirtualMemorySize64
            HandleCount = [int64]$Process.HandleCount
            ThreadCount = [int64]$Process.Threads.Count
        }
    }
    catch {
        throw "Flux process snapshot failed during ${Context}: pid=$($Process.Id) ($($_.Exception.Message))"
    }
}

function Get-CpuTimeMillisecond([System.Diagnostics.Process]$Process, [string]$Context = "CPU snapshot") {
    Assert-ProcessAlive $Process $Context
    try {
        return $Process.TotalProcessorTime.TotalMilliseconds
    }
    catch {
        throw "Flux CPU snapshot failed during ${Context}: pid=$($Process.Id) ($($_.Exception.Message))"
    }
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

function Initialize-FluxKeyboardLayout([IntPtr]$Handle) {
    # 启动器会在显示时把自身 UI 线程切到英文布局（设置 > 常规 > 键盘布局，默认启用），
    # 但该切换依赖本机已安装英文布局。中文 Windows 常常只装了中文输入法布局，此时
    # 下面的物理按键验证会被输入法拦截（先进入组合态，永远产不出 WM_CHAR）。这里在
    # 缺少英文布局时为本窗口加载系统自带的 US 布局，使烟雾测试保持确定性；已装英文
    # 布局的主机走空操作分支，保留启动器自身的真实行为。
    $processId = [uint32]0
    $threadId = [FluxWallpaper]::GetWindowThreadProcessId($Handle, [ref]$processId)
    $installed = [FluxWallpaper]::InstalledKeyboardLayouts()
    $before = [FluxWallpaper]::GetKeyboardLayout($threadId)
    $loaded = [IntPtr]::Zero
    if (![FluxWallpaper]::IsEnglishKeyboardLayout($before)) {
        $target = [IntPtr]::Zero
        foreach ($layout in $installed) {
            if ([FluxWallpaper]::IsEnglishKeyboardLayout($layout)) {
                $target = $layout
                break
            }
        }
        if ($target -eq [IntPtr]::Zero) {
            # 不使用 KLF_ACTIVATE：只让启动器窗口所在线程切换布局，避免改动本进程。
            $target = [FluxWallpaper]::LoadKeyboardLayout("00000409", 0)
            $loaded = $target
        }
        if ($target -ne [IntPtr]::Zero) {
            [FluxWallpaper]::PostMessage($Handle, 0x0050, [UIntPtr]::Zero, $target) | Out-Null
        }
        $deadline = (Get-Date).AddSeconds(3)
        while ((Get-Date) -lt $deadline) {
            if ([FluxWallpaper]::IsEnglishKeyboardLayout([FluxWallpaper]::GetKeyboardLayout($threadId))) {
                break
            }
            Start-Sleep -Milliseconds 100
        }
    }
    $after = [FluxWallpaper]::GetKeyboardLayout($threadId)
    $probe = [ordered]@{
        ThreadId = $threadId
        InstalledLayouts = @($installed | ForEach-Object { [FluxWallpaper]::DescribeKeyboardLayout($_) })
        LayoutBefore = [FluxWallpaper]::DescribeKeyboardLayout($before)
        LayoutAfter = [FluxWallpaper]::DescribeKeyboardLayout($after)
        LoadedUsLayout = [FluxWallpaper]::DescribeKeyboardLayout($loaded)
        EnglishLayoutActive = [FluxWallpaper]::IsEnglishKeyboardLayout($after)
    }
    $probe | ConvertTo-Json -Depth 4 | Set-Content -Encoding utf8 (Join-Path $OutputDirectory "keyboard-layout-probe.json")
    Write-Host "Keyboard layout probe: installed=[$($probe.InstalledLayouts -join ',')] before=$($probe.LayoutBefore) after=$($probe.LayoutAfter) loaded=$($probe.LoadedUsLayout) english_active=$($probe.EnglishLayoutActive)"
    if (!$probe.EnglishLayoutActive) {
        throw "Unable to establish an English keyboard layout for the launcher window (installed: $($probe.InstalledLayouts -join ', ')). Physical keystroke probes would be consumed by the host IME; install the English (United States) keyboard layout or run this smoke on the Windows runner."
    }
    return $probe
}
function Open-FluxTrayMenu {
    # 走真实 Win32 托盘路径弹出右键菜单：先把光标移到目标点（windi 的 track_menu
    # 用 GetCursorPos 决定菜单弹出位置），再向启动器窗口投递托盘回调消息。
    # 返回实测的原生菜单矩形（窗口类 #32768），供调用方按行定位菜单项。
    param(
        [Parameter(Mandatory = $true)][IntPtr]$LauncherHandle,
        [Parameter(Mandatory = $true)][int]$MenuX,
        [Parameter(Mandatory = $true)][int]$MenuY,
        [Parameter(Mandatory = $true)][string]$Context
    )
    $wmTrayIcon = 0x8001
    $wmRButtonUp = 0x0205
    [FluxWallpaper]::SetCursorPos($MenuX, $MenuY) | Out-Null
    if (![FluxWallpaper]::PostMessage($LauncherHandle, $wmTrayIcon, [UIntPtr]::Zero, [IntPtr]$wmRButtonUp)) {
        throw "$Context could not post the real tray-menu callback."
    }
    $menuDeadline = (Get-Date).AddSeconds(2)
    $menuReady = $false
    while ((Get-Date) -lt $menuDeadline) {
        if ([FluxWallpaper]::WindowClassAtPoint($MenuX, $MenuY) -eq "#32768") {
            $menuReady = $true
            break
        }
        Start-Sleep -Milliseconds 50
    }
    if (!$menuReady) {
        throw "$Context could not observe the native popup menu."
    }
    # 用真实弹窗 HWND 与实测矩形定位，而不是假设键盘焦点一定落在菜单上。
    $menuHandle = [FluxWallpaper]::WindowHandleAtPoint($MenuX, $MenuY)
    if ($menuHandle -eq [IntPtr]::Zero) {
        throw "$Context could not resolve the popup menu HWND."
    }
    $menuRect = New-Object FluxWallpaper+RECT
    if (![FluxWallpaper]::GetWindowRect($menuHandle, [ref]$menuRect)) {
        throw "$Context could not read the popup menu rectangle."
    }
    $menuWidth = [FluxWallpaper]::RectWidth($menuRect)
    $menuHeight = [FluxWallpaper]::RectHeight($menuRect)
    if ($menuWidth -lt 80 -or $menuHeight -lt 60) {
        throw "$Context observed an invalid popup menu rectangle: ${menuWidth}x${menuHeight}."
    }
    Write-Host "$Context tray popup rectangle: ${menuWidth}x${menuHeight}"
    return [pscustomobject]@{
        Left = $menuRect.Left
        Top = $menuRect.Top
        Width = $menuWidth
        Height = $menuHeight
    }
}

function Invoke-FluxTrayMenuItemClick([int]$X, [int]$Y) {
    [FluxWallpaper]::SetCursorPos($X, $Y) | Out-Null
    [FluxWallpaper]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
    [FluxWallpaper]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
}

function Invoke-FluxTraySettingsClick {
    # 真实托盘菜单选择 Settings（菜单顺序：Show launcher / Settings / 分隔 /
    # Game Mode / 分隔 / Exit，第二行即 Settings），随后测量设置面板几何。
    # 返回对象同时给出「首个可见帧」与「最终帧」尺寸：首个可见帧用于断言面板
    # 不得先以紧凑搜索条闪现，最终帧用于既有的「打开后达到完整尺寸」断言。
    param(
        [Parameter(Mandatory = $true)][IntPtr]$LauncherHandle,
        [Parameter(Mandatory = $true)][int]$MenuX,
        [Parameter(Mandatory = $true)][int]$MenuY,
        [Parameter(Mandatory = $true)][string]$Context,
        [int]$MinimumWidth = 680,
        [int]$MinimumHeight = 400
    )
    $menu = Open-FluxTrayMenu -LauncherHandle $LauncherHandle -MenuX $MenuX -MenuY $MenuY -Context $Context
    $settingsX = $menu.Left + [int]($menu.Width / 2)
    $settingsY = $menu.Top + [int]($menu.Height * 0.38)
    Write-Host "$Context Settings click=($settingsX,$settingsY)"
    Invoke-FluxTrayMenuItemClick -X $settingsX -Y $settingsY
    $geometryDeadline = (Get-Date).AddSeconds(3)
    $firstVisible = $false
    $firstWidth = 0
    $firstHeight = 0
    $finalWidth = 0
    $finalHeight = 0
    $full = $false
    while ((Get-Date) -lt $geometryDeadline) {
        if ([FluxWallpaper]::IsWindowVisible($LauncherHandle)) {
            $panelRect = New-Object FluxWallpaper+RECT
            if ([FluxWallpaper]::GetWindowRect($LauncherHandle, [ref]$panelRect)) {
                $finalWidth = [FluxWallpaper]::RectWidth($panelRect)
                $finalHeight = [FluxWallpaper]::RectHeight($panelRect)
                if (!$firstVisible) {
                    $firstWidth = $finalWidth
                    $firstHeight = $finalHeight
                    $firstVisible = $true
                    Write-Host "$Context first visible frame: ${firstWidth}x${firstHeight}"
                }
                if ($finalWidth -ge $MinimumWidth -and $finalHeight -ge $MinimumHeight) {
                    $full = $true
                    break
                }
            }
        }
        Start-Sleep -Milliseconds 100
    }
    return [pscustomobject]@{
        FirstVisible = $firstVisible
        FirstWidth = $firstWidth
        FirstHeight = $firstHeight
        Width = $finalWidth
        Height = $finalHeight
        Full = $full
        MenuWidth = $menu.Width
        MenuHeight = $menu.Height
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
$screenLeft = [int]$screen.Left
$screenTop = [int]$screen.Top
$screenWidth = [int]$screen.Width
$screenHeight = [int]$screen.Height
$form.Location = [System.Drawing.Point]::new(($screenLeft + 1), ($screenTop + 1))
$form.Size = [System.Drawing.Size]::new(($screenWidth - 2), ($screenHeight - 2))
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
$compactFixtureTarget = Join-Path $env:TEMP ("FluxLauncherCompactAppFixture_{0}.cmd" -f $PID)
Set-Content -Encoding ascii -Path $compactFixtureTarget -Value "@echo off`r`nexit /b 0"
$compactAppProbePath = Join-Path $OutputDirectory "compact-application-probe.log"
Remove-Item $compactAppProbePath -Force -ErrorAction SilentlyContinue
$iconProbePath = Join-Path $OutputDirectory "icon-probe.log"
$queryProbePath = Join-Path $OutputDirectory "query-probe.log"
Remove-Item $iconProbePath, $queryProbePath -Force -ErrorAction SilentlyContinue
$env:FLUX_ICON_PROBE_FILE = $iconProbePath
$env:FLUX_QUERY_PROBE_FILE = $queryProbePath
$obsidianConfigRoot = Join-Path $env:APPDATA "obsidian"
$obsidianConfigBackupRoot = Join-Path $env:TEMP ("FluxLauncher-ObsidianConfig-backup-{0}" -f $PID)
$obsidianFixtureVaultRoot = Join-Path $env:TEMP ("FluxLauncher-ObsidianVault-{0}" -f $PID)
$obsidianConfigWasBackedUp = $false
$obsidianIconProbe = $false
if ($ObsidianIconSmoke) {
    # Use an isolated vault so the screenshot exercises the built-in Obsidian
    # provider without requiring Obsidian to be installed on the runner.
    if (Test-Path $obsidianConfigRoot) {
        Remove-Item $obsidianConfigBackupRoot -Recurse -Force -ErrorAction SilentlyContinue
        Move-Item -LiteralPath $obsidianConfigRoot -Destination $obsidianConfigBackupRoot -Force -ErrorAction Stop
        $obsidianConfigWasBackedUp = $true
    }
    New-Item -ItemType Directory -Force -Path $obsidianConfigRoot, $obsidianFixtureVaultRoot | Out-Null
    Set-Content -Encoding utf8 -Path (Join-Path $obsidianFixtureVaultRoot "Ornith-1.0-9B-MTP-NVFP4.md") -Value "# Flux Obsidian icon smoke"
    [ordered]@{
        vaults = [ordered]@{
            "flux-smoke" = [ordered]@{
                path = $obsidianFixtureVaultRoot
                name = "Notes"
            }
        }
    } | ConvertTo-Json -Depth 6 | Set-Content -Encoding utf8 (Join-Path $obsidianConfigRoot "obsidian.json")
}
$wabFixtureNames = @(
    # A real-looking spaced app name makes the compact-query screenshot
    # deterministic even when the hosted runner has no LM Studio install.
    "LM Studio.lnk",
    "WAB Primary Application.lnk",
    "WAB Secondary Application.lnk",
    "WAB Microsoft Windows Web Account Manager Diagnostic Resource Long Name.lnk",
    "WAB Microsoft Windows Web Account Manager Support Center Long Name.lnk"
)
$shortcutShell = New-Object -ComObject WScript.Shell
$absoluteExecutable = [System.IO.Path]::GetFullPath($Executable)
foreach ($fixtureName in $wabFixtureNames) {
    $shortcut = $shortcutShell.CreateShortcut((Join-Path $wabFixtureRoot $fixtureName))
    if ($fixtureName -eq "LM Studio.lnk") {
        # Use a neutral unique command fixture so the smoke cannot pass by
        # matching `lmstudio` in the executable name; only the spaced shortcut
        # title must satisfy the compact application matcher.
        $shortcut.TargetPath = $compactFixtureTarget
        $shortcut.WorkingDirectory = Split-Path -Parent $compactFixtureTarget
    } else {
        $shortcut.TargetPath = $absoluteExecutable
        $shortcut.WorkingDirectory = Split-Path -Parent $absoluteExecutable
    }
    $shortcut.Description = if ($fixtureName -eq "LM Studio.lnk") {
        "LM Studio compact query smoke application fixture"
    } else {
        "Flux WAB smoke application fixture"
    }
    $shortcut.Save()
}
$launchProbeShortcut = $shortcutShell.CreateShortcut((Join-Path $wabFixtureRoot "Zq7LaunchProbe.lnk"))
$launchProbeShortcut.TargetPath = Join-Path $env:WINDIR "System32\cmd.exe"
$launchProbeShortcut.Arguments = "/c exit"
$launchProbeShortcut.WorkingDirectory = $env:WINDIR
    $launchProbeShortcut.Description = "Zq7LaunchProbe deterministic process creation smoke fixture"
    $launchProbeShortcut.Save()
    $resultPointerShortcut = $shortcutShell.CreateShortcut((Join-Path $wabFixtureRoot "Result Mouse Probe.lnk"))
    $resultPointerShortcut.TargetPath = $absoluteExecutable
    $resultPointerShortcut.Arguments = '--folder-launch-smoke "{0}"' -f $folderFixtureRoot
    $resultPointerShortcut.WorkingDirectory = Split-Path -Parent $absoluteExecutable
    $resultPointerShortcut.Description = "Result mouse interaction windowless launch smoke fixture"
    $resultPointerShortcut.Save()

    $existingEverythingGuideIds = @(

    Get-Process | Where-Object {
        $_.MainWindowTitle -like "Command Line Options - Everything*"
    } | Select-Object -ExpandProperty Id
)
$stdoutPath = Join-Path $OutputDirectory "launcher.stdout.log"
$stderrPath = Join-Path $OutputDirectory "launcher.stderr.log"
$launchTracePath = Join-Path $OutputDirectory "launch-trace.log"
$inputTracePath = Join-Path $OutputDirectory "input-trace.log"
Remove-Item $launchTracePath, $inputTracePath -Force -ErrorAction SilentlyContinue
$env:FLUX_LAUNCH_TRACE_FILE = $launchTracePath
$env:FLUX_COMPACT_APP_PROBE_FILE = $compactAppProbePath
if ($ImeMessageSmoke) {
    $env:FLUX_INPUT_TRACE_FILE = $inputTracePath
} else {
    Remove-Item Env:FLUX_INPUT_TRACE_FILE -ErrorAction SilentlyContinue
}
$imeMessageProbe = !$ImeMessageSmoke
$imeMessageDetails = $null
$legacyFlowPluginRoot = Join-Path $env:APPDATA "FluxLauncher\Plugins\NativeFlowFixture"
$legacyFlowPluginBackupRoot = Join-Path $env:TEMP ("FluxLauncher-NativeFlowFixture.compact-smoke-disabled-{0}" -f $PID)
$legacyFlowPluginWasDisabled = $false
if ($CommandPrioritySmoke -and (Test-Path $legacyFlowPluginRoot)) {
    # The workflow's legacy fixture answers every query. Keep it out of the
    # compact-name frame so the screenshot proves ApplicationCatalog matching.
    Remove-Item $legacyFlowPluginBackupRoot -Recurse -Force -ErrorAction SilentlyContinue
    Move-Item -LiteralPath $legacyFlowPluginRoot -Destination $legacyFlowPluginBackupRoot -Force -ErrorAction SilentlyContinue
    $legacyFlowPluginWasDisabled = !(Test-Path $legacyFlowPluginRoot) -and (Test-Path $legacyFlowPluginBackupRoot)
}
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
    $idleMemory = Get-MemorySnapshot $process
    Save-Screenshot "mica-desktop.png"

    # Regression probe: exercise the real global Alt+Space hide/show path twice
    # while the query is empty. Capture this before any query edit, so a later
    # repaint cannot hide a failure where the DirectComposition surface attaches
    # only after typing.
    $launcherHandle = Get-LauncherWindowHandle $process
    if ($launcherHandle -eq [IntPtr]::Zero) { throw "Flux launcher has no main window handle after waiting for startup." }
    Initialize-FluxKeyboardLayout $launcherHandle | Out-Null
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
    # Toggling follows the window's real visibility, so if hide-on-deactivate fired
    # between the setup above and this dispatch (the hosted runner can steal focus at
    # any moment) the window is already hidden and the hotkey legitimately *shows* it
    # again. Retry that case instead of reporting it as a failed hide; a hotkey that
    # truly does nothing while the window stays visible still fails on every attempt.
    $hiddenAfterFirstHotkey = $false
    for ($hotkeyToggleAttempt = 0; $hotkeyToggleAttempt -lt 3 -and !$hiddenAfterFirstHotkey; $hotkeyToggleAttempt++) {
        if (![FluxWallpaper]::IsWindowVisible($launcherHandle)) {
            # Restore through the real global activation bind, then re-check.
            [FluxWallpaper]::keybd_event(0x12, 0, 0, [UIntPtr]::Zero)
            [FluxWallpaper]::keybd_event(0x20, 0, 0, [UIntPtr]::Zero)
            [FluxWallpaper]::keybd_event(0x20, 0, 2, [UIntPtr]::Zero)
            [FluxWallpaper]::keybd_event(0x12, 0, 2, [UIntPtr]::Zero)
            Start-Sleep -Milliseconds 800
            if (![FluxWallpaper]::IsWindowVisible($launcherHandle)) {
                continue
            }
        }
        [FluxWallpaper]::SetForegroundWindow($launcherHandle) | Out-Null
        Start-Sleep -Milliseconds 250
        [FluxWallpaper]::SendMessage($launcherHandle, $wmHotkey, [UIntPtr]::Zero, [IntPtr]::Zero) | Out-Null
        Start-Sleep -Milliseconds 450
        $hiddenAfterFirstHotkey = ![FluxWallpaper]::IsWindowVisible($launcherHandle)
    }
    if ($IdlePerformanceSmoke) {
        if (!$hiddenAfterFirstHotkey) {
            throw "Idle performance probe could not hide the launcher."
        }
        # Let queued hide work and startup activity settle before sampling.
        Start-Sleep -Milliseconds 1500
        $idleCpuBefore = Get-CpuTimeMillisecond $process
        Start-Sleep -Seconds 3
        $idleCpuAfter = Get-CpuTimeMillisecond $process
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

    if ($ImeMessageSmoke) {
        # First-keystroke regression: after a real global hotkey show, the focused
        # search control must accept a normal character without a mouse click.
        $firstKeystrokeBefore = if (Test-Path $queryProbePath) { @(Get-Content $queryProbePath).Count } else { 0 }
        $firstKeystrokeInputBefore = if (Test-Path $inputTracePath) { @(Get-Content $inputTracePath).Count } else { 0 }
        [FluxWallpaper]::keybd_event(0x51, 0, 0, [UIntPtr]::Zero)
        [FluxWallpaper]::keybd_event(0x51, 0, 2, [UIntPtr]::Zero)
        Start-Sleep -Milliseconds 500
        $firstKeystrokeLines = if (Test-Path $queryProbePath) {
            @(Get-Content $queryProbePath | Select-Object -Skip $firstKeystrokeBefore)
        } else {
            @()
        }
        $firstKeystrokeInputLines = if (Test-Path $inputTracePath) {
            @(Get-Content $inputTracePath | Select-Object -Skip $firstKeystrokeInputBefore)
        } else {
            @()
        }
        $firstKeystrokeSnapshotProbe = @($firstKeystrokeLines | Where-Object { $_ -match "query=q" }).Count -gt 0
        $firstKeystrokeInputProbe =
            @($firstKeystrokeInputLines | Where-Object { $_ -match "message=wm_char .*emitted=true .*routed=true" }).Count -gt 0 -and
            @($firstKeystrokeInputLines | Where-Object { $_ -match "message=key_event_dispatch .*emitted=true .*routed=true" }).Count -gt 0
        # Provider result merging may lag behind the first character on a busy
        # release runner; input routing is the lifecycle property under test.
        $firstKeystrokeProbe = $firstKeystrokeSnapshotProbe -or $firstKeystrokeInputProbe
        Write-Host "First-keystroke probe: snapshot=$firstKeystrokeSnapshotProbe routed_input=$firstKeystrokeInputProbe"
        if (!$firstKeystrokeProbe) {
            throw "First-keystroke focus smoke failed after global hotkey show."
        }

        # Clear the first character through the real launcher hide/show lifecycle
        # rather than relying on a synthetic selection shortcut. This also proves
        # that focus is restored before the Unicode messages are delivered.
        [FluxWallpaper]::SendMessage($launcherHandle, $wmHotkey, [UIntPtr]::Zero, [IntPtr]::Zero) | Out-Null
        Start-Sleep -Milliseconds 450
        if ([FluxWallpaper]::IsWindowVisible($launcherHandle)) {
            throw "IME message smoke could not hide the launcher before its clean-query cycle."
        }
        [FluxWallpaper]::SendMessage($launcherHandle, $wmHotkey, [UIntPtr]::Zero, [IntPtr]::Zero) | Out-Null
        Start-Sleep -Milliseconds 650
        if (![FluxWallpaper]::IsWindowVisible($launcherHandle) -or [FluxWallpaper]::GetForegroundWindow() -ne $launcherHandle) {
            throw "IME message smoke could not restore a focused launcher before Unicode injection."
        }
        Start-Sleep -Milliseconds 250

        $wmChar = 0x0102
        $wmImeChar = 0x0286
        $unicodeProbeBefore = if (Test-Path $queryProbePath) { @(Get-Content $queryProbePath).Count } else { 0 }
        $unicodeTraceBefore = if (Test-Path $inputTracePath) { @(Get-Content $inputTracePath).Count } else { 0 }
        [FluxWallpaper]::SendMessage($launcherHandle, $wmChar, [UIntPtr]0x4E2D, [IntPtr]1) | Out-Null
        Start-Sleep -Milliseconds 500
        $unicodeCharLines = if (Test-Path $queryProbePath) {
            @(Get-Content $queryProbePath | Select-Object -Skip $unicodeProbeBefore)
        } else {
            @()
        }
        $unicodeCharTrace = if (Test-Path $inputTracePath) {
            @(Get-Content $inputTracePath | Select-Object -Skip $unicodeTraceBefore)
        } else {
            @()
        }
        $wmCharProbe =
            @($unicodeCharTrace | Where-Object { $_ -match "message=wm_char .*emitted=true .*routed=true" }).Count -gt 0 -and
            @($unicodeCharTrace | Where-Object { $_ -match "message=key_event_dispatch .*emitted=true .*routed=true" }).Count -gt 0
        $unicodeQueryProbeObserved = @($unicodeCharLines | Where-Object { $_ -match "query=(?:q)?中(?:\s|$)" }).Count -gt 0

        $unicodeImeBefore = if (Test-Path $queryProbePath) { @(Get-Content $queryProbePath).Count } else { 0 }
        $unicodeImeTraceBefore = if (Test-Path $inputTracePath) { @(Get-Content $inputTracePath).Count } else { 0 }
        [FluxWallpaper]::SendMessage($launcherHandle, $wmImeChar, [UIntPtr]0x6587, [IntPtr]1) | Out-Null
        Start-Sleep -Milliseconds 500
        $unicodeImeLines = if (Test-Path $queryProbePath) {
            @(Get-Content $queryProbePath | Select-Object -Skip $unicodeImeBefore)
        } else {
            @()
        }
        $unicodeImeTrace = if (Test-Path $inputTracePath) {
            @(Get-Content $inputTracePath | Select-Object -Skip $unicodeImeTraceBefore)
        } else {
            @()
        }
        $wmImeCharProbe =
            @($unicodeImeTrace | Where-Object { $_ -match "message=wm_ime_char .*emitted=true .*routed=true" }).Count -gt 0 -and
            @($unicodeImeTrace | Where-Object { $_ -match "message=key_event_dispatch .*emitted=true .*routed=true" }).Count -gt 0
        $unicodeImeQueryProbeObserved = @($unicodeImeLines | Where-Object { $_ -match "query=(?:q)?(?:文|中文)(?:\s|$)" }).Count -gt 0
        $imeMessageProbe = $wmCharProbe -and $wmImeCharProbe
        $imeMessageDetails = [ordered]@{
            FirstKeystroke = $firstKeystrokeProbe
            WMChar = $wmCharProbe
            WMImeChar = $wmImeCharProbe
            UnicodeProbeLines = @($firstKeystrokeLines + $unicodeCharLines + $unicodeImeLines).Count
            UnicodeQueryProbeObserved = $unicodeQueryProbeObserved -and $unicodeImeQueryProbeObserved
            GenuineImeCompositionCovered = $false
        }
        if (!$imeMessageProbe) {
            throw "Unicode message routing smoke failed: WM_CHAR=$wmCharProbe WM_IME_CHAR=$wmImeCharProbe"
        }
    }

    $focusToggleProbe = $false
    $focusToggleVisibleAfterReopen = $false
    $focusToggleForegroundAfterReopen = $false
    $deactivationClickProbe = $false
    $deactivationHiddenAfterClick = $false
    $deactivationForegroundAfterClick = $false
    $deactivationCpuDelta = 0.0
    $deactivationIdleMemory = $null
    $traySettingsAfterDeactivationProbe = $false
    $traySettingsAfterDeactivationWindowWidth = 0
    $traySettingsAfterDeactivationWindowHeight = 0
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
        $launcherRect = New-Object FluxWallpaper+RECT
        if (![FluxWallpaper]::GetWindowRect($launcherHandle, [ref]$launcherRect)) {
            throw "Deactivation smoke could not read the launcher rectangle."
        }
        $deactivationTraceBeforeCount = if (Test-Path $launchTracePath) { @(Get-Content $launchTracePath).Count } else { 0 }
        [FluxWallpaper]::SetForegroundWindow($launcherHandle) | Out-Null
        Start-Sleep -Milliseconds 250
        if (![FluxWallpaper]::IsWindowVisible($launcherHandle)) {
            throw "Deactivation smoke expected Flux to be visible before the outside click."
        }
        if ($TraySettingsAfterDeactivationSmoke) {
            # Match the user's exact state: type a query, do not select a result,
            # then click another top-level window before opening Settings from tray.
            $deactivationQuery = "settings-tray-probe"
            $deactivationQueryBefore = if (Test-Path $queryProbePath) {
                @(Get-Content $queryProbePath).Count
            } else {
                0
            }
            $deactivationInputBefore = if (Test-Path $inputTracePath) {
                @(Get-Content $inputTracePath).Count
            } else {
                0
            }
            # Reset through the same global activation path used by the user,
            # then deliver ordinary WM_CHAR units synchronously to the real Flux
            # HWND. This preserves the no-Enter/no-selection state while avoiding
            # WScript.Shell focus races on the hosted Windows runner.
            [FluxWallpaper]::keybd_event(0x12, 0, 0, [UIntPtr]::Zero)
            [FluxWallpaper]::keybd_event(0x20, 0, 0, [UIntPtr]::Zero)
            [FluxWallpaper]::keybd_event(0x20, 0, 2, [UIntPtr]::Zero)
            [FluxWallpaper]::keybd_event(0x12, 0, 2, [UIntPtr]::Zero)
            Start-Sleep -Milliseconds 450
            [FluxWallpaper]::keybd_event(0x12, 0, 0, [UIntPtr]::Zero)
            [FluxWallpaper]::keybd_event(0x20, 0, 0, [UIntPtr]::Zero)
            [FluxWallpaper]::keybd_event(0x20, 0, 2, [UIntPtr]::Zero)
            [FluxWallpaper]::keybd_event(0x12, 0, 2, [UIntPtr]::Zero)
            Start-Sleep -Milliseconds 650
            if (![FluxWallpaper]::IsWindowVisible($launcherHandle)) {
                throw "Deactivation smoke could not restore Flux before typing the active query."
            }
            [FluxWallpaper]::SetForegroundWindow($launcherHandle) | Out-Null
            [FluxWallpaper]::SetFocus($launcherHandle) | Out-Null
            Start-Sleep -Milliseconds 200
            $deactivationShell = New-Object -ComObject WScript.Shell
            $deactivationShell.AppActivate($process.Id) | Out-Null
            $deactivationShell.SendKeys("^a")
            $deactivationShell.SendKeys("{BACKSPACE}")
            $deactivationShell.SendKeys($deactivationQuery)
            $deactivationQueryDeadline = (Get-Date).AddSeconds(3)
            $deactivationQueryObserved = $false
            $deactivationInputCharacters = 0
            while ((Get-Date) -lt $deactivationQueryDeadline) {
                if (Test-Path $queryProbePath) {
                    $deactivationQueryObserved = @(Get-Content $queryProbePath | Select-Object -Skip $deactivationQueryBefore |
                        Where-Object { $_ -match "query=$deactivationQuery" }).Count -gt 0
                }
                if (Test-Path $inputTracePath) {
                    $deactivationInputCharacters = @(Get-Content $inputTracePath | Select-Object -Skip $deactivationInputBefore |
                        Where-Object { $_ -match '^message=(wm_char|wm_ime_char) ' }).Count
                }
                # The trace is metadata-only and never contains the query text.
                # It proves that all requested characters were routed before the
                # outside click even when result-provider snapshots are delayed.
                if ($deactivationQueryObserved -or $deactivationInputCharacters -ge $deactivationQuery.Length) {
                    break
                }
                Start-Sleep -Milliseconds 100
            }
            Write-Host "Pre-deactivation query probe: characters=$deactivationInputCharacters expected=$($deactivationQuery.Length) snapshot_observed=$deactivationQueryObserved"
            if (!$deactivationQueryObserved -and $deactivationInputCharacters -lt $deactivationQuery.Length) {
                throw "Deactivation smoke could not observe the complete typed query before the outside click."
            }
            # Let result/layout updates settle before the outside click. This is the
            # normal user path after typing and keeps the test outside the backend's
            # short internal resize-transition suppression window.
            Start-Sleep -Milliseconds 700
        }
        # The probe covers the virtual screen, so its center can be underneath the
        # launcher itself. Select a point only after WindowFromPoint proves that the
        # actual top-level owner is the probe process and that it is outside Flux.
        $probeProcessId = [uint32]$probeProcess.Id
        $candidatePoints = @(
            [pscustomobject]@{ X = [int]($probeRect.Left + 8); Y = [int]($probeRect.Top + 8) },
            [pscustomobject]@{ X = [int]($probeRect.Right - 8); Y = [int]($probeRect.Top + 8) },
            [pscustomobject]@{ X = [int]($probeRect.Left + 8); Y = [int]($probeRect.Bottom - 32) },
            [pscustomobject]@{ X = [int]($probeRect.Right - 8); Y = [int]($probeRect.Bottom - 32) },
            [pscustomobject]@{ X = [int]($probeRect.Left + 20); Y = [int](($probeRect.Top + $probeRect.Bottom) / 2) },
            [pscustomobject]@{ X = [int]($probeRect.Right - 20); Y = [int](($probeRect.Top + $probeRect.Bottom) / 2) },
            [pscustomobject]@{ X = [int](($probeRect.Left + $probeRect.Right) / 2); Y = [int]($probeRect.Top + 20) },
            [pscustomobject]@{ X = [int](($probeRect.Left + $probeRect.Right) / 2); Y = [int]($probeRect.Bottom - 40) }
        )
        $outsidePoint = $null
        foreach ($candidate in $candidatePoints) {
            $insideLauncher = $candidate.X -ge $launcherRect.Left -and
                $candidate.X -lt $launcherRect.Right -and
                $candidate.Y -ge $launcherRect.Top -and
                $candidate.Y -lt $launcherRect.Bottom
            $ownerProcessId = [FluxWallpaper]::WindowProcessIdAtPoint($candidate.X, $candidate.Y)
            Write-Host "Deactivation candidate x=$($candidate.X) y=$($candidate.Y) owner_pid=$ownerProcessId inside_launcher=$insideLauncher"
            if (!$insideLauncher -and $ownerProcessId -eq $probeProcessId) {
                $outsidePoint = $candidate
                break
            }
        }
        if ($null -eq $outsidePoint) {
            throw "Deactivation smoke could not find a probe-owned point outside the launcher."
        }
        $clickX = [int]$outsidePoint.X
        $clickY = [int]$outsidePoint.Y
        [FluxWallpaper]::SetCursorPos($clickX, $clickY) | Out-Null
        [FluxWallpaper]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
        [FluxWallpaper]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
        $deactivationEvent = $null
        $deactivationTraceLines = @()
        for ($sample = 0; $sample -lt 20; $sample++) {
            $deactivationHiddenAfterClick = ![FluxWallpaper]::IsWindowVisible($launcherHandle)
            $deactivationForegroundAfterClick = [FluxWallpaper]::GetForegroundWindow() -ne $launcherHandle
            $deactivationTraceLines = if (Test-Path $launchTracePath) {
                @(Get-Content $launchTracePath | Select-Object -Skip $deactivationTraceBeforeCount)
            } else {
                @()
            }
            $deactivationEvent = $deactivationTraceLines |
                Where-Object { $_ -match "`twindow-deactivated$" } |
                Select-Object -First 1
            if ($deactivationHiddenAfterClick -and $deactivationForegroundAfterClick -and $deactivationEvent) {
                break
            }
            Start-Sleep -Milliseconds 100
        }
        $deactivationClickProbe =
            $deactivationHiddenAfterClick -and
            $deactivationForegroundAfterClick -and
            [bool]$deactivationEvent
        if (!$deactivationClickProbe) {
            throw "Deactivation smoke failed: hidden=$deactivationHiddenAfterClick foreground_probe=$deactivationForegroundAfterClick callback=$([bool]$deactivationEvent)."
        }
        if ($TraySettingsAfterDeactivationSmoke) {
            # This is the user's lifecycle: leave a query active, activate another
            # top-level window so Flux hides, then use the real Win32 tray menu to
            # choose Settings. The shared tray helper exercises TrayMenuItem,
            # TrackPopupMenu, the Settings callback, and native show/resize order
            # without relying on brittle taskbar notification-area coordinates.
            # The cleanup block below still posts the raw tray callback, so the
            # shared menu anchor point and message constants stay here.
            $wmTrayIcon = 0x8001
            $wmRButtonUp = 0x0205
            $trayMenuX = $clickX
            $trayMenuY = $clickY
            $afterDeactivationTray = Invoke-FluxTraySettingsClick `
                -LauncherHandle $launcherHandle -MenuX $trayMenuX -MenuY $trayMenuY `
                -Context "Settings after deactivation"
            $traySettingsAfterDeactivationWindowWidth = $afterDeactivationTray.Width
            $traySettingsAfterDeactivationWindowHeight = $afterDeactivationTray.Height
            $traySettingsAfterDeactivationProbe = $afterDeactivationTray.Full
            Write-Host "Settings after deactivation geometry: ${traySettingsAfterDeactivationWindowWidth}x${traySettingsAfterDeactivationWindowHeight} full=$traySettingsAfterDeactivationProbe"
            if (!$traySettingsAfterDeactivationProbe) {
                throw "Tray Settings after deactivation smoke reproduced compact/invalid window: ${traySettingsAfterDeactivationWindowWidth}x${traySettingsAfterDeactivationWindowHeight}."
            }
            $settingsAfterDeactivationRect = New-Object FluxWallpaper+RECT
            if (![FluxWallpaper]::GetWindowRect($launcherHandle, [ref]$settingsAfterDeactivationRect)) {
                throw "Tray Settings deactivation smoke could not read the expanded Settings rectangle."
            }
            Save-Screenshot "tray-settings-after-deactivation.png"
            # Let the native Settings resize/deactivation guard settle before the
            # next genuine outside click. The user can click another app while the
            # panel is still painting, but the smoke must not mistake that internal
            # 250 ms resize grace period for a product hide failure.
            Start-Sleep -Milliseconds 350
            # Reproduce the user's second half exactly: click an empty area of the
            # other top-level window while Settings is open, then choose Settings
            # from the tray a second time and require another full panel.
            $secondOutsidePoint = $null
            foreach ($candidate in $candidatePoints) {
                $insideSettings = $candidate.X -ge $settingsAfterDeactivationRect.Left -and
                    $candidate.X -lt $settingsAfterDeactivationRect.Right -and
                    $candidate.Y -ge $settingsAfterDeactivationRect.Top -and
                    $candidate.Y -lt $settingsAfterDeactivationRect.Bottom
                $ownerProcessId = [FluxWallpaper]::WindowProcessIdAtPoint($candidate.X, $candidate.Y)
                if (!$insideSettings -and $ownerProcessId -eq $probeProcessId) {
                    $secondOutsidePoint = $candidate
                    break
                }
            }
            if ($null -eq $secondOutsidePoint) {
                throw "Tray Settings regression could not find a probe-owned point outside the expanded Settings panel."
            }
            $secondClickX = [int]$secondOutsidePoint.X
            $secondClickY = [int]$secondOutsidePoint.Y
            $secondOwnerBeforeClick = [FluxWallpaper]::WindowProcessIdAtPoint($secondClickX, $secondClickY)
            $secondWindowBeforeClick = [FluxWallpaper]::WindowHandleAtPoint($secondClickX, $secondClickY)
            $secondForegroundBeforeClick = [FluxWallpaper]::GetForegroundWindow()
            Write-Host "Expanded Settings rect=$($settingsAfterDeactivationRect.Left),$($settingsAfterDeactivationRect.Top)-$($settingsAfterDeactivationRect.Right),$($settingsAfterDeactivationRect.Bottom) launcher_hwnd=$launcherHandle probe_hwnd=$deactivationProbeHandle"
            Write-Host "Second outside click x=$secondClickX y=$secondClickY owner_pid=$secondOwnerBeforeClick owner_hwnd=$secondWindowBeforeClick foreground_before=$secondForegroundBeforeClick visible_before=$([FluxWallpaper]::IsWindowVisible($launcherHandle))"
            [FluxWallpaper]::SetCursorPos($secondClickX, $secondClickY) | Out-Null
            [FluxWallpaper]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
            [FluxWallpaper]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
            $secondDeactivationDeadline = (Get-Date).AddSeconds(2)
            $secondDeactivationHidden = $false
            $secondDeactivationSample = 0
            while ((Get-Date) -lt $secondDeactivationDeadline) {
                $secondDeactivationHidden = ![FluxWallpaper]::IsWindowVisible($launcherHandle)
                $secondDeactivationForeground = [FluxWallpaper]::GetForegroundWindow()
                $secondDeactivationOwner = [FluxWallpaper]::WindowProcessIdAtPoint($secondClickX, $secondClickY)
                $secondDeactivationWindow = [FluxWallpaper]::WindowHandleAtPoint($secondClickX, $secondClickY)
                $secondDeactivationTraceCount = if (Test-Path $launchTracePath) {
                    @(Get-Content $launchTracePath).Count
                } else {
                    0
                }
                if ($secondDeactivationSample -eq 0 -or $secondDeactivationHidden -or ($secondDeactivationSample % 5) -eq 0) {
                    Write-Host "Second outside sample=$secondDeactivationSample visible=$(!$secondDeactivationHidden) foreground=$secondDeactivationForeground owner_pid=$secondDeactivationOwner owner_hwnd=$secondDeactivationWindow trace_lines=$secondDeactivationTraceCount"
                }
                if ($secondDeactivationHidden) { break }
                $secondDeactivationSample++
                Start-Sleep -Milliseconds 100
            }
            if (!$secondDeactivationHidden) {
                throw "Tray Settings regression could not hide the first Settings panel after the second outside click."
            }
            $secondTrayAttempt = Invoke-FluxTraySettingsClick `
                -LauncherHandle $launcherHandle -MenuX $trayMenuX -MenuY $trayMenuY `
                -Context "Tray Settings regression (second attempt)"
            $secondSettingsWidth = $secondTrayAttempt.Width
            $secondSettingsHeight = $secondTrayAttempt.Height
            $secondSettingsFull = $secondTrayAttempt.Full
            Write-Host "Settings second tray attempt geometry: ${secondSettingsWidth}x${secondSettingsHeight} full=$secondSettingsFull"
            if (!$secondSettingsFull) {
                throw "Tray Settings regression reproduced on the second attempt: ${secondSettingsWidth}x${secondSettingsHeight}."
            }
            Save-Screenshot "tray-settings-second-attempt.png"
            # Return to the launcher's compact state through the real tray menu,
            # then hide it so the existing Alt+Space restore assertion below still
            # starts from the same hidden state as the normal deactivation smoke.
            [FluxWallpaper]::SetCursorPos($trayMenuX, $trayMenuY) | Out-Null
            if (![FluxWallpaper]::PostMessage($launcherHandle, $wmTrayIcon, [UIntPtr]::Zero, [IntPtr]$wmRButtonUp)) {
                throw "Tray Settings deactivation smoke could not reopen the tray menu for cleanup."
            }
            $cleanupMenuDeadline = (Get-Date).AddSeconds(2)
            $cleanupMenuReady = $false
            while ((Get-Date) -lt $cleanupMenuDeadline) {
                if ([FluxWallpaper]::WindowClassAtPoint($trayMenuX, $trayMenuY) -eq "#32768") {
                    $cleanupMenuReady = $true
                    break
                }
                Start-Sleep -Milliseconds 50
            }
            if (!$cleanupMenuReady) {
                throw "Tray Settings deactivation smoke could not observe the cleanup popup menu."
            }
            $cleanupMenuHandle = [FluxWallpaper]::WindowHandleAtPoint($trayMenuX, $trayMenuY)
            $cleanupMenuRect = New-Object FluxWallpaper+RECT
            if ($cleanupMenuHandle -eq [IntPtr]::Zero -or
                ![FluxWallpaper]::GetWindowRect($cleanupMenuHandle, [ref]$cleanupMenuRect)) {
                throw "Tray Settings deactivation smoke could not read the cleanup popup menu rectangle."
            }
            $cleanupShowX = $cleanupMenuRect.Left + [int]([FluxWallpaper]::RectWidth($cleanupMenuRect) / 2)
            $cleanupShowY = $cleanupMenuRect.Top + [int]([FluxWallpaper]::RectHeight($cleanupMenuRect) * 0.12)
            [FluxWallpaper]::SetCursorPos($cleanupShowX, $cleanupShowY) | Out-Null
            [FluxWallpaper]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
            [FluxWallpaper]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
            Start-Sleep -Milliseconds 500
            if (![FluxWallpaper]::IsWindowVisible($launcherHandle)) {
                throw "Tray Settings deactivation smoke cleanup could not show the launcher."
            }
            [FluxWallpaper]::keybd_event(0x12, 0, 0, [UIntPtr]::Zero)
            [FluxWallpaper]::keybd_event(0x20, 0, 0, [UIntPtr]::Zero)
            [FluxWallpaper]::keybd_event(0x20, 0, 2, [UIntPtr]::Zero)
            [FluxWallpaper]::keybd_event(0x12, 0, 2, [UIntPtr]::Zero)
            Start-Sleep -Milliseconds 500
            if ([FluxWallpaper]::IsWindowVisible($launcherHandle)) {
                throw "Tray Settings deactivation smoke cleanup could not hide the compact launcher."
            }
        }
        if ($IdlePerformanceSmoke) {
            Start-Sleep -Milliseconds 1200
            $deactivationCpuBefore = Get-CpuTimeMillisecond $process
            Start-Sleep -Seconds 3
            $deactivationIdleMemory = Get-MemorySnapshot $process
            $deactivationCpuAfter = Get-CpuTimeMillisecond $process
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
    $resultRmbLaunchProbe = $false
    $resultRmbDispatchObserved = $false
    $resultRmbWindowHidden = $false
    $resultNormalHoverTextCursor = $false
    $resultNormalHoverCopyDisabledProbe = $false
    $resultNormalClickLaunchProbe = $false
    $resultNormalClickDispatchObserved = $false
    $resultNormalClickWindowHidden = $false
    $resultCtrlHoverTextCursor = $false
    $resultCtrlCopyProbe = $false
    $resultCtrlShiftCopyProbe = $false
    $resultCtrlSelectionWindowVisible = $false
    $commandPriorityProbe = $false
    $compactAppProbe = $false
    $compactAppProbeLine = $null
    $powerShellDedupeProbe = $false
    $powerShellIconProbe = $false
    $calculatorProbe = $false
    $calculatorPolicyProbe = $false
    $resourceProfileProbe = !$ResourceProfileSmoke
    $resourceProfileSamples = @()
    $resourceProfileSummary = $null
    if ($ResourceProfileSmoke) {
        # Exercise the same high-churn query path that populates application, Everything,
        # and shell-icon results. The workload is intentionally bounded and records
        # process counters instead of treating allocator high-water marks as a leak.
        $profileQueries = @("wab", "ext:zip", ".png", "lmstudio", "ob ornith")
        $profileCycles = [Math]::Max(1, $ResourceProfileCycles)
        $profileStart = Get-MemorySnapshot $process
        $profileCpuStart = Get-CpuTimeMillisecond $process
        $profilePeakPrivate = [int64]$profileStart.PrivateBytes
        $profilePeakWorkingSet = [int64]$profileStart.WorkingSetBytes
        $profilePeakHandles = [int64]$profileStart.HandleCount
        $profilePeakThreads = [int64]$profileStart.ThreadCount
        # Scenario baselines: Idle is the settled, empty-query reference every other
        # phase is compared against. Phases are recorded rather than gated so the
        # first runs establish numbers before any budget is claimed for them.
        $resourceProfileScenarios = [ordered]@{
            Idle = $profileStart
        }
        for ($cycle = 0; $cycle -lt $profileCycles; $cycle++) {
            $profileQuery = $profileQueries[$cycle % $profileQueries.Count]
            $shell.SendKeys("^a")
            $shell.SendKeys("{BACKSPACE}")
            $shell.SendKeys($profileQuery)
            Start-Sleep -Milliseconds 110
            if (($cycle + 1) % 4 -eq 0) {
                $sample = Get-MemorySnapshot $process
                $resourceProfileSamples += [ordered]@{
                    Cycle = $cycle + 1
                    Query = $profileQuery
                    WorkingSetBytes = $sample.WorkingSetBytes
                    PrivateBytes = $sample.PrivateBytes
                    VirtualBytes = $sample.VirtualBytes
                    HandleCount = $sample.HandleCount
                    ThreadCount = $sample.ThreadCount
                }
                $profilePeakPrivate = [Math]::Max($profilePeakPrivate, [int64]$sample.PrivateBytes)
                $profilePeakWorkingSet = [Math]::Max($profilePeakWorkingSet, [int64]$sample.WorkingSetBytes)
                $profilePeakHandles = [Math]::Max($profilePeakHandles, [int64]$sample.HandleCount)
                $profilePeakThreads = [Math]::Max($profilePeakThreads, [int64]$sample.ThreadCount)
            }
        }
        $shell.SendKeys("^a")
        $shell.SendKeys("{BACKSPACE}")
        Start-Sleep -Milliseconds 750
        $profileEnd = Get-MemorySnapshot $process
        Start-Sleep -Seconds 3
        $profileQuietEnd = Get-MemorySnapshot $process
        $profileCpuEnd = Get-CpuTimeMillisecond $process
        $resourceProfileScenarios.ChurnQuiet = $profileQuietEnd
        $profilePrivateGrowth = [int64]$profileEnd.PrivateBytes - [int64]$profileStart.PrivateBytes
        $profileWorkingSetGrowth = [int64]$profileEnd.WorkingSetBytes - [int64]$profileStart.WorkingSetBytes
        $profileHandleGrowth = [int64]$profileEnd.HandleCount - [int64]$profileStart.HandleCount
        $profileThreadGrowth = [int64]$profileEnd.ThreadCount - [int64]$profileStart.ThreadCount
        $profileQuietPrivateGrowth = [int64]$profileQuietEnd.PrivateBytes - [int64]$profileStart.PrivateBytes
        $profileQuietWorkingSetGrowth = [int64]$profileQuietEnd.WorkingSetBytes - [int64]$profileStart.WorkingSetBytes
        $profileQuietHandleGrowth = [int64]$profileQuietEnd.HandleCount - [int64]$profileStart.HandleCount
        $profileQuietThreadGrowth = [int64]$profileQuietEnd.ThreadCount - [int64]$profileStart.ThreadCount
        $profileCpuDelta = [Math]::Round($profileCpuEnd - $profileCpuStart, 2)
        # Icon cache pressure: single-character queries surface the widest distinct
        # icon target set, driving the 128-entry shell icon LRU towards its bound.
        # Recorded only; the gated budget above stays the single CI tripwire.
        foreach ($pressureCode in 97..122) {
            $shell.SendKeys("^a")
            $shell.SendKeys("{BACKSPACE}")
            $shell.SendKeys([string][char]$pressureCode)
            Start-Sleep -Milliseconds 90
        }
        $iconPressureSnapshot = Get-MemorySnapshot $process
        $resourceProfileScenarios.IconCachePressure = $iconPressureSnapshot
        $shell.SendKeys("^a")
        $shell.SendKeys("{BACKSPACE}")
        Start-Sleep -Seconds 3
        $iconPressureQuietSnapshot = Get-MemorySnapshot $process
        $resourceProfileScenarios.IconCachePressureQuiet = $iconPressureQuietSnapshot
        $iconPressureQuietPrivateGrowth =
            [int64]$iconPressureQuietSnapshot.PrivateBytes - [int64]$profileStart.PrivateBytes
        $resourceProfileSummary = [ordered]@{
            Cycles = $profileCycles
            Queries = $profileQueries
            Start = $profileStart
            End = $profileEnd
            QuietEndAfterSeconds = 3
            QuietEnd = $profileQuietEnd
            PeakPrivateBytes = $profilePeakPrivate
            PeakWorkingSetBytes = $profilePeakWorkingSet
            PeakHandleCount = $profilePeakHandles
            PeakThreadCount = $profilePeakThreads
            PrivateGrowthBytes = $profilePrivateGrowth
            WorkingSetGrowthBytes = $profileWorkingSetGrowth
            HandleGrowth = $profileHandleGrowth
            ThreadGrowth = $profileThreadGrowth
            QuietPrivateGrowthBytes = $profileQuietPrivateGrowth
            QuietWorkingSetGrowthBytes = $profileQuietWorkingSetGrowth
            QuietHandleGrowth = $profileQuietHandleGrowth
            QuietThreadGrowth = $profileQuietThreadGrowth
            CpuTimeMilliseconds = $profileCpuDelta
            Scenarios = $resourceProfileScenarios
            IconCachePressurePrivateGrowthBytes = $iconPressureQuietPrivateGrowth
        }
        # This is a coarse CI guard for catastrophic retained growth, not proof that a
        # process is leak-free. Long-run PerfMon/WPR remains the authoritative follow-up.
        $resourceProfileProbe =
            $profileQuietPrivateGrowth -lt (128 * 1024 * 1024) -and
            $profileQuietHandleGrowth -le 256 -and
            $profileQuietThreadGrowth -le 8
        if (!$resourceProfileProbe) {
            throw "Resource profile growth budget exceeded after quiet period: private=$profileQuietPrivateGrowth bytes handles=$profileQuietHandleGrowth threads=$profileQuietThreadGrowth after $profileCycles cycles."
        }
        Write-Host "Resource profile: cycles=$profileCycles private_growth=$profilePrivateGrowth quiet_private_growth=$profileQuietPrivateGrowth working_set_growth=$profileWorkingSetGrowth quiet_working_set_growth=$profileQuietWorkingSetGrowth handle_growth=$profileHandleGrowth quiet_handle_growth=$profileQuietHandleGrowth thread_growth=$profileThreadGrowth quiet_thread_growth=$profileQuietThreadGrowth cpu_ms=$profileCpuDelta"
        Write-Host "Resource profile scenarios (private bytes): idle=$($profileStart.PrivateBytes) churn_quiet=$($profileQuietEnd.PrivateBytes) icon_cache_pressure=$($iconPressureSnapshot.PrivateBytes) icon_cache_pressure_quiet=$($iconPressureQuietSnapshot.PrivateBytes) icon_quiet_growth=$iconPressureQuietPrivateGrowth"
    }
    if ($CommandPrioritySmoke) {
        foreach ($commandQuery in @("cmd", "powershell", "pwsh")) {
            $shell.SendKeys("^a")
            $shell.SendKeys("{BACKSPACE}")
            $shell.SendKeys($commandQuery)
            Start-Sleep -Milliseconds 700
            Save-Screenshot ("command-priority-{0}.png" -f $commandQuery)
        }
        # Keep a real compact-name capture alongside the console priority frames.
        # The temporary LM Studio.lnk above makes this an end-to-end
        # ApplicationCatalog check even when the hosted runner has no LM Studio
        # installation of its own.
        $shell.SendKeys("^a")
        $shell.SendKeys("{BACKSPACE}")
        $shell.SendKeys("lmstudio")
        Start-Sleep -Milliseconds 2000
        Save-Screenshot "compact-query-lmstudio.png"
        if (Test-Path $compactAppProbePath) {
            foreach ($probeLine in @(Get-Content $compactAppProbePath)) {
                if ($probeLine -match "^query=lmstudio`ttitle=LM Studio`tid=(?<id>[^`t]+)`ttarget=") {
                    $probeId = [string]$Matches["id"]
                    if ($probeId -notmatch "lmstudio") {
                        $compactAppProbeLine = $probeLine
                        $compactAppProbe = $true
                        break
                    }
                }
            }
        }
        Write-Host "Compact application probe: passed=$compactAppProbe line=[$compactAppProbeLine]"
        if (!$compactAppProbe) {
            throw "Compact application smoke did not return LM Studio by title with an executable identity free of lmstudio: $compactAppProbePath"
        }
        $commandPriorityProbe = $true
    }
    if ($PowerShellSmoke) {
        $powerShellRows = @()
        $powerShellDedupeProbe = $true
        foreach ($powerShellQuery in @("powershell", "pwsh")) {
            if (Test-Path $queryProbePath) {
                Clear-Content -Path $queryProbePath -ErrorAction SilentlyContinue
            }
            $shell.SendKeys("^a")
            $shell.SendKeys("{BACKSPACE}")
            $shell.SendKeys($powerShellQuery)
            Start-Sleep -Seconds 2
            Save-Screenshot ("{0}-results.png" -f $powerShellQuery)

            $queryRows = @()
            $querySnapshot = $null
            $expectedQueryCount = $null
            $queryDeadline = (Get-Date).AddSeconds(3)
            while ((Get-Date) -lt $queryDeadline) {
                if (Test-Path $queryProbePath) {
                    $queryLines = @(Get-Content $queryProbePath)
                    $queryHeaders = @($queryLines | Where-Object {
                        $_ -match "^snapshot=(\d+)`tquery=$powerShellQuery`tcount=(\d+)$"
                    })
                    if ($queryHeaders.Count -gt 0) {
                        $header = $queryHeaders | Select-Object -Last 1
                        if ($header -match "^snapshot=(\d+)`tquery=$powerShellQuery`tcount=(\d+)$") {
                            $querySnapshot = [uint64]$Matches[1]
                            $expectedQueryCount = [int]$Matches[2]
                            $queryRows = @($queryLines | Where-Object {
                                $_ -match "^snapshot=$querySnapshot`tquery=$powerShellQuery`tindex="
                            })
                            if ($queryRows.Count -ge $expectedQueryCount) { break }
                        }
                    }
                }
                Start-Sleep -Milliseconds 100
            }
            $powerShellRows += $queryRows
            $identityRows = @($queryRows | Where-Object {
                $_ -match "`tsource=(ApplicationCatalog|BuiltIn|Everything)`t" -and
                $_ -match "`tkind=(Application|Command)`t"
            })
            $identities = @($identityRows | ForEach-Object {
                if ($_ -match "`tidentity=([^`t]*)") { $Matches[1] }
            } | Where-Object { $_ -and $_.Length -gt 0 })
            $duplicates = @($identities | Group-Object | Where-Object Count -gt 1)
            $queryPassed =
                $null -ne $expectedQueryCount -and
                $identityRows.Count -gt 0 -and
                $identities.Count -eq $identityRows.Count -and
                $duplicates.Count -eq 0
            $powerShellDedupeProbe = $powerShellDedupeProbe -and $queryPassed
            Write-Host "PowerShell query '$powerShellQuery': rows=$($queryRows.Count) identity_rows=$($identityRows.Count) unique_identities=$($identities.Count) duplicate_identities=$($duplicates.Count)"
        }

        $powerShellIconLines = @(Get-Content $iconProbePath -ErrorAction SilentlyContinue | Where-Object {
            $_ -match "(?i)^title=.*(powershell|pwsh).*`ttarget="
        })
        $powerShellIconTargets = @($powerShellIconLines | ForEach-Object {
            if ($_ -match "`ticon_target=([^`t]*)") { $Matches[1] }
        } | Where-Object { $_ -and $_.Length -gt 0 } | Sort-Object -Unique)
        # 图标请求节流后，只有「立即提取窗口」（可见行 + 选区邻域，probe 行的 eager=True）
        # 内的行才保证完成提取；窗口外的行保持字形回退，不作为烟雾测试断言对象。
        $eagerPowerShellIconTargets = @($powerShellIconLines | Where-Object {
            $_ -match "`teager=True"
        } | ForEach-Object {
            if ($_ -match "`ticon_target=([^`t]*)") { $Matches[1] }
        } | Where-Object { $_ -and $_.Length -gt 0 } | Sort-Object -Unique)
        $loadedPowerShellTargets = @(Get-Content $iconProbePath -ErrorAction SilentlyContinue | ForEach-Object {
            if ($_ -match "(?i)^target=(.*)`tloaded=True$") { $Matches[1] }
        } | Where-Object { $_ -and $_.Length -gt 0 } | Sort-Object -Unique)
        $loadedEagerPowerShellIconTargets = @($eagerPowerShellIconTargets | Where-Object {
            $loadedPowerShellTargets -contains $_
        })
        $unloadedEagerPowerShellIconTargets = @($eagerPowerShellIconTargets | Where-Object {
            $loadedPowerShellTargets -notcontains $_
        })
        $powerShellIconProbe =
            $eagerPowerShellIconTargets.Count -gt 0 -and
            $unloadedEagerPowerShellIconTargets.Count -eq 0 -and
            $loadedEagerPowerShellIconTargets.Count -eq $eagerPowerShellIconTargets.Count
        Write-Host "PowerShell probes: icon_targets=$($powerShellIconTargets.Count) eager_targets=$($eagerPowerShellIconTargets.Count) loaded_eager=$($loadedEagerPowerShellIconTargets.Count) unloaded_eager=$($unloadedEagerPowerShellIconTargets.Count)"
        if (!$powerShellDedupeProbe) {
            throw "PowerShell dedupe smoke failed: rows=$($powerShellRows.Count)."
        }
        if (!$powerShellIconProbe) {
            throw "PowerShell icon smoke failed: no loaded icon matched a PowerShell result target; targets=$($powerShellIconTargets -join ', ')."
        }
    }
    if ($CalculatorSmoke) {
        $shell.SendKeys("^a")
        $shell.SendKeys("{BACKSPACE}")
        # WScript.Shell.SendKeys treats plus as a Shift modifier; escape it
        # so the application receives the literal expression `1+1`.
        $shell.SendKeys("1{+}1")
        Start-Sleep -Milliseconds 700
        Save-Screenshot "calculator-1-plus-1.png"

        $calculatorProbe = $false
        $calculatorSnapshot = $null
        $calculatorExpectedCount = $null
        $calculatorRows = @()
        $calculatorDeadline = (Get-Date).AddSeconds(3)
        while ((Get-Date) -lt $calculatorDeadline) {
            if (Test-Path $queryProbePath) {
                $calculatorLines = @(Get-Content $queryProbePath)
                $calculatorHeaders = @($calculatorLines | Where-Object {
                    $_ -match "^snapshot=(\d+)`tquery=1\+1`tcount=(\d+)$"
                })
                if ($calculatorHeaders.Count -gt 0) {
                    $calculatorHeader = $calculatorHeaders | Select-Object -Last 1
                    if ($calculatorHeader -match "^snapshot=(\d+)`tquery=1\+1`tcount=(\d+)$") {
                        $calculatorSnapshot = [uint64]$Matches[1]
                        $calculatorExpectedCount = [int]$Matches[2]
                        $calculatorRows = @($calculatorLines | Where-Object {
                            $_ -match "^snapshot=$calculatorSnapshot`tquery=1\+1`tindex="
                        })
                        if ($calculatorRows.Count -ge $calculatorExpectedCount) {
                            $calculatorProbe = $calculatorRows[0] -match "`tindex=0`tid=builtin:calculator`t"
                            break
                        }
                    }
                }
            }
            Start-Sleep -Milliseconds 100
        }
        Write-Host "Calculator ordering probe: passed=$calculatorProbe snapshot=$calculatorSnapshot rows=$($calculatorRows.Count) expected=$calculatorExpectedCount"
        if (!$calculatorProbe) {
            throw "Calculator ordering smoke failed: the latest complete 1+1 snapshot did not put builtin:calculator at index 0."
        }
    }
    if ($CalculatorPolicySmoke) {
        if (Test-Path $queryProbePath) {
            Clear-Content -Path $queryProbePath -ErrorAction SilentlyContinue
        }
        $shell.SendKeys("^a")
        $shell.SendKeys("{BACKSPACE}")
        $shell.SendKeys("2026-08")
        Start-Sleep -Milliseconds 700
        Save-Screenshot "calculator-date-like.png"

        $calculatorPolicyDeadline = (Get-Date).AddSeconds(3)
        $calculatorPolicySnapshot = $null
        $calculatorPolicyExpectedCount = $null
        $calculatorPolicyRows = @()
        while ((Get-Date) -lt $calculatorPolicyDeadline) {
            if (Test-Path $queryProbePath) {
                $calculatorPolicyLines = @(Get-Content $queryProbePath)
                $calculatorPolicyHeaders = @($calculatorPolicyLines | Where-Object {
                    $_ -match "^snapshot=(\d+)`tquery=2026-08`tcount=(\d+)$"
                })
                if ($calculatorPolicyHeaders.Count -gt 0) {
                    $calculatorPolicyHeader = $calculatorPolicyHeaders | Select-Object -Last 1
                    if ($calculatorPolicyHeader -match "^snapshot=(\d+)`tquery=2026-08`tcount=(\d+)$") {
                        $calculatorPolicySnapshot = [uint64]$Matches[1]
                        $calculatorPolicyExpectedCount = [int]$Matches[2]
                        $calculatorPolicyRows = @($calculatorPolicyLines | Where-Object {
                            $_ -match "^snapshot=$calculatorPolicySnapshot`tquery=2026-08`tindex="
                        })
                        if ($calculatorPolicyRows.Count -ge $calculatorPolicyExpectedCount) {
                            $calculatorPolicyProbe = $calculatorPolicyRows[0] -match "`tindex=0`tid=builtin:calculator`t"
                            break
                        }
                    }
                }
            }
            Start-Sleep -Milliseconds 100
        }
        Write-Host "Calculator date-like policy probe: passed=$calculatorPolicyProbe snapshot=$calculatorPolicySnapshot rows=$($calculatorPolicyRows.Count) expected=$calculatorPolicyExpectedCount"
        if (!$calculatorPolicyProbe) {
            throw "Calculator date-like policy smoke failed: latest complete 2026-08 snapshot did not put builtin:calculator at index 0."
        }
    }
    if ($ObsidianIconSmoke) {
        $shell.SendKeys("^a")
        $shell.SendKeys("{BACKSPACE}")
        $shell.SendKeys("ob ornith")
        Start-Sleep -Milliseconds 900
        Save-Screenshot "obsidian-icon.png"
        $obsidianIconProbe = $true
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
    # Use the unique shortcut fixture created above for the launch/hide ordering
    # probe. A generic query such as `wifi` can return a different provider result
    # in front of the built-in target, so Home/Down would no longer prove the
    # real shell-launch path deterministically. Recycle Bin commands are covered
    # separately and the first one intentionally opens confirmation mode instead
    # of dispatching a shell launch.
    $enterHideQuery = "zq7launchprobe"
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
    $queryMemory = Get-MemorySnapshot $process
    Save-Screenshot "everything-fallback.png"

    if ($ResultMouseInteractionSmoke) {
        # Reproduce the two reported interactions on a deterministic Start Menu
        # shortcut. The current behavior is expected to fail this block: RichText
        # owns the title's right-click and exposes a text cursor on plain hover.
        $resultPointerQuery = "resultmouseprobe"
        $shell.SendKeys("^a")
        $shell.SendKeys("{BACKSPACE}")
        $shell.SendKeys($resultPointerQuery)
        Start-Sleep -Seconds 2

        # With the query input focused, Right Arrow must move the caret when it
        # is not at the end. Reassert focus by clicking the search field before
        # sending the navigation keys; otherwise SendKeys can target the runner
        # or a stale result widget after asynchronous result publication.
        [FluxWallpaper]::SetForegroundWindow($launcherHandle) | Out-Null
        $caretFocusRect = New-Object FluxWallpaper+RECT
        if (![FluxWallpaper]::GetWindowRect($launcherHandle, [ref]$caretFocusRect)) {
            throw "Unable to locate launcher before caret focus probe."
        }
        # `resultmouseprobe` is roughly 160 DIP wide; click inside its middle,
        # not at a fixed right-side coordinate that can land at the end.
        [FluxWallpaper]::SetCursorPos($caretFocusRect.Left + 115, $caretFocusRect.Top + 28) | Out-Null
        [FluxWallpaper]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
        [FluxWallpaper]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
        Start-Sleep -Milliseconds 250
        $shell.SendKeys("{RIGHT}")
        Start-Sleep -Milliseconds 500
        $caretMiddleRect = New-Object FluxWallpaper+RECT
        $caretMiddleWindowVisible = [FluxWallpaper]::IsWindowVisible($launcherHandle)
        $caretMiddleWindowHeight = if ([FluxWallpaper]::GetWindowRect($launcherHandle, [ref]$caretMiddleRect)) {
            [FluxWallpaper]::RectHeight($caretMiddleRect)
        } else {
            0
        }
        $resultRightArrowMiddleCaretActionMenu = $caretMiddleWindowVisible -and $caretMiddleWindowHeight -ge 240
        $resultRightArrowMiddleCaretProbe = $caretMiddleWindowVisible -and !$resultRightArrowMiddleCaretActionMenu
        Write-Host "Right Arrow caret-middle probe: visible=$caretMiddleWindowVisible height=$caretMiddleWindowHeight action_menu=$resultRightArrowMiddleCaretActionMenu passed=$resultRightArrowMiddleCaretProbe"
        Save-Screenshot "result-keyboard-caret-middle.png"
        if ($resultRightArrowMiddleCaretActionMenu) {
            [FluxWallpaper]::keybd_event(0x1B, 0, 0, [UIntPtr]::Zero)
            [FluxWallpaper]::keybd_event(0x1B, 0, 2, [UIntPtr]::Zero)
            Start-Sleep -Milliseconds 250
        }

        $resultPointerRect = New-Object FluxWallpaper+RECT
        if (![FluxWallpaper]::GetWindowRect($launcherHandle, [ref]$resultPointerRect)) {
            throw "Result mouse interaction smoke could not locate launcher rectangle."
        }
        $resultTitleX = $resultPointerRect.Left + 80
        $resultTitleY = $resultPointerRect.Top + 75
        [FluxWallpaper]::SetCursorPos($resultTitleX, $resultTitleY) | Out-Null
        Start-Sleep -Milliseconds 450
        $resultNormalHoverTextCursor = [FluxWallpaper]::IsTextCursor()
        $resultNormalHoverCopyDisabledProbe = !$resultNormalHoverTextCursor
        Save-Screenshot "result-pointer-normal-hover.png"

        # A plain click on the title must launch the result. RichText currently
        # consumes this gesture as a text-selection start, so this is expected to
        # fail in the pre-fix baseline.
        $normalClickTraceBeforeCount = if (Test-Path $launchTracePath) { @(Get-Content $launchTracePath).Count } else { 0 }
        [FluxWallpaper]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
        [FluxWallpaper]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
        Start-Sleep -Milliseconds 900
        $normalClickTraceLines = if (Test-Path $launchTracePath) {
            @(Get-Content $launchTracePath | Select-Object -Skip $normalClickTraceBeforeCount)
        } else {
            @()
        }
        $resultNormalClickDispatchObserved = [bool]($normalClickTraceLines | Where-Object { $_ -match "`tlaunch-dispatch$" } | Select-Object -First 1)
        $resultNormalClickWindowHidden = ![FluxWallpaper]::IsWindowVisible($launcherHandle)
        $resultNormalClickLaunchProbe = $resultNormalClickDispatchObserved -and $resultNormalClickWindowHidden
        Save-Screenshot "result-pointer-normal-click.png"
        # Restore through the same bounded WM_HOTKEY path used by the stable
        # Enter smoke. A synthetic Alt+Space can be swallowed while Explorer
        # becomes foreground after the result launch.
        $restoreDeadline = (Get-Date).AddSeconds(5)
        while (![FluxWallpaper]::IsWindowVisible($launcherHandle) -and (Get-Date) -lt $restoreDeadline) {
            [FluxWallpaper]::SendMessage($launcherHandle, $wmHotkey, [UIntPtr]::Zero, [IntPtr]::Zero) | Out-Null
            Start-Sleep -Milliseconds 650
        }
        if (![FluxWallpaper]::IsWindowVisible($launcherHandle)) {
            throw "Result mouse interaction smoke could not restore launcher after normal click."
        }
        [FluxWallpaper]::SetForegroundWindow($launcherHandle) | Out-Null
        Start-Sleep -Milliseconds 200
        $foregroundDeadline = (Get-Date).AddSeconds(3)
        while ([FluxWallpaper]::GetForegroundWindow() -ne $launcherHandle -and (Get-Date) -lt $foregroundDeadline) {
            [FluxWallpaper]::SetForegroundWindow($launcherHandle) | Out-Null
            Start-Sleep -Milliseconds 150
        }
        if ([FluxWallpaper]::GetForegroundWindow() -ne $launcherHandle) {
            throw "Result mouse interaction smoke could not focus launcher before Ctrl selection."
        }
        $shell.SendKeys("^a")
        $shell.SendKeys("{BACKSPACE}")
        $shell.SendKeys($resultPointerQuery)
        Start-Sleep -Seconds 2

        # Ctrl-hover is the explicit opt-in path for text selection/copying.
        [FluxWallpaper]::SetForegroundWindow($launcherHandle) | Out-Null
        Start-Sleep -Milliseconds 200
        $foregroundDeadline = (Get-Date).AddSeconds(3)
        while ([FluxWallpaper]::GetForegroundWindow() -ne $launcherHandle -and (Get-Date) -lt $foregroundDeadline) {
            [FluxWallpaper]::SetForegroundWindow($launcherHandle) | Out-Null
            Start-Sleep -Milliseconds 150
        }
        if ([FluxWallpaper]::GetForegroundWindow() -ne $launcherHandle) {
            throw "Result mouse interaction smoke lost launcher focus before Ctrl hover."
        }
        [FluxWallpaper]::keybd_event(0x11, 0, 0, [UIntPtr]::Zero)
        [FluxWallpaper]::SetCursorPos($resultTitleX, $resultTitleY) | Out-Null
        Start-Sleep -Milliseconds 350
        $resultCtrlHoverTextCursor = [FluxWallpaper]::IsTextCursor()
        # Start inside the title glyphs, not the leading icon. Starting at
        # left+55 hits the row anchor and legitimately launches the result.
        $selectStartX = $resultPointerRect.Left + 82
        $selectEndX = $resultPointerRect.Left + 205
        [FluxWallpaper]::SetCursorPos($selectStartX, $resultTitleY) | Out-Null
        [FluxWallpaper]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
        [FluxWallpaper]::SetCursorPos($selectEndX, $resultTitleY) | Out-Null
        Start-Sleep -Milliseconds 250
        [FluxWallpaper]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
        [FluxWallpaper]::SetForegroundWindow($launcherHandle) | Out-Null
        [FluxWallpaper]::SetFocus($launcherHandle) | Out-Null
        Start-Sleep -Milliseconds 200
        if ([FluxWallpaper]::GetForegroundWindow() -ne $launcherHandle) {
            throw "Result mouse interaction smoke lost launcher focus before Ctrl+C."
        }
        # Deliver Ctrl+C as real keyboard input to the focused launcher window;
        # direct WM_KEYDOWN bypasses the normal foreground/input route.
        [FluxWallpaper]::keybd_event(0x43, 0, 0, [UIntPtr]::Zero)
        [FluxWallpaper]::keybd_event(0x43, 0, 2, [UIntPtr]::Zero)
        Start-Sleep -Milliseconds 250
        [FluxWallpaper]::keybd_event(0x11, 0, 2, [UIntPtr]::Zero)
        Start-Sleep -Milliseconds 250
        try {
            $resultClipboardText = (Get-Clipboard -Raw -ErrorAction Stop).Trim()
            $normalizedResultClipboardText = [regex]::Replace(
                $resultClipboardText.ToLowerInvariant(),
                "\s+",
                ""
            )
            $resultCtrlCopyProbe = $resultClipboardText.Length -gt 0 -and
                $normalizedResultClipboardText -eq "resultmouseprobe"
        } catch {
            $resultClipboardText = ""
            $resultCtrlCopyProbe = $false
        }
        Save-Screenshot "result-pointer-ctrl-selection.png"
        $resultCtrlSelectionWindowVisible = [FluxWallpaper]::IsWindowVisible($launcherHandle)
        if (!$resultCtrlSelectionWindowVisible) {
            # Keep the two reproductions independent if the selection probe
            # leaves Flux hidden before the RMB probe begins.
            $restoreDeadline = (Get-Date).AddSeconds(5)
            while (![FluxWallpaper]::IsWindowVisible($launcherHandle) -and (Get-Date) -lt $restoreDeadline) {
                [FluxWallpaper]::SendMessage($launcherHandle, $wmHotkey, [UIntPtr]::Zero, [IntPtr]::Zero) | Out-Null
                Start-Sleep -Milliseconds 650
            }
            if (![FluxWallpaper]::IsWindowVisible($launcherHandle)) {
                throw "Result mouse interaction smoke could not restore launcher after Ctrl selection."
            }
        }
        [FluxWallpaper]::SetForegroundWindow($launcherHandle) | Out-Null
        Start-Sleep -Milliseconds 200
        $shell.SendKeys("^a")
        $shell.SendKeys("{BACKSPACE}")
        $shell.SendKeys($resultPointerQuery)
        Start-Sleep -Seconds 2
        $resultPointerRect = New-Object FluxWallpaper+RECT
        if (![FluxWallpaper]::GetWindowRect($launcherHandle, [ref]$resultPointerRect)) {
            throw "Result mouse interaction smoke could not relocate launcher after Ctrl selection."
        }
        $resultTitleX = $resultPointerRect.Left + 80
        $resultTitleY = $resultPointerRect.Top + 75

        # Right-click the title must open the same in-window action list as
        # Right-arrow. It must not launch the result or show RichText copy UI.
        $resultTraceBeforeCount = if (Test-Path $launchTracePath) { @(Get-Content $launchTracePath).Count } else { 0 }
        [FluxWallpaper]::SetCursorPos($resultTitleX, $resultTitleY) | Out-Null
        Start-Sleep -Milliseconds 250
        [FluxWallpaper]::mouse_event(0x0008, 0, 0, 0, [UIntPtr]::Zero)
        [FluxWallpaper]::mouse_event(0x0010, 0, 0, 0, [UIntPtr]::Zero)
        Start-Sleep -Milliseconds 900
        $resultTraceLines = if (Test-Path $launchTracePath) {
            @(Get-Content $launchTracePath | Select-Object -Skip $resultTraceBeforeCount)
        } else {
            @()
        }
        $resultRmbDispatchObserved = [bool]($resultTraceLines | Where-Object { $_ -match "`tlaunch-dispatch$" } | Select-Object -First 1)
        $resultRmbWindowVisible = [FluxWallpaper]::IsWindowVisible($launcherHandle)
        $resultRmbRect = New-Object FluxWallpaper+RECT
        $resultRmbWindowHeight = if ([FluxWallpaper]::GetWindowRect($launcherHandle, [ref]$resultRmbRect)) {
            [FluxWallpaper]::RectHeight($resultRmbRect)
        } else {
            0
        }
        $resultRmbActionMenuVisible = $resultRmbWindowVisible -and
            $resultRmbWindowHeight -ge 240 -and
            !$resultRmbDispatchObserved
        $resultRmbLaunchProbe = $resultRmbDispatchObserved -and !$resultRmbWindowVisible
        Save-Screenshot "result-pointer-right-click-menu.png"
        [FluxWallpaper]::SetCursorPos($resultTitleX, $resultTitleY) | Out-Null
        [FluxWallpaper]::keybd_event(0x1B, 0, 0, [UIntPtr]::Zero)
        [FluxWallpaper]::keybd_event(0x1B, 0, 2, [UIntPtr]::Zero)
    }

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
        if ($logicalClientHeight -le 56) {
            # The action bar is intentionally hidden in compact Search state.
            # The native telemetry can still contain the previous expanded
            # bounds, so do not validate that stale geometry against a 56-DIP
            # client rectangle.
            Write-Host "Action bar hidden in compact state: client=${logicalClientWidth}x${logicalClientHeight} dpi=$dpi"
            $actionBarProbe = $true
        } else {
            $expectedActionBarX = 10 + [int][Math]::Floor(
                [Math]::Max(0, $logicalClientWidth - 20 - 340) / 2.0
            )
            # Intrinsic layout uses equal vertical insets and can round the centered
            # result by a few DIP. Keep the accepted bottom breathing room narrow so
            # the old weighted-spacer gap cannot return.
            $actionBarBottomInset = $logicalClientHeight - ($actionBarGeometry.Y + $actionBarGeometry.Height)
            $actionBarProbe =
                [Math]::Abs($actionBarGeometry.X - $expectedActionBarX) -le 1 -and
                $actionBarGeometry.Width -eq 340 -and
                $actionBarGeometry.Height -eq 22 -and
                $actionBarBottomInset -ge 14 -and
                $actionBarBottomInset -le 20
            Write-Host "Action bar geometry: x=$($actionBarGeometry.X) y=$($actionBarGeometry.Y) width=$($actionBarGeometry.Width) height=$($actionBarGeometry.Height) bottom_inset=$actionBarBottomInset client=${logicalClientWidth}x${logicalClientHeight} dpi=$dpi"
            if (!$actionBarProbe) {
                throw "Action bar geometry is not centered between launcher insets or has the wrong bottom inset: $($actionBarGeometry | ConvertTo-Json -Compress)."
            }
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
            -FirstPath $emptyScreenshot `
            -SecondPath $reopenScreenshot `
            -X ($reopenRect.Left - [System.Windows.Forms.SystemInformation]::VirtualScreen.Left) `
            -Y ($reopenRect.Top - [System.Windows.Forms.SystemInformation]::VirtualScreen.Top) `
            -Width ($reopenRect.Right - $reopenRect.Left) `
            -Height 56
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
    # 部分主流 runner 上前台窗口虽是 Flux，但键盘焦点尚未落到搜索框，Enter 会被
    # 路由到别处，使 launch-dispatch 晚于 hide-on-deactivate 形成逆序。这里等待
    # 前台线程的 GUI 焦点真正落在启动器窗口上；超时仅跳过等待，不视为失败。
    $enterFocusDeadline = (Get-Date).AddSeconds(1)
    while ((Get-Date) -lt $enterFocusDeadline) {
        $enterFocusInfo = New-Object FluxWallpaper+GUITHREADINFO
        $enterFocusInfo.cbSize = [System.Runtime.InteropServices.Marshal]::SizeOf([type][FluxWallpaper+GUITHREADINFO])
        if ([FluxWallpaper]::GetGUIThreadInfo(0, [ref]$enterFocusInfo) -and
            $enterFocusInfo.hwndFocus -eq $launcherHandle) {
            break
        }
        Start-Sleep -Milliseconds 20
    }
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
    $processCreatedLine = $enterTraceLines | Where-Object { $_ -match "`tprocess-created$" } | Select-Object -First 1
    $launchDispatchTimestamp = if ($launchDispatchLine) { [double]($launchDispatchLine -split "`t", 2)[0] } else { 0.0 }
    $processCreatedTimestamp = if ($processCreatedLine) { [double]($processCreatedLine -split "`t", 2)[0] } else { 0.0 }
    # The launcher hides itself via ctx.hide_window() after dispatching the
    # launch; no `window-deactivated` trace fires in this path because the
    # window is hidden by its own code, not by another window stealing focus.
    # The dispatch probe therefore only requires launch-dispatch to exist after
    # Enter was sent; the hide is separately verified by $enterLaunchHidden.
    $enterLaunchDispatchProbe = $launchDispatchTimestamp -gt 0.0
    $enterProcessCreatedProbe = $processCreatedTimestamp -gt 0.0
    $enterLaunchHidden = ![FluxWallpaper]::IsWindowVisible($launcherHandle)
    $enterHideLatencyProbe =
        $enterLaunchHidden -and
        ($enterHideDispatchMilliseconds -lt $EnterHideDispatchBudgetMilliseconds) -and
        $enterLaunchDispatchProbe
    if (!$enterLaunchHidden) {
        throw "Enter launch did not hide the launcher window."
    }
    if (!$enterHideLatencyProbe) {
        $tracePreview = ($enterTraceLines -join ' | ')
        throw "Enter launch dispatch failed: dispatch_observed=$enterLaunchDispatchProbe, hide_dispatch_ms=$enterHideDispatchMilliseconds, budget_ms=$EnterHideDispatchBudgetMilliseconds, trace=$tracePreview."
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
    $launchProbeTimer = [System.Diagnostics.Stopwatch]::StartNew()
    [FluxWallpaper]::SendMessage($launcherHandle, $wmKeyDown, [UIntPtr]::new(0x0D), [IntPtr]::Zero) | Out-Null
    $launchProbeTimer.Stop()
    $launchProbeHideDispatchMilliseconds = [Math]::Round($launchProbeTimer.Elapsed.TotalMilliseconds, 2)
    # ShellExecuteEx may create the child after the launcher hide callback on a
    # busy CI runner; wait beyond that asynchronous worker boundary before reading
    # the opt-in lifecycle trace. This remains bounded and does not alter the
    # production launch path.
    Start-Sleep -Milliseconds 6000
    $launchProbeAllTraceLines = if (Test-Path $launchTracePath) {
        @(Get-Content $launchTracePath)
    } else {
        @()
    }
    $launchProbeDispatchIndexes = @(
        for ($traceIndex = 0; $traceIndex -lt $launchProbeAllTraceLines.Count; $traceIndex++) {
            if ($launchProbeAllTraceLines[$traceIndex] -match "`tlaunch-dispatch$") {
                $traceIndex
            }
        }
    )
    $launchProbeTraceLines = if ($launchProbeDispatchIndexes.Count -gt 0) {
        @($launchProbeAllTraceLines | Select-Object -Skip $launchProbeDispatchIndexes[-1])
    } else {
        @()
    }
    $launchProbeDispatchLine = $launchProbeTraceLines | Where-Object { $_ -match "`tlaunch-dispatch$" } | Select-Object -First 1
    $launchProbeProcessLine = $launchProbeTraceLines | Where-Object { $_ -match "`tprocess-created$" } | Select-Object -First 1
    $launchProbeShellReturnLine = $launchProbeTraceLines | Where-Object { $_ -match "`tshell-return$" } | Select-Object -First 1
    $launchProbeDispatchTimestamp = if ($launchProbeDispatchLine) { [double]($launchProbeDispatchLine -split "`t", 2)[0] } else { 0.0 }
    $launchProbeProcessTimestamp = if ($launchProbeProcessLine) { [double]($launchProbeProcessLine -split "`t", 2)[0] } else { 0.0 }
    $launchProbeShellReturnTimestamp = if ($launchProbeShellReturnLine) { [double]($launchProbeShellReturnLine -split "`t", 2)[0] } else { 0.0 }
    $launchProbeCompletionTimestamp = if ($launchProbeProcessTimestamp -gt 0.0) {
        $launchProbeProcessTimestamp
    } else {
        $launchProbeShellReturnTimestamp
    }
    # This path also hides the launcher through its own ctx.hide_window() call, so
    # no `window-deactivated` trace is emitted here either; the previous
    # dispatch-before-hide ordering assertion could never be satisfied. The probe
    # now requires the dispatch to be observed and the launch to complete, while
    # window_hidden only reports the observed hide state.
    $launchProbeDispatchObserved = $launchProbeDispatchTimestamp -gt 0.0
    $launchProbeLaunchSucceeded = $launchProbeCompletionTimestamp -gt 0.0
    $launchProbeWindowHidden = ![FluxWallpaper]::IsWindowVisible($launcherHandle)
    $launchProbeProcessCreationMilliseconds = if ($launchProbeLaunchSucceeded) {
        [Math]::Round($launchProbeCompletionTimestamp - $launchProbeDispatchTimestamp, 3)
    } else {
        0.0
    }
    # Hiding is intentionally synchronous, while ShellExecuteEx runs on Flux's
    # launch worker, so process-created may legitimately arrive after the hide.
    $launchProcessCreationProbe =
        $launchProbeDispatchObserved -and $launchProbeLaunchSucceeded
    if (!$launchProcessCreationProbe) {
        throw "Launch process probe failed: dispatch_observed=$launchProbeDispatchObserved, launch_succeeded=$launchProbeLaunchSucceeded, process_created=$($launchProbeProcessTimestamp -gt 0.0), shell_return=$($launchProbeShellReturnTimestamp -gt 0.0), window_hidden=$launchProbeWindowHidden, hide_dispatch_ms=$launchProbeHideDispatchMilliseconds."
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
        [FluxWallpaper]::SetForegroundWindow($launcherHandle) | Out-Null
        [FluxWallpaper]::keybd_event(0x11, 0, 0, [UIntPtr]::Zero)
        [FluxWallpaper]::keybd_event(0x10, 0, 0, [UIntPtr]::Zero)
        Start-Sleep -Milliseconds 120
        [FluxWallpaper]::keybd_event(0x43, 0, 0, [UIntPtr]::Zero)
        Start-Sleep -Milliseconds 120
        [FluxWallpaper]::keybd_event(0x43, 0, 2, [UIntPtr]::Zero)
        [FluxWallpaper]::keybd_event(0x10, 0, 2, [UIntPtr]::Zero)
        [FluxWallpaper]::keybd_event(0x11, 0, 2, [UIntPtr]::Zero)
        Start-Sleep -Milliseconds 450
        try {
            $dropPaths = @([System.Windows.Forms.Clipboard]::GetFileDropList() | ForEach-Object { $_.ToString() })
            Write-Host "Ctrl+Shift+C file drop list: $($dropPaths -join '; ')"
            $resultCtrlShiftCopyProbe = $dropPaths.Count -gt 0 -and ($dropPaths | Where-Object { [System.IO.Path]::IsPathFullyQualified($_) }).Count -gt 0
        } catch {
            Write-Host "Ctrl+Shift+C clipboard read failed: $($_.Exception.Message)"
            $resultCtrlShiftCopyProbe = $false
        }
        if (!$resultCtrlShiftCopyProbe) {
            throw "Ctrl+Shift+C did not copy a Windows file object."
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
    $historyMemory = Get-MemorySnapshot $process
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
        # 托盘设置烟雾测试走真实托盘菜单触发，不再依赖任何环境变量钩子；
        # 子进程保持隐藏启动，唯一的显示路径就是托盘菜单里的 Settings。
        Remove-Item Env:FLUX_OPEN_SETTINGS -ErrorAction SilentlyContinue
    } else {
        $env:FLUX_OPEN_SETTINGS = "1"
    }
    if ($VisualSettingsSmoke) {
        $env:FLUX_SMOKE_SETTINGS_UI = "1"
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
    }
    if ($TraySettingsSmoke) {
        # The Settings child must become an independent instance even without the
        # visual smoke: it starts hidden like the tray-resident launcher, so the
        # real tray menu is the only path that makes it visible.
        $env:FLUX_DISABLE_SINGLE_INSTANCE = "1"
    }
    $settingsStdoutPath = Join-Path $OutputDirectory "settings.stdout.log"
    $settingsStderrPath = Join-Path $OutputDirectory "settings.stderr.log"
    $settingsArguments = @()
    if ($TraySettingsSmoke) {
        # 托盘设置烟雾测试：子进程以 --startup 隐藏启动，等同常驻托盘的启动器。
        # 唯一的显示路径是真实托盘菜单里的 Settings，其 on_window_show 回调会在
        # 首帧呈现前应用完整设置尺寸，因此不会闪出紧凑搜索条。
        $settingsArguments = @("--startup")
    }
    $settingsProcess = Start-Process -FilePath $Executable -ArgumentList $settingsArguments -PassThru -RedirectStandardOutput $settingsStdoutPath -RedirectStandardError $settingsStderrPath
    $settingsWindowHeight = 0
    $settingsWindowWidth = 0
    $settingsWindowFound = $false
    $settingsUiProbe = $false
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
            # 真实托盘生命周期：子进程以 --startup 隐藏启动（等同常驻托盘的启动器），
            # 唯一的显示路径是托盘右键菜单里的 Settings。这里同时锁定第 4 节不变量
            # （首个可见帧必须是完整设置面板，不得先闪紧凑搜索条）与实际尺寸。
            $settingsHandle = Get-LauncherWindowHandle $settingsProcess
            if ($settingsHandle -eq [IntPtr]::Zero) {
                throw "Tray Settings smoke could not resolve the hidden launcher HWND."
            }
            if ([FluxWallpaper]::IsWindowVisible($settingsHandle)) {
                throw "Tray Settings smoke expected the launcher child to start hidden."
            }
            $probeProcess.Refresh()
            $trayAnchorHandle = $probeProcess.MainWindowHandle
            if ($trayAnchorHandle -eq [IntPtr]::Zero) {
                $trayAnchorHandle = [FluxWallpaper]::FindVisibleWindowByProcessId([uint32]$probeProcess.Id)
            }
            if ($trayAnchorHandle -eq [IntPtr]::Zero) {
                throw "Tray Settings smoke could not find the deterministic probe window."
            }
            $trayAnchorRect = New-Object FluxWallpaper+RECT
            if (![FluxWallpaper]::GetWindowRect($trayAnchorHandle, [ref]$trayAnchorRect)) {
                throw "Tray Settings smoke could not read the probe window rectangle."
            }
            $trayAnchorX = [int](($trayAnchorRect.Left + $trayAnchorRect.Right) / 2)
            $trayAnchorY = [int](($trayAnchorRect.Top + $trayAnchorRect.Bottom) / 2)
            $traySettingsAttempt = Invoke-FluxTraySettingsClick `
                -LauncherHandle $settingsHandle -MenuX $trayAnchorX -MenuY $trayAnchorY `
                -Context "Tray Settings"
            if (!$traySettingsAttempt.FirstVisible) {
                throw "Tray Settings smoke could not observe the first Settings frame."
            }
            $firstFrameWidth = $traySettingsAttempt.FirstWidth
            $firstFrameHeight = $traySettingsAttempt.FirstHeight
            Write-Host "Tray Settings first-frame geometry: ${firstFrameWidth}x${firstFrameHeight}"
            if ($firstFrameHeight -lt 480 -or $firstFrameWidth -lt 680) {
                throw "Tray Settings first frame was undersized: ${firstFrameWidth}x${firstFrameHeight}."
            }
            if (!$traySettingsAttempt.Full) {
                throw "Tray Settings smoke observed an incomplete Settings panel: $($traySettingsAttempt.Width)x$($traySettingsAttempt.Height)."
            }
        }
        Start-Sleep -Seconds 2
        $settingsProcess.Refresh()
        if ($VisualSettingsSmoke -and $settingsProcess.HasExited) {
            throw "Visual Settings smoke process exited before creating its Settings window with code $($settingsProcess.ExitCode)."
        }
        $settingsContractLog = if (Test-Path $settingsStderrPath) { Get-Content $settingsStderrPath -Raw } else { "" }
        if ($VisualSettingsSmoke) {
            # 版本可能携带 SemVer 预发布标识（如 0.2.0-beta.1），正则需接受可选后缀。
            $settingsUiProbe = $settingsContractLog -match "Settings UI contract: UpdateActionVersionLabel=Current version: \d+\.\d+\.\d+(-[0-9A-Za-z.\-]+)?; SmoothCaretTab=Visual; SmoothCaretGeneral=false"
            if (!$settingsUiProbe) {
                throw "Visual Settings smoke did not observe the expected Update version/Smooth Caret tab contract."
            }
        }
        if ($VisualSettingsSmoke) {
            $settingsHwnd = Get-LauncherWindowHandle $settingsProcess
        } elseif ($TraySettingsSmoke) {
            # The tray-driven child already exposed its HWND for the first-frame
            # probe; reuse it instead of consulting a possibly stale MainWindowHandle.
            $settingsHwnd = $settingsHandle
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
            $directSliderY = $settingsRect.Top + [int][Math]::Round(360 * $settingsScale)
            $directPointClass = [FluxWallpaper]::WindowClassAtPoint($directSliderLeft, $directSliderY)
            Write-Host "Visual slider direct probe: left=$directSliderLeft right=$directSliderRight y=$directSliderY windowClass=$directPointClass"
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

            $heightDirectY = $settingsRect.Top + [int][Math]::Round(438 * $settingsScale)
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
            # The reopen child must open the panel on startup, so FLUX_OPEN_SETTINGS has to
            # be set again: the tray-settings run deliberately unsets it for the hidden tray
            # child above. Environment changes only affect processes started from now on.
            $env:FLUX_OPEN_SETTINGS = "1"
            $reopenStdoutPath = Join-Path $OutputDirectory "settings-reopen.stdout.log"
            $reopenStderrPath = Join-Path $OutputDirectory "settings-reopen.stderr.log"
            $reopenedSettingsProcess = Start-Process -FilePath $Executable -PassThru -RedirectStandardOutput $reopenStdoutPath -RedirectStandardError $reopenStderrPath
            Start-Sleep -Seconds 2
            $reopenedSettingsHwnd = Get-LauncherWindowHandle $reopenedSettingsProcess
            $reopenedSettingsRect = New-Object FluxWallpaper+RECT
            if (![FluxWallpaper]::GetWindowRect($reopenedSettingsHwnd, [ref]$reopenedSettingsRect)) {
                throw "Visual persistence smoke could not find reopened Settings HWND."
            }
            # The preview child is announced after the reopened process boots D2D, opens the
            # panel, and spawns the child; poll instead of assuming one fixed delay suffices.
            $reopenPidMatch = $null
            for ($attempt = 0; $attempt -lt 150 -and $null -eq $reopenPidMatch; $attempt++) {
                $reopenLog = if (Test-Path $reopenStderrPath) { Get-Content $reopenStderrPath -Raw } else { "" }
                $reopenPidMatch = [regex]::Match($reopenLog, "Visual preview process started: pid=(\d+)")
                if (!$reopenPidMatch.Success) {
                    $reopenPidMatch = $null
                    Start-Sleep -Milliseconds 100
                }
            }
            if ($null -eq $reopenPidMatch) {
                throw "Visual persistence smoke did not observe a preview child after reopening Settings."
            }
            $reopenedPreviewProcessId = [int]$reopenPidMatch.Groups[1].Value
            $reopenGeometryMatch = $null
            for ($attempt = 0; $attempt -lt 30 -and $null -eq $reopenGeometryMatch; $attempt++) {
                $reopenLog = Get-Content $reopenStderrPath -Raw
                $reopenGeometryMatches = [regex]::Matches(
                    $reopenLog,
                    "VisualPreviewChild: GEOMETRY $reopenedPreviewProcessId (\d+) (\d+) (\d+) (\d+) (\d+)"
                )
                if ($reopenGeometryMatches.Count -gt 0) { $reopenGeometryMatch = $reopenGeometryMatches[$reopenGeometryMatches.Count - 1] }
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
        Remove-Item Env:FLUX_SMOKE_SETTINGS_UI -ErrorAction SilentlyContinue
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
        ResultMouseInteractionSmoke = [bool]$ResultMouseInteractionSmoke
        ResultRightClickDispatchObserved = $resultRmbDispatchObserved
        ResultRightClickWindowVisible = $resultRmbWindowVisible
        ResultRightClickWindowHeight = $resultRmbWindowHeight
        ResultRightClickActionMenuVisible = (!$ResultMouseInteractionSmoke) -or $resultRmbActionMenuVisible
        ResultRightClickWindowHidden = $resultRmbWindowHidden
        ResultRightClickLaunchProbe = $resultRmbLaunchProbe
        ResultRightArrowMiddleCaretActionMenu = $resultRightArrowMiddleCaretActionMenu
        ResultRightArrowMiddleCaretProbe = (!$ResultMouseInteractionSmoke) -or $resultRightArrowMiddleCaretProbe
        ResultNormalHoverTextCursor = $resultNormalHoverTextCursor
        ResultNormalHoverCopyDisabledProbe = (!$ResultMouseInteractionSmoke) -or $resultNormalHoverCopyDisabledProbe
        ResultNormalClickDispatchObserved = $resultNormalClickDispatchObserved
        ResultNormalClickWindowHidden = $resultNormalClickWindowHidden
        ResultNormalClickLaunchProbe = (!$ResultMouseInteractionSmoke) -or $resultNormalClickLaunchProbe
        ResultCtrlHoverTextCursor = $resultCtrlHoverTextCursor
        ResultCtrlSelectionWindowVisible = $resultCtrlSelectionWindowVisible
        ResultCtrlClipboardText = $resultClipboardText
        ResultCtrlCopyProbe = (!$ResultMouseInteractionSmoke) -or $resultCtrlCopyProbe
        ScrollbarGapProbe = (!$ScrollbarGapSmoke) -or $scrollbarGapProbe
        ActionBarProbe = (!$ActionBarSmoke) -or $actionBarProbe
        ActionBarGeometry = $actionBarGeometry
        QueryClearOnReopenProbe = (!$QueryClearOnReopenSmoke) -or $queryClearOnReopenProbe
        CtrlRProbe = (!$CtrlRSmoke) -or $ctrlRProbe
        CtrlCProbe = (!$CtrlCSmoke) -or $ctrlCProbe
        CtrlShiftCopyProbe = (!$CtrlCSmoke) -or $resultCtrlShiftCopyProbe
        TabNavigationProbe = $TabNavigationCycles -gt 0
        EverythingSyntaxProbe = $true
        QueryResponsivenessProbe = (!$QueryResponsivenessSmoke) -or $queryResponsivenessProbe
        FirstKeystrokeProbe = if ($ImeMessageSmoke) { $firstKeystrokeProbe } else { $true }
        ImeMessageRoutingProbe = $imeMessageProbe
        ImeMessageDetails = $imeMessageDetails
        InputTraceCollected = $ImeMessageSmoke -and (Test-Path $inputTracePath)
        CommandPriorityProbe = (!$CommandPrioritySmoke) -or $commandPriorityProbe
        CompactApplicationProbe = (!$CommandPrioritySmoke) -or $compactAppProbe
        CompactApplicationProbeLine = $compactAppProbeLine
        PowerShellDedupeProbe = (!$PowerShellSmoke) -or $powerShellDedupeProbe
        PowerShellIconProbe = (!$PowerShellSmoke) -or $powerShellIconProbe
        CalculatorProbe = (!$CalculatorSmoke) -or $calculatorProbe
        CalculatorPolicyProbe = (!$CalculatorPolicySmoke) -or $calculatorPolicyProbe
        ObsidianIconProbe = (!$ObsidianIconSmoke) -or $obsidianIconProbe
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
        DeactivationIdleMemory = $deactivationIdleMemory
        HistoryPanelProbe = $true
        HistoryUpProbe = $true
        HistoryAltUpProbe = $true
        HistoryAltDownProbe = $true
        SettingsOpenPath = if ($TraySettingsSmoke) { "tray-lifecycle" } else { "startup-env" }
        SettingsWindowFound = $settingsWindowFound
        SettingsWindowHeight = $settingsWindowHeight
        SettingsWindowWidth = $settingsWindowWidth
        SettingsPanelProbe = $settingsWindowFound -and ($settingsWindowHeight -ge 400) -and ($settingsWindowWidth -ge 680)
        SettingsUiContractProbe = (!$VisualSettingsSmoke) -or $settingsUiProbe
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
        TraySettingsAfterDeactivationProbe = (!$TraySettingsAfterDeactivationSmoke) -or $traySettingsAfterDeactivationProbe
        TraySettingsAfterDeactivationWindowWidth = $traySettingsAfterDeactivationWindowWidth
        TraySettingsAfterDeactivationWindowHeight = $traySettingsAfterDeactivationWindowHeight
        KeyboardSelectionProbe = $true
        ActionModeProbe = $true
        EnterActionProbe = $true
        EnterLaunchHideProbe = $enterLaunchHidden
        EnterHideDispatchMilliseconds = $enterHideDispatchMilliseconds
        EnterHideLatencyProbe = $enterHideLatencyProbe
        EnterLaunchDispatchProbe = $enterLaunchDispatchProbe
        EnterProcessCreatedProbe = $enterProcessCreatedProbe
        EnterLaunchDispatchTimestamp = $launchDispatchTimestamp
        EnterProcessCreatedTimestamp = $processCreatedTimestamp
        EnterHideQuery = $enterHideQuery
        LaunchProcessCreationProbe = $launchProcessCreationProbe
        LaunchProbeQuery = $launchProbeQuery
        LaunchProbeDispatchObserved = $launchProbeDispatchObserved
        LaunchProbeWindowHidden = $launchProbeWindowHidden
        LaunchProbeProcessCreationMilliseconds = $launchProbeProcessCreationMilliseconds
        LaunchProbeHideDispatchMilliseconds = $launchProbeHideDispatchMilliseconds
        ResourceProfileProbe = $resourceProfileProbe
        ResourceProfileSamples = @($resourceProfileSamples)
        ResourceProfile = $resourceProfileSummary
        Memory = [ordered]@{
            Idle = $idleMemory
            Query = $queryMemory
            HistoryPanel = $historyMemory
            HiddenIdleAfterDeactivation = $deactivationIdleMemory
        }
    } | ConvertTo-Json | Set-Content -Encoding utf8 (Join-Path $OutputDirectory "environment.json")
    if ($ResultMouseInteractionSmoke -and (!$resultRmbActionMenuVisible -or $resultRmbLaunchProbe -or !$resultNormalClickLaunchProbe -or !$resultCtrlCopyProbe -or !$resultCtrlShiftCopyProbe)) {
        throw "Result mouse interaction smoke failed: right_click_action_menu=$resultRmbActionMenuVisible, right_click_launch=$resultRmbLaunchProbe, normal_click_launch=$resultNormalClickLaunchProbe, ctrl_copy=$resultCtrlCopyProbe, ctrl_shift_copy=$resultCtrlShiftCopyProbe."
    }
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
    if (Test-Path $compactFixtureTarget) {
        Remove-Item -Force $compactFixtureTarget
    }
    if ($ObsidianIconSmoke -and (Test-Path $obsidianConfigRoot)) {
        Remove-Item $obsidianConfigRoot -Recurse -Force -ErrorAction SilentlyContinue
    }
    if (Test-Path $obsidianFixtureVaultRoot) {
        Remove-Item $obsidianFixtureVaultRoot -Recurse -Force -ErrorAction SilentlyContinue
    }
    if ($obsidianConfigWasBackedUp -and (Test-Path $obsidianConfigBackupRoot)) {
        Move-Item -LiteralPath $obsidianConfigBackupRoot -Destination $obsidianConfigRoot -Force -ErrorAction SilentlyContinue
    }
    if ($legacyFlowPluginWasDisabled -and (Test-Path $legacyFlowPluginBackupRoot)) {
        Move-Item -LiteralPath $legacyFlowPluginBackupRoot -Destination $legacyFlowPluginRoot -Force -ErrorAction SilentlyContinue
    }
    Remove-Item Env:FLUX_LAUNCH_TRACE_FILE -ErrorAction SilentlyContinue
    Remove-Item Env:FLUX_INPUT_TRACE_FILE -ErrorAction SilentlyContinue
    Remove-Item Env:FLUX_COMPACT_APP_PROBE_FILE -ErrorAction SilentlyContinue
    Remove-Item Env:FLUX_ICON_PROBE_FILE -ErrorAction SilentlyContinue
    Remove-Item Env:FLUX_QUERY_PROBE_FILE -ErrorAction SilentlyContinue
}

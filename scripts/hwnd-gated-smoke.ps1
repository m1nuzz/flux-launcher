param(
    [Parameter(Mandatory = $true)]
    [string]$Executable,
    [Parameter(Mandatory = $true)]
    [string]$OutputDirectory,
    [string]$Query = 'grok'
)

$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class HwndGatedSmoke {
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr hWnd, IntPtr insertAfter, int x, int y, int cx, int cy, uint flags);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int command);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern IntPtr SetFocus(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern IntPtr GetFocus();
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint processId);
    [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
    [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint attach, uint attachTo, bool attachInput);
    [DllImport("user32.dll")] public static extern uint SendInput(uint count, INPUT[] inputs, int size);

    [StructLayout(LayoutKind.Sequential)] public struct RECT {
        public int Left; public int Top; public int Right; public int Bottom;
    }
    [StructLayout(LayoutKind.Sequential)] public struct KEYBDINPUT {
        public ushort wVk; public ushort wScan; public uint dwFlags; public uint time; public UIntPtr dwExtraInfo;
    }
    [StructLayout(LayoutKind.Explicit, Size = 40)] public struct INPUT {
        [FieldOffset(0)] public uint type;
        [FieldOffset(8)] public KEYBDINPUT ki;
    }
}
"@

function Focus-Window([IntPtr]$handle) {
    [uint32]$targetPid = 0
    $targetThread = [HwndGatedSmoke]::GetWindowThreadProcessId($handle, [ref]$targetPid)
    $currentThread = [HwndGatedSmoke]::GetCurrentThreadId()
    $foreground = [HwndGatedSmoke]::GetForegroundWindow()
    [uint32]$foregroundPid = 0
    $foregroundThread = if ($foreground -ne [IntPtr]::Zero) {
        [HwndGatedSmoke]::GetWindowThreadProcessId($foreground, [ref]$foregroundPid)
    } else { 0 }
    if ($foregroundThread -ne 0 -and $foregroundThread -ne $currentThread) {
        [HwndGatedSmoke]::AttachThreadInput($currentThread, $foregroundThread, $true) | Out-Null
    }
    if ($targetThread -ne 0 -and $targetThread -ne $currentThread) {
        [HwndGatedSmoke]::AttachThreadInput($currentThread, $targetThread, $true) | Out-Null
    }
    [HwndGatedSmoke]::SetForegroundWindow($handle) | Out-Null
    [HwndGatedSmoke]::SetFocus($handle) | Out-Null
    if ($targetThread -ne 0 -and $targetThread -ne $currentThread) {
        [HwndGatedSmoke]::AttachThreadInput($currentThread, $targetThread, $false) | Out-Null
    }
    if ($foregroundThread -ne 0 -and $foregroundThread -ne $currentThread) {
        [HwndGatedSmoke]::AttachThreadInput($currentThread, $foregroundThread, $false) | Out-Null
    }
}

function Send-Vk([uint16]$vk) {
    $down = [HwndGatedSmoke+INPUT]::new()
    $down.type = 1
    $down.ki.wVk = $vk
    $up = [HwndGatedSmoke+INPUT]::new()
    $up.type = 1
    $up.ki.wVk = $vk
    $up.ki.dwFlags = 2
    $inputs = [HwndGatedSmoke+INPUT[]]@($down, $up)
    $sent = [HwndGatedSmoke]::SendInput(2, $inputs, [Runtime.InteropServices.Marshal]::SizeOf([type][HwndGatedSmoke+INPUT]))
    if ($sent -ne 2) { throw "SendInput VK $vk sent $sent/2 events" }
}

function Send-UnicodeText([string]$text) {
    foreach ($character in $text.ToCharArray()) {
        $down = [HwndGatedSmoke+INPUT]::new()
        $down.type = 1
        $down.ki.wScan = [uint16][char]$character
        $down.ki.dwFlags = 4
        $up = [HwndGatedSmoke+INPUT]::new()
        $up.type = 1
        $up.ki.wScan = [uint16][char]$character
        $up.ki.dwFlags = 6
        $inputs = [HwndGatedSmoke+INPUT[]]@($down, $up)
        $sent = [HwndGatedSmoke]::SendInput(2, $inputs, [Runtime.InteropServices.Marshal]::SizeOf([type][HwndGatedSmoke+INPUT]))
        if ($sent -ne 2) { throw "SendInput Unicode '$character' sent $sent/2 events" }
    }
}

function Save-Screen([string]$name, $screen) {
    $bounds = $screen.Bounds
    $bitmap = New-Object System.Drawing.Bitmap $bounds.Width, $bounds.Height
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    try {
        $graphics.CopyFromScreen($bounds.Left, $bounds.Top, 0, 0, $bounds.Size)
        $bitmap.Save((Join-Path $OutputDirectory $name), [System.Drawing.Imaging.ImageFormat]::Png)
    } finally {
        $graphics.Dispose()
        $bitmap.Dispose()
    }
}

$screen = [System.Windows.Forms.Screen]::AllScreens | Where-Object { -not $_.Primary } | Select-Object -First 1
if ($null -eq $screen) { throw 'No secondary monitor available.' }
$bounds = $screen.Bounds
$area = $screen.WorkingArea
$x = $area.X + [int](($area.Width - 420) / 2)
$y = $area.Y + [int](($area.Height - 250) / 2)
$previousForeground = [HwndGatedSmoke]::GetForegroundWindow()
$process = $null
$previousSmokeEnv = $env:FLUX_SMOKE_DISPLAY2
$env:FLUX_SMOKE_DISPLAY2 = '1'
try {
    $process = Start-Process -FilePath $Executable -WorkingDirectory (Split-Path -Parent $Executable) -WindowStyle Hidden -PassThru
    $handle = [IntPtr]::Zero
    for ($attempt = 0; $attempt -lt 100; $attempt++) {
        Start-Sleep -Milliseconds 100
        $process.Refresh()
        $handle = $process.MainWindowHandle
        if ($handle -ne [IntPtr]::Zero) { break }
    }
    if ($handle -eq [IntPtr]::Zero) { throw 'Flux HWND was not created.' }

    # Move and size while hidden. The first visible state is therefore already on DISPLAY2.
    $swpNoZOrderNoActivate = 0x0014
    [HwndGatedSmoke]::SetWindowPos($handle, [IntPtr]::Zero, $x, $y, 420, 250, $swpNoZOrderNoActivate) | Out-Null
    [HwndGatedSmoke]::ShowWindow($handle, 5) | Out-Null
    Focus-Window $handle
    Start-Sleep -Milliseconds 250

    [HwndGatedSmoke+RECT]$rect = [HwndGatedSmoke+RECT]::new()
    [HwndGatedSmoke]::GetWindowRect($handle, [ref]$rect) | Out-Null
    $foreground = [HwndGatedSmoke]::GetForegroundWindow()
    if ($rect.Left -lt $bounds.Left -or $rect.Right -gt $bounds.Right -or $rect.Top -lt $bounds.Top -or $rect.Bottom -gt $bounds.Bottom) { throw "Launcher rect is outside DISPLAY2 bounds $($bounds): $($rect.Left),$($rect.Top),$($rect.Right),$($rect.Bottom)" }
    if ($foreground -ne $handle) { throw "Launcher is not foreground after DISPLAY2 show: hwnd=$handle foreground=$foreground" }

    $metadata = [ordered]@{
        Query = $Query
        Display = $screen.DeviceName
        Bounds = $screen.Bounds.ToString()
        LauncherRect = "$($rect.Left),$($rect.Top),$($rect.Right),$($rect.Bottom)"
        ForegroundMatches = ($foreground -eq $handle)
        FirstVisibleDisplay = $screen.DeviceName
        MouseClickUsed = $false
    }
    $metadata | ConvertTo-Json | Set-Content -Encoding utf8 (Join-Path $OutputDirectory 'hwnd-gated-result.json')
    Save-Screen 'hwnd-gated-empty.png' $screen

    # WM_KEYDOWN Tab is routed by windui to the first focusable control.
    Send-Vk 0x09
    Start-Sleep -Milliseconds 150
    Send-UnicodeText $Query
    Start-Sleep -Seconds 2
    Save-Screen 'hwnd-gated-grok.png' $screen
    [ordered]@{
        Query = $Query
        EmptyHash = (Get-FileHash (Join-Path $OutputDirectory 'hwnd-gated-empty.png') -Algorithm SHA256).Hash
        QueryHash = (Get-FileHash (Join-Path $OutputDirectory 'hwnd-gated-grok.png') -Algorithm SHA256).Hash
        HashesDiffer = ((Get-FileHash (Join-Path $OutputDirectory 'hwnd-gated-empty.png') -Algorithm SHA256).Hash -ne (Get-FileHash (Join-Path $OutputDirectory 'hwnd-gated-grok.png') -Algorithm SHA256).Hash)
    } | ConvertTo-Json | Set-Content -Encoding utf8 (Join-Path $OutputDirectory 'hwnd-gated-input-result.json')
} finally {
    if ($process -and !$process.HasExited) { Stop-Process -Id $process.Id -Force }
    if ($previousForeground -ne [IntPtr]::Zero) { [HwndGatedSmoke]::SetForegroundWindow($previousForeground) | Out-Null }
    $env:FLUX_SMOKE_DISPLAY2 = $previousSmokeEnv
}
Write-Output "HWND-gated smoke complete: $OutputDirectory"

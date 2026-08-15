param(
    [Parameter(Mandatory = $true)]
    [string]$Executable,

    [Parameter(Mandatory = $true)]
    [string]$OutputDirectory
)

$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class FluxSecondarySmoke {
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr z, int x, int y, int w, int height, uint flags);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
}
'@

function Save-Screen([string]$name, $screen) {
    $bounds = $screen.Bounds
    $bitmap = New-Object System.Drawing.Bitmap $bounds.Width, $bounds.Height
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    try {
        $graphics.CopyFromScreen($bounds.Left, $bounds.Top, 0, 0, $bounds.Size)
        $bitmap.Save((Join-Path $OutputDirectory $name), [System.Drawing.Imaging.ImageFormat]::Png)
    }
    finally {
        $graphics.Dispose()
        $bitmap.Dispose()
    }
}

$screen = [System.Windows.Forms.Screen]::AllScreens | Where-Object { -not $_.Primary } | Select-Object -First 1
if ($null -eq $screen) { throw 'No secondary monitor is available.' }
$area = $screen.WorkingArea
$x = $area.X + [int](($area.Width - 420) / 2)
$y = $area.Y + [int](($area.Height - 400) / 2)
[ordered]@{
    DeviceName = $screen.DeviceName
    Bounds = $screen.Bounds.ToString()
    WorkingArea = $area.ToString()
    LauncherX = $x
    LauncherY = $y
    PrimaryWasProtected = $true
} | ConvertTo-Json | Set-Content -Encoding utf8 (Join-Path $OutputDirectory 'monitor-placement.json')

$previousForeground = [FluxSecondarySmoke]::GetForegroundWindow()
$process = Start-Process -FilePath $Executable -WorkingDirectory (Split-Path -Parent $Executable) -PassThru
try {
    Start-Sleep -Seconds 3
    $handle = (Get-Process -Id $process.Id).MainWindowHandle
    if ($handle -eq [IntPtr]::Zero) { throw 'Flux Launcher window handle is zero.' }
    [FluxSecondarySmoke]::SetWindowPos($handle, [IntPtr]::Zero, $x, $y, 420, 400, 0x40) | Out-Null
    [FluxSecondarySmoke]::SetForegroundWindow($handle) | Out-Null
    Start-Sleep -Milliseconds 400
    [FluxSecondarySmoke]::SetCursorPos($x + 210, $y + 36) | Out-Null
    [FluxSecondarySmoke]::mouse_event(2, 0, 0, 0, [UIntPtr]::Zero)
    [FluxSecondarySmoke]::mouse_event(4, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 250
    (New-Object -ComObject WScript.Shell).SendKeys('steam')
    Start-Sleep -Seconds 2
    Save-Screen 'windows11-steam-before-down.png' $screen
    (New-Object -ComObject WScript.Shell).SendKeys('{DOWN}')
    Start-Sleep -Milliseconds 350
    Save-Screen 'windows11-steam-after-down.png' $screen
    [ordered]@{ Query = 'steam'; WindowHandle = $handle.ToInt64(); ProcessId = $process.Id; CapturedOn = 'secondary monitor only' } | ConvertTo-Json | Set-Content -Encoding utf8 (Join-Path $OutputDirectory 'smoke-result.json')
}
finally {
    if ($process -and !$process.HasExited) { Stop-Process -Id $process.Id -Force }
    if ($previousForeground -ne [IntPtr]::Zero) { [FluxSecondarySmoke]::SetForegroundWindow($previousForeground) | Out-Null }
}
Write-Output "Windows 11 secondary-monitor smoke complete: $OutputDirectory"

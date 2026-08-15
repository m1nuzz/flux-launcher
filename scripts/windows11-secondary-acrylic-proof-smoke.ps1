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
public static class FluxAcrylicProof {
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr z, int x, int y, int w, int height, uint flags);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool BringWindowToTop(IntPtr h);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int command);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
    [DllImport("user32.dll")] public static extern void keybd_event(byte virtualKey, byte scanCode, uint flags, UIntPtr extra);
}
'@

function Send-VirtualKey([byte]$virtualKey, [byte]$scanCode = 0, [bool]$extended = $false) {
    $flags = if ($extended) { 1 } else { 0 }
    [FluxAcrylicProof]::keybd_event($virtualKey, $scanCode, $flags, [UIntPtr]::Zero)
    [FluxAcrylicProof]::keybd_event($virtualKey, $scanCode, ($flags -bor 2), [UIntPtr]::Zero)
}

function Send-AsciiText([string]$text) {
    foreach ($character in $text.ToUpperInvariant().ToCharArray()) {
        Send-VirtualKey ([byte][char]$character)
    }
}

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
$bounds = $screen.Bounds
$area = $screen.WorkingArea
$x = $area.X + [int](($area.Width - 420) / 2)
$y = $area.Y + [int](($area.Height - 400) / 2)

# This is only a deterministic test fixture behind Flux. It is not part of the app UI.
# Large color blocks make Acrylic bleed measurable without relying on the user's wallpaper.
$fixture = New-Object System.Windows.Forms.Form
$fixture.FormBorderStyle = [System.Windows.Forms.FormBorderStyle]::None
$fixture.StartPosition = [System.Windows.Forms.FormStartPosition]::Manual
$fixture.Location = New-Object System.Drawing.Point($bounds.X, $bounds.Y)
$fixture.Size = New-Object System.Drawing.Size($bounds.Width, $bounds.Height)
$fixture.ShowInTaskbar = $false
$fixture.TopMost = $false
$fixture.BackColor = [System.Drawing.Color]::FromArgb(8, 12, 24)
$fixture.Add_Paint({
    param($sender, $eventArgs)
    $g = $eventArgs.Graphics
    $w = $sender.ClientSize.Width
    $h = $sender.ClientSize.Height
    $brushes = @(
        [System.Drawing.SolidBrush]::new([System.Drawing.Color]::FromArgb(20, 92, 170)),
        [System.Drawing.SolidBrush]::new([System.Drawing.Color]::FromArgb(88, 42, 142)),
        [System.Drawing.SolidBrush]::new([System.Drawing.Color]::FromArgb(18, 124, 108)),
        [System.Drawing.SolidBrush]::new([System.Drawing.Color]::FromArgb(132, 66, 34))
    )
    try {
        for ($i = 0; $i -lt $brushes.Count; $i++) {
            $left = [int](($w / $brushes.Count) * $i)
            $right = [int](($w / $brushes.Count) * ($i + 1))
            $g.FillRectangle($brushes[$i], $left, 0, $right - $left, $h)
        }
        $g.FillEllipse($brushes[0], [int]($w * 0.08), [int]($h * 0.12), [int]($w * 0.32), [int]($h * 0.42))
        $g.FillEllipse($brushes[2], [int]($w * 0.56), [int]($h * 0.48), [int]($w * 0.36), [int]($h * 0.38))
    }
    finally {
        $brushes | ForEach-Object { $_.Dispose() }
    }
})
$fixture.Show()
Start-Sleep -Milliseconds 300

$previousForeground = [FluxAcrylicProof]::GetForegroundWindow()
$process = Start-Process -FilePath $Executable -WorkingDirectory (Split-Path -Parent $Executable) -WindowStyle Hidden -PassThru
try {
    Start-Sleep -Seconds 3
    $handle = (Get-Process -Id $process.Id).MainWindowHandle
    if ($handle -eq [IntPtr]::Zero) { throw 'Flux Launcher window handle is zero.' }
    [FluxAcrylicProof]::SetWindowPos($handle, [IntPtr]::Zero, $x, $y, 420, 400, 0x40) | Out-Null
    [FluxAcrylicProof]::ShowWindow($handle, 5) | Out-Null
    [FluxAcrylicProof]::BringWindowToTop($handle) | Out-Null
    [FluxAcrylicProof]::SetForegroundWindow($handle) | Out-Null
    Start-Sleep -Milliseconds 400
    [FluxAcrylicProof]::SetCursorPos($x + 210, $y + 36) | Out-Null
    [FluxAcrylicProof]::mouse_event(2, 0, 0, 0, [UIntPtr]::Zero)
    [FluxAcrylicProof]::mouse_event(4, 0, 0, 0, [UIntPtr]::Zero)
    [FluxAcrylicProof]::SetForegroundWindow($handle) | Out-Null
    Start-Sleep -Milliseconds 250
    Save-Screen 'windows11-acrylic-proof-empty.png' $screen
    Send-AsciiText 'steam'
    Start-Sleep -Seconds 2
    Save-Screen 'windows11-acrylic-proof-before-down.png' $screen
    Send-VirtualKey 0x28 0x50 $true
    Start-Sleep -Milliseconds 350
    Save-Screen 'windows11-acrylic-proof-after-down.png' $screen
    Send-VirtualKey 0x27 0x4D $true
    Start-Sleep -Milliseconds 350
    Save-Screen 'windows11-acrylic-proof-action-mode.png' $screen
    [ordered]@{
        Query = 'steam'
        Display = $screen.DeviceName
        Bounds = $screen.Bounds.ToString()
        LauncherX = $x
        LauncherY = $y
        PrimaryWasProtected = $true
        CapturedOn = 'secondary monitor with controlled multicolor fixture'
    } | ConvertTo-Json | Set-Content -Encoding utf8 (Join-Path $OutputDirectory 'proof-result.json')
}
finally {
    if ($process -and !$process.HasExited) { Stop-Process -Id $process.Id -Force }
    if (-not $fixture.IsDisposed) { $fixture.Close(); $fixture.Dispose() }
    if ($previousForeground -ne [IntPtr]::Zero) { [FluxAcrylicProof]::SetForegroundWindow($previousForeground) | Out-Null }
}
Write-Output "Windows 11 Acrylic proof smoke complete: $OutputDirectory"

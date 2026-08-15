param(
    [Parameter(Mandatory = $true)]
    [string]$Executable,

    [Parameter(Mandatory = $true)]
    [string]$OutputDirectory
)

$ErrorActionPreference = "Stop"

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

$probeScriptPath = Join-Path $OutputDirectory "probe-screen.ps1"
@'
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
Add-Type -TypeDefinition @"
using System;
using System.Drawing;
using System.Drawing.Drawing2D;
using System.Windows.Forms;
public sealed class FluxProbeForm : Form
{
    public FluxProbeForm()
    {
        FormBorderStyle = FormBorderStyle.None;
        WindowState = FormWindowState.Maximized;
        ShowInTaskbar = false;
        StartPosition = FormStartPosition.Manual;
        DoubleBuffered = true;
        BackColor = Color.FromArgb(21, 46, 105);
    }

    protected override void OnPaint(PaintEventArgs e)
    {
        base.OnPaint(e);
        using var gradient = new LinearGradientBrush(
            ClientRectangle,
            Color.FromArgb(255, 21, 46, 105),
            Color.FromArgb(255, 154, 41, 99),
            0.0f);
        e.Graphics.FillRectangle(gradient, ClientRectangle);
        var size = Math.Min(ClientSize.Width, ClientSize.Height) * 0.32f;
        var x = (ClientSize.Width - size) / 2.0f;
        var y = (ClientSize.Height - size) / 2.0f;
        using var gold = new SolidBrush(Color.Gold);
        e.Graphics.FillEllipse(gold, x, y, size, size);
    }
}
"@ | Add-Type
$form = New-Object FluxProbeForm
[System.Windows.Forms.Application]::Run($form)
'@ | Set-Content -Encoding utf8 $probeScriptPath
$probeProcess = Start-Process -FilePath "pwsh" -ArgumentList @("-NoProfile", "-File", $probeScriptPath) -PassThru
Start-Sleep -Seconds 2

$stdoutPath = Join-Path $OutputDirectory "launcher.stdout.log"
$stderrPath = Join-Path $OutputDirectory "launcher.stderr.log"
$process = Start-Process -FilePath $Executable -PassThru -RedirectStandardOutput $stdoutPath -RedirectStandardError $stderrPath
try {
    Start-Sleep -Seconds 3
    $idleMemory = Get-MemorySnapshot $process.Id
    Save-Screenshot "mica-desktop.png"

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
    $shell.SendKeys("se")
    Start-Sleep -Seconds 2
    $queryMemory = Get-MemorySnapshot $process.Id
    Save-Screenshot "everything-fallback.png"

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

    $env:FLUX_OPEN_SETTINGS = "1"
    $settingsStdoutPath = Join-Path $OutputDirectory "settings.stdout.log"
    $settingsStderrPath = Join-Path $OutputDirectory "settings.stderr.log"
    $settingsProcess = Start-Process -FilePath $Executable -PassThru -RedirectStandardOutput $settingsStdoutPath -RedirectStandardError $settingsStderrPath
    try {
        Start-Sleep -Seconds 2
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
        CapturedAtUtc = (Get-Date).ToUniversalTime().ToString("O")
        WallpaperProbe = $true
        QueryExpandedProbe = $true
        SettingsPanelProbe = $true
        KeyboardSelectionProbe = $true
        ActionModeProbe = $true
        EnterActionProbe = $true
        Memory = [ordered]@{
            Idle = $idleMemory
            Query = $queryMemory
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
}

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

public static class FluxPluginsSmokeNative {
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

    public static int RectWidth(RECT rect) { return rect.Right - rect.Left; }
    public static int RectHeight(RECT rect) { return rect.Bottom - rect.Top; }
}
'@

function Get-WindowForProcess([System.Diagnostics.Process]$Process) {
    try {
        $Process.Refresh()
        if ($Process.MainWindowHandle -ne [IntPtr]::Zero) {
            return $Process.MainWindowHandle
        }
        return [FluxPluginsSmokeNative]::FindWindowByProcessId([uint32]$Process.Id)
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

function Stop-SmokeProcess([System.Diagnostics.Process]$Process) {
    if ($null -ne $Process) {
        try {
            if (!$Process.HasExited) {
                Stop-Process -Id $Process.Id -Force -ErrorAction SilentlyContinue
                $Process.WaitForExit(5000)
            }
        } catch {
        }
    }
}

function Invoke-PluginStateSmoke([string]$Name, [bool]$InstalledFixture, [string]$Root, [string]$ExecutablePath) {
    $appData = Join-Path $Root $Name
    $configDirectory = Join-Path $appData "FluxLauncher"
    New-Item -ItemType Directory -Path $configDirectory -Force | Out-Null
    $settings = if ($InstalledFixture) {
        '{"start_with_windows":false,"auto_enable_everything":false,"everything_install_prompt_seen":true,"update_checks_enabled":false}'
    } else {
        '{"start_with_windows":false,"auto_enable_everything":true,"everything_install_prompt_seen":true,"update_checks_enabled":false}'
    }
    Set-Content -LiteralPath (Join-Path $configDirectory "settings.json") -Value $settings -Encoding utf8

    $stdoutPath = Join-Path $Root "$Name.stdout.log"
    $stderrPath = Join-Path $Root "$Name.stderr.log"
    $process = $null
    $result = [ordered]@{
        State = $Name
        InstalledFixture = $InstalledFixture
        ProcessId = 0
        WindowHandle = "0"
        WindowVisible = $false
        ForegroundMatchesFlux = $false
        WindowWidth = 0
        WindowHeight = 0
        PluginsTelemetryProbe = $false
        PluginsContentProbe = $false
        EverythingSectionVisible = $false
        AutoEnableCheckboxVisible = $false
        StatusLabelVisible = $false
        InstallButtonLabelProbe = $false
        AlreadyInstalledLabelProbe = $false
        TabVisible = $false
        AutoEnable = $false
        Installed = $false
        InstallButtonVisible = $false
        AlreadyInstalledVisible = $false
        WingetProcessCountBefore = 0
        WingetProcessCountAfter = 0
        Error = $null
    }

    try {
        $env:APPDATA = $appData
        $env:FLUX_OPEN_SETTINGS = "1"
        $env:FLUX_SMOKE_SETTINGS_TAB = "3"
        $env:FLUX_SMOKE_EVERYTHING_PLUGINS = "1"
        $env:FLUX_DISABLE_EVERYTHING_PROMPT = "1"
        $env:FLUX_DISABLE_UPDATE_CHECKS = "1"
        $env:WINDUI_D2D = "0"
        if ($InstalledFixture) {
            Remove-Item Env:FLUX_SMOKE_EVERYTHING_MISSING -ErrorAction SilentlyContinue
            $env:FLUX_SMOKE_EVERYTHING_INSTALLED = "1"
        } else {
            Remove-Item Env:FLUX_SMOKE_EVERYTHING_INSTALLED -ErrorAction SilentlyContinue
            $env:FLUX_SMOKE_EVERYTHING_MISSING = "1"
        }

        $result.WingetProcessCountBefore = @(Get-Process -Name "winget" -ErrorAction SilentlyContinue).Count
        $process = Start-Process -FilePath $ExecutablePath -PassThru -RedirectStandardOutput $stdoutPath -RedirectStandardError $stderrPath
        $result.ProcessId = $process.Id
        $deadline = (Get-Date).AddSeconds(10)
        $window = [IntPtr]::Zero
        while ((Get-Date) -lt $deadline -and $window -eq [IntPtr]::Zero) {
            $window = Get-WindowForProcess $process
            if ($window -eq [IntPtr]::Zero) {
                Start-Sleep -Milliseconds 50
            }
        }
        if ($window -eq [IntPtr]::Zero) {
            throw "Plugins smoke could not find the Flux Settings window for $Name."
        }
        $result.WindowHandle = $window.ToInt64().ToString()
        [FluxPluginsSmokeNative]::SetForegroundWindow($window) | Out-Null
        Start-Sleep -Milliseconds 700
        $result.WindowVisible = [FluxPluginsSmokeNative]::IsWindowVisible($window)
        $result.ForegroundMatchesFlux = [FluxPluginsSmokeNative]::GetForegroundWindow().ToInt64().ToString() -eq $result.WindowHandle
        $rect = New-Object FluxPluginsSmokeNative+RECT
        if (![FluxPluginsSmokeNative]::GetWindowRect($window, [ref]$rect)) {
            throw "Plugins smoke could not measure the Settings window for $Name."
        }
        $result.WindowWidth = [FluxPluginsSmokeNative]::RectWidth($rect)
        $result.WindowHeight = [FluxPluginsSmokeNative]::RectHeight($rect)
        Save-DesktopScreenshot (Join-Path $Root "everything-plugins-$Name.png")
        Start-Sleep -Milliseconds 300
        $stderr = if (Test-Path $stderrPath) { Get-Content $stderrPath -Raw } else { "" }
        $telemetry = [regex]::Match($stderr, "Everything Plugins UI: tab_visible=true everything_section=true auto_enable_checkbox=true status_label=true install_button_label=Install_Everything already_installed_label=Everything_is_already_installed auto_enable=(?<auto>true|false) installed=(?<installed>true|false) install_button_visible=(?<button>true|false) already_installed_visible=(?<already>true|false) status=(?<status>[^\r\n]+)")
        $result.PluginsTelemetryProbe = $telemetry.Success
        $result.PluginsContentProbe = $telemetry.Success
        $result.EverythingSectionVisible = $telemetry.Success
        $result.AutoEnableCheckboxVisible = $telemetry.Success
        $result.StatusLabelVisible = $telemetry.Success
        $result.InstallButtonLabelProbe = $telemetry.Success
        $result.AlreadyInstalledLabelProbe = $telemetry.Success
        if ($telemetry.Success) {
            $result.TabVisible = $true
            $result.AutoEnable = $telemetry.Groups["auto"].Value -eq "true"
            $result.Installed = $telemetry.Groups["installed"].Value -eq "true"
            $result.InstallButtonVisible = $telemetry.Groups["button"].Value -eq "true"
            $result.AlreadyInstalledVisible = $telemetry.Groups["already"].Value -eq "true"
        } else {
            throw "Everything Plugins UI telemetry was not observed for $Name; screenshot is not treated as proof."
        }
        $result.WingetProcessCountAfter = @(Get-Process -Name "winget" -ErrorAction SilentlyContinue).Count
        $result | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $Root "everything-plugins-$Name-summary.json") -Encoding utf8

        if (!$result.PluginsContentProbe -or !$result.EverythingSectionVisible -or !$result.AutoEnableCheckboxVisible -or !$result.StatusLabelVisible -or !$result.InstallButtonLabelProbe -or !$result.AlreadyInstalledLabelProbe) {
            throw "Everything Plugins content probe was incomplete for $Name."
        }
        if (!$result.WindowVisible -or !$result.ForegroundMatchesFlux) {
            throw "Plugins Settings window was not visible and foreground for $Name."
        }
        if ($result.WindowWidth -lt 600 -or $result.WindowHeight -lt 400) {
            throw "Plugins Settings window was undersized for ${Name}: $($result.WindowWidth)x$($result.WindowHeight)."
        }
        if (!$InstalledFixture) {
            if (!$result.AutoEnable -or $result.Installed -or !$result.InstallButtonVisible -or $result.AlreadyInstalledVisible) {
                throw "Missing Everything Plugins state was incorrect: auto=$($result.AutoEnable), installed=$($result.Installed), install_button=$($result.InstallButtonVisible), already_installed=$($result.AlreadyInstalledVisible)."
            }
        } else {
            if ($result.Installed -ne $true -or $result.InstallButtonVisible -or !$result.AlreadyInstalledVisible) {
                throw "Installed Everything Plugins state was incorrect: installed=$($result.Installed), install_button=$($result.InstallButtonVisible), already_installed=$($result.AlreadyInstalledVisible)."
            }
        }
        if ($result.WingetProcessCountAfter -gt $result.WingetProcessCountBefore) {
            throw "Plugins smoke unexpectedly launched winget for $Name."
        }
        return $result
    } catch {
        $result.Error = $_.Exception.Message
        $result.WingetProcessCountAfter = @(Get-Process -Name "winget" -ErrorAction SilentlyContinue).Count
        $result | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $Root "everything-plugins-$Name-summary.json") -Encoding utf8
        throw
    } finally {
        Stop-SmokeProcess $process
        Remove-Item Env:FLUX_SMOKE_EVERYTHING_MISSING -ErrorAction SilentlyContinue
        Remove-Item Env:FLUX_SMOKE_EVERYTHING_INSTALLED -ErrorAction SilentlyContinue
    }
}

New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null
$executablePath = (Resolve-Path $Executable).Path
$missing = $null
$installed = $null
try {
    $missing = Invoke-PluginStateSmoke "missing" $false $OutputDirectory $executablePath
    $installed = Invoke-PluginStateSmoke "installed" $true $OutputDirectory $executablePath
    [ordered]@{
        Missing = $missing
        Installed = $installed
        Error = $null
    } | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $OutputDirectory "everything-plugins-summary.json") -Encoding utf8
    Write-Host "Everything Plugins smoke passed: missing state exposed Install Everything, installed state exposed Already installed, and winget stayed untouched."
} catch {
    [ordered]@{
        Missing = $missing
        Installed = $installed
        Error = $_.Exception.Message
    } | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $OutputDirectory "everything-plugins-summary.json") -Encoding utf8
    throw
} finally {
    Remove-Item Env:APPDATA -ErrorAction SilentlyContinue
    Remove-Item Env:FLUX_OPEN_SETTINGS -ErrorAction SilentlyContinue
    Remove-Item Env:FLUX_SMOKE_SETTINGS_TAB -ErrorAction SilentlyContinue
    Remove-Item Env:FLUX_SMOKE_EVERYTHING_PLUGINS -ErrorAction SilentlyContinue
    Remove-Item Env:FLUX_DISABLE_EVERYTHING_PROMPT -ErrorAction SilentlyContinue
    Remove-Item Env:FLUX_DISABLE_UPDATE_CHECKS -ErrorAction SilentlyContinue
    Remove-Item Env:WINDUI_D2D -ErrorAction SilentlyContinue
}


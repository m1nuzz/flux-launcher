param(
    [Parameter(Mandatory = $true)]
    [string]$Executable,

    [Parameter(Mandatory = $true)]
    [string]$WorkDirectory,

    [int]$ObservationMilliseconds = 5000
)

$ErrorActionPreference = "Stop"

Add-Type @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;

public static class FluxStartupWindows {
    [StructLayout(LayoutKind.Sequential)]
    public struct WindowInfo {
        public IntPtr Hwnd;
        public uint ProcessId;
        public string ClassName;
        public string Title;
    }

    [DllImport("user32.dll", SetLastError = true)]
    private static extern bool EnumWindows(EnumWindowsProc callback, IntPtr lParam);

    [DllImport("user32.dll", SetLastError = true)]
    private static extern bool IsWindowVisible(IntPtr hWnd);

    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern int GetClassName(IntPtr hWnd, char[] className, int maxCount);

    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern int GetWindowText(IntPtr hWnd, char[] text, int maxCount);

    [DllImport("user32.dll", SetLastError = true)]
    private static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint processId);

    private delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);

    public static WindowInfo[] GetVisibleTopLevelWindows() {
        var windows = new List<WindowInfo>();
        EnumWindows((hWnd, lParam) => {
            if (!IsWindowVisible(hWnd)) {
                return true;
            }

            uint processId;
            if (GetWindowThreadProcessId(hWnd, out processId) == 0) {
                return true;
            }

            var classBuffer = new char[256];
            var classLength = GetClassName(hWnd, classBuffer, classBuffer.Length);
            var titleBuffer = new char[512];
            var titleLength = GetWindowText(hWnd, titleBuffer, titleBuffer.Length);
            windows.Add(new WindowInfo {
                Hwnd = hWnd,
                ProcessId = processId,
                ClassName = classLength > 0 ? new string(classBuffer, 0, classLength) : "<unknown>",
                Title = titleLength > 0 ? new string(titleBuffer, 0, titleLength) : ""
            });
            return true;
        }, IntPtr.Zero);
        return windows.ToArray();
    }
}
'@

function Get-ProcessDetails([uint32]$ProcessId) {
    $name = "<exited>"
    $commandLine = "<unavailable>"
    try {
        $process = Get-Process -Id $ProcessId -ErrorAction Stop
        $name = $process.ProcessName
    } catch {
    }
    try {
        $processInfo = Get-CimInstance Win32_Process -Filter "ProcessId = $ProcessId" -ErrorAction Stop
        if ($null -ne $processInfo -and $processInfo.CommandLine) {
            $commandLine = [string]$processInfo.CommandLine
        }
    } catch {
    }
    [ordered]@{
        ProcessId = $ProcessId
        ProcessName = $name
        CommandLine = $commandLine
    }
}

function Get-WindowKey([IntPtr]$Hwnd) {
    return $Hwnd.ToInt64().ToString()
}

function Get-WindowRecords {
    @(
        foreach ($window in [FluxStartupWindows]::GetVisibleTopLevelWindows()) {
            $details = Get-ProcessDetails $window.ProcessId
            [pscustomobject]@{
                Hwnd = Get-WindowKey $window.Hwnd
                ProcessId = $window.ProcessId
                ProcessName = $details.ProcessName
                CommandLine = $details.CommandLine
                ClassName = $window.ClassName
                Title = $window.Title
            }
        }
    )
}

function Stop-FluxProcesses {
    $fluxName = [System.IO.Path]::GetFileNameWithoutExtension($Executable)
    foreach ($process in @(Get-Process -Name $fluxName -ErrorAction SilentlyContinue)) {
        try {
            $process.Refresh()
            $commandLine = (Get-CimInstance Win32_Process -Filter "ProcessId = $($process.Id)" -ErrorAction Stop).CommandLine
            if ($commandLine -notmatch '--visual-preview' -and
                $commandLine -notmatch '--plugin-host' -and
                $commandLine -notmatch '--folder-launch-smoke') {
                Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
            }
        } catch {
            # Ignore a process that exits while preflight is collecting its command line.
        }
    }
    Start-Sleep -Milliseconds 750
}

function Get-EverythingCount {
    return @(
        Get-Process -Name "Everything" -ErrorAction SilentlyContinue
    ).Count
}

function Wait-FluxReady([System.Diagnostics.Process]$Process, [int]$TimeoutMilliseconds) {
    $deadline = (Get-Date).AddMilliseconds($TimeoutMilliseconds)
    while ((Get-Date) -lt $deadline) {
        try {
            $Process.Refresh()
            if ($Process.HasExited) {
                throw "Flux process exited during startup: pid=$($Process.Id) code=$($Process.ExitCode)."
            }
            if ($Process.MainWindowHandle -ne [IntPtr]::Zero) {
                return
            }
        } catch {
            if ($_.Exception.Message -like "Flux process exited during startup*") {
                throw
            }
        }
        Start-Sleep -Milliseconds 50
    }
    throw "Flux main window did not become ready within $TimeoutMilliseconds ms."
}

function Observe-Launch(
    [string]$Label,
    [System.Diagnostics.Process]$Process,
    [hashtable]$BaselineWindows,
    [System.Collections.Generic.List[object]]$ObservationLog,
    [System.Collections.Generic.HashSet[string]]$SeenWindows,
    [int]$TimeoutMilliseconds
) {
    $launchStart = Get-Date
    $deadline = $launchStart.AddMilliseconds($TimeoutMilliseconds)
    $mainWindowSeen = $false
    $terminalSeen = $false
    $newWindowSeen = $false

    while ((Get-Date) -lt $deadline) {
        $windows = Get-WindowRecords
        foreach ($window in $windows) {
            $key = "$Label|$($window.Hwnd)"
            if ($BaselineWindows.ContainsKey($window.Hwnd)) {
                continue
            }
            $newWindowSeen = $true
            if (!$SeenWindows.Contains($key)) {
                [void]$SeenWindows.Add($key)
                $isFlux = $window.ProcessId -eq [uint32]$Process.Id
                $isConsole = $window.ClassName -match '^(ConsoleWindowClass|PseudoConsoleWindow|CASCADIA_HOSTING_WINDOW_CLASS)$' -or
                    $window.ProcessName -match '^(conhost|OpenConsole|WindowsTerminal)$'
                $record = [ordered]@{
                    Utc = (Get-Date).ToUniversalTime().ToString('O')
                    Launch = $Label
                    Hwnd = $window.Hwnd
                    ProcessId = $window.ProcessId
                    ProcessName = $window.ProcessName
                    ClassName = $window.ClassName
                    Title = $window.Title
                    CommandLine = $window.CommandLine
                    FluxProcessWindow = $isFlux
                    ConsoleLike = $isConsole
                }
                $ObservationLog.Add([pscustomobject]$record)
                Write-Host ("Startup window: launch={0} hwnd={1} pid={2} process={3} class={4} title=[{5}] console_like={6} command=[{7}]" -f $Label, $window.Hwnd, $window.ProcessId, $window.ProcessName, $window.ClassName, $window.Title, $isConsole, $window.CommandLine)
                if ($isConsole) {
                    $terminalSeen = $true
                }
            }
            if ($window.ProcessId -eq [uint32]$Process.Id) {
                $mainWindowSeen = $true
            }
        }
        try {
            $Process.Refresh()
            if ($Process.HasExited) {
                throw "Flux $Label process exited during observation: pid=$($Process.Id) code=$($Process.ExitCode)."
            }
        } catch {
            if ($_.Exception.Message -like "Flux $Label process exited during observation*") {
                throw
            }
        }
        if ($mainWindowSeen -and $terminalSeen) {
            # Keep observing until the requested window is gone, so a short-lived
            # console window is captured even after Flux itself is ready.
        }
        Start-Sleep -Milliseconds 20
    }

    [pscustomobject]@{
        Label = $Label
        MainWindowSeen = $mainWindowSeen
        TerminalSeen = $terminalSeen
        NewWindowSeen = $newWindowSeen
        ProcessId = $Process.Id
    }
}

New-Item -ItemType Directory -Path $WorkDirectory -Force | Out-Null
$appData = Join-Path $WorkDirectory "AppData"
New-Item -ItemType Directory -Path (Join-Path $appData "FluxLauncher") -Force | Out-Null
$env:APPDATA = $appData
$env:FLUX_DISABLE_UPDATE_CHECKS = "1"
$env:FLUX_DISABLE_EVERYTHING_PROMPT = "1"
$env:WINDUI_D2D = "0"

$settingsPath = Join-Path $appData "FluxLauncher\settings.json"
@'
{
  "start_with_windows": false,
  "auto_enable_everything": true,
  "everything_install_prompt_seen": true,
  "update_checks_enabled": false,
  "clear_query_on_activation": true
}
'@ | Set-Content -LiteralPath $settingsPath -Encoding utf8

Stop-FluxProcesses
$initialEverythingCount = Get-EverythingCount
$baselineWindows = @{}
foreach ($window in Get-WindowRecords) {
    $baselineWindows[$window.Hwnd] = $true
}
$observationLog = [System.Collections.Generic.List[object]]::new()
$seenWindows = [System.Collections.Generic.HashSet[string]]::new()
$results = [System.Collections.Generic.List[object]]::new()
$first = $null
$second = $null

try {
    $first = Start-Process -FilePath $Executable -PassThru
    $firstResult = Observe-Launch -Label "first" -Process $first -BaselineWindows $baselineWindows -ObservationLog $observationLog -SeenWindows $seenWindows -TimeoutMilliseconds $ObservationMilliseconds
    $results.Add($firstResult)
    Wait-FluxReady $first 1000
    $firstEverythingCount = Get-EverythingCount
    if ($firstEverythingCount -gt ($initialEverythingCount + 1)) {
        throw "First Flux launch created more than one additional Everything process: before=$initialEverythingCount after=$firstEverythingCount."
    }
    Stop-Process -Id $first.Id -Force -ErrorAction SilentlyContinue
    Start-Sleep -Milliseconds 1000

    $second = Start-Process -FilePath $Executable -PassThru
    $secondResult = Observe-Launch -Label "second" -Process $second -BaselineWindows $baselineWindows -ObservationLog $observationLog -SeenWindows $seenWindows -TimeoutMilliseconds $ObservationMilliseconds
    $results.Add($secondResult)
    Wait-FluxReady $second 1000
    $secondEverythingCount = Get-EverythingCount
    if ($secondEverythingCount -gt ($initialEverythingCount + 1)) {
        throw "Second Flux launch created a duplicate Everything process: before=$initialEverythingCount after=$secondEverythingCount."
    }

    $terminalObservations = @($observationLog | Where-Object { $_.ConsoleLike })
    $summary = [ordered]@{
        InitialEverythingCount = $initialEverythingCount
        FirstEverythingCount = $firstEverythingCount
        SecondEverythingCount = $secondEverythingCount
        Results = @($results)
        TerminalObservations = @($terminalObservations)
        AllNewVisibleWindows = @($observationLog)
        ObservationMilliseconds = $ObservationMilliseconds
    }
    $summaryPath = Join-Path $WorkDirectory "startup-console-summary.json"
    $summary | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $summaryPath -Encoding utf8
    $observationLog | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $WorkDirectory "startup-visible-windows.json") -Encoding utf8

    if ($terminalObservations.Count -gt 0) {
        throw "Startup console window observed: $($terminalObservations.Count) new console-like window(s). See $summaryPath."
    }
    if (@($results | Where-Object { !$_.MainWindowSeen }).Count -gt 0) {
        throw "Startup smoke did not observe a Flux top-level window for every launch. See $summaryPath."
    }

    Write-Host "Startup console smoke passed: no new console-like window appeared on first or second Flux launch; Everything process count remained idempotent."
}
finally {
    if ($null -ne $first) {
        Stop-Process -Id $first.Id -Force -ErrorAction SilentlyContinue
    }
    if ($null -ne $second) {
        Stop-Process -Id $second.Id -Force -ErrorAction SilentlyContinue
    }
    Stop-FluxProcesses
}

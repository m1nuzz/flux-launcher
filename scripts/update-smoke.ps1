param(
    [Parameter(Mandatory = $true)]
    [string]$Executable,
    [Parameter(Mandatory = $true)]
    [string]$Installer,
    [Parameter(Mandatory = $true)]
    [string]$WorkDirectory,
    [Parameter(Mandatory = $true)]
    [string]$CurrentVersion
)

$ErrorActionPreference = "Stop"

Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class FluxUpdateSmokeNative {
    [DllImport("user32.dll")]
    public static extern bool IsHungAppWindow(IntPtr hWnd);
}
"@

$workRoot = (New-Item -ItemType Directory -Force -Path $WorkDirectory).FullName
$fixtureRoot = Join-Path $workRoot "fixture"
$appDataRoot = Join-Path $workRoot "appdata"
$installRoot = Join-Path $workRoot "updated-install"
$tracePath = Join-Path $workRoot "update-trace.log"
$transitionPath = Join-Path $workRoot "first-release-requested.marker"
$serverScript = Join-Path $PSScriptRoot "update-fixture-server.ps1"
$fixtureInstaller = Join-Path $fixtureRoot "FluxLauncherCN-Setup.exe"
$firstReleasePath = Join-Path $fixtureRoot "latest.json"
$stableReleasePath = Join-Path $fixtureRoot "latest-done.json"
$server = $null
$launcher = $null
# 脚本会覆盖 APPDATA/LOCALAPPDATA 指向隔离目录。CI 每个 step 使用全新 shell，
# 覆盖不影响后续步骤；但本地复用时脚本常在同一会话中连续运行，泄漏会污染后续
# 命令（真实用户目录）。因此记录原值并在 finally 中恢复。
$previousAppData = $env:APPDATA
$previousLocalAppData = $env:LOCALAPPDATA

function Write-JsonFile {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path,
        [Parameter(Mandatory = $true)]
        [object]$Value
    )
    $Value | ConvertTo-Json -Depth 8 | Set-Content -Path $Path -Encoding utf8
}

function Wait-Until {
    param(
        [Parameter(Mandatory = $true)]
        [scriptblock]$Condition,
        [Parameter(Mandatory = $true)]
        [string]$Description,
        [int]$TimeoutSeconds = 120
    )
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        if (& $Condition) {
            return
        }
        Start-Sleep -Milliseconds 250
    }
    throw "Timed out waiting for $Description"
}

function Get-TraceLine {
    if (-not (Test-Path $tracePath)) {
        return @()
    }
    return @(Get-Content -Path $tracePath)
}

function Stop-ExistingFluxProcess {
    [CmdletBinding(SupportsShouldProcess, ConfirmImpact = 'None')]
    param()
    if (!$PSCmdlet.ShouldProcess("flux-launcher", "Stop pre-existing launcher processes")) {
        return
    }
    $existing = @(Get-Process -Name "flux-launcher" -ErrorAction SilentlyContinue)
    foreach ($process in $existing) {
        if (!$process.HasExited) {
            Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
        }
    }
    $deadline = (Get-Date).AddSeconds(10)
    while ((Get-Date) -lt $deadline) {
        if (@(Get-Process -Name "flux-launcher" -ErrorAction SilentlyContinue).Count -eq 0) {
            return
        }
        Start-Sleep -Milliseconds 250
    }
    throw "A previous Flux process remained alive before update smoke"
}

function Assert-LauncherResponsive {
    param(
        [Parameter(Mandatory = $true)]
        [System.Diagnostics.Process]$Process
    )
    $Process.Refresh()
    if ($Process.HasExited) {
        throw "Launcher exited before update installation completed"
    }
    $handle = $Process.MainWindowHandle
    if ($handle -ne [IntPtr]::Zero -and [FluxUpdateSmokeNative]::IsHungAppWindow($handle)) {
        throw "Launcher UI became hung while downloading the update"
    }
}

function Get-UpdatedLauncherProcess {
    $expectedPath = [System.IO.Path]::GetFullPath((Join-Path $installRoot "flux-launcher.exe"))
    return @(Get-Process -Name "flux-launcher" -ErrorAction SilentlyContinue | Where-Object {
        try {
            $_.Path -eq $expectedPath
        }
        catch {
            $false
        }
    })
}

New-Item -ItemType Directory -Force -Path $fixtureRoot, $appDataRoot | Out-Null
Stop-ExistingFluxProcess
Copy-Item -LiteralPath $Installer -Destination $fixtureInstaller -Force
$installerHash = (Get-FileHash -Algorithm SHA256 -Path $fixtureInstaller).Hash.ToLowerInvariant()
$port = 18963
$prefix = "http://127.0.0.1:$port/"
# 端口被占用时 fixture 服务器的绑定失败发生在子进程里，本脚本察觉不到，
# 启动器的更新检查会打到未知监听者（例如上一轮残留的服务器，其计数状态
# 已消费，会返回"无更新"），随后脚本在不相关的等待上超时。这里快速失败。
if (Get-NetTCPConnection -LocalPort $port -State Listen -ErrorAction SilentlyContinue) {
    throw "Update smoke fixture port $port is already in use; stop the stale fixture server and retry."
}
# $CurrentVersion 只在脚本顶层使用，PSSA 的 PSReviewUnusedParameter 也仅在
# 顶层统计引用，因此这里直接推导合成 release tag，不再包一层函数。
$normalizedCurrentVersion = $CurrentVersion.Trim().TrimStart('v')
# 当前版本可能携带 SemVer 预发布标识（如 0.3.0-beta.1）；.NET [version] 不接受，
# 合成夹具版本只依赖数字核心（Major.Minor.Build+1），先剥掉后缀再解析。
$numericCurrentVersion = $normalizedCurrentVersion -replace '-.*$', ''
$parsedCurrentVersion = [version]::Parse($numericCurrentVersion)
$syntheticLatestTag = "v{0}.{1}.{2}" -f $parsedCurrentVersion.Major, $parsedCurrentVersion.Minor, ($parsedCurrentVersion.Build + 1)
$currentStableTag = "v$normalizedCurrentVersion"

Write-JsonFile -Path $firstReleasePath -Value @{
    tag_name = $syntheticLatestTag
    html_url = "https://example.test/releases/tag/$syntheticLatestTag"
    draft = $false
    prerelease = $false
    assets = @(@{
        name = "FluxLauncherCN-Setup.exe"
        browser_download_url = "${prefix}FluxLauncherCN-Setup.exe"
        digest = "sha256:$installerHash"
    })
}
Write-JsonFile -Path $stableReleasePath -Value @{
    tag_name = $currentStableTag
    html_url = "https://example.test/releases/tag/$currentStableTag"
    draft = $false
    # 当前版本自身携带预发布标识时，夹具如实标记为 prerelease：稳定更新器
    # 必须忽略它（不变量：beta release 不得触发稳定更新）；纯数字版本保持
    # 原有的「同版本不算更新」路径。
    prerelease = ($normalizedCurrentVersion -ne $numericCurrentVersion)
    assets = @()
}

# Use an isolated settings file so the test exercises the real automatic update path.
$settingsDirectory = Join-Path $appDataRoot "FluxLauncher"
New-Item -ItemType Directory -Force -Path $settingsDirectory | Out-Null
Write-JsonFile -Path (Join-Path $settingsDirectory "settings.json") -Value @{
    update_checks_enabled = $true
    auto_install_updates = $true
    last_update_check_unix = 0
    update_interval_hours = 24
    start_with_windows = $false
    everything_enabled = $false
    everything_install_prompt_seen = $true
}

$server = Start-Process -FilePath "pwsh" -ArgumentList @(
    "-NoProfile",
    "-File", $serverScript,
    "-Prefix", $prefix,
    "-Root", $fixtureRoot,
    "-TransitionFile", $transitionPath
) -PassThru -WindowStyle Hidden

# fixture 服务器是异步子进程，pwsh 冷启动 + HttpListener 绑定需要时间；启动器
# 的首次更新检查没有重试，若在绑定完成前发起会直接失败且不会恢复。启动前先
# 探测监听就绪（探测未知路径会得到 404，不影响服务器的请求计数与 transition
# 标记），失败则给出明确错误而不是让脚本在无关的等待上超时。
$serverReadyDeadline = (Get-Date).AddSeconds(15)
$serverReady = $false
while ((Get-Date) -lt $serverReadyDeadline) {
    if ($server.HasExited) {
        throw "Update fixture server exited during startup; port $port may be occupied."
    }
    try {
        Invoke-WebRequest -Uri "${prefix}flux-update-smoke-probe" -UseBasicParsing -TimeoutSec 2 | Out-Null
        $serverReady = $true
        break
    }
    catch {
        if ($null -ne $_.Exception.Response) {
            $serverReady = $true
            break
        }
    }
    Start-Sleep -Milliseconds 250
}
if (-not $serverReady) {
    throw "Update fixture server did not become ready on port $port within 15s."
}

try {
    $env:APPDATA = $appDataRoot
    $env:LOCALAPPDATA = $appDataRoot
    $env:FLUX_UPDATE_API_URL = "${prefix}latest"
    $env:FLUX_FORCE_UPDATE_CHECK = "1"
    $env:FLUX_UPDATE_TRACE_FILE = $tracePath
    $env:FLUX_UPDATE_INSTALL_DIR = $installRoot
    $launcher = Start-Process -FilePath (Resolve-Path $Executable).Path -PassThru

    Wait-Until -Description "the first update progress event" -Condition {
        Assert-LauncherResponsive -Process $launcher
        (Get-TraceLine | Where-Object { $_ -like "update-progress*" }).Count -ge 2
    }
    Assert-LauncherResponsive -Process $launcher

    Wait-Until -Description "the installer handoff" -Condition {
        (Get-TraceLine | Where-Object { $_ -like "update-installer-started*" }).Count -ge 1
    }
    # 安装器交接后旧进程退出与重启的等待放宽到 300s：开发机上未签名安装器的
    # Defender 实时扫描冷启动可能超过默认的 120s（CI runner 不受影响）。
    Wait-Until -Description "the old launcher process to exit" -TimeoutSeconds 300 -Condition {
        $launcher.Refresh()
        $launcher.HasExited
    }
    Wait-Until -Description "the updated install root" -TimeoutSeconds 300 -Condition {
        Test-Path (Join-Path $installRoot "flux-launcher.exe")
    }
    Wait-Until -Description "the updated launcher restart hidden in the tray" -TimeoutSeconds 300 -Condition {
        $updated = @(Get-UpdatedLauncherProcess)
        if ($updated.Count -lt 1) {
            return $false
        }
        $updated[0].Refresh()
        $updated[0].MainWindowHandle -eq [IntPtr]::Zero
    }

    $updatedProcesses = @(Get-UpdatedLauncherProcess)
    if ($updatedProcesses.Count -ne 1) {
        throw "Expected exactly one updated Flux process, found $($updatedProcesses.Count)"
    }
    $updatedProcesses[0].Refresh()
    if ($updatedProcesses[0].MainWindowHandle -ne [IntPtr]::Zero) {
        throw "Automatic update relaunched a visible Search window instead of staying hidden in the tray"
    }

    $traceLines = Get-TraceLine
    if ($traceLines | Where-Object { $_ -like "update-failed*" }) {
        throw "Update trace contains a failure: $($traceLines -join ' | ')"
    }
    $progressLines = Get-TraceLine | Where-Object { $_ -like "update-progress*" }
    $parsedProgress = @(
        $progressLines | ForEach-Object {
            $parts = $_ -split "`t"
            [pscustomobject]@{
                Received = [UInt64]$parts[2]
                Total = [UInt64]($parts[3] -replace '[^0-9]', '')
            }
        }
    )
    if ($parsedProgress.Count -lt 2) {
        throw "Update emitted fewer than two progress events"
    }
    for ($index = 1; $index -lt $parsedProgress.Count; $index++) {
        if ($parsedProgress[$index].Received -lt $parsedProgress[$index - 1].Received) {
            throw "Update progress moved backwards"
        }
        if ($parsedProgress[$index].Total -ne $parsedProgress[0].Total) {
            throw "Update progress total changed during download"
        }
    }
    if ($parsedProgress[-1].Received -ne $parsedProgress[-1].Total) {
        throw "Update progress did not finish at 100 percent"
    }
    if (-not (Test-Path $transitionPath)) {
        throw "Fixture server did not receive the release check"
    }
    Write-Host "Update smoke passed: real HTTP download, SHA256 verification, monotonic byte progress, non-hung UI, installer handoff, hidden tray-only automatic restart, and single updated process were verified."
}
catch {
    Write-Host "Update smoke trace before failure:"
    Get-TraceLine | ForEach-Object { Write-Host $_ }
    throw
}
finally {
    if ($null -ne $launcher) {
        $launcher.Refresh()
        if (-not $launcher.HasExited) {
            Stop-Process -Id $launcher.Id -Force -ErrorAction SilentlyContinue
        }
    }
    Get-Process -Name "flux-launcher" -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    if ($null -ne $server -and -not $server.HasExited) {
        Stop-Process -Id $server.Id -Force -ErrorAction SilentlyContinue
    }
    Remove-Item Env:FLUX_UPDATE_API_URL -ErrorAction SilentlyContinue
    Remove-Item Env:FLUX_FORCE_UPDATE_CHECK -ErrorAction SilentlyContinue
    Remove-Item Env:FLUX_UPDATE_TRACE_FILE -ErrorAction SilentlyContinue
    Remove-Item Env:FLUX_UPDATE_INSTALL_DIR -ErrorAction SilentlyContinue
    $env:APPDATA = $previousAppData
    $env:LOCALAPPDATA = $previousLocalAppData
}

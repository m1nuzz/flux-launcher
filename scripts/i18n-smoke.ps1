<#
.SYNOPSIS
Capture localized settings and test language application on a disposable desktop.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Executable,
    [Parameter(Mandatory)][string]$OutputDirectory,
    [switch]$AllowDesktopAutomation
)
$ErrorActionPreference = 'Stop'
if (-not $AllowDesktopAutomation) { throw 'Use -AllowDesktopAutomation on a disposable Windows desktop.' }
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms
Add-Type @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class FluxLocaleSmoke {
    public delegate bool EnumCallback(IntPtr hwnd, IntPtr data);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumCallback callback, IntPtr data);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
    [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr hwnd,StringBuilder text,int count);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hwnd,int command);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hwnd,out RECT rect);
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr hwnd,out RECT rect);
    [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr hwnd,uint message,UIntPtr w,IntPtr l);
    [DllImport("user32.dll")] public static extern void keybd_event(byte key,byte scan,uint flags,UIntPtr extra);
    [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr context);
    public static IntPtr Find(uint target) {
        IntPtr found=IntPtr.Zero;
        EnumWindows((hwnd,data)=> {
            uint pid; GetWindowThreadProcessId(hwnd,out pid);
            var title=new StringBuilder(256); GetWindowText(hwnd,title,256);
            if(pid==target && title.ToString()=="Flux Launcher") { found=hwnd; return false; }
            return true;
        },IntPtr.Zero);
        return found;
    }
    public static void Key(byte key) { keybd_event(key,0,0,UIntPtr.Zero); keybd_event(key,0,2,UIntPtr.Zero); }
    public static void Click(IntPtr hwnd,int x,int y) {
        var point=new IntPtr((y<<16)|(x&0xffff));
        PostMessage(hwnd,0x0201,new UIntPtr(1),point); PostMessage(hwnd,0x0202,UIntPtr.Zero,point);
    }
}
'@
[FluxLocaleSmoke]::SetProcessDpiAwarenessContext([IntPtr]::new(-4)) | Out-Null
$source=(Resolve-Path -LiteralPath $Executable).Path
New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null
$output=(Resolve-Path -LiteralPath $OutputDirectory).Path
$fixture=Join-Path $output 'fixture'
New-Item -ItemType Directory -Path $fixture -Force | Out-Null
$binary=Join-Path $fixture 'flux-launcher.exe'
Copy-Item -LiteralPath $source -Destination $binary -Force
$profile=Join-Path $output 'profile'
$configDirectory=Join-Path $profile 'FluxLauncher'
New-Item -ItemType Directory -Path $configDirectory -Force | Out-Null
$configPath=Join-Path $configDirectory 'settings.json'
$runKey='HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$oldStartup=(Get-ItemProperty -LiteralPath $runKey -ErrorAction SilentlyContinue).'Flux Launcher'
$environmentNames=@('APPDATA','FLUX_OPEN_SETTINGS','FLUX_DISABLE_SINGLE_INSTANCE','FLUX_SMOKE_SETTINGS_TAB','FLUX_DISABLE_UPDATE_CHECKS','FLUX_SMOKE_SETTINGS_UI','FLUX_SMOKE_UI_PROBE_FILE')
$savedEnvironment=@{}
foreach ($name in $environmentNames) { $savedEnvironment[$name]=[Environment]::GetEnvironmentVariable($name,'Process') }
$catalogPath=Join-Path $PSScriptRoot '..\crates\flux-launcher\locales\translations.json'
$catalog=Get-Content -LiteralPath $catalogPath -Raw -Encoding utf8 | ConvertFrom-Json -AsHashtable

function Assert-LocaleProbe([string]$LogPath,[string]$Language,[uint32]$ExpectedProcessId) {
    $deadline=[DateTime]::UtcNow.AddSeconds(3)
    do {
        $probes=@(Get-Content -LiteralPath $LogPath -Encoding utf8 -ErrorAction SilentlyContinue |
            Where-Object { $_.StartsWith('Settings UI locale probe: ') } |
            ForEach-Object { try { $_.Substring('Settings UI locale probe: '.Length) | ConvertFrom-Json -ErrorAction Stop } catch { } } |
            Where-Object { $_.process_id -eq $ExpectedProcessId })
        if ($probes.Count -gt 0) { break }
        Start-Sleep -Milliseconds 100
    } while ([DateTime]::UtcNow -lt $deadline)
    if ($probes.Count -eq 0) { throw "Missing translated UI probe for process $ExpectedProcessId" }
    $probe=$probes[-1]
    if ($probe.settings_visible -isnot [bool] -or -not $probe.settings_visible) { throw 'Settings panel was not visible in the locale probe.' }
    $expectedCaption=if ($Language -eq 'en-US') { 'Language' } else { $catalog['Language'][$Language] }
    if ($probe.language -ne $Language -or $probe.caption -cne $expectedCaption) {
        throw "UI translation mismatch for ${Language}: $($probe | ConvertTo-Json -Compress)"
    }
    foreach ($origin in @('Start Menu','App Paths')) {
        $key="Application • $origin"
        $expected=if ($Language -eq 'en-US') { $key } else { $catalog[$key][$Language] }
        if ($probe.application_subtitles.$origin -cne $expected) {
            throw "Application subtitle translation mismatch for $Language / $origin"
        }
    }
    if ($probe.open_settings_leaked -ne $false) { throw 'One-shot Settings request leaked into the resident process.' }
    return $probe
}

function Stop-Fixture {
    Get-Process flux-launcher -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $binary } | Stop-Process -Force
}
function Get-FixtureWindow([System.Diagnostics.Process]$Process) {
    $deadline=[DateTime]::UtcNow.AddSeconds(10)
    do {
        $Process.Refresh()
        if ($Process.HasExited) { throw "Fixture exited: $($Process.ExitCode)" }
        $handle=[FluxLocaleSmoke]::Find([uint32]$Process.Id)
        if ($handle -ne [IntPtr]::Zero) {
            [FluxLocaleSmoke]::ShowWindow($handle,5) | Out-Null
            [FluxLocaleSmoke]::SetForegroundWindow($handle) | Out-Null
            Start-Sleep -Milliseconds 600
            return $handle
        }
        Start-Sleep -Milliseconds 100
    } while ([DateTime]::UtcNow -lt $deadline)
    throw 'Fixture window was not created.'
}
function Save-Capture([IntPtr]$Handle,[string]$Name,[switch]$FullScreen) {
    $rect=New-Object FluxLocaleSmoke+RECT
    if ($FullScreen) {
        $bounds=[System.Windows.Forms.Screen]::PrimaryScreen.Bounds
        $rect.Left=$bounds.Left; $rect.Top=$bounds.Top; $rect.Right=$bounds.Right; $rect.Bottom=$bounds.Bottom
    } elseif (-not [FluxLocaleSmoke]::GetWindowRect($Handle,[ref]$rect)) { throw 'Could not read window bounds.' }
    $width=$rect.Right-$rect.Left; $height=$rect.Bottom-$rect.Top
    if ($width -lt 500 -or $height -lt 350) { throw "Invalid settings capture: ${width}x${height}" }
    $bitmap=New-Object System.Drawing.Bitmap($width,$height)
    $graphics=[System.Drawing.Graphics]::FromImage($bitmap)
    try {
        $graphics.CopyFromScreen($rect.Left,$rect.Top,0,0,$bitmap.Size)
        $bitmap.Save((Join-Path $output $Name),[System.Drawing.Imaging.ImageFormat]::Png)
    } finally { $graphics.Dispose(); $bitmap.Dispose() }
}
$evidence=@()
$languages=@(
    'en-US','zh-CN','zh-TW','ja-JP','ru-RU',
    'ko','uk-UA','de','fr','es','es-419','it','pt-BR','pt-PT','pl',
    'tr','nl','cs','da','sk','sr','vi','ar','he','nb'
)
if (($languages | Select-Object -Unique).Count -ne 25) { throw 'Expected 25 unique language catalogs.' }
try {
    $env:APPDATA=$profile; $env:FLUX_OPEN_SETTINGS='1'; $env:FLUX_DISABLE_SINGLE_INSTANCE='1'; $env:FLUX_DISABLE_UPDATE_CHECKS='1'; $env:FLUX_SMOKE_SETTINGS_UI='1'
    for ($localeIndex=0; $localeIndex -lt $languages.Count; $localeIndex++) {
        $language=$languages[$localeIndex]
        for ($tab=0; $tab -lt 5; $tab++) {
            Stop-Fixture
            @{language=$language;language_explicit=$true;start_with_windows=$false;update_checks_enabled=$false;auto_enable_everything=$false;everything_install_prompt_seen=$true;switch_to_english_layout=$false} | ConvertTo-Json | Set-Content -LiteralPath $configPath -Encoding utf8
            $env:FLUX_SMOKE_SETTINGS_TAB=$tab.ToString()
            $logPath=Join-Path $output "$language-tab-$tab.stderr.log"
            $probePath=Join-Path $output "$language-tab-$tab.probe.log"
            $env:FLUX_SMOKE_UI_PROBE_FILE=$probePath
            $app=Start-Process -FilePath $binary -WindowStyle Hidden -RedirectStandardError $logPath -PassThru
            $handle=Get-FixtureWindow $app
            $null=Assert-LocaleProbe $probePath $language ([uint32]$app.Id)
            Save-Capture $handle "$language-tab-$tab.png"
            if ($tab -ne 0) { continue }
            $client=New-Object FluxLocaleSmoke+RECT
            [FluxLocaleSmoke]::GetClientRect($handle,[ref]$client) | Out-Null
            $scale=[FluxLocaleSmoke]::GetDpiForWindow($handle)/96.0
            [FluxLocaleSmoke]::Click($handle,($client.Right-[int](130*$scale)),[int](172*$scale))
            Start-Sleep -Milliseconds 300
            Save-Capture $handle "$language-dropdown.png" -FullScreen
            $targetIndex=($localeIndex+1)%$languages.Count
            [FluxLocaleSmoke]::Key(0x24)
            for ($step=0; $step -le $targetIndex; $step++) { [FluxLocaleSmoke]::Key(0x28) }
            [FluxLocaleSmoke]::Key(0x0D)
            Start-Sleep -Milliseconds 300
            Save-Capture $handle "$language-before-apply.png"
            # Change tabs with the unsaved language draft, then verify that the
            # separately owned native preview is reaped before the handoff.
            [FluxLocaleSmoke]::Click($handle,[int](80*$scale),[int](106*$scale))
            $previewDeadline=[DateTime]::UtcNow.AddSeconds(5)
            do {
                Start-Sleep -Milliseconds 150
                $previews=@(Get-CimInstance Win32_Process -Filter "ParentProcessId = $($app.Id)" | Where-Object { $_.CommandLine -match '--visual-preview' })
            } while ($previews.Count -eq 0 -and [DateTime]::UtcNow -lt $previewDeadline)
            if ($previews.Count -ne 1) { throw 'Visual tab did not create exactly one native preview.' }
            Save-Capture $handle "$language-preview-before-apply.png" -FullScreen
            # Click inside the common right edge: translated captions change width.
            [FluxLocaleSmoke]::Click($handle,($client.Right-[int](30*$scale)),($client.Bottom-[int](24*$scale)))
            $deadline=[DateTime]::UtcNow.AddSeconds(15)
            do {
                Start-Sleep -Milliseconds 150
                $app.Refresh()
                $saved=Get-Content -LiteralPath $configPath -Raw | ConvertFrom-Json
            } while ((-not $app.HasExited -or $saved.language -ne $languages[$targetIndex]) -and [DateTime]::UtcNow -lt $deadline)
            if (-not $app.HasExited -or $saved.language -ne $languages[$targetIndex] -or -not $saved.language_explicit) { throw "Language apply/restart failed: $language -> $($languages[$targetIndex])" }
            foreach ($preview in $previews) {
                if (Get-Process -Id $preview.ProcessId -ErrorAction SilentlyContinue) { throw 'Old preview survived the language handoff.' }
            }
            $replacement=@(Get-Process flux-launcher -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $binary -and $_.Id -ne $app.Id })
            if ($replacement.Count -ne 1) { throw 'Language handoff did not leave exactly one replacement.' }
            $handle=Get-FixtureWindow $replacement[0]
            $translated=Assert-LocaleProbe $probePath $languages[$targetIndex] ([uint32]$replacement[0].Id)
            Save-Capture $handle "$language-after-restart.png"
            [FluxLocaleSmoke]::PostMessage($handle,0x8001,[UIntPtr]::Zero,[IntPtr]::new(0x0205)) | Out-Null
            Start-Sleep -Milliseconds 300
            Save-Capture $handle "$language-tray.png" -FullScreen
            [FluxLocaleSmoke]::Key(0x1B)
            $evidence += [pscustomobject]@{language=$language;changedTo=$saved.language;restarted=$true;caption=$translated.caption;applicationSubtitles=$translated.application_subtitles;oneShotConsumed=(-not $translated.open_settings_leaked);settingsVisible=$translated.settings_visible}
        }
    }
    $evidence | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $output 'language-handoffs.json') -Encoding utf8
    if ($evidence.Count -ne $languages.Count) { throw 'Language handoff evidence is incomplete.' }
    Write-Output "PASS: all $($languages.Count) languages, settings tabs, dropdowns, trays, and language handoffs."
} finally {
    Stop-Fixture
    foreach ($name in $environmentNames) { [Environment]::SetEnvironmentVariable($name,$savedEnvironment[$name],'Process') }
    if ($null -ne $oldStartup) { Set-ItemProperty -LiteralPath $runKey -Name 'Flux Launcher' -Value $oldStartup }
    else { Remove-ItemProperty -LiteralPath $runKey -Name 'Flux Launcher' -ErrorAction SilentlyContinue }
}

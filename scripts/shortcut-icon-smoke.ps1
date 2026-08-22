param(
    [Parameter(Mandatory = $true)]
    [string]$Executable,
    [Parameter(Mandatory = $true)]
    [string]$WorkDirectory
)

$ErrorActionPreference = "Stop"
New-Item -ItemType Directory -Path $WorkDirectory -Force | Out-Null

$systemRoot = [Environment]::GetFolderPath("Windows")
$shell32 = Join-Path $systemRoot "System32\shell32.dll"
$notepad = Join-Path $systemRoot "System32\notepad.exe"
$urlShortcut = Join-Path $WorkDirectory "Steam-style.url"
$lnkShortcut = Join-Path $WorkDirectory "Steam-style.lnk"

@"
[InternetShortcut]
URL=steam://rungameid/730
IconFile=$shell32
IconIndex=3
"@ | Set-Content -LiteralPath $urlShortcut -Encoding ascii

$wshell = New-Object -ComObject WScript.Shell
$shortcut = $wshell.CreateShortcut($lnkShortcut)
$shortcut.TargetPath = $notepad
$shortcut.Arguments = "steam://rungameid/730"
$shortcut.IconLocation = "$shell32,3"
$shortcut.Save()

foreach ($shortcutPath in @($urlShortcut, $lnkShortcut)) {
    $process = Start-Process -FilePath $Executable -ArgumentList @(
        "--shortcut-icon-smoke",
        ('"{0}"' -f $shortcutPath)
    ) -Wait -PassThru -WindowStyle Hidden
    if ($process.ExitCode -ne 0) {
        throw "Shortcut icon extraction failed for $shortcutPath with exit code $($process.ExitCode)."
    }
}

Write-Host "Shortcut icon smoke passed for Steam-style .url and .lnk fixtures."

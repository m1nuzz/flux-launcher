[CmdletBinding()]
param(
    [string]$ArtifactDirectory,
    [string[]]$Files,
    [string]$CertificateBase64 = $env:WINDOWS_SIGNING_CERTIFICATE_BASE64,
    [string]$CertificatePassword = $env:WINDOWS_SIGNING_CERTIFICATE_PASSWORD,
    [string]$TimestampUrl = $(if ($env:WINDOWS_TIMESTAMP_URL) { $env:WINDOWS_TIMESTAMP_URL } else { "http://timestamp.digicert.com" }),
    [switch]$RequireSigning
)

$ErrorActionPreference = "Stop"

if ($Files -and $Files.Count -gt 0) {
    $signableFiles = @($Files | ForEach-Object { (Resolve-Path $_).Path })
}
elseif (-not [string]::IsNullOrWhiteSpace($ArtifactDirectory)) {
    $artifactPath = (Resolve-Path $ArtifactDirectory).Path
    $signableFiles = @(
        (Join-Path $artifactPath "FluxLauncher-Setup.exe"),
        (Join-Path $artifactPath "FluxLauncher-Portable.exe")
    ) | Where-Object { Test-Path $_ }
}
else {
    throw "Provide either ArtifactDirectory or Files."
}

$hasCertificate = -not [string]::IsNullOrWhiteSpace($CertificateBase64)
if (-not $hasCertificate) {
    if ($RequireSigning) {
        throw "Windows signing was required, but WINDOWS_SIGNING_CERTIFICATE_BASE64 is not configured."
    }
    Write-Host "No Windows signing certificate configured; leaving release artifacts unsigned."
    exit 0
}
if ([string]::IsNullOrWhiteSpace($CertificatePassword)) {
    throw "WINDOWS_SIGNING_CERTIFICATE_PASSWORD must be set when a signing certificate is configured."
}
if ($signableFiles.Count -eq 0) {
    throw "No signable Windows release artifacts were found."
}

$signtool = Get-ChildItem `
    -Path "${env:ProgramFiles(x86)}\Windows Kits\10\bin" `
    -Filter signtool.exe `
    -File `
    -Recurse `
    -ErrorAction SilentlyContinue |
    Sort-Object FullName -Descending |
    Select-Object -First 1
if ($null -eq $signtool) {
    throw "signtool.exe was not found in the Windows 10 SDK."
}

$certificatePath = Join-Path $env:RUNNER_TEMP "flux-signing-certificate.pfx"
try {
    [System.IO.File]::WriteAllBytes($certificatePath, [Convert]::FromBase64String($CertificateBase64))
    foreach ($file in $signableFiles) {
        Write-Host "Signing $file"
        & $signtool.FullName sign /fd SHA256 /f $certificatePath /p $CertificatePassword /tr $TimestampUrl /td SHA256 $file
        if ($LASTEXITCODE -ne 0) {
            throw "signtool failed for $file with exit code $LASTEXITCODE."
        }
    }
}
finally {
    Remove-Item $certificatePath -Force -ErrorAction SilentlyContinue
}

Write-Host "Signed $($signableFiles.Count) Windows release artifact(s) with Authenticode."

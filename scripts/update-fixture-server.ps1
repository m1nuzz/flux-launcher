param(
    [Parameter(Mandatory = $true)]
    [string]$Prefix,
    [Parameter(Mandatory = $true)]
    [string]$Root,
    [Parameter(Mandatory = $true)]
    [string]$TransitionFile
)

$ErrorActionPreference = "Stop"
$listener = [System.Net.HttpListener]::new()
$listener.Prefixes.Add($Prefix)
$listener.Start()
$count = 0

try {
    while ($listener.IsListening) {
        $context = $listener.GetContext()
        $relative = $context.Request.Url.AbsolutePath.TrimStart('/')
        if ($relative -eq 'latest') {
            $count++
            $name = if ($count -eq 1) { 'latest.json' } else { 'latest-done.json' }
            if ($count -eq 1) {
                New-Item -ItemType File -Force -Path $TransitionFile | Out-Null
            }
            $relative = $name
        }
        $path = Join-Path $Root ($relative -replace '/', '\')
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
            $context.Response.StatusCode = 404
            $context.Response.Close()
            continue
        }
        $bytes = [System.IO.File]::ReadAllBytes($path)
        $context.Response.StatusCode = 200
        $context.Response.ContentLength64 = $bytes.Length
        if ($path.EndsWith('.json')) {
            $context.Response.ContentType = 'application/json'
        } else {
            $context.Response.ContentType = 'application/octet-stream'
        }
        if ($path.EndsWith('.json')) {
            $context.Response.OutputStream.Write($bytes, 0, $bytes.Length)
        } else {
            foreach ($chunk in [System.Linq.Enumerable]::Chunk($bytes, 64KB)) {
                $context.Response.OutputStream.Write($chunk, 0, $chunk.Length)
                $context.Response.OutputStream.Flush()
                Start-Sleep -Milliseconds 100
            }
        }
        $context.Response.Close()
    }
}
finally {
    if ($listener.IsListening) {
        $listener.Stop()
    }
    $listener.Close()
}

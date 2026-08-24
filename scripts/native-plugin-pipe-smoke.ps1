[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$Executable,
    [Parameter(Mandatory = $true)]
    [string]$PluginRoot
)
$ErrorActionPreference = "Stop"
$pipeName = "flux-plugin-smoke-$PID"
$pipePath = "\\.\pipe\$pipeName"
$process = $null
$client = $null
try {
    $process = Start-Process -FilePath $Executable -ArgumentList @(
        "--plugin-host",
        $PluginRoot,
        $pipePath
    ) -PassThru -WindowStyle Hidden
    $client = [System.IO.Pipes.NamedPipeClientStream]::new(
        ".",
        $pipeName,
        [System.IO.Pipes.PipeDirection]::InOut,
        [System.IO.Pipes.PipeOptions]::None
    )
    $client.Connect(5000)
    $writer = [System.IO.StreamWriter]::new($client)
    $writer.AutoFlush = $true
    $reader = [System.IO.StreamReader]::new($client)
    $writer.WriteLine('{"jsonrpc":"2.0","id":1,"method":"query","params":{"query":"ex hello","action_keyword":"ex","locale":"en-US"}}')
    $queryResponse = $reader.ReadLine()
    $writer.WriteLine('{"jsonrpc":"2.0","id":2,"method":"execute","plugin":"Example Native","params":{"action":{"type":"copy_text","text":"hello"}}}')
    $executeResponse = $reader.ReadLine()
    if ($queryResponse -notmatch 'Example: hello') {
        throw "Named Pipe query response did not contain Example: hello: $queryResponse"
    }
    if ($executeResponse -notmatch '"success":true') {
        throw "Named Pipe execute response did not report success: $executeResponse"
    }
    $writer.Dispose()
    $reader.Dispose()
    $client.Dispose()
    $client = $null
    if (!$process.WaitForExit(5000)) {
        $process.Kill()
        throw "Native plugin host did not exit after Named Pipe client disconnected."
    }
    if ($process.ExitCode -ne 0) {
        throw "Native plugin host exited with code $($process.ExitCode)."
    }
    Write-Host "native plugin host Named Pipe smoke passed"
}
finally {
    if ($null -ne $client) { $client.Dispose() }
    if ($null -ne $process -and !$process.HasExited) { $process.Kill() }
}

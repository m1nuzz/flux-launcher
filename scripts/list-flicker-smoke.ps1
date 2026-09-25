#requires -Version 5.1
<#
.SYNOPSIS
    Regression smoke for result-list flicker.

.DESCRIPTION
    Types a query into a running-on-this-desktop Flux Launcher build at a fixed
    keystroke cadence while sampling the launcher's list area in memory every
    ~15 ms, and reads the app's own paint trace (FLUX_PAINT_TRACE_FILE).

    Two properties must hold while the keystrokes keep coming:
      * the result list is published at most twice per keystroke - once with the
        built-in and application results that must stay actionable immediately,
        and once when Everything answers - and
      * asynchronously arriving shell icons cause no visible repaint at all:
        they may only fill in after the query has been quiet.

    Violations exit non-zero and print the offending samples with the events that
    fell inside the same gap.

    Requires a desktop session: the launcher must be able to show a window and
    receive synthetic input. Run it the same way scripts/capture-mica.ps1 runs.

.PARAMETER Executable
    Path to the flux-launcher.exe under test (debug or release).

.PARAMETER Query
    Text typed one character at a time.

.PARAMETER InterKeyMs
    Delay between characters. Keep it below the app's icon quiet window
    (ICON_REFRESH_QUIET_MS) so the quiet case is not accidentally tested.

.PARAMETER QuietMs
    Minimum gap without a keystroke before icon arrivals are allowed to paint.
    Must match ICON_REFRESH_QUIET_MS in the build under test.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Executable,
    [string]$Query = 'launcher',
    [int]$InterKeyMs = 55,
    [int]$QuietMs = 120,
    [int]$SampleEveryMs = 15,
    [int]$RowHeight = 24,
    [int]$HeaderHeight = 56,
    [string]$OutDir = (Join-Path $env:TEMP 'flux-list-flicker')
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

if (-not (Test-Path $Executable)) { throw "Executable not found: $Executable" }
if ($InterKeyMs -ge $QuietMs) {
    throw "InterKeyMs ($InterKeyMs) must stay below QuietMs ($QuietMs) or the test measures the paused-typing case."
}
if (Test-Path $OutDir) { Remove-Item -Recurse -Force $OutDir }
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

$tracePath = Join-Path $OutDir 'paint.trace'
$scratchAppData = Join-Path $OutDir 'appdata'
New-Item -ItemType Directory -Force -Path $scratchAppData | Out-Null

Add-Type -Namespace Flicker -Name Input -MemberDefinition @'
[DllImport("user32.dll", CharSet=CharSet.Unicode)]
public static extern IntPtr SendMessage(IntPtr hWnd, uint Msg, IntPtr wParam, IntPtr lParam);
[DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
[DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);
[DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
[DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr arg);
[DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);
[DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
[DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extraInfo);
public delegate bool EnumProc(IntPtr hWnd, IntPtr arg);
public struct RECT { public int Left; public int Top; public int Right; public int Bottom; }
public static IntPtr FindByPid(uint targetPid) {
    IntPtr best = IntPtr.Zero;
    long bestArea = 0;
    EnumWindows((hWnd, arg) => {
        uint pid;
        GetWindowThreadProcessId(hWnd, out pid);
        if (pid == targetPid) {
            RECT r;
            GetWindowRect(hWnd, out r);
            long area = (long)(r.Right - r.Left) * (r.Bottom - r.Top);
            if (area > bestArea) { bestArea = area; best = hWnd; }
        }
        return true;
    }, IntPtr.Zero);
    return best;
}
'@

Add-Type -Namespace Flicker -Name Sampler -ReferencedAssemblies 'System.Drawing' -UsingNamespace 'System.Drawing','System.Drawing.Imaging','System.Text','System.Threading' -MemberDefinition @'
[StructLayout(LayoutKind.Sequential)]
public struct RECT { public int Left; public int Top; public int Right; public int Bottom; }

public static class Worker {
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);

    static byte[] Grab(IntPtr hWnd, out int height, out int stride) {
        RECT r;
        GetWindowRect(hWnd, out r);
        height = 0; stride = 0;
        int width = r.Right - r.Left;
        height = r.Bottom - r.Top;
        if (width <= 0 || height <= 0) { height = 0; return null; }
        using (Bitmap bmp = new Bitmap(width, height)) {
            using (Graphics g = Graphics.FromImage(bmp)) {
                g.CopyFromScreen(r.Left, r.Top, 0, 0, new Size(width, height));
            }
            BitmapData data = bmp.LockBits(new Rectangle(0, 0, width, height), ImageLockMode.ReadOnly, PixelFormat.Format32bppRgb);
            byte[] buffer = new byte[data.Stride * height];
            Marshal.Copy(data.Scan0, buffer, 0, buffer.Length);
            bmp.UnlockBits(data);
            stride = data.Stride;
            return buffer;
        }
    }

    public static string Run(IntPtr hWnd, int durationMs, int intervalMs, int header, int rowHeight) {
        StringBuilder report = new StringBuilder();
        byte[] previous = null;
        int previousHeight = 0, previousStride = 0;
        long previousUnix = 0;
        int deadline = Environment.TickCount + durationMs;
        while (true) {
            int remaining = deadline - Environment.TickCount;
            if (remaining <= 0) { break; }
            int height; int stride;
            byte[] current = Grab(hWnd, out height, out stride);
            long unix = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds();
            if (current != null && previous != null && stride == previousStride && height == previousHeight) {
                int bands = Math.Max(1, (height - header) / rowHeight);
                StringBuilder changed = new StringBuilder();
                for (int band = 0; band < bands; band++) {
                    int rowStart = header + band * rowHeight;
                    int rowEnd = Math.Min(height, rowStart + rowHeight);
                    long delta = 0;
                    for (int y = rowStart; y < rowEnd; y++) {
                        int line = y * stride;
                        for (int x = 0; x + 3 < stride; x += 4) {
                            delta += Math.Abs((int)current[line + x] - previous[line + x])
                                 + Math.Abs((int)current[line + x + 1] - previous[line + x + 1])
                                 + Math.Abs((int)current[line + x + 2] - previous[line + x + 2]);
                        }
                    }
                    double mean = delta / (double)Math.Max(1, (rowEnd - rowStart) * (stride / 4));
                    if (mean > 1.0) {
                        if (changed.Length > 0) { changed.Append(','); }
                        changed.Append(band).Append(':').Append(mean.ToString("0.0"));
                    }
                }
                if (changed.Length > 0) {
                    report.AppendLine("sample unix=" + unix + " after=" + (unix - previousUnix) + "ms bands=[" + changed + "]");
                }
            }
            previous = current; previousHeight = height; previousStride = stride; previousUnix = unix;
            Thread.Sleep(intervalMs);
        }
        return report.ToString();
    }
}
'@

function Get-TraceEvents {
    if (-not (Test-Path $tracePath)) { return @() }
    $parsed = @()
    foreach ($line in (Get-Content $tracePath)) {
        if ($line -match 'unix=(\d+) .*event=([a-z-]+) (.*)') {
            $parsed += [pscustomobject]@{
                unix   = [long]$matches[1]
                event  = $matches[2]
                detail = $matches[3]
            }
        }
    }
    return $parsed
}

$previousAppData = $env:APPDATA
$env:APPDATA = $scratchAppData
$env:FLUX_DISABLE_SINGLE_INSTANCE = '1'
$env:FLUX_DISABLE_UPDATE_CHECKS = '1'
$env:FLUX_DISABLE_EVERYTHING_PROMPT = '1'
$env:FLUX_PAINT_TRACE_FILE = $tracePath

$violations = @()
$process = Start-Process -FilePath $Executable -PassThru
Write-Host "launcher pid=$($process.Id) query=$Query inter_key=${InterKeyMs}ms quiet=${QuietMs}ms sample=${SampleEveryMs}ms"

try {
    $handle = [IntPtr]::Zero
    for ($attempt = 0; $attempt -lt 80; $attempt++) {
        $handle = [Flicker.Input]::FindByPid([uint32]$process.Id)
        if ($handle -ne [IntPtr]::Zero) { break }
        Start-Sleep -Milliseconds 250
    }
    if ($handle -eq [IntPtr]::Zero) { throw 'launcher window never appeared' }

    # Tray-resident start: WM_HOTKEY shows it deterministically, because the real
    # Alt+Space may already be registered by another Flux instance.
    [void][Flicker.Input]::SendMessage($handle, 0x0312, [IntPtr]::Zero, [IntPtr]::Zero)
    Start-Sleep -Milliseconds 600
    $rect = New-Object Flicker.Input+RECT
    [void][Flicker.Input]::GetWindowRect($handle, [ref]$rect)

    $shell = New-Object -ComObject WScript.Shell
    for ($attempt = 0; $attempt -lt 3; $attempt++) {
        # Focus probe: one character, and refuse to continue if the app did not see
        # it, so a focus loss cannot type the query into another window. Foreground
        # hand-off is racy on a busy desktop, hence the retry with a real click.
        [void][Flicker.Input]::SetForegroundWindow($handle)
        [void]$shell.AppActivate($process.Id)
        Start-Sleep -Milliseconds 300
        [void][Flicker.Input]::SetCursorPos($rect.Left + [int](($rect.Right - $rect.Left) / 2), $rect.Top + [int](($rect.Bottom - $rect.Top) / 2))
        [Flicker.Input]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
        [Flicker.Input]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
        Start-Sleep -Milliseconds 200
        [void][Flicker.Input]::SetForegroundWindow($handle)
        Start-Sleep -Milliseconds 150
        $shell.SendKeys('z')
        Start-Sleep -Milliseconds 400
        $probeSeen = @(Get-TraceEvents | Where-Object { $_.event -eq 'query' -and $_.detail -like 'value=z *' })
        $shell.SendKeys('{BACKSPACE}')
        Start-Sleep -Milliseconds 250
        if ($probeSeen.Count -gt 0) { break }
        Write-Host "focus attempt $($attempt + 1) did not reach the launcher, retrying"
    }
    if ($probeSeen.Count -eq 0) { throw 'typed characters never reached the launcher window' }

    $observations = @()
    $typed = ''
    foreach ($char in $Query.ToCharArray()) {
        $typed += [string]$char
        if (-not [Flicker.Input]::IsWindowVisible($handle)) {
            [void][Flicker.Input]::SendMessage($handle, 0x0312, [IntPtr]::Zero, [IntPtr]::Zero)
            Start-Sleep -Milliseconds 250
        }
        [void][Flicker.Input]::SetForegroundWindow($handle)
        [void]$shell.AppActivate($process.Id)
        Start-Sleep -Milliseconds 120
        $keyUnix = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
        $shell.SendKeys([string]$char)
        $samples = [Flicker.Sampler+Worker]::Run($handle, $InterKeyMs, $SampleEveryMs, $HeaderHeight, $RowHeight)
        $observations += [pscustomobject]@{ typed = $typed; char = $char; unix = $keyUnix; samples = $samples }
    }

    Write-Host ''
    Write-Host 'keystroke   list-writes  icon-paints'
    for ($index = 0; $index -lt $observations.Count; $index++) {
        $entry = $observations[$index]
        $from = $entry.unix
        # A keystroke owns everything up to the next keystroke; the final one is
        # watched through the quiet window so a late icon wave is still visible.
        if ($index -lt $observations.Count - 1) {
            $to = $observations[$index + 1].unix
        } else {
            $to = $from + $InterKeyMs + $QuietMs + 200
        }
        $events = @(Get-TraceEvents | Where-Object { $_.unix -ge $from -and $_.unix -lt $to })
        $writes = @($events | Where-Object { $_.event -eq 'list-write' })
        $iconLines = @($entry.samples -split "`r?`n" | Where-Object { $_ -like 'sample *' })
        $iconPaints = 0
        foreach ($line in $iconLines) {
            if ($line -match 'unix=(\d+) after=(\d+)ms') {
                $sampleUnix = [long]$matches[1]
                $gapStart = $sampleUnix - [int]$matches[2]
                $gapEvents = @($events | Where-Object { $_.unix -gt $gapStart -and $_.unix -le $sampleUnix })
                $iconish = @($gapEvents | Where-Object { $_.event -eq 'icon-refresh' })
                if ($iconish.Count -gt 0) { $iconPaints++ }
            }
        }
        Write-Host ("{0,-11} {1,11}  {2,11}" -f $entry.typed, $writes.Count, $iconPaints)
        # Two publishes per keystroke is the intended shape: built-in and application
        # results publish at once so a system command is actionable immediately, and
        # the complete snapshot replaces it when Everything answers. A third swap
        # means another provider is republishing the same generation.
        if ($writes.Count -gt 2) {
            $violations += "query '$($entry.typed)' published the list $($writes.Count) times while typing (expected at most 2)"
        }
        if ($iconPaints -gt 0 -and $entry.typed -ne $Query) {
            $violations += "query '$($entry.typed)' repainted $iconPaints time(s) from shell-icon arrivals before the query had been quiet for ${QuietMs}ms"
        }
    }
} finally {
    Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
    $env:APPDATA = $previousAppData
    Remove-Item Env:FLUX_DISABLE_SINGLE_INSTANCE -ErrorAction SilentlyContinue
    Remove-Item Env:FLUX_DISABLE_UPDATE_CHECKS -ErrorAction SilentlyContinue
    Remove-Item Env:FLUX_DISABLE_EVERYTHING_PROMPT -ErrorAction SilentlyContinue
    Remove-Item Env:FLUX_PAINT_TRACE_FILE -ErrorAction SilentlyContinue
}

Write-Host ''
Write-Host "trace and samples kept under: $OutDir"
if ($violations.Count -gt 0) {
    foreach ($violation in $violations) { Write-Host "FAIL: $violation" }
    throw "list flicker smoke failed with $($violations.Count) violation(s)"
}
Write-Host 'PASS: list publishes stayed within budget and no icon-driven repaint happened while typing.'

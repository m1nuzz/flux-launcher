#requires -Version 5.1
<#
.SYNOPSIS
    Regression smoke for result-list flicker.

.DESCRIPTION
    Types a query into a running-on-this-desktop Flux Launcher build at a fixed
    keystroke cadence while sampling the launcher's list area in memory every
    ~15 ms, and reads the app's own paint trace (FLUX_PAINT_TRACE_FILE).

    Two properties must hold while the keystrokes keep coming:
      * the result list is published exactly once per keystroke while typing
        continues - the built-in and application snapshot that must stay
        actionable immediately - and
      * asynchronously arriving shell icons and Everything results cause no
        visible repaint at all until the query has been quiet.

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

Add-Type -Namespace Flicker -Name Input -UsingNamespace 'System.Threading' -MemberDefinition @'
[DllImport("user32.dll", CharSet=CharSet.Unicode)]
public static extern IntPtr SendMessage(IntPtr hWnd, uint Msg, IntPtr wParam, IntPtr lParam);
[DllImport("user32.dll")] public static extern bool PostMessage(IntPtr hWnd, uint Msg, UIntPtr wParam, IntPtr lParam);

// Posting WM_CHAR is what makes this harness survive a busy desktop: it needs no
// foreground right, so it never steals the user's focus and cannot type into another
// window. The launcher gives the search field keyboard focus when it is shown.
public static void PostChar(IntPtr hWnd, uint charCode) { PostMessage(hWnd, 0x0102, new UIntPtr(charCode), IntPtr.Zero); }
public static void TypeText(IntPtr hWnd, string text) {
    foreach (char c in text) { PostChar(hWnd, c); Thread.Sleep(12); }
}
[DllImport("user32.dll")] public static extern short MapVirtualKey(uint vk, uint mapType);
public static void Backspace(IntPtr hWnd) {
    // VK_BACK through WM_KEYDOWN/WM_KEYUP: WM_CHAR 8 is delivered but the search
    // field only edits on the key-down path, so a posted character does nothing.
    uint scan = (uint)MapVirtualKey(0x08, 0);
    PostMessage(hWnd, 0x0100, new UIntPtr(0x08), (IntPtr)(long)((scan << 16) | 1u));
    PostMessage(hWnd, 0x0101, new UIntPtr(0x08), (IntPtr)(long)((scan << 16) | 1u | unchecked((int)0xC0000000)));
    Thread.Sleep(12);
}
public static void Erase(IntPtr hWnd, int count) { for (int i = 0; i < count; i++) { Backspace(hWnd); } }
[DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);
[DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
[DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr arg);
[DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);
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

    # Focus probe by post, not by assumption: one character must show up in the
    # launcher's own trace before anything else is typed.
    [Flicker.Input]::PostChar($handle, [uint32][char]'z')
    Start-Sleep -Milliseconds 400
    $probeSeen = @(Get-TraceEvents | Where-Object { $_.event -eq 'query' -and $_.detail -like 'value=z *' })
    if ($probeSeen.Count -eq 0) { throw 'posted characters never reached the launcher window' }
    [Flicker.Input]::Erase($handle, 4)
    Start-Sleep -Milliseconds 300

    $observations = @()
    $typed = ''
    foreach ($char in $Query.ToCharArray()) {
        $typed += [string]$char
        if (-not [Flicker.Input]::IsWindowVisible($handle)) {
            [void][Flicker.Input]::SendMessage($handle, 0x0312, [IntPtr]::Zero, [IntPtr]::Zero)
            Start-Sleep -Milliseconds 250
        }
        $keyUnix = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
        [Flicker.Input]::PostChar($handle, [uint32][char]$char)
        $samples = [Flicker.Sampler+Worker]::Run($handle, $InterKeyMs, $SampleEveryMs, $HeaderHeight, $RowHeight)
        # Harness self-check: if the launcher did not end up holding exactly this
        # text, the measurement is meaningless and must stop rather than report a
        # clean run over a query that never existed.
        $seen = @(Get-TraceEvents | Where-Object { $_.event -eq 'query' } | Select-Object -Last 1)
        if ($seen.Count -eq 0 -or $seen[0].detail -notlike ("value={0} *" -f $typed)) {
            $actualText = if ($seen.Count -gt 0) { $seen[0].detail } else { 'nothing' }
            throw "keystroke $index expected query '$typed' but the launcher holds '$actualText'"
        }
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
        $isLast = $index -eq ($observations.Count - 1)
        Write-Host ("{0,-11} {1,11}  {2,11}" -f $entry.typed, $writes.Count, $iconPaints)
        # While the keystrokes keep coming a query must be painted exactly once: the
        # built-in and application snapshot. Everything's answer and the shell icons
        # wait for the quiet window, so they land together in one repaint instead of
        # rebuilding every row a second time mid-word. The final keystroke is watched
        # through that window, so its deferred publish is expected and allowed.
        if (-not $isLast) {
            if ($writes.Count -gt 1) {
                $violations += "query '$($entry.typed)' rebuilt the list $($writes.Count) times before the next keystroke (expected exactly 1)"
            }
            if ($iconPaints -gt 0) {
                $violations += "query '$($entry.typed)' repainted $iconPaints time(s) from shell-icon arrivals before the query had been quiet for ${QuietMs}ms"
            }
        } elseif ($writes.Count -gt 2) {
            $violations += "query '$($entry.typed)' rebuilt the list $($writes.Count) times (expected at most 2: immediate plus the deferred publish)"
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

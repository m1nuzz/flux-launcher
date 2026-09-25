#requires -Version 5.1
<#
.SYNOPSIS
    Regression smoke for result-list flicker.

.DESCRIPTION
    Drives a Flux Launcher build on this desktop with posted window messages,
    samples the launcher's own surface every ~15 ms, and reads the app's paint
    trace (FLUX_PAINT_TRACE_FILE).

    Two phases are measured: typing a word letter by letter, then appending and
    deleting the same letters quickly - the repro where the panel collapsed to a
    single row and refilled, which reads as a full-list flash.

    A third phase acts on the list while it still shows an earlier keystroke's
    rows, which is the fast-typing case where Enter opened the previous prefix's
    top hit instead of what was typed.

    A fourth phase navigates to another row and then erases a character: the
    highlight must return to the first row of the shorter query instead of staying
    pinned to the row the arrows (or a recalled history entry) picked.

    While keystrokes keep coming, every step must hold:
      * at most one list publish (one row-tree rebuild),
      * no repaint caused by shell-icon arrivals before the query has been quiet,
      * no publish of a snapshot smaller than the rows on screen while a provider
        still owes its answer (that collapse-and-refill is the flash),
      * no force-publish from a keystroke that only edits the field: settling the
        panel is what Enter and the navigation keys do, never typing.

    The final step is watched through the quiet window, so its deferred publish is
    expected and only the rebuild budget is checked.

    Input is posted (WM_CHAR for text, WM_KEYDOWN for the backspace and the
    acting key) rather than typed as keystrokes: the launcher is translucent, so
    grabbing the foreground would type into whatever the user actually has focused.
    The surface is captured with PrintWindow because a screen grab also contains
    the desktop behind the panel.

.PARAMETER Executable
    Path to the flux-launcher.exe under test (debug or release).

.PARAMETER Query
    Text typed one character at a time.

.PARAMETER Toggle
    Characters appended and then deleted again in the second phase.

.PARAMETER InterKeyMs
    Delay between synthetic keystrokes. Keep it below -QuietMs.

.PARAMETER QuietMs
    Gap without a keystroke before deferred results may paint. Must match
    TYPING_QUIET_MS in the build under test.

.PARAMETER Settle
    Query used by the last phase, which acts on the list while an earlier
    keystroke is still on screen.

.PARAMETER SettleKeyMs
    Delay between characters of -Settle. Shorter than -InterKeyMs on purpose: the
    panel must still belong to an earlier keystroke when the action arrives.

.PARAMETER SettleActMs
    Base delay added to the sweep in -SettleRounds: attempt N acts N*10 ms after
    the final character. Keep it small - the phase measures the stale state on
    purpose.

.PARAMETER SettleRounds
    Attempts, because whether the panel is still stale when the action lands is a
    race with the providers. Every attempt that does catch a stale panel must
    settle it; an attempt that finds the panel fresh is reported and skipped.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Executable,
    [string]$Query = 'chatgpt',
    [string]$Toggle = 'pt',
    [int]$Rounds = 4,
    [int]$InterKeyMs = 150,
    [int]$QuietMs = 250,
    [int]$IconBudgetMs = 150,
    [string]$Settle = 'chat',
    [int]$SettleKeyMs = 25,
    [int]$SettleActMs = 0,
    [int]$SettleRounds = 6,
    [int]$SampleEveryMs = 15,
    [int]$RowHeight = 24,
    [int]$HeaderHeight = 56,
    [string]$OutDir = (Join-Path $env:TEMP 'flux-list-flicker'),
    [string]$SettingsFile = (Join-Path $env:APPDATA 'FluxLauncher\settings.json')
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
New-Item -ItemType Directory -Force -Path (Join-Path $scratchAppData 'FluxLauncher') | Out-Null

# The collapse this smoke guards is a race between the applications commit and
# Everything's, and it only happens for queries that already have built-in rows on
# screen - which means the profile matters. Seed the scratch profile from a real
# settings file (read only: the instance under test writes to the copy) so the run
# measures the same provider mix the user sees.
if ($SettingsFile -and (Test-Path $SettingsFile)) {
    Copy-Item -Path $SettingsFile -Destination (Join-Path $scratchAppData 'FluxLauncher\settings.json') -Force
    Write-Host "seeded profile from $SettingsFile"
} else {
    Write-Host 'no settings file to seed: running with default settings (built-in providers off)'
}

Add-Type -Namespace Flicker -Name Input -UsingNamespace 'System.Threading' -MemberDefinition @'
[DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr ctx);
[DllImport("user32.dll", CharSet=CharSet.Unicode)]
public static extern IntPtr SendMessage(IntPtr hWnd, uint Msg, IntPtr wParam, IntPtr lParam);
[DllImport("user32.dll")] public static extern bool PostMessage(IntPtr hWnd, uint Msg, UIntPtr wParam, IntPtr lParam);
[DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);
[DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
[DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr arg);
[DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);
[DllImport("user32.dll")] public static extern short MapVirtualKey(uint vk, uint mapType);
public delegate bool EnumProc(IntPtr hWnd, IntPtr arg);
public struct RECT { public int Left; public int Top; public int Right; public int Bottom; }

// Posting messages needs no foreground right, so the harness never steals the
// user's focus and cannot type into another window.
public static void PostVk(IntPtr hWnd, uint vk) {
    uint scan = (uint)MapVirtualKey(vk, 0);
    PostMessage(hWnd, 0x0100, new UIntPtr(vk), (IntPtr)(long)(scan << 16 | 1u));
    PostMessage(hWnd, 0x0101, new UIntPtr(vk), (IntPtr)(long)((scan << 16) | 1u | 0x40000000u | unchecked((int)0xC0000000)));
}
public static void TypeChar(IntPtr hWnd, char value) {
    // windui turns WM_CHAR into a `Key::Char` key event, so this reaches the same
    // handlers a physical keystroke does, including the one that settles a held
    // panel - which is how a keystroke that only edits the field can flash the list.
    PostMessage(hWnd, 0x0102, new UIntPtr((uint)value), IntPtr.Zero);
    Thread.Sleep(12);
}
public static void Backspace(IntPtr hWnd) {
    // The search field edits on the key-down path; a posted WM_CHAR backspace is
    // delivered and ignored, which once let this harness measure a query that
    // still carried its probe character.
    PostVk(hWnd, 0x08);
    Thread.Sleep(12);
}
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
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr hWnd, IntPtr hdc, uint flags);

    static byte[] Grab(IntPtr hWnd, out int height, out int stride) {
        RECT r;
        GetWindowRect(hWnd, out r);
        height = 0; stride = 0;
        int width = r.Right - r.Left;
        height = r.Bottom - r.Top;
        if (width <= 0 || height <= 0) { height = 0; return null; }
        using (Bitmap bmp = new Bitmap(width, height)) {
            using (Graphics g = Graphics.FromImage(bmp)) {
                // PW_RENDERFULLCONTENT: the panel is translucent and painted through
                // DirectComposition, so a screen grab would capture the desktop behind
                // it rather than the rows.
                IntPtr hdc = g.GetHdc();
                PrintWindow(hWnd, hdc, 2u);
                g.ReleaseHdc(hdc);
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
            if (deadline - Environment.TickCount <= 0) { break; }
            int height; int stride;
            byte[] current = Grab(hWnd, out height, out stride);
            long unix = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds();
            if (current != null && previous != null && stride == previousStride && height == previousHeight) {
                int bands = Math.Max(1, (height - header) / rowHeight);
                StringBuilder changed = new StringBuilder();
                int samples = 0;
                int moved = 0;
                for (int band = 0; band < bands; band++) {
                    int rowStart = header + band * rowHeight;
                    int rowEnd = Math.Min(height, rowStart + rowHeight);
                    long delta = 0;
                    int bandSamples = 0;
                    int bandMoved = 0;
                    for (int y = rowStart; y < rowEnd; y++) {
                        int line = y * stride;
                        for (int x = 0; x + 3 < stride; x += 4) {
                            int pixel = Math.Abs((int)current[line + x] - previous[line + x])
                                     + Math.Abs((int)current[line + x + 1] - previous[line + x + 1])
                                     + Math.Abs((int)current[line + x + 2] - previous[line + x + 2]);
                            delta += pixel;
                            bandSamples++;
                            if (pixel > 30) { bandMoved++; }
                        }
                    }
                    samples += bandSamples;
                    moved += bandMoved;
                    double mean = delta / (double)Math.Max(1, bandSamples);
                    if (mean > 1.0) {
                        if (changed.Length > 0) { changed.Append(','); }
                        changed.Append(band).Append(':').Append((100.0 * bandMoved / Math.Max(1, bandSamples)).ToString("0"));
                    }
                }
                if (changed.Length > 0) {
                    report.AppendLine("sample unix=" + unix + " after=" + (unix - previousUnix)
                        + "ms changed=" + (100.0 * moved / Math.Max(1, samples)).ToString("0.0")
                        + " bands=[" + changed + "]");
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

# The capture must see the same physical pixels the launcher writes.
[void][Flicker.Input]::SetProcessDpiAwarenessContext([IntPtr](-4))

$previousAppData = $env:APPDATA
$env:APPDATA = $scratchAppData
$env:FLUX_DISABLE_SINGLE_INSTANCE = '1'
$env:FLUX_DISABLE_UPDATE_CHECKS = '1'
$env:FLUX_DISABLE_EVERYTHING_PROMPT = '1'
$env:FLUX_PAINT_TRACE_FILE = $tracePath

$violations = @()
$script:observations = @()
$process = Start-Process -FilePath $Executable -PassThru
Write-Host "pid=$($process.Id) query=$Query toggle=$Toggle rounds=$Rounds inter_key=${InterKeyMs}ms quiet=${QuietMs}ms sample=${SampleEveryMs}ms"

function Add-Observation([string]$phase, [string]$label, [string]$after, [long]$unix, $samples) {
    # Harness self-check: the launcher must actually hold the text this harness
    # believes it typed, otherwise the run measures a query that never existed.
    $seen = @(Get-TraceEvents | Where-Object { $_.event -eq 'query' } | Select-Object -Last 1)
    $actual = 'nothing'
    if ($seen.Count -gt 0) { $actual = $seen[0].detail }
    if ($seen.Count -eq 0 -or $actual -notlike ("value={0} *" -f $after)) {
        throw "phase '$phase' step '$label': expected the launcher to hold '$after', it holds '$actual'"
    }
    $script:observations += [pscustomobject]@{
        phase = $phase; label = $label; after = $after; unix = $unix; samples = $samples
    }
}

try {
    $handle = [IntPtr]::Zero
    for ($attempt = 0; $attempt -lt 80; $attempt++) {
        $handle = [Flicker.Input]::FindByPid([uint32]$process.Id)
        if ($handle -ne [IntPtr]::Zero) { break }
        Start-Sleep -Milliseconds 250
    }
    if ($handle -eq [IntPtr]::Zero) { throw 'launcher window never appeared' }

    # Tray-resident start: WM_HOTKEY toggles it deterministically, because the real
    # Alt+Space may be registered by the user's own instance.
    [void][Flicker.Input]::SendMessage($handle, 0x0312, [IntPtr]::Zero, [IntPtr]::Zero)
    Start-Sleep -Milliseconds 700

    [Flicker.Input]::TypeChar($handle, 'z')
    Start-Sleep -Milliseconds 400
    if (@(Get-TraceEvents | Where-Object { $_.event -eq 'query' -and $_.detail -like 'value=z *' }).Count -eq 0) {
        throw 'posted characters never reached the launcher window'
    }
    for ($i = 0; $i -lt 4; $i++) { [Flicker.Input]::Backspace($handle) }
    Start-Sleep -Milliseconds 400

    $typed = ''
    foreach ($char in $Query.ToCharArray()) {
        $typed += [string]$char
        if (-not [Flicker.Input]::IsWindowVisible($handle)) {
            [void][Flicker.Input]::SendMessage($handle, 0x0312, [IntPtr]::Zero, [IntPtr]::Zero)
            Start-Sleep -Milliseconds 250
        }
        $unix = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
        [Flicker.Input]::TypeChar($handle, $char)
        $samples = [Flicker.Sampler+Worker]::Run($handle, $InterKeyMs, $SampleEveryMs, $HeaderHeight, $RowHeight)
        Add-Observation 'type' "add $char" $typed $unix $samples
    }

    $base = $Query
    if ($base.Length -gt $Toggle.Length) { $base = $base.Substring(0, $base.Length - $Toggle.Length) }
    # Walk back to the reported starting point: the field holds $Query here, and
    # each backspace removes the last character.
    $held = $Query
    for ($k = 0; $k -lt $Toggle.Length; $k++) {
        $held = $held.Substring(0, $held.Length - 1)
        $unix = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
        [Flicker.Input]::Backspace($handle)
        $samples = [Flicker.Sampler+Worker]::Run($handle, $InterKeyMs, $SampleEveryMs, $HeaderHeight, $RowHeight)
        Add-Observation 'settle' "drop $($k + 1)" $held $unix $samples
    }

    for ($round = 1; $round -le $Rounds; $round++) {
        $current = $base
        foreach ($char in $Toggle.ToCharArray()) {
            $current += [string]$char
            $unix = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
            [Flicker.Input]::TypeChar($handle, $char)
            $samples = [Flicker.Sampler+Worker]::Run($handle, $InterKeyMs, $SampleEveryMs, $HeaderHeight, $RowHeight)
            Add-Observation 'toggle' "r$round add $char" $current $unix $samples
        }
        for ($k = 0; $k -lt $Toggle.Length; $k++) {
            $current = $current.Substring(0, $current.Length - 1)
            $unix = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
            [Flicker.Input]::Backspace($handle)
            $samples = [Flicker.Sampler+Worker]::Run($handle, $InterKeyMs, $SampleEveryMs, $HeaderHeight, $RowHeight)
            Add-Observation 'toggle' "r$round delete" $current $unix $samples
        }
    }

    Write-Host ''
    Write-Host 'phase    step             list-writes   icon-paints   max-changed%'
    for ($index = 0; $index -lt $observations.Count; $index++) {
        $entry = $observations[$index]
        $from = $entry.unix
        if ($index -lt $observations.Count - 1) {
            $to = $observations[$index + 1].unix
        } else {
            $to = $from + $InterKeyMs + $QuietMs + 200
        }
        $events = @(Get-TraceEvents | Where-Object { $_.unix -ge $from -and $_.unix -lt $to })
        $writes = @($events | Where-Object { $_.event -eq 'list-write' })
        # The collapse this smoke exists for: publishing a snapshot smaller than
        # the rows already on screen while a provider still owes its answer, which
        # blanks most of the panel for a frame and refills it on the next.
        $premature = @($writes | Where-Object {
            $_.detail -match 'rows=(\d+) shown=(\d+) complete=0' -and [int]$matches[1] -lt [int]$matches[2]
        })
        $iconPaints = 0
        $maxChanged = 0.0
        foreach ($line in @($entry.samples -split "`r?`n" | Where-Object { $_ -like 'sample *' })) {
            if ($line -match 'unix=(\d+) after=(\d+)ms changed=([0-9.]+)') {
                $sampleUnix = [long]$matches[1]
                $changedPct = [double]$matches[3]
                if ($changedPct -gt $maxChanged) { $maxChanged = $changedPct }
                $gapStart = $sampleUnix - [int]$matches[2]
                $gapEvents = @($events | Where-Object { $_.unix -gt $gapStart -and $_.unix -le $sampleUnix })
                if (@($gapEvents | Where-Object { $_.event -eq 'icon-refresh' }).Count -gt 0) { $iconPaints++ }
            }
        }
        $isLast = $index -eq ($observations.Count - 1)
        Write-Host ("{0,-8} {1,-15} {2,11}   {3,11}   {4,12}" -f $entry.phase, $entry.label, $writes.Count, $iconPaints, [Math]::Round($maxChanged, 1))
        # These two steps type and erase characters, so nothing here is allowed to
        # publish a snapshot the panel then has to grow back: that collapse-and-refill
        # is the flash, and a keystroke that only edits the field must never trigger it.
        $resolves = @($events | Where-Object { $_.event -eq 'resolve' })
        if ($resolves.Count -gt 0) {
            $violations += "typing '$($entry.after)' acted on the list like Enter does ( $($resolves.Count) force-publish(es)), which repaints a half-filled snapshot"
        }
        $movedSelections = @($events | Where-Object { $_.event -eq 'select' })
        if ($movedSelections.Count -gt 0) {
            $violations += "typing '$($entry.after)' rewrote the row highlight every keystroke ( $($movedSelections.Count) write(s) on the selection signals), which repaints rows the user never moved"
        }
        if ($premature.Count -gt 0) {
            $violations += "query '$($entry.after)' painted $($premature.Count) snapshot(s) smaller than the rows on screen while a provider still owed its answer (the panel collapses and refills)"
        }
        if (-not $isLast) {
            if ($writes.Count -gt 1) {
                $violations += "query '$($entry.after)' rebuilt the list $($writes.Count) times before the next keystroke (expected exactly 1)"
            }
            # One icon repaint per keystroke is the point: the page of icons is
            # propagated once the icon thread has drained, so a new query gains its
            # icons promptly without one full-panel repaint per icon.
            if ($iconPaints -gt 1) {
                $violations += "query '$($entry.after)' was repainted $iconPaints times by shell-icon arrivals (expected at most 1: one page, one repaint)"
            }
            # A cold page must reach the screen without waiting for the typing to
            # stop: measure the gap between the last icon the thread loaded and the
            # refresh that propagated it. Nothing was loaded in this window means
            # the page was already cached, which is not a delay. The default budget
            # is above the ~100 ms the first page of a run can wait for the tick that
            # polls it, and far below the 207-771 ms the held-back propagation cost.
            $allEvents = @(Get-TraceEvents)
            $loads = @($allEvents | Where-Object { $_.event -eq 'icon-loaded' -and $_.unix -ge $from -and $_.unix -lt $to })
            if ($loads.Count -gt 0) {
                $lastLoad = ($loads | Measure-Object -Property unix -Maximum).Maximum
                $refresh = @($allEvents | Where-Object { $_.event -eq 'icon-refresh' -and $_.unix -ge $lastLoad } | Select-Object -First 1)
                if ($refresh.Count -eq 0) {
                    $violations += "query '$($entry.after)' loaded $($loads.Count) icon(s) that were never propagated to the screen"
                } else {
                    $delay = $refresh[0].unix - $lastLoad
                    Write-Host ("           icons: loaded={0} propagated {1}ms after the last load" -f $loads.Count, $delay)
                    if ($delay -gt $IconBudgetMs) {
                        $violations += "query '$($entry.after)' showed its icons $delay ms after the last one finished loading (budget ${IconBudgetMs}ms)"
                    }
                }
            }
        }
    }
    Write-Host ''
    Write-Host 'settle: act on a panel that still shows an earlier keystroke'
    # Deliberately faster than the typing phases and repeated: whether the panel is
    # still stale when the action lands is a race with Everything, so several attempts
    # measure the case the user actually hits - typing a word and acting on it in the
    # same breath.
    $stale = 0
    for ($attempt = 1; $attempt -le $SettleRounds; $attempt++) {
        # Sweep the delay instead of pinning it: the state this phase needs - the
        # applications provider has answered the new character, Everything has not -
        # lasts tens of milliseconds, and which attempt lands inside it depends on
        # how warm the Everything index is right now.
        $actDelayMs = $SettleActMs + $attempt * 10
        for ($k = 0; $k -lt ($Settle.Length + 4); $k++) { [Flicker.Input]::Backspace($handle) }
        Start-Sleep -Milliseconds 300
        foreach ($char in $Settle.ToCharArray()) {
            [Flicker.Input]::TypeChar($handle, $char)
            Start-Sleep -Milliseconds $SettleKeyMs
        }
        Start-Sleep -Milliseconds $actDelayMs
        # VK_DOWN moves the highlight without opening anything: the keystroke still
        # runs the whole action path, which is the path that must not act on a row
        # the user never asked for. The stamp is taken around the post, so what counts
        # as "on screen when the user acted" is the state the handler itself sees.
        $keyUnix = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
        [Flicker.Input]::PostVk($handle, 0x28)
        $keyUnix = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
        Start-Sleep -Milliseconds 700
        $allEvents = @(Get-TraceEvents)
        if (@($allEvents | Where-Object { $_.event -eq 'query' -and $_.detail -like "value=$Settle *" }).Count -eq 0) {
            throw "phase 'settle' attempt $attempt : the launcher never held '$Settle', the posted characters were lost"
        }
        $before = @($allEvents | Where-Object { $_.event -eq 'list-write' -and $_.unix -le $keyUnix } | Select-Object -Last 1)
        $painted = 'nothing'
        if ($before.Count -gt 0 -and $before[0].detail -match 'query=(\S+)') { $painted = $matches[1] }
        $resolves = @($allEvents | Where-Object { $_.event -eq 'resolve' -and $_.unix -ge $keyUnix })
        $settledWrites = @($allEvents | Where-Object {
            $_.event -eq 'list-write' -and $_.unix -ge $keyUnix -and $_.unix -le ($keyUnix + 80) -and $_.detail -like "query=$Settle *"
        })
        $seen = 'fresh'
        if ($painted -ne $Settle) { $stale++; $seen = "stale('$painted')" }
        Write-Host ("         attempt {0}: panel {1} for '{2}'; keystroke +{3}ms -> {4} resolve, {5} repaint of '{2}'" -f $attempt, $seen, $Settle, $actDelayMs, $resolves.Count, $settledWrites.Count)
        foreach ($entry in $resolves) {
            Write-Host "           $($entry.detail)"
            if ($entry.detail -notmatch "query=$Settle ") {
                $violations += "acting on the stale '$painted' panel while '$Settle' was typed resolved against a different query"
            }
            if ($entry.detail -notmatch 'rows=[1-9]') {
                $violations += "settling '$Settle' left the panel empty, which turns the keystroke into a silent no-op"
            }
        }
        if ($painted -ne $Settle -and $settledWrites.Count -eq 0) {
            # The rows on screen belong to an earlier keystroke and nothing painted
            # the typed text around the action, so Enter would have opened the
            # previous keystroke's top hit - the reported bug.
            $violations += "acting on the stale '$painted' panel while '$Settle' was typed left '$painted' rows on screen (Enter would have launched one of them)"
        }
    }
    if ($stale -eq 0) {
        Write-Host '         note: every provider answered before each keystroke, so no attempt caught a stale panel'
    } else {
        Write-Host "         $stale of $SettleRounds attempts caught the panel showing an earlier keystroke"
    }
    Write-Host ''
    Write-Host 'recall: editing the text must not keep a navigated row highlighted'
    for ($k = 0; $k -lt ($Settle.Length + 6); $k++) { [Flicker.Input]::Backspace($handle) }
    Start-Sleep -Milliseconds 400
    foreach ($char in $Settle.ToCharArray()) {
        [Flicker.Input]::TypeChar($handle, $char)
        Start-Sleep -Milliseconds $InterKeyMs
    }
    Start-Sleep -Milliseconds $QuietMs
    function Get-HeadAfterEdit([string]$label, [string]$expect, [long]$since) {
        # The end-to-end form of his report: after the query got shorter, the row the
        # app calls selected must be the row the new list actually starts with. The
        # violation is returned rather than appended, because a function cannot write
        # to the caller's $violations and a silently dropped one would always pass.
        $writes = @(Get-TraceEvents | Where-Object {
            $_.event -eq 'list-write' -and $_.unix -ge $since -and $_.detail -like "query=$expect *"
        })
        if ($writes.Count -eq 0) {
            Write-Host "         ${label}: no publish of '$expect' yet, nothing to compare"
            return $null
        }
        $match = [regex]::Match($writes[-1].detail, 'head=(?<head>.*?) selected=(?<sel>.*)@(?<idx>\d+)$')
        if (-not $match.Success) {
            Write-Host "         ${label}: could not read head/selected out of '$($writes[-1].detail)'"
            return $null
        }
        $onHead = $match.Groups['sel'].Value -eq $match.Groups['head'].Value
        Write-Host ("         {0}: query='{1}' selected_index={2} on_head={3}" -f $label, $expect, $match.Groups['idx'].Value, $onHead)
        if ($onHead) { return $null }
        return "${label}: after erasing a character the highlight stayed on '$($match.Groups['sel'].Value)' instead of the top hit of '$expect'"
    }
    # Step A - move the highlight with the arrow keys, then erase one character: the
    # list belongs to the shorter query now, so the highlight has to return to row 0.
    [Flicker.Input]::PostVk($handle, 0x28)
    Start-Sleep -Milliseconds 80
    [Flicker.Input]::PostVk($handle, 0x28)
    Start-Sleep -Milliseconds 80
    $eraseUnix = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
    [Flicker.Input]::Backspace($handle)
    Start-Sleep -Milliseconds 900
    $moved = @(Get-TraceEvents | Where-Object { $_.event -eq 'select' -and $_.unix -ge $eraseUnix })
    if ($moved.Count -eq 0) {
        $violations += "erasing one character of '$Settle' kept the highlight on the row that was navigated to (a recalled or arrow-picked row must return to the top)"
    }
    foreach ($entry in $moved) {
        if ($entry.detail -notmatch 'index=0') {
            $violations += "erasing one character moved the highlight to an index other than 0: $($entry.detail)"
        }
    }
    $problem = Get-HeadAfterEdit 'arrows+erase' ($Settle.Substring(0, $Settle.Length - 1)) $eraseUnix
    if ($problem) { $violations += $problem }
    # Step B - his actual flow: recall a query from the history (plain Up on an empty
    # field takes the newest one, which is the same code path Alt+Up uses) and erase a
    # character of the recalled text.
    for ($k = 0; $k -lt ($Settle.Length + 6); $k++) { [Flicker.Input]::Backspace($handle) }
    Start-Sleep -Milliseconds 500
    [Flicker.Input]::PostVk($handle, 0x26)
    Start-Sleep -Milliseconds 900
    $recalled = @((Get-TraceEvents | Where-Object { $_.event -eq 'query' } | Select-Object -Last 1))
    if ($recalled.Count -eq 0 -or $recalled[0].detail -notmatch '^value=(\S+) ') {
        throw "phase 'recall': the field text after Up could not be read"
    }
    $recalledQuery = $recalled[0].detail -replace '^value=(\S+) .*', '$1'
    if ($recalledQuery.Length -lt 2) {
        Write-Host "         recall+erase: skipped, the profile recalled '$recalledQuery' (needs a query of two or more characters)"
    } else {
        $recallEraseUnix = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
        [Flicker.Input]::Backspace($handle)
        Start-Sleep -Milliseconds 900
        $problem = Get-HeadAfterEdit 'recall+erase' ($recalledQuery.Substring(0, $recalledQuery.Length - 1)) $recallEraseUnix
        if ($problem) { $violations += $problem }
    }
} finally {
    Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
    $env:APPDATA = $previousAppData
    Remove-Item Env:FLUX_DISABLE_SINGLE_INSTANCE -ErrorAction SilentlyContinue
    Remove-Item Env:FLUX_DISABLE_UPDATE_CHECKS -ErrorAction SilentlyContinue
    Remove-Item Env:FLUX_DISABLE_EVERYTHING_PROMPT -ErrorAction SilentlyContinue
    Remove-Item Env:FLUX_PAINT_TRACE_FILE -ErrorAction SilentlyContinue
    Write-Host ''
    Write-Host "trace and samples kept under: $OutDir"
}

if ($violations.Count -gt 0) {
    foreach ($violation in $violations) { Write-Host "FAIL: $violation" }
    throw "list flicker smoke failed with $($violations.Count) violation(s)"
}
Write-Host 'PASS: one list publish per keystroke, at most one icon repaint, and icons reach the screen within budget.'

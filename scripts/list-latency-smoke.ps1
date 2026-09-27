<#
.SYNOPSIS
Times how fast a launcher build answers typed text, and compares two builds with the
same instrument.

.DESCRIPTION
The flicker smoke reads the launcher's own paint trace, which only exists on builds that
carry it, so it cannot time `main` or any other binary. This script needs nothing but the
executable: it types text at a fixed cadence and samples the window with
PrintWindow(PW_RENDERFULLCONTENT) - the same capture the flicker smoke uses, because the
panel is translucent and a screen grab would photograph the desktop behind it.

Per keystroke it reports how long the panel took to show a change, how many times it
repainted before the next key, and whether it changed size. Per query it reports how many
keystrokes were answered, the median and worst latency, and whether the panel ended in the
state it was in before the query - which is how a build that answers a path with an empty
panel shows up here. It also reports what the process cost: private bytes idle, at peak and
at the end, working set, CPU time for the workload, threads and handles.

`-Compare` runs a second executable through the identical sequence and prints the two side
by side with the difference, so a refactor can be held to numbers instead of adjectives.

Everything is printed; only the latency budget is asserted, because that is the one thing a
user feels on every keystroke.

.EXAMPLE
scripts/list-latency-smoke.ps1 -Executable target\debug\flux-launcher.exe -QueryList 'chatgpt;steam;1+1'

.EXAMPLE
scripts/list-latency-smoke.ps1 -Executable target\debug\flux-launcher.exe `
    -Compare C:\tmp\main\target\debug\flux-launcher.exe -Rounds 2
#>
param(
    [Parameter(Mandatory = $true)][string]$Executable,
    [string]$Compare,
    [switch]$AssertCompare,
    [string]$QueryList = 'chatgpt;steam;1+1;2026-08;cmd',
    [int]$InterKeyMs = 70,
    [int]$SampleMs = 15,
    [int]$Rounds = 1,
    [int]$LatencyBudgetMs = 150,
    [int]$IdleBeforeMs = 0,
    [string]$OutDir = (Join-Path $env:TEMP 'flux-list-latency'),
    [string]$SettingsFile = (Join-Path $env:APPDATA 'FluxLauncher\settings.json')
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

if (-not (Test-Path $Executable)) { throw "Executable not found: $Executable" }
if ($Compare -and -not (Test-Path $Compare)) { throw "Compare executable not found: $Compare" }
if ($InterKeyMs -le $SampleMs) { throw "InterKeyMs ($InterKeyMs) must exceed SampleMs ($SampleMs)" }

# Self-contained capture: posting keys and sampling from one thread, so no second runspace
# and no dependency on the launcher's internals. The posted WM_CHAR reaches the same
# handlers a physical key does; the erase goes through WM_KEYDOWN with a scan code because
# the field edits on the key-down path.
$helper = @'
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Drawing;
using System.Drawing.Imaging;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;

[StructLayout(LayoutKind.Sequential)]
public struct RECT { public int Left; public int Top; public int Right; public int Bottom; }

public static class LatencyProbe {
    [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr ctx);
    [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr hWnd, uint Msg, IntPtr wParam, IntPtr lParam);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr hWnd, uint Msg, UIntPtr wParam, IntPtr lParam);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr hWnd, IntPtr hdc, uint flags);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr arg);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);
    [DllImport("user32.dll")] public static extern uint MapVirtualKey(uint vk, uint mapType);
    public delegate bool EnumProc(IntPtr hWnd, IntPtr arg);

    public static void DpiAware() { SetProcessDpiAwarenessContext(new IntPtr(-4)); }

    public static void Erase(IntPtr h) {
        uint scan = MapVirtualKey(0x08, 0);
        PostMessage(h, 0x0100, new UIntPtr(0x08), new IntPtr((long)(scan << 16 | 1u)));
        PostMessage(h, 0x0101, new UIntPtr(0x08), new IntPtr((long)((scan << 16) | 1u | 0x40000000u | unchecked((int)0xC0000000u))));
        Thread.Sleep(12);
    }
    public static void TypeChar(IntPtr h, char c) {
        PostMessage(h, 0x0102, new UIntPtr((uint)c), IntPtr.Zero);
        Thread.Sleep(12);
    }
    public static IntPtr FindByPid(uint pid) {
        IntPtr best = IntPtr.Zero; long area = 0;
        EnumWindows(delegate(IntPtr w, IntPtr a) {
            uint p; GetWindowThreadProcessId(w, out p);
            if (p == pid) {
                RECT r; GetWindowRect(w, out r);
                long s = (long)(r.Right - r.Left) * (r.Bottom - r.Top);
                if (s > area) { area = s; best = w; }
            }
            return true;
        }, IntPtr.Zero);
        return best;
    }
    public static bool Visible(IntPtr h) { return IsWindowVisible(h); }
    public static void Show(IntPtr h) { SendMessage(h, 0x0111, new IntPtr(0xF230), IntPtr.Zero); }
    public static string Size(IntPtr h) {
        RECT r; GetWindowRect(h, out r);
        return (r.Right - r.Left) + "x" + (r.Bottom - r.Top);
    }

    static byte[] Grab(IntPtr h, out int height, out int stride) {
        RECT r; GetWindowRect(h, out r);
        int w = r.Right - r.Left; height = r.Bottom - r.Top; stride = w;
        if (w <= 0 || height <= 0) { height = 0; return null; }
        using (Bitmap bmp = new Bitmap(w, height)) {
            using (Graphics g = Graphics.FromImage(bmp)) {
                IntPtr hdc = g.GetHdc();
                PrintWindow(h, hdc, 2u);
                g.ReleaseHdc(hdc);
            }
            BitmapData data = bmp.LockBits(new Rectangle(0, 0, w, height), ImageLockMode.ReadOnly, PixelFormat.Format32bppRgb);
            byte[] buffer = new byte[data.Stride * height];
            Marshal.Copy(data.Scan0, buffer, 0, buffer.Length);
            bmp.UnlockBits(data);
            stride = data.Stride;
            return buffer;
        }
    }

    // Percentage of pixels that moved. -1 means the window changed size, which is not a
    // repaint and must not be counted as one: the branch resizes its panel to the row count.
    static double Changed(byte[] a, byte[] b) {
        if (a == null || b == null || a.Length != b.Length) return -1.0;
        int diff = 0, samples = 0;
        for (int i = 0; i + 3 < a.Length; i += 4) {
            samples++;
            int d = Math.Abs((int)a[i] - b[i]) + Math.Abs((int)a[i + 1] - b[i + 1]) + Math.Abs((int)a[i + 2] - b[i + 2]);
            if (d > 30) diff++;
        }
        return samples == 0 ? 0 : 100.0 * diff / samples;
    }

    // One query. Clears the field, types it, and for every keystroke reports the time to
    // the first visible change, the repaints, and the resizes before the next key.
    // `idle` is the frame before the query started: a panel that ends looking like that
    // showed nothing, whatever it published.
    public static string RunQuery(IntPtr h, string query, int interKeyMs, int sampleMs) {
        for (int k = 0; k < 60; k++) { Erase(h); }
        Thread.Sleep(700);
        if (!Visible(h)) { Show(h); Thread.Sleep(400); }
        int ih, is_;
        byte[] idle = Grab(h, out ih, out is_);
        StringBuilder log = new StringBuilder();
        for (int idx = 0; idx < query.Length; idx++) {
            Stopwatch k = Stopwatch.StartNew();
            int ph, ps;
            byte[] prev = Grab(h, out ph, out ps);
            TypeChar(h, query[idx]);
            int firstChange = -1, repaints = 0, resizes = 0, slept = 0;
            double maxPct = 0;
            while (slept < interKeyMs) {
                Thread.Sleep(sampleMs); slept += sampleMs;
                int ch, cs;
                byte[] cur = Grab(h, out ch, out cs);
                if (cur == null) { continue; }
                double pct = Changed(prev, cur);
                if (pct < 0) { resizes++; }
                else if (pct > 0.05) {
                    repaints++;
                    if (firstChange < 0) { firstChange = (int)k.ElapsedMilliseconds; }
                    if (pct > maxPct) { maxPct = pct; }
                }
                prev = cur;
            }
            log.Append(idx).Append('|').Append(firstChange).Append('|').Append(repaints)
               .Append('|').Append(Math.Round(maxPct, 1)).Append('|').Append(resizes).Append(';');
        }
        int lh, ls;
        byte[] last = Grab(h, out lh, out ls);
        double endedBlank = Changed(idle, last);
        log.Append("#endedBlank=").Append(endedBlank < 0 ? -1 : Math.Round(endedBlank, 2));
        return log.ToString();
    }
}
'@
Add-Type -TypeDefinition $helper -ReferencedAssemblies 'System.Drawing'
[LatencyProbe]::DpiAware()

$queries = @($QueryList -split ';' | Where-Object { $_ -ne '' })
$violations = New-Object System.Collections.Generic.List[string]
$reports = @{}

function Invoke-Build([string]$exe, [string]$label) {
    $root = Join-Path $OutDir $label
    if (Test-Path $root) { Remove-Item -Recurse -Force $root }
    New-Item -ItemType Directory -Force -Path (Join-Path $root 'appdata\FluxLauncher') | Out-Null
    # The application catalog reads the user Start Menu from %APPDATA%, so the scratch
    # profile mirrors it: otherwise every per-user app vanishes from the measured catalog.
    $realStartMenu = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu'
    $scratchStartMenu = Join-Path $root 'appdata\Microsoft\Windows\Start Menu'
    if (Test-Path $realStartMenu) {
        New-Item -ItemType Directory -Force -Path (Split-Path $scratchStartMenu) | Out-Null
        if (-not (Test-Path $scratchStartMenu)) { & cmd /c mklink /J "$scratchStartMenu" "$realStartMenu" | Out-Null }
    }
    if ($SettingsFile -and (Test-Path $SettingsFile)) {
        Copy-Item -Path $SettingsFile -Destination (Join-Path $root 'appdata\FluxLauncher\settings.json') -Force
    }
    $previousAppData = $env:APPDATA
    $env:APPDATA = Join-Path $root 'appdata'
    $env:FLUX_DISABLE_SINGLE_INSTANCE = '1'
    $env:FLUX_DISABLE_UPDATE_CHECKS = '1'
    $env:FLUX_DISABLE_EVERYTHING_PROMPT = '1'
    $process = Start-Process -FilePath $exe -PassThru
    try {
        $handle = [IntPtr]::Zero
        for ($wait = 0; $wait -lt 150 -and $handle -eq [IntPtr]::Zero; $wait++) {
            Start-Sleep -Milliseconds 100
            $handle = [LatencyProbe]::FindByPid([uint32]$process.Id)
        }
        if ($handle -eq [IntPtr]::Zero) { throw "$label : the launcher window never appeared" }
        # A cold process is what the owner sees on the first open; a warmed one is what he
        # sees later. The two differ by the idle icon warm-up, which is worth separating
        # before blaming the search path for either number.
        if ($IdleBeforeMs -gt 0) {
            Write-Host "   ($label : idling $IdleBeforeMs ms before measuring)"
            Start-Sleep -Milliseconds $IdleBeforeMs
        }
        Start-Sleep -Seconds 3
        $process.Refresh()
        $idle = [pscustomobject]@{
            privateBytes = $process.PrivateMemorySize64
            workingSet = $process.WorkingSet64
            threads = $process.Threads.Count
            handles = $process.HandleCount
            cpuMs = $process.TotalProcessorTime.TotalMilliseconds
        }
        $peak = $idle.privateBytes
        $rows = @()
        foreach ($round in 1..$Rounds) {
            foreach ($query in $queries) {
                $watch = [Diagnostics.Stopwatch]::StartNew()
                $raw = [LatencyProbe]::RunQuery($handle, $query, $InterKeyMs, $SampleMs)
                $watch.Stop()
                $process.Refresh()
                if ($process.PrivateMemorySize64 -gt $peak) { $peak = $process.PrivateMemorySize64 }
                $latency = @(); $repaints = @(); $resizes = @(); $changed = @()
                foreach ($part in ($raw -split ';')) {
                    if ($part -match '^(?<i>\d+)\|(?<f>-?\d+)\|(?<r>\d+)\|(?<m>[\d.]+)\|(?<z>\d+)$') {
                        $latency += [int]$Matches['f']; $repaints += [int]$Matches['r']
                        $changed += [double]$Matches['m']; $resizes += [int]$Matches['z']
                    }
                }
                $endedBlank = if ($raw -match '#endedBlank=(?<b>-?[\d.]+)') { [double]$Matches['b'] } else { -1 }
                $answered = @($latency | Where-Object { $_ -ge 0 })
                $rows += [pscustomobject]@{
                    build = $label
                    round = $round
                    query = $query
                    keys = $query.Length
                    answered = $answered.Count
                    medianMs = if ($answered.Count) { ($answered | Sort-Object)[[int]($answered.Count / 2)] } else { -1 }
                    maxMs = if ($answered.Count) { ($answered | Measure-Object -Maximum).Maximum } else { -1 }
                    repaints = ($repaints | Measure-Object -Sum).Sum
                    resizes = ($resizes | Measure-Object -Sum).Sum
                    maxChangedPct = if ($changed.Count) { ($changed | Measure-Object -Maximum).Maximum } else { 0 }
                    endedBlankPct = $endedBlank
                    window = [LatencyProbe]::Size($handle)
                    wallMs = $watch.ElapsedMilliseconds
                    perKeystrokeMs = ($latency -join ',')
                }
            }
        }
        $process.Refresh()
        $final = [pscustomobject]@{
            privateBytes = $process.PrivateMemorySize64
            workingSet = $process.WorkingSet64
            cpuMs = $process.TotalProcessorTime.TotalMilliseconds
            peakPrivateBytes = $peak
        }
        return [pscustomobject]@{ label = $label; exe = $exe; idle = $idle; final = $final; rows = $rows }
    } finally {
        Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
        $env:APPDATA = $previousAppData
        Remove-Item Env:FLUX_DISABLE_SINGLE_INSTANCE -ErrorAction SilentlyContinue
        Remove-Item Env:FLUX_DISABLE_UPDATE_CHECKS -ErrorAction SilentlyContinue
        Remove-Item Env:FLUX_DISABLE_EVERYTHING_PROMPT -ErrorAction SilentlyContinue
    }
}

Write-Host "latency probe: $InterKeyMs ms between characters, sampled every $SampleMs ms, $Rounds round(s)"
Write-Host "queries: $($queries -join ' | ')"
$reports['build'] = Invoke-Build $Executable 'build'
if ($Compare) { $reports['compare'] = Invoke-Build $Compare 'compare' }

$labels = @($reports.Keys | Sort-Object)
foreach ($key in $labels) {
    $report = $reports[$key]
    Write-Host ''
    Write-Host ("{0} ({1})" -f $report.label, $report.exe)
    Write-Host ("   resources: idle private {0:N0} B, peak {1:N0} B, final {2:N0} B, working set {3:N0} B" -f `
        $report.idle.privateBytes, $report.final.peakPrivateBytes, $report.final.privateBytes, $report.idle.workingSet)
    Write-Host ("               CPU for the workload {0:N0} ms, idle CPU {1:N0} ms, threads {2}, handles {3}" -f `
        ($report.final.cpuMs - $report.idle.cpuMs), $report.idle.cpuMs, $report.idle.threads, $report.idle.handles)
    Write-Host ''
    Write-Host '   round  query               keys  answered  medianMs  maxMs  repaints  resizes  endedBlank%  window'
    foreach ($row in $report.rows) {
        Write-Host ("   {0,-6} {1,-18} {2,4}  {3,8}  {4,9}  {5,6}  {6,9}  {7,8}  {8,11}  {9}" -f `
            $row.round, $row.query, $row.keys, $row.answered, $row.medianMs, $row.maxMs,
            $row.repaints, $row.resizes, $row.endedBlankPct, $row.window)
    }
    Write-Host ''
    Write-Host '   per keystroke, ms from the keypress to the first visible change (- = no change needed):'
    foreach ($row in $report.rows) {
        Write-Host ("   {0,-6} {1,-18} {2}" -f $row.round, $row.query, $row.perKeystrokeMs)
    }
    foreach ($row in $report.rows) {
        if ($row.maxMs -gt $LatencyBudgetMs) {
            if ($key -eq 'compare' -and -not $AssertCompare) { continue }
            $violations.Add("$($report.label): '$($row.query)' took $($row.maxMs) ms to answer a keystroke (budget ${LatencyBudgetMs}ms)")
        }
    }}

if ($Compare) {
    Write-Host ''
    Write-Host 'side by side (build minus compare)'
    Write-Host '   query               answered        medianMs          repaints        private idle'
    $b = @($reports['build'].rows | Where-Object { $_.round -eq 1 })
    $c = @($reports['compare'].rows | Where-Object { $_.round -eq 1 })
    foreach ($row in $b) {
        $other = $c | Where-Object { $_.query -eq $row.query } | Select-Object -First 1
        if ($null -eq $other) { continue }
        Write-Host ("   {0,-18} {1,6} vs {2,-6} {3,9} vs {4,-6} {5,9} vs {6,-6} {7,10} vs {8,-10:N0}" -f `
            $row.query, $row.answered, $other.answered, $row.medianMs, $other.medianMs,
            $row.repaints, $other.repaints,
            $reports['build'].idle.privateBytes, $reports['compare'].idle.privateBytes)
    }
}

Write-Host ''
Write-Host "samples and scratch profiles kept under: $OutDir"
if ($violations.Count -gt 0) {
    foreach ($violation in $violations) { Write-Host "FAIL: $violation" }
    throw "list latency smoke failed with $($violations.Count) violation(s)"
}
Write-Host "PASS: every answered keystroke painted within ${LatencyBudgetMs}ms."

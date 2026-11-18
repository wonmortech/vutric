# M1 desktop UI spike harness (ADR-0030). Run in the interactive session (schtasks /IT), not in session 0.
# For each app and run: launch, time to first visible window, time to the app's own first frame, process-tree RAM
# while open, the app's frame-interval percentiles, time for the whole tree to exit after WM_CLOSE.
param(
    [Parameter(Mandatory)] [string] $Bin,
    [Parameter(Mandatory)] [string] $Out,
    [int] $Runs = 5,
    [int] $Frames = 600
)
$ErrorActionPreference = 'Stop'
Start-Transcript -Path "$Out.log" -Force | Out-Null

Add-Type @"
using System; using System.Runtime.InteropServices;
public static class W {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc f, IntPtr l);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern int GetWindowTextLength(IntPtr h);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  public static IntPtr Find(int[] pids) {
    IntPtr hit = IntPtr.Zero;
    EnumWindows((h, l) => {
      if (!IsWindowVisible(h) || GetWindowTextLength(h) == 0) return true;
      uint pid; GetWindowThreadProcessId(h, out pid);
      if (Array.IndexOf(pids, (int)pid) >= 0) { hit = h; return false; }
      return true;
    }, IntPtr.Zero);
    return hit;
  }
}
"@

function Get-Tree([int]$root) {
    $all = Get-CimInstance Win32_Process -Property ProcessId, ParentProcessId
    $ids = [System.Collections.Generic.List[int]]::new(); $ids.Add($root)
    $grew = $true
    while ($grew) {
        $grew = $false
        foreach ($p in $all) {
            if ($ids.Contains([int]$p.ParentProcessId) -and -not $ids.Contains([int]$p.ProcessId)) { $ids.Add([int]$p.ProcessId); $grew = $true }
        }
    }
    $ids
}

function Get-TreeMemory([int[]]$ids) {
    $priv = 0; $ws = 0; $n = 0
    foreach ($id in $ids) {
        $p = Get-Process -Id $id -ErrorAction SilentlyContinue
        if ($p) { $priv += $p.PrivateMemorySize64; $ws += $p.WorkingSet64; $n++ }
    }
    [pscustomobject]@{ private_mb = [math]::Round($priv / 1MB, 1); working_set_mb = [math]::Round($ws / 1MB, 1); processes = $n }
}

function Find-Window([int[]]$ids) { [W]::Find($ids) }

function Get-TreeCpu([int[]]$ids) {
    $ms = 0.0
    foreach ($id in $ids) { $p = Get-Process -Id $id -ErrorAction SilentlyContinue; if ($p) { $ms += $p.TotalProcessorTime.TotalMilliseconds } }
    $ms
}

function Save-Shot([IntPtr]$h, [string]$path) {
    Add-Type -AssemblyName System.Drawing
    $r = New-Object W+RECT; [void][W]::GetWindowRect($h, [ref]$r)
    $w = $r.R - $r.L; $hh = $r.B - $r.T
    if ($w -le 0 -or $hh -le 0) { return }
    $bmp = New-Object System.Drawing.Bitmap $w, $hh
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($r.L, $r.T, 0, 0, $bmp.Size)
    $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png); $g.Dispose(); $bmp.Dispose()
}

function Read-Log($path) { if (Test-Path $path) { Get-Content $path | Where-Object { $_ } | ForEach-Object { $_ | ConvertFrom-Json } } }

$apps = @(
    @{ name = 'slint';     exe = Join-Path $Bin 'vutric-m1-slint.exe';                  probe = $true },
    @{ name = 'slint-sw';  exe = Join-Path $Bin 'vutric-m1-slint.exe';                  probe = $false; backend = 'winit-software'; perf = $true },
    @{ name = 'tauri';     exe = Join-Path $Bin 'vutric-m1-tauri.exe';                  probe = $true },
    @{ name = 'flutter';   exe = Join-Path $Bin 'flutter\vutric_m1_flutter.exe';        probe = $true },
    @{ name = 'localsend'; exe = Join-Path $Bin 'localsend\localsend_app.exe';          probe = $false }
)

$results = @()
foreach ($app in $apps) {
    for ($run = 1; $run -le $Runs; $run++) {
        $log = Join-Path $env:TEMP "vutric-m1-$($app.name)-$run.jsonl"
        Remove-Item $log -ErrorAction SilentlyContinue
        $env:VUTRIC_M1_LOG = $log
        $env:VUTRIC_M1_FRAMES = "$Frames"
        if ($app.backend) { $env:SLINT_BACKEND = $app.backend } else { Remove-Item Env:SLINT_BACKEND -ErrorAction SilentlyContinue }
        $sw = [Diagnostics.Stopwatch]::StartNew()
        $t0 = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
        $stdout = Join-Path $env:TEMP "vutric-m1-$($app.name)-$run.out"
        if ($app.perf) {
            $env:SLINT_DEBUG_PERFORMANCE = 'refresh_lazy,console'
            $proc = Start-Process -FilePath $app.exe -PassThru -WorkingDirectory (Split-Path $app.exe) -RedirectStandardOutput $stdout -RedirectStandardError "$stdout.err"
        } else {
            Remove-Item Env:SLINT_DEBUG_PERFORMANCE -ErrorAction SilentlyContinue
            $proc = Start-Process -FilePath $app.exe -PassThru -WorkingDirectory (Split-Path $app.exe)
        }
        $win = [IntPtr]::Zero; $windowMs = $null
        while ($sw.ElapsedMilliseconds -lt 15000) {
            $win = Find-Window @($proc.Id)
            if ($win -ne [IntPtr]::Zero) { $windowMs = $sw.ElapsedMilliseconds; break }
            Start-Sleep -Milliseconds 5
        }
        $firstFrameMs = $null
        if ($app.probe) {
            while ($sw.ElapsedMilliseconds -lt 15000 -and -not $firstFrameMs) {
                $ff = Read-Log $log | Where-Object event -eq 'first_frame' | Select-Object -Last 1
                if ($ff) { $firstFrameMs = [int64]$ff.unix_ms - $t0 } else { Start-Sleep -Milliseconds 10 }
            }
        }
        Start-Sleep -Seconds 3
        $tree = Get-Tree $proc.Id
        $open = Get-TreeMemory $tree
        if ($run -eq 1 -and $win -ne [IntPtr]::Zero) { Save-Shot $win (Join-Path (Split-Path $Out) "m1-shot-$($app.name).png") }
        $cpu0 = Get-TreeCpu $tree; $cpuSw = [Diagnostics.Stopwatch]::StartNew()
        $stats = $null
        if ($app.probe) {
            $deadline = $sw.ElapsedMilliseconds + 30000
            while ($sw.ElapsedMilliseconds -lt $deadline -and -not $stats) {
                $stats = Read-Log $log | Where-Object event -eq 'frames' | Select-Object -Last 1
                if (-not $stats) { Start-Sleep -Milliseconds 200 }
            }
        } else {
            Start-Sleep -Seconds 10
        }
        $tree = Get-Tree $proc.Id
        $late = Get-TreeMemory $tree
        $cpuPct = [math]::Round(((Get-TreeCpu $tree) - $cpu0) / $cpuSw.ElapsedMilliseconds * 100, 1)
        $closeSw = [Diagnostics.Stopwatch]::StartNew()
        if ($win -ne [IntPtr]::Zero) { [void][W]::PostMessage($win, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) }
        $closeMs = $null
        while ($closeSw.ElapsedMilliseconds -lt 8000) {
            $alive = @($tree | Where-Object { Get-Process -Id $_ -ErrorAction SilentlyContinue })
            if ($alive.Count -eq 0) { $closeMs = $closeSw.ElapsedMilliseconds; break }
            Start-Sleep -Milliseconds 20
        }
        $left = Get-TreeMemory $tree
        foreach ($id in $tree) { Stop-Process -Id $id -Force -ErrorAction SilentlyContinue }
        $results += [pscustomobject]@{
            app = $app.name; run = $run
            window_ms = $windowMs; first_frame_ms = $firstFrameMs
            open = $open; after_10s = $late
            frames = $stats; cpu_pct_of_one_core = $cpuPct
            perf_tail = if ($app.perf -and (Test-Path $stdout)) { (Get-Content $stdout -Tail 5) -join ' | ' } else { $null }
            exit_ms = $closeMs; left_after_close = $left
        }
        Start-Sleep -Seconds 2
    }
}
Stop-Transcript | Out-Null
$results | ConvertTo-Json -Depth 6 | Set-Content -Encoding utf8 $Out

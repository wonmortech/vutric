# D1 / ADR-0009 §4 measurement: daemon memory at rest, around one effect (cold GPU), after the GPU is released, with
# the pairing card on screen, and shutdown. Run in the interactive session (schtasks /IT).
param(
    [Parameter(Mandatory)] [string] $Exe,
    [Parameter(Mandatory)] [string] $Out,
    [int] $IdleSeconds = 600,
    [int] $Every = 10
)
$ErrorActionPreference = 'Stop'
Start-Transcript -Path "$Out.log" -Force | Out-Null

$trace = "$Out.trace"
Remove-Item $trace -ErrorAction SilentlyContinue
$env:VUTRIC_TRACE = $trace

function Sample($p) {
    $p.Refresh()
    [pscustomobject]@{
        t = [math]::Round($sw.Elapsed.TotalSeconds, 1)
        private_mb = [math]::Round($p.PrivateMemorySize64 / 1MB, 2)
        working_set_mb = [math]::Round($p.WorkingSet64 / 1MB, 2)
        cpu_ms = [math]::Round($p.TotalProcessorTime.TotalMilliseconds)
        threads = $p.Threads.Count
    }
}

# The release exe is a GUI-subsystem app: `& $Exe` would neither wait for it nor capture its stdout.
function Ctl($op) {
    $o = "$Out.ctl"
    Start-Process -FilePath $Exe -ArgumentList 'ctl', $op -Wait -NoNewWindow -RedirectStandardOutput $o
    (Get-Content $o -Raw) -replace '\s+$', ''
}

$sw = [Diagnostics.Stopwatch]::StartNew()
$p = Start-Process -FilePath $Exe -ArgumentList '--portable' -PassThru
Start-Sleep -Seconds 5
$status = Ctl 'status'
$idle = @()
while ($sw.Elapsed.TotalSeconds -lt $IdleSeconds) { $idle += Sample $p; Start-Sleep -Seconds $Every }

$before = Sample $p
$preview = Ctl 'preview_effect'
$during = @()
for ($i = 0; $i -lt 12; $i++) { Start-Sleep -Milliseconds 250; $during += Sample $p }
Start-Sleep -Seconds 5
$after = Sample $p
Start-Sleep -Seconds 40
$released = Sample $p

$show = Ctl 'show'
Start-Sleep -Seconds 3
$withCard = Sample $p

$quitSw = [Diagnostics.Stopwatch]::StartNew()
$quit = Ctl 'quit'
$exited = $p.WaitForExit(5000)
if (-not $exited) { Stop-Process -Id $p.Id -Force }

$steady = $idle | Where-Object { $_.t -ge 60 }
[pscustomobject]@{
    status = $status
    idle_samples = $idle
    idle_avg_private_mb = [math]::Round(($steady | Measure-Object private_mb -Average).Average, 2)
    idle_max_private_mb = ($steady | Measure-Object private_mb -Maximum).Maximum
    idle_avg_working_set_mb = [math]::Round(($steady | Measure-Object working_set_mb -Average).Average, 2)
    idle_cpu_ms_per_min = if ($steady.Count -gt 1) { [math]::Round(($steady[-1].cpu_ms - $steady[0].cpu_ms) / (($steady[-1].t - $steady[0].t) / 60), 1) } else { $null }
    before_effect = $before; during_effect = $during; after_effect_5s = $after; after_gpu_release = $released
    with_pair_card = $withCard
    preview_reply = $preview; show_reply = $show; quit_reply = $quit
    exit_ms = if ($exited) { $quitSw.ElapsedMilliseconds } else { $null }
    trace = if (Test-Path $trace) { @(Get-Content $trace) } else { @() }
} | ConvertTo-Json -Depth 5 | Set-Content -Encoding utf8 $Out
Stop-Transcript | Out-Null

# Boots the pinned Pumpkin build in a run dir, optionally with mods, and records
# startup time, resident memory and CPU use at idle. Usage:
#   scripts\boot-measure.ps1 -RunDir run\baseline [-Mods engine\pumpkin\target\release\counter.dll] [-IdleSeconds 60] [-Commands counter]
param(
    [Parameter(Mandatory)] [string]$RunDir,
    [string[]]$Mods = @(),
    [int]$IdleSeconds = 60,
    [string[]]$Commands = @()
)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$exe = Join-Path $root 'engine\pumpkin\target\release\pumpkin.exe'
$RunDir = Join-Path $root $RunDir
New-Item -ItemType Directory -Force (Join-Path $RunDir 'plugins') | Out-Null
foreach ($m in $Mods) { Copy-Item (Join-Path $root $m) (Join-Path $RunDir 'plugins') -Force }

$psi = [Diagnostics.ProcessStartInfo]::new($exe)
$psi.WorkingDirectory = $RunDir
$psi.UseShellExecute = $false
$psi.RedirectStandardInput = $true
$psi.RedirectStandardOutput = $true
$psi.RedirectStandardError = $true
$log = [Collections.Concurrent.ConcurrentQueue[string]]::new()
$p = [Diagnostics.Process]::new()
$p.StartInfo = $psi
$handler = { if ($EventArgs.Data) { $Event.MessageData.Enqueue($EventArgs.Data) } }
Register-ObjectEvent $p OutputDataReceived -Action $handler -MessageData $log | Out-Null
Register-ObjectEvent $p ErrorDataReceived -Action $handler -MessageData $log | Out-Null

# Windows PowerShell writes child stdin with a UTF-8 BOM unless told otherwise; the BOM breaks the first command.
[Console]::InputEncoding = [Text.UTF8Encoding]::new($false)
$sw = [Diagnostics.Stopwatch]::StartNew()
$p.Start() | Out-Null
$p.BeginOutputReadLine(); $p.BeginErrorReadLine()
$ready = $null
while (-not $ready -and $sw.Elapsed.TotalSeconds -lt 300 -and -not $p.HasExited) {
    Start-Sleep -Milliseconds 50
    if ($log.ToArray() -match 'Started server; took') { $ready = $sw.Elapsed.TotalSeconds }
}
if (-not $ready) {
    if (-not $p.HasExited) { $p.Kill() }
    $log.ToArray() | Set-Content -Encoding utf8 (Join-Path $RunDir 'boot.log')
    throw "server did not start (exited: $($p.HasExited), code: $(if ($p.HasExited) { $p.ExitCode }))"
}

foreach ($c in $Commands) { $p.StandardInput.WriteLine($c); Start-Sleep -Seconds 1 }

$p.Refresh(); $cpu0 = $p.TotalProcessorTime
Start-Sleep -Seconds $IdleSeconds
$p.Refresh()
$cpuPct = 100 * ($p.TotalProcessorTime - $cpu0).TotalSeconds / $IdleSeconds / [Environment]::ProcessorCount
$result = [pscustomobject]@{
    run           = Split-Path $RunDir -Leaf
    mods          = ($Mods | ForEach-Object { Split-Path $_ -Leaf }) -join ','
    startup_s     = [math]::Round($ready, 2)
    rss_mb        = [math]::Round($p.WorkingSet64 / 1MB, 1)
    private_mb    = [math]::Round($p.PrivateMemorySize64 / 1MB, 1)
    peak_rss_mb   = [math]::Round($p.PeakWorkingSet64 / 1MB, 1)
    idle_cpu_pct  = [math]::Round($cpuPct, 2)
    idle_window_s = $IdleSeconds
    cores         = [Environment]::ProcessorCount
}
$p.StandardInput.WriteLine('stop')
if (-not $p.WaitForExit(60000)) { $p.Kill() }
Start-Sleep -Milliseconds 300
$log.ToArray() | Set-Content -Encoding utf8 (Join-Path $RunDir 'boot.log')
$result

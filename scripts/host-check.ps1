# M4 check: `moddedmc run` starts the server (and tunnel), moddedmc-bot joins, `stop` on the
# host console shuts everything down.
#   scripts\host-check.ps1                  # local only
#   scripts\host-check.ps1 -Tunnel playit   # the bot joins through the account's public playit address
# Needs: cargo build -p moddedmc --release, scripts\engine.ps1 build, and the debug moddedmc-bot.
param([ValidateSet('none', 'playit')] [string]$Tunnel = 'none')
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$dir = Join-Path $root "run\host-$Tunnel"
if (Test-Path $dir) { Remove-Item -Recurse -Force $dir }
New-Item -ItemType Directory -Force $dir | Out-Null
# The bot speaks offline mode without encryption or compression.
[IO.File]::WriteAllText((Join-Path $dir 'pumpkin.toml'), @"
[telemetry]
enabled = false

[networking.java]
online_mode = false
encryption = false
view_distance = 4

[networking.java.compression]
enabled = false

[networking.bedrock]
enabled = false
"@)

[Console]::InputEncoding = [Text.UTF8Encoding]::new($false)   # otherwise child stdin starts with a BOM
$log = [Collections.Concurrent.ConcurrentQueue[string]]::new()
function Start-Logged($exe, $arguments) {
    $psi = [Diagnostics.ProcessStartInfo]::new($exe, $arguments)
    $psi.UseShellExecute = $false
    $psi.RedirectStandardInput = $true; $psi.RedirectStandardOutput = $true; $psi.RedirectStandardError = $true
    $p = [Diagnostics.Process]::new(); $p.StartInfo = $psi
    $h = { if ($EventArgs.Data) { $Event.MessageData.Enqueue(($EventArgs.Data -replace '\x1b\[[0-9;]*m', '')) } }
    Register-ObjectEvent $p OutputDataReceived -Action $h -MessageData $log | Out-Null
    Register-ObjectEvent $p ErrorDataReceived -Action $h -MessageData $log | Out-Null
    $p.Start() | Out-Null; $p.BeginOutputReadLine(); $p.BeginErrorReadLine()
    $p
}
$all = [Collections.Generic.List[string]]::new()
function Wait-For($regex, $seconds) {
    $deadline = (Get-Date).AddSeconds($seconds)
    while ((Get-Date) -lt $deadline) {
        $line = $null
        while ($log.TryDequeue([ref]$line)) { $all.Add($line); Write-Host $line }
        $m = $all | Where-Object { $_ -match $regex } | Select-Object -First 1
        if ($m) { return $m }
        Start-Sleep -Milliseconds 200
    }
    throw "timed out waiting for /$regex/"
}

try {
    $t0 = Get-Date
    $app = Start-Logged (Join-Path $root 'target\release\moddedmc.exe') `
        "run --dir `"$dir`" --server `"$root\engine\target\release\pumpkin.exe`" --tunnel $Tunnel"
    $address = '127.0.0.1:25565'
    if ($Tunnel -eq 'playit') {
        $m = [regex]::Match((Wait-For 'players join' 30), 'players join (\S+) \(or [0-9.]+:([0-9]+)\)')
        $address = "$($m.Groups[1].Value):$($m.Groups[2].Value)"   # domain:port, so the handshake names the tunnel
    }
    Wait-For 'Started server' 120 | Out-Null
    Write-Host "== server ready after $([int]((Get-Date) - $t0).TotalMilliseconds) ms"
    if ($Tunnel -eq 'playit') {
        Start-Sleep -Seconds 8   # let the agent register with playit
        $agent = Get-Process playit -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($agent) { Write-Host "== playit agent: $([int]($agent.WorkingSet64 / 1MB)) MB working set" }
    }
    Write-Host "== bot dials $address"
    $bot = Start-Logged (Join-Path $root 'engine\target\debug\moddedmc-bot.exe') "$address hostbot"
    Wait-For '\[bot\] joined as hostbot' 60 | Out-Null
    Wait-For 'hostbot joined the game' 10 | Out-Null
    $bot.StandardInput.WriteLine('quit'); Start-Sleep -Seconds 2
    $app.StandardInput.WriteLine('stop')
    if (-not $app.WaitForExit(60000)) { throw 'moddedmc did not exit after stop' }
    Start-Sleep -Seconds 1
    if ($app.ExitCode -ne 0) { throw "moddedmc exited with $($app.ExitCode)" }
    if (Get-Process playit -ErrorAction SilentlyContinue) { throw 'playit agent still running after stop' }
    Write-Host 'HOST CHECK PASSED'
} finally {
    # A failed run must not leave a server holding the port or a second agent on the same key.
    foreach ($name in 'moddedmc', 'pumpkin', 'playit', 'moddedmc-bot') { cmd /c "taskkill /F /IM $name.exe >nul 2>&1" }
}

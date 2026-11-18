# Runs a console scenario against the built server and checks the log.
#   scripts\scenario.ps1 -Scenario tests\scenarios\m1-counter.txt [-RunDir run\m1] [-Fresh]
# Scenario lines (blank lines and # comments ignored):
#   mods <path> [...]        copy mods (.mmc, .jar, .dll) into the run dir's mods/ (before the first start)
#   start                    start the server and wait until it is ready
#   cmd <console line>       send a console line
#   wait <seconds>
#   expect <regex>           fail unless a log line since the previous expect/start matches
#   reject <regex>           fail if any log line since the previous expect/start matches
#   stop                     send stop and wait for exit
#   botstart <name>          join with moddedmc-bot (its output goes into the log)
#   measure <seconds>        log server memory and CPU use over a window
#   bot <step>               send a step to the bot: cmd <command> | use <x> <y> <z> | quit
param(
    [Parameter(Mandatory)] [string]$Scenario,
    [string]$RunDir = 'run\scenario',
    [switch]$Fresh,
    [string]$Exe = 'engine\target\pack\pack-dev.exe',
    [string]$BotExe = 'engine\target\pack\moddedmc-bot.exe',
    [string]$ExeArgs = ''   # e.g. the Fabric oracle: -Exe <java> -ExeArgs '-jar fabric-server-launch.jar nogui'
)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$RunDir = Join-Path $root $RunDir
if ($Fresh -and (Test-Path $RunDir)) { Get-ChildItem $RunDir | Remove-Item -Recurse -Force }
New-Item -ItemType Directory -Force (Join-Path $RunDir 'mods') | Out-Null
[Console]::InputEncoding = [Text.UTF8Encoding]::new($false)

$script:log = [Collections.Concurrent.ConcurrentQueue[string]]::new()
$script:all = [Collections.Generic.List[string]]::new()
$script:cursor = 0
$script:p = $null
$failures = 0

function Get-NewLines {
    $line = $null
    while ($script:log.TryDequeue([ref]$line)) { $script:all.Add(($line -replace '\x1b\[[0-9;]*m', '')) }
    $script:all.GetRange($script:cursor, $script:all.Count - $script:cursor)
}

function Start-Server {
    $psi = [Diagnostics.ProcessStartInfo]::new((Join-Path $root $Exe), $ExeArgs)
    $psi.WorkingDirectory = $RunDir
    $psi.UseShellExecute = $false
    $psi.RedirectStandardInput = $true
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $script:p = [Diagnostics.Process]::new()
    $script:p.StartInfo = $psi
    $handler = { if ($EventArgs.Data) { $Event.MessageData.Enqueue($EventArgs.Data) } }
    Register-ObjectEvent $script:p OutputDataReceived -Action $handler -MessageData $script:log | Out-Null
    Register-ObjectEvent $script:p ErrorDataReceived -Action $handler -MessageData $script:log | Out-Null
    $script:p.Start() | Out-Null
    $script:p.BeginOutputReadLine(); $script:p.BeginErrorReadLine()
    # Expectations after a start search from its first line: mods may register before "Started server".
    $first = $script:all.Count
    $deadline = (Get-Date).AddSeconds(600)  # a new .jar in mods/ is converted on the first start
    while ((Get-Date) -lt $deadline -and -not $script:p.HasExited) {
        Start-Sleep -Milliseconds 100
        $new = @(Get-NewLines)
        for ($i = 0; $i -lt $new.Count; $i++) {
            # Pumpkin, or a vanilla/Fabric server.
            if ($new[$i] -match 'Started server; took|Done \(') { $script:cursor = $first; return }
        }
    }
    throw "server did not start"
}

# Test settings, written before the first start; Pumpkin merges in the remaining defaults.
# Offline mode without encryption/compression lets moddedmc-bot join; telemetry stays off.
$config = Join-Path $RunDir 'pumpkin.toml'
if (-not (Test-Path $config)) {
    [IO.File]::WriteAllText($config, @"
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

# The operator's grant for test mods that keep files in their own data folder.
[plugins]
allowed_permissions = ["fs.read.data", "fs.write.data"]
"@)
}
$script:bot = $null

foreach ($raw in Get-Content $Scenario) {
    $line = $raw.Trim()
    if (-not $line -or $line.StartsWith('#')) { continue }
    $verb, $arg = $line -split '\s+', 2
    Write-Host "> $line"
    switch ($verb) {
        'mods' { foreach ($m in $arg -split '\s+') { Copy-Item (Join-Path $root $m) (Join-Path $RunDir 'mods') -Force } }
        'start' { Start-Server }
        'cmd' { $script:p.StandardInput.WriteLine($arg) }
        'botstart' {
            $psi = [Diagnostics.ProcessStartInfo]::new((Join-Path $root $BotExe), "127.0.0.1:25565 $arg")
            $psi.UseShellExecute = $false
            $psi.RedirectStandardInput = $true
            $psi.RedirectStandardOutput = $true
            $psi.RedirectStandardError = $true
            $script:bot = [Diagnostics.Process]::new()
            $script:bot.StartInfo = $psi
            $handler = { if ($EventArgs.Data) { $Event.MessageData.Enqueue($EventArgs.Data) } }
            Register-ObjectEvent $script:bot OutputDataReceived -Action $handler -MessageData $script:log | Out-Null
            Register-ObjectEvent $script:bot ErrorDataReceived -Action $handler -MessageData $script:log | Out-Null
            $script:bot.Start() | Out-Null
            $script:bot.BeginOutputReadLine(); $script:bot.BeginErrorReadLine()
        }
        'bot' { $script:bot.StandardInput.WriteLine($arg) }
        'measure' {
            $secs = [double]$arg
            $script:p.Refresh(); $cpu0 = $script:p.TotalProcessorTime
            Start-Sleep -Milliseconds ([int]($secs * 1000))
            $script:p.Refresh()
            $cpu = 100 * ($script:p.TotalProcessorTime - $cpu0).TotalSeconds / $secs / [Environment]::ProcessorCount
            $line = 'measure rss_mb={0:N1} private_mb={1:N1} cpu_pct={2:N2} window_s={3} cores={4}' -f ($script:p.WorkingSet64 / 1MB), ($script:p.PrivateMemorySize64 / 1MB), $cpu, $secs, [Environment]::ProcessorCount
            $script:log.Enqueue($line)
            Write-Host "  $line"
        }
        'wait' { Start-Sleep -Milliseconds ([int]([double]$arg * 1000)) }
        'expect' {
            # Consume up to the first match only, so the next expect can match later lines.
            $new = @(Get-NewLines)
            $i = 0
            while ($i -lt $new.Count -and $new[$i] -notmatch $arg) { $i++ }
            if ($i -lt $new.Count) { Write-Host "  ok: $($new[$i])" -ForegroundColor Green; $script:cursor += $i + 1 }
            else { Write-Host "  FAIL: no line matches /$arg/" -ForegroundColor Red; $failures++ }
        }
        'reject' {
            $hits = @(Get-NewLines | Where-Object { $_ -match $arg })
            if ($hits) { Write-Host "  FAIL: $($hits[0])" -ForegroundColor Red; $failures++ }
            else { Write-Host "  ok: none" -ForegroundColor Green }
        }
        'stop' {
            $script:p.StandardInput.WriteLine('stop')
            if (-not $script:p.WaitForExit(60000)) { $script:p.Kill(); Write-Host '  FAIL: did not stop' -ForegroundColor Red; $failures++ }
            Start-Sleep -Milliseconds 300
            Get-NewLines | Out-Null
        }
        default { throw "unknown scenario verb: $verb" }
    }
}
if ($script:bot -and -not $script:bot.HasExited) { $script:bot.Kill() }
if ($script:p -and -not $script:p.HasExited) { $script:p.Kill() }
Get-NewLines | Out-Null
$script:all | Set-Content -Encoding utf8 (Join-Path $RunDir 'scenario.log')
if ($failures) { Write-Host "$failures check(s) failed; log: $RunDir\scenario.log" -ForegroundColor Red; exit 1 }
Write-Host "all checks passed; log: $RunDir\scenario.log" -ForegroundColor Green

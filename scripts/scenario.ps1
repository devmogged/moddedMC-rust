# Runs a console scenario against the built server and checks the log.
#   scripts\scenario.ps1 -Scenario tests\scenarios\m1-counter.txt [-RunDir run\m1] [-Fresh] [-Keep] [-Port 25565] [-Seed <long>]
# Parallel runs need distinct run directories and ports, including each resource pack listener at -Port + 1.
# A passing run deletes its run dir, because worlds add up fast. -Keep leaves it; a failing run is always kept.
# Scenario lines (blank lines and # comments ignored):
#   mods <path> [...]        copy mods (.mmc, .jar, .dll) into the run dir's mods/ (before the first start)
#   datapack <path> <name>   copy an existing fixture directory into world/datapacks before start
#   start                    start the server and wait until it is ready
#   cmd <console line>       send a console line
#   wait <seconds>
#   expect <regex>           fail unless a log line since the previous expect/start matches
#   expect-all <regex> && <regex>  require each pattern in the same log window, in any order
#   await [seconds] <regex>  wait for acknowledgement; default 5s when no timeout supplied
#   reject <regex>           fail if any log line since the previous expect/start matches
#   stop                     send stop and wait for exit
#   botstart <name>          join with moddedmc-bot (its output goes into the log)
#   botselect <name>         switch the active bot when multiple are connected
#   measure <seconds>        log server memory and CPU use over a window
#   bot <step>               send a step to the bot: cmd <command> | use <x> <y> <z> | quit
param(
    [Parameter(Mandatory)] [string]$Scenario,
    [string]$RunDir = 'run\scenario',
    [switch]$Fresh,
    [switch]$Keep,
    [int]$Port = 25565,
    [long]$Seed,
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

function Resolve-RepoPath([string]$Path) {
    if ([IO.Path]::IsPathRooted($Path)) { $Path } else { Join-Path $root $Path }
}

function Start-Server {
    $psi = [Diagnostics.ProcessStartInfo]::new((Resolve-RepoPath $Exe), $ExeArgs)
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
    $seedSetting = if ($PSBoundParameters.ContainsKey('Seed')) { "seed = `"$Seed`"`n" } else { '' }
    [IO.File]::WriteAllText($config, @"
${seedSetting}[telemetry]
enabled = false

[networking.java]
address = "127.0.0.1:$Port"
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
$bots = @{}

foreach ($raw in Get-Content $Scenario) {
    $line = $raw.Trim()
    if (-not $line -or $line.StartsWith('#')) { continue }
    $verb, $arg = $line -split '\s+', 2
    Write-Host "> $line"
    switch ($verb) {
        'mods' { foreach ($m in $arg -split '\s+') { Copy-Item (Join-Path $root $m) (Join-Path $RunDir 'mods') -Force } }
        'datapack' {
            $source, $name = $arg -split '\s+', 2
            if (-not $source -or $name -notmatch '^[a-zA-Z0-9_.-]+$') { throw 'datapack requires a fixture path and simple destination name' }
            $sourcePath = Join-Path $root $source
            if (-not (Test-Path -LiteralPath $sourcePath -PathType Container)) { throw "datapack fixture not found: $source" }
            $destinationRoot = Join-Path $RunDir 'world\datapacks'
            New-Item -ItemType Directory -Force $destinationRoot | Out-Null
            $destination = Join-Path $destinationRoot $name
            if (Test-Path -LiteralPath $destination) { throw "datapack destination already exists: $destination" }
            Copy-Item -LiteralPath $sourcePath -Destination $destination -Recurse
        }
        'start' { Start-Server }
        'cmd' { $script:p.StandardInput.WriteLine($arg) }
        'botstart' {
            if ($bots.ContainsKey($arg) -and -not $bots[$arg].HasExited) {
                throw "bot $arg is already running"
            }
            $psi = [Diagnostics.ProcessStartInfo]::new((Resolve-RepoPath $BotExe), "127.0.0.1:${Port} $arg")
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
            $bots[$arg] = $script:bot
        }
        'botselect' {
            if (-not $bots.ContainsKey($arg)) { throw "unknown bot: $arg" }
            $script:bot = $bots[$arg]
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
        { $_ -in 'expect', 'expect-all', 'await' } {
            if ($_ -eq 'await') {
                $seconds = 5
                if ($arg -match '^([0-9]+(?:\.[0-9]+)?)\s+(.+)$') {
                    $seconds = [double]$Matches[1]
                    $arg = $Matches[2]
                }
                $deadline = [Diagnostics.Stopwatch]::StartNew()
                do {
                    $new = @(Get-NewLines)
                    if ($new -match $arg) { break }
                    $script:p.Refresh()
                    if ($script:p.HasExited) { break }
                    Start-Sleep -Milliseconds 25
                } while ($deadline.Elapsed.TotalSeconds -lt [double]$seconds)
            } else {
                $new = @(Get-NewLines)
            }
            $patterns = if ($_ -eq 'expect-all') { $arg -split ' && ' } else { $arg }
            $consumed = 0
            foreach ($pattern in $patterns) {
                $i = 0
                while ($i -lt $new.Count -and $new[$i] -notmatch $pattern) { $i++ }
                if ($i -lt $new.Count) {
                    Write-Host "  ok: $($new[$i])" -ForegroundColor Green
                    $consumed = [Math]::Max($consumed, $i + 1)
                } else {
                    Write-Host "  FAIL: no line matches /$pattern/" -ForegroundColor Red
                    $failures++
                }
            }
            # Keep later log lines for the next check.
            $script:cursor += $consumed
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
foreach ($bot in $bots.Values) {
    if (-not $bot.HasExited) { $bot.Kill() }
}
if ($script:p -and -not $script:p.HasExited) { $script:p.Kill() }
Get-NewLines | Out-Null
$script:all | Set-Content -Encoding utf8 (Join-Path $RunDir 'scenario.log')
if ($failures) { Write-Host "$failures check(s) failed; log: $RunDir\scenario.log" -ForegroundColor Red; exit 1 }
if ($Keep) { Write-Host "all checks passed; log: $RunDir\scenario.log" -ForegroundColor Green; exit 0 }
Remove-Item -Recurse -Force $RunDir
Write-Host "all checks passed; removed $RunDir (pass -Keep to keep it)" -ForegroundColor Green

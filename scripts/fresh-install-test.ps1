# Pre-release check: what a new user's PC goes through. Installs the setup .exe into a temp
# folder, serves a local copy of the release's server files, launches the app with empty app data
# and checks that it downloads and verifies the server files, starts the agent, and that a server
# can be made, configured, started, joined, stopped and shut down. Then uninstalls and restores.
#   scripts\fresh-install-test.ps1 -Installer <setup.exe> -Server <server.exe> -Agent <agent.exe> [-Mod <x.mmc>]
# Run it with no moddedMC app or agent running: it refuses to touch ones it didn't start.
param(
    [Parameter(Mandatory)] [string] $Installer,
    [Parameter(Mandatory)] [string] $Server,
    [Parameter(Mandatory)] [string] $Agent,
    [string] $Mod,
    [int] $Port = 25570
)
$ErrorActionPreference = 'Stop'
$results = [Collections.Generic.List[object]]::new()
function Check($name, [scriptblock] $test) {
    try { $detail = & $test; $results.Add([pscustomobject]@{ Check = $name; Result = 'PASS'; Detail = "$detail" }) }
    catch { $results.Add([pscustomobject]@{ Check = $name; Result = 'FAIL'; Detail = $_.Exception.Message }) }
}
function Wait-Until([scriptblock] $condition, [int] $seconds, [string] $what) {
    $deadline = (Get-Date).AddSeconds($seconds)
    while ((Get-Date) -lt $deadline) { if (& $condition) { return }; Start-Sleep -Milliseconds 500 }
    throw "timed out waiting for $what"
}
$api = 'http://127.0.0.1:25599'

if (Get-Process moddedmc-app, moddedMC, moddedmc-agent -ErrorAction SilentlyContinue) {
    throw 'Close moddedMC (tray: Quit) first; this test needs a clean start.'
}

$root = Join-Path ([IO.Path]::GetTempPath()) "moddedmc-fresh-$(Get-Random)"
$appData = Join-Path $env:LOCALAPPDATA 'gg.devmogged.moddedmc'
$backup = "$appData.before-fresh-test"
if (Test-Path $backup) { throw "$backup exists from an earlier run; restore or delete it first." }
New-Item -ItemType Directory -Force "$root\release", "$root\home" | Out-Null
if (Test-Path $appData) { Rename-Item $appData $backup }
$listener = $null; $app = $null
try {
    # A local copy of the release, laid out like the GitHub one.
    Copy-Item $Server "$root\release\moddedmc-server-windows-x86_64.exe"
    Copy-Item $Agent "$root\release\moddedmc-agent-windows-x86_64.exe"
    $base = 'http://127.0.0.1:25601/'
    $files = foreach ($role in 'server', 'agent') {
        $path = "$root\release\moddedmc-$role-windows-x86_64.exe"
        [ordered]@{ role = $role; target = 'windows-x86_64'; url = "${base}moddedmc-$role-windows-x86_64.exe"
            sha256 = (Get-FileHash $path -Algorithm SHA256).Hash.ToLower(); size = (Get-Item $path).Length }
    }
    # No byte-order mark, like the release workflow writes it (Windows PowerShell's utf8 adds one).
    [IO.File]::WriteAllText("$root\release\runtime.json",
        ([ordered]@{ version = 'v0.0.0-fresh'; files = @($files) } | ConvertTo-Json -Depth 5))
    $listener = Start-Job -ArgumentList "$root\release", $base {
        param($dir, $prefix)
        $http = [Net.HttpListener]::new(); $http.Prefixes.Add($prefix); $http.Start()
        while ($true) {
            $context = $http.GetContext()
            $file = Join-Path $dir ($context.Request.Url.AbsolutePath.TrimStart('/'))
            if (Test-Path -LiteralPath $file -PathType Leaf) {
                $bytes = [IO.File]::ReadAllBytes($file)
                $context.Response.ContentLength64 = $bytes.Length
                $context.Response.OutputStream.Write($bytes, 0, $bytes.Length)
            } else { $context.Response.StatusCode = 404 }
            $context.Response.Close()
        }
    }

    Check 'Installer runs silently' {
        $p = Start-Process $Installer -ArgumentList '/S', "/D=$root\app" -PassThru -Wait
        if ($p.ExitCode) { throw "exit code $($p.ExitCode)" }
        if (-not (Test-Path "$root\app\moddedMC.exe")) { throw 'moddedMC.exe not installed' }
        "installed to $root\app"
    }

    Check 'First launch installs the server files' {
        $env:MODDEDMC_RUNTIME_MANIFEST = "${base}runtime.json"
        $env:MODDEDMC_HOME = "$root\home"
        $script:app = Start-Process "$root\app\moddedMC.exe" -PassThru
        Wait-Until { (Get-Content "$appData\runtime\version.txt" -ErrorAction SilentlyContinue) -eq 'v0.0.0-fresh' } 120 'version.txt'
        foreach ($role in 'server', 'agent') {
            $got = (Get-FileHash "$appData\runtime\moddedmc-$role.exe" -Algorithm SHA256).Hash
            $want = (Get-FileHash "$root\release\moddedmc-$role-windows-x86_64.exe" -Algorithm SHA256).Hash
            if ($got -ne $want) { throw "$role differs from the release" }
        }
        'server and agent downloaded and verified'
    }

    Check 'The app starts the agent' {
        Wait-Until { try { Invoke-RestMethod "$api/v1/device" -TimeoutSec 2 | Out-Null; $true } catch { $false } } 60 'the agent'
        $agentProc = Get-Process moddedmc-agent
        if ($agentProc.Path -notlike "$appData*") { throw "agent runs from $($agentProc.Path)" }
        if (-not (Test-Path "$root\home\agent.toml")) { throw 'agent did not use the fresh home' }
        "pid $($agentProc.Id)"
    }

    $exe = "$appData\runtime\moddedmc-server.exe"
    Check 'Make a server' {
        Invoke-RestMethod -Method Post "$api/v1/servers" -ContentType application/json `
            -Body (@{ name = 'fresh'; exe = $exe; port = $Port } | ConvertTo-Json) | Out-Null
        ((Invoke-RestMethod "$api/v1/servers") | Where-Object name -eq fresh).state
    }

    Check 'Change its settings' {
        $config = Invoke-RestMethod "$api/v1/servers/fresh/config"
        $config.motd = 'fresh install test'; $config.max_players = 7
        Invoke-RestMethod -Method Put "$api/v1/servers/fresh/config" -ContentType application/json -Body ($config | ConvertTo-Json -Depth 5) | Out-Null
        $after = Invoke-RestMethod "$api/v1/servers/fresh/config"
        if ($after.motd -ne 'fresh install test' -or $after.max_players -ne 7) { throw 'settings did not stick' }
        "motd and max players saved"
    }

    if ($Mod) {
        Check 'Add a drop-in mod' {
            $name = Split-Path $Mod -Leaf
            Invoke-RestMethod -Method Put "$api/v1/servers/fresh/mods/$name" -InFile $Mod -ContentType application/octet-stream | Out-Null
            ((Invoke-RestMethod "$api/v1/servers/fresh/mods") | ConvertTo-Json -Compress -Depth 3)
        }
    }

    Check 'Start it and join (server list ping)' {
        Invoke-RestMethod -Method Post "$api/v1/servers/fresh/start" | Out-Null
        Wait-Until { ((Invoke-RestMethod "$api/v1/servers") | Where-Object name -eq fresh).state -eq 'running' } 120 'running'
        $reply = $null
        Wait-Until {
            try {
                $c = [Net.Sockets.TcpClient]::new('127.0.0.1', $Port); $s = $c.GetStream(); $s.ReadTimeout = 5000
                $host_ = [Text.Encoding]::ASCII.GetBytes('127.0.0.1')
                $body = [byte[]](0x00, 0xFF, 0x05, $host_.Length) + $host_ + [byte[]](($Port -shr 8), ($Port -band 0xFF), 0x01)
                $s.Write([byte[]]($body.Length) + $body, 0, $body.Length + 1); $s.Write([byte[]](0x01, 0x00), 0, 2)
                $buf = New-Object byte[] 4096; $n = 0
                do { $k = $s.Read($buf, $n, $buf.Length - $n); $n += $k } while ($k -gt 0 -and $n -lt 200)
                $c.Close(); $script:reply = [Text.Encoding]::UTF8.GetString($buf, 0, $n); $true
            } catch { $false }
        } 60 'a ping reply'
        if ($script:reply -notmatch 'fresh install test') { throw "ping reply lacks the motd: $($script:reply)" }
        'answers with the new motd'
    }

    Check 'Uses the C runtime it ships with' {
        # A fresh PC may lack the VC++ redistributable; nothing we ship may import it.
        $missing = foreach ($p in $exe, "$appData\runtime\moddedmc-agent.exe", "$root\app\moddedMC.exe") {
            $bytes = [IO.File]::ReadAllBytes($p); $text = [Text.Encoding]::ASCII.GetString($bytes)
            if ($text -match 'VCRUNTIME140\.dll') { Split-Path $p -Leaf }
        }
        if ($missing) { throw "needs VCRUNTIME140.dll: $($missing -join ', ')" }
        'none needs VCRUNTIME140.dll'
    }

    Check 'Join addresses' {
        $a = Invoke-RestMethod "$api/v1/servers/fresh/addresses"
        "$(($a.addresses | ForEach-Object { "$($_.kind)=$($_.address)" }) -join ', '); playit: $($a.playit)"
    }

    Check 'playit sign-in link' {
        $s = Invoke-RestMethod "$api/v1/playit"
        if ($s.signed_in) { return 'already signed in (an existing playit sign-in on this PC was reused)' }
        $c = Invoke-RestMethod -Method Post "$api/v1/playit/claim"
        if ($c.url -notmatch '^https://playit\.gg/claim/[0-9a-f]{10}$') { throw "bad link $($c.url)" }
        "link $($c.url) (not opened)"
    }

    Check 'Quit stops everything' {
        Invoke-RestMethod -Method Post "$api/v1/shutdown" | Out-Null
        Wait-Until { -not (Get-Process moddedmc-agent, moddedmc-server -ErrorAction SilentlyContinue) } 60 'the agent and server to exit'
        'agent and server exited'
    }
} finally {
    if ($app -and -not $app.HasExited) { Stop-Process -Id $app.Id -Force }
    Remove-Item Env:MODDEDMC_RUNTIME_MANIFEST, Env:MODDEDMC_HOME -ErrorAction SilentlyContinue
    if ($listener) { Stop-Job $listener; Remove-Job $listener -Force }
    if (Test-Path "$root\app\uninstall.exe") {
        Check 'Uninstaller runs silently' {
            $p = Start-Process "$root\app\uninstall.exe" -ArgumentList '/S' -PassThru -Wait
            Wait-Until { -not (Test-Path "$root\app\moddedMC.exe") } 30 'files to go'
            "exit code $($p.ExitCode)"
        }
    }
    Remove-Item -Recurse -Force $appData -ErrorAction SilentlyContinue
    if (Test-Path $backup) { Rename-Item $backup $appData }
    Remove-Item -Recurse -Force $root -ErrorAction SilentlyContinue
}
$results | Format-Table -AutoSize -Wrap
if ($results.Result -contains 'FAIL') { exit 1 }

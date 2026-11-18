# Pregeneration: our Chunky port (packs/pregen) against Fabric 26.3 + Chunky, same seed and radius.
# Prints time and peak RSS for each run. The bar for ports is in docs/PORTS.md.
#   scripts\pregen-bench.ps1 -Radius 1000 -Runs 3
param(
    [int]$Radius = 1000,
    [int]$Runs = 1,
    [string]$Seed = '12345',
    [string]$Chunky = '1.5.66',
    [string]$ChunkyUrl = 'https://cdn.modrinth.com/data/fALzjamp/versions/86FKD9qx/Chunky-Fabric-1.5.66.jar',
    [string]$JavaHeap = '4G',
    [switch]$SkipJava
)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$driver = Join-Path $PSScriptRoot 'pregen-bench.mjs'

cargo run -q --release -p moddedmc-pack -- build (Join-Path $root 'packs\pregen\pack.toml') --target laptop
if ($LASTEXITCODE -ne 0) { throw 'building packs/pregen failed' }
$pack = Join-Path $root 'target\packs\pregen\laptop\pack-pregen.exe'
$commands = "chunky radius $Radius;chunky start"

$rust = Join-Path $root 'run\bench-pregen-rust'
for ($i = 1; $i -le $Runs; $i++) {
    if (Test-Path $rust) { Remove-Item -Recurse -Force $rust }
    New-Item -ItemType Directory -Force $rust | Out-Null
    [IO.File]::WriteAllText((Join-Path $rust 'pumpkin.toml'), @"
seed = "$Seed"
[networking.java]
online_mode = false
address = "127.0.0.1:25591"
[networking.bedrock]
enabled = false
[telemetry]
enabled = false
"@)
    Push-Location $rust
    $result = node $driver . 'Started server' 'Task finished' $commands -- $pack
    Pop-Location
    Write-Output "rust radius=$Radius run=${i}: $result"
}
if ($SkipJava) { return }

$cache = Join-Path $root 'cache\fabric'
New-Item -ItemType Directory -Force $cache | Out-Null
$chunkyJar = Join-Path $cache "Chunky-Fabric-$Chunky.jar"
if (-not (Test-Path $chunkyJar)) { Invoke-WebRequest -UseBasicParsing $ChunkyUrl -OutFile $chunkyJar }
# The launcher on PATH can be a shim; time and measure the JVM itself.
$settings = cmd /c 'java -XshowSettings:properties -version 2>&1'
$javaHome = ($settings | Select-String 'java.home = (.*)').Matches[0].Groups[1].Value.Trim()
$javaExe = Join-Path $javaHome 'bin\java.exe'
for ($i = 1; $i -le $Runs; $i++) {
    & (Join-Path $PSScriptRoot 'fabric-oracle.ps1') -RunDir 'run\bench-pregen-java' -Mods @("cache\fabric\Chunky-Fabric-$Chunky.jar")
    $java = Join-Path $root 'run\bench-pregen-java'
    Add-Content (Join-Path $java 'server.properties') "level-seed=$Seed`nserver-port=25592"
    Push-Location $java
    $heap = "-Xmx$JavaHeap"
    $result = node $driver . 'Done \(' 'Task finished' $commands -- $javaExe $heap -jar fabric-server-launch.jar nogui
    Pop-Location
    Write-Output "java radius=$Radius run=${i}: $result"
}

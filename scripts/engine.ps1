# Builds a pack server: Pumpkin (engine/pumpkin, our fork devmogged/Pumpkin, branch moddedmc) with
# the content host and the pack's compiled-in ports, plus the test bot.
#   scripts\engine.ps1 build [-Pack dev] [-Profile pack|release|dev]
#     pack     (default) optimized, incremental, no fat LTO: fast rebuilds while working
#     release  fat LTO, for deploys and benchmarks (slow: start it in the background)
#     dev      unoptimized debug build
# Output: engine\target\<profile dir>\pack-<name>.exe and moddedmc-bot.exe.
# Engine changes are ordinary commits in engine/pumpkin (push them to devmogged/Pumpkin).
param(
    [Parameter(Mandatory)][ValidateSet('build')] [string]$Action,
    [string]$Pack = 'dev',
    [ValidateSet('pack', 'release', 'dev')] [string]$Profile = 'pack'
)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
if (-not $env:PUMPKIN_MINECRAFT_SERVER_JAR) {
    $jar = Join-Path $root 'cache\mc-26.3\server.jar'
    if (Test-Path $jar) { $env:PUMPKIN_MINECRAFT_SERVER_JAR = $jar }
}
Push-Location (Join-Path $root 'engine')
try {
    cargo build --profile $Profile -p "pack-$Pack" -p moddedmc-bot
    if ($LASTEXITCODE) { throw 'build failed' }
}
finally { Pop-Location }

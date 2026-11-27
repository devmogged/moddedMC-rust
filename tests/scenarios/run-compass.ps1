# Run exactly one compass regression in a new directory. No build is performed.
# Invoke only after the prime agent grants the scenario gate.
[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [ValidateSet('items','search','negative','dimension','explorer','inventory','height','absent','custom-rings','offset','weighted-group','reset','visibility','visibility-reload','multiplayer','interrupted','same-item','repair','explorer-repair','invalid-xp','invalid-durability','group-navigation','durability','explorer-durability','xp-nature','xp-explorer')]
    [string]$Case,
    [ValidateRange(1024,65534)]
    [int]$Port = 26180,
    [string]$RunName = '',
    [switch]$Keep
)
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
if (-not $RunName) { $RunName = 'compass-' + $Case + '-' + (Get-Date -Format 'yyyyMMdd-HHmmss') }
if ($RunName -notmatch '^[a-zA-Z0-9_-]+$') { throw 'RunName must be a single directory name' }
$run = "run/$RunName"
$absolute = Join-Path $root $run
if (Test-Path -LiteralPath $absolute) { throw "Run directory already exists: $absolute" }
$scenario = Join-Path $PSScriptRoot "compasses-$Case.txt"
if ($Case -eq 'xp-nature') { $scenario = Join-Path $PSScriptRoot 'compasses-fixtures/pending-xp/nature.txt' }
if ($Case -eq 'xp-explorer') { $scenario = Join-Path $PSScriptRoot 'compasses-fixtures/pending-xp/explorer.txt' }
$fixture = switch ($Case) {
    'absent' { 'zero-frequency' }
    'custom-rings' { 'custom-rings' }
    'offset' { 'weighted-group' }
    'weighted-group' { 'weighted-group' }
    'reset' { 'weighted-group' }
    'visibility' { 'weighted-group' }
    'visibility-reload' { 'visibility-reload' }
    'multiplayer' { 'weighted-group' }
    'interrupted' { 'zero-frequency' }
    'same-item' { 'weighted-group' }
    'explorer-repair' { 'weighted-group' }
    'group-navigation' { 'weighted-group' }
    'explorer-durability' { 'weighted-group' }
    'xp-explorer' { 'weighted-group' }
    default { $null }
}
foreach ($path in @('engine/target/pack/pack-dev.exe','engine/target/pack/moddedmc-bot.exe','target/mods/compasses.mmc')) {
    if (-not (Test-Path -LiteralPath (Join-Path $root $path) -PathType Leaf)) {
        throw "Missing coordinated build artifact: $path"
    }
}
# Never use -Fresh: it would discard staged datapacks.
if ($fixture) {
    $dest = Join-Path $absolute 'world/datapacks'
    New-Item -ItemType Directory -Force -Path $dest | Out-Null
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot "compasses-fixtures/$fixture") -Destination $dest -Recurse
}
if ($Case -eq 'visibility') {
    $source = Join-Path $PSScriptRoot 'compasses-fixtures/visibility'
    Copy-Item -LiteralPath (Join-Path $source 'data/c') -Destination (Join-Path $dest 'weighted-group/data') -Recurse
    $config = Join-Path $absolute 'config/compasses'
    New-Item -ItemType Directory -Force -Path $config | Out-Null
    Copy-Item -Path (Join-Path $source 'config/*.json') -Destination $config
}
if ($Case -eq 'interrupted') {
    $source = Join-Path $PSScriptRoot 'compasses-fixtures/interrupted'
    $set = Join-Path $dest 'zero-frequency/data/aaa_compass_test/worldgen/structure_set/dense_pyramids.json'
    Copy-Item -LiteralPath (Join-Path $source 'dense_pyramids.json') -Destination $set -Force
    $config = Join-Path $absolute 'config/compasses'
    New-Item -ItemType Directory -Force -Path $config | Out-Null
    Copy-Item -LiteralPath (Join-Path $source 'explorerscompass.json') -Destination $config
}
if ($Case -in @('repair','explorer-repair','invalid-xp','invalid-durability','durability','explorer-durability','xp-nature','xp-explorer')) {
    $configFixture = switch ($Case) {
        'explorer-repair' { 'repair' }
        'explorer-durability' { 'durability' }
        'xp-nature' { 'pending-xp' }
        'xp-explorer' { 'pending-xp' }
        default { $Case }
    }
    $source = Join-Path $PSScriptRoot "compasses-fixtures/$configFixture/config"
    $config = Join-Path $absolute 'config/compasses'
    New-Item -ItemType Directory -Force -Path $config | Out-Null
    Copy-Item -Path (Join-Path $source '*.json') -Destination $config
}
# The underlying runner retains failing worlds and logs; successful worlds
# are discarded unless Keep is requested to avoid accumulating large data.
$runner = Join-Path $root 'scripts/scenario.ps1'
$arguments = @('-NoProfile','-File',$runner,'-Scenario',$scenario,'-RunDir',$run,'-Port',$Port,'-Seed',42)
if ($Keep) { $arguments += '-Keep' }
& powershell.exe @arguments
exit $LASTEXITCODE

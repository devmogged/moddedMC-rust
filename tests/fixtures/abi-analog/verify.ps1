# Compile and verify the native analog fixture against real Pumpkin comparator blocks.
param([int] $Port = 26000, [string] $Exe = 'target\pack\pack-dev.exe')
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..\..')).Path
& (Join-Path $PSScriptRoot 'pack.ps1')
if ($LASTEXITCODE) { throw 'Could not pack analog fixture' }
& (Join-Path $root 'tests\fixtures\abi-saved-data\pack.ps1')
if ($LASTEXITCODE) { throw 'Could not pack vanilla control fixture' }
foreach ($scenario in @('abi-analog', 'abi-comparator-vanilla')) {
    $run = 'run\0-' + $scenario
    & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $root 'scripts\scenario.ps1') -Scenario "tests\scenarios\$scenario.txt" -Port $Port -RunDir $run -Fresh -Exe $Exe
    if ($LASTEXITCODE) { throw "Comparator scenario failed: $scenario" }
}
Write-Host 'PASS custom analog output, saved comparator signals and vanilla control'

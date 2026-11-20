# End-to-end dependency ordered cross-mod service ABI acceptance.
param(
    [int] $Port = 26000,
    [string] $Exe = 'target\pack\pack-dev.exe'
)
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..\..')).Path
& (Join-Path $PSScriptRoot 'pack.ps1')
if ($LASTEXITCODE) { throw 'Service fixture build failed' }
foreach ($case in @('abi-services', 'abi-services-missing', 'abi-services-cycle')) {
    $run = 'run\0-' + $case
    & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $root 'scripts\scenario.ps1') -Scenario "tests\scenarios\$case.txt" -Port $Port -RunDir $run -Fresh -Exe $Exe
    if ($LASTEXITCODE) { throw "Service scenario failed: $case" }
}
Write-Host 'PASS: provider-first registration, service invocation, restart, missing dependency'

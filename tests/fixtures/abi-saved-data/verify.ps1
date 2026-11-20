# Deterministic acceptance test for moddedmc:saved_data v1.
# Runs two independent save roots, each across a restart, then corrupt-file fail-closed.
param(
    [int] $Port = 26000,
    [string] $Exe = 'target\pack\pack-dev.exe',
    [switch] $Keep
)
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..\..')).Path
$scenario = Join-Path $root 'scripts\scenario.ps1'

& (Join-Path $PSScriptRoot 'pack.ps1')
if ($LASTEXITCODE) { throw 'Could not build saved-data test mod' }

foreach ($name in @('a', 'b')) {
    $run = "run\0-saved-data-$name"
    $args = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $scenario,
        '-Scenario', 'tests\scenarios\abi-saved-data.txt',
        '-Port', "$Port", '-RunDir', $run, '-Exe', $Exe, '-Fresh')
    if ($Keep) { $args += '-Keep' }
    & powershell @args
    if ($LASTEXITCODE) { throw "Saved-data test failed for $name" }
}

# An unreadable snapshot must fail closed and survive shutdown unmodified.
$run = 'run\0-saved-data-corrupt'
$directory = Join-Path $root $run
if (Test-Path $directory) { Remove-Item -LiteralPath $directory -Recurse -Force }
New-Item -ItemType Directory -Force (Join-Path $directory 'world\moddedmc') | Out-Null
$file = Join-Path $directory 'world\moddedmc\saved_data.nbt'
[IO.File]::WriteAllBytes($file, [byte[]](10, 0, 0, 7, 0, 2))
$before = (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash
$args = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $scenario,
    '-Scenario', 'tests\scenarios\abi-saved-data-corrupt.txt',
    '-Port', "$Port", '-RunDir', $run, '-Exe', $Exe, '-Keep')
& powershell @args
if ($LASTEXITCODE) { throw 'Corrupt saved-data test failed' }
$after = (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash
if ($before -ne $after) { throw 'Corrupt file was overwritten' }
if (-not $Keep) { Remove-Item -LiteralPath $directory -Recurse -Force }
Write-Host 'PASS: two independent save roots, process restarts, corrupt file preserved'

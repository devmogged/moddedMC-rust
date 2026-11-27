# Run a coordinated, sequential Compass regression batch. Never invokes Cargo.
# Start only with exclusive runtime authorization. A failed run is retained.
[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [string[]]$Cases,
    [ValidateRange(1024,65534)]
    [int]$Port = 26180,
    [string]$BatchName = '',
    [switch]$KeepPassedWorlds,
    [switch]$ContinueOnFailure
)
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
if (-not $BatchName) { $BatchName = 'compass-batch-' + (Get-Date -Format 'yyyyMMdd-HHmmss') }
if ($BatchName -notmatch '^[a-zA-Z0-9_-]+$') { throw 'BatchName must be a single directory name' }
$runRoot = Join-Path $root 'run'
New-Item -ItemType Directory -Force -Path $runRoot | Out-Null
$runRoot = (Resolve-Path -LiteralPath $runRoot).Path
$evidence = Join-Path $runRoot $BatchName
if (Test-Path -LiteralPath $evidence) { throw "Batch directory already exists: $evidence" }
$valid = @(Get-ChildItem -LiteralPath $PSScriptRoot -Filter 'compasses-*.txt' -File | ForEach-Object {
    $_.BaseName.Substring('compasses-'.Length)
})
foreach ($case in $Cases) {
    if ($case -notin $valid) { throw "Unknown Compass case: $case" }
}
if (@($Cases | Select-Object -Unique).Count -ne $Cases.Count) { throw 'Duplicate case in batch' }
$artifacts = @('target/mods/compasses.mmc','engine/target/pack/pack-dev.exe','engine/target/pack/moddedmc-bot.exe') | ForEach-Object {
    $file = Get-Item -LiteralPath (Join-Path $root $_)
    [pscustomobject]@{
        Path = $_
        Length = $file.Length
        Modified = $file.LastWriteTimeUtc.ToString('o')
        Sha256 = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash
    }
}
function Assert-Artifacts {
    foreach ($artifact in $artifacts) {
        $file = Get-Item -LiteralPath (Join-Path $root $artifact.Path)
        if ($file.Length -ne $artifact.Length -or $file.LastWriteTimeUtc.ToString('o') -ne $artifact.Modified) {
            throw "Build artifact changed during batch: $($artifact.Path)"
        }
    }
}
function Assert-Idle {
    $active = @(Get-Process pack-dev,moddedmc-bot -ErrorAction SilentlyContinue)
    if ($active.Count) { throw "Server/bot process still present: $($active.Id -join ', ')" }
}
Assert-Idle
New-Item -ItemType Directory -Path $evidence | Out-Null
$artifacts | ConvertTo-Json | Set-Content -Encoding utf8 (Join-Path $evidence 'artifacts.json')
$results = Join-Path $evidence 'results.tsv'
"case`texit`tseconds`tlog" | Set-Content -Encoding utf8 $results
$failed = 0
foreach ($case in $Cases) {
    Assert-Artifacts
    Assert-Idle
    $name = "$BatchName-$case"
    $run = Join-Path $runRoot $name
    if (Test-Path -LiteralPath $run) { throw "Case directory already exists: $run" }
    Write-Host "=== $case ==="
    $watch = [Diagnostics.Stopwatch]::StartNew()
    & powershell.exe -NoProfile -File (Join-Path $PSScriptRoot 'run-compass.ps1') -Case $case -Port $Port -RunName $name -Keep
    $code = $LASTEXITCODE
    $watch.Stop()
    $sourceLog = Join-Path $run 'scenario.log'
    $savedLog = Join-Path $evidence "$case.log"
    if (Test-Path -LiteralPath $sourceLog) {
        Copy-Item -LiteralPath $sourceLog -Destination $savedLog
        # A cursor-based final reject does not prove earlier steps were error-free.
        if ($code -eq 0 -and (Select-String -LiteralPath $savedLog -Pattern '\[bot\] error:|(?i)panicked' -Quiet)) {
            Write-Warning "Unexpected bot error/panic anywhere in $case log"
            $code = 1
        }
    } else {
        $code = 1
    }
    "$case`t$code`t$([Math]::Round($watch.Elapsed.TotalSeconds,2))`t$savedLog" | Add-Content -Encoding utf8 $results
    Write-Host "=== $case exit=$code seconds=$([Math]::Round($watch.Elapsed.TotalSeconds,2)) ==="
    Assert-Idle
    Assert-Artifacts
    if ($code -eq 0) {
        if (-not $KeepPassedWorlds) {
            # Delete only the unique passing run directory created above,
            # after saving its complete log. Never delete historical runs.
            $resolved = (Resolve-Path -LiteralPath $run).Path
            if ($resolved -ne (Join-Path $runRoot $name) -or
                -not $resolved.StartsWith($runRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
                throw "Refusing unsafe cleanup: $resolved"
            }
            Remove-Item -LiteralPath $resolved -Recurse -Force
        }
    } else {
        $failed++
        if (-not $ContinueOnFailure) { break }
    }
}
Write-Host "Batch evidence: $evidence"
if ($failed) { exit 1 }
exit 0

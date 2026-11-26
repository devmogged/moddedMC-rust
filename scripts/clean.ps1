# Frees disk taken by test runs and build caches, then prints what was freed.
#   scripts\clean.ps1          run dirs, cross builds, and stale incremental caches (keeps the
#                              newest per crate, so the next build stays incremental). Cheap:
#                              run it at the start of every session.
#   scripts\clean.ps1 -Deep    also debug builds and all incremental caches
#   scripts\clean.ps1 -All     also the pack and release builds (next build starts from scratch)
# Covers target, engine\target and $env:CARGO_TARGET_DIR.
# Stop servers and builds first: a running process keeps its files locked.
param([switch]$Deep, [switch]$All)
$ErrorActionPreference = 'Stop'
if ($All) { $Deep = $true }
$root = Split-Path $PSScriptRoot -Parent
$drive = (Get-Item $root).PSDrive
$before = $drive.Free

$targets = @('target', 'engine\target') | ForEach-Object { Join-Path $root $_ }
if ($env:CARGO_TARGET_DIR) { $targets += $env:CARGO_TARGET_DIR }
$targets = $targets | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -Unique
$paths = @(Get-ChildItem (Join-Path $root 'run') -Directory -ErrorAction SilentlyContinue | ForEach-Object FullName)
foreach ($t in $targets) {
    $paths += Get-ChildItem $t -Directory -Filter '*-unknown-linux-gnu' -ErrorAction SilentlyContinue | ForEach-Object FullName
    foreach ($profile in Get-ChildItem $t -Directory -ErrorAction SilentlyContinue) {
        $inc = Join-Path $profile.FullName 'incremental'
        if (-not (Test-Path -LiteralPath $inc)) { continue }
        if ($Deep) { $paths += $inc; continue }
        # rustc keeps one cache per crate hash and never removes old hashes, so these pile up.
        Get-ChildItem $inc -Directory | Group-Object { $_.Name -replace '-[^-]+$', '' } | ForEach-Object {
            $paths += $_.Group | Sort-Object LastWriteTime -Descending | Select-Object -Skip 1 | ForEach-Object FullName
        }
    }
    if ($Deep) { $paths += Join-Path $t 'debug' }
    if ($All) { $paths += @('pack', 'release') | ForEach-Object { Join-Path $t $_ } }
}
# Target dirs from before .cargo/config.toml shared them.
$paths += @('engine\pumpkin\target', 'app\src-tauri\target') | ForEach-Object { Join-Path $root $_ }

foreach ($p in $paths | Where-Object { Test-Path -LiteralPath $_ }) { Remove-Item -LiteralPath $p -Recurse -Force }
$freed = ((Get-PSDrive $drive.Name).Free - $before) / 1GB
Write-Host ("freed {0:N1} GB; {1:N1} GB free on {2}:" -f $freed, ((Get-PSDrive $drive.Name).Free / 1GB), $drive.Name)

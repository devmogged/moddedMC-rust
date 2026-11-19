# Frees disk taken by test runs and build caches, then prints what was freed.
#   scripts\clean.ps1          run dirs, debug builds, incremental caches, cross builds
#   scripts\clean.ps1 -All     also the pack and release builds (next build starts from scratch)
# Stop servers and builds first: a running process keeps its files locked.
param([switch]$All)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$drive = (Get-Item $root).PSDrive
$before = $drive.Free

$targets = @('target', 'engine\target') | ForEach-Object { Join-Path $root $_ }
$paths = @(Get-ChildItem (Join-Path $root 'run') -Directory -ErrorAction SilentlyContinue | ForEach-Object FullName)
foreach ($t in $targets) {
    $paths += Join-Path $t 'debug'
    $paths += Get-ChildItem $t -Directory -Filter '*-unknown-linux-gnu' -ErrorAction SilentlyContinue | ForEach-Object FullName
    $paths += Get-ChildItem $t -Directory -ErrorAction SilentlyContinue | ForEach-Object { Join-Path $_.FullName 'incremental' }
    if ($All) { $paths += @('pack', 'release') | ForEach-Object { Join-Path $t $_ } }
}
# Target dirs from before .cargo/config.toml shared them.
$paths += @('engine\pumpkin\target', 'app\src-tauri\target') | ForEach-Object { Join-Path $root $_ }

foreach ($p in $paths | Where-Object { Test-Path -LiteralPath $_ }) { Remove-Item -LiteralPath $p -Recurse -Force }
$freed = ((Get-PSDrive $drive.Name).Free - $before) / 1GB
Write-Host ("freed {0:N1} GB; {1:N1} GB free on {2}:" -f $freed, ((Get-PSDrive $drive.Name).Free / 1GB), $drive.Name)

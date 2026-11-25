# Build a deliberately small ABI test mod outside the primary workspace and pack its Windows
# library as a native drop-in. Only used by tests/scenarios/abi-random-tick.txt.
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..\..')).Path
$manifest = Join-Path $PSScriptRoot 'Cargo.toml'
$env:CARGO_TARGET_DIR = Join-Path $root 'target'
cargo build --release --manifest-path $manifest
if ($LASTEXITCODE) { throw "fixture build failed ($LASTEXITCODE)" }

$library = Join-Path $root 'target\release\abi_random_tick.dll'
$outDir = Join-Path $root 'target\mods'
New-Item -ItemType Directory -Force $outDir | Out-Null
$out = Join-Path $outDir 'abi-random-tick.mmc'
Remove-Item -LiteralPath $out -ErrorAction SilentlyContinue
Add-Type -AssemblyName System.IO.Compression,System.IO.Compression.FileSystem
$zip = [IO.Compression.ZipFile]::Open($out, 'Create')
try {
    $entry = $zip.CreateEntry('mod.json').Open()
    $bytes = [Text.Encoding]::UTF8.GetBytes('{"id":"abi_random_tick","version":"0.1.0","abi":1}')
    $entry.Write($bytes, 0, $bytes.Length)
    $entry.Dispose()
    [IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip, $library, 'windows-x86_64.dll') | Out-Null
} finally { $zip.Dispose() }
Write-Host "packed $out"

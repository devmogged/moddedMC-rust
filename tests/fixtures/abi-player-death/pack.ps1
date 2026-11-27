# Build the claimant and observer for tests/scenarios/abi-player-death.txt.
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..\..')).Path
$manifest = Join-Path $PSScriptRoot 'Cargo.toml'
$env:CARGO_TARGET_DIR = Join-Path $root 'target'
$library = Join-Path $root 'target\release\abi_player_death.dll'
$outDir = Join-Path $root 'target\mods'
New-Item -ItemType Directory -Force $outDir | Out-Null
Add-Type -AssemblyName System.IO.Compression,System.IO.Compression.FileSystem

function Pack-Fixture([string] $Name, [string] $Id, [string[]] $Features) {
    cargo build --release --manifest-path $manifest @Features
    if ($LASTEXITCODE) { throw "fixture build failed ($LASTEXITCODE)" }
    $out = Join-Path $outDir "$Name.mmc"
    Remove-Item -LiteralPath $out -ErrorAction SilentlyContinue
    $zip = [IO.Compression.ZipFile]::Open($out, 'Create')
    try {
        $entry = $zip.CreateEntry('mod.json').Open()
        $meta = @{id = $Id; version = '0.1.0'; abi = 1} | ConvertTo-Json -Compress
        $bytes = [Text.Encoding]::UTF8.GetBytes($meta)
        $entry.Write($bytes, 0, $bytes.Length)
        $entry.Dispose()
        [IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip, $library, 'windows-x86_64.dll') | Out-Null
    } finally { $zip.Dispose() }
    Write-Host "packed $out"
}

Pack-Fixture 'abi-player-death' 'abi_player_death' @()
Pack-Fixture 'z-abi-player-death-observer' 'abi_player_death_observer' @('--no-default-features', '--features', 'observer')

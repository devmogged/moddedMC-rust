# Package independent provider/consumer DLLs; consumer sorts before provider.
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..\..')).Path
$env:CARGO_TARGET_DIR = Join-Path $root 'target'
Add-Type -AssemblyName System.IO.Compression,System.IO.Compression.FileSystem
$dir = Join-Path $root 'target\mods'
New-Item -ItemType Directory -Force $dir | Out-Null
foreach ($kind in @('provider','consumer')) {
    $manifest = Join-Path $PSScriptRoot "$kind\Cargo.toml"
    cargo build --release --manifest-path $manifest
    if ($LASTEXITCODE) { throw "Could not build $kind test mod" }
    $crate = "abi_$kind"
    $library = Join-Path $root "target\release\$crate.dll"
    $filename = if ($kind -eq 'provider') { 'z-abi-provider.mmc' } else { 'a-abi-consumer.mmc' }
    $id = "abi_$kind"
    [string[]]$depends = @()
    if ($kind -eq 'consumer') { $depends = [string[]]@('abi_provider') }
    $json = @{ id = $id; version = '0.1.0'; abi = 1; dependencies = $depends } | ConvertTo-Json -Compress
    $out = Join-Path $dir $filename
    Remove-Item -LiteralPath $out -Force -ErrorAction SilentlyContinue
    $zip = [IO.Compression.ZipFile]::Open($out, 'Create')
    try {
        $entry = $zip.CreateEntry('mod.json').Open()
        $bytes = [Text.Encoding]::UTF8.GetBytes($json)
        $entry.Write($bytes, 0, $bytes.Length)
        $entry.Dispose()
        [IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip, $library, 'windows-x86_64.dll') | Out-Null
    } finally { $zip.Dispose() }
    Write-Output "packed $out"
}
# Publish a cyclic fixture without recompiling: both modules must be skipped.
$cycle = Join-Path $dir 'z-abi-provider-cycle.mmc'
Remove-Item -LiteralPath $cycle -Force -ErrorAction SilentlyContinue
$zip = [IO.Compression.ZipFile]::Open($cycle, 'Create')
try {
    $data = @{ id = 'abi_provider'; version = '0.1.0'; abi = 1; dependencies = [string[]]@('abi_consumer') } | ConvertTo-Json -Compress
    $entry = $zip.CreateEntry('mod.json').Open()
    $bytes = [Text.Encoding]::UTF8.GetBytes($data)
    $entry.Write($bytes, 0, $bytes.Length)
    $entry.Dispose()
    $providerLibrary = Join-Path $root 'target\release\abi_provider.dll'
    [IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip, $providerLibrary, 'windows-x86_64.dll') | Out-Null
} finally { $zip.Dispose() }
Write-Output "packed $cycle"

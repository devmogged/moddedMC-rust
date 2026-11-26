# Small independent ABI fixture; only pack under the prime's build gate.
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..\..')).Path
$env:CARGO_TARGET_DIR = Join-Path $root 'target'
cargo build --release --manifest-path (Join-Path $PSScriptRoot 'Cargo.toml')
if ($LASTEXITCODE) { throw "fixture build failed ($LASTEXITCODE)" }
$library = Join-Path $root 'target\release\abi_content_replacement.dll'
$outDir = Join-Path $root 'target\mods'
New-Item -ItemType Directory -Force $outDir | Out-Null
$out = Join-Path $outDir 'abi-content-replacement.mmc'
$scratch = "$out.$([Guid]::NewGuid()).tmp"
$abi = [regex]::Match((Get-Content (Join-Path $root 'api\src\abi.rs') -Raw), 'ABI_VERSION: u32 = (\d+)').Groups[1].Value
Add-Type -AssemblyName System.IO.Compression,System.IO.Compression.FileSystem
$zip = [IO.Compression.ZipFile]::Open($scratch, 'Create')
try {
    $entry = $zip.CreateEntry('mod.json').Open()
    $bytes = [Text.Encoding]::UTF8.GetBytes("{`"id`":`"abi_content_replacement`",`"version`":`"0.1.0`",`"abi`":$abi}")
    $entry.Write($bytes, 0, $bytes.Length)
    $entry.Dispose()
    [IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip, $library, 'windows-x86_64.dll') | Out-Null
    $zip.Dispose()
    if ([IO.File]::Exists($out)) {
        [IO.File]::Replace($scratch, $out, [NullString]::Value)
    } else {
        [IO.File]::Move($scratch, $out)
    }
} finally {
    $zip.Dispose()
    if ([IO.File]::Exists($scratch)) { [IO.File]::Delete($scratch) }
}
Write-Host "packed $out"

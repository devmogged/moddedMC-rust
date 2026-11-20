$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..\..')).Path
$env:CARGO_TARGET_DIR = Join-Path $root 'target'
cargo build --release --manifest-path (Join-Path $PSScriptRoot 'Cargo.toml')
if ($LASTEXITCODE) { throw 'Could not build analog fixture' }
Add-Type -AssemblyName System.IO.Compression, System.IO.Compression.FileSystem
$dir = Join-Path $root 'target\mods'
New-Item -ItemType Directory -Force $dir | Out-Null
$dest = Join-Path $dir 'abi-analog.mmc'
if (Test-Path $dest) { Remove-Item -LiteralPath $dest -Force }
$zip = [IO.Compression.ZipFile]::Open($dest, 'Create')
try {
    $entry = $zip.CreateEntry('mod.json').Open()
    $bytes = [Text.Encoding]::UTF8.GetBytes('{"id":"abi_analog","version":"0.1.0","abi":1}')
    $entry.Write($bytes, 0, $bytes.Length)
    $entry.Dispose()
    $dll = Join-Path $root 'target\release\abi_analog.dll'
    [IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip, $dll, 'windows-x86_64.dll') | Out-Null
} finally { $zip.Dispose() }
Write-Output "packed $dest"

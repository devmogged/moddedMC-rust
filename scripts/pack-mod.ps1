# Builds a Rust mod and packs it as <OutDir>\<crate>.mmc, the file server owners drop into mods/.
#   scripts\pack-mod.ps1 -Crate wayshrine [-OutDir target\mods] [-Pi]
# A .mmc is a zip: mod.json (id, version, abi) plus one library per platform, named
# <os>-<arch>.<ext> (windows-x86_64.dll, linux-x86_64.so, macos-aarch64.dylib). This script adds
# the library for the machine it runs on; add other platforms' builds to the zip the same way.
# -Pi also adds linux-aarch64.so (Raspberry Pi 5), cross-built with cargo-zigbuild and zig on PATH.
# The crate's assets/ folder, if any, goes in as is (resource pack files: textures, models, lang).
param(
    [Parameter(Mandatory)] [string] $Crate,
    [string] $OutDir,  # default: target\mods
    [switch] $Pi
)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
if (-not $OutDir) { $OutDir = Join-Path $root 'target\mods' }
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"

Push-Location $root
try {
    cargo build --release -p $Crate
    if ($LASTEXITCODE) { throw "cargo failed ($LASTEXITCODE)" }
    $version = ((cargo pkgid -p $Crate) -split '[@#]')[-1]
    if ($Pi) {
        cargo-zigbuild build --release -p $Crate --target aarch64-unknown-linux-gnu.2.36
        if ($LASTEXITCODE) { throw "cargo-zigbuild failed ($LASTEXITCODE)" }
    }
} finally { Pop-Location }

$abi = [regex]::Match((Get-Content (Join-Path $root 'api\src\abi.rs') -Raw), 'ABI_VERSION: u32 = (\d+)').Groups[1].Value
$arch = @{ AMD64 = 'x86_64'; ARM64 = 'aarch64' }[$env:PROCESSOR_ARCHITECTURE]
$library = Join-Path $root "target\release\$($Crate -replace '-', '_').dll"

Add-Type -AssemblyName System.IO.Compression, System.IO.Compression.FileSystem
New-Item -ItemType Directory -Force $OutDir | Out-Null
$out = Join-Path (Resolve-Path $OutDir) "$Crate.mmc"
Remove-Item $out -ErrorAction SilentlyContinue
$zip = [IO.Compression.ZipFile]::Open($out, 'Create')
try {
    $manifest = $zip.CreateEntry('mod.json').Open()
    $bytes = [Text.Encoding]::UTF8.GetBytes((@{ id = $Crate; version = $version; abi = [int]$abi } | ConvertTo-Json -Compress))
    $manifest.Write($bytes, 0, $bytes.Length)
    $manifest.Dispose()
    [IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip, $library, "windows-$arch.dll") | Out-Null
    if ($Pi) {
        $so = Join-Path $root "target\aarch64-unknown-linux-gnu\release\lib$($Crate -replace '-', '_').so"
        [IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip, $so, 'linux-aarch64.so') | Out-Null
    }
    # Resource pack files: the server merges every mod's assets/ into one pack for players.
    $assets = Join-Path $root "mods\$Crate\assets"
    if (Test-Path $assets) {
        foreach ($f in Get-ChildItem $assets -Recurse -File) {
            $name = 'assets/' + $f.FullName.Substring($assets.Length + 1).Replace('\', '/')
            [IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip, $f.FullName, $name) | Out-Null
        }
    }
} finally { $zip.Dispose() }
Write-Host "packed $out ($((Get-Item $out).Length) bytes)"

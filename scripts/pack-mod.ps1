# Builds a Rust mod and packs it as <OutDir>\<crate>.mmc, the file server owners drop into mods/.
#   scripts\pack-mod.ps1 -Crate wayshrine [-OutDir target\mods]
# A .mmc is a zip: mod.json (id, version, abi) plus one library per platform, named
# <os>-<arch>.<ext> (windows-x86_64.dll, linux-x86_64.so, macos-aarch64.dylib), plus the crate's
# resources\data\ and resources\assets\ as data\ and assets\. This script adds the library for the machine it runs on; add other
# platforms' builds to the zip the same way.
param(
    [Parameter(Mandatory)] [string] $Crate,
    [string] $OutDir   # default: target\mods
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
    # resources\data\ loads like a datapack; resources\assets\ goes into the players' resource pack.
    foreach ($folder in 'data', 'assets') {
        $dir = Join-Path $root "mods\$Crate\resources\$folder"
        if (-not (Test-Path -LiteralPath $dir)) { continue }
        Get-ChildItem -LiteralPath $dir -Recurse -File | ForEach-Object {
            $name = "$folder/" + $_.FullName.Substring($dir.Length + 1).Replace('\', '/')
            [IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip, $_.FullName, $name) | Out-Null
        }
    }
    $license = Join-Path $root "mods\$Crate\LICENSE.md"
    if (Test-Path -LiteralPath $license) {
        [IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip, $license, 'LICENSE.md') | Out-Null
    }
} finally { $zip.Dispose() }
Write-Host "packed $out ($((Get-Item $out).Length) bytes)"

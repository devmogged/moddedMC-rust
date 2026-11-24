# Builds a Rust mod and packs it as <OutDir>\<crate>.mmc, the file server owners drop into mods/.
#   scripts\pack-mod.ps1 -Crate wayshrine [-OutDir target\mods] [-Dependencies create,other] [-AssetPack path.zip]
# A .mmc is a zip: mod.json (id, version, abi, optional dependencies) plus one library per platform, named
# <os>-<arch>.<ext> (windows-x86_64.dll, linux-x86_64.so, macos-aarch64.dylib), plus the crate's
# resources\data\ and resources\assets\ as data\ and assets\. This script adds the library for the machine it runs on; add other
# platforms' builds to the zip the same way.
param(
    [Parameter(Mandatory)] [string] $Crate,
    [string] $OutDir,  # default: target\mods
    [string[]] $Dependencies = @(),
    [string] $AssetPack,
    [switch] $IncludeStructureTemplates,
    [string[]] $Features = @()
)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
if (-not $OutDir) { $OutDir = Join-Path $root 'target\mods' }
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"

Push-Location $root
try {
    $buildArgs = @('build', '--release', '-p', $Crate)
    if ($Features.Count) { $buildArgs += @('--features', ($Features -join ',')) }
    cargo @buildArgs
    if ($LASTEXITCODE) { throw "cargo failed ($LASTEXITCODE)" }
    $version = ((cargo pkgid -p $Crate) -split '[@#]')[-1]
} finally { Pop-Location }

$abi = [regex]::Match((Get-Content (Join-Path $root 'api\src\abi.rs') -Raw), 'ABI_VERSION: u32 = (\d+)').Groups[1].Value
$arch = @{ AMD64 = 'x86_64'; ARM64 = 'aarch64' }[$env:PROCESSOR_ARCHITECTURE]
$library = Join-Path $root "target\release\$($Crate -replace '-', '_').dll"

$metadata = @{ id = $Crate; version = $version; abi = [int]$abi }
$deps = [Collections.Generic.List[string]]::new()
foreach ($group in $Dependencies) {
    foreach ($entry in $group.Split(',')) {
        $dep = $entry.Trim()
        if ($dep -cnotmatch '^[a-z0-9_.-]+$' -or $dep -eq $Crate -or $deps.Contains($dep)) {
            throw "Invalid, self or duplicate dependency: $dep"
        }
        $deps.Add($dep)
    }
}
if ($deps.Count -gt 32) { throw 'Too many dependencies (maximum 32)' }
if ($deps.Count -gt 0) { $metadata.dependencies = [string[]]$deps.ToArray() }
$metadataJson = $metadata | ConvertTo-Json -Compress

Add-Type -AssemblyName System.IO.Compression, System.IO.Compression.FileSystem
New-Item -ItemType Directory -Force $OutDir | Out-Null
$out = Join-Path (Resolve-Path $OutDir) "$Crate.mmc"
$scratch = "$out.$([Guid]::NewGuid()).tmp"
$names = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
$zip = [IO.Compression.ZipFile]::Open($scratch, 'Create')
try {
    $manifest = $zip.CreateEntry('mod.json').Open()
    $bytes = [Text.Encoding]::UTF8.GetBytes($metadataJson)
    $manifest.Write($bytes, 0, $bytes.Length)
    $manifest.Dispose()
    [IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip, $library, "windows-$arch.dll") | Out-Null
    # resources\data\ loads like a datapack; resources\assets\ goes into the players' resource pack.
    foreach ($folder in 'data', 'assets') {
        $dir = Join-Path $root "mods\$Crate\resources\$folder"
        if (-not (Test-Path -LiteralPath $dir)) { continue }
        Get-ChildItem -LiteralPath $dir -Recurse -File | ForEach-Object {
            $name = "$folder/" + $_.FullName.Substring($dir.Length + 1).Replace('\', '/')
            [void]$names.Add($name)
            [IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip, $_.FullName, $name) | Out-Null
        }
    }
    if ($AssetPack) {
        $assets = [IO.Compression.ZipFile]::OpenRead((Resolve-Path -LiteralPath $AssetPack).Path)
        try {
            foreach ($entry in $assets.Entries) {
                $name = $entry.FullName
                $isAsset = $name.StartsWith('assets/', [StringComparison]::Ordinal)
                $isTemplate = $IncludeStructureTemplates -and $name -cmatch '^data/[a-z0-9_.-]+/structure/([a-z0-9_.-]+/)*[a-z0-9_.-]+\.nbt$'
                if ((!$isAsset -and !$isTemplate) -or !$entry.Name) { continue }
                if (($isAsset -and $name -cnotmatch '^assets/[a-z0-9_.-]+(/[a-z0-9_.-]+)+$') -or
                    ($name -split '/') -contains '..' -or ($name -split '/') -contains '.') {
                    throw "Invalid resource path in AssetPack: $name"
                }
                if (!$names.Add($name)) { throw "Duplicate resource path: $name" }
                if ($entry.Length -gt 8MB) { throw "Resource exceeds the host's asset limit: $name" }
                $reader = $entry.Open()
                try {
                    $writer = $zip.CreateEntry($name).Open()
                    try { $reader.CopyTo($writer) } finally { $writer.Dispose() }
                } finally { $reader.Dispose() }
            }
        } finally { $assets.Dispose() }
    }
    $license = Join-Path $root "mods\$Crate\LICENSE.md"
    if (Test-Path -LiteralPath $license) {
        [IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip, $license, 'LICENSE.md') | Out-Null
    }
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
Write-Host "packed $out ($((Get-Item $out).Length) bytes)"

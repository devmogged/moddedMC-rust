# Package the 0.9.88 Rust port and its approved visual and audio assets in one local .mmc.
param(
    [Parameter(Mandatory)] [string] $Jar,
    [string] $OutDir
)
$ErrorActionPreference = 'Stop'
$assetPack = & "$PSScriptRoot/stage-cindervane-pack.ps1" -Jar $Jar -AllDragons
& "$PSScriptRoot/pack-mod.ps1" -Crate saints-dragons -OutDir $OutDir -AssetPack $assetPack -IncludeStructureTemplates -Features original-assets

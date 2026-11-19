# Creates mods\<id>\, a mod crate that builds and runs as is: one block with its model, texture,
# recipe, loot table and mineable tag, plus tests\scenarios\<id>.txt to check it on a server.
#   scripts\new-mod.ps1 -Id ember [-Name "Ember"]
# Then: scripts\pack-mod.ps1 -Crate ember, and
#       scripts\scenario.ps1 -Scenario tests\scenarios\ember.txt -RunDir run\ember -Fresh
param(
    [Parameter(Mandatory)] [ValidatePattern('^[a-z][a-z0-9_]*$')] [string] $Id,
    [string] $Name
)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$dir = Join-Path $root "mods\$Id"
if (Test-Path -LiteralPath $dir) { throw "mods\$Id already exists" }
if (-not $Name) { $Name = (Get-Culture).TextInfo.ToTitleCase($Id.Replace('_', ' ')) }
$type = ($Name -replace '[^A-Za-z0-9]', '')
if ($type -notmatch '^[A-Za-z]') { $type = "Mod$type" }

function Write-File([string] $relative, [string] $text) {
    $path = Join-Path $dir $relative.Replace('{{id}}', $Id)
    New-Item -ItemType Directory -Force (Split-Path $path -Parent) | Out-Null
    $text = $text.Replace('{{id}}', $Id).Replace('{{name}}', $Name).Replace('{{type}}', $type)
    [IO.File]::WriteAllText($path, $text.Replace("`r`n", "`n"))
}

Write-File 'Cargo.toml' @'
[package]
name = "{{id}}"
version = "0.1.0"
edition.workspace = true
license.workspace = true
description = "{{name}}"

[lib]
crate-type = ["cdylib"]

[dependencies]
moddedmc-api.workspace = true
'@

Write-File 'src\lib.rs' @'
//! {{name}}. Behaviour lives here; recipes, loot tables and tags live in `resources/data/`, models
//! and textures in `resources/assets/`. See docs/PORTS.md and docs/MOD-ABI.md.

use std::sync::OnceLock;

use moddedmc_api::{
    InterfaceUnavailable, Mod, RegistrationError, Registry,
    content::{self, Block, Content, State},
};

const EXAMPLE_BLOCK: &str = "{{id}}:example_block";

static CONTENT: OnceLock<Content> = OnceLock::new();

struct {{type}};

impl Mod for {{type}} {
    fn start(&self) -> Result<(), InterfaceUnavailable> {
        let _ = CONTENT.set(content::require()?);
        Ok(())
    }

    fn register(&self, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
        let content = CONTENT.get().ok_or(RegistrationError::InvalidDefinition)?;
        content.add_block(
            registry,
            &Block {
                id: EXAMPLE_BLOCK,
                display_name: "Example Block",
                entity: "",
                item_model: "{{id}}:item/example_block",
                states: &[State {
                    properties: "",
                    // Hardness, sounds and collision come from this vanilla block.
                    physical: "minecraft:stone",
                    model: "{{id}}:block/example_block",
                    rotating_model: "",
                    x: 0,
                    y: 0,
                    orientation: [0.0, 0.0, 0.0, 1.0],
                    shape: &[],
                }],
            },
            None,
        )
    }
}

moddedmc_api::export_mod!({{type}});
'@

Write-File 'resources\assets\{{id}}\models\block\example_block.json' @'
{
  "parent": "minecraft:block/cube_all",
  "textures": {
    "all": "{{id}}:block/example_block"
  }
}
'@

Write-File 'resources\assets\{{id}}\models\item\example_block.json' @'
{
  "parent": "{{id}}:block/example_block"
}
'@

Write-File 'resources\data\{{id}}\loot_table\blocks\example_block.json' @'
{
  "type": "minecraft:block",
  "pools": [
    {
      "rolls": 1,
      "entries": [{ "type": "minecraft:item", "name": "{{id}}:example_block" }],
      "conditions": [{ "condition": "minecraft:survives_explosion" }]
    }
  ]
}
'@

Write-File 'resources\data\{{id}}\recipe\example_block.json' @'
{
  "type": "minecraft:crafting_shapeless",
  "ingredients": ["minecraft:cobblestone", "minecraft:redstone"],
  "result": { "id": "{{id}}:example_block", "count": 1 }
}
'@

Write-File 'resources\data\minecraft\tags\block\mineable\pickaxe.json' @'
{
  "values": ["{{id}}:example_block"]
}
'@

# A 16x16 two-tone texture, so the block is visibly the mod's own.
Add-Type -AssemblyName System.Drawing
$texture = Join-Path $dir "resources\assets\$Id\textures\block\example_block.png"
New-Item -ItemType Directory -Force (Split-Path $texture -Parent) | Out-Null
$bitmap = [Drawing.Bitmap]::new(16, 16)
try {
    $hue = [Math]::Abs($Id.GetHashCode()) % 360
    $light = [Drawing.Color]::FromArgb(255, [int](128 + 100 * [Math]::Cos($hue * [Math]::PI / 180)), [int](128 + 100 * [Math]::Cos(($hue - 120) * [Math]::PI / 180)), [int](128 + 100 * [Math]::Cos(($hue + 120) * [Math]::PI / 180)))
    $dark = [Drawing.Color]::FromArgb(255, [int]($light.R * 0.6), [int]($light.G * 0.6), [int]($light.B * 0.6))
    for ($x = 0; $x -lt 16; $x++) {
        for ($y = 0; $y -lt 16; $y++) {
            $edge = $x -eq 0 -or $y -eq 0 -or $x -eq 15 -or $y -eq 15
            $bitmap.SetPixel($x, $y, $(if ($edge -or (($x -band 4) -xor ($y -band 4))) { $dark } else { $light }))
        }
    }
    $bitmap.Save($texture, [Drawing.Imaging.ImageFormat]::Png)
} finally { $bitmap.Dispose() }

$scenario = Join-Path $root "tests\scenarios\$Id.txt"
[IO.File]::WriteAllText($scenario, @"
# $Name on a fresh server. Build with scripts/pack-mod.ps1 -Crate $Id and scripts/engine.ps1 build.
# Run scripts/scenario.ps1 -Scenario tests/scenarios/$Id.txt -RunDir run/$Id -Fresh
mods target\mods\$Id.mmc
start
expect registered content block ${Id}:example_block
expect Loaded built-in datapack 'mod/$Id': 1 recipe\(s\), 1 loot table\(s\), 1 tag\(s\)
botstart tester
wait 3
expect \[bot\] fetched content resource pack
expect \[bot\] joined as tester
cmd setblock 0 100 0 ${Id}:example_block
wait 1
cmd execute if block 0 100 0 #minecraft:mineable/pickaxe
wait 1
expect Test passed
cmd loot spawn 0 101 0 mine 0 100 0
wait 1
expect Dropped 1 .*Example Block
reject ERROR|panicked
stop
"@.Replace("`r`n", "`n"))

Write-Host "created mods\$Id and tests\scenarios\$Id.txt"

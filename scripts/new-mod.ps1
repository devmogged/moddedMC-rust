# Creates mods\<id>\, a mod crate that builds and runs as is: one block with its model, texture,
# recipe, loot table and mineable tag, one item with its model and texture, plus tests\scenarios\<id>.txt to check it on a server.
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
    items::{self, Item, Items},
};

const EXAMPLE_BLOCK: &str = "{{id}}:example_block";
const EXAMPLE_ITEM: &str = "{{id}}:example_item";

static CONTENT: OnceLock<Content> = OnceLock::new();
static ITEMS: OnceLock<Items> = OnceLock::new();

struct {{type}};

impl Mod for {{type}} {
    fn start(&self) -> Result<(), InterfaceUnavailable> {
        let _ = CONTENT.set(content::require()?);
        let _ = ITEMS.set(items::require()?);
        Ok(())
    }

    fn register(&self, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
        ITEMS.get().ok_or(RegistrationError::InvalidDefinition)?.add_item(
            registry,
            &Item {
                id: EXAMPLE_ITEM,
                display_name: "Example Item",
                // Stack size and other components come from this vanilla item.
                carrier: "minecraft:iron_ingot",
                model: "{{id}}:item/example_item",
            },
        )?;
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

Write-File 'resources\assets\{{id}}\models\item\example_item.json' @'
{
  "parent": "minecraft:item/generated",
  "textures": {
    "layer0": "{{id}}:item/example_item"
  }
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

# 16x16 two-tone textures, so the block and the item are visibly the mod's own.
Add-Type -AssemblyName System.Drawing
$hue = [Math]::Abs($Id.GetHashCode()) % 360
$light = [Drawing.Color]::FromArgb(255, [int](128 + 100 * [Math]::Cos($hue * [Math]::PI / 180)), [int](128 + 100 * [Math]::Cos(($hue - 120) * [Math]::PI / 180)), [int](128 + 100 * [Math]::Cos(($hue + 120) * [Math]::PI / 180)))
$dark = [Drawing.Color]::FromArgb(255, [int]($light.R * 0.6), [int]($light.G * 0.6), [int]($light.B * 0.6))
function Write-Texture([string] $relative, [scriptblock] $pixel) {
    $texture = Join-Path $dir "resources\assets\$Id\textures\$relative"
    New-Item -ItemType Directory -Force (Split-Path $texture -Parent) | Out-Null
    $bitmap = [Drawing.Bitmap]::new(16, 16)
    try {
        for ($x = 0; $x -lt 16; $x++) {
            for ($y = 0; $y -lt 16; $y++) { $bitmap.SetPixel($x, $y, (& $pixel $x $y)) }
        }
        $bitmap.Save($texture, [Drawing.Imaging.ImageFormat]::Png)
    } finally { $bitmap.Dispose() }
}
Write-Texture 'block\example_block.png' {
    param($x, $y)
    $edge = $x -eq 0 -or $y -eq 0 -or $x -eq 15 -or $y -eq 15
    if ($edge -or (($x -band 4) -xor ($y -band 4))) { $dark } else { $light }
}
# A diamond on a transparent background.
Write-Texture 'item\example_item.png' {
    param($x, $y)
    $distance = [Math]::Abs($x - 7.5) + [Math]::Abs($y - 7.5)
    if ($distance -lt 5) { $light } elseif ($distance -lt 7) { $dark } else { [Drawing.Color]::Transparent }
}

$scenario = Join-Path $root "tests\scenarios\$Id.txt"
[IO.File]::WriteAllText($scenario, @"
# $Name on a fresh server. Build with scripts/pack-mod.ps1 -Crate $Id and scripts/engine.ps1 build.
# Run scripts/scenario.ps1 -Scenario tests/scenarios/$Id.txt -RunDir run/$Id -Fresh
mods target\mods\$Id.mmc
start
expect registered content item ${Id}:example_item
expect registered content block ${Id}:example_block
expect Loaded built-in datapack 'mod/$Id': 1 recipe\(s\), 1 loot table\(s\), 1 tag\(s\)
botstart tester
wait 3
expect \[bot\] fetched content resource pack
expect \[bot\] joined as tester
cmd tp tester 0.5 101 0.5
wait 3
cmd setblock 0 100 0 ${Id}:example_block
wait 1
cmd execute if block 0 100 0 #minecraft:mineable/pickaxe
wait 1
expect Test passed
cmd loot spawn 0 101 0 mine 0 100 0
wait 1
expect Dropped 1 .*Example Block
cmd give tester ${Id}:example_item 3
wait 1
expect Gave 3 .*Example Item
reject ERROR|panicked
stop
"@.Replace("`r`n", "`n"))

Write-Host "created mods\$Id and tests\scenarios\$Id.txt"

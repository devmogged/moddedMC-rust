//! Native 2.0.9 block IDs. Stock-client states use vanilla barrel carriers;
//! the generated pack supplies consistent built-in vanilla placeholder models.

use crate::network::{Node, Role};
use moddedmc_api::{
    RegistrationError, Registry,
    content::{Block, BlockMaterial, Content, Placement, State},
};
use std::sync::OnceLock;

#[derive(Clone, Copy)]
pub struct Definition {
    pub id: &'static str,
    pub label: &'static str,
    pub role: Role,
    pub model: &'static str,
}
pub const BLOCKS: &[Definition] = &[
    Definition {
        id: "refinedstorage:controller",
        label: "Controller",
        role: Role::Controller,
        model: "minecraft:block/diamond_block",
    },
    Definition {
        id: "refinedstorage:cable",
        label: "Cable",
        role: Role::Cable,
        model: "minecraft:block/iron_block",
    },
    Definition {
        id: "refinedstorage:disk_drive",
        label: "Disk Drive",
        role: Role::Drive,
        model: "minecraft:block/barrel",
    },
    Definition {
        id: "refinedstorage:1k_storage_block",
        label: "1k Storage Block",
        role: Role::Storage,
        model: "minecraft:block/barrel",
    },
    Definition {
        id: "refinedstorage:4k_storage_block",
        label: "4k Storage Block",
        role: Role::Storage,
        model: "minecraft:block/barrel",
    },
    Definition {
        id: "refinedstorage:16k_storage_block",
        label: "16k Storage Block",
        role: Role::Storage,
        model: "minecraft:block/barrel",
    },
    Definition {
        id: "refinedstorage:64k_storage_block",
        label: "64k Storage Block",
        role: Role::Storage,
        model: "minecraft:block/barrel",
    },
    Definition {
        id: "refinedstorage:grid",
        label: "Grid",
        role: Role::Grid,
        model: "minecraft:block/lectern",
    },
    Definition {
        id: "refinedstorage:crafting_grid",
        label: "Crafting Grid",
        role: Role::CraftingGrid,
        model: "minecraft:block/crafting_table",
    },
    Definition {
        id: "refinedstorage:pattern_grid",
        label: "Pattern Grid",
        role: Role::PatternGrid,
        model: "minecraft:block/crafting_table",
    },
    Definition {
        id: "refinedstorage:autocrafter",
        label: "Autocrafter",
        role: Role::Autocrafter,
        model: "minecraft:block/dispenser",
    },
    Definition {
        id: "refinedstorage:crafter",
        label: "Crafter",
        role: Role::Crafter,
        model: "minecraft:block/dropper",
    },
    Definition {
        id: "refinedstorage:crafter_manager",
        label: "Crafter Manager",
        role: Role::CrafterManager,
        model: "minecraft:block/lectern",
    },
    Definition {
        id: "refinedstorage:external_storage",
        label: "External Storage",
        role: Role::External,
        model: "minecraft:block/hopper",
    },
    Definition {
        id: "refinedstorage:importer",
        label: "Importer",
        role: Role::Importer,
        model: "minecraft:block/hopper",
    },
    Definition {
        id: "refinedstorage:exporter",
        label: "Exporter",
        role: Role::Exporter,
        model: "minecraft:block/dropper",
    },
    Definition {
        id: "refinedstorage:wireless_transmitter",
        label: "Wireless Transmitter",
        role: Role::Wireless,
        model: "minecraft:block/amethyst_block",
    },
];
const FACING: [&str; 6] = ["down", "up", "north", "south", "west", "east"];

fn placed(ctx: Placement<'_>) -> Option<u32> {
    let runtime = crate::RUNTIME.get()?;
    let def = kind(runtime.content, ctx.state)?.0;
    runtime.content.state(&format!(
        "{}[facing={}]",
        def.id,
        FACING.get(ctx.face as usize)?
    ))
}

pub fn register(content: Content, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
    for def in BLOCKS {
        let properties: Vec<_> = FACING.iter().map(|f| format!("facing={f}")).collect();
        let physical: Vec<_> = FACING
            .iter()
            .map(|f| format!("minecraft:barrel[facing={f},open=false]"))
            .collect();
        let states: Vec<_> = properties
            .iter()
            .zip(physical.iter())
            .map(|(properties, physical)| State {
                properties,
                physical,
                model: def.model,
                rotating_model: "",
                x: 0,
                y: 0,
                orientation: [0.0, 0.0, 0.0, 1.0],
                shape: &[],
            })
            .collect();
        content.add_block(
            registry,
            &Block {
                id: def.id,
                display_name: def.label,
                entity: def.id,
                item_model: "minecraft:item/barrel",
                states: &states,
            },
            Some(placed),
        )?;
        content.set_block_material(
            registry,
            def.id,
            &BlockMaterial {
                template: "minecraft:barrel",
                hardness: Some(1.5),
                map_color: None,
            },
        )?;
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct Mapping {
    state: u32,
    def: Definition,
    face: u8,
}
static STATES: OnceLock<Vec<Mapping>> = OnceLock::new();

pub fn kind(content: Content, state: u32) -> Option<(Definition, u8)> {
    let mappings = STATES.get_or_init(|| {
        BLOCKS
            .iter()
            .flat_map(|def| {
                FACING.iter().enumerate().filter_map(move |(face, name)| {
                    Some(Mapping {
                        state: content.state(&format!("{}[facing={}]", def.id, name))?,
                        def: *def,
                        face: face as u8,
                    })
                })
            })
            .collect()
    });
    mappings
        .iter()
        .find(|mapping| mapping.state == state)
        .map(|mapping| (mapping.def, mapping.face))
}

pub fn node(content: Content, state: u32) -> Option<Node> {
    kind(content, state).map(|(def, face)| Node {
        role: def.role,
        face,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_ids_are_distinct_and_include_core_progression() {
        let names: std::collections::HashSet<_> = BLOCKS.iter().map(|d| d.id).collect();
        assert_eq!(names.len(), BLOCKS.len());
        for id in [
            "refinedstorage:controller",
            "refinedstorage:disk_drive",
            "refinedstorage:crafting_grid",
            "refinedstorage:importer",
            "refinedstorage:wireless_transmitter",
            "refinedstorage:crafter",
            "refinedstorage:crafter_manager",
        ] {
            assert!(names.contains(id), "missing {id}");
        }
    }
}

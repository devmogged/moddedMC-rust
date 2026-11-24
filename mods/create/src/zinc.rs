//! Zinc ore and storage blocks from Create's AllBlocks and AllItems.

use moddedmc_api::{
    RegistrationError, Registry,
    content::{Block, Content, State},
    items::Item,
};

use crate::ITEMS;

pub fn register(content: Content, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
    for (path, name, physical) in [
        ("zinc_ore", "Zinc Ore", "minecraft:gold_ore"),
        (
            "deepslate_zinc_ore",
            "Deepslate Zinc Ore",
            "minecraft:deepslate_gold_ore",
        ),
        (
            "raw_zinc_block",
            "Block of Raw Zinc",
            "minecraft:raw_gold_block",
        ),
        ("zinc_block", "Block of Zinc", "minecraft:iron_block"),
    ] {
        content.add_block(
            registry,
            &Block {
                id: &format!("create:{path}"),
                display_name: name,
                entity: "",
                item_model: "",
                states: &[State {
                    properties: "",
                    physical,
                    model: &format!("create:block/{path}"),
                    rotating_model: "",
                    x: 0,
                    y: 0,
                    orientation: [0.0, 0.0, 0.0, 1.0],
                    shape: &[],
                }],
            },
            None,
        )?;
    }
    ITEMS
        .get()
        .ok_or(RegistrationError::InvalidDefinition)?
        .add_item(
            registry,
            &Item {
                id: "create:raw_zinc",
                display_name: "Raw Zinc",
                carrier: "minecraft:raw_gold",
                model: "create:item/raw_zinc",
            },
        )
}

//! Passive content from CAItems and CABlocks in Create Crafts & Additions 1.7.2.
//! Energy, tool and machine behaviors are deliberately not registered here.

use std::sync::OnceLock;

use moddedmc_api::{
    InterfaceUnavailable, Mod, RegistrationError, Registry,
    content::{self, Block, Content, State},
    items::{self, Item, Items},
};

const ITEMS: [(&str, &str, &str, &str); 16] = [
    (
        "createaddition:electrum_ingot",
        "Electrum Ingot",
        "minecraft:gold_ingot",
        "createaddition:item/electrum_ingot",
    ),
    (
        "createaddition:electrum_nugget",
        "Electrum Nugget",
        "minecraft:gold_nugget",
        "createaddition:item/electrum_nugget",
    ),
    (
        "createaddition:electrum_sheet",
        "Electrum Sheet",
        "minecraft:iron_ingot",
        "createaddition:item/electrum_sheet",
    ),
    (
        "createaddition:zinc_sheet",
        "Zinc Sheet",
        "minecraft:iron_ingot",
        "createaddition:item/zinc_sheet",
    ),
    // All of these are plain Item::new in CAItems.
    (
        "createaddition:diamond_grit",
        "Diamond Grit",
        "minecraft:gunpowder",
        "createaddition:item/diamond_grit",
    ),
    (
        "createaddition:copper_wire",
        "Copper Wire",
        "minecraft:iron_ingot",
        "createaddition:item/copper_wire",
    ),
    (
        "createaddition:iron_wire",
        "Iron Wire",
        "minecraft:iron_ingot",
        "createaddition:item/iron_wire",
    ),
    (
        "createaddition:gold_wire",
        "Gold Wire",
        "minecraft:iron_ingot",
        "createaddition:item/gold_wire",
    ),
    (
        "createaddition:electrum_wire",
        "Electrum Wire",
        "minecraft:iron_ingot",
        "createaddition:item/electrum_wire",
    ),
    (
        "createaddition:copper_rod",
        "Copper Rod",
        "minecraft:stick",
        "createaddition:item/copper_rod",
    ),
    (
        "createaddition:iron_rod",
        "Iron Rod",
        "minecraft:stick",
        "createaddition:item/iron_rod",
    ),
    (
        "createaddition:gold_rod",
        "Gold Rod",
        "minecraft:stick",
        "createaddition:item/gold_rod",
    ),
    (
        "createaddition:electrum_rod",
        "Electrum Rod",
        "minecraft:stick",
        "createaddition:item/electrum_rod",
    ),
    (
        "createaddition:brass_rod",
        "Brass Rod",
        "minecraft:stick",
        "createaddition:item/brass_rod",
    ),
    (
        "createaddition:cake_base",
        "Cake Base",
        "minecraft:paper",
        "createaddition:item/cake_base",
    ),
    (
        "createaddition:cake_base_baked",
        "Baked Cake Base",
        "minecraft:paper",
        "createaddition:item/cake_base_baked",
    ),
];

static CONTENT: OnceLock<Content> = OnceLock::new();
static ITEM_API: OnceLock<Items> = OnceLock::new();

pub struct CreateAddition;

impl Mod for CreateAddition {
    fn start(&self) -> Result<(), InterfaceUnavailable> {
        let _ = CONTENT.set(content::require()?);
        let _ = ITEM_API.set(items::require()?);
        Ok(())
    }

    fn register(&self, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
        let items = ITEM_API.get().ok_or(RegistrationError::InvalidDefinition)?;
        for &(id, display_name, carrier, model) in &ITEMS {
            // All source entries are plain Item::new with 64-stack defaults.
            items.add_item(
                registry,
                &Item {
                    id,
                    display_name,
                    carrier,
                    model,
                },
            )?;
        }

        CONTENT
            .get()
            .ok_or(RegistrationError::InvalidDefinition)?
            .add_block(
                registry,
                &Block {
                    id: "createaddition:electrum_block",
                    display_name: "Electrum Block",
                    entity: "",
                    item_model: "createaddition:item/electrum_block",
                    states: &[State {
                        properties: "",
                        // CABlocks.ELECTRUM_BLOCK uses Blocks.GOLD_BLOCK.
                        physical: "minecraft:gold_block",
                        model: "createaddition:block/electrum_block/block",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_plain_items_have_unique_identifiers() {
        let mut names = std::collections::HashSet::new();
        for &(id, _, carrier, model) in &ITEMS {
            assert!(names.insert(id), "duplicate registry id: {id}");
            assert!(id.starts_with("createaddition:"));
            assert!(model.starts_with("createaddition:item/"));
            assert!(matches!(
                carrier,
                "minecraft:gold_ingot"
                    | "minecraft:gold_nugget"
                    | "minecraft:iron_ingot"
                    | "minecraft:gunpowder"
                    | "minecraft:stick"
                    | "minecraft:paper"
            ));
        }
        assert_eq!(names.len(), 16);
    }
}

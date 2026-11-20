//! Passive New Age items from CNAItems in 1.2.0+mc1.21.1.
//! ElectricWireItem and every kinetic/reactor block remain unregistered.

use std::sync::OnceLock;

use moddedmc_api::{
    InterfaceUnavailable, Mod, RegistrationError, Registry,
    content::{self, Block, Content, State},
    items::{self, Item, Items},
};

const ITEMS: [(&str, &str, &str, &str); 10] = [
    (
        "create_new_age:overcharged_gold",
        "Overcharged Gold",
        "minecraft:gold_ingot",
        "create_new_age:item/overcharged_gold",
    ),
    (
        "create_new_age:overcharged_iron",
        "Overcharged Iron",
        "minecraft:iron_ingot",
        "create_new_age:item/overcharged_iron",
    ),
    (
        "create_new_age:overcharged_diamond",
        "Overcharged Diamond",
        "minecraft:diamond",
        "create_new_age:item/overcharged_diamond",
    ),
    (
        "create_new_age:nuclear_fuel",
        "Nuclear Fuel",
        "minecraft:iron_ingot",
        "create_new_age:item/nuclear_fuel",
    ),
    (
        "create_new_age:thorium",
        "Thorium",
        "minecraft:iron_ingot",
        "create_new_age:item/thorium",
    ),
    (
        "create_new_age:radioactive_thorium",
        "Radioactive Thorium",
        "minecraft:iron_ingot",
        "create_new_age:item/radioactive_thorium",
    ),
    (
        "create_new_age:overcharged_iron_sheet",
        "Overcharged Iron Sheet",
        "minecraft:iron_ingot",
        "create_new_age:item/overcharged_iron_sheet",
    ),
    (
        "create_new_age:overcharged_golden_sheet",
        "Overcharged Golden Sheet",
        "minecraft:gold_ingot",
        "create_new_age:item/overcharged_golden_sheet",
    ),
    (
        "create_new_age:blank_circuit",
        "Blank Circuit",
        "minecraft:paper",
        "create_new_age:item/blank_circuit",
    ),
    (
        "create_new_age:copper_circuit",
        "Copper Circuit",
        "minecraft:paper",
        "create_new_age:item/copper_circuit",
    ),
];

const MAGNETS: [(&str, &str, f32, &str, &str); 5] = [
    (
        "create_new_age:magnetite_block",
        "Magnetite Block",
        1.0,
        "create_new_age:block/magnetite_block",
        "create_new_age:item/magnetite_block",
    ),
    (
        "create_new_age:redstone_magnet",
        "Redstone Magnet",
        2.0,
        "create_new_age:block/redstone_magnet",
        "create_new_age:item/redstone_magnet",
    ),
    (
        "create_new_age:layered_magnet",
        "Layered Magnet",
        4.0,
        "create_new_age:block/layered_magnet",
        "create_new_age:item/layered_magnet",
    ),
    (
        "create_new_age:fluxuated_magnetite",
        "Fluxuated Magnetite",
        8.0,
        "create_new_age:block/fluxuated_magnetite",
        "create_new_age:item/fluxuated_magnetite",
    ),
    (
        "create_new_age:netherite_magnet",
        "Netherite Magnet",
        24.0,
        "create_new_age:block/netherite_magnet",
        "create_new_age:item/netherite_magnet",
    ),
];

static CONTENT: OnceLock<Content> = OnceLock::new();

static ITEMS_API: OnceLock<Items> = OnceLock::new();

pub struct CreateNewAge;

impl Mod for CreateNewAge {
    fn start(&self) -> Result<(), InterfaceUnavailable> {
        let _ = ITEMS_API.set(items::require()?);
        let _ = CONTENT.set(content::require()?);
        Ok(())
    }

    fn register(&self, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
        let items = ITEMS_API
            .get()
            .ok_or(RegistrationError::InvalidDefinition)?;
        for &(id, display_name, carrier, model) in &ITEMS {
            // These are all source Item::new; 64-stack default, no use behavior.
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
        let content = CONTENT.get().ok_or(RegistrationError::InvalidDefinition)?;
        for &(id, display_name, _, model, item_model) in &MAGNETS {
            content.add_block(
                registry,
                &Block {
                    id,
                    display_name,
                    entity: "",
                    item_model,
                    states: &[State {
                        properties: "",
                        // Original ImplementedMagnetBlock uses 5.0/7.0 hardness/resistance.
                        // Iron-block template is the closest exposed host physical state.
                        physical: "minecraft:iron_block",
                        model,
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
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registered_plain_items_have_distinct_source_names() {
        let mut names = std::collections::HashSet::new();
        for &(id, _, carrier, model) in &ITEMS {
            assert!(names.insert(id));
            assert!(id.starts_with("create_new_age:"));
            assert!(model.starts_with("create_new_age:item/"));
            assert!(matches!(
                carrier,
                "minecraft:gold_ingot"
                    | "minecraft:iron_ingot"
                    | "minecraft:diamond"
                    | "minecraft:paper"
            ));
        }
        assert_eq!(names.len(), 10);
    }

    #[test]
    fn original_implemented_magnet_strengths_and_ids() {
        let observed: Vec<_> = MAGNETS
            .iter()
            .map(|(id, _, strength, _, _)| (*id, *strength))
            .collect();
        assert_eq!(
            observed,
            [
                ("create_new_age:magnetite_block", 1.0),
                ("create_new_age:redstone_magnet", 2.0),
                ("create_new_age:layered_magnet", 4.0),
                ("create_new_age:fluxuated_magnetite", 8.0),
                ("create_new_age:netherite_magnet", 24.0),
            ]
        );
        let mut around_coil = [0.0; 12];
        around_coil[..5].copy_from_slice(&[1.0, 2.0, 4.0, 8.0, 24.0]);
        assert_eq!(
            crate::generator_coil::stress_with_magnets(6.0, around_coil),
            45.0
        );
    }
}

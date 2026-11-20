//! Passive New Age items from CNAItems in 1.2.0+mc1.21.1.
//! ElectricWireItem and every kinetic/reactor block remain unregistered.

use std::sync::OnceLock;

use moddedmc_api::{
    InterfaceUnavailable, Mod, RegistrationError, Registry,
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

static ITEMS_API: OnceLock<Items> = OnceLock::new();

pub struct CreateNewAge;

impl Mod for CreateNewAge {
    fn start(&self) -> Result<(), InterfaceUnavailable> {
        let _ = ITEMS_API.set(items::require()?);
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
}

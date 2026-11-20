//! Create Enchantment Industry 2.5.4 (Fabric, Minecraft 1.20.1).
//! Passive items only. No enchantment, fluid or Create processing claims.

use std::sync::OnceLock;

use moddedmc_api::{
    InterfaceUnavailable, Mod, RegistrationError, Registry,
    items::{self, Item, Items},
};

const ITEMS: [(&str, &str, &str, &str); 2] = [
    (
        "create_enchantment_industry:blazes_enchanting_handbook",
        "Blaze's Enchanting Handbook",
        "minecraft:book",
        "create_enchantment_industry:item/blazes_enchanting_handbook",
    ),
    (
        "create_enchantment_industry:experience_cake_base",
        "Cake Base o' Enchanting",
        "minecraft:paper",
        "create_enchantment_industry:item/experience_cake_base",
    ),
];

static ITEMS_API: OnceLock<Items> = OnceLock::new();

pub struct EnchantmentIndustry;

impl Mod for EnchantmentIndustry {
    fn start(&self) -> Result<(), InterfaceUnavailable> {
        let _ = ITEMS_API.set(items::require()?);
        Ok(())
    }

    fn register(&self, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
        let items = ITEMS_API
            .get()
            .ok_or(RegistrationError::InvalidDefinition)?;
        for &(id, display_name, carrier, model) in &ITEMS {
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
    fn selected_entries_are_the_published_plain_items() {
        assert_eq!(
            ITEMS.map(|(id, _, _, _)| id),
            [
                "create_enchantment_industry:blazes_enchanting_handbook",
                "create_enchantment_industry:experience_cake_base",
            ]
        );
        assert_ne!(ITEMS[0].2, ITEMS[1].2);
    }
}

//! Partial native ports of Nature's Compass 3.3.1 and Explorer's Compass 3.3.2.
//! The original releases are CC BY-NC-SA 4.0. No upstream artwork is included.
//! Search, saved item state and the client GUI are not yet connected to the host.

pub mod biome_spiral;

use moddedmc_api::{
    InterfaceUnavailable, Mod, RegistrationError, Registry,
    items::{self, Item, Items},
};
use std::sync::OnceLock;

static ITEMS: OnceLock<Items> = OnceLock::new();

struct Compasses;

impl Mod for Compasses {
    fn start(&self) -> Result<(), InterfaceUnavailable> {
        let _ = ITEMS.set(items::require()?);
        Ok(())
    }

    fn register(&self, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
        let items = ITEMS.get().ok_or(RegistrationError::InvalidDefinition)?;
        // Use a vanilla compass carrier until the licensed 32-frame models are delivered to clients.
        for (id, name) in [
            ("naturescompass:naturescompass", "Nature's Compass"),
            ("explorerscompass:explorerscompass", "Explorer's Compass"),
        ] {
            items.add_item(
                registry,
                &Item {
                    id,
                    display_name: name,
                    carrier: "minecraft:compass",
                    model: "minecraft:item/compass",
                },
            )?;
        }
        Ok(())
    }
}

moddedmc_api::export_mod!(Compasses);

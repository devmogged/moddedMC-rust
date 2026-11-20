//! Farmer's Delight 1.3.4 storage crates (carrot, potato and beetroot).
//! Source: ModBlocks and ModItems in vectorwing/FarmersDelight branch 1.21.
//! These are plain blocks with vanilla oak-plank physics and BlockItem items.

use std::sync::OnceLock;

use moddedmc_api::{
    InterfaceUnavailable, Mod, RegistrationError, Registry,
    content::{self, Block, Content, State},
};

static CONTENT: OnceLock<Content> = OnceLock::new();

const CRATES: [(&str, &str); 3] = [
    ("carrot_crate", "Carrot Crate"),
    ("potato_crate", "Potato Crate"),
    ("beetroot_crate", "Beetroot Crate"),
];

struct FarmersDelight;

impl Mod for FarmersDelight {
    fn start(&self) -> Result<(), InterfaceUnavailable> {
        let _ = CONTENT.set(content::require()?);
        Ok(())
    }

    fn register(&self, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
        let content = CONTENT.get().ok_or(RegistrationError::InvalidDefinition)?;
        for (name, display_name) in CRATES {
            let id = format!("farmersdelight:{name}");
            let model = format!("farmersdelight:block/{name}");
            let item_model = format!("farmersdelight:item/{name}");
            content.add_block(
                registry,
                &Block {
                    id: &id,
                    display_name,
                    entity: "",
                    item_model: &item_model,
                    states: &[State {
                        properties: "",
                        physical: "minecraft:oak_planks",
                        model: &model,
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

moddedmc_api::export_mod!(FarmersDelight);

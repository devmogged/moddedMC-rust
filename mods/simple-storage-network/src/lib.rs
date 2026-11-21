//! Simple Storage Network 1.13.2 base cable.

use std::sync::OnceLock;

use moddedmc_api::{
    InterfaceUnavailable, Mod, RegistrationError, Registry,
    content::{self, Block, Content, ShapeBox, State},
};

static CONTENT: OnceLock<Content> = OnceLock::new();

struct SimpleStorageNetwork;

impl Mod for SimpleStorageNetwork {
    fn start(&self) -> Result<(), InterfaceUnavailable> {
        let _ = CONTENT.set(content::require()?);
        Ok(())
    }

    fn register(&self, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
        CONTENT
            .get()
            .ok_or(RegistrationError::InvalidDefinition)?
            .add_block(
                registry,
                &Block {
                    id: "storagenetwork:kabel",
                    display_name: "Network Cable",
                    entity: "",
                    item_model: "storagenetwork:item/kabel",
                    states: &[State {
                        properties: "",
                        physical: "minecraft:stone",
                        model: "storagenetwork:block/kabel",
                        rotating_model: "",
                        x: 0,
                        y: 0,
                        orientation: [0.0, 0.0, 0.0, 1.0],
                        // shortcut: isolated cable only, add neighbor states when topology lands.
                        shape: &[ShapeBox {
                            min: [6, 6, 6],
                            max: [10, 10, 10],
                        }],
                    }],
                },
                None,
            )
    }
}

moddedmc_api::export_mod!(SimpleStorageNetwork);

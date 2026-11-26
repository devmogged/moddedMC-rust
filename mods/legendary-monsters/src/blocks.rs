//! Full-cube Enderitium blocks, with source-derived physical properties and loot.
use moddedmc_api::{
    RegistrationError, Registry,
    block_physics::BlockPhysics,
    content::{Block, Content, State},
};

pub fn register(
    content: Content,
    physics: BlockPhysics,
    registry: &mut Registry<'_>,
) -> Result<(), RegistrationError> {
    for (id, name, template, strength) in [
        (
            "enderitium_block",
            "Enderitium Block",
            "minecraft:netherite_block",
            50.0,
        ),
        (
            "enderitium_ore",
            "Enderitium Ore",
            "minecraft:end_stone",
            2.5,
        ),
    ] {
        let full_id = format!("legendary_monsters:{id}");
        let model = format!("legendary_monsters:block/{id}");
        let item_model = format!("legendary_monsters:item/{id}");
        content.add_block(
            registry,
            &Block {
                id: &full_id,
                display_name: name,
                entity: "",
                item_model: &item_model,
                states: &[State {
                    properties: "",
                    physical: template,
                    model: &model,
                    rotating_model: "",
                    x: 0,
                    y: 0,
                    orientation: [0., 0., 0., 1.],
                    shape: &[],
                }],
            },
            None,
        )?;
        physics.set(registry, &full_id, strength, strength, true, template)?;
    }
    Ok(())
}

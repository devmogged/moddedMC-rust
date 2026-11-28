//! Create's train and framed glass trapdoors.
use moddedmc_api::{
    RegistrationError, Registry,
    block_physics::BlockPhysics,
    content::{Block, BlockMaterial, Content, State},
};

pub const TRAIN: &str = "create:train_trapdoor";
pub const GLASS: &str = "create:framed_glass_trapdoor";

pub fn register(
    content: Content,
    physics: BlockPhysics,
    registry: &mut Registry<'_>,
) -> Result<(), RegistrationError> {
    for (id, name, map_color, sound) in [
        (
            TRAIN,
            "Train Trapdoor",
            "terracotta_cyan",
            "minecraft:netherite_block",
        ),
        (GLASS, "Framed Glass Trapdoor", "none", "minecraft:glass"),
    ] {
        let (_, path) = id
            .split_once(':')
            .ok_or(RegistrationError::InvalidDefinition)?;
        let variants = std::array::from_fn::<_, 64, _>(|index| {
            let (facing, rotation) =
                [("north", 0), ("east", 90), ("south", 180), ("west", 270)][index / 16];
            let open = index & 2 != 0;
            let top = index & 4 != 0;
            let powered = index & 8 != 0;
            let wet = index & 1 != 0;
            let half = if top { "top" } else { "bottom" };
            let shape = if open { "open" } else { half };
            let y = if id == GLASS && !open {
                0
            } else if id == TRAIN && open && top {
                (rotation + 180) % 360
            } else {
                rotation
            };
            let x = if id == TRAIN && open && top { 180 } else { 0 };
            let props = format!(
                "facing={facing},half={half},open={open},powered={powered},waterlogged={wet}"
            );
            let physical = format!("minecraft:iron_trapdoor[{props}]");
            let model = format!("create:block/{path}/block_{shape}");
            (props, physical, model, x, y)
        });
        let states = variants
            .each_ref()
            .map(|(props, physical, model, x, y)| State {
                properties: props,
                physical,
                model,
                rotating_model: "",
                x: *x,
                y: *y,
                orientation: [0.0, 0.0, 0.0, 1.0],
                shape: &[],
            });
        content.add_block(
            registry,
            &Block {
                id,
                display_name: name,
                entity: "",
                item_model: &format!("create:item/{path}"),
                states: &states,
            },
            None,
        )?;
        content.set_block_material(
            registry,
            id,
            &BlockMaterial {
                template: "minecraft:gold_block",
                hardness: None,
                map_color: Some(map_color),
            },
        )?;
        physics.set(registry, id, 3.0, 6.0, true, sound)?;
    }
    Ok(())
}

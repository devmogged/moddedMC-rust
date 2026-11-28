//! Create's train and framed glass trapdoors.
use moddedmc_api::{
    RegistrationError, Registry,
    block_physics::BlockPhysics,
    content::{Block, BlockMaterial, CollisionBox, Content, State},
};

use crate::{
    DIRECTIONS,
    collider::Collider,
    contraption::PaletteEntry,
    kinetics::{face_axis, positive},
};

const THICKNESS: f64 = 3.0 / 16.0;

pub const TRAIN: &str = "create:train_trapdoor";
pub const GLASS: &str = "create:framed_glass_trapdoor";

pub fn is_trapdoor(id: &str) -> bool {
    matches!(id, TRAIN | GLASS)
}

pub fn toggle(entry: &PaletteEntry) -> Option<(PaletteEntry, Collider, bool)> {
    if !is_trapdoor(&entry.name) {
        return None;
    }
    let open = entry.properties.get("open")? == "false";
    let mut shape = CollisionBox {
        min: [0.0; 3],
        max: [1.0; 3],
    };
    if open {
        let facing = entry.properties.get("facing")?;
        let face = DIRECTIONS.iter().position(|name| *name == facing)? as u8;
        let axis = usize::from(face_axis(face));
        if positive(face) {
            shape.max[axis] = THICKNESS;
        } else {
            shape.min[axis] = 1.0 - THICKNESS;
        }
    } else if entry.properties.get("half")? == "top" {
        shape.min[1] = 1.0 - THICKNESS;
    } else {
        shape.max[1] = THICKNESS;
    }
    let mut toggled = entry.clone();
    toggled.properties.insert("open".into(), open.to_string());
    Some((toggled, Collider::from_shape(shape), open))
}

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

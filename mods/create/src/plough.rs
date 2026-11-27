//! Mechanical Plough behavior from Create's AttachedActorBlock and PloughMovementBehaviour.

use std::sync::OnceLock;

use moddedmc_api::{
    Pos, RegistrationError, Registry,
    abi::ENTITY_PASSENGER,
    content::{Content, World},
    entities::{ENTITY_ITEM, ENTITY_PUSH_IGNORED, EntityChange, SET_MOTION},
};

use crate::{
    CONTENT, ENTITIES, TAGS,
    belt_tunnel::{read, shift},
    harvester,
};

pub(crate) const ID: &str = "create:mechanical_plough";
pub(crate) const ACTIVE_AREA_OFFSET: f64 = 0.45;

pub(crate) fn register(
    content: Content,
    registry: &mut Registry<'_>,
) -> Result<(), RegistrationError> {
    harvester::register_attached(
        content,
        registry,
        ID,
        "Mechanical Plough",
        "create:block/mechanical_plough",
        "create:item/mechanical_plough",
    )
}

pub(crate) fn breakable(world: World<'_>, pos: Pos) -> Option<(f32, String)> {
    let content = CONTENT.get()?;
    let farmland = content.state("minecraft:farmland")?;
    let soil = read(world, shift(pos, 0));
    if soil == farmland
        || content
            .describe_state(soil)?
            .starts_with("minecraft:farmland[")
    {
        return None;
    }
    let mut name = content.describe_state(read(world, pos))?;
    if let Some(properties) = name.find('[') {
        name.truncate(properties);
    }
    let tags = TAGS.get()?;
    if matches!(
        name.as_str(),
        "minecraft:air"
            | "minecraft:cave_air"
            | "minecraft:void_air"
            | "minecraft:water"
            | "minecraft:lava"
            | "minecraft:bubble_column"
            | "create:fake_track"
    ) || tags.contains("block", "create:plough_blacklist", &name)
        || (!tags.contains("block", "create:tracks", &name)
            && !tags.contains("block", "create:plough_whitelist", &name)
            && !world.collision_shape(pos)?.is_empty())
    {
        return None;
    }
    let hardness = world.destroy_speed(pos)?;
    (hardness >= 0.0).then_some((hardness, name))
}

fn tool<'a>(tag: &'a OnceLock<Vec<u8>>, name: &str) -> &'a [u8] {
    tag.get_or_init(|| {
        fastnbt::to_bytes(&fastnbt::nbt!({"id": name, "count": 1i32})).unwrap_or_default()
    })
}

pub(crate) fn snow_drops(world: World<'_>, pos: Pos) -> Vec<Vec<u8>> {
    static SHOVEL: OnceLock<Vec<u8>> = OnceLock::new();
    world
        .block_drops_with_player_actor(pos, tool(&SHOVEL, "minecraft:iron_shovel"))
        .unwrap_or_default()
}

pub(crate) fn visit(world: World<'_>, pos: Pos, motion: [f64; 3], speed: f64) {
    if world.redstone_conductor(pos) == Some(false) {
        throw_entities(world, pos, motion, speed);
    }
    static HOE: OnceLock<Vec<u8>> = OnceLock::new();
    world.apply_block_transformer(pos, 0, tool(&HOE, "minecraft:diamond_hoe"));
}

fn throw_entities(world: World<'_>, pos: Pos, motion: [f64; 3], speed: f64) {
    let Some(&entities) = ENTITIES.get() else {
        return;
    };
    let min = [pos.x, pos.y, pos.z].map(f64::from);
    let mut boost = [motion[0], motion[1] + speed / 4.0, motion[2]];
    let length = boost[0].hypot(boost[1]).hypot(boost[2]);
    if length > 4.0 {
        boost = boost.map(|component| component * 4.0 / length);
    }
    for id in entities.entities_in(world, min, min.map(|coord| coord + 1.0), 0) {
        entities.with_entity(world, id, |entity| {
            if let Some(view) = entity.view()
                && view.kind & ENTITY_ITEM == 0
                && entity.flags() & (ENTITY_PUSH_IGNORED | ENTITY_PASSENGER) == 0
            {
                entity.change(&EntityChange {
                    mask: SET_MOTION,
                    motion: std::array::from_fn(|axis| view.motion[axis] + boost[axis]),
                    ..EntityChange::default()
                });
            }
        });
    }
}

//! Natural spawns from 0.9.88: the `DragonSpawnRegistry` entries the biome spawner uses with
//! their config defaults, Moop and Mossback, and each species' `canSpawnHere`. Raevyx, Stegonaut
//! and Volitans use the release's custom spawners instead (`spawners`).

use moddedmc_api::{
    Pos, RegistrationError, Registry,
    content::World,
    entities::ENTITY_ALIVE,
    mobs::{Mobs, Spawn, SpawnCategory, SpawnCheck, SpawnLocation},
};

use crate::{CONTENT, ENTITIES, MOBS, TAGS, taming};

const SPAWNS: &[Spawn<'static>] = &[
    Spawn {
        kind: "saintsdragons:cindervane",
        biomes: "#saintsdragons:has_cindervane",
        category: SpawnCategory::Creature,
        weight: 8,
        min_count: 1,
        max_count: 2,
        location: SpawnLocation::OnGround,
        carrier_rules: false,
    },
    Spawn {
        kind: "saintsdragons:atroxiia",
        biomes: "#saintsdragons:has_atroxiia",
        category: SpawnCategory::Creature,
        weight: 6,
        min_count: 1,
        max_count: 1,
        location: SpawnLocation::OnGround,
        carrier_rules: false,
    },
    // `Nulljaw.canSpawnHere`: the darkness and difficulty checks are the spider carrier's
    // monster rules.
    Spawn {
        kind: "saintsdragons:nulljaw",
        biomes: "#saintsdragons:has_nulljaw",
        category: SpawnCategory::Monster,
        weight: 4,
        min_count: 4,
        max_count: 4,
        location: SpawnLocation::Unrestricted,
        carrier_rules: true,
    },
    // Moop uses vanilla's surface water animal rule, shared with cod.
    Spawn {
        kind: "saintsdragons:moop",
        biomes: "#saintsdragons:has_moop",
        category: SpawnCategory::WaterAmbient,
        weight: 4,
        min_count: 1,
        max_count: 1,
        location: SpawnLocation::InWater,
        carrier_rules: true,
    },
    Spawn {
        kind: "saintsdragons:mossback",
        biomes: "#saintsdragons:has_mossback",
        category: SpawnCategory::Creature,
        weight: 10,
        min_count: 1,
        max_count: 2,
        location: SpawnLocation::OnGround,
        carrier_rules: false,
    },
];

/// `DragonEntity` subclasses, for the density limit.
pub const DRAGONS: &[&str] = &[
    "saintsdragons:raevyx",
    "saintsdragons:stegonaut",
    "saintsdragons:cindervane",
    "saintsdragons:varasuchus",
    "saintsdragons:ignivorus",
    "saintsdragons:volitans",
    "saintsdragons:nulljaw",
    "saintsdragons:atroxiia",
];

/// `DragonSpawnRules`.
const MIN_COMPETING_CREATURE_WEIGHT: i32 = 5;
const SAME_SPECIES_RADIUS: f64 = 96.0;
const ANY_DRAGON_RADIUS: f64 = 160.0;
const SAME_SPECIES_LIMIT: usize = 1;
const TOTAL_DRAGON_LIMIT: usize = 3;
const CINDERVANE_SAME_SPECIES_RADIUS: f64 = 64.0;
const CINDERVANE_ANY_DRAGON_RADIUS: f64 = 128.0;
const CINDERVANE_SAME_SPECIES_LIMIT: usize = 2;
const CINDERVANE_TOTAL_DRAGON_LIMIT: usize = 4;
const UP: u8 = 1;

pub fn register(registry: &mut Registry<'_>, mobs: Mobs) -> Result<(), RegistrationError> {
    for spawn in SPAWNS {
        mobs.add_spawn(registry, spawn, check)?;
    }
    Ok(())
}

fn check(world: World<'_>, spawn: &SpawnCheck<'_>) -> bool {
    match spawn.kind {
        "saintsdragons:cindervane" => {
            spawn.biome != "minecraft:snowy_slopes"
                && spawn.vanilla_weight >= MIN_COMPETING_CREATURE_WEIGHT
                && dry_ground(world, spawn.position)
                && natural_density(world, spawn)
        }
        "saintsdragons:atroxiia" => {
            dry_ground(world, spawn.position) && natural_density(world, spawn)
        }
        "saintsdragons:nulljaw" => {
            let above = up(spawn.position);
            spawn.biome == "minecraft:end_barrens"
                && dry(world, spawn.position)
                && dry(world, above)
                && open(world, spawn.position)
                && open(world, above)
        }
        "saintsdragons:mossback" => mossback_ground(world, spawn.position),
        _ => true,
    }
}

fn up(pos: Pos) -> Pos {
    Pos {
        y: pos.y + 1,
        ..pos
    }
}

fn down(pos: Pos) -> Pos {
    Pos {
        y: pos.y - 1,
        ..pos
    }
}

/// `Animal.checkAnimalSpawnRules`, rather than the wolf's species-specific substrate.
fn mossback_ground(world: World<'_>, pos: Pos) -> bool {
    let (Some(content), Some(tags)) = (CONTENT.get(), TAGS.get()) else {
        return false;
    };
    let mut state = [0];
    world.read(&[down(pos)], &mut state)
        && content.describe_state(state[0]).is_some_and(|state| {
            let id = state.split('[').next().unwrap_or(&state);
            tags.contains("block", "minecraft:animals_spawnable_on", id)
        })
        && world.raw_brightness(pos, 0).is_some_and(|light| light > 8)
}

fn dry(world: World<'_>, pos: Pos) -> bool {
    world.fluid_id(pos).as_deref() == Some("minecraft:empty")
}

fn open(world: World<'_>, pos: Pos) -> bool {
    world
        .collision_shape(pos)
        .is_some_and(|boxes| boxes.is_empty())
}

/// `DragonSpawnRules.hasDryGroundSpawnSpace`.
pub fn dry_ground(world: World<'_>, pos: Pos) -> bool {
    let below = down(pos);
    dry(world, pos)
        && dry(world, below)
        && world.face_sturdy(below, UP)
        && open(world, pos)
        && open(world, up(pos))
}

/// Java skips the density limit off the server thread, which is where chunk generation spawns
/// run.
fn natural_density(world: World<'_>, spawn: &SpawnCheck<'_>) -> bool {
    spawn.chunk_generation || density(world, spawn.kind, spawn.position)
}

/// `DragonSpawnRules.passesNearbyDragonDensityCheck` for a natural spawn: wild dragons of the
/// same species, and wild dragons of any species, in cubes around it.
pub fn density(world: World<'_>, kind: &str, pos: Pos) -> bool {
    let cindervane = kind == "saintsdragons:cindervane";
    let (same_radius, any_radius, same_limit, total_limit) = if cindervane {
        (
            CINDERVANE_SAME_SPECIES_RADIUS,
            CINDERVANE_ANY_DRAGON_RADIUS,
            CINDERVANE_SAME_SPECIES_LIMIT,
            CINDERVANE_TOTAL_DRAGON_LIMIT,
        )
    } else {
        (
            SAME_SPECIES_RADIUS,
            ANY_DRAGON_RADIUS,
            SAME_SPECIES_LIMIT,
            TOTAL_DRAGON_LIMIT,
        )
    };
    // Java counts tamed ones of the same species too, except for Cindervane.
    let same = dragons_near(world, pos, same_radius)
        .iter()
        .filter(|(k, tamed)| *k == kind && !(cindervane && *tamed))
        .count();
    if same >= same_limit {
        return false;
    }
    let wild = dragons_near(world, pos, any_radius)
        .iter()
        .filter(|(_, tamed)| !tamed)
        .count();
    wild < total_limit
}

/// (kind, tamed) of every live dragon intersecting the cube around the block's center.
// shortcut: the host queries carrier boxes; use registered species dimensions once available.
pub fn dragons_near(world: World<'_>, pos: Pos, radius: f64) -> Vec<(&'static str, bool)> {
    let (Some(entities), Some(mobs)) = (ENTITIES.get(), MOBS.get()) else {
        return Vec::new();
    };
    let center = [pos.x, pos.y, pos.z].map(|c| f64::from(c) + 0.5);
    entities
        .entities_matching(
            world,
            center.map(|c| c - radius),
            center.map(|c| c + radius),
            -1,
            0,
        )
        .into_iter()
        .filter_map(|id| {
            entities.with_entity(world, id, |e| {
                let kind = mobs.kind(e).filter(|k| DRAGONS.contains(k))?;
                if e.flags() & ENTITY_ALIVE == 0 {
                    return None;
                }
                Some((kind, taming::is_tamed(e)))
            })?
        })
        .collect()
}

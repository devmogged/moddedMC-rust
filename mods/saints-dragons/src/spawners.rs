//! The 0.9.88 custom spawners that replace the biome pools for three species, with their config
//! defaults: `RaevyxStormSpawner` (open sky during thunder), `StegonautLushCaveSpawner` (under
//! cover in lush caves) and `VolitansUnderwaterSpawner` (on the floor of a water column). Each
//! picks a player standing in its biomes, searches around them and spawns a small pack.

use std::{
    collections::{HashMap, HashSet},
    sync::{Mutex, PoisonError},
};

use moddedmc_api::{Pos, abi::heightmap, content::World, entities::ENTITY_ALIVE};

use crate::{
    ENTITIES, MOBS,
    ai::random,
    spawns::{density, dragons_near, dry_ground},
};

const SPECTATOR: i32 = 3;
// shortcut: overworld build limits; every tag these spawners use names overworld biomes.
const MIN_Y: i32 = -64;
const MAX_Y: i32 = 320;

struct Spawner {
    kind: &'static str,
    biomes: &'static str,
    interval: u32,
    /// `SPAWN_WEIGHT` turned into a percent chance per check.
    chance: u64,
    min_group: u64,
    max_group: u64,
    cluster: i32,
    nearby_radius: f64,
    /// Bounding box width and height, for `noCollision`.
    size: (f64, f64),
}

const RAEVYX: Spawner = Spawner {
    kind: "saintsdragons:raevyx",
    biomes: "#saintsdragons:has_raevyx",
    interval: 120,
    chance: 12,
    min_group: 1,
    max_group: 2,
    cluster: 160,
    nearby_radius: 128.0,
    size: (4.0, 2.75),
};
const STEGONAUT: Spawner = Spawner {
    kind: "saintsdragons:stegonaut",
    biomes: "#saintsdragons:has_stegonaut_caves",
    interval: 200,
    chance: 7,
    min_group: 1,
    max_group: 4,
    cluster: 128,
    nearby_radius: 96.0,
    size: (2.75, 2.0),
};
const VOLITANS: Spawner = Spawner {
    kind: "saintsdragons:volitans",
    biomes: "#saintsdragons:has_volitans",
    interval: 300,
    chance: 6,
    min_group: 1,
    max_group: 1,
    cluster: 128,
    nearby_radius: 96.0,
    size: (5.0, 4.0),
};
const SPAWNERS: [&Spawner; 3] = [&RAEVYX, &STEGONAUT, &VOLITANS];

/// Per world: each spawner's tick counter and the clusters it filled.
#[derive(Default)]
struct State {
    counters: [u32; 3],
    clusters: [HashSet<(i32, i32)>; 3],
}

static STATE: Mutex<Option<HashMap<usize, State>>> = Mutex::new(None);

struct Player {
    position: [f64; 3],
    alive: bool,
}

pub fn tick(world: World<'_>) {
    let due: Vec<usize> = {
        let mut state = STATE.lock().unwrap_or_else(PoisonError::into_inner);
        let state = state.get_or_insert_default().entry(world.id()).or_default();
        (0..SPAWNERS.len())
            .filter(|&index| {
                state.counters[index] += 1;
                let due = state.counters[index] >= SPAWNERS[index].interval;
                if due {
                    state.counters[index] = 0;
                }
                due
            })
            .collect()
    };
    for index in due {
        run(world, index);
    }
}

fn run(world: World<'_>, index: usize) {
    let spawner = SPAWNERS[index];
    let raevyx = index == 0;
    let players = players(world);
    if players.is_empty() || (raevyx && !world.is_thundering()) || random(100) >= spawner.chance {
        return;
    }
    let eligible: Vec<Pos> = players
        .iter()
        .filter(|player| player.alive)
        .map(|player| block(player.position))
        .filter(|&pos| {
            world.in_biome(pos, spawner.biomes) == Some(true)
                && match index {
                    0 => world.can_see_sky(pos),
                    1 => !world.can_see_sky(pos),
                    _ => true,
                }
                && !wild_near(world, spawner, pos)
        })
        .collect();
    let Some(&center) = eligible.get(random(eligible.len() as u64) as usize) else {
        return;
    };
    let cluster = (
        center.x.div_euclid(spawner.cluster),
        center.z.div_euclid(spawner.cluster),
    );
    {
        let mut state = STATE.lock().unwrap_or_else(PoisonError::into_inner);
        let clusters = &mut state
            .get_or_insert_default()
            .entry(world.id())
            .or_default()
            .clusters[index];
        if clusters.contains(&cluster) {
            if wild_near(world, spawner, center) {
                return;
            }
            clusters.remove(&cluster);
        }
    }
    let Some(origin) = find(world, index, center, &players) else {
        return;
    };
    if !density(world, spawner.kind, origin) {
        return;
    }
    if spawn_pack(world, index, origin, &players) {
        let mut state = STATE.lock().unwrap_or_else(PoisonError::into_inner);
        state
            .get_or_insert_default()
            .entry(world.id())
            .or_default()
            .clusters[index]
            .insert(cluster);
    }
}

fn players(world: World<'_>) -> Vec<Player> {
    let (Some(entities), Some(mobs)) = (ENTITIES.get(), MOBS.get()) else {
        return Vec::new();
    };
    entities
        .players(world)
        .into_iter()
        .filter_map(|id| {
            entities.with_entity(world, id, |player| {
                (mobs.game_mode(player) != Some(SPECTATOR)).then(|| Player {
                    position: player.view().map_or([0.0; 3], |view| view.position),
                    alive: player.flags() & ENTITY_ALIVE != 0,
                })
            })?
        })
        .collect()
}

fn block(position: [f64; 3]) -> Pos {
    let [x, y, z] = position.map(|c| c.floor() as i32);
    Pos { x, y, z }
}

fn offset(spread: i32) -> i32 {
    random(u64::from(spread.unsigned_abs()) * 2 + 1) as i32 - spread
}

/// A wild one of this species in the cube around `pos`.
fn wild_near(world: World<'_>, spawner: &Spawner, pos: Pos) -> bool {
    dragons_near(world, pos, spawner.nearby_radius)
        .iter()
        .any(|(kind, tamed, _)| *kind == spawner.kind && !tamed)
}

/// `Level.hasNearbyAlivePlayer(x + 0.5, y, z + 0.5, 24)`.
fn player_clear(pos: Pos, players: &[Player]) -> bool {
    let center = [
        f64::from(pos.x) + 0.5,
        f64::from(pos.y),
        f64::from(pos.z) + 0.5,
    ];
    players.iter().filter(|player| player.alive).all(|player| {
        (0..3)
            .map(|i| (player.position[i] - center[i]).powi(2))
            .sum::<f64>()
            > 24.0 * 24.0
    })
}

/// `Level.noCollision` for the species' box standing at the block's center.
// shortcut: any block shape in the box's cells counts as a collision; vanilla tests the shapes
// against the box, which only differs for partial blocks at the box's edges.
fn fits(world: World<'_>, spawner: &Spawner, pos: Pos) -> bool {
    let (width, height) = spawner.size;
    let half = width / 2.0;
    let [cx, cz] = [f64::from(pos.x) + 0.5, f64::from(pos.z) + 0.5];
    let range = |min: f64, max: f64| (min.floor() as i32)..=((max - 1e-7).floor() as i32);
    let y = f64::from(pos.y);
    range(cx - half, cx + half).all(|x| {
        range(cz - half, cz + half).all(|z| {
            range(y, y + height).all(|y| {
                world
                    .collision_shape(Pos { x, y, z })
                    .is_some_and(|shape| shape.is_empty())
            })
        })
    })
}

fn find(world: World<'_>, index: usize, center: Pos, players: &[Player]) -> Option<Pos> {
    match index {
        0 => (0..24).find_map(|_| {
            let (x, z) = (center.x + offset(96), center.z + offset(96));
            let y = world.surface_height(x, z, heightmap::MOTION_BLOCKING_NO_LEAVES)?;
            let pos = Pos { x, y, z };
            // `Raevyx.canSpawnHere` for a natural spawn: the thunder check passed already.
            (world.in_biome(pos, RAEVYX.biomes) == Some(true)
                && world.can_see_sky(pos)
                && dry_ground(world, pos)
                && density(world, RAEVYX.kind, pos)
                && fits(world, &RAEVYX, pos))
            .then_some(pos)
        }),
        1 => {
            let top = (center.y + 24).clamp(MIN_Y + 1, MAX_Y - 2);
            let bottom = (center.y - 32).clamp(MIN_Y + 1, MAX_Y - 2);
            (0..24).find_map(|_| {
                let (x, z) = (center.x + offset(48), center.z + offset(48));
                (bottom..=top).rev().map(|y| Pos { x, y, z }).find(|&pos| {
                    world.in_biome(pos, STEGONAUT.biomes) == Some(true)
                        && !world.can_see_sky(pos)
                        && player_clear(pos, players)
                        && dry_ground(world, pos)
                        && fits(world, &STEGONAUT, pos)
                        && density(world, STEGONAUT.kind, pos)
                })
            })
        }
        _ => (0..64).find_map(|_| {
            let (x, z) = (center.x + offset(96), center.z + offset(96));
            underwater(world, x, z, MIN_Y + 1, MAX_Y - 2).filter(|&pos| fits(world, &VOLITANS, pos))
        }),
    }
}

/// `VolitansUnderwaterSpawner.findUnderwaterSpawnPos`: the floor of the highest fluid column
/// at least three blocks deep between `min_y` and `max_y`.
fn underwater(world: World<'_>, x: i32, z: i32, min_y: i32, max_y: i32) -> Option<Pos> {
    let wet = |y: i32| {
        world
            .fluid_id(Pos { x, y, z })
            .is_some_and(|fluid| fluid != "minecraft:empty")
    };
    // Nothing above the world surface heightmap holds fluid, so start there.
    let surface = world.surface_height(x, z, heightmap::WORLD_SURFACE)?;
    let start = max_y.clamp(MIN_Y + 1, MAX_Y - 2).min(surface - 1);
    let end = min_y.clamp(MIN_Y + 1, MAX_Y - 2);
    let top = (end..=start).rev().find(|&y| wet(y))?;
    let bottom = (end..=top).rev().take_while(|&y| wet(y)).last()?;
    if top - bottom + 1 < 3 {
        return None;
    }
    let pos = Pos { x, y: bottom, z };
    (world.in_biome(pos, VOLITANS.biomes) == Some(true)
        && (0..3).all(|dy| {
            let above = Pos {
                y: bottom + dy,
                ..pos
            };
            wet(above.y)
                && world
                    .collision_shape(above)
                    .is_some_and(|shape| shape.is_empty())
        }))
    .then_some(pos)
}

/// A pack member's spot near the first: an `EVENT` spawn, so no thunder or density check.
fn near(world: World<'_>, index: usize, origin: Pos, players: &[Player]) -> Option<Pos> {
    match index {
        0 => (0..10).find_map(|_| {
            let (x, z) = (origin.x + offset(8), origin.z + offset(8));
            let y = world.surface_height(x, z, heightmap::MOTION_BLOCKING_NO_LEAVES)?;
            let pos = Pos { x, y, z };
            (world.can_see_sky(pos) && dry_ground(world, pos) && fits(world, &RAEVYX, pos))
                .then_some(pos)
        }),
        1 => (0..12).find_map(|_| {
            let (x, z) = (origin.x + offset(6), origin.z + offset(6));
            ((origin.y - 3)..=(origin.y + 3))
                .rev()
                .map(|y| Pos { x, y, z })
                .find(|&pos| {
                    world.in_biome(pos, STEGONAUT.biomes) == Some(true)
                        && player_clear(pos, players)
                        && dry_ground(world, pos)
                        && !world.can_see_sky(pos)
                        && fits(world, &STEGONAUT, pos)
                })
        }),
        _ => (0..12).find_map(|_| {
            let (x, z) = (origin.x + offset(8), origin.z + offset(8));
            underwater(world, x, z, origin.y - 8, origin.y + 8)
                .filter(|&pos| fits(world, &VOLITANS, pos))
        }),
    }
}

fn spawn_pack(world: World<'_>, index: usize, origin: Pos, players: &[Player]) -> bool {
    let spawner = SPAWNERS[index];
    let count = spawner.min_group + random(spawner.max_group - spawner.min_group + 1);
    if !spawn_one(world, index, origin, players) {
        return false;
    }
    for _ in 1..count {
        if let Some(pos) = near(world, index, origin, players) {
            spawn_one(world, index, pos, players);
        }
    }
    true
}

fn spawn_one(world: World<'_>, index: usize, pos: Pos, players: &[Player]) -> bool {
    let spawner = SPAWNERS[index];
    if (index == 1 && !player_clear(pos, players)) || !fits(world, spawner, pos) {
        return false;
    }
    let Some(mobs) = MOBS.get() else {
        return false;
    };
    // The wild appearance roll is `finalizeSpawn`, on its first AI tick (`roster::finalize_spawn`).
    mobs.spawn(
        world,
        spawner.kind,
        [
            f64::from(pos.x) + 0.5,
            f64::from(pos.y),
            f64::from(pos.z) + 0.5,
        ],
    )
    .is_some()
}

#[cfg(test)]
mod tests {
    use super::offset;

    #[test]
    fn offsets_stay_in_the_search_square() {
        for _ in 0..1000 {
            assert!((-96..=96).contains(&offset(96)));
        }
    }
}

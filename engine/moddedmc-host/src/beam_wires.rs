//! Transient wire power from mod entities. Recompute on the serial server tick.
use std::{
    collections::{HashMap, HashSet},
    sync::{LazyLock, Mutex, OnceLock},
};

use moddedmc_api::abi::{self, WorldRef};
use pumpkin_core::{
    block::blocks::redstone::redstone_wire::{install_wire_power_hook, update_power_strength},
    server::Server,
    world::World,
};
use pumpkin_data::Block;
use pumpkin_util::math::position::BlockPos;
use uuid::Uuid;

#[derive(Default)]
struct Level {
    sources: HashMap<Uuid, (i64, HashSet<BlockPos>)>,
    active: HashSet<BlockPos>,
}

static LEVELS: LazyLock<Mutex<HashMap<Uuid, Level>>> = LazyLock::new(Mutex::default);
static INSTALLED: OnceLock<bool> = OnceLock::new();

fn powered(world: &World, pos: &BlockPos) -> bool {
    crate::lock(&LEVELS)
        .get(&world.uuid)
        .is_some_and(|level| level.active.contains(pos))
}

pub unsafe extern "C" fn power(
    raw_world: WorldRef,
    source: abi::EntityRef,
    positions: *const abi::Pos,
    count: usize,
) -> bool {
    // SAFETY: the handles are borrowed for the active mod callback.
    let (Some(world), Some(source)) = (unsafe { crate::content::world(raw_world) }, unsafe {
        crate::content::entity(source)
    }) else {
        return false;
    };
    let source = source.get_entity();
    if count > 8192
        || positions.is_null()
        || source.is_removed()
        || source.world.load().uuid != world.uuid
    {
        return false;
    }
    if !*INSTALLED.get_or_init(|| install_wire_power_hook(powered)) {
        return false;
    }
    // SAFETY: the caller supplies the positions array for this synchronous call.
    let positions = unsafe { std::slice::from_raw_parts(positions, count) };
    let mut levels = crate::lock(&LEVELS);
    let level = levels.entry(world.uuid).or_default();
    if count == 0 {
        level.sources.remove(&source.entity_uuid);
    } else {
        level.sources.insert(
            source.entity_uuid,
            (
                world.get_world_age(),
                positions
                    .iter()
                    .copied()
                    .map(crate::content::block_pos)
                    .collect(),
            ),
        );
    }
    true
}

pub fn tick(server: &Server) {
    if !INSTALLED.get().copied().unwrap_or(false) {
        return;
    }
    for world in server.worlds.load().iter() {
        let affected = {
            let mut levels = crate::lock(&LEVELS);
            let Some(level) = levels.get_mut(&world.uuid) else {
                continue;
            };
            let now = world.get_world_age();
            level
                .sources
                .retain(|_, (last, _)| now.saturating_sub(*last) <= 1);
            let active: HashSet<_> = level
                .sources
                .values()
                .flat_map(|(_, positions)| positions.iter().copied())
                .collect();
            let affected: Vec<_> = level.active.union(&active).copied().collect();
            level.active = active;
            if level.sources.is_empty() {
                levels.remove(&world.uuid);
            }
            affected
        };
        // Never hold the map lock across neighbor updates: they query the power hook.
        for pos in affected {
            if world.is_loaded(&pos) && world.get_block(&pos) == &Block::REDSTONE_WIRE {
                update_power_strength(world, &pos);
            }
        }
    }
}

//! Loaded vanilla fluid cells and scheduled ticks for drop-in mods.

use moddedmc_api::abi::{self, Pos, Sink, Str, WorldRef};
use pumpkin_core::block::fluid::{
    flowing_trait::FlowingFluid, lava::FlowingLava, water::FlowingWater,
};
use pumpkin_data::{BlockDirection, BlockState, HorizontalFacingExt, fluid::Fluid};
use pumpkin_util::math::position::BlockPos;
use pumpkin_world::tick::TickPriority;

use crate::content::{block_pos, world};

/// `Shapes.EPSILON`.
const EPSILON: f64 = 1.0e-7;

pub static FLUID_WORLD: abi::FluidWorldV1 = abi::FluidWorldV1 {
    size: size_of::<abi::FluidWorldV1>(),
    inspect,
    schedule_tick,
    would_regenerate_source,
    water_evaporates,
};

unsafe extern "C" fn inspect(
    raw: WorldRef,
    pos: Pos,
    result: *mut abi::FluidCell,
    out: Sink,
) -> bool {
    // SAFETY: the world is borrowed for the call.
    let Some(world) = (unsafe { world(raw) }) else {
        return false;
    };
    let pos = block_pos(pos);
    let Some(id) = world.get_block_state_id_if_loaded(&pos) else {
        return false;
    };
    if result.is_null() {
        return false;
    }
    let block = id.to_block();
    let state = BlockState::from_id(id);
    let (fluid, fluid_state) = world.get_fluid_and_fluid_state(&pos);
    let mut flags = 0;
    if state.replaceable() {
        flags |= abi::FLUID_REPLACEABLE;
    }
    if block.has_property("waterlogged")
        || pumpkin_data::content::state(id.as_u16())
            .is_some_and(|entry| entry.properties.contains_key("waterlogged"))
    {
        flags |= abi::FLUID_HAS_WATERLOGGED;
    }
    if state.is_waterlogged() {
        flags |= abi::FLUID_WATERLOGGED;
    }
    if fluid_state.is_source {
        flags |= abi::FLUID_SOURCE;
    }
    if !fluid_state.is_empty {
        let still = if fluid.matches_type(&Fluid::WATER) {
            &Fluid::WATER
        } else if fluid.matches_type(&Fluid::LAVA) {
            &Fluid::LAVA
        } else {
            fluid
        };
        let id = format!("minecraft:{}", still.name);
        // SAFETY: the id is borrowed by this synchronous sink call.
        unsafe { (out.put)(out.ctx, 0, Str::from(id.as_str())) };
    }
    // SAFETY: the caller supplies a live, aligned output pointer.
    unsafe {
        result.write(abi::FluidCell {
            state: u32::from(id.as_u16()),
            flags,
        });
    };
    true
}

fn registered_fluid(name: &str) -> Option<&'static Fluid> {
    Fluid::from_registry_key(name.strip_prefix("minecraft:")?)
        .filter(|fluid| fluid.id != Fluid::EMPTY.id)
}

unsafe extern "C" fn schedule_tick(raw: WorldRef, pos: Pos, name: Str, delay: u32) -> bool {
    // SAFETY: world and fluid name are borrowed for the call.
    let (Some(world), Some(fluid)) = (
        unsafe { world(raw) },
        registered_fluid(unsafe { name.str() }),
    ) else {
        return false;
    };
    let pos = block_pos(pos);
    if world.get_block_state_id_if_loaded(&pos).is_none() {
        return false;
    }
    let fluid = fluid.to_flowing();
    let delay = if delay == abi::DEFAULT_FLUID_TICK_DELAY {
        if fluid.matches_type(&Fluid::WATER) {
            FlowingWater.get_flow_speed(world)
        } else if fluid.matches_type(&Fluid::LAVA) {
            FlowingLava.get_flow_speed(world)
        } else {
            return false;
        }
    } else {
        let Ok(delay) = u8::try_from(delay) else {
            return false;
        };
        delay
    };
    world.schedule_fluid_tick(fluid, pos, delay, TickPriority::Normal);
    true
}

/// The horizontal collision face of a neighbor against an empty liquid shape.
fn seals_face(state: &BlockState, pos: &BlockPos, direction: BlockDirection) -> bool {
    let mut slices = Vec::new();
    for shape in state.get_block_collision_shapes_at(pos) {
        let (min, max) = (shape.min, shape.max);
        let (normal, edge, a, b) = match direction {
            BlockDirection::North => (min.z, 0.0, [min.x, max.x], [min.y, max.y]),
            BlockDirection::South => (max.z, 1.0, [min.x, max.x], [min.y, max.y]),
            BlockDirection::West => (min.x, 0.0, [min.z, max.z], [min.y, max.y]),
            BlockDirection::East => (max.x, 1.0, [min.z, max.z], [min.y, max.y]),
            _ => return false,
        };
        if (normal - edge).abs() >= EPSILON {
            continue;
        }
        if a[0] <= 0.0 && a[1] >= 1.0 && b[0] <= 0.0 && b[1] >= 1.0 {
            return true;
        }
        slices.push((a, b));
    }
    if slices.is_empty() {
        return false;
    }
    let mut edges = vec![0.0, 1.0];
    for (a, _) in &slices {
        edges.extend(a.map(|value| value.clamp(0.0, 1.0)));
    }
    edges.sort_by(f64::total_cmp);
    slices.sort_by(|(_, a), (_, b)| a[0].total_cmp(&b[0]));
    edges.windows(2).all(|pair| {
        let (low, high) = (pair[0], pair[1]);
        if high <= low {
            return true;
        }
        let mut covered: f64 = 0.0;
        for (a, b) in &slices {
            if a[0] <= low && a[1] >= high {
                if b[0] > covered {
                    break;
                }
                covered = covered.max(b[1]);
            }
        }
        covered >= 1.0
    })
}

/// Source conversion branch of Java 26.3 FlowingFluid.getNewLiquid. The hypothetical block
/// is a flowing liquid with an empty collision shape.
unsafe extern "C" fn would_regenerate_source(raw: WorldRef, pos: Pos, name: Str) -> i32 {
    // SAFETY: world and fluid name are borrowed for the call.
    let (Some(world), Some(fluid)) = (
        unsafe { world(raw) },
        registered_fluid(unsafe { name.str() }),
    ) else {
        return -1;
    };
    let pos = block_pos(pos);
    if world.get_block_state_id_if_loaded(&pos).is_none() {
        return -1;
    }
    let info = world.level_info.load();
    let rules = &info.game_rules;
    let conversion = if fluid.matches_type(&Fluid::WATER) {
        rules.water_source_conversion
    } else if fluid.matches_type(&Fluid::LAVA) {
        rules.lava_source_conversion
    } else {
        fluid.can_convert_to_source
    };
    if !conversion {
        return 0;
    }
    let mut sources = 0;
    let mut unavailable = 0;
    for facing in BlockDirection::horizontal_worldgen() {
        let direction = facing.to_block_direction();
        let neighbor = pos.offset(direction.to_offset());
        let Some(id) = world.get_block_state_id_if_loaded(&neighbor) else {
            unavailable += 1;
            continue;
        };
        let (other, state) = world.get_fluid_and_fluid_state(&neighbor);
        if state.is_source
            && fluid.matches_type(other)
            && !seals_face(BlockState::from_id(id), &neighbor, direction.opposite())
        {
            sources += 1;
        }
    }
    if sources + unavailable < 2 {
        return 0;
    }
    let below = pos.down();
    let Some(id) = world.get_block_state_id_if_loaded(&below) else {
        return -1;
    };
    let (other, state) = world.get_fluid_and_fluid_state(&below);
    if !(BlockState::from_id(id).is_solid() || state.is_source && fluid.matches_type(other)) {
        return 0;
    }
    if sources >= 2 { 1 } else { -1 }
}

unsafe extern "C" fn water_evaporates(raw: WorldRef, pos: Pos) -> bool {
    // SAFETY: the world is borrowed for this call.
    unsafe { world(raw) }.is_some_and(|world| {
        world.dimension.water_evaporates
            && world
                .get_block_state_id_if_loaded(&block_pos(pos))
                .is_some()
    })
}

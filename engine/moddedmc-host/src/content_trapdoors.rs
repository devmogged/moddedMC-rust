//! Placement and redstone behavior for registered hand-openable iron trapdoors.
use std::sync::Arc;

use pumpkin_core::{
    block::{
        ExplodeArgs, GetStateForNeighborUpdateArgs, NormalUseArgs, OnNeighborUpdateArgs,
        OnPlaceArgs, PathComputationType, blocks::redstone::block_receives_redstone_power,
        registry::BlockActionResult,
    },
    entity::EntityBase,
    world::{World, game_event::GameEventContext},
};
use pumpkin_data::{
    Block, BlockDirection, BlockState, BlockStateId,
    block_properties::{BlockProperties, Half, OakTrapdoorLikeProperties},
    fluid::Fluid,
    game_event::GameEvent,
    sound::{Sound, SoundCategory},
};
use pumpkin_util::{
    math::position::BlockPos,
    random::{RandomImpl, get_seed, xoroshiro128::Xoroshiro},
};
use pumpkin_world::{tick::TickPriority, world::BlockFlags};

use crate::content::RegisteredState;

type Properties = OakTrapdoorLikeProperties;

pub const CARRIER: &Block = &Block::WAXED_OXIDIZED_COPPER_TRAPDOOR;
const STOCK_CARRIER: &Block = &Block::OXIDIZED_COPPER_TRAPDOOR;

pub fn has_trapdoor_shape(states: &[RegisteredState]) -> bool {
    states.len() == Block::IRON_TRAPDOOR.states.len()
        && states.iter().all(|state| {
            let physical = state.physical.id;
            physical.to_block() == &Block::IRON_TRAPDOOR
                && state.shape.is_empty()
                && state.properties.len() == 5
                && Properties::from_state_id(physical)
                    .to_props()
                    .iter()
                    .all(|prop| {
                        state
                            .properties
                            .get(prop.key_str())
                            .is_some_and(|value| value == prop.value_str())
                    })
        })
}

pub fn allocate(physical: BlockStateId, slot: bool) -> (BlockStateId, BlockStateId) {
    // POWERED changes redstone on the server but has no effect on the client model or shape.
    let mut props = Properties::from_state_id(physical);
    props.powered = slot;
    (props.to_state_id(CARRIER), props.to_state_id(STOCK_CARRIER))
}

fn properties(block: &Block, state: BlockStateId) -> Option<Properties> {
    let raw = block.properties(state)?.to_props();
    Some(Properties::from_props(&raw, &Block::IRON_TRAPDOOR))
}

fn state_id(block: &Block, props: Properties) -> BlockStateId {
    block.from_properties(&props.to_props()).to_state_id(block)
}

pub fn on_place(args: &OnPlaceArgs<'_>) -> BlockStateId {
    let face =
        BlockDirection::try_from(args.use_item_on.face.0).unwrap_or(args.direction.opposite());
    let mut props = Properties::default(&Block::IRON_TRAPDOOR);
    props.waterlogged = args.replacing.water_source();
    props.powered = block_receives_redstone_power(args.world, args.position);
    props.open = props.powered;
    if args.position != &args.use_item_on.position
        && let Some(facing) = face.to_horizontal_facing()
    {
        props.facing = facing;
        props.half = if args.use_item_on.cursor_pos.y > 0.5 {
            Half::Top
        } else {
            Half::Bottom
        };
    } else {
        props.facing = args.player.get_entity().get_horizontal_facing().opposite();
        props.half = if face == BlockDirection::Up {
            Half::Bottom
        } else {
            Half::Top
        };
    }
    state_id(args.block, props)
}

pub fn normal_use(args: &NormalUseArgs<'_>) -> BlockActionResult {
    let state = args.world.get_block_state_id(args.position);
    let Some(props) = properties(args.block, state) else {
        return BlockActionResult::Pass;
    };
    toggle(
        args.world,
        args.position,
        args.block,
        props,
        Some(args.player.as_ref()),
    );
    BlockActionResult::SuccessServer
}

pub fn explode(args: &ExplodeArgs<'_>) {
    // Destroying explosions call this after removing the block. Triggering explosions leave it in place.
    let state = args.world.get_block_state_id(args.position);
    if state.to_block() == args.block
        && let Some(props) = properties(args.block, state)
        && !props.powered
    {
        toggle(args.world, args.position, args.block, props, None);
    }
}

fn toggle(
    world: &Arc<World>,
    pos: &BlockPos,
    block: &Block,
    mut props: Properties,
    player: Option<&dyn EntityBase>,
) {
    props.open = !props.open;
    write(world, pos, block, props);
    play_toggle(world, pos, props.open, player);
}

pub fn neighbor_update(args: &OnNeighborUpdateArgs<'_>) {
    let state = args.world.get_block_state_id(args.position);
    let Some(mut props) = properties(args.block, state) else {
        return;
    };
    let powered = block_receives_redstone_power(args.world, args.position);
    if powered == props.powered {
        return;
    }
    if props.open != powered {
        props.open = powered;
        play_toggle(args.world, args.position, powered, None);
    }
    props.powered = powered;
    write(args.world, args.position, args.block, props);
}

pub fn neighbor_shape(args: &GetStateForNeighborUpdateArgs<'_>) -> BlockStateId {
    if args.state_id.to_state().is_waterlogged() {
        schedule_water(args.world, args.position);
    }
    args.state_id
}

pub fn is_pathfindable(state: &BlockState, computation: PathComputationType) -> bool {
    properties(state.id.to_block(), state.id).is_some_and(|props| match computation {
        PathComputationType::Land | PathComputationType::Air => props.open,
        PathComputationType::Water => props.waterlogged,
    })
}

fn write(world: &Arc<World>, pos: &BlockPos, block: &Block, props: Properties) {
    world.set_block_state(pos, state_id(block, props), BlockFlags::NOTIFY_LISTENERS);
    if props.waterlogged {
        schedule_water(world, pos);
    }
}

fn schedule_water(world: &World, pos: &BlockPos) {
    world.schedule_fluid_tick(
        &Fluid::WATER,
        *pos,
        Fluid::WATER.flow_speed as u8,
        TickPriority::Normal,
    );
}

fn play_toggle(world: &World, pos: &BlockPos, open: bool, source_entity: Option<&dyn EntityBase>) {
    let (sound, event) = if open {
        (Sound::BlockIronTrapdoorOpen, GameEvent::BlockOpen)
    } else {
        (Sound::BlockIronTrapdoorClose, GameEvent::BlockClose)
    };
    let pitch = Xoroshiro::from_seed(get_seed()).next_f32() * 0.1 + 0.9;
    let center = pos.to_centered_f64();
    world.play_sound_fine(sound, SoundCategory::Blocks, &center, 1.0, pitch);
    world.emit_game_event_with_context(event.name(), center, &GameEventContext { source_entity });
}

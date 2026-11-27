//! Native placement and wire states for permanent scaffolding.
use pumpkin_core::block::{
    GetStateForNeighborUpdateArgs, OnPlaceArgs, blocks::scaffolding::ScaffoldingBlock,
};
use pumpkin_data::{
    Block, BlockDirection, BlockStateId,
    block_properties::{BlockProperties, OakTrapdoorLikeProperties, ScaffoldingLikeProperties},
    fluid::Fluid,
};
use pumpkin_world::tick::TickPriority;

use crate::content::RegisteredState;

pub fn has_scaffolding_shape(states: &[RegisteredState]) -> bool {
    states.len() == Block::SCAFFOLDING.states.len()
        && states.iter().all(|state| {
            let physical = state.physical.id;
            physical.to_block() == &Block::SCAFFOLDING
                && state.properties.len() == 3
                && ScaffoldingLikeProperties::from_state_id(physical)
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

pub fn allocate(
    physical: BlockStateId,
    (carrier, replacement): (&Block, &Block),
) -> (BlockStateId, BlockStateId, bool) {
    let scaffold = ScaffoldingLikeProperties::from_state_id(physical);
    let mut props = OakTrapdoorLikeProperties::default(carrier);
    props.facing = if scaffold.bottom {
        pumpkin_data::block_properties::HorizontalFacing::South
    } else {
        pumpkin_data::block_properties::HorizontalFacing::North
    };
    props.open = false;
    props.waterlogged = scaffold.waterlogged;
    (
        props.to_state_id(carrier),
        props.to_state_id(replacement),
        scaffold.bottom,
    )
}

pub fn on_place(args: OnPlaceArgs<'_>) -> BlockStateId {
    let distance = ScaffoldingBlock::get_distance(args.world, args.position);
    let props = ScaffoldingLikeProperties {
        distance,
        waterlogged: args.replacing.water_source(),
        bottom: distance > 0 && args.world.get_block(&args.position.down()) != args.block,
    };
    state_id(args.block, &props)
}

pub fn neighbor_update(args: GetStateForNeighborUpdateArgs<'_>) -> BlockStateId {
    let Some(raw) = args.block.properties(args.state_id) else {
        return args.state_id;
    };
    let mut props = ScaffoldingLikeProperties::from_props(&raw.to_props(), &Block::SCAFFOLDING);
    if props.waterlogged {
        args.world.schedule_fluid_tick(
            &Fluid::WATER,
            *args.position,
            Fluid::WATER.flow_speed as u8,
            TickPriority::Normal,
        );
    }
    if args.direction != BlockDirection::Down {
        return args.state_id;
    }
    let (below, state) = args.world.get_block_and_state(&args.position.down());
    props.bottom = below != args.block && !state.is_side_solid(BlockDirection::Up);
    state_id(args.block, &props)
}

fn state_id(block: &Block, props: &ScaffoldingLikeProperties) -> BlockStateId {
    block.from_properties(&props.to_props()).to_state_id(block)
}

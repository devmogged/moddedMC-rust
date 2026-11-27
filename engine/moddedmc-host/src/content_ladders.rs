//! Open trapdoor carriers have the same collision plane as a ladder.
use pumpkin_data::{
    Block, BlockStateId,
    block_properties::{BlockProperties, LadderLikeProperties, OakTrapdoorLikeProperties},
};
use std::io;

use crate::content::RegisteredState;

pub const CARRIERS: [(&Block, &Block); 3] = [
    (&Block::WAXED_COPPER_TRAPDOOR, &Block::COPPER_TRAPDOOR),
    (
        &Block::WAXED_EXPOSED_COPPER_TRAPDOOR,
        &Block::EXPOSED_COPPER_TRAPDOOR,
    ),
    (
        &Block::WAXED_WEATHERED_COPPER_TRAPDOOR,
        &Block::WEATHERED_COPPER_TRAPDOOR,
    ),
];

pub fn has_ladder_shape(states: &[RegisteredState]) -> bool {
    states.len() == Block::LADDER.states.len()
        && states.iter().all(|state| {
            state.physical.id.to_block() == &Block::LADDER
                && state.shape.is_empty()
                && state.properties.len() == 2
                && LadderLikeProperties::from_state_id(state.physical.id)
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
) -> (BlockStateId, BlockStateId) {
    let ladder = LadderLikeProperties::from_state_id(physical);
    let mut props = OakTrapdoorLikeProperties::default(carrier);
    props.facing = ladder.facing;
    props.open = true;
    props.waterlogged = ladder.waterlogged;
    (props.to_state_id(carrier), props.to_state_id(replacement))
}

pub fn reserve_carriers(
    registry: &mut pumpkin_data::content::Registry,
    definitions: &[pumpkin_data::content::BlockDefinition],
) -> io::Result<()> {
    for (carrier, replacement) in CARRIERS {
        if definitions.iter().any(|block| {
            block
                .states
                .iter()
                .any(|state| state.carrier.to_block() == carrier)
        }) {
            registry.reserve_climbable_carrier(carrier, replacement)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_ladder_carrier_keeps_the_ladder_plane_and_water() {
        for pair in CARRIERS {
            for state in Block::LADDER.states {
                let (carrier, _) = allocate(state.id, pair);
                let state_bounds = |id: BlockStateId| {
                    id.to_state()
                        .get_block_collision_shapes()
                        .map(|bounds| (bounds.min, bounds.max))
                        .collect::<Vec<_>>()
                };
                assert_eq!(state_bounds(state.id), state_bounds(carrier));
                assert_eq!(state.id.is_waterlogged(), carrier.is_waterlogged());
            }
        }
    }
}

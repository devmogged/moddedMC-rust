//! Use the vanilla `SlabBlock` behavior and stock slab carriers for content slabs.

use pumpkin_core::block::{
    BlockBehaviour, BlockIsReplacing, CanUpdateAtArgs, GetStateForNeighborUpdateArgs, OnPlaceArgs,
    blocks::slabs::SlabBlock,
};
use pumpkin_data::{
    Block, BlockStateId, Property,
    block_properties::{BlockProperties, SlabProperties, SlabType},
};

use crate::content::RegisteredState;

pub const CARRIERS: [(&Block, &Block); 4] = [
    (&Block::WAXED_CUT_COPPER_SLAB, &Block::CUT_COPPER_SLAB),
    (
        &Block::WAXED_EXPOSED_CUT_COPPER_SLAB,
        &Block::EXPOSED_CUT_COPPER_SLAB,
    ),
    (
        &Block::WAXED_WEATHERED_CUT_COPPER_SLAB,
        &Block::WEATHERED_CUT_COPPER_SLAB,
    ),
    (
        &Block::WAXED_OXIDIZED_CUT_COPPER_SLAB,
        &Block::OXIDIZED_CUT_COPPER_SLAB,
    ),
];

pub const DISPLAY_CARRIER: (&Block, &Block) = (&Block::PETRIFIED_OAK_SLAB, &Block::OAK_SLAB);

pub fn carrier_model(block: &Block, variant: &str) -> Option<String> {
    let (_, original) = CARRIERS
        .into_iter()
        .chain([DISPLAY_CARRIER])
        .find(|(carrier, _)| carrier.id == block.id)?;
    let model = if variant.split(',').any(|prop| prop == "type=double") {
        if original == &Block::OAK_SLAB {
            "oak_planks".to_owned()
        } else {
            original.name.strip_suffix("_slab")?.to_owned()
        }
    } else if variant.split(',').any(|prop| prop == "type=top") {
        format!("{}_top", original.name)
    } else {
        original.name.to_owned()
    };
    Some(format!("minecraft:block/{model}"))
}

/// A complete slab state set using one vanilla slab as its physical template.
pub fn template(states: &[RegisteredState]) -> Option<&'static Block> {
    let base = states.first()?.physical.id.to_block();
    if !SlabProperties::handles_block_id(base.id) || states.len() != 6 {
        return None;
    }
    states
        .iter()
        .all(|state| {
            if state.physical.id.to_block() != base || state.properties.len() != 2 {
                return false;
            }
            let props = SlabProperties::from_state_id(state.physical.id);
            state
                .properties
                .get("type")
                .is_some_and(|ty| Property::SlabType(props.r#type).value_str() == ty)
                && state
                    .properties
                    .get("waterlogged")
                    .is_some_and(|wet| Property::Waterlogged(props.waterlogged).value_str() == wet)
        })
        .then_some(base)
}

fn vanilla_state(block: &Block, state: BlockStateId, template: &Block) -> Option<BlockStateId> {
    let props = block.properties(state)?.to_props();
    Some(template.from_properties(&props).to_state_id(template))
}

pub fn can_update_at(args: &CanUpdateAtArgs<'_>, template: &Block) -> bool {
    let Some(state_id) = vanilla_state(args.block, args.state_id, template) else {
        return false;
    };
    // An adjacent same-item slab can always merge if it is still a single slab.
    if args.position != &args.use_item_on.position {
        return SlabProperties::from_state_id(state_id).r#type != SlabType::Double;
    }
    SlabBlock.can_update_at(CanUpdateAtArgs {
        block: template,
        state_id,
        ..*args
    })
}

pub fn on_place(args: OnPlaceArgs<'_>, template: &Block) -> BlockStateId {
    let block = args.block;
    let replacing = match args.replacing {
        BlockIsReplacing::Itself(state) => {
            let Some(state) = vanilla_state(block, state, template) else {
                return block.default_state.id;
            };
            BlockIsReplacing::Itself(state)
        }
        other => other,
    };
    let vanilla = SlabBlock.on_place(OnPlaceArgs {
        block: template,
        replacing,
        ..args
    });
    let props = SlabProperties::from_state_id(vanilla);
    block.from_properties(&props.to_props()).to_state_id(block)
}

pub fn neighbor_update(args: &GetStateForNeighborUpdateArgs<'_>, template: &Block) -> BlockStateId {
    let state = args.state_id;
    if state.is_waterlogged()
        && let Some(state_id) = vanilla_state(args.block, state, template)
    {
        SlabBlock.get_state_for_neighbor_update(GetStateForNeighborUpdateArgs {
            block: template,
            state_id,
            ..*args
        });
    }
    state
}

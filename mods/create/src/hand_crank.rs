//! Hand Crank placement, collision and player-driven rotation.
use moddedmc_api::{
    Pos, RegistrationError, Registry,
    content::{Block, Content, State, World},
    interaction::{BlockUseResult, UseBlock},
};

use crate::{
    DIRECTIONS, HALF, StateIds, WorldLevel,
    kinetics::{FACES, Kind},
    offset,
    piston::axial_box,
};

pub(crate) const ID: &str = "create:hand_crank";
pub(crate) const SPEED: i32 = 32;
pub(crate) const USE_TICKS: i32 = 10;
const EXHAUSTION_PER_RPM: f32 = 0.01;

fn variants() -> impl Iterator<Item = (u8, bool, String)> {
    DIRECTIONS
        .into_iter()
        .enumerate()
        .flat_map(|(face, facing)| {
            [false, true].map(|wet| {
                (
                    face as u8,
                    wet,
                    format!("facing={facing},waterlogged={wet}"),
                )
            })
        })
}

pub(crate) fn register(
    content: Content,
    registry: &mut Registry<'_>,
) -> Result<(), RegistrationError> {
    let rows: Vec<_> = variants()
        .map(|(face, wet, properties)| {
            (
                face,
                wet,
                properties,
                [axial_box(face, 0, 6, 5, 11), axial_box(face, 3, 8, 1, 15)],
            )
        })
        .collect();
    let states: Vec<_> = rows
        .iter()
        .map(|(face, wet, properties, shape)| {
            let [x, y] =
                [[180, 0], [0, 0], [90, 0], [90, 180], [90, 270], [90, 90]][usize::from(*face)];
            let orientation = [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
                [-HALF, 0.0, 0.0, HALF],
                [HALF, 0.0, 0.0, HALF],
                [0.0, 0.0, HALF, HALF],
                [0.0, 0.0, -HALF, HALF],
            ][usize::from(*face)];
            State {
                properties,
                physical: if *wet {
                    "minecraft:oak_slab[type=bottom,waterlogged=true]"
                } else {
                    "minecraft:oak_slab[type=bottom,waterlogged=false]"
                },
                model: "create:block/hand_crank/block",
                rotating_model: "create:block/hand_crank/handle",
                x,
                y,
                orientation,
                shape,
            }
        })
        .collect();
    content.add_block(
        registry,
        &Block {
            id: ID,
            display_name: "Hand Crank",
            entity: ID,
            item_model: "create:item/hand_crank",
            states: &states,
        },
        Some(crate::placed_state),
    )
}

pub(crate) fn add_states(content: Content, states: &mut StateIds) {
    for (face, _, properties) in variants() {
        states.add(content, ID, &properties, Some(Kind::HandCrank(face)));
    }
}

pub(crate) fn supported(world: World<'_>, pos: Pos, facing: u8) -> Option<bool> {
    let support = offset(pos, FACES[usize::from(facing ^ 1)]);
    world
        .collision_shape(support)
        .map(|boxes| !boxes.is_empty())
}

pub(crate) fn turn(context: &UseBlock<'_>) -> BlockUseResult {
    if context.with_item && context.stack.count() == 0 {
        return BlockUseResult::TryEmptyHand;
    }
    if context.player.is_none() {
        return BlockUseResult::Pass;
    }
    let world = context.world;
    let pos = context.position;
    let ticks = if context.sneaking {
        -USE_TICKS
    } else {
        USE_TICKS
    };
    let Some(states) = crate::states() else {
        return BlockUseResult::Fail;
    };
    let turned = crate::with_world(world, |kinetics| {
        let Some(be) = kinetics.get_mut(pos) else {
            return false;
        };
        if be.generated.signum() == ticks.signum() {
            be.generated = ticks;
            be.dirty = true;
        } else {
            kinetics.set_generated(&mut WorldLevel { world, states }, pos, ticks);
        }
        true
    });
    if turned {
        context.add_exhaustion(SPEED as f32 * EXHAUSTION_PER_RPM);
        BlockUseResult::Success
    } else {
        BlockUseResult::Fail
    }
}

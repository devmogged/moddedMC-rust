//! Windmill sails and frames.
use moddedmc_api::{
    Pos, RegistrationError, Registry,
    content::{self, Block, BlockUpdate, Content, ShapeBox, State, World},
    interaction::{BlockUseResult, UseBlock},
};

use crate::{
    DIRECTIONS, StateIds,
    contraption::PaletteEntry,
    kinetics::{FACES, face_axis},
    offset,
    piston::axial_box,
    processing_recipes::item_id,
};

pub(crate) const FRAME: &str = "create:sail_frame";
pub(crate) const CANVAS: &str = "create:white_sail";

pub(crate) fn is_sail(block: &str) -> bool {
    matches!(block, FRAME | CANVAS)
}

pub(crate) fn register(
    content: Content,
    registry: &mut Registry<'_>,
) -> Result<(), RegistrationError> {
    let properties = DIRECTIONS.map(|facing| format!("facing={facing}"));
    for (id, display_name, model, item_model) in [
        (
            CANVAS,
            "Windmill Sail",
            "create:block/white_sail",
            "create:item/white_sail",
        ),
        (
            FRAME,
            "Windmill Sail Frame",
            "create:block/sail_frame",
            "create:item/sail_frame",
        ),
    ] {
        let shapes = std::array::from_fn::<_, 6, _>(|face| collision(face as u8, id == FRAME));
        let states = std::array::from_fn::<_, 6, _>(|face| {
            let [x, y] = [[180, 0], [0, 0], [90, 0], [90, 180], [90, 270], [90, 90]][face];
            State {
                properties: &properties[face],
                physical: "minecraft:oak_planks",
                model,
                rotating_model: "",
                x,
                y,
                orientation: [0.0, 0.0, 0.0, 1.0],
                shape: &shapes[face],
            }
        });
        content.add_block(
            registry,
            &Block {
                id,
                display_name,
                entity: id,
                item_model,
                states: &states,
            },
            Some(crate::placed_state),
        )?;
    }
    Ok(())
}

fn collision(face: u8, frame: bool) -> Vec<ShapeBox> {
    let slab = axial_box(face, 5, if frame { 9 } else { 10 }, 0, 16);
    if !frame {
        return vec![slab];
    }
    let u = (usize::from(face_axis(face)) + 1) % 3;
    let v = (u + 1) % 3;
    let mut sides = [slab; 4];
    sides[0].max[u] = 2;
    sides[1].min[u] = 14;
    sides[2].min[u] = 2;
    sides[2].max[u] = 14;
    sides[2].max[v] = 2;
    sides[3].min[u] = 2;
    sides[3].max[u] = 14;
    sides[3].min[v] = 14;
    sides.to_vec()
}

pub(crate) fn add_states(content: Content, states: &mut StateIds) {
    for block in [CANVAS, FRAME] {
        for facing in DIRECTIONS {
            states.add(content, block, &format!("facing={facing}"), None);
        }
    }
}

pub(crate) fn plane(entry: &PaletteEntry) -> Option<u8> {
    is_sail(&entry.name).then_some(())?;
    let name = entry.properties.get("facing")?;
    DIRECTIONS
        .iter()
        .position(|facing| facing == name)
        .map(|face| face_axis(face as u8))
}

pub(crate) fn prefer_neighbor(states: &StateIds, support: u32, face: u8) -> Option<u8> {
    let neighbor = states.by_id.get(&support)?;
    is_sail(neighbor.block).then_some(())?;
    let (_, facing) = neighbor.selector.strip_suffix(']')?.rsplit_once('=')?;
    let facing = DIRECTIONS.iter().position(|name| *name == facing)? as u8;
    (face_axis(facing) != face_axis(face)).then_some(facing)
}

fn state_at(world: World<'_>, pos: Pos) -> Option<u32> {
    let mut state = [content::UNLOADED_STATE];
    world.read(&[pos], &mut state).then_some(state[0])
}

fn edge_order(context: &UseBlock<'_>, axis: u8) -> Vec<(u8, [i32; 3])> {
    let pos = context.position;
    let center = [pos.x, pos.y, pos.z].map(|v| v as f32 + 0.5);
    let inward = std::array::from_fn::<_, 3, _>(|i| center[i] - context.hit[i]);
    let mut sides: Vec<_> = FACES
        .into_iter()
        .enumerate()
        .filter(|(face, _)| face_axis(*face as u8) != axis)
        .map(|(face, step)| (face as u8, step))
        .collect();
    let distance = |step: [i32; 3]| -> f32 { (0..3).map(|i| inward[i] * step[i] as f32).sum() };
    sides.sort_by(|(_, a), (_, b)| distance(*a).total_cmp(&distance(*b)));
    sides
}

pub(crate) fn use_item_on(
    context: &mut UseBlock<'_>,
    states: &StateIds,
    selector: &str,
) -> BlockUseResult {
    let Some(bytes) = context.stack.read() else {
        return BlockUseResult::TryEmptyHand;
    };
    let Ok(stack) = fastnbt::from_bytes::<fastnbt::Value>(&bytes) else {
        return BlockUseResult::TryEmptyHand;
    };
    let entry = PaletteEntry::parse(selector);
    let Some(axis) = plane(&entry) else {
        return BlockUseResult::Pass;
    };
    match item_id(&stack) {
        Some(FRAME | CANVAS)
            if !context.sneaking
                && context.main_hand
                && context.player.is_some_and(|player| player.may_build()) =>
        {
            for (face, step) in edge_order(context, axis) {
                if state_at(context.world, offset(context.position, step))
                    .is_some_and(|state| states.replaceable.contains(&state))
                    && context.place_held_block(context.position, face)
                {
                    if !context.creative {
                        context.stack.shrink(1);
                    }
                    return BlockUseResult::Success;
                }
            }
            BlockUseResult::Fail
        }
        Some("minecraft:shears") if entry.name == CANVAS => {
            let Some(facing) = entry.properties.get("facing") else {
                return BlockUseResult::Fail;
            };
            let Some(state) = states.lookup(&format!("{FRAME}[facing={facing}]")) else {
                return BlockUseResult::Fail;
            };
            let changed = context.world.write(&[BlockUpdate {
                position: context.position,
                state,
                flags: content::UPDATE_ALL,
            }]);
            if changed == 1 {
                BlockUseResult::Success
            } else {
                BlockUseResult::Fail
            }
        }
        _ => BlockUseResult::TryEmptyHand,
    }
}

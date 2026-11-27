//! The Large Water Wheel's axle and eight structural blocks.
use std::collections::HashSet;

use moddedmc_api::{
    Pos, RegistrationError, Registry,
    content::{self, Block, BlockUpdate, Content, ContentEvent, State, World},
    interaction::PlayerWillDestroy,
};

use crate::{
    AXES, DIRECTIONS, StateIds,
    kinetics::{FACES, Kind, face_axis},
    offset, relay_orientation,
};

pub(crate) const ID: &str = "create:large_water_wheel";
pub(crate) const STRUCTURE: &str = "create:water_wheel_structure";
pub(crate) const SPEED: i32 = 4;

const NEGATIVE: [usize; 3] = [4, 0, 2];
const ORIENTATION: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

fn axles() -> impl Iterator<Item = (u8, String)> {
    [1u8, 0, 2].into_iter().flat_map(|axis| {
        [false, true].map(|extension| {
            (
                axis,
                format!("axis={},extension={extension}", AXES[usize::from(axis)]),
            )
        })
    })
}

pub(crate) fn register(
    content: Content,
    registry: &mut Registry<'_>,
) -> Result<(), RegistrationError> {
    let properties: Vec<_> = axles().collect();
    let axles: Vec<_> = properties
        .iter()
        .map(|(axis, properties)| State {
            properties,
            physical: "minecraft:spruce_planks",
            model: "",
            rotating_model: "create:block/large_water_wheel/stock_rotor",
            x: 0,
            y: 0,
            orientation: relay_orientation(*axis),
            shape: &[],
        })
        .collect();
    content.add_block(
        registry,
        &Block {
            id: ID,
            display_name: "Large Water Wheel",
            entity: ID,
            item_model: "create:item/stock_large_water_wheel",
            states: &axles,
        },
        Some(crate::placed_state),
    )?;

    let properties = DIRECTIONS.map(|facing| format!("facing={facing}"));
    let structure = properties.each_ref().map(|properties| State {
        properties,
        physical: "minecraft:spruce_planks",
        model: "moddedmc:block/empty",
        rotating_model: "",
        x: 0,
        y: 0,
        orientation: ORIENTATION,
        shape: &[],
    });
    content.add_block(
        registry,
        &Block {
            id: STRUCTURE,
            display_name: "Large Water Wheel",
            entity: STRUCTURE,
            item_model: "",
            states: &structure,
        },
        None,
    )
}

pub(crate) fn add_states(content: Content, states: &mut StateIds) {
    for (axis, properties) in axles() {
        states.add(content, ID, &properties, Some(Kind::LargeWaterWheel(axis)));
    }
    for (facing, name) in DIRECTIONS.into_iter().enumerate() {
        let properties = format!("facing={name}");
        states.add(content, STRUCTURE, &properties, None);
        states.wheel_parts[facing] = states.lookup(&format!("{STRUCTURE}[{properties}]"));
    }
}

fn axle_state(states: &StateIds, neighbor: u32, axis: u8) -> Option<u32> {
    let extension = matches!(states.kind(neighbor), Some(Kind::LargeWaterWheel(_)));
    states.lookup(&format!(
        "{ID}[axis={},extension={extension}]",
        AXES[usize::from(axis)]
    ))
}

fn facing(states: &StateIds, state: u32) -> Option<usize> {
    states
        .wheel_parts
        .iter()
        .position(|part| *part == Some(state))
}

fn parts(center: Pos, axis: u8) -> impl Iterator<Item = (Pos, usize)> {
    const CLOCKWISE: [[usize; 6]; 3] = [[3, 2, 0, 1, 4, 5], [0, 1, 5, 4, 2, 3], [4, 5, 2, 3, 1, 0]];
    FACES
        .into_iter()
        .enumerate()
        .filter(move |(side, _)| face_axis(*side as u8) != axis)
        .flat_map(move |(side, step)| {
            let adjacent = offset(center, step);
            let corner = CLOCKWISE[usize::from(axis)][side];
            [
                (adjacent, side ^ 1),
                (offset(adjacent, FACES[corner]), corner ^ 1),
            ]
        })
}

fn read(world: World<'_>, pos: Pos) -> Option<u32> {
    let mut state = [content::UNLOADED_STATE];
    (world.read(&[pos], &mut state) && state[0] != content::UNLOADED_STATE).then_some(state[0])
}

fn replaceable(states: &StateIds, state: u32) -> bool {
    states.replaceable.contains(&state) || states.wheel_replaceable.contains(&state)
}

pub(crate) fn placed(
    world: World<'_>,
    states: &StateIds,
    pos: Pos,
    neighbor: u32,
    axis: u8,
) -> Option<u32> {
    if parts(pos, axis)
        .all(|(part, _)| read(world, part).is_some_and(|state| replaceable(states, state)))
    {
        axle_state(states, neighbor, axis)
    } else {
        None
    }
}

pub(crate) fn negative(axis: u8) -> usize {
    NEGATIVE[usize::from(axis)]
}

fn build(world: World<'_>, states: &StateIds, pos: Pos, axis: u8) -> Option<bool> {
    let mut updates = Vec::with_capacity(8);
    for (part, facing) in parts(pos, axis) {
        let state = read(world, part)?;
        let expected = states.wheel_parts[facing]?;
        if state == expected {
            continue;
        }
        if !replaceable(states, state) {
            return Some(false);
        }
        updates.push(BlockUpdate {
            position: part,
            state: expected,
            flags: content::UPDATE_ALL,
        });
    }
    Some(updates.is_empty() || world.write(&updates) == updates.len())
}

// None means the axle's chunk is unavailable; Some(None) is an orphaned part.
fn master(world: World<'_>, states: &StateIds, pos: Pos, state: u32) -> Option<Option<Pos>> {
    let Some(first) = facing(states, state) else {
        return Some(None);
    };
    let mut target = offset(pos, FACES[first]);
    let mut state = read(world, target)?;
    if let Some(next) = facing(states, state) {
        target = offset(target, FACES[next]);
        state = read(world, target)?;
    }
    Some(match states.kind(state) {
        Some(Kind::LargeWaterWheel(axis))
            if parts(target, axis).any(|part| part == (pos, first)) =>
        {
            Some(target)
        }
        _ => None,
    })
}

pub(crate) fn player_will_destroy(context: &PlayerWillDestroy<'_>) {
    if context.creative
        && let Some(states) = crate::states()
        && let Some(Some(pos)) = master(context.world, states, context.position, context.state)
    {
        context.world.destroy(pos, false);
    }
}

pub(crate) fn event(
    world: World<'_>,
    states: &StateIds,
    pending: &mut HashSet<Pos>,
    event: &ContentEvent,
) {
    let pos = event.position;
    if event.reason == content::STATE_CHANGED {
        let before = states.kind(event.previous);
        let after = states.kind(event.state);
        if before != after {
            if let Some(Kind::LargeWaterWheel(axis)) = before
                && let Some(air) = states.lookup("minecraft:air")
            {
                let updates: Vec<_> = parts(pos, axis)
                    .filter(|(part, facing)| {
                        read(world, *part)
                            .is_some_and(|state| states.wheel_parts[*facing] == Some(state))
                    })
                    .map(|(position, _)| BlockUpdate {
                        position,
                        state: air,
                        flags: content::UPDATE_ALL,
                    })
                    .collect();
                if !updates.is_empty() {
                    world.write(&updates);
                }
            }
            if let Some(Kind::LargeWaterWheel(axis)) = after
                && read(world, pos).and_then(|state| states.kind(state)) == after
                && build(world, states, pos, axis) == Some(false)
            {
                world.destroy(pos, false);
            }
        }
        if facing(states, event.previous).is_some()
            && event.previous != event.state
            && read(world, pos) != Some(event.previous)
            && let Some(Some(center)) = master(world, states, pos, event.previous)
        {
            world.destroy(center, true);
        }
    }
    if event.reason == content::UNLOADED {
        pending.remove(&pos);
    }
    if matches!(event.reason, content::LOADED | content::NEIGHBOR_CHANGED)
        && (matches!(states.kind(event.state), Some(Kind::LargeWaterWheel(_)))
            || facing(states, event.state).is_some())
        && let Some(state) = read(world, pos)
    {
        if let Some(Kind::LargeWaterWheel(axis)) = states.kind(state)
            && let Some(neighbor) = read(world, offset(pos, FACES[negative(axis)]))
            && let Some(next) = axle_state(states, neighbor, axis)
            && next != state
        {
            world.write(&[BlockUpdate {
                position: pos,
                state: next,
                flags: content::UPDATE_CLIENTS,
            }]);
        } else if facing(states, state).is_some() && check_part(world, states, pos, state) {
            pending.insert(pos);
        }
    }
}

fn check_part(world: World<'_>, states: &StateIds, pos: Pos, state: u32) -> bool {
    match master(world, states, pos, state) {
        None => true,
        Some(None) => {
            world.destroy(pos, false);
            false
        }
        Some(Some(_)) => false,
    }
}

pub(crate) fn refresh_parts(world: World<'_>, states: &StateIds, pending: &mut HashSet<Pos>) {
    pending.retain(|&pos| {
        read(world, pos).is_some_and(|state| {
            facing(states, state).is_some() && check_part(world, states, pos, state)
        })
    });
}

pub(crate) fn refresh_axle(world: World<'_>, states: &StateIds, pos: Pos, axis: u8) -> bool {
    if build(world, states, pos, axis) == Some(false) {
        world.destroy(pos, false);
        return false;
    }
    true
}

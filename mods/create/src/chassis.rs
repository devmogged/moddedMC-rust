//! Create 6.0.10's AbstractChassisBlock and ChassisBlockEntity.

use std::collections::{HashSet, VecDeque};

use moddedmc_api::{
    Pos, RegistrationError, Registry,
    content::{self, Block, BlockUpdate, Content, State, UNLOADED_STATE, World},
    interaction::{BlockUseResult, UseBlock},
};
use serde::Deserialize;

use crate::{
    AXES, CONTENT, STONE, StateIds, TAGS,
    contraption::{BlockMovement, BlockMovementChecks, PaletteEntry, StructureTransform},
    kinetics::{FACES, face_axis, offset, positive},
    processing_recipes::item_id,
};

const LINEAR: &str = "create:linear_chassis";
const SECONDARY: &str = "create:secondary_linear_chassis";
const RADIAL: &str = "create:radial_chassis";
const BLOCKS: [&str; 3] = [LINEAR, SECONDARY, RADIAL];
const MAX_CHASSIS_RANGE: i32 = 16;
const DEFAULT_RANGE: i32 = MAX_CHASSIS_RANGE / 2;

pub fn is_chassis(id: &str) -> bool {
    BLOCKS.contains(&id)
}

fn glueable_side(id: &str, axis: u8, face: u8) -> Option<&'static str> {
    if id != RADIAL {
        return (face_axis(face) == axis).then_some(if positive(face) {
            "sticky_top"
        } else {
            "sticky_bottom"
        });
    }
    match (axis, face) {
        (0, 1) | (1, 2) | (2, 1) => Some("sticky_north"),
        (0, 0) | (1, 3) | (2, 0) => Some("sticky_south"),
        (0, 2) | (1 | 2, 4) => Some("sticky_west"),
        (0, 3) | (1 | 2, 5) => Some("sticky_east"),
        _ => None,
    }
}

fn properties(id: &str, axis: u8, glue: u8) -> String {
    let mut properties = format!("axis={}", AXES[usize::from(axis)]);
    for (face, key) in
        (0..6u8).filter_map(|face| glueable_side(id, axis, face).map(|key| (face, key)))
    {
        properties.push_str(&format!(",{key}={}", glue & (1 << face) != 0));
    }
    properties
}

fn variants(id: &str) -> impl Iterator<Item = (u8, u8)> + '_ {
    [1, 0, 2].into_iter().flat_map(move |axis| {
        let permitted = (0..6u8)
            .filter(|&face| glueable_side(id, axis, face).is_some())
            .fold(0u8, |mask, face| mask | (1 << face));
        (0..64)
            .filter(move |mask| mask & !permitted == 0)
            .map(move |mask| (axis, mask))
    })
}

pub fn register(content: Content, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
    for (id, name, item_model, block_model) in [
        (
            LINEAR,
            "Linear Chassis",
            "create:item/linear_chassis",
            "create:block/linear_chassis",
        ),
        (
            SECONDARY,
            "Secondary Linear Chassis",
            "create:item/secondary_linear_chassis",
            "create:block/secondary_linear_chassis",
        ),
        (
            RADIAL,
            "Radial Chassis",
            "create:item/radial_chassis",
            "create:block/radial_chassis/base",
        ),
    ] {
        let definitions: Vec<_> = variants(id)
            .map(|(axis, glue)| {
                let shaft = axis_faces(axis);
                let suffix = if id == RADIAL {
                    ""
                } else {
                    match (glue & (1 << shaft[0]) != 0, glue & (1 << shaft[1]) != 0) {
                        (true, true) => "_top_bottom",
                        (true, false) => "_top",
                        (false, true) => "_bottom",
                        (false, false) => "",
                    }
                };
                (
                    axis,
                    properties(id, axis, glue),
                    format!("{block_model}{suffix}"),
                )
            })
            .collect();
        let states: Vec<_> = definitions
            .iter()
            .map(|(axis, properties, model)| State {
                properties,
                physical: STONE,
                model,
                rotating_model: "",
                x: if *axis == 1 { 0 } else { 90 },
                y: if *axis == 2 && id == RADIAL {
                    0
                } else {
                    [90, 0, 180][usize::from(*axis)]
                },
                orientation: crate::relay_orientation(*axis),
                shape: &[],
            })
            .collect();
        content.add_block(
            registry,
            &Block {
                id,
                display_name: name,
                entity: "create:chassis",
                item_model,
                states: &states,
            },
            Some(crate::placed_state),
        )?;
    }
    Ok(())
}

pub fn add_states(content: Content, ids: &mut StateIds) {
    for id in BLOCKS {
        for (axis, glue) in variants(id) {
            ids.add(content, id, &properties(id, axis, glue), None);
        }
    }
}

fn axis_faces(axis: u8) -> [u8; 2] {
    let face = [5, 1, 3][usize::from(axis)];
    [face, face ^ 1]
}

fn read(world: World<'_>, pos: Pos) -> Option<PaletteEntry> {
    let mut state = [UNLOADED_STATE];
    world.read(&[pos], &mut state);
    Some(PaletteEntry::parse(
        &CONTENT.get()?.describe_state(state[0])?,
    ))
}

fn axis(entry: &PaletteEntry) -> Option<u8> {
    AXES.iter()
        .position(|&axis| {
            entry
                .properties
                .get("axis")
                .is_some_and(|value| value == axis)
        })
        .map(|axis| axis as u8)
}

fn sticky(entry: &PaletteEntry, axis: u8, face: u8) -> bool {
    glueable_side(&entry.name, axis, face)
        .and_then(|key| entry.properties.get(key))
        .is_some_and(|value| value == "true")
}

pub fn placed_state(
    world: World<'_>,
    id: &str,
    pos: Pos,
    face: u8,
    look_axis: u8,
    sneak: bool,
) -> Option<u32> {
    let selected = if sneak {
        face_axis(face)
    } else if id != RADIAL {
        read(world, offset(pos, FACES[usize::from(face)]))
            .filter(|entry| matches!(entry.name.as_str(), LINEAR | SECONDARY))
            .and_then(|entry| axis(&entry))
            .unwrap_or(look_axis)
    } else {
        face_axis(face)
    };
    CONTENT
        .get()?
        .state(&format!("{id}[{}]", properties(id, selected, 0)))
}

fn allowed(world: World<'_>, entry: &PaletteEntry, axis_value: u8, pos: Pos, face: u8) -> bool {
    entry.name == RADIAL
        || read(world, offset(pos, FACES[usize::from(face)]))
            .is_some_and(|other| other.name != entry.name || axis(&other) != Some(axis_value))
}

pub fn use_item_on(context: &UseBlock<'_>) -> BlockUseResult {
    let Some(mut entry) = CONTENT
        .get()
        .and_then(|content| content.describe_state(context.state))
        .map(|selector| PaletteEntry::parse(&selector))
    else {
        return BlockUseResult::Pass;
    };
    let Some(axis) = axis(&entry) else {
        return BlockUseResult::Pass;
    };
    let Some(key) = glueable_side(&entry.name, axis, context.face) else {
        return BlockUseResult::TryEmptyHand;
    };
    let stack = context
        .stack
        .read()
        .and_then(|bytes| fastnbt::from_bytes(&bytes).ok());
    let slime = stack.as_ref().and_then(item_id).is_some_and(|id| {
        matches!(id, "minecraft:slime_ball" | "create:super_glue")
            || TAGS
                .get()
                .is_some_and(|tags| tags.contains("item", "c:slime_balls", id))
    });
    let glued = sticky(&entry, axis, context.face);
    if slime && glued {
        for face in 0..6 {
            if let Some(key) = glueable_side(&entry.name, axis, face)
                && allowed(context.world, &entry, axis, context.position, face)
            {
                entry.properties.insert(key.into(), "true".into());
            }
        }
    } else {
        if !slime && (stack.is_some() || !context.sneaking) {
            return BlockUseResult::TryEmptyHand;
        }
        if glued == slime || !allowed(context.world, &entry, axis, context.position, context.face) {
            return BlockUseResult::Pass;
        }
        entry.properties.insert(key.into(), slime.to_string());
    }
    if let Some(state) = entry.state()
        && context.world.write(&[BlockUpdate {
            position: context.position,
            state,
            flags: content::UPDATE_ALL,
        }]) == 1
    {
        return BlockUseResult::Success;
    }
    BlockUseResult::Pass
}

pub fn event(world: World<'_>, states: &StateIds, event: &content::ContentEvent) {
    if !states
        .by_id
        .get(&event.state)
        .is_some_and(|state| matches!(state.block, LINEAR | SECONDARY))
    {
        return;
    }
    let Some(mut entry) = read(world, event.position)
        .filter(|entry| matches!(entry.name.as_str(), LINEAR | SECONDARY))
    else {
        return;
    };
    let Some(axis) = axis(&entry) else { return };
    let mut changed = false;
    for face in axis_faces(axis) {
        let neighbor = offset(event.position, FACES[usize::from(face)]);
        if sticky(&entry, axis, face)
            && read(world, neighbor)
                .is_some_and(|other| other.name == entry.name && self::axis(&other) == Some(axis))
            && let Some(key) = glueable_side(&entry.name, axis, face)
        {
            entry.properties.insert(key.into(), "false".into());
            changed = true;
        }
    }
    if changed && let Some(state) = entry.state() {
        world.write(&[BlockUpdate {
            position: event.position,
            state,
            flags: content::UPDATE_CLIENTS,
        }]);
    }
}

#[derive(Deserialize)]
struct ChassisData {
    #[serde(rename = "ScrollValue")]
    range: i32,
}

pub fn attachments(
    world: World<'_>,
    checks: &BlockMovementChecks,
    pos: Pos,
    entry: &PaletteEntry,
    forced: Option<u8>,
) -> Option<Vec<Pos>> {
    if !is_chassis(&entry.name) {
        return Some(Vec::new());
    }
    let rotation_axis = axis(entry)?;
    let radial = entry.name == RADIAL;
    let range = world
        .entity(pos)
        .and_then(|bytes| fastnbt::from_bytes::<ChassisData>(&bytes).ok())
        .map_or(DEFAULT_RANGE, |data| data.range.clamp(1, MAX_CHASSIS_RANGE));
    let mut included = Vec::new();
    for face in 0..6 {
        if radial && face_axis(face) != rotation_axis {
            continue;
        }
        let side = offset(pos, FACES[usize::from(face)]);
        if read(world, side)
            .is_some_and(|other| other.name == entry.name && axis(&other) == Some(rotation_axis))
        {
            included.push(side);
        }
    }
    if !radial {
        for facing in axis_faces(rotation_axis) {
            if forced != Some(facing) && !sticky(entry, rotation_axis, facing) {
                continue;
            }
            for distance in 1..=range {
                let position = offset(pos, FACES[usize::from(facing)].map(|axis| axis * distance));
                if matches!(checks.classify(world, position)?, BlockMovement::Air) {
                    break;
                }
                included.push(position);
            }
        }
        return Some(included);
    }
    let limit_squared = (2 * range + 1).pow(2);
    for face in 0..6 {
        if !sticky(entry, rotation_axis, face) {
            continue;
        }
        let mut frontier = VecDeque::from([offset(pos, FACES[usize::from(face)])]);
        let mut visited = HashSet::from([pos]);
        while let Some(cell) = frontier.pop_front() {
            let squared =
                (cell.x - pos.x).pow(2) + (cell.y - pos.y).pow(2) + (cell.z - pos.z).pow(2);
            if 4 * squared >= limit_squared || !visited.insert(cell) {
                continue;
            }
            if matches!(checks.classify(world, cell)?, BlockMovement::Air) {
                continue;
            }
            included.push(cell);
            for side in (0..6).filter(|&side| face_axis(side) != rotation_axis) {
                frontier.push_back(offset(cell, FACES[usize::from(side)]));
            }
        }
    }
    Some(included)
}

pub fn apply_rotation(entry: &PaletteEntry, transform: StructureTransform) -> Option<PaletteEntry> {
    let current_axis = axis(entry)?;
    let new_axis = face_axis(transform.rotate_face(axis_faces(current_axis)[0]));
    let glue = (0..6)
        .filter(|&face| sticky(entry, current_axis, face))
        .fold(0u8, |mask, face| mask | (1 << transform.rotate_face(face)));
    Some(PaletteEntry::parse(&format!(
        "{}[{}]",
        entry.name,
        properties(&entry.name, new_axis, glue)
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quarter_turn_moves_glue_onto_the_new_physical_face() {
        let transform = StructureTransform {
            anchor: Pos { x: 0, y: 0, z: 0 },
            axis: 1,
            angle: 90.0,
        };
        let linear = PaletteEntry::parse(
            "create:linear_chassis[axis=x,sticky_top=true,sticky_bottom=false]",
        );
        let radial = PaletteEntry::parse("create:radial_chassis[axis=y,sticky_east=true]");
        assert_eq!(
            apply_rotation(&linear, transform),
            Some(PaletteEntry::parse(
                "create:linear_chassis[axis=z,sticky_top=false,sticky_bottom=true]",
            ))
        );
        assert_eq!(
            apply_rotation(&radial, transform),
            Some(PaletteEntry::parse(
                "create:radial_chassis[axis=y,sticky_north=true,sticky_south=false,sticky_east=false,sticky_west=false]",
            ))
        );
    }
}

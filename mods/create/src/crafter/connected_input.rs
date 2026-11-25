use std::collections::{HashSet, VecDeque};

use moddedmc_api::{Pos, content::World, effects::SOUND_BLOCKS};
use serde::{Deserialize, Serialize};

use super::{CrafterBlock, data, edit, plane, plane_block};
use crate::{
    EFFECTS, Kinetics,
    kinetics::{FACES, face_axis},
    offset,
};

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
struct InputOffset {
    x: i32,
    y: i32,
    z: i32,
}

impl InputOffset {
    fn between(from: Pos, to: Pos) -> Self {
        Self {
            x: to.x - from.x,
            y: to.y - from.y,
            z: to.z - from.z,
        }
    }

    fn apply(self, pos: Pos) -> Option<Pos> {
        Some(Pos {
            x: pos.x.checked_add(self.x)?,
            y: pos.y.checked_add(self.y)?,
            z: pos.z.checked_add(self.z)?,
        })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(super) struct ConnectedInput {
    controller: bool,
    data: Vec<InputOffset>,
}

fn controller(kinetics: &Kinetics, pos: Pos) -> Option<Pos> {
    match &data(kinetics, pos)?.input {
        Some(input) if !input.controller => input.data.first()?.apply(pos),
        _ => Some(pos),
    }
}

fn group(kinetics: &Kinetics, pos: Pos) -> Option<(Pos, Vec<Pos>)> {
    let root = controller(kinetics, pos)?;
    let nodes = match &data(kinetics, root)?.input {
        Some(input) if input.controller => input
            .data
            .iter()
            .map(|relative| relative.apply(root))
            .collect::<Option<Vec<_>>>()?,
        None if root == pos => vec![pos],
        _ => return None,
    };
    nodes.contains(&pos).then_some((root, nodes))
}

fn set_group(kinetics: &mut Kinetics, root: Pos, members: &[Pos]) {
    for &pos in members {
        let mut input = None;
        if members.len() > 1 {
            input = Some(ConnectedInput {
                controller: pos == root,
                data: if pos == root {
                    members
                        .iter()
                        .map(|&member| InputOffset::between(pos, member))
                        .collect()
                } else {
                    vec![InputOffset::between(pos, root)]
                },
            });
        }
        edit(kinetics, pos, |data| data.input = input);
    }
}

fn reachable(remaining: &mut HashSet<Pos>, start: Pos, block: CrafterBlock) -> Vec<Pos> {
    remaining.remove(&start);
    let mut group = vec![start];
    let mut queue = VecDeque::from([start]);
    while let Some(pos) = queue.pop_front() {
        for face in plane(block.facing) {
            let next = offset(pos, FACES[usize::from(face)]);
            if remaining.remove(&next) {
                group.push(next);
                queue.push_back(next);
            }
        }
    }
    group
}

fn toggle(kinetics: &mut Kinetics, first: Pos, second: Pos, block: CrafterBlock) -> bool {
    let (Some((first_root, mut first_group)), Some((second_root, second_group))) =
        (group(kinetics, first), group(kinetics, second))
    else {
        return false;
    };
    if first_group
        .iter()
        .chain(&second_group)
        .any(|&pos| plane_block(kinetics, pos, block.facing).is_none())
    {
        return false;
    }
    if first_root != second_root {
        first_group.extend(second_group);
        set_group(kinetics, first_root, &first_group);
    } else {
        let mut rest: HashSet<_> = first_group.into_iter().collect();
        rest.remove(&first);
        let detached = reachable(&mut rest, second, block);
        let attached: Vec<_> = std::iter::once(first).chain(rest).collect();
        set_group(kinetics, first, &attached);
        set_group(kinetics, second, &detached);
    }
    true
}

pub(super) fn on_remove(kinetics: &mut Kinetics, pos: Pos, block: CrafterBlock) {
    let Some((root, members)) = group(kinetics, pos) else {
        return;
    };
    let mut remaining: HashSet<_> = members
        .into_iter()
        .filter(|&member| member != pos && controller(kinetics, member) == Some(root))
        .collect();
    while let Some(start) = remaining.iter().min_by_key(|p| (p.y, p.z, p.x)).copied() {
        let component = reachable(&mut remaining, start, block);
        set_group(kinetics, start, &component);
    }
}

/// The combined inventory is visited top to bottom, then left to right as seen from the front.
pub(super) fn inventory(kinetics: &Kinetics, pos: Pos, block: CrafterBlock) -> Option<Vec<Pos>> {
    let (root, mut slots) = group(kinetics, pos)?;
    slots.retain(|&member| {
        plane_block(kinetics, member, block.facing).is_some()
            && controller(kinetics, member) == Some(root)
    });
    slots.sort_unstable_by_key(|&member| {
        let [x, y] = block.grid_position(member);
        (std::cmp::Reverse(y), x)
    });
    Some(slots)
}

pub(super) fn wrench_edge(
    world: World<'_>,
    kinetics: &mut Kinetics,
    pos: Pos,
    block: CrafterBlock,
    face: u8,
    hit: [f32; 3],
) -> bool {
    if face == block.facing || face >= 6 || !hit.iter().all(|part| (0.0..=1.0).contains(part)) {
        return false;
    }
    let face_offset = FACES[usize::from(face)];
    if world.face_sturdy(offset(pos, face_offset), face ^ 1) {
        return false;
    }
    // The edge interaction hitbox is 6/16 wide, inflated by 1/256.
    const BORDER: f32 = 10.0 / 16.0 - 1.0 / 256.0;
    let adjacent = plane(block.facing).into_iter().find_map(|direction| {
        let axis = usize::from(face_axis(direction));
        if axis == usize::from(face_axis(face)) {
            return None;
        }
        let toward = if direction.is_multiple_of(2) {
            1.0 - hit[axis]
        } else {
            hit[axis]
        };
        if toward < BORDER {
            return None;
        }
        let adjacent = offset(pos, FACES[usize::from(direction)]);
        (!world.face_sturdy(offset(adjacent, face_offset), face ^ 1)
            && plane_block(kinetics, adjacent, block.facing).is_some())
        .then_some(adjacent)
    });
    if !adjacent.is_some_and(|adjacent| toggle(kinetics, pos, adjacent, block)) {
        return false;
    }
    if let Some(effects) = EFFECTS.get() {
        effects.play_sound(
            world,
            [
                f64::from(pos.x) + 0.5,
                f64::from(pos.y) + 0.5,
                f64::from(pos.z) + 0.5,
            ],
            "minecraft:entity.item_frame.add_item",
            SOUND_BLOCKS,
            0.25,
            0.1,
        );
    }
    true
}

//! Common straight-track targeting for stations and signals.
use super::{graph::TrackEdge, math::Vec3, railway, runtime, save::RailwayData};
use fastnbt::IntArray;
use moddedmc_api::{
    Pos,
    content::{self, BlockUpdate, Content, World},
    effects::SOUND_BLOCKS,
    interaction::{UseOnBlock, UseResult},
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(super) struct Selection {
    pub dimension: String,
    pub target: [i32; 3],
    pub front: bool,
}

pub(super) fn pos([x, y, z]: [i32; 3]) -> Pos {
    Pos { x, y, z }
}

pub(super) fn coords(pos: Pos) -> [i32; 3] {
    [pos.x, pos.y, pos.z]
}

pub(super) fn state_at(world: World<'_>, pos: Pos) -> Option<u32> {
    let mut state = [content::UNLOADED_STATE];
    (world.read(&[pos], &mut state) && state[0] != content::UNLOADED_STATE).then_some(state[0])
}

pub(super) fn directed_edge<'a>(
    data: &'a RailwayData,
    world: World<'_>,
    content: Content,
    target: &Selection,
) -> Option<(&'a TrackEdge, f64)> {
    let position = pos(target.target);
    let (shape, _) = runtime::track_state(content, state_at(world, position)?)?;
    let [axis] = shape.axes() else { return None };
    locate(data, target, *axis)
}

pub(super) fn locate<'a>(
    data: &'a RailwayData,
    target: &Selection,
    axis: Vec3,
) -> Option<(&'a TrackEdge, f64)> {
    let position = pos(target.target);
    let center = Vec3 {
        x: f64::from(position.x) + 0.5,
        y: f64::from(position.y) + if axis.y == 0.0 { 0.0 } else { 0.5 },
        z: f64::from(position.z) + 0.5,
    };
    let wanted = axis
        .normalized()
        .scale(if target.front { -1.0 } else { 1.0 });

    data.graphs
        .iter()
        .flat_map(|saved| saved.graph.connections.values())
        .flat_map(|edges| edges.values())
        .find_map(|edge| {
            if edge.turn.is_some()
                || edge.from.dimension != target.dimension
                || edge.to.dimension != target.dimension
            {
                return None;
            }
            let from = edge.from.world();
            let direction = edge.to.world().minus(from).normalized();
            if direction.dot(wanted) < 1.0 - 1.0 / 4096.0 {
                return None;
            }
            let offset = center.minus(from);
            let distance = offset.dot(direction);
            let error = offset.minus(direction.scale(distance));
            ((0.0..edge.length()).contains(&distance) && error.dot(error) < 1.0 / 4096.0)
                .then_some((edge, distance))
        })
}

pub(super) enum TargetUse {
    Handled(UseResult),
    Place(Selection, u32),
}

pub(super) fn use_target(
    context: &mut UseOnBlock<'_>,
    content: Content,
    available: impl FnOnce(World<'_>, &Selection) -> bool,
) -> TargetUse {
    let world = context.world;
    let (Some(player), Some(clicked)) = (context.player, state_at(context.world, context.position))
    else {
        return TargetUse::Handled(UseResult::Fail);
    };
    let Some(dimension) = context.world.dimension_id() else {
        return TargetUse::Handled(UseResult::Fail);
    };

    let saved = context
        .stack
        .data()
        .and_then(|bytes| fastnbt::from_bytes::<Selection>(&bytes).ok());
    if context.sneaking && saved.is_some() {
        return TargetUse::Handled(if context.stack.set_data(&[]) {
            sound(
                context.world,
                context.position,
                "minecraft:block.lever.click",
                0.5,
            );
            UseResult::Success
        } else {
            UseResult::Fail
        });
    }
    if let Some((shape, _)) = runtime::track_state(content, clicked) {
        let (Some(view), [axis]) = (player.view(), shape.axes()) else {
            return TargetUse::Handled(UseResult::Fail);
        };
        let (sin, cos) = view.yaw.to_radians().sin_cos();
        let selected = Selection {
            dimension: dimension.to_owned(),
            target: coords(context.position),
            front: (-f64::from(sin) * axis.x + f64::from(cos) * axis.z) < 0.0,
        };
        let Ok(graphs) = railway::read(&context.world) else {
            return TargetUse::Handled(UseResult::Fail);
        };
        let Ok(bytes) = fastnbt::to_bytes(&selected) else {
            return TargetUse::Handled(UseResult::Fail);
        };
        return TargetUse::Handled(
            if available(world, &selected)
                && directed_edge(&graphs, context.world, content, &selected).is_some()
                && context.stack.set_data(&bytes)
            {
                sound(
                    context.world,
                    context.position,
                    "minecraft:block.lever.click",
                    1.0,
                );
                UseResult::Success
            } else {
                UseResult::Fail
            },
        );
    }

    let Some(selection) = saved.filter(|saved| saved.dimension == dimension) else {
        return TargetUse::Handled(UseResult::Fail);
    };
    TargetUse::Place(selection, clicked)
}

pub(super) fn placement(
    context: &UseOnBlock<'_>,
    content: Content,
    selection: &Selection,
    clicked: u32,
) -> Option<(Pos, u32, [i32; 3])> {
    const RANGE: i64 = 16;
    let world = context.world;
    let states = crate::states()?;
    let position = if states.replaceable.contains(&clicked) {
        context.position
    } else {
        super::item::relative(context.position, context.face)?
    };
    let previous = state_at(world, position).filter(|state| states.replaceable.contains(state))?;
    let placed = coords(position);
    let offsets =
        std::array::from_fn::<_, 3, _>(|i| i64::from(selection.target[i]) - i64::from(placed[i]));
    if offsets.iter().any(|&offset| offset.abs() >= RANGE)
        || offsets.iter().map(|offset| offset * offset).sum::<i64>() >= RANGE * RANGE
        || !railway::read(&world)
            .is_ok_and(|data| directed_edge(&data, world, content, selection).is_some())
    {
        return None;
    }
    Some((position, previous, offsets.map(|offset| offset as i32)))
}

#[derive(Serialize)]
struct TargetNbt {
    #[serde(rename = "TargetTrack")]
    target: IntArray,
    #[serde(rename = "TargetDirection")]
    front: bool,
}

pub(super) fn place_block(
    context: &mut UseOnBlock<'_>,
    selection: &Selection,
    placement: (Pos, u32, [i32; 3]),
    state: u32,
    persist: impl FnOnce() -> bool,
) -> UseResult {
    let (position, previous, offsets) = placement;
    let world = context.world;
    let Ok(nbt) = fastnbt::to_bytes(&TargetNbt {
        target: IntArray::new(offsets.to_vec()),
        front: selection.front,
    }) else {
        return UseResult::Fail;
    };
    let Ok(selected) = fastnbt::to_bytes(selection) else {
        return UseResult::Fail;
    };
    if !context.stack.set_data(&[]) {
        return UseResult::Fail;
    }
    let set_state = |state| {
        world.write(&[BlockUpdate {
            position,
            state,
            flags: content::UPDATE_ALL,
        }]) == 1
    };
    if !set_state(state) || !world.set_entity(position, &nbt) || !persist() {
        set_state(previous);
        context.stack.set_data(&selected);
        return UseResult::Fail;
    }
    if !context.creative {
        context.stack.shrink(1);
    }
    sound(world, position, "minecraft:block.stone.place", 0.8);
    UseResult::Success
}

pub(super) fn sound(world: World<'_>, position: Pos, sound: &str, pitch: f32) {
    if let Some(effects) = crate::EFFECTS.get() {
        effects.play_sound(
            world,
            coords(position).map(|axis| f64::from(axis) + 0.5),
            sound,
            SOUND_BLOCKS,
            0.75,
            pitch,
        );
    }
}

//! TrackBlockItem.useOn: selection and placement of track connections.

use super::{
    curve,
    math::Vec3,
    placement::{curve_placement, offset_position},
    runtime,
    track::{self, TrackShape},
};
use moddedmc_api::Pos;
use moddedmc_api::{
    content::{self, BlockUpdate, Content},
    interaction::{UseOnBlock, UseResult},
};
use serde::{Deserialize, Serialize};

/// Match TrackPlacement.ConnectingFrom: start block, selected axis, normal,
/// and rail curve start. The wire format here is our mod-scoped stack data,
/// not the client's Forge data component representation.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub(super) struct ConnectingFrom {
    pub pos: [i32; 3],
    pub axis: [f64; 3],
    pub normal: [f64; 3],
    pub end: [f64; 3],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Reject {
    SamePoint,
    TooFar,
    InvalidAxes,
    NotStraight,
    Obstructed,
}

pub(super) fn encode(selection: &ConnectingFrom) -> Result<Vec<u8>, String> {
    fastnbt::to_bytes(selection).map_err(|e| e.to_string())
}

pub(super) fn decode(bytes: &[u8]) -> Option<ConnectingFrom> {
    let from: ConnectingFrom = fastnbt::from_bytes(bytes).ok()?;
    let finite = from
        .axis
        .iter()
        .chain(from.normal.iter())
        .chain(from.end.iter())
        .all(|v| v.is_finite());
    (finite && from.axis.iter().map(|v| v * v).sum::<f64>() > 1.0e-5).then_some(from)
}

#[cfg(test)]
fn vec3([x, y, z]: [f64; 3]) -> Vec3 {
    Vec3 { x, y, z }
}
fn array(v: Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

pub(super) fn select(pos: Pos, shape: TrackShape, look: Vec3) -> Option<ConnectingFrom> {
    // TrackBlockItem blocks junction and tilted track starts.
    if shape.axes().len() != 1 || shape == TrackShape::None {
        return None;
    }
    let mut nearest_axis = None;
    let mut nearest_distance = f64::MAX;
    for &axis in shape.axes() {
        let axis_dir = axis.normalized();
        if axis_dir.dot(axis_dir) == 0.0 {
            continue;
        }
        for sign in [1.0, -1.0] {
            // ITrackBlock.getNearestTrackAxis uses the signed look distance to
            // choose an axis, then adjusts its facing with a horizontal dot.
            let delta = axis_dir.minus(look.scale(sign));
            let distance = delta.dot(delta);
            if distance <= nearest_distance {
                nearest_distance = distance;
                nearest_axis = Some(axis);
            }
        }
    }
    let axis = nearest_axis?;
    let horizontal = Vec3 {
        x: axis.x,
        y: 0.0,
        z: axis.z,
    }
    .normalized();
    let facing = if look.dot(horizontal) < 0.0 {
        -1.0
    } else {
        1.0
    };
    let axis = axis.scale(facing);
    let elevation = if axis.y != 0.0 { 0.5 } else { 0.0 };
    let center = Vec3 {
        x: f64::from(pos.x) + 0.5,
        y: f64::from(pos.y) + elevation,
        z: f64::from(pos.z) + 0.5,
    };
    let end = center.plus(axis.scale(0.5));
    // TrackShape.getNormal: the ascending track's surface normal is opposite
    // its horizontal slope direction.
    // The surface normal belongs to the block shape, independent of which
    // signed track axis the player selected.
    let normal = shape.normal();
    Some(ConnectingFrom {
        pos: [pos.x, pos.y, pos.z],
        axis: array(axis),
        normal: array(normal),
        end: array(end),
    })
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct StraightPlacement {
    /// Intermediate positions only; endpoints already contain rails.
    pub positions: Vec<Pos>,
    pub cost: u32,
}

/// A safe subset of TrackPlacement.tryConnect's parallel skipCurve branch:
/// both endpoints exist as straight/diagonal track, are aligned on one axis,
/// and the segment between them is a line with no forbidden blocks.
///
/// Forcing a curve or slope through here would lose turn block-entity state.
/// Keep that geometry in BezierConnection, not a fake line approximation.
pub(super) fn straight(
    selected: ConnectingFrom,
    second: Pos,
    first_shape: TrackShape,
    second_shape: TrackShape,
    max_length: i32,
    mut can_place: impl FnMut(Pos) -> bool,
) -> Result<StraightPlacement, Reject> {
    if selected.pos == [second.x, second.y, second.z] {
        return Err(Reject::SamePoint);
    }
    let diff = [
        i64::from(second.x) - i64::from(selected.pos[0]),
        i64::from(second.y) - i64::from(selected.pos[1]),
        i64::from(second.z) - i64::from(selected.pos[2]),
    ];
    // Even malformed selection data with opposite i32 extremes must not
    // overflow the distance check before the 32-block limit rejects it.
    let max_distance = i128::from(max_length) * i128::from(max_length);
    let dist_sq = diff
        .iter()
        .map(|&n| i128::from(n) * i128::from(n))
        .sum::<i128>();
    if max_length < 1 || dist_sq > max_distance {
        return Err(Reject::TooFar);
    }
    let axis = match first_shape {
        TrackShape::XOrtho if second_shape == first_shape => [1i64, 0, 0],
        TrackShape::ZOrtho if second_shape == first_shape => [0, 0, 1],
        TrackShape::PositiveDiagonal if second_shape == first_shape => [1, 0, 1],
        TrackShape::NegativeDiagonal if second_shape == first_shape => [-1, 0, 1],
        TrackShape::AscendingNorth if second_shape == first_shape => [0, 1, -1],
        TrackShape::AscendingSouth if second_shape == first_shape => [0, 1, 1],
        TrackShape::AscendingEast if second_shape == first_shape => [1, 1, 0],
        TrackShape::AscendingWest if second_shape == first_shape => [-1, 1, 0],
        _ => return Err(Reject::NotStraight),
    };
    let count = (0..3)
        .find(|&index| axis[index] != 0)
        .map(|i| diff[i] / axis[i])
        .ok_or(Reject::InvalidAxes)?;
    if count == 0 || diff != axis.map(|n| n * count) {
        return Err(Reject::NotStraight);
    }
    let steps = count.unsigned_abs();
    if steps > 2048 {
        return Err(Reject::TooFar);
    }
    let step = axis.map(|v| v * count.signum());
    let mut positions = Vec::with_capacity(steps.saturating_sub(1) as usize);
    for index in 1..steps {
        let coords = [
            i64::from(selected.pos[0]) + step[0] * index as i64,
            i64::from(selected.pos[1]) + step[1] * index as i64,
            i64::from(selected.pos[2]) + step[2] * index as i64,
        ];
        let [Ok(x), Ok(y), Ok(z)] = coords.map(i32::try_from) else {
            return Err(Reject::TooFar);
        };
        let pos = Pos { x, y, z };
        if !can_place(pos) {
            return Err(Reject::Obstructed);
        }
        positions.push(pos);
    }
    Ok(StraightPlacement {
        cost: positions.len() as u32,
        positions,
    })
}

/// Vanilla Direction offsets, with overflow-safe world coordinates.
fn relative(pos: Pos, face: u8) -> Option<Pos> {
    let [dx, dy, dz] = match face {
        0 => [0, -1, 0],
        1 => [0, 1, 0],
        2 => [0, 0, -1],
        3 => [0, 0, 1],
        4 => [-1, 0, 0],
        5 => [1, 0, 0],
        _ => return None,
    };
    Some(Pos {
        x: pos.x.checked_add(dx)?,
        y: pos.y.checked_add(dy)?,
        z: pos.z.checked_add(dz)?,
    })
}

pub(crate) fn use_on_block(context: &mut UseOnBlock<'_>) -> UseResult {
    let Some(content) = crate::CONTENT.get().copied() else {
        return UseResult::Fail;
    };
    let Some(player) = context.player else {
        return UseResult::Pass;
    };
    let existing_data = context.stack.data().unwrap_or_default();
    let saved = decode(&existing_data);

    if saved.is_some() && context.sneaking {
        return if context.stack.set_data(&[]) {
            UseResult::Success
        } else {
            UseResult::Fail
        };
    }

    let Some(view) = player.view() else {
        return UseResult::Fail;
    };
    let (sin_yaw, cos_yaw) = view.yaw.to_radians().sin_cos();
    let (sin_pitch, cos_pitch) = view.pitch.to_radians().sin_cos();
    let look = Vec3 {
        x: -f64::from(sin_yaw * cos_pitch),
        y: -f64::from(sin_pitch),
        z: f64::from(cos_yaw * cos_pitch),
    };

    // TrackBlockItem accepts an existing rail or a new endpoint placed on terrain.
    let mut current = [content::UNLOADED_STATE];
    if !context.world.read(&[context.position], &mut current) {
        return UseResult::Fail;
    }
    let mut new_endpoint = None;
    let (target, clicked_shape) =
        if let Some((shape, _)) = runtime::track_state(content, current[0]) {
            (context.position, shape)
        } else {
            if saved.is_none() {
                // No selection: normal BlockItem placement, as in Java.
                return UseResult::Pass;
            }
            let Some(state_ids) = crate::states() else {
                return UseResult::Fail;
            };
            let target = if state_ids.replaceable.contains(&current[0]) {
                context.position
            } else {
                let Some(adjacent) = relative(context.position, context.face) else {
                    return UseResult::Fail;
                };
                adjacent
            };
            let mut existing = [content::UNLOADED_STATE];
            if !context.world.read(&[target], &mut existing)
                || !state_ids.replaceable.contains(&existing[0])
            {
                return UseResult::Fail;
            }
            let Some(state) = super::runtime::placed_item_target(
                content,
                context.world,
                target,
                context.face,
                view.yaw,
                view.pitch,
            ) else {
                return UseResult::Fail;
            };
            let Some((shape, _)) = runtime::track_state(content, state) else {
                return UseResult::Fail;
            };
            new_endpoint = Some(state);
            (target, shape)
        };
    if clicked_shape.axes().len() != 1 {
        // A junction cannot serve as the selected track axis.
        return if saved.is_some() {
            UseResult::Fail
        } else {
            UseResult::Success
        };
    }

    if let Some(start) = saved {
        let first = Pos {
            x: start.pos[0],
            y: start.pos[1],
            z: start.pos[2],
        };
        let mut source = [content::UNLOADED_STATE];
        if !context.world.read(&[first], &mut source) {
            return UseResult::Fail;
        }
        let Some((source_shape, _)) = runtime::track_state(content, source[0]) else {
            return UseResult::Fail;
        };
        let [x, y, z] = start.axis;
        let Some(start) = select(first, source_shape, Vec3 { x, y, z }) else {
            return UseResult::Fail;
        };
        let world = context.world;
        let result = straight(start, target, source_shape, clicked_shape, 32, |pos| {
            let mut state = [content::UNLOADED_STATE];
            world.read(&[pos], &mut state)
                && crate::states().is_some_and(|states| states.replaceable.contains(&state[0]))
        });
        let plan = match result {
            Ok(plan) => plan,
            Err(Reject::NotStraight) => {
                let Some(second) = select(target, clicked_shape, look.scale(-1.0)) else {
                    return UseResult::Fail;
                };
                return place_curve(
                    context,
                    content,
                    start,
                    second,
                    [source_shape, clicked_shape],
                );
            }
            Err(_) => return UseResult::Fail,
        };
        // A click on terrain also creates the second endpoint block.
        let total_cost = plan.cost + u32::from(new_endpoint.is_some());
        if !context.creative && total_cost > context.stack.count() {
            // TODO(abi): TrackPlacement inventory-wide material gathering and
            // offhand pavement, not just the clicked stack's count.
            return UseResult::Fail;
        }
        let mut updates =
            Vec::with_capacity(plan.positions.len() + usize::from(new_endpoint.is_some()));
        for pos in plan.positions {
            let mut existing = [content::UNLOADED_STATE];
            if !world.read(&[pos], &mut existing) {
                return UseResult::Fail;
            }
            let wet = content.state("minecraft:water") == Some(existing[0]);
            let Some(state) = super::runtime::rail_state(content, source_shape, wet) else {
                return UseResult::Fail;
            };
            updates.push(BlockUpdate {
                position: pos,
                state,
                flags: content::UPDATE_ALL,
            });
        }
        if let Some(state) = new_endpoint {
            updates.push(BlockUpdate {
                position: target,
                state,
                flags: content::UPDATE_ALL,
            });
        }
        // Check the saved component before touching the world. A failed stack
        // edit must not leave a world with only half the expected interaction.
        if !context.stack.set_data(&[]) {
            return UseResult::Fail;
        }
        let count = updates.len();
        if world.write(&updates) != count {
            // Do not charge for an incomplete placement.
            return UseResult::Fail;
        }
        if !context.creative {
            context.stack.shrink(total_cost);
        }
        return UseResult::Success;
    }

    let Some(start) = select(target, clicked_shape, look) else {
        return UseResult::Success;
    };
    let Ok(data) = encode(&start) else {
        return UseResult::Fail;
    };
    if context.stack.set_data(&data) {
        UseResult::Success
    } else {
        UseResult::Fail
    }
}

fn place_curve(
    context: &mut UseOnBlock<'_>,
    content: Content,
    first: ConnectingFrom,
    second: ConnectingFrom,
    shapes: [TrackShape; 2],
) -> UseResult {
    let Some(plan) = curve_placement(first, second) else {
        return UseResult::Fail;
    };
    let world = context.world;
    let (Some(states), Some(geometry)) = (crate::states(), plan.curve.geometry()) else {
        return UseResult::Fail;
    };
    let mut cost = 0;
    let mut updates = Vec::new();
    let mut rollback = Vec::new();
    let mut endpoints = Vec::new();

    for (i, connection) in [first, second].into_iter().enumerate() {
        let endpoint = plan.extents[i];
        for step in 0..=endpoint {
            let Some([x, y, z]) =
                offset_position(connection.pos, plan.axes[i].scale(f64::from(step)))
            else {
                return UseResult::Fail;
            };
            let position = Pos { x, y, z };
            let mut current = [content::UNLOADED_STATE];
            if !world.read(&[position], &mut current) {
                return UseResult::Fail;
            }
            let old = current[0];
            let turn = step == endpoint;
            let (shape, wet, had_turn) =
                if let Some((shape, had_turn)) = runtime::track_state(content, old) {
                    (
                        track::overlay(shape, shapes[i]),
                        runtime::track_waterlogged(content, old),
                        had_turn,
                    )
                } else if states.replaceable.contains(&old) {
                    cost += 1;
                    (
                        shapes[i],
                        world.fluid_id(position).as_deref() == Some("minecraft:water"),
                        false,
                    )
                } else {
                    return UseResult::Fail;
                };
            let Some(state) = runtime::rail_state_with_turn(content, shape, wet, turn || had_turn)
            else {
                return UseResult::Fail;
            };
            if state != old {
                updates.push(BlockUpdate {
                    position,
                    state,
                    flags: content::UPDATE_ALL,
                });
                rollback.push(BlockUpdate {
                    position,
                    state: old,
                    flags: content::UPDATE_ALL,
                });
            }
            if turn {
                let previous = if had_turn {
                    let Some(previous) = curve::read(world, position) else {
                        return UseResult::Fail;
                    };
                    previous
                } else {
                    Vec::new()
                };
                let record = if i == 0 {
                    plan.curve.clone()
                } else {
                    plan.curve.secondary()
                };
                let duplicate = previous
                    .iter()
                    .any(|value| value.block_positions[1] == record.block_positions[1]);
                if i == 0 && !duplicate {
                    cost += geometry.track_item_cost() as u32;
                }
                let Some(bytes) = curve::add_connection(&previous, record) else {
                    return UseResult::Fail;
                };
                endpoints.push((position, previous, bytes));
            }
        }
    }

    if !context.creative && cost > context.stack.count() {
        return UseResult::Fail;
    }
    if world.write(&updates) != updates.len() {
        return UseResult::Fail;
    }
    if !endpoints
        .iter()
        .all(|(pos, _, bytes)| world.set_entity(*pos, bytes))
        || !context.stack.set_data(&[])
    {
        for (pos, previous, _) in endpoints {
            if let Some(bytes) = curve::encode_connections(&previous) {
                world.set_entity(pos, &bytes);
            }
        }
        world.write(&rollback);
        return UseResult::Fail;
    }
    if !context.creative {
        context.stack.shrink(cost);
    }
    crate::with_world(world, |kinetics| {
        for (pos, _, _) in endpoints {
            kinetics.rail_updates.connection_changed(pos);
        }
    });
    UseResult::Success
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(x: i32, y: i32, z: i32) -> Pos {
        Pos { x, y, z }
    }
    const EAST: Vec3 = Vec3 {
        x: 1.0,
        y: 0.0,
        z: 0.0,
    };
    const NORTH: Vec3 = Vec3 {
        x: 0.0,
        y: 0.0,
        z: -1.0,
    };

    #[test]
    fn clicked_face_offsets_are_vanilla_order_and_bounds_checked() {
        let origin = point(1, 64, -2);
        assert_eq!(relative(origin, 0), Some(point(1, 63, -2)));
        assert_eq!(relative(origin, 1), Some(point(1, 65, -2)));
        assert_eq!(relative(origin, 2), Some(point(1, 64, -3)));
        assert_eq!(relative(origin, 3), Some(point(1, 64, -1)));
        assert_eq!(relative(origin, 4), Some(point(0, 64, -2)));
        assert_eq!(relative(origin, 5), Some(point(2, 64, -2)));
        assert_eq!(relative(origin, 6), None);
        assert_eq!(relative(point(i32::MAX, 64, 0), 5), None);
    }

    #[test]
    fn selection_roundtrips_in_mod_scoped_stack_nbt() {
        let first = select(point(-14, 63, 27), TrackShape::XOrtho, EAST).unwrap();
        assert_eq!(first.pos, [-14, 63, 27]);
        assert_eq!(first.axis, [1.0, 0.0, 0.0]);
        assert_eq!(first.end, [-13.0, 63.0, 27.5]);
        let bytes = encode(&first).unwrap();
        assert_eq!(decode(&bytes), Some(first));
        assert!(decode(&[]).is_none());
        let mut corrupt = first;
        corrupt.axis = [f64::NAN, 0.0, 0.0];
        assert!(decode(&encode(&corrupt).unwrap()).is_none());
    }

    #[test]
    fn first_click_selects_signed_track_axis_and_surface_normal() {
        let east = select(point(0, 64, 0), TrackShape::XOrtho, EAST).unwrap();
        let west = select(point(0, 64, 0), TrackShape::XOrtho, EAST.scale(-1.0)).unwrap();
        assert_eq!(east.axis, [1.0, 0.0, 0.0]);
        assert_eq!(west.axis, [-1.0, 0.0, 0.0]);
        assert_eq!(west.end, [0.0, 64.0, 0.5]);
        let north = select(point(2, 100, -7), TrackShape::ZOrtho, NORTH).unwrap();
        assert_eq!(north.axis, [0.0, 0.0, -1.0]);
        assert_eq!(north.end, [2.5, 100.0, -7.0]);
        let rise = select(point(1, 80, 3), TrackShape::AscendingNorth, NORTH).unwrap();
        assert_eq!(rise.axis, [0.0, 1.0, -1.0]);
        assert_eq!(rise.end, [1.5, 81.0, 3.0]);
        assert!(vec3(rise.normal).dot(vec3(rise.axis)).abs() < 1.0e-12);
        let reverse_rise = select(
            point(1, 80, 3),
            TrackShape::AscendingNorth,
            NORTH.scale(-1.0),
        )
        .unwrap();
        assert_eq!(reverse_rise.normal, rise.normal);
        assert_eq!(reverse_rise.axis, [0.0, -1.0, 1.0]);
    }

    #[test]
    fn junctions_and_none_shapes_cannot_be_first_point() {
        assert!(select(point(0, 64, 0), TrackShape::CrossOrtho, EAST).is_none());
        assert!(select(point(0, 64, 0), TrackShape::CrossDiagonal, EAST).is_none());
        assert!(select(point(0, 64, 0), TrackShape::None, EAST).is_none());
    }

    #[test]
    fn straight_branch_inserts_only_missing_track_positions() {
        let start = select(point(0, 64, 0), TrackShape::XOrtho, EAST).unwrap();
        let mut seen = Vec::new();
        let result = straight(
            start,
            point(6, 64, 0),
            TrackShape::XOrtho,
            TrackShape::XOrtho,
            32,
            |pos| {
                seen.push(pos);
                true
            },
        )
        .unwrap();
        assert_eq!(result.cost, 5);
        assert_eq!(seen, result.positions);
        assert_eq!(
            result.positions,
            (1..=5).map(|x| point(x, 64, 0)).collect::<Vec<_>>()
        );
        let reversed = straight(
            start,
            point(-3, 64, 0),
            TrackShape::XOrtho,
            TrackShape::XOrtho,
            32,
            |_| true,
        )
        .unwrap();
        assert_eq!(reversed.positions, vec![point(-1, 64, 0), point(-2, 64, 0)]);
    }

    #[test]
    fn diagonal_rails_use_diagonal_steps_and_manhattan_axis_ratio() {
        let selected = select(point(0, 64, 0), TrackShape::PositiveDiagonal, EAST).unwrap();
        let result = straight(
            selected,
            point(3, 64, 3),
            TrackShape::PositiveDiagonal,
            TrackShape::PositiveDiagonal,
            32,
            |_| true,
        )
        .unwrap();
        assert_eq!(result.positions, vec![point(1, 64, 1), point(2, 64, 2)]);
        assert_eq!(result.cost, 2);
        assert_eq!(
            straight(
                selected,
                point(4, 64, 3),
                TrackShape::PositiveDiagonal,
                TrackShape::PositiveDiagonal,
                32,
                |_| true
            ),
            Err(Reject::NotStraight)
        );
    }

    #[test]
    fn invalid_distance_shape_or_obstruction_never_mutates_world() {
        let first = select(point(0, 64, 0), TrackShape::XOrtho, EAST).unwrap();
        assert_eq!(
            straight(
                first,
                point(0, 64, 0),
                TrackShape::XOrtho,
                TrackShape::XOrtho,
                32,
                |_| true
            ),
            Err(Reject::SamePoint)
        );
        assert_eq!(
            straight(
                first,
                point(64, 64, 0),
                TrackShape::XOrtho,
                TrackShape::XOrtho,
                32,
                |_| true
            ),
            Err(Reject::TooFar)
        );
        assert_eq!(
            straight(
                first,
                point(4, 64, 0),
                TrackShape::XOrtho,
                TrackShape::ZOrtho,
                32,
                |_| true
            ),
            Err(Reject::NotStraight)
        );
        assert_eq!(
            straight(
                first,
                point(4, 65, 0),
                TrackShape::XOrtho,
                TrackShape::XOrtho,
                32,
                |_| true
            ),
            Err(Reject::NotStraight)
        );
        let mut corrupted = first;
        corrupted.pos[0] = i32::MIN;
        assert_eq!(
            straight(
                corrupted,
                point(i32::MAX, 64, 0),
                TrackShape::XOrtho,
                TrackShape::XOrtho,
                32,
                |_| true
            ),
            Err(Reject::TooFar)
        );
        let mut count = 0;
        assert_eq!(
            straight(
                first,
                point(4, 64, 0),
                TrackShape::XOrtho,
                TrackShape::XOrtho,
                32,
                |pos| {
                    count += 1;
                    pos.x != 2
                }
            ),
            Err(Reject::Obstructed)
        );
        assert_eq!(count, 2);
    }
}

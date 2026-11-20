//! The track item's first-click selection and the straight-placement branch
//! of TrackPlacement.tryConnect. Curve/slope turns are deliberately not made
//! into straight rails; they are delegated to the Bezier implementation.

use super::{math::Vec3, track::TrackShape};
use moddedmc_api::Pos;
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

/// Partial TrackBlockItem.useOn: selecting the first track, clearing it while
/// sneaking, and placing an aligned straight run between two existing rails.
/// Curves, slope transitions, and offhand material gathering still require the
/// full TrackPlacement and host block-entity support.
pub(crate) fn use_on_block(
    context: &mut moddedmc_api::interaction::UseOnBlock<'_>,
) -> moddedmc_api::interaction::UseResult {
    use moddedmc_api::{
        content::{self, BlockUpdate},
        interaction::UseResult,
    };

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

    // The clicked block must be a track. When nothing is selected, let the
    // host's normal block-item placement work on non-tracks as Create does.
    let mut current = [content::UNLOADED_STATE];
    if !context.world.read(&[context.position], &mut current) {
        return UseResult::Fail;
    }
    let Some((clicked_shape, clicked_turn)) = super::runtime::track_state(content, current[0])
    else {
        return if saved.is_none() {
            UseResult::Pass
        } else {
            UseResult::Fail
        };
    };
    if clicked_shape.axes().len() != 1 || clicked_turn {
        // TrackBlockItem blocks junction and tilted first points.
        return UseResult::Success;
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
        let Some((source_shape, source_turn)) = super::runtime::track_state(content, source[0])
        else {
            return UseResult::Fail;
        };
        if source_turn {
            return UseResult::Fail;
        }
        let world = context.world;
        let result = straight(
            start,
            context.position,
            source_shape,
            clicked_shape,
            32,
            |pos| {
                let mut state = [content::UNLOADED_STATE];
                world.read(&[pos], &mut state)
                    && crate::states().is_some_and(|states| states.replaceable.contains(&state[0]))
            },
        );
        let Ok(plan) = result else {
            return UseResult::Fail;
        };
        if !context.creative && plan.cost > context.stack.count() {
            // TODO(abi): TrackPlacement inventory-wide material gathering and
            // offhand pavement, not just the clicked stack's count.
            return UseResult::Fail;
        }
        let mut updates = Vec::with_capacity(plan.positions.len());
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
        // Check the saved component before touching the world. A failed stack
        // edit must not leave a world with only half the expected interaction.
        if !context.stack.set_data(&[]) {
            return UseResult::Fail;
        }
        let count = updates.len();
        if world.write(&updates) != count {
            // The world could unload during the callback; don't charge tracks
            // if any write did not commit. A save-global graph is NOT created
            // here until WS-3-01 is implemented.
            return UseResult::Fail;
        }
        if !context.creative {
            context.stack.shrink(plan.cost);
        }
        return UseResult::Success;
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
    let Some(start) = select(context.position, clicked_shape, look) else {
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

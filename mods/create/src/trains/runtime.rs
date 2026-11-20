//! Create 6.0.10 TrackBlock content registration and first-click placement.
//! Blockstate/model variants are copied from the original jar's
//! assets/create/blockstates/track.json; the jar assets are *not* included here.
use super::math::Vec3;
use super::track::{TrackShape, select_placement_shape};
use moddedmc_api::{
    Pos, RegistrationError, Registry,
    content::{Block, Content, Placement, ShapeBox, State},
};
use std::{collections::HashMap, sync::OnceLock};

const ID: &str = "create:track";
const SHAPE_FLAT: [ShapeBox; 1] = [ShapeBox {
    min: [0, 0, 0],
    max: [16, 2, 16],
}];
// TrackBlock.getCollisionShape returns Shapes.empty() for ascending tracks.
const SHAPE_SLOPE: [ShapeBox; 0] = [];
const ALL_SHAPES: [TrackShape; 19] = [
    TrackShape::ZOrtho, // Java TrackBlock's defaultState is ZO.
    TrackShape::XOrtho,
    TrackShape::PositiveDiagonal,
    TrackShape::NegativeDiagonal,
    TrackShape::AscendingNorth,
    TrackShape::AscendingSouth,
    TrackShape::AscendingEast,
    TrackShape::AscendingWest,
    TrackShape::TeleportNorth,
    TrackShape::TeleportSouth,
    TrackShape::TeleportEast,
    TrackShape::TeleportWest,
    TrackShape::CrossOrtho,
    TrackShape::CrossDiagonal,
    TrackShape::CrossPositiveX,
    TrackShape::CrossPositiveZ,
    TrackShape::CrossNegativeX,
    TrackShape::CrossNegativeZ,
    TrackShape::None,
];

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct TrackState {
    shape: TrackShape,
    has_be: bool,
    wet: bool,
}

#[derive(Default)]
struct TrackStates {
    by_id: HashMap<u32, TrackState>,
    by_properties: HashMap<TrackState, u32>,
}
static STATE_IDS: OnceLock<TrackStates> = OnceLock::new();

fn model(shape: TrackShape) -> (&'static str, i16) {
    use TrackShape as T;
    match shape {
        T::None => ("minecraft:block/air", 0),
        T::ZOrtho => ("create:block/track/z_ortho", 0),
        T::XOrtho => ("create:block/track/x_ortho", 0),
        T::PositiveDiagonal => ("create:block/track/diag", 0),
        T::NegativeDiagonal => ("create:block/track/diag_2", 0),
        T::AscendingNorth => ("create:block/track/ascending", 180),
        T::AscendingSouth => ("create:block/track/ascending", 0),
        T::AscendingEast => ("create:block/track/ascending", 270),
        T::AscendingWest => ("create:block/track/ascending", 90),
        T::TeleportNorth => ("create:block/track/teleport", 180),
        T::TeleportSouth => ("create:block/track/teleport", 0),
        T::TeleportEast => ("create:block/track/teleport", 270),
        T::TeleportWest => ("create:block/track/teleport", 90),
        T::CrossOrtho => ("create:block/track/cross_ortho", 0),
        T::CrossDiagonal => ("create:block/track/cross_diag", 0),
        T::CrossPositiveX => ("create:block/track/cross_d1_xo", 0),
        T::CrossPositiveZ => ("create:block/track/cross_d1_zo", 0),
        T::CrossNegativeX => ("create:block/track/cross_d2_xo", 0),
        T::CrossNegativeZ => ("create:block/track/cross_d2_zo", 0),
    }
}

fn properties(row: TrackState) -> String {
    format!(
        "shape={},turn={},waterlogged={}",
        row.shape.serialized(),
        row.has_be,
        row.wet
    )
}

fn variants() -> Vec<TrackState> {
    let mut result = Vec::with_capacity(19 * 4);
    for shape in ALL_SHAPES {
        for has_be in [false, true] {
            for wet in [false, true] {
                result.push(TrackState { shape, has_be, wet });
            }
        }
    }
    result
}

pub(crate) fn register(
    content: &Content,
    registry: &mut Registry<'_>,
) -> Result<(), RegistrationError> {
    let rows = variants();
    let props: Vec<_> = rows.iter().copied().map(properties).collect();
    let states: Vec<_> = rows
        .iter()
        .zip(&props)
        .map(|(row, properties)| {
            let (model, y) = model(row.shape);
            State {
                properties,
                physical: "minecraft:rail",
                model,
                rotating_model: "",
                x: 0,
                y,
                orientation: [0.0, 0.0, 0.0, 1.0],
                shape: if matches!(
                    row.shape,
                    TrackShape::AscendingNorth
                        | TrackShape::AscendingSouth
                        | TrackShape::AscendingEast
                        | TrackShape::AscendingWest
                ) {
                    &SHAPE_SLOPE
                } else {
                    &SHAPE_FLAT
                },
            }
        })
        .collect();
    content.add_block(
        registry,
        &Block {
            id: ID,
            display_name: "Train Track",
            // Java creates a block entity only for turn=true; the current
            // generic content API attaches one to every state or none.
            // TODO(abi): per-state block entities for curved tracks (WS-3-02).
            entity: "",
            item_model: "create:item/track",
            states: &states,
        },
        Some(super::super::placed_state),
    )
}

/// Read-only diagnostic for a track state; used by the WS-3 bot scenario.
pub(crate) fn describe_state(content: Content, state: u32) -> Option<String> {
    states(content)
        .by_id
        .get(&state)
        .map(|row| format!("{ID}[{}]", properties(*row)))
}

fn states(content: Content) -> &'static TrackStates {
    STATE_IDS.get_or_init(|| {
        let mut result = TrackStates::default();
        for row in variants() {
            let selector = format!("{ID}[{}]", properties(row));
            if let Some(state) = content.state(&selector) {
                result.by_id.insert(state, row);
                result.by_properties.insert(row, state);
            }
        }
        result
    })
}

/// Called by Create's shared placement callback only for track states.
/// All other blocks return None to let the existing kinetic callback run.
pub(crate) fn placed_state(args: &Placement<'_>, content: Content) -> Option<u32> {
    states(content).by_id.get(&args.state)?;
    let (sin_yaw, cos_yaw) = args.yaw.to_radians().sin_cos();
    let cos_pitch = args.pitch.to_radians().cos();
    let look = Vec3 {
        x: -f64::from(sin_yaw * cos_pitch),
        y: -f64::from(args.pitch.to_radians().sin()),
        z: f64::from(cos_yaw * cos_pitch),
    };
    let start = select_placement_shape(look, args.yaw, false, false);

    let (negative, positive) = match start {
        TrackShape::XOrtho => ([-1, 0, 0], [1, 0, 0]),
        TrackShape::ZOrtho => ([0, 0, -1], [0, 0, 1]),
        _ => ([0, 0, 0], [0, 0, 0]),
    };
    let check_sturdy = |direction: [i32; 3]| {
        if direction == [0, 0, 0] {
            return false;
        }
        let neighbor = Pos {
            x: args.position.x + direction[0],
            y: args.position.y + direction[1],
            z: args.position.z + direction[2],
        };
        let above = Pos {
            y: neighbor.y + 1,
            ..neighbor
        };
        args.world.face_sturdy(neighbor, 1) && !args.world.face_sturdy(above, 0)
    };
    let shape = select_placement_shape(
        look,
        args.yaw,
        check_sturdy(negative),
        check_sturdy(positive),
    );

    let mut placed = [0u32];
    if !args.world.read(&[args.position], &mut placed) {
        return None;
    }
    let wet = content.state("minecraft:water") == Some(placed[0]);

    states(content)
        .by_properties
        .get(&TrackState {
            shape,
            has_be: false,
            wet,
        })
        .copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_76_create_blockstate_variants_are_unique_and_default_to_zo() {
        let rows = variants();
        assert_eq!(rows.len(), 76);
        assert_eq!(rows[0].shape, TrackShape::ZOrtho);
        assert_eq!(properties(rows[0]), "shape=zo,turn=false,waterlogged=false");
        assert_eq!(
            rows.iter()
                .copied()
                .collect::<std::collections::HashSet<_>>()
                .len(),
            76
        );
        assert!(
            rows.iter()
                .any(|row| properties(*row) == "shape=tn,turn=true,waterlogged=true")
        );
        assert!(
            rows.iter()
                .any(|row| properties(*row) == "shape=none,turn=false,waterlogged=false")
        );
    }

    #[test]
    fn model_paths_and_y_rotations_match_create_6010_assets() {
        assert_eq!(
            model(TrackShape::AscendingNorth),
            ("create:block/track/ascending", 180)
        );
        assert_eq!(
            model(TrackShape::AscendingEast),
            ("create:block/track/ascending", 270)
        );
        assert_eq!(
            model(TrackShape::CrossNegativeZ),
            ("create:block/track/cross_d2_zo", 0)
        );
        assert_eq!(model(TrackShape::XOrtho), ("create:block/track/x_ortho", 0));
        assert_eq!(model(TrackShape::None), ("minecraft:block/air", 0));
        assert_eq!(SHAPE_FLAT[0].max, [16, 2, 16]);
        assert!(SHAPE_SLOPE.is_empty());
    }
}

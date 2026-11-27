//! Stock stair carriers with matching collision, including waterlogged states.

use pumpkin_data::{
    Block, BlockStateId,
    block_properties::{BlockProperties, Half, HorizontalFacing, StairsShape},
    tag,
};
use serde_json::{Map, Value, json};

use crate::content::RegisteredState;

type StairsProperties = pumpkin_data::block_properties::WhiteWoolStairsLikeProperties;

const DISPLAY: (&Block, &Block) = (
    &Block::WAXED_OXIDIZED_CUT_COPPER_STAIRS,
    &Block::OXIDIZED_CUT_COPPER_STAIRS,
);

pub const STRAIGHT_CARRIERS: [(&Block, &Block); 3] = [
    (&Block::WAXED_CUT_COPPER_STAIRS, &Block::CUT_COPPER_STAIRS),
    (
        &Block::WAXED_EXPOSED_CUT_COPPER_STAIRS,
        &Block::EXPOSED_CUT_COPPER_STAIRS,
    ),
    (
        &Block::WAXED_WEATHERED_CUT_COPPER_STAIRS,
        &Block::WEATHERED_CUT_COPPER_STAIRS,
    ),
];

pub fn corner_carriers() -> impl Iterator<Item = &'static Block> {
    tag::Block::MINECRAFT_STAIRS
        .0
        .iter()
        .filter_map(|name| Block::from_name(name))
        .filter(|block| block.id != DISPLAY.0.id && StairsProperties::handles_block_id(block.id))
}

/// A complete stair state set with the stock shape for every selector.
pub fn has_stair_shape(states: &[RegisteredState]) -> bool {
    states.len() == Block::STONE_STAIRS.states.len()
        && states.iter().all(|state| {
            StairsProperties::handles_block_id(state.physical.id.to_block().id)
                && state.shape.is_empty()
                && state.properties.len() == 4
                && StairsProperties::from_state_id(state.physical.id)
                    .to_props()
                    .iter()
                    .all(|prop| {
                        state
                            .properties
                            .get(prop.key_str())
                            .is_some_and(|value| value == prop.value_str())
                    })
        })
}

pub fn allocate(
    state: &mut RegisteredState,
    corners: Option<&Block>,
    straights: Option<(&Block, &Block)>,
) -> (BlockStateId, BlockStateId) {
    let props = StairsProperties::from_state_id(state.physical.id);
    let visual = &mut state.visual;
    visual.uvlock = visual.x != 0 || visual.y != 0;
    match (props.shape, corners, straights) {
        (StairsShape::Straight, _, Some((carrier, replacement))) => {
            return (props.to_state_id(carrier), props.to_state_id(replacement));
        }
        (_, Some(block), _) => {
            if let Some((left, right)) = corner_pair(props) {
                return (left.to_state_id(block), right.to_state_id(block));
            }
        }
        _ => {}
    }

    visual.rotating_model = std::mem::take(&mut visual.model);
    visual.bounded = true;
    visual.straight_stair_display = props.shape == StairsShape::Straight;
    let (sx, cx) = (-f32::from(visual.x).to_radians() * 0.5).sin_cos();
    let (sy, cy) = (-f32::from(visual.y).to_radians() * 0.5).sin_cos();
    visual.orientation = [cy * sx, sy * cx, -sy * sx, cy * cx];
    (props.to_state_id(DISPLAY.0), props.to_state_id(DISPLAY.1))
}

/// Preorient the two stair elements so default face UVs stay aligned to the world.
pub fn straight_display_elements(x: i16, y: i16) -> Value {
    let top = x == 180;
    let step = if top { 0 } else { 8 };
    let slab = 8 - step;
    let (from, to) = match y {
        90 => ([0, step, 8], [16, step + 8, 16]),
        180 => ([0, step, 0], [8, step + 8, 16]),
        270 => ([0, step, 0], [16, step + 8, 8]),
        _ => ([8, step, 0], [16, step + 8, 16]),
    };
    let element = |from: [u8; 3], to: [u8; 3], omit: Option<&str>| {
        let faces: Map<_, _> = [
            ("down", if top { "#top" } else { "#bottom" }),
            ("up", if top { "#bottom" } else { "#top" }),
            ("north", "#side"),
            ("south", "#side"),
            ("west", "#side"),
            ("east", "#side"),
        ]
        .into_iter()
        .filter(|(face, _)| Some(*face) != omit)
        .map(|(face, texture)| (face.to_owned(), json!({"texture": texture})))
        .collect();
        json!({"from": from, "to": to, "faces": faces})
    };
    Value::Array(vec![
        element([0, slab, 0], [16, slab + 8, 16], None),
        element(from, to, Some(if top { "up" } else { "down" })),
    ])
}

fn corner_pair(props: StairsProperties) -> Option<(StairsProperties, StairsProperties)> {
    // A left corner and the right corner one quarter turn counterclockwise
    // have the same model and collision. Vanilla clients can use the right variant.
    let mut left = props;
    let mut right = props;
    match props.shape {
        StairsShape::InnerLeft | StairsShape::OuterLeft => {
            right.shape = if props.shape == StairsShape::InnerLeft {
                StairsShape::InnerRight
            } else {
                StairsShape::OuterRight
            };
            right.facing = right.facing.rotate_counter_clockwise();
        }
        StairsShape::InnerRight | StairsShape::OuterRight => {
            left.shape = if props.shape == StairsShape::InnerRight {
                StairsShape::InnerLeft
            } else {
                StairsShape::OuterLeft
            };
            left.facing = left.facing.rotate_clockwise();
        }
        StairsShape::Straight => return None,
    }
    Some((left, right))
}

pub fn corners_share_models(states: &[RegisteredState]) -> bool {
    let mut models = std::collections::HashMap::new();
    states.iter().all(|state| {
        let props = StairsProperties::from_state_id(state.physical.id);
        let Some((left, _)) = corner_pair(props) else {
            return true;
        };
        let visual = &state.visual;
        let model = (&visual.model, visual.x, visual.y);
        models
            .insert(left.to_index(), model)
            .is_none_or(|other| other == model)
    })
}

pub fn carrier_model(block: &Block, state: BlockStateId) -> Option<(String, i16, i16)> {
    if !StairsProperties::handles_block_id(block.id) {
        return None;
    }
    let props = StairsProperties::from_state_id(state);
    let top = props.half == Half::Top;
    let mut y: i16 = match props.facing {
        HorizontalFacing::East => 0,
        HorizontalFacing::South => 90,
        HorizontalFacing::West => 180,
        HorizontalFacing::North => 270,
    };
    if matches!(props.shape, StairsShape::InnerLeft | StairsShape::OuterLeft) {
        y -= 90;
    }
    if top && props.shape != StairsShape::Straight {
        y += 90;
    }
    let suffix = match props.shape {
        StairsShape::Straight => "",
        StairsShape::InnerLeft | StairsShape::InnerRight => "_inner",
        StairsShape::OuterLeft | StairsShape::OuterRight => "_outer",
    };
    let name = block.name.strip_prefix("waxed_").unwrap_or(block.name);
    Some((
        format!("minecraft:block/{name}{suffix}"),
        if top { 180 } else { 0 },
        y.rem_euclid(360),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stair_carriers_and_displays_keep_the_vanilla_shape() -> Result<(), &'static str> {
        let block = &Block::STONE_STAIRS;
        for state in block.states {
            let props = StairsProperties::from_state_id(state.id);
            if let Some((left, right)) = corner_pair(props) {
                let left = left.to_state_id(block).to_state();
                let right = right.to_state_id(block).to_state();
                assert_eq!(left.collision_shapes, right.collision_shapes);
                assert_eq!(
                    carrier_model(block, left.id),
                    carrier_model(block, right.id)
                );
                continue;
            }

            let (_, x, y) = carrier_model(block, state.id).ok_or("missing stair model")?;
            let model = straight_display_elements(x, y);
            let elements = model.as_array().ok_or("missing stair elements")?;
            let collision: Vec<_> = state.get_block_collision_shapes().collect();
            for x in [0.25, 0.75] {
                for y in [0.25, 0.75] {
                    for z in [0.25, 0.75] {
                        let solid = collision.iter().any(|shape| {
                            x > shape.min.x
                                && x < shape.max.x
                                && y > shape.min.y
                                && y < shape.max.y
                                && z > shape.min.z
                                && z < shape.max.z
                        });
                        let drawn = elements.iter().any(|element| {
                            [x, y, z].into_iter().enumerate().all(|(i, p)| {
                                element["from"][i]
                                    .as_f64()
                                    .is_some_and(|min| min < p * 16.0)
                                    && element["to"][i].as_f64().is_some_and(|max| max > p * 16.0)
                            })
                        });
                        assert_eq!(solid, drawn, "state {} at {x}, {y}, {z}", state.id.as_u16());
                    }
                }
            }
        }
        Ok(())
    }
}

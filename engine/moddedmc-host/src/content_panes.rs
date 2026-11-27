//! Stock carrier states for pane-shaped content.
use serde_json::{Map, Value, json};

use pumpkin_data::{
    Block, BlockStateId,
    block_properties::{BlockProperties, OakFenceLikeProperties},
};

use crate::content::RegisteredState;

pub const CARRIERS: [(&Block, &Block); 3] = [
    (&Block::WAXED_COPPER_BARS, &Block::COPPER_BARS),
    (
        &Block::WAXED_EXPOSED_COPPER_BARS,
        &Block::EXPOSED_COPPER_BARS,
    ),
    (
        &Block::WAXED_WEATHERED_COPPER_BARS,
        &Block::WEATHERED_COPPER_BARS,
    ),
];
pub const DISPLAY_CARRIER: (&Block, &Block) = (
    &Block::WAXED_OXIDIZED_COPPER_BARS,
    &Block::OXIDIZED_COPPER_BARS,
);
pub const SIDES: [&str; 4] = ["north", "east", "south", "west"];

pub fn has_pane_shape(states: &[RegisteredState]) -> bool {
    states.len() == Block::GLASS_PANE.states.len()
        && states.iter().all(|state| {
            state.physical.id.to_block() == &Block::GLASS_PANE
                && state.shape.is_empty()
                && state.properties.len() == 5
                && OakFenceLikeProperties::from_state_id(state.physical.id)
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
    physical: BlockStateId,
    carrier: Option<(&Block, &Block)>,
) -> (BlockStateId, BlockStateId) {
    let (carrier, replacement) = carrier.unwrap_or(DISPLAY_CARRIER);
    let props = OakFenceLikeProperties::from_state_id(physical);
    (props.to_state_id(carrier), props.to_state_id(replacement))
}

pub fn side_mask(state: BlockStateId) -> u8 {
    let props = OakFenceLikeProperties::from_state_id(state);
    u8::from(props.north)
        | (u8::from(props.east) << 1)
        | (u8::from(props.south) << 2)
        | (u8::from(props.west) << 3)
}

pub struct PanePart {
    pub when: Option<(usize, bool)>,
    pub model: String,
    pub y: i32,
}

impl PanePart {
    pub fn read(part: &Value) -> Option<Self> {
        let when = if let Some(condition) = part.get("when") {
            let condition = condition.as_object()?;
            let (side, flag) = condition.iter().next()?;
            if condition.len() != 1 {
                return None;
            }
            let side = SIDES.iter().position(|name| name == side)?;
            let connected = flag.as_str()?.parse::<bool>().ok()?;
            Some((side, connected))
        } else {
            None
        };
        let apply = part.get("apply")?;
        let model = apply.get("model")?.as_str()?.to_owned();
        let y = match apply.get("y") {
            None => 0,
            Some(y) => i32::try_from(y.as_i64()?).ok()?,
        };
        (y.rem_euclid(90) == 0).then_some(Self { when, model, y })
    }

    pub fn visible(&self, mask: usize) -> bool {
        self.when
            .is_none_or(|(index, connected)| (mask & (1 << index) != 0) == connected)
    }
}

/// Preorient the released pane part for an item display, where blockstate rotations
/// are unavailable.
pub fn orient_elements(elements: &[Value], y: i32) -> Option<Vec<Value>> {
    let turns = usize::try_from(y.rem_euclid(360) / 90).ok()?;
    elements
        .iter()
        .map(|element| {
            let mut element = element.clone();
            let corner = |name: &str| -> Option<[f64; 3]> {
                let point = element.get(name)?.as_array()?;
                Some([
                    point.first()?.as_f64()?,
                    point.get(1)?.as_f64()?,
                    point.get(2)?.as_f64()?,
                ])
            };
            let rotate = |mut point: [f64; 3]| {
                for _ in 0..turns {
                    [point[0], point[2]] = [16.0 - point[2], point[0]];
                }
                point
            };
            let a = rotate(corner("from")?);
            let b = rotate(corner("to")?);
            element["from"] = json!(std::array::from_fn::<_, 3, _>(|i| a[i].min(b[i])));
            element["to"] = json!(std::array::from_fn::<_, 3, _>(|i| a[i].max(b[i])));
            let raw = element.get_mut("faces")?.as_object_mut()?;
            let mut faces = Map::new();
            for (direction, mut face) in std::mem::take(raw) {
                face.as_object_mut()?.remove("cullface");
                let out = match direction.as_str() {
                    "up" | "down" => {
                        let uv_rotation = face.get("rotation").and_then(Value::as_i64).unwrap_or(0);
                        let degrees = i64::from(y) * if direction == "up" { 1 } else { -1 };
                        face["rotation"] = json!((uv_rotation + degrees).rem_euclid(360));
                        direction.as_str()
                    }
                    side => {
                        let index = SIDES.iter().position(|name| *name == side)?;
                        SIDES[(index + turns) % SIDES.len()]
                    }
                };
                faces.insert(out.to_owned(), face);
            }
            *raw = faces;
            Some(element)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::orient_elements;
    use serde_json::json;

    #[test]
    fn north_pane_arm_rotates_east_for_item_display() {
        let elements = [json!({
            "from": [7, 0, 0],
            "to": [9, 16, 7],
            "faces": {
                "north": {"texture": "#edge", "cullface": "north"},
                "up": {"texture": "#edge", "uv": [7, 0, 9, 7]},
                "down": {"texture": "#edge", "uv": [7, 0, 9, 7]},
                "east": {"texture": "#pane", "uv": [9, 0, 16, 16]}
            }
        })];
        assert_eq!(
            orient_elements(&elements, 90),
            Some(vec![json!({
                "from": [9.0, 0.0, 7.0],
                "to": [16.0, 16.0, 9.0],
                "faces": {
                    "east": {"texture": "#edge"},
                    "up": {"texture": "#edge", "uv": [7, 0, 9, 7], "rotation": 90},
                    "down": {"texture": "#edge", "uv": [7, 0, 9, 7], "rotation": 270},
                    "south": {"texture": "#pane", "uv": [9, 0, 16, 16]}
                }
            })])
        );
    }
}

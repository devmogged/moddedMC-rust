//! Stock collision and item-model selectors for wall-shaped content.

use pumpkin_data::{
    Block, BlockStateId,
    block_properties::{BlockProperties, EastWall, NorthWall, SouthWall, WallProperties, WestWall},
    data_component_impl::CustomModelDataImpl,
};
use serde_json::{Value, json};

use crate::content::RegisteredState;

pub const SIDES: [&str; 4] = ["north", "east", "south", "west"];
pub const WALL_OVERLAY_SCALE: f32 = 1.001;

pub fn has_wall_overlay(states: &[RegisteredState]) -> bool {
    states.first().is_some_and(|first| {
        first.visual.model.ends_with("_wall_post")
            && states.iter().all(|state| {
                state.visual.model == first.visual.model && state.visual.rotating_model.is_empty()
            })
            && has_wall_shape(states)
    })
}

pub fn has_wall_shape(states: &[RegisteredState]) -> bool {
    states.len() == Block::COBBLESTONE_WALL.states.len()
        && states.iter().all(|state| {
            WallProperties::handles_block_id(state.physical.id.to_block().id)
                && state.shape.is_empty()
                && state.properties.len() == 6
                && WallProperties::from_state_id(state.physical.id)
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

#[derive(Clone, Copy)]
pub struct WallSelector {
    post: bool,
    sides: [&'static str; 4],
}

impl WallSelector {
    pub fn from_state(state: BlockStateId) -> Self {
        let props = WallProperties::from_state_id(state);
        Self {
            post: props.up,
            sides: [
                match props.north {
                    NorthWall::None => "none",
                    NorthWall::Low => "low",
                    NorthWall::Tall => "tall",
                },
                match props.east {
                    EastWall::None => "none",
                    EastWall::Low => "low",
                    EastWall::Tall => "tall",
                },
                match props.south {
                    SouthWall::None => "none",
                    SouthWall::Low => "low",
                    SouthWall::Tall => "tall",
                },
                match props.west {
                    WestWall::None => "none",
                    WestWall::Low => "low",
                    WestWall::Tall => "tall",
                },
            ],
        }
    }

    pub fn component(self) -> CustomModelDataImpl {
        CustomModelDataImpl {
            floats: Vec::new(),
            flags: vec![self.post],
            strings: self.sides.map(str::to_owned).into(),
            colors: Vec::new(),
        }
    }
}

pub fn arm_model(parent: &str, side: usize, top: u8) -> Value {
    // Preorient the boxes so the model's default UVs preserve the multipart model's UV lock.
    let (from, to, inner) = [
        ([5, 0, 0], [11, top, 8], "south"),
        ([8, 0, 5], [16, top, 11], "west"),
        ([5, 0, 8], [11, top, 16], "north"),
        ([0, 0, 5], [8, top, 11], "east"),
    ][side];
    let faces = SIDES
        .into_iter()
        .chain(["up", "down"])
        .filter(|&face| face != inner)
        .map(|face| (face.to_owned(), json!({"texture": "#wall"})))
        .collect::<serde_json::Map<_, _>>();
    json!({
        "parent": parent,
        "elements": [{"from": from, "to": to, "faces": faces}],
    })
}

pub fn item_model(post: &str, arm_prefix: &str) -> Value {
    let model = |id: &str| json!({"type": "minecraft:model", "model": id});
    let empty = json!({"type": "minecraft:empty"});
    let post = json!({
        "type": "minecraft:condition",
        "property": "minecraft:custom_model_data",
        "index": 0,
        "on_true": model(post),
        "on_false": empty,
    });
    let parts: Vec<_> = std::iter::once(post)
        .chain(SIDES.into_iter().enumerate().map(|(index, side)| {
            let cases = ["low", "tall"].map(|shape| {
                json!({
                    "when": shape,
                    "model": model(&format!("{arm_prefix}_{side}_{shape}")),
                })
            });
            json!({
                "type": "minecraft:select",
                "property": "minecraft:custom_model_data",
                "index": index,
                "fallback": empty,
                "cases": cases,
            })
        }))
        .collect();
    json!({"model": {"type": "minecraft:composite", "models": parts}})
}

//! Create 6.0.10 FluidValveBlock and its kinetic pointer.

use moddedmc_api::{
    RegistrationError, Registry,
    content::{Block, Content, ShapeBox, State},
};
use serde::{Deserialize, Serialize};

use crate::{DIRECTIONS, face_axis, relay_orientation};

pub const ID: &str = "create:fluid_valve";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ValveState {
    pub facing: u8,
    pub along_first: bool,
    pub enabled: bool,
    pub wet: bool,
}

impl ValveState {
    pub fn properties(self) -> String {
        let facing = DIRECTIONS[usize::from(self.facing)];
        format!(
            "axis_along_first={},enabled={},facing={facing},waterlogged={}",
            self.along_first, self.enabled, self.wet
        )
    }

    pub fn shaft_axis(self) -> u8 {
        match face_axis(self.facing) {
            0 if self.along_first => 1,
            2 if !self.along_first => 1,
            1 | 2 if self.along_first => 0,
            _ => 2,
        }
    }

    pub fn pipe_axis(self) -> u8 {
        3 - face_axis(self.facing) - self.shaft_axis()
    }

    fn model_rotation(self) -> (i16, i16) {
        match self.facing {
            0 => (270, if self.along_first { 0 } else { 90 }),
            1 => (90, if self.along_first { 0 } else { 90 }),
            2 => (0, 180),
            3 => (0, 0),
            4 => (0, 90),
            _ => (0, 270),
        }
    }

    fn shape(self) -> [ShapeBox; 2] {
        let mut min = [3, 0, 3];
        let mut max = [13, 16, 13];
        let pipe = usize::from(self.pipe_axis());
        min.swap(1, pipe);
        max.swap(1, pipe);
        [
            ShapeBox { min, max },
            ShapeBox {
                min: [2, 2, 2],
                max: [14, 14, 14],
            },
        ]
    }
}

pub fn variants() -> impl Iterator<Item = ValveState> {
    (0..6).flat_map(|facing| {
        [false, true].into_iter().flat_map(move |along_first| {
            [false, true].into_iter().flat_map(move |enabled| {
                [false, true].map(move |wet| ValveState {
                    facing,
                    along_first,
                    enabled,
                    wet,
                })
            })
        })
    })
}

pub fn register(content: Content, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
    let rows: Vec<_> = variants()
        .map(|valve| {
            let style = if valve.shaft_axis() == 1 {
                "vertical"
            } else {
                "horizontal"
            };
            let state = if valve.enabled { "open" } else { "closed" };
            (
                valve,
                valve.properties(),
                format!("create:block/fluid_valve/block_{style}_{state}"),
                format!("minecraft:iron_chain[axis=y,waterlogged={}]", valve.wet),
                valve.shape(),
            )
        })
        .collect();
    let states: Vec<_> = rows
        .iter()
        .map(|(valve, properties, model, physical, shape)| {
            let (x, y) = valve.model_rotation();
            State {
                properties,
                physical,
                model,
                rotating_model: "create:block/shaft",
                x,
                y,
                orientation: relay_orientation(valve.shaft_axis()),
                shape,
            }
        })
        .collect();
    content.add_block(
        registry,
        &Block {
            id: ID,
            display_name: "Fluid Valve",
            entity: ID,
            item_model: "create:item/fluid_valve",
            states: &states,
        },
        Some(crate::placed_state),
    )
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct Pointer {
    value: f32,
    target: f32,
    speed: f32,
}

impl Pointer {
    pub fn advance(&mut self, rpm: f32) -> Option<bool> {
        if !self.value.is_finite() {
            self.value = 0.0;
        }
        self.target = f32::from(u8::from(rpm > 0.0));
        self.speed = (rpm.abs() / 16.0 / 20.0).clamp(0.0, 1.0);
        self.value = if self.target == 1.0 {
            (self.value + self.speed).min(1.0)
        } else {
            (self.value - self.speed).max(0.0)
        };
        match self.value {
            0.0 => Some(false),
            1.0 => Some(true),
            _ => None,
        }
    }
}

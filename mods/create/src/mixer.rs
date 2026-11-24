//! Create 6.0.10's MechanicalMixerBlock and the basin mixing cycle.

use moddedmc_api::{
    Pos, RegistrationError, Registry,
    block_displays::BlockDisplay,
    content::{self, Block, Content, ShapeBox, State, World},
    items::Item,
};
use serde::{Deserialize, Serialize};

use crate::{
    BLOCK_DISPLAYS, CONTENT, ITEMS, STONE, StateIds, basin,
    kinetics::{Kind, Kinetics, offset},
    processing_recipes::RecipeBook,
};

pub const ID: &str = "create:mechanical_mixer";
const MINIMUM_RPM: f32 = 30.0;
const PROCESSING_STROKE: u8 = 20;
const STROKE_END: u8 = 40;
const DISPLAY_MODELS: [(&str, &str, &str); 2] = [
    (
        "create:mechanical_mixer_pole",
        "Mixer Pole",
        "create:block/mechanical_mixer/pole",
    ),
    (
        "create:mechanical_mixer_head",
        "Mixer Head",
        "create:block/mechanical_mixer/head",
    ),
];

pub fn register(content: Content, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
    content.add_block(
        registry,
        &Block {
            id: ID,
            display_name: "Mechanical Mixer",
            item_model: "create:item/mechanical_mixer",
            entity: ID,
            states: &[State {
                properties: "",
                physical: STONE,
                model: "create:block/mechanical_mixer/block",
                rotating_model: "create:block/cogwheel_shaftless",
                x: 0,
                y: 0,
                orientation: [0.0, 0.0, 0.0, 1.0],
                shape: &[
                    ShapeBox {
                        min: [0, 0, 0],
                        max: [4, 16, 16],
                    },
                    ShapeBox {
                        min: [12, 0, 0],
                        max: [16, 16, 16],
                    },
                    ShapeBox {
                        min: [4, 0, 0],
                        max: [12, 16, 4],
                    },
                    ShapeBox {
                        min: [4, 0, 12],
                        max: [12, 16, 16],
                    },
                ],
            }],
        },
        None,
    )?;
    for (id, display_name, model) in DISPLAY_MODELS {
        content.add_block(
            registry,
            &Block {
                id,
                display_name,
                entity: "",
                item_model: "",
                states: &[State {
                    properties: "",
                    physical: STONE,
                    model,
                    rotating_model: "",
                    x: 0,
                    y: 0,
                    orientation: [0.0, 0.0, 0.0, 1.0],
                    shape: &[],
                }],
            },
            None,
        )?;
    }
    ITEMS
        .get()
        .ok_or(RegistrationError::InvalidDefinition)?
        .add_item(
            registry,
            &Item {
                id: "create:whisk",
                display_name: "Whisk",
                carrier: "minecraft:stick",
                model: "create:item/whisk",
            },
        )
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct MixerData {
    #[serde(default)]
    ticks: u8,
    #[serde(default)]
    processing_ticks: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    recipe: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Mixer {
    pub data: MixerData,
    displays: [i32; 2],
    angle: f32,
    shown: Option<(f32, f32)>,
}

impl Mixer {
    pub fn load(data: MixerData) -> Self {
        Self {
            data,
            ..Self::default()
        }
    }

    pub fn clear(&mut self, world: World<'_>) {
        if let Some(displays) = BLOCK_DISPLAYS.get() {
            for id in self.displays.iter_mut().filter(|id| **id != 0) {
                displays.remove(world, std::mem::take(id));
            }
        }
    }

    fn show(&mut self, world: World<'_>, pos: Pos, rpm: f32) {
        let Some((displays, content)) = BLOCK_DISPLAYS.get().zip(CONTENT.get()) else {
            return;
        };
        let active = self.data.recipe.is_some();
        let tick = self.data.ticks;
        let head_speed = if !active {
            rpm / 2.0
        } else if (15..=PROCESSING_STROKE).contains(&tick) {
            rpm * 2.0
        } else {
            rpm
        };
        self.angle = (self.angle + head_speed * std::f32::consts::TAU / 1200.0)
            .rem_euclid(std::f32::consts::TAU);
        let stroke =
            f32::from(tick.min(STROKE_END.saturating_sub(tick))) / f32::from(PROCESSING_STROKE);
        let down = 7.0 / 16.0
            + if active {
                (1.0 - (stroke * std::f32::consts::PI).cos()) / 2.0
            } else {
                0.0
            };
        let last = self.shown;
        let position = [pos.x, pos.y, pos.z].map(|coord| f64::from(coord) + 0.5);
        for (index, ((model, _, _), id)) in DISPLAY_MODELS
            .into_iter()
            .zip(&mut self.displays)
            .enumerate()
        {
            let head = index == 1;
            if *id != 0
                && last.is_some_and(|(y, angle)| y == down && (!head || angle == self.angle))
            {
                continue;
            }
            let angle = if head { self.angle } else { 0.0 };
            let (sin, cos) = angle.sin_cos();
            let shown = BlockDisplay {
                position,
                translation: [-(cos + sin) * 0.5, -0.5 - down, -(cos - sin) * 0.5],
                rotation: [0.0, (angle / 2.0).sin(), 0.0, (angle / 2.0).cos()],
                scale: [1.0; 3],
                transform_interpolation: 1,
                position_interpolation: 0,
                brightness: -1,
            };
            if (*id == 0 || !displays.update(world, *id, &shown))
                && let Some(state) = content.state(model)
            {
                *id = displays.spawn(world, state, &shown).unwrap_or_default();
            }
        }
        self.shown = Some((down, self.angle));
    }
}

pub fn tick(world: World<'_>, time: u64, states: &StateIds, kinetics: &mut Kinetics) {
    let positions = kinetics.positions(|kind| kind == Kind::Mixer);
    if positions.is_empty() {
        return;
    }
    let Some(book) = crate::processing_recipes::current() else {
        return;
    };
    for pos in positions {
        let Some(be) = kinetics.get_mut(pos) else {
            continue;
        };
        let rpm = be.get_speed();
        let speed = rpm.abs();
        let mixer = be.mixer.get_or_insert_with(Default::default);
        if mixer.data.recipe.is_some() || (speed >= MINIMUM_RPM && time.is_multiple_of(5)) {
            let before = mixer.data.clone();
            mixer.data.advance(world, pos, states, speed, &book);
            be.dirty |= mixer.data != before;
        }
        mixer.show(world, pos, rpm);
    }
}

impl MixerData {
    fn advance(
        &mut self,
        world: World<'_>,
        pos: Pos,
        states: &StateIds,
        speed: f32,
        book: &RecipeBook,
    ) {
        let basin_pos = offset(pos, [0, -2, 0]);
        let mut state = [content::UNLOADED_STATE];
        if !world.read(&[basin_pos], &mut state) || !basin::is_basin(states, state[0]) {
            *self = Self::default();
            return;
        }
        let Some(recipe) = self.recipe.as_deref() else {
            let heat = crate::blaze_burner::heat_at(world, offset(basin_pos, [0, -1, 0]));
            if let Some(recipe) = basin::Basin::load(world, basin_pos)
                .and_then(|basin| book.matching_mixing(&basin, heat))
            {
                self.recipe = Some(recipe.to_owned());
            }
            return;
        };
        if self.ticks >= STROKE_END {
            *self = Self::default();
            return;
        }
        if speed < MINIMUM_RPM && self.ticks <= PROCESSING_STROKE {
            self.ticks = (STROKE_END - self.ticks).max(PROCESSING_STROKE + 1);
            self.processing_ticks = 0;
            return;
        }
        if self.ticks != PROCESSING_STROKE {
            self.ticks += 1;
            return;
        }
        let Some(duration) = book.mixing_duration(recipe) else {
            self.ticks += 1;
            return;
        };
        if self.processing_ticks == 0 {
            let speed_factor = if duration > 0 {
                duration as f32 / 100.0
            } else {
                1.0
            };
            self.processing_ticks = ((512.0 / speed) as u32)
                .max(1)
                .ilog2()
                .saturating_mul((15.0 * speed_factor).ceil() as u32)
                .saturating_add(1);
            return;
        }
        self.processing_ticks -= 1;
        if self.processing_ticks != 0 {
            return;
        }
        self.ticks += 1;
        let heat = crate::blaze_burner::heat_at(world, offset(basin_pos, [0, -1, 0]));
        if let Some(mut basin) = basin::Basin::load(world, basin_pos)
            .and_then(|basin| book.apply_mixing(recipe, &basin, heat))
            && basin.save(world, basin_pos)
            && book.can_continue_mixing(recipe, &basin, heat)
        {
            self.ticks = PROCESSING_STROKE;
        }
    }
}

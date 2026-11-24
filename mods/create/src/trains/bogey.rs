//! Standard bogeys placed from Train Casing in a station's assembly area.
use super::{runtime, targeting::state_at, track::TrackShape};
use moddedmc_api::{
    Pos, RegistrationError, Registry,
    content::{self, Block, BlockUpdate, Content, ShapeBox, State, World},
};
use std::sync::OnceLock;

const IDS: [&str; 2] = ["create:small_bogey", "create:large_bogey"];
const PROPERTIES: [&str; 4] = [
    "axis=x,waterlogged=false",
    "axis=x,waterlogged=true",
    "axis=z,waterlogged=false",
    "axis=z,waterlogged=true",
];
const MAX_ASSEMBLY_LENGTH: i32 = 128;
const MAX_BOGEY_COUNT: usize = 20;
static STATES: OnceLock<[[Option<u32>; 4]; 2]> = OnceLock::new();

pub(super) fn register(
    content: Content,
    registry: &mut Registry<'_>,
) -> Result<(), RegistrationError> {
    let frames = [
        "create:block/track/bogey/bogey_frame",
        "create:block/track/bogey/bogey_drive",
    ];
    for ((id, display_name), frame) in IDS
        .into_iter()
        .zip(["Small Bogey", "Large Bogey"])
        .zip(frames)
    {
        content.add_block(
            registry,
            &Block {
                id,
                display_name,
                entity: id,
                item_model: "create:item/railway_casing",
                states: &std::array::from_fn::<_, 4, _>(|i| State {
                    properties: PROPERTIES[i],
                    physical: if i % 2 == 0 {
                        "minecraft:cut_copper_slab[type=bottom,waterlogged=false]"
                    } else {
                        "minecraft:cut_copper_slab[type=bottom,waterlogged=true]"
                    },
                    model: "create:block/track/bogey/top",
                    rotating_model: frame,
                    x: 0,
                    y: if i < 2 { 90 } else { 0 },
                    orientation: if i < 2 {
                        [
                            0.0,
                            std::f32::consts::FRAC_1_SQRT_2,
                            0.0,
                            std::f32::consts::FRAC_1_SQRT_2,
                        ]
                    } else {
                        [0.0, 0.0, 0.0, 1.0]
                    },
                    shape: &[ShapeBox {
                        min: [0; 3],
                        max: [16; 3],
                    }],
                }),
            },
            None,
        )?;
    }
    Ok(())
}

fn states(content: Content) -> &'static [[Option<u32>; 4]; 2] {
    STATES.get_or_init(|| {
        IDS.map(|id| PROPERTIES.map(|props| content.state(&format!("{id}[{props}]"))))
    })
}

pub(super) struct AssemblyTrack {
    pub anchor: Pos,
    pub step: [i32; 3],
    pub shape: TrackShape,
    pub waterlogged: bool,
}

pub(super) enum Placement {
    None,
    Place,
    Resized,
    Rejected,
}

impl AssemblyTrack {
    fn rail(&self, offset: i32) -> Pos {
        let [x, y, z] = self.step.map(|step| step * (offset + 1));
        crate::offset(self.anchor, [x, y, z])
    }

    pub(super) fn place(
        &self,
        world: World<'_>,
        content: Content,
        clicked: Pos,
        has_casing: bool,
    ) -> Placement {
        if clicked.y != self.anchor.y {
            return Placement::None;
        }
        let offset = (clicked.x - self.anchor.x) * self.step[0]
            + (clicked.z - self.anchor.z) * self.step[2]
            - 1;
        if !(0..MAX_ASSEMBLY_LENGTH - 1).contains(&offset) || self.rail(offset) != clicked {
            return Placement::None;
        }
        let states = states(content);
        let mut nearby = Vec::new();
        for i in 0..MAX_ASSEMBLY_LENGTH - 1 {
            let rail = self.rail(i);
            let Some(state) = state_at(world, rail) else {
                if i <= offset + 1 {
                    return Placement::Rejected;
                }
                break;
            };
            let aligned = runtime::track_state(content, state)
                .is_some_and(|(shape, _)| shape == self.shape)
                && runtime::track_waterlogged(content, state) == self.waterlogged;
            if !aligned {
                if i <= offset + 1 {
                    return Placement::None;
                }
                break;
            }
            let bogey = crate::offset(rail, [0, 1, 0]);
            if let Some(state) = state_at(world, bogey)
                && let Some((size, row)) = states.iter().enumerate().find_map(|(size, rows)| {
                    rows.iter()
                        .position(|&id| id == Some(state))
                        .map(|row| (size, row))
                })
            {
                nearby.push((i, bogey, size, row));
            }
        }

        let update = |position, state| {
            world.write(&[BlockUpdate {
                position,
                state,
                flags: content::UPDATE_ALL,
            }]) == 1
        };
        if let Some((_, bogey, size, row)) = nearby
            .iter()
            .copied()
            .find(|(i, _, _, _)| (i - offset).abs() <= 1)
        {
            return if states[1 - size][row].is_some_and(|state| update(bogey, state)) {
                Placement::Resized
            } else {
                Placement::Rejected
            };
        }
        if !has_casing
            || nearby.len() >= MAX_BOGEY_COUNT
            || (offset != 0 && (offset < 3 || nearby.is_empty()))
            || nearby.iter().any(|(i, _, _, _)| (i - offset).abs() <= 2)
        {
            return Placement::Rejected;
        }
        let position = crate::offset(clicked, [0, 1, 0]);
        let Some(existing) = state_at(world, position) else {
            return Placement::Rejected;
        };
        if !world
            .destroy_speed(position)
            .is_some_and(|speed| speed >= 0.0)
        {
            return Placement::Rejected;
        }
        let wet = world.fluid_id(clicked).as_deref() == Some("minecraft:water");
        let row = if self.step[0] == 0 { 2 } else { 0 } + usize::from(wet);
        let Some(state) = states[0][row] else {
            return Placement::Rejected;
        };
        if Some(existing) != content.state("minecraft:air") && !world.destroy(position, true) {
            return Placement::Rejected;
        }
        if update(position, state) {
            Placement::Place
        } else {
            Placement::Rejected
        }
    }
}

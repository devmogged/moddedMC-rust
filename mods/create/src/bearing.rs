//! Create 6.0.10's MechanicalBearingBlockEntity and BearingContraption.

use moddedmc_api::{
    Pos, RegistrationError, Registry,
    content::{self, Block, BlockUpdate, Content, State, World},
    interaction::{BlockUseResult, UseBlock},
};
use serde::{Deserialize, Serialize};

use crate::{
    DIRECTIONS, STONE, StateIds,
    contraption::{
        BlockMovementChecks, Contraption, ContraptionDisplays, MAX_ASSEMBLY_DATA_BYTES,
        StructureTransform,
    },
    kinetics::{Kind, Kinetics, Level, face_axis, offset},
};

const ID: &str = "create:mechanical_bearing";
const WINDMILL: &str = "create:windmill_bearing";
const MINIMUM_SAIL_COUNT: i32 = 8;
const WINDMILL_SAILS_PER_RPM: i32 = 8;
const ROTATIONS: [(i16, i16); 6] = [(180, 0), (0, 0), (90, 0), (90, 180), (90, 270), (90, 90)];

pub fn register(content: Content, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
    let properties = DIRECTIONS.map(|face| format!("facing={face}"));
    for (id, display_name, model, item_model) in [
        (
            ID,
            "Mechanical Bearing",
            "create:block/mechanical_bearing",
            "create:item/mechanical_bearing",
        ),
        (
            WINDMILL,
            "Windmill Bearing",
            "create:block/windmill_bearing",
            "create:item/windmill_bearing",
        ),
    ] {
        let states = std::array::from_fn::<_, 6, _>(|i| {
            let (x, y) = ROTATIONS[i];
            State {
                properties: &properties[i],
                physical: STONE,
                model,
                rotating_model: "",
                x,
                y,
                orientation: crate::relay_orientation(face_axis(i as u8)),
                shape: &[],
            }
        });
        content.add_block(
            registry,
            &Block {
                id,
                display_name,
                entity: id,
                item_model,
                states: &states,
            },
            Some(crate::placed_state),
        )?;
    }
    Ok(())
}

pub fn add_states(content: Content, states: &mut StateIds) {
    for (face, direction) in DIRECTIONS.iter().enumerate() {
        let properties = format!("facing={direction}");
        states.add(content, ID, &properties, Some(Kind::Bearing(face as u8)));
        states.add(
            content,
            WINDMILL,
            &properties,
            Some(Kind::Windmill(face as u8)),
        );
    }
}

#[derive(Clone, Debug, Default)]
pub struct MechanicalBearing {
    pub data: BearingData,
    displays: ContraptionDisplays,
    assemble_next_tick: bool,
    previous_speed: f32,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct BearingData {
    running: bool,
    angle: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    contraption: Option<Contraption>,
}

impl MechanicalBearing {
    pub fn get_generated_speed(&self) -> i32 {
        self.data.contraption.as_ref().map_or(0, |contraption| {
            (contraption.sails / WINDMILL_SAILS_PER_RPM).clamp(1, 16)
        })
    }

    fn assemble(
        &mut self,
        world: World<'_>,
        blocks: &BlockMovementChecks,
        pos: Pos,
        facing: u8,
        windmill: bool,
    ) -> bool {
        let (Some(contraption), Some(air)) = (
            blocks.scan(world, pos, facing),
            blocks.state("minecraft:air"),
        ) else {
            return false;
        };
        if windmill && contraption.sails < MINIMUM_SAIL_COUNT {
            return false;
        }
        let Some(origin) = contraption.anchor() else {
            return false;
        };
        let remove: Vec<_> = contraption
            .blocks
            .block_list
            .iter()
            .map(|block| BlockUpdate {
                position: offset(origin, block.local_pos()),
                state: air,
                flags: content::UPDATE_MOVED | content::UPDATE_CLIENTS,
            })
            .collect();
        let saved = BearingData {
            running: true,
            angle: 0.0,
            contraption: Some(contraption),
        };
        if !fastnbt::to_bytes(&saved).is_ok_and(|bytes| bytes.len() <= MAX_ASSEMBLY_DATA_BYTES)
            || world.write(&remove) != remove.len()
        {
            return false;
        }
        self.data = saved;
        true
    }

    fn disassemble(&mut self, world: World<'_>, blocks: &BlockMovementChecks, axis: u8) -> bool {
        let Some(contraption) = &self.data.contraption else {
            return false;
        };
        let Some(anchor) = contraption.anchor() else {
            return false;
        };
        let transform = StructureTransform {
            anchor,
            axis,
            angle: self.data.angle,
        };
        if !contraption.blocks.place(world, blocks, transform, None) {
            return false;
        }
        self.clear_displays(world);
        self.data = BearingData::default();
        self.assemble_next_tick = false;
        true
    }

    pub fn clear_displays(&mut self, world: World<'_>) {
        self.displays.clear(world);
    }

    fn show(&mut self, world: World<'_>, axis: u8) {
        let Some(contraption) = &self.data.contraption else {
            return;
        };
        if let Some(anchor) = contraption.anchor() {
            self.displays.show(
                world,
                &contraption.blocks,
                StructureTransform {
                    anchor,
                    axis,
                    angle: self.data.angle,
                },
                [0.0; 3],
            );
        }
    }

    fn tick(
        &mut self,
        world: World<'_>,
        blocks: &BlockMovementChecks,
        pos: Pos,
        kind: Kind,
        speed: f32,
    ) -> bool {
        let (facing, windmill) = match kind {
            Kind::Bearing(facing) => (facing, false),
            Kind::Windmill(facing) => (facing, true),
            _ => return false,
        };
        let previous_speed = std::mem::replace(&mut self.previous_speed, speed);
        let mut dirty = false;
        if previous_speed != speed {
            self.assemble_next_tick |= !windmill;
            if previous_speed != 0.0
                && previous_speed.signum() != speed.signum()
                && self.data.running
            {
                self.data.angle = (self.data.angle + 0.5).floor();
                dirty = true;
            }
        }
        if std::mem::take(&mut self.assemble_next_tick)
            && !self.data.running
            && (windmill || speed != 0.0)
        {
            dirty |= self.assemble(world, blocks, pos, facing, windmill);
        }
        if self.data.running {
            let axis = face_axis(facing);
            if !windmill && speed == 0.0 && self.disassemble(world, blocks, axis) {
                return true;
            }
            let speed = if windmill && speed != 0.0 {
                self.get_generated_speed() as f32
            } else {
                speed
            };
            self.data.angle = (self.data.angle + speed * 360.0 / 60.0 / 20.0) % 360.0;
            self.show(world, axis);
            dirty |= speed != 0.0;
        }
        dirty
    }
}

pub fn use_item_on(
    kinetics: &mut Kinetics,
    checks: &BlockMovementChecks,
    context: &mut UseBlock<'_>,
) -> BlockUseResult {
    if context.sneaking {
        return BlockUseResult::Fail;
    }
    if context.stack.read().is_some() {
        return BlockUseResult::TryEmptyHand;
    }
    let Some(be) = kinetics.get_mut(context.position) else {
        return BlockUseResult::Pass;
    };
    let Some(bearing) = &mut be.bearing else {
        return BlockUseResult::Pass;
    };
    if bearing.data.running {
        be.dirty |= bearing.disassemble(context.world, checks, be.kind.axis());
    } else {
        bearing.assemble_next_tick = true;
    }
    BlockUseResult::Success
}

pub fn tick(
    world: World<'_>,
    checks: &BlockMovementChecks,
    kinetics: &mut Kinetics,
    level: &mut dyn Level,
) {
    for (pos, axis, mut bearing) in kinetics.dropped_bearings.drain(..) {
        if !bearing.disassemble(world, checks, axis) {
            bearing.clear_displays(world);
            if let Some(contraption) = bearing.data.contraption {
                contraption.blocks.drop_contents(world, pos, |item| item);
            }
        }
    }
    for pos in kinetics.positions(Kind::is_bearing) {
        let Some(be) = kinetics.get_mut(pos) else {
            continue;
        };
        let speed = be.get_speed();
        if let Some(bearing) = &mut be.bearing {
            be.dirty |= bearing.tick(world, checks, pos, be.kind, speed);
            if matches!(be.kind, Kind::Windmill(_)) {
                let generated = bearing.get_generated_speed();
                if be.generated != generated {
                    kinetics.set_generated(level, pos, generated);
                }
            }
        }
    }
}

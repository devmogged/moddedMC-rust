//! Create 6.0.10's MechanicalBearingBlockEntity and BearingContraption.

use fastnbt::Value;
use moddedmc_api::{
    Pos, RegistrationError, Registry,
    block_displays::BlockDisplays,
    content::{self, Block, BlockUpdate, Content, State, UNLOADED_STATE, World},
    interaction::{BlockUseResult, UseBlock},
};
use serde::{Deserialize, Serialize};

use crate::{
    BLOCK_DISPLAYS, DIRECTIONS, STACKS, STONE, StateIds,
    contraption::{BlockMovementChecks, Contraption, StructureTransform},
    kinetics::{Kind, Kinetics, face_axis, offset},
};

pub const ID: &str = "create:mechanical_bearing";
const ROTATIONS: [(i16, i16); 6] = [(180, 0), (0, 0), (90, 0), (90, 180), (90, 270), (90, 90)];

pub fn register(content: Content, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
    let properties = DIRECTIONS.map(|face| format!("facing={face}"));
    let states = std::array::from_fn::<_, 6, _>(|i| {
        let (x, y) = ROTATIONS[i];
        State {
            properties: &properties[i],
            physical: STONE,
            model: "create:block/mechanical_bearing",
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
            id: ID,
            display_name: "Mechanical Bearing",
            entity: ID,
            item_model: "create:item/mechanical_bearing",
            states: &states,
        },
        Some(crate::placed_state),
    )
}

pub fn add_states(content: Content, states: &mut StateIds) {
    for (face, direction) in DIRECTIONS.iter().enumerate() {
        states.add(
            content,
            ID,
            &format!("facing={direction}"),
            Some(Kind::Bearing(face as u8)),
        );
    }
}

#[derive(Clone, Debug, Default)]
pub struct MechanicalBearing {
    pub data: BearingData,
    displays: Vec<i32>,
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
    fn assemble(
        &mut self,
        world: World<'_>,
        blocks: &BlockMovementChecks,
        pos: Pos,
        facing: u8,
    ) -> bool {
        let (Some(contraption), Some(air)) = (
            blocks.scan(world, pos, facing),
            blocks.state("minecraft:air"),
        ) else {
            return false;
        };
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
                flags: content::UPDATE_ALL,
            })
            .collect();
        if world.write(&remove) != remove.len() {
            return false;
        }
        self.data.contraption = Some(contraption);
        self.data.angle = 0.0;
        self.data.running = true;
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
        let Some(place) = contraption
            .blocks
            .block_list
            .iter()
            .map(|block| {
                let entry = contraption.blocks.palette.get(block.palette_index())?;
                Some(BlockUpdate {
                    position: transform.apply(block.local_pos()),
                    state: blocks.state(&entry.name)?,
                    flags: content::UPDATE_ALL,
                })
            })
            .collect::<Option<Vec<_>>>()
        else {
            return false;
        };
        let positions: Vec<_> = place.iter().map(|update| update.position).collect();
        let mut destination = vec![UNLOADED_STATE; place.len()];
        world.read(&positions, &mut destination);
        // shortcut: preserve the assembly when obstructed until replacement and drop rules are ported.
        if !destination.into_iter().all(|state| blocks.is_air(state))
            || world.write(&place) != place.len()
        {
            return false;
        }
        self.clear_displays(world);
        self.data = BearingData::default();
        self.assemble_next_tick = false;
        true
    }

    pub fn clear_displays(&mut self, world: World<'_>) {
        if let Some(displays) = BLOCK_DISPLAYS.get() {
            for id in self.displays.drain(..).filter(|&id| id != 0) {
                displays.remove(world, id);
            }
        }
    }

    fn show(
        &mut self,
        world: World<'_>,
        displays: BlockDisplays,
        blocks: &BlockMovementChecks,
        axis: u8,
    ) {
        let Some(contraption) = &self.data.contraption else {
            return;
        };
        let Some(anchor) = contraption.anchor() else {
            return;
        };
        let transform = StructureTransform {
            anchor,
            axis,
            angle: self.data.angle,
        };
        self.displays.resize(contraption.blocks.block_list.len(), 0);
        for (block, id) in contraption.blocks.block_list.iter().zip(&mut self.displays) {
            let shown = transform.block_display(block.local_pos());
            if (*id == 0 || !displays.update(world, *id, &shown))
                && let Some(state) = contraption
                    .blocks
                    .palette
                    .get(block.palette_index())
                    .and_then(|entry| blocks.state(&entry.name))
            {
                *id = displays.spawn(world, state, &shown).unwrap_or(0);
            }
        }
    }

    fn tick(
        &mut self,
        world: World<'_>,
        blocks: &BlockMovementChecks,
        pos: Pos,
        facing: u8,
        speed: f32,
    ) -> bool {
        let previous_speed = std::mem::replace(&mut self.previous_speed, speed);
        let mut dirty = false;
        if previous_speed != speed {
            self.assemble_next_tick = true;
            if previous_speed != 0.0
                && previous_speed.signum() != speed.signum()
                && self.data.running
            {
                self.data.angle = (self.data.angle + 0.5).floor();
                dirty = true;
            }
        }
        if std::mem::take(&mut self.assemble_next_tick) && !self.data.running && speed != 0.0 {
            dirty |= self.assemble(world, blocks, pos, facing);
        }
        if self.data.running {
            let axis = face_axis(facing);
            if speed == 0.0 && self.disassemble(world, blocks, axis) {
                return true;
            }
            self.data.angle = (self.data.angle + speed * 360.0 / 60.0 / 20.0) % 360.0;
            if let Some(&displays) = BLOCK_DISPLAYS.get() {
                self.show(world, displays, blocks, axis);
            }
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

pub fn tick(world: World<'_>, checks: &BlockMovementChecks, kinetics: &mut Kinetics) {
    for (pos, axis, mut bearing) in kinetics.dropped_bearings.drain(..) {
        if !bearing.disassemble(world, checks, axis) {
            bearing.clear_displays(world);
            if let (Some(contraption), Some(stacks)) = (bearing.data.contraption, STACKS.get()) {
                for block in contraption.blocks.block_list {
                    if let Some(entry) = contraption.blocks.palette.get(block.palette_index()) {
                        let item = Value::Compound(std::collections::HashMap::from([
                            ("id".into(), Value::String(entry.name.clone())),
                            ("count".into(), Value::Int(1)),
                        ]));
                        if let Ok(bytes) = fastnbt::to_bytes(&item) {
                            stacks.spawn(
                                world,
                                [pos.x, pos.y, pos.z].map(|n| f64::from(n) + 0.5),
                                [0.0, 0.1, 0.0],
                                &bytes,
                            );
                        }
                    }
                }
            }
        }
    }
    for pos in kinetics.positions(|kind| matches!(kind, Kind::Bearing(_))) {
        let Some(be) = kinetics.get_mut(pos) else {
            continue;
        };
        let Kind::Bearing(facing) = be.kind else {
            continue;
        };
        let speed = be.get_speed();
        if let Some(bearing) = &mut be.bearing {
            be.dirty |= bearing.tick(world, checks, pos, facing, speed);
        }
    }
}

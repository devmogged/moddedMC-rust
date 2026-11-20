//! Create 6.0.10's MechanicalPistonBlockEntity and PistonContraption.

use std::collections::{HashSet, VecDeque};

use moddedmc_api::{
    Pos, RegistrationError, Registry,
    content::{
        self, Block, BlockUpdate, Content, Placement, ShapeBox, State, UNLOADED_STATE, World,
    },
    interaction::{BlockUseResult, UseBlock},
};
use serde::{Deserialize, Serialize};

use crate::{
    CONTENT, DIRECTIONS, STONE, StateIds,
    contraption::{
        BlockMovement, BlockMovementChecks, Blocks, ContraptionDisplays, MAX_ASSEMBLY_DATA_BYTES,
        PaletteEntry, StructureTransform,
    },
    kinetics::{FACES, Kind, Kinetics, face_axis, offset, positive},
};

const ID: &str = "create:mechanical_piston";
const STICKY: &str = "create:sticky_mechanical_piston";
pub const POLE: &str = "create:piston_extension_pole";
const HEAD: &str = "create:mechanical_piston_head";
const MAX_PISTON_POLES: i32 = 64;
const MAX_CHASSIS_RANGE: i32 = 16;
const RPM_PER_BLOCK_PER_TICK: f32 = 512.0;
const MAX_MOVEMENT_SPEED: f32 = 0.49;
const ROTATIONS: [(i16, i16); 6] = [(180, 0), (0, 0), (90, 0), (90, 180), (90, 270), (90, 90)];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PistonBlock {
    facing: u8,
    along_first: bool,
    pub sticky: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PistonState {
    Retracted,
    Moving,
    Extended,
}

impl PistonState {
    const ALL: [Self; 3] = [Self::Retracted, Self::Moving, Self::Extended];

    const fn name(self) -> &'static str {
        match self {
            Self::Retracted => "retracted",
            Self::Moving => "moving",
            Self::Extended => "extended",
        }
    }
}

impl PistonBlock {
    const fn id(self) -> &'static str {
        if self.sticky { STICKY } else { ID }
    }

    pub const fn shaft_axis(self) -> u8 {
        match face_axis(self.facing) {
            0 if self.along_first => 1,
            0 => 2,
            _ if self.along_first => 0,
            1 => 2,
            _ => 1,
        }
    }

    fn properties(self, state: PistonState) -> String {
        format!(
            "axis_along_first={},facing={},state={}",
            self.along_first,
            DIRECTIONS[usize::from(self.facing)],
            state.name()
        )
    }

    fn state(self, state: PistonState) -> Option<u32> {
        CONTENT
            .get()?
            .state(&format!("{}[{}]", self.id(), self.properties(state)))
    }

    fn movement_speed(self, rpm: f32) -> f32 {
        let movement =
            (rpm / RPM_PER_BLOCK_PER_TICK).clamp(-MAX_MOVEMENT_SPEED, MAX_MOVEMENT_SPEED);
        if positive(self.facing) ^ (face_axis(self.facing) == 2) {
            -movement
        } else {
            movement
        }
    }

    fn collision_offset(self, offset: f32, extending: bool) -> i32 {
        let axis_sign = if positive(self.facing) { 1 } else { -1 };
        let world_offset = (offset * axis_sign as f32).floor() as i32;
        // ContraptionCollider checks the next grid cell when moving in a positive direction.
        (world_offset + i32::from(extending == positive(self.facing))) * axis_sign
    }

    fn at(self, pos: Pos, distance: i32) -> Pos {
        offset(
            pos,
            FACES[usize::from(self.facing)].map(|axis| axis * distance),
        )
    }

    fn variants(sticky: bool) -> impl Iterator<Item = (Self, PistonState)> {
        (0..6).flat_map(move |facing| {
            [false, true].into_iter().flat_map(move |along_first| {
                PistonState::ALL.into_iter().map(move |state| {
                    (
                        Self {
                            facing,
                            along_first,
                            sticky,
                        },
                        state,
                    )
                })
            })
        })
    }

    fn head(self) -> PaletteEntry {
        part(HEAD, self.facing, self.sticky, false)
    }
}

fn part(id: &str, facing: u8, sticky: bool, wet: bool) -> PaletteEntry {
    let face = DIRECTIONS[usize::from(facing)];
    let piston_type = if sticky { "sticky" } else { "normal" };
    let properties = if id == HEAD {
        format!(",type={piston_type}")
    } else {
        String::new()
    };
    PaletteEntry::parse(&format!(
        "{id}[facing={face}{properties},waterlogged={wet}]"
    ))
}

fn axial_box(facing: u8, from: i16, to: i16, min: i16, max: i16) -> ShapeBox {
    let axis = usize::from(face_axis(facing));
    let mut shape = ShapeBox {
        min: [min; 3],
        max: [max; 3],
    };
    (shape.min[axis], shape.max[axis]) = if positive(facing) {
        (from, to)
    } else {
        (16 - to, 16 - from)
    };
    shape
}

pub fn register(content: Content, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
    for (id, name, item_model, sticky) in [
        (
            ID,
            "Mechanical Piston",
            "create:item/mechanical_piston",
            false,
        ),
        (
            STICKY,
            "Sticky Mechanical Piston",
            "create:item/sticky_mechanical_piston",
            true,
        ),
    ] {
        let definitions: Vec<_> = PistonBlock::variants(sticky)
            .map(|(piston, state)| {
                let model = match state {
                    PistonState::Retracted if sticky => "sticky",
                    PistonState::Retracted => "normal",
                    PistonState::Moving => "moving",
                    PistonState::Extended => "extended",
                };
                let rotated = if piston.along_first ^ (face_axis(piston.facing) == 0) {
                    "_rotated"
                } else {
                    ""
                };
                let shape = if state == PistonState::Retracted {
                    vec![]
                } else {
                    let mut shape = vec![axial_box(piston.facing, 0, 12, 0, 16)];
                    if state == PistonState::Extended {
                        shape.push(axial_box(piston.facing, 12, 16, 6, 10));
                    }
                    shape
                };
                (
                    piston,
                    piston.properties(state),
                    format!("create:block/mechanical_piston/{model}/block{rotated}"),
                    shape,
                )
            })
            .collect();
        let states: Vec<_> = definitions
            .iter()
            .map(|(piston, properties, model, shape)| {
                let (x, y) = ROTATIONS[usize::from(piston.facing)];
                State {
                    properties,
                    physical: STONE,
                    model,
                    rotating_model: "",
                    x,
                    y,
                    orientation: crate::relay_orientation(piston.shaft_axis()),
                    shape,
                }
            })
            .collect();
        content.add_block(
            registry,
            &Block {
                id,
                display_name: name,
                entity: ID,
                item_model,
                states: &states,
            },
            Some(crate::placed_state),
        )?;
    }
    for (id, name, item_model) in [
        (
            POLE,
            "Piston Extension Pole",
            "create:item/piston_extension_pole",
        ),
        (HEAD, "Mechanical Piston Head", ""),
    ] {
        let mut definitions = Vec::new();
        for (face, name) in DIRECTIONS.iter().enumerate() {
            for wet in [false, true] {
                let pole = vec![axial_box(face as u8, 0, 16, 6, 10)];
                if id == POLE {
                    definitions.push((
                        face,
                        format!("facing={name},waterlogged={wet}"),
                        "create:block/piston_extension_pole",
                        pole,
                        wet,
                    ));
                    continue;
                }
                let head = vec![
                    axial_box(face as u8, 0, 12, 6, 10),
                    axial_box(face as u8, 12, 16, 0, 16),
                ];
                for (kind, model) in [
                    ("normal", "create:block/mechanical_piston/normal/head"),
                    ("sticky", "create:block/mechanical_piston/sticky/head"),
                ] {
                    definitions.push((
                        face,
                        format!("facing={name},type={kind},waterlogged={wet}"),
                        model,
                        head.clone(),
                        wet,
                    ));
                }
            }
        }
        let states: Vec<_> = definitions
            .iter()
            .map(|(face, properties, model, shape, wet)| {
                let (x, y) = ROTATIONS[*face];
                State {
                    properties,
                    physical: if *wet {
                        "minecraft:oak_stairs[waterlogged=true]"
                    } else {
                        STONE
                    },
                    model,
                    rotating_model: "",
                    x,
                    y,
                    orientation: [0.0, 0.0, 0.0, 1.0],
                    shape,
                }
            })
            .collect();
        content.add_block(
            registry,
            &Block {
                id,
                display_name: name,
                entity: "",
                item_model,
                states: &states,
            },
            Some(crate::placed_state),
        )?;
    }
    Ok(())
}

pub fn add_states(content: Content, ids: &mut StateIds) {
    for sticky in [false, true] {
        for (piston, state) in PistonBlock::variants(sticky) {
            ids.add(
                content,
                piston.id(),
                &piston.properties(state),
                Some(Kind::Piston(piston)),
            );
        }
    }
    for name in DIRECTIONS {
        for wet in [false, true] {
            ids.add(
                content,
                POLE,
                &format!("facing={name},waterlogged={wet}"),
                None,
            );
        }
    }
}

pub fn placed_state(
    args: Placement<'_>,
    facing: u8,
    connected: impl Iterator<Item = u8>,
    sticky: bool,
) -> Option<u32> {
    let axis = face_axis(facing);
    let preferred: Vec<_> = connected
        .map(face_axis)
        .filter(|&shaft| shaft != axis)
        .collect();
    let along_first = if axis == 1 {
        preferred
            .first()
            .filter(|&first| preferred.iter().all(|axis| axis == first))
            .map_or_else(
                || face_axis(crate::nearest_looking_direction(args.yaw, 0.0)) == 0,
                |&axis| axis == 0,
            )
    } else if preferred.contains(&1) {
        axis == 0
    } else {
        axis == 2
    };
    PistonBlock {
        facing,
        along_first,
        sticky,
    }
    .state(PistonState::Retracted)
}

pub fn pole_placement(args: Placement<'_>, wet: bool) -> Option<u32> {
    part(POLE, args.face ^ 1, false, wet).state()
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
struct PistonContraption {
    blocks: Blocks,
    initial_length: i32,
    extension_length: i32,
}

fn block_state(world: World<'_>, pos: Pos) -> Option<PaletteEntry> {
    let mut state = [UNLOADED_STATE];
    world.read(&[pos], &mut state);
    Some(PaletteEntry::parse(
        &CONTENT.get()?.describe_state(state[0])?,
    ))
}

impl PistonContraption {
    fn collect_extensions(world: World<'_>, base: Pos, piston: PistonBlock) -> Option<(i32, i32)> {
        let direction = DIRECTIONS[usize::from(piston.facing)];
        let is_pole = |entry: &PaletteEntry| {
            entry.name == POLE
                && entry
                    .properties
                    .get("facing")
                    .and_then(|face| DIRECTIONS.iter().position(|&name| name == face))
                    .is_some_and(|face| face_axis(face as u8) == face_axis(piston.facing))
        };
        let mut front = 0;
        if block_state(world, base)?
            .properties
            .get("state")
            .is_some_and(|state| state == "extended")
        {
            loop {
                let entry = block_state(world, piston.at(base, front + 1))?;
                if front == MAX_PISTON_POLES {
                    return None;
                }
                front += 1;
                if entry.name == HEAD
                    && entry
                        .properties
                        .get("facing")
                        .is_some_and(|face| face == direction)
                {
                    break;
                }
                if !is_pole(&entry) {
                    return None;
                }
            }
        }
        let mut back = 0;
        while is_pole(&block_state(world, piston.at(base, -back - 1))?) {
            back += 1;
            if front + back > MAX_PISTON_POLES {
                return None;
            }
        }
        (front + back > 0).then_some((front, back))
    }

    fn assemble(
        world: World<'_>,
        checks: &BlockMovementChecks,
        base: Pos,
        piston: PistonBlock,
        extending: bool,
    ) -> Option<Self> {
        let (front, back) = Self::collect_extensions(world, base, piston)?;
        if (extending && back == 0) || (!extending && front == 0) {
            return None;
        }
        let start = piston.at(base, front);
        let mut frontier = VecDeque::new();
        if extending || piston.sticky {
            let range = if extending { MAX_CHASSIS_RANGE + 1 } else { 1 };
            for distance in 1..=range {
                let pos = piston.at(start, distance);
                match checks.classify(world, pos) {
                    Some(BlockMovement::Movable(entry)) => frontier.push_back((pos, entry)),
                    Some(BlockMovement::Air) => break,
                    _ if !extending => break,
                    _ => return None,
                }
            }
        }
        let ignored = (-back..=front)
            .map(|distance| piston.at(base, distance))
            .collect();
        let (mut blocks, mut palette) =
            Blocks::collect_attached(start, frontier, ignored, |pos| checks.classify(world, pos))?;
        blocks.capture_entities(world, start)?;
        let origin = Pos { x: 0, y: 0, z: 0 };
        for index in -back..=front {
            let entry = if index == front {
                piston.head()
            } else {
                let wet = index != 0
                    && world
                        .fluid_id(piston.at(base, index))
                        .is_some_and(|id| id == "minecraft:water");
                part(POLE, piston.facing, false, wet)
            };
            blocks.push(piston.at(origin, index - front), entry, &mut palette);
        }
        Some(Self {
            blocks,
            initial_length: front,
            extension_length: front + back,
        })
    }

    fn occupied(&self, anchor: Pos) -> HashSet<Pos> {
        self.blocks
            .block_list
            .iter()
            .map(|block| offset(anchor, block.local_pos()))
            .collect()
    }

    fn is_colliding_with_world(
        &self,
        world: World<'_>,
        base: Pos,
        piston: PistonBlock,
        shift: i32,
        extending: bool,
        sources: &HashSet<Pos>,
    ) -> bool {
        let step = if extending { 1 } else { -1 };
        let destinations = self.occupied(piston.at(base, shift));
        destinations.iter().any(|&pos| {
            pos != base
                && !sources.contains(&pos)
                && !destinations.contains(&piston.at(pos, step))
                && world
                    .collision_shape(pos)
                    .is_none_or(|shape| !shape.is_empty())
        })
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct PistonData {
    offset: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    contraption: Option<PistonContraption>,
}

#[derive(Clone, Debug, Default)]
pub struct MechanicalPiston {
    pub data: PistonData,
    pub displays: ContraptionDisplays,
    assemble_next_tick: bool,
    previous_speed: f32,
}

impl MechanicalPiston {
    fn assemble(
        &mut self,
        world: World<'_>,
        checks: &BlockMovementChecks,
        base: Pos,
        piston: PistonBlock,
        extending: bool,
    ) -> bool {
        let (Some(contraption), Some(moving), Some(air)) = (
            PistonContraption::assemble(world, checks, base, piston, extending),
            piston.state(PistonState::Moving),
            checks.state("minecraft:air"),
        ) else {
            return false;
        };
        let initial = contraption.initial_length;
        let sources = contraption.occupied(piston.at(base, initial));
        if contraption.is_colliding_with_world(
            world,
            base,
            piston,
            initial + if extending { 1 } else { -1 },
            extending,
            &sources,
        ) {
            return false;
        }
        let anchor = piston.at(base, initial);
        let remove: Vec<_> = contraption
            .blocks
            .block_list
            .iter()
            .map(|block| {
                let position = offset(anchor, block.local_pos());
                BlockUpdate {
                    position,
                    state: if position == base { moving } else { air },
                    flags: content::UPDATE_MOVED | content::UPDATE_CLIENTS,
                }
            })
            .collect();
        let data = PistonData {
            offset: initial as f32,
            contraption: Some(contraption),
        };
        if !fastnbt::to_bytes(&data).is_ok_and(|bytes| bytes.len() <= MAX_ASSEMBLY_DATA_BYTES)
            || world.write(&remove) != remove.len()
        {
            return false;
        }
        self.data = data;
        true
    }

    fn disassemble(
        &mut self,
        world: World<'_>,
        checks: &BlockMovementChecks,
        base: Pos,
        piston: PistonBlock,
        removed: bool,
    ) -> bool {
        let Some(contraption) = &self.data.contraption else {
            return false;
        };
        let current = (self.data.offset + 0.5).floor() as i32;
        let state = if current == 0 {
            PistonState::Retracted
        } else {
            PistonState::Extended
        };
        let Some(state) = piston.state(state) else {
            return false;
        };
        let transform = StructureTransform {
            anchor: piston.at(base, current),
            axis: 0,
            angle: 0.0,
        };
        if !contraption
            .blocks
            .place(world, checks, transform, Some(base))
        {
            return false;
        }
        if !removed {
            world.write(&[BlockUpdate {
                position: base,
                state,
                flags: content::UPDATE_ALL,
            }]);
        }
        self.displays.clear(world);
        self.data = PistonData::default();
        true
    }

    fn tick(
        &mut self,
        world: World<'_>,
        checks: &BlockMovementChecks,
        base: Pos,
        piston: PistonBlock,
        rpm: f32,
    ) -> bool {
        let previous = std::mem::replace(&mut self.previous_speed, rpm);
        let speed = piston.movement_speed(rpm);
        let extending = speed > 0.0;
        let queued = std::mem::take(&mut self.assemble_next_tick) || previous != rpm;
        if self.data.contraption.is_none() {
            return queued && speed != 0.0 && self.assemble(world, checks, base, piston, extending);
        }
        if speed == 0.0 && self.disassemble(world, checks, base, piston, false) {
            return true;
        }
        let Some(contraption) = &self.data.contraption else {
            return false;
        };
        let end = contraption.extension_length as f32;
        let next = (self.data.offset + speed).clamp(0.0, end);
        let shift = piston.collision_offset(next, extending);
        let collision = speed != 0.0
            && contraption.is_colliding_with_world(
                world,
                base,
                piston,
                shift,
                extending,
                &HashSet::new(),
            );
        if collision {
            let grid = (self.data.offset + 0.5).floor();
            let speed = speed.clamp(-grid, end - grid);
            self.data.offset = (grid - speed + 0.5).floor().clamp(0.0, end);
        } else {
            self.data.offset = next;
        }
        if (collision || next == 0.0 || next == end)
            && self.disassemble(world, checks, base, piston, false)
        {
            self.assemble_next_tick = collision && extending;
            return true;
        }
        if let Some(contraption) = &self.data.contraption {
            self.displays.show(
                world,
                &contraption.blocks,
                StructureTransform {
                    anchor: base,
                    axis: 0,
                    angle: 0.0,
                },
                FACES[usize::from(piston.facing)].map(|axis| axis as f32 * self.data.offset),
            );
        }
        speed != 0.0
    }
}

pub fn use_item_on(kinetics: &mut Kinetics, context: &UseBlock<'_>) -> BlockUseResult {
    if context.sneaking || context.stack.read().is_some() {
        return BlockUseResult::TryEmptyHand;
    }
    let Some(piston) = kinetics
        .get_mut(context.position)
        .and_then(|be| be.piston.as_mut())
    else {
        return BlockUseResult::Pass;
    };
    piston.assemble_next_tick = true;
    BlockUseResult::Success
}

pub fn tick(world: World<'_>, checks: &BlockMovementChecks, kinetics: &mut Kinetics) {
    for (pos, block, mut piston) in kinetics.dropped_pistons.drain(..) {
        if !piston.disassemble(world, checks, pos, block, true) {
            piston.displays.clear(world);
            if let Some(contraption) = piston.data.contraption {
                contraption
                    .blocks
                    .drop_contents(world, pos, |item| if item == HEAD { POLE } else { item });
            }
        }
    }
    for pos in kinetics.positions(|kind| matches!(kind, Kind::Piston(_))) {
        let Some(be) = kinetics.get_mut(pos) else {
            continue;
        };
        let Kind::Piston(block) = be.kind else {
            continue;
        };
        let speed = be.get_speed();
        if let Some(piston) = &mut be.piston {
            be.dirty |= piston.tick(world, checks, pos, block, speed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn piston_projection_matches_signed_rpm_and_collision_grid() {
        let face = |name| DIRECTIONS.iter().position(|&face| name == face).unwrap() as u8;
        let east = PistonBlock {
            facing: face("east"),
            along_first: true,
            sticky: false,
        };
        let west = PistonBlock {
            facing: face("west"),
            ..east
        };
        let north = PistonBlock {
            facing: face("north"),
            ..east
        };
        assert_eq!(east.movement_speed(-128.0), 0.25);
        assert_eq!(west.movement_speed(-128.0), -0.25);
        assert_eq!(north.movement_speed(-128.0), 0.25);
        assert_eq!(east.collision_offset(1.0, true), 2);
        assert_eq!(west.collision_offset(1.0, true), 1);
        assert_eq!(east.collision_offset(0.75, false), 0);
        assert_eq!(west.collision_offset(0.75, false), 0);
    }
}

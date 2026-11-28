//! Create 6.0.10's Gantry Shaft, Carriage, and GantryContraptionEntity.

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
    CONTENT, DIRECTIONS, STONE, StateIds, collider,
    contraption::{
        BlockMovementChecks, Blocks, ContraptionDisplays, MAX_ASSEMBLY_DATA_BYTES, PaletteEntry,
        StructureTransform,
    },
    kinetics::{
        FACES, Kind, Kinetics, Level, actuator_speed, directional_shaft_axis, face_axis, offset,
        positive,
    },
};

const SHAFT: &str = "create:gantry_shaft";
const CARRIAGE: &str = "create:gantry_carriage";
const ROTATIONS: [(i16, i16); 6] = [(180, 0), (0, 0), (90, 0), (90, 180), (90, 270), (90, 90)];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Part {
    Single,
    Start,
    Middle,
    End,
}

impl Part {
    const ALL: [Self; 4] = [Self::Single, Self::Start, Self::Middle, Self::End];

    const fn name(self) -> &'static str {
        match self {
            Self::Single => "single",
            Self::Start => "start",
            Self::Middle => "middle",
            Self::End => "end",
        }
    }

    const fn connected(back: bool, front: bool) -> Self {
        match (back, front) {
            (true, true) => Self::Middle,
            (true, false) => Self::End,
            (false, true) => Self::Start,
            (false, false) => Self::Single,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Shaft {
    pub facing: u8,
    part: Part,
    pub powered: bool,
}

impl Shaft {
    fn variants() -> impl Iterator<Item = Self> {
        (0..6).flat_map(|facing| {
            Part::ALL.into_iter().flat_map(move |part| {
                [false, true].into_iter().map(move |powered| Self {
                    facing,
                    part,
                    powered,
                })
            })
        })
    }

    fn properties(self) -> String {
        format!(
            "facing={},part={},powered={}",
            DIRECTIONS[usize::from(self.facing)],
            self.part.name(),
            self.powered
        )
    }

    fn state(self) -> Option<u32> {
        CONTENT
            .get()?
            .state(&format!("{SHAFT}[{}]", self.properties()))
    }

    fn model(self) -> String {
        let part = self.part.name();
        if !self.powered && positive(self.facing) {
            return format!("create:block/gantry_shaft/block_{part}");
        }
        let power = if self.powered { "_powered" } else { "" };
        let flip = if positive(self.facing) {
            ""
        } else {
            "_flipped"
        };
        format!("create:block/gantry_shaft_{part}{power}{flip}")
    }

    fn pinion_speed(rpm: f32) -> f32 {
        actuator_speed(-rpm)
    }

    fn can_assemble_on(self, speed: f32) -> bool {
        !self.powered
            && match self.part {
                Part::Single => false,
                Part::Start => speed > 0.0,
                Part::Middle => speed != 0.0,
                Part::End => speed < 0.0,
            }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Carriage {
    pub facing: u8,
    along_first: bool,
}

impl Carriage {
    fn variants() -> impl Iterator<Item = Self> {
        (0..6).flat_map(|facing| {
            [false, true].into_iter().map(move |along_first| Self {
                facing,
                along_first,
            })
        })
    }

    fn properties(self) -> String {
        format!(
            "axis_along_first={},facing={}",
            self.along_first,
            DIRECTIONS[usize::from(self.facing)]
        )
    }

    pub const fn axis(self) -> u8 {
        directional_shaft_axis(self.facing, self.along_first)
    }

    fn state(self) -> Option<u32> {
        CONTENT
            .get()?
            .state(&format!("{CARRIAGE}[{}]", self.properties()))
    }
}

pub fn pinion_modifier(shaft: Shaft, carriage: Carriage) -> f32 {
    let reverse = match face_axis(shaft.facing) {
        0 => matches!(carriage.facing, 0 | 3),
        1 => matches!(carriage.facing, 2 | 5),
        _ => matches!(carriage.facing, 1 | 4),
    };
    if positive(shaft.facing) ^ reverse {
        1.0
    } else {
        -1.0
    }
}

pub fn register(content: Content, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
    let shafts: Vec<_> = Shaft::variants()
        .map(|shaft| {
            let axis = usize::from(face_axis(shaft.facing));
            let mut shape = ShapeBox {
                min: [4; 3],
                max: [12; 3],
            };
            (shape.min[axis], shape.max[axis]) = (0, 16);
            (shaft, shaft.properties(), shaft.model(), [shape])
        })
        .collect();
    let states: Vec<_> = shafts
        .iter()
        .map(|(shaft, properties, model, shape)| {
            let (x, y) = ROTATIONS[usize::from(shaft.facing)];
            State {
                properties,
                physical: STONE,
                model,
                rotating_model: "",
                x,
                y,
                orientation: crate::relay_orientation(face_axis(shaft.facing)),
                shape,
            }
        })
        .collect();
    content.add_block(
        registry,
        &Block {
            id: SHAFT,
            display_name: "Gantry Shaft",
            entity: SHAFT,
            item_model: "create:item/gantry_shaft",
            states: &states,
        },
        Some(crate::placed_state),
    )?;

    let carriages: Vec<_> = Carriage::variants()
        .map(|carriage| (carriage, carriage.properties()))
        .collect();
    let states: Vec<_> = carriages
        .iter()
        .map(|(carriage, properties)| {
            let axis = face_axis(carriage.facing);
            let (x, y) = match carriage.facing {
                0 => (270, if carriage.along_first { 0 } else { 90 }),
                1 => (90, if carriage.along_first { 0 } else { 90 }),
                2 => (0, 180),
                3 => (0, 0),
                4 => (0, 90),
                _ => (0, 270),
            };
            let model = if axis != 1 && carriage.axis() == 1 {
                "create:block/gantry_carriage/vertical"
            } else {
                "create:block/gantry_carriage/horizontal"
            };
            State {
                properties,
                physical: STONE,
                model,
                rotating_model: "",
                x,
                y,
                orientation: crate::relay_orientation(carriage.axis()),
                shape: &[],
            }
        })
        .collect();
    content.add_block(
        registry,
        &Block {
            id: CARRIAGE,
            display_name: "Gantry Carriage",
            entity: CARRIAGE,
            item_model: "create:item/gantry_carriage",
            states: &states,
        },
        Some(crate::placed_state),
    )
}

pub fn add_states(content: Content, ids: &mut StateIds) {
    for shaft in Shaft::variants() {
        ids.add(
            content,
            SHAFT,
            &shaft.properties(),
            Some(Kind::GantryShaft(shaft)),
        );
    }
    for carriage in Carriage::variants() {
        ids.add(
            content,
            CARRIAGE,
            &carriage.properties(),
            Some(Kind::GantryCarriage(carriage)),
        );
    }
}

fn at(pos: Pos, facing: u8, cells: i32) -> Pos {
    offset(pos, FACES[usize::from(facing)].map(|axis| axis * cells))
}

pub fn place_shaft(
    args: Placement<'_>,
    states: &StateIds,
    around: &[u32],
    look: u8,
) -> Option<u32> {
    let mut facing = if args.sneak { look } else { look ^ 1 };
    let clicked = [facing ^ 1, args.face].into_iter().find_map(|side| {
        let Kind::GantryShaft(shaft) = states.kind(around[usize::from(side)])? else {
            return None;
        };
        (face_axis(facing) == face_axis(shaft.facing)).then_some(shaft.facing)
    });
    if let Some(clicked) = clicked {
        facing = if args.sneak { clicked ^ 1 } else { clicked };
    }
    let matches = |side: u8| match states.kind(around[usize::from(side)]) {
        Some(Kind::GantryShaft(shaft)) if shaft.facing == facing => Some(shaft),
        _ => None,
    };
    let [back, front] = [matches(facing ^ 1), matches(facing)];
    let part = Part::connected(back.is_some(), front.is_some());
    let powered = args.world.has_neighbor_signal(args.position)
        || [back, front]
            .into_iter()
            .flatten()
            .any(|shaft| shaft.powered);
    Shaft {
        facing,
        part,
        powered,
    }
    .state()
}

pub fn place_carriage(facing: u8, support: Option<Kind>) -> Option<u32> {
    let Kind::GantryShaft(shaft) = support? else {
        return None;
    };
    let mut carriage = Carriage {
        facing,
        along_first: false,
    };
    if face_axis(shaft.facing) == face_axis(carriage.facing) {
        return None;
    }
    carriage.along_first = carriage.axis() == face_axis(shaft.facing);
    carriage.state()
}

fn line(
    kinetics: &Kinetics,
    start: Pos,
    facing: u8,
    forward: bool,
) -> impl Iterator<Item = Pos> + '_ {
    std::iter::successors(Some(start), move |&pos| {
        let next = at(pos, facing, if forward { 1 } else { -1 });
        match kinetics.kind(next) {
            Some(Kind::GantryShaft(shaft)) if shaft.facing == facing => Some(next),
            _ => None,
        }
    })
}

pub fn refresh_lines(world: World<'_>, level: &mut dyn Level, kinetics: &mut Kinetics) {
    let mut visited = HashSet::new();
    for pos in kinetics.positions(|kind| matches!(kind, Kind::GantryShaft(_))) {
        let Some(Kind::GantryShaft(shaft)) = kinetics.kind(pos) else {
            continue;
        };
        if visited.contains(&pos) {
            continue;
        }
        let start = line(kinetics, pos, shaft.facing, false)
            .last()
            .unwrap_or(pos);
        let line: Vec<_> = line(kinetics, start, shaft.facing, true).collect();
        visited.extend(line.iter().copied());
        let powered = line.iter().any(|&pos| world.has_neighbor_signal(pos));
        for (index, pos) in line.iter().copied().enumerate() {
            let Some(Kind::GantryShaft(previous)) = kinetics.kind(pos) else {
                continue;
            };
            let part = Part::connected(index != 0, index + 1 != line.len());
            let next = Shaft {
                part,
                powered,
                ..previous
            };
            if next == previous {
                continue;
            }
            let Some(state) = next.state() else { continue };
            if world.write(&[BlockUpdate {
                position: pos,
                state,
                flags: content::UPDATE_CLIENTS,
            }]) == 1
            {
                if powered != previous.powered {
                    kinetics.detach(level, pos, true);
                }
                kinetics.set_kind(pos, Kind::GantryShaft(next));
            }
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct GantryContraption {
    anchor: [i32; 3],
    gantry_axis: u8,
    facing: u8,
    offset: f32,
    blocks: Blocks,
    #[serde(skip)]
    displays: ContraptionDisplays,
    #[serde(skip)]
    age: u8,
}

impl GantryContraption {
    pub fn clear_displays(&mut self, world: World<'_>) {
        self.displays.clear(world);
    }

    fn anchor(&self) -> Pos {
        let [x, y, z] = self.anchor;
        Pos { x, y, z }
    }

    fn grid_pos(&self) -> Pos {
        let axis = if positive(self.gantry_axis) {
            self.offset
        } else {
            -self.offset
        };
        let direction = if positive(self.gantry_axis) {
            self.gantry_axis
        } else {
            self.gantry_axis ^ 1
        };
        at(self.anchor(), direction, (axis + 0.5).floor() as i32)
    }

    fn shaft_pos(&self) -> Pos {
        at(self.grid_pos(), self.facing ^ 1, 1)
    }

    fn show(&mut self, world: World<'_>) {
        let transform = StructureTransform {
            anchor: self.anchor(),
            axis: 0,
            angle: 0.0,
        };
        self.displays.show(
            world,
            &self.blocks,
            transform,
            FACES[usize::from(self.gantry_axis)].map(|axis| axis as f32 * self.offset),
        );
    }

    fn disassemble(&mut self, world: World<'_>, checks: &BlockMovementChecks) -> bool {
        let transform = StructureTransform {
            anchor: self.grid_pos(),
            axis: 0,
            angle: 0.0,
        };
        if !self.blocks.place(
            world,
            checks,
            transform,
            None,
            collider::Frame::translated(
                self.anchor(),
                FACES[usize::from(self.gantry_axis)]
                    .map(|axis| f64::from(axis) * f64::from(self.offset)),
            ),
        ) {
            return false;
        }
        self.clear_displays(world);
        true
    }

    fn tick(
        &mut self,
        world: World<'_>,
        checks: &BlockMovementChecks,
        kinetics: &Kinetics,
    ) -> bool {
        if self.gantry_axis > 5 || self.facing > 5 || !self.offset.is_finite() {
            return true;
        }
        let shaft = kinetics.get(self.shaft_pos()).and_then(|be| {
            let Kind::GantryShaft(shaft) = be.kind else {
                return None;
            };
            Some((shaft, Shaft::pinion_speed(be.get_speed())))
        });
        let Some((shaft, speed)) = shaft.filter(|(shaft, speed)| !shaft.powered && *speed != 0.0)
        else {
            if self.disassemble(world, checks) {
                return true;
            }
            self.show(world);
            return false;
        };
        let order = if positive(shaft.facing) { 1.0 } else { -1.0 };
        let axis = self.offset
            * if positive(self.gantry_axis) {
                1.0
            } else {
                -1.0
            };
        let next = axis + order * speed;
        let leaving = (axis + 0.5).floor() + 0.5 < next + 0.5;
        if (leaving != (speed * order < 0.0)) && !shaft.can_assemble_on(speed) {
            return self.disassemble(world, checks);
        }
        let direction = if speed > 0.0 {
            shaft.facing
        } else {
            shaft.facing ^ 1
        };
        let positive_direction = if positive(shaft.facing) {
            shaft.facing
        } else {
            shaft.facing ^ 1
        };
        let cell = at(
            self.anchor(),
            positive_direction,
            axis.floor() as i32 + i32::from(positive(direction)),
        );
        let before = self.offset;
        let next_offset = before
            + if self.age < 2 {
                0.0
            } else if self.gantry_axis == shaft.facing {
                speed
            } else {
                -speed
            };
        let translation = |distance: f32| {
            FACES[usize::from(self.gantry_axis)].map(|axis| f64::from(axis) * f64::from(distance))
        };
        let anchor = self.anchor();
        if self.blocks.tick_breakers(
            world,
            collider::Frame::translated(anchor, translation(before)),
            collider::Frame::translated(anchor, translation(next_offset)),
        ) {
            self.show(world);
            return false;
        }
        if self
            .blocks
            .is_colliding_with_world(world, cell, direction, &HashSet::new(), None)
        {
            return self.disassemble(world, checks);
        }
        self.offset = next_offset;
        collider::collide_entities(
            world,
            &mut self.blocks,
            anchor,
            translation(before),
            translation(self.offset),
        );
        self.age = self.age.saturating_add(1);
        self.show(world);
        false
    }
}

fn try_assemble(
    world: World<'_>,
    checks: &BlockMovementChecks,
    kinetics: &mut Kinetics,
    pos: Pos,
    carriage: Carriage,
    rail: (Pos, Shaft, f32),
) -> Option<()> {
    let (shaft_pos, shaft, speed) = rail;
    let start = PaletteEntry::parse(&format!("{CARRIAGE}[{}]", carriage.properties()));
    let frontier = VecDeque::from([(pos, start)]);
    let (mut blocks, _) =
        checks.collect_attached(world, pos, frontier, HashSet::from([shaft_pos]), None)?;
    blocks.capture_entities(world, pos, kinetics)?;
    let sources = blocks.positions(pos).collect();
    let direction = if speed > 0.0 {
        shaft.facing
    } else {
        shaft.facing ^ 1
    };
    if blocks.is_colliding_with_world(world, at(pos, direction, 1), direction, &sources, None) {
        return None;
    }
    let remove: Vec<_> = blocks.removals(pos, checks)?.collect();
    let be = kinetics.get_mut(shaft_pos)?;
    be.gantries.push(GantryContraption {
        anchor: [pos.x, pos.y, pos.z],
        gantry_axis: shaft.facing,
        facing: carriage.facing,
        offset: 0.0,
        blocks,
        displays: ContraptionDisplays::default(),
        age: 0,
    });
    if !fastnbt::to_bytes(&GantrySave {
        gantries: &be.gantries,
    })
    .is_ok_and(|bytes| bytes.len() <= MAX_ASSEMBLY_DATA_BYTES)
        || world.write(&remove) != remove.len()
    {
        be.gantries.pop();
        return None;
    }
    if let Some(gantry) = be.gantries.last_mut() {
        gantry.blocks.detach_glue(world);
    }
    be.dirty = true;
    crate::contraption::detach_moved_belts(world, &remove, kinetics);
    Some(())
}

#[derive(Serialize)]
struct GantrySave<'a> {
    gantries: &'a [GantryContraption],
}

pub fn use_item_on(kinetics: &mut Kinetics, use_block: &UseBlock<'_>) -> BlockUseResult {
    if use_block.sneaking || use_block.stack.read().is_some() {
        return BlockUseResult::TryEmptyHand;
    }
    if let Some(carriage) = kinetics.get_mut(use_block.position) {
        carriage.last_gantry_shaft = None;
    }
    BlockUseResult::Success
}

pub fn tick(world: World<'_>, checks: &BlockMovementChecks, kinetics: &mut Kinetics) {
    for mut gantry in kinetics.dropped_gantries.drain(..) {
        if !gantry.disassemble(world, checks) {
            gantry.clear_displays(world);
            let position = gantry.grid_pos();
            gantry.blocks.drop_contents(world, position, |item| item);
        }
    }
    for pos in kinetics.positions(|kind| matches!(kind, Kind::GantryCarriage(_))) {
        let Some(Kind::GantryCarriage(carriage)) = kinetics.kind(pos) else {
            continue;
        };
        let shaft_pos = at(pos, carriage.facing ^ 1, 1);
        let Some((shaft, speed)) = kinetics.get(shaft_pos).and_then(|be| {
            let Kind::GantryShaft(shaft) = be.kind else {
                return None;
            };
            (face_axis(shaft.facing) != face_axis(carriage.facing))
                .then_some((shaft, Shaft::pinion_speed(be.get_speed())))
        }) else {
            let mut support = [UNLOADED_STATE];
            if world.read(&[shaft_pos], &mut support) && support[0] != UNLOADED_STATE {
                world.destroy(pos, true);
            }
            continue;
        };
        let Some(be) = kinetics.get_mut(pos) else {
            continue;
        };
        let previous = be.last_gantry_shaft.replace((shaft, speed));
        if previous != Some((shaft, speed)) && shaft.can_assemble_on(speed) {
            try_assemble(
                world,
                checks,
                kinetics,
                pos,
                carriage,
                (shaft_pos, shaft, speed),
            );
        }
    }
    for pos in kinetics.positions(|kind| matches!(kind, Kind::GantryShaft(_))) {
        let Some(be) = kinetics.get_mut(pos).filter(|be| !be.gantries.is_empty()) else {
            continue;
        };
        let mut gantries = std::mem::take(&mut be.gantries);
        let mut dirty = false;
        gantries.retain_mut(|gantry| {
            let offset = gantry.offset;
            dirty |= gantry.displays.apply_clicks(world, &mut gantry.blocks);
            let finished = gantry.tick(world, checks, kinetics);
            dirty |= finished || offset != gantry.offset;
            !finished
        });
        if let Some(be) = kinetics.get_mut(pos) {
            be.gantries = gantries;
            be.dirty |= dirty;
        }
    }
}

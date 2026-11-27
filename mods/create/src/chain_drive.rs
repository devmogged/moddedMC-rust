//! Encased Chain Drive casing connections and Adjustable Chain Gearshift ratios.

use moddedmc_api::{
    Pos, RegistrationError, Registry,
    content::{
        Block, BlockUpdate, Content, State, UNLOADED_STATE, UPDATE_ALL, UPDATE_CLIENTS,
        UPDATE_KNOWN_SHAPE, World,
    },
    effects::SOUND_BLOCKS,
    interaction::{UseOnBlock, UseResult},
};

use crate::{
    AXES, EFFECTS, StateIds,
    contraption::{PaletteEntry, StructureTransform},
    kinetics::{FACES, Kind, face_axis, offset, positive},
    relay_orientation,
};

const DRIVE: &str = "create:encased_chain_drive";
const GEARSHIFT: &str = "create:adjustable_chain_gearshift";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Part {
    None,
    Start,
    Middle,
    End,
}

impl Part {
    const fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Start => "start",
            Self::Middle => "middle",
            Self::End => "end",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Drive {
    pub(crate) axis: u8,
    along_first: bool,
    part: Part,
    pub(crate) powered: Option<bool>,
}

impl Drive {
    fn variants(gearshift: bool) -> impl Iterator<Item = Self> {
        [1, 0, 2].into_iter().flat_map(move |axis| {
            [Part::None, Part::Start, Part::Middle, Part::End]
                .into_iter()
                .flat_map(move |part| {
                    [false, true].into_iter().flat_map(move |along_first| {
                        [false, true]
                            .into_iter()
                            .take(if gearshift { 2 } else { 1 })
                            .map(move |powered| Self {
                                axis,
                                along_first,
                                part,
                                powered: gearshift.then_some(powered),
                            })
                    })
                })
        })
    }

    fn properties(self) -> String {
        let mut props = format!(
            "axis={},axis_along_first={},part={}",
            AXES[usize::from(self.axis)],
            self.along_first,
            self.part.name()
        );
        if let Some(powered) = self.powered {
            props.push_str(if powered {
                ",powered=true"
            } else {
                ",powered=false"
            });
        }
        props
    }

    pub(crate) fn selector(self) -> String {
        let block = if self.powered.is_some() {
            GEARSHIFT
        } else {
            DRIVE
        };
        format!("{block}[{}]", self.properties())
    }

    /// ChainDriveGenerator's model and quarter turns.
    fn visual(self) -> (String, i16, i16) {
        let (suffix, x, y) = if self.part == Part::None {
            (
                "single",
                if self.axis == 1 { 90 } else { 0 },
                if self.axis == 0 { 90 } else { 0 },
            )
        } else {
            let start = self.part == Part::Start;
            let end = self.part == Part::End;
            let flip = end && !self.along_first || start && self.along_first;
            let (x, y) = match self.axis {
                0 => (i16::from(self.along_first) * 90 + i16::from(start) * 180, 0),
                1 => (0, i16::from(self.along_first) * 90 + i16::from(flip) * 180),
                _ => (
                    if self.along_first {
                        0
                    } else if start {
                        270
                    } else {
                        90
                    },
                    if self.along_first && end { 270 } else { 90 },
                ),
            };
            let suffix = match (self.axis == 1, self.part == Part::Middle) {
                (true, true) => "middle_vertical",
                (true, false) => "end_vertical",
                (false, true) => "middle_horizontal",
                (false, false) => "end_horizontal",
            };
            (suffix, x, y)
        };
        let model = if let Some(powered) = self.powered {
            let power = if powered { "_powered" } else { "" };
            format!("create:block/adjustable_chain_gearshift_{suffix}{power}")
        } else {
            format!("create:block/encased_chain_drive/{suffix}")
        };
        (model, x, y)
    }

    fn connection_axis(self) -> u8 {
        match (self.axis, self.along_first) {
            (0, true) | (2, false) => 1,
            (_, true) => 0,
            _ => 2,
        }
    }

    pub(crate) fn same_topology(self, other: Self) -> bool {
        self.part == other.part && self.axis == other.axis && self.along_first == other.along_first
    }

    pub(crate) fn connects(self, other: Self, face: u8) -> bool {
        let axis = self.connection_axis();
        axis == other.connection_axis()
            && axis == face_axis(face)
            && (self.part == Part::Middle
                || self.part
                    == if positive(face) {
                        Part::Start
                    } else {
                        Part::End
                    })
    }

    /// ChainDriveBlock.updateShape keeps a linked casing on its existing connection axis.
    fn update_shape(mut self, face: u8, neighbor: Option<Self>) -> Self {
        let axis = face_axis(face);
        if self.axis == axis {
            return self;
        }
        let first = along_first(self.axis, axis);
        let positive = positive(face);
        let has_neighbor = if positive { Part::Start } else { Part::End };
        let Some(neighbor) = neighbor else {
            if first == self.along_first {
                self.part = match self.part {
                    Part::Middle => {
                        if positive {
                            Part::End
                        } else {
                            Part::Start
                        }
                    }
                    part if part == has_neighbor => Part::None,
                    part => part,
                };
            }
            return self;
        };
        if neighbor.axis == axis
            || (neighbor.part != Part::None && neighbor.connection_axis() != axis)
        {
            return self;
        }
        if self.part == Part::None {
            self.along_first = first;
            self.part = has_neighbor;
        } else if self.connection_axis() == axis && self.part != has_neighbor {
            self.part = Part::Middle;
        }
        self
    }
}

const fn along_first(axis: u8, connection_axis: u8) -> bool {
    if axis == 0 {
        connection_axis == 1
    } else {
        connection_axis == 0
    }
}

/// ChainDriveBlock.transform rotates both axes and swaps the end caps if the chain flips.
pub(crate) fn transform(
    entry: &PaletteEntry,
    rotation: StructureTransform,
) -> Option<PaletteEntry> {
    if !matches!(entry.name.as_str(), DRIVE | GEARSHIFT) {
        return None;
    }
    const POSITIVE_FACES: [u8; 3] = [5, 1, 3];
    let axis = AXES
        .iter()
        .position(|axis| *axis == entry.properties.get("axis").map_or("", String::as_str))?;
    let drive = Drive {
        axis: axis as u8,
        along_first: entry
            .properties
            .get("axis_along_first")
            .is_some_and(|value| value == "true"),
        part: Part::None,
        powered: None,
    };
    let shaft = rotation.rotate_face(POSITIVE_FACES[axis]);
    let chain = rotation.rotate_face(POSITIVE_FACES[usize::from(drive.connection_axis())]);
    let axis = face_axis(shaft);
    let mut moved = entry.clone();
    moved
        .properties
        .insert("axis".into(), AXES[usize::from(axis)].into());
    moved.properties.insert(
        "axis_along_first".into(),
        along_first(axis, face_axis(chain)).to_string(),
    );
    if !positive(chain)
        && let Some(part) = moved.properties.get_mut("part")
    {
        match part.as_str() {
            "start" => *part = "end".into(),
            "end" => *part = "start".into(),
            _ => {}
        }
    }
    Some(moved)
}

pub(crate) fn from_kind(kind: Option<Kind>) -> Option<Drive> {
    if let Some(Kind::ChainDrive(drive)) = kind {
        Some(drive)
    } else {
        None
    }
}

pub(crate) fn modifier(signal: u8) -> f32 {
    if signal == 0 {
        1.0
    } else {
        1.0 + (f32::from(signal) + 1.0) / 16.0
    }
}

pub(crate) fn register(
    content: Content,
    registry: &mut Registry<'_>,
) -> Result<(), RegistrationError> {
    for (id, name, item, entity, gearshift) in [
        (
            DRIVE,
            "Encased Chain Drive",
            "create:item/encased_chain_drive",
            "create:encased_shaft",
            false,
        ),
        (
            GEARSHIFT,
            "Adjustable Chain Gearshift",
            "create:item/adjustable_chain_gearshift",
            GEARSHIFT,
            true,
        ),
    ] {
        let rows: Vec<_> = Drive::variants(gearshift)
            .map(|drive| (drive, drive.properties(), drive.visual()))
            .collect();
        let states: Vec<_> = rows
            .iter()
            .map(|(drive, props, (model, x, y))| State {
                properties: props,
                // ChainDriveBlock.shouldCheckWeakPower keeps the casing from conducting redstone.
                physical: "minecraft:copper_grate",
                model,
                rotating_model: "create:block/shaft",
                x: *x,
                y: *y,
                orientation: relay_orientation(drive.axis),
                shape: &[],
            })
            .collect();
        content.add_block(
            registry,
            &Block {
                id,
                display_name: name,
                entity,
                item_model: item,
                states: &states,
            },
            Some(crate::placed_state),
        )?;
    }
    Ok(())
}

pub(crate) fn add_states(content: Content, states: &mut StateIds) {
    for (id, gearshift) in [(DRIVE, false), (GEARSHIFT, true)] {
        for drive in Drive::variants(gearshift) {
            states.add(
                content,
                id,
                &drive.properties(),
                Some(Kind::ChainDrive(drive)),
            );
        }
    }
}

pub(crate) fn placed(
    world: World<'_>,
    states: &StateIds,
    pos: Pos,
    mut drive: Drive,
    axis: u8,
    neighbors: &[u32],
) -> Option<u32> {
    drive.axis = axis;
    drive = shape_from_neighbors(states, drive, neighbors);
    if drive.powered.is_some() {
        drive.powered = Some(world.has_neighbor_signal(pos));
    }
    states.state_of(Kind::ChainDrive(drive))
}

fn shape_from_neighbors(states: &StateIds, mut drive: Drive, neighbors: &[u32]) -> Drive {
    for (face, &state) in (0..6).zip(neighbors) {
        if state != UNLOADED_STATE {
            drive = drive.update_shape(face, from_kind(states.kind(state)));
        }
    }
    drive
}

pub(crate) fn wrench(context: &UseOnBlock<'_>, states: &StateIds) -> Option<UseResult> {
    if !context.player.is_some_and(|player| player.may_build()) {
        return None;
    }
    let pos = context.position;
    let mut positions = [pos; 7];
    for (target, offset_by) in positions.iter_mut().zip(FACES) {
        *target = offset(pos, offset_by);
    }
    let world = context.world;
    let mut ids = [0; 7];
    if !world.read(&positions, &mut ids) {
        return None;
    }
    let drive = from_kind(states.kind(ids[6]))?;
    let (sound, pitch) = if context.sneaking {
        let drops = if context.creative {
            Vec::new()
        } else {
            world.block_drops(pos, context.stack.read().as_deref())?
        };
        if !world.destroy(pos, false) {
            return Some(UseResult::Fail);
        }
        for stack in drops {
            context.give(&stack);
        }
        ("minecraft:block.stone.break", 0.8)
    } else {
        let around = if drive.part == Part::None {
            face_axis(context.face)
        } else {
            drive.connection_axis()
        };
        let axis = if around == drive.axis {
            drive.axis
        } else {
            3 - around - drive.axis
        };
        let base = Drive {
            axis,
            part: Part::None,
            along_first: false,
            ..drive
        };
        let next = shape_from_neighbors(states, base, &ids[..6]);
        if next == drive {
            return Some(UseResult::Success);
        }
        let state = states.state_of(Kind::ChainDrive(next))?;
        if world.write(&[BlockUpdate {
            position: pos,
            state,
            flags: UPDATE_ALL,
        }]) != 1
        {
            return Some(UseResult::Fail);
        }
        ("minecraft:block.iron_trapdoor.close", 1.0)
    };
    if let Some(effects) = EFFECTS.get() {
        effects.play_sound(
            world,
            [pos.x, pos.y, pos.z].map(|n| f64::from(n) + 0.5),
            sound,
            SOUND_BLOCKS,
            1.0,
            pitch,
        );
    }
    Some(UseResult::Success)
}

pub(crate) fn refresh(world: World<'_>, states: &StateIds, pos: Pos) {
    let mut positions = [pos; 7];
    for (position, step) in positions.iter_mut().skip(1).zip(FACES) {
        *position = offset(pos, step);
    }
    let mut ids = [0; 7];
    if !world.read(&positions, &mut ids) {
        return;
    }
    for (i, pos) in positions.into_iter().enumerate() {
        let Some(drive) = from_kind(states.kind(ids[i])) else {
            continue;
        };
        let next = if i == 0 {
            shape_from_neighbors(states, drive, &ids[1..])
        } else {
            drive.update_shape((i as u8 - 1) ^ 1, from_kind(states.kind(ids[0])))
        };
        if next != drive
            && let Some(state) = states.state_of(Kind::ChainDrive(next))
        {
            world.write(&[BlockUpdate {
                position: pos,
                state,
                flags: UPDATE_CLIENTS | UPDATE_KNOWN_SHAPE,
            }]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotating_vertical_chain_through_180_around_its_shaft_reverses_its_end_caps() {
        let start = PaletteEntry::parse(
            "create:encased_chain_drive[axis=x,axis_along_first=true,part=start]",
        );
        let after = transform(
            &start,
            StructureTransform {
                anchor: Pos { x: 0, y: 0, z: 0 },
                axis: 0,
                angle: 180.0,
            },
        )
        .expect("rotated chain drive");
        assert_eq!(after.properties["axis"], "x");
        assert_eq!(after.properties["axis_along_first"], "true");
        assert_eq!(after.properties["part"], "end");
    }
}

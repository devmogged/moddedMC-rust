//! Create 6.0.10's PulleyBlockEntity and PulleyContraption.

use std::collections::{HashSet, VecDeque};

use moddedmc_api::{
    Pos, RegistrationError, Registry,
    block_displays::BlockDisplay,
    content::{self, Block, BlockUpdate, Content, ShapeBox, State, UNLOADED_STATE, World},
    interaction::{BlockUseResult, UseBlock},
};
use serde::{Deserialize, Serialize};

use crate::{
    AXES, BLOCK_DISPLAYS, CONTENT, STONE, StateIds, collider,
    contraption::{
        BlockMovement, BlockMovementChecks, Blocks, ContraptionDisplays, MAX_ASSEMBLY_DATA_BYTES,
        StructureTransform,
    },
    elevator::{self, ElevatorMotion},
    kinetics::{Kind, Kinetics, actuator_speed, offset},
};

const ID: &str = "create:rope_pulley";
const ROPE: &str = "create:rope";
const MAGNET: &str = "create:pulley_magnet";
const MAX_ROPE_LENGTH: i32 = 384;
const DOWN: u8 = 0;
const UP: u8 = 1;

pub fn register(content: Content, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
    let states = [0, 2].map(|axis| State {
        properties: if axis == 0 { "axis=x" } else { "axis=z" },
        physical: STONE,
        model: "create:block/rope_pulley/block",
        rotating_model: "",
        x: 0,
        y: if axis == 0 { 90 } else { 0 },
        orientation: crate::relay_orientation(axis),
        shape: &[],
    });
    content.add_block(
        registry,
        &Block {
            id: ID,
            display_name: "Rope Pulley",
            entity: ID,
            item_model: "create:item/rope_pulley",
            states: &states,
        },
        Some(crate::placed_state),
    )?;
    let pole = ShapeBox {
        min: [6, 0, 6],
        max: [10, 16, 10],
    };
    let magnet = [
        pole,
        ShapeBox {
            min: [3, -3, 3],
            max: [13, 2, 13],
        },
    ];
    for (id, name, model, shape) in [
        (
            ROPE,
            "Rope",
            "create:block/rope_pulley/rope",
            std::slice::from_ref(&pole),
        ),
        (
            MAGNET,
            "Pulley Magnet",
            "create:block/rope_pulley/pulley_magnet",
            magnet.as_slice(),
        ),
    ] {
        let states = [false, true].map(|wet| State {
            properties: if wet {
                "waterlogged=true"
            } else {
                "waterlogged=false"
            },
            physical: if wet {
                "minecraft:oak_stairs[waterlogged=true]"
            } else {
                STONE
            },
            model,
            rotating_model: "",
            x: 0,
            y: 0,
            orientation: [0.0, 0.0, 0.0, 1.0],
            shape,
        });
        content.add_block(
            registry,
            &Block {
                id,
                display_name: name,
                entity: "",
                item_model: "",
                states: &states,
            },
            None,
        )?;
    }
    Ok(())
}

pub fn add_states(content: Content, ids: &mut StateIds) {
    for axis in [0, 2] {
        ids.add(
            content,
            ID,
            &format!("axis={}", AXES[usize::from(axis)]),
            Some(Kind::Pulley(axis)),
        );
    }
    for id in [ROPE, MAGNET] {
        for wet in [false, true] {
            ids.add(content, id, &format!("waterlogged={wet}"), None);
        }
    }
}

pub fn placed_state(axis: u8) -> Option<u32> {
    CONTENT
        .get()?
        .state(&format!("{ID}[axis={}]", AXES[usize::from(axis)]))
}

fn below(pos: Pos, distance: i32) -> Pos {
    offset(pos, [0, -distance, 0])
}

fn state_at(world: World<'_>, pos: Pos) -> u32 {
    let mut state = [UNLOADED_STATE];
    world.read(&[pos], &mut state);
    state[0]
}

fn is_rope(states: &StateIds, state: u32) -> bool {
    states
        .by_id
        .get(&state)
        .is_some_and(|entry| matches!(entry.block, ROPE | MAGNET))
}

fn rope_state(id: &str, wet: bool) -> Option<u32> {
    CONTENT.get()?.state(&format!("{id}[waterlogged={wet}]"))
}

fn extension_range(world: World<'_>, pos: Pos) -> i32 {
    // The loaded controller and the rope column are in the same chunk.
    let (mut low, mut high) = (0, MAX_ROPE_LENGTH);
    while low < high {
        let middle = (low + high + 1) / 2;
        if state_at(world, below(pos, middle + 1)) == UNLOADED_STATE {
            high = middle - 1;
        } else {
            low = middle;
        }
    }
    low
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct PulleyData {
    running: bool,
    initial_offset: i32,
    offset: f32,
    extension_range: i32,
    blocks: Blocks,
    #[serde(skip_serializing_if = "Option::is_none")]
    elevator: Option<ElevatorMotion>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mirror_parent: Option<[i32; 3]>,
}

#[derive(Clone, Debug, Default)]
pub struct RopePulley {
    pub data: PulleyData,
    displays: ContraptionDisplays,
    cable_displays: [i32; 2],
    assemble_next_tick: bool,
    previous_speed: f32,
    previous_elevator_speed: f32,
}

impl RopePulley {
    fn assemble(
        &mut self,
        world: World<'_>,
        states: &StateIds,
        pos: Pos,
        speed: f32,
        kinetics: &mut Kinetics,
    ) -> bool {
        let range = extension_range(world, pos);
        let is_elevator = matches!(kinetics.kind(pos), Some(Kind::Elevator(_)));
        let initial = if is_elevator {
            (1..=range)
                .take_while(|&distance| {
                    let pos = below(pos, distance);
                    states.replaceable.contains(&state_at(world, pos))
                        || world
                            .collision_shape(pos)
                            .is_some_and(|shape| shape.is_empty())
                })
                .count() as i32
        } else {
            (1..=range)
                .take_while(|&distance| is_rope(states, state_at(world, below(pos, distance))))
                .count() as i32
        };
        if !is_elevator && ((initial == 0 && speed < 0.0) || (initial >= range && speed > 0.0)) {
            return false;
        }
        let checks = &states.contraption_blocks;
        let anchor = below(pos, initial + 1);
        let mut blocks = match checks.classify(world, anchor) {
            Some(BlockMovement::Movable(entry)) => {
                let anchors = (0..=initial).map(|distance| below(pos, distance)).collect();
                let frontier = VecDeque::from([(anchor, entry)]);
                let Some((blocks, _)) =
                    checks.collect_attached(world, anchor, frontier, anchors, None)
                else {
                    return false;
                };
                blocks
            }
            Some(BlockMovement::Air) => Blocks::default(),
            Some(BlockMovement::Immovable) if speed < 0.0 => Blocks::default(),
            _ => return false,
        };
        let elevator = if is_elevator {
            let Some(motion) = ElevatorMotion::from_blocks(&blocks, anchor) else {
                return false;
            };
            if kinetics
                .positions(|kind| matches!(kind, Kind::Elevator(_)))
                .into_iter()
                .any(|other| {
                    kinetics
                        .get_mut(other)
                        .and_then(|be| be.pulley.as_ref())
                        .and_then(|pulley| pulley.data.elevator.as_ref())
                        .is_some_and(|other| motion.same_column(other))
                })
            {
                return false;
            }
            Some(motion)
        } else {
            None
        };
        if blocks.capture_entities(world, anchor, kinetics).is_none() {
            return false;
        }
        let sources: HashSet<_> = blocks
            .positions(anchor)
            .chain((1..=initial).map(|distance| below(pos, distance)))
            .collect();
        let direction = if speed > 0.0 { DOWN } else { UP };
        if !is_elevator
            && blocks.is_colliding_with_world(
                world,
                below(anchor, if speed > 0.0 { 1 } else { -1 }),
                direction,
                &sources,
                None,
            )
        {
            return false;
        }
        let Some((air, water)) = checks.state("minecraft:air").zip(
            CONTENT
                .get()
                .and_then(|content| content.state("minecraft:water")),
        ) else {
            return false;
        };
        let Some(removals) = blocks.removals(anchor, checks) else {
            return false;
        };
        let rope_length = if is_elevator { 0 } else { initial };
        let remove: Vec<_> = (1..=rope_length)
            .rev()
            .map(|distance| {
                let position = below(pos, distance);
                let wet = world
                    .fluid_id(position)
                    .is_some_and(|id| id == "minecraft:water");
                BlockUpdate {
                    position,
                    state: if wet { water } else { air },
                    flags: content::UPDATE_MOVED | content::UPDATE_CLIENTS,
                }
            })
            .chain(removals)
            .collect();
        let mut data = PulleyData {
            running: true,
            offset: initial as f32,
            initial_offset: initial,
            extension_range: range,
            blocks,
            elevator,
            mirror_parent: None,
        };
        if !fastnbt::to_bytes(&data).is_ok_and(|bytes| bytes.len() <= MAX_ASSEMBLY_DATA_BYTES)
            || (!remove.is_empty() && world.write(&remove) != remove.len())
        {
            return false;
        }
        data.blocks.detach_glue(world);
        crate::contraption::detach_moved_belts(world, &remove, kinetics);
        if let Some(motion) = &mut data.elevator {
            Self::start_mirrors(&data.blocks, anchor, pos, kinetics);
            motion.gather_all(world, states, pos.y + motion.contact_y - 1, range);
            motion.trigger_contact(world, states, true);
        }
        self.data = data;
        true
    }

    fn start_mirrors(blocks: &Blocks, anchor: Pos, pos: Pos, kinetics: &mut Kinetics) {
        let occupied: HashSet<_> = blocks.positions(anchor).collect();
        for block in occupied.iter().filter(|block| block.y == anchor.y) {
            if occupied.contains(&offset(*block, [0, 1, 0])) {
                continue;
            }
            let other = Pos { y: pos.y, ..*block };
            if other != pos
                && let Some(be) = kinetics.get_mut(other)
                && matches!(be.kind, Kind::Elevator(_))
                && let Some(pulley) = &mut be.pulley
                && !pulley.data.running
                && pulley.data.mirror_parent.is_none()
            {
                pulley.data.mirror_parent = Some([pos.x, pos.y, pos.z]);
                be.dirty = true;
            }
        }
    }

    fn grid_offset(&self, offset: f32) -> i32 {
        ((offset + 0.5).floor() as i32).clamp(0, self.data.extension_range)
    }

    fn disassemble(
        &mut self,
        world: World<'_>,
        checks: &BlockMovementChecks,
        pos: Pos,
        removed: bool,
    ) -> bool {
        if !self.data.running {
            return true;
        }
        let length = self.grid_offset(self.data.offset);
        let rope = if removed || self.data.elevator.is_some() {
            0
        } else {
            length
        };
        let Some(rope) = (1..=rope)
            .rev()
            .map(|distance| {
                let position = below(pos, distance);
                let current = state_at(world, position);
                let wet = world
                    .fluid_id(position)
                    .is_some_and(|id| id == "minecraft:water");
                // shortcut: preserve the assembly if a new block obstructs the rope during motion.
                if !checks.is_air(current) && !wet {
                    return None;
                }
                let state = rope_state(if distance == length { MAGNET } else { ROPE }, wet)?;
                Some(BlockUpdate {
                    position,
                    state,
                    flags: content::UPDATE_MOVED | content::UPDATE_CLIENTS,
                })
            })
            .collect::<Option<Vec<_>>>()
        else {
            return false;
        };
        let transform = StructureTransform {
            anchor: below(pos, length + 1),
            axis: 1,
            angle: 0.0,
        };
        if !self.data.blocks.place(
            world,
            checks,
            transform,
            None,
            collider::Frame::translated(below(pos, 1), [0.0, -f64::from(self.data.offset), 0.0]),
        ) {
            return false;
        }
        if !rope.is_empty() {
            world.write(&rope);
        }
        self.clear_displays(world);
        self.data = PulleyData::default();
        true
    }

    pub fn clear_displays(&mut self, world: World<'_>) {
        self.displays.clear(world);
        if let Some(displays) = BLOCK_DISPLAYS.get() {
            for id in self.cable_displays.iter_mut() {
                if *id != 0 {
                    displays.remove(world, std::mem::take(id));
                }
            }
        }
    }

    fn show(&mut self, world: World<'_>, pos: Pos) {
        let transform = StructureTransform {
            anchor: below(pos, 1),
            axis: 1,
            angle: 0.0,
        };
        self.displays.show(
            world,
            &self.data.blocks,
            transform,
            [0.0, -self.data.offset, 0.0],
        );
        self.show_cable(world, pos, self.data.offset);
    }

    fn show_cable(&mut self, world: World<'_>, pos: Pos, offset: f32) {
        let Some(displays) = BLOCK_DISPLAYS.get() else {
            return;
        };
        let length = (offset - 1.0).max(0.0);
        for ((block, y, scale), id) in [(MAGNET, offset, 1.0), (ROPE, length, length)]
            .into_iter()
            .zip(&mut self.cable_displays)
        {
            let shown = BlockDisplay {
                position: [pos.x, pos.y, pos.z].map(|coord| f64::from(coord) + 0.5),
                translation: [-0.5, -0.5 - y, -0.5],
                rotation: [0.0, 0.0, 0.0, 1.0],
                scale: [1.0, scale.max(0.001), 1.0],
                transform_interpolation: 1,
                position_interpolation: 0,
                brightness: -1,
            };
            if (*id == 0 || !displays.update(world, *id, &shown))
                && let Some(state) = rope_state(block, false)
            {
                *id = displays.spawn(world, state, &shown).unwrap_or(0);
            }
        }
    }

    fn tick_elevator(&mut self, world: World<'_>, states: &StateIds, pos: Pos, rpm: f32) -> bool {
        let Some(motion) = &mut self.data.elevator else {
            return false;
        };
        let before = self.data.offset;
        let previous_target = motion.target;
        motion.update_calls(world, states);
        let target = (pos.y - motion.target + motion.contact_y - 1) as f32;
        let diff = target - before;
        let speed = elevator::movement_speed(rpm, diff, self.previous_elevator_speed);
        self.previous_elevator_speed = speed;
        motion.arrived = diff.abs() < 0.5;
        let delta = if motion.arrived {
            if diff.abs() > 1.0 / 128.0 {
                diff * 0.25
            } else {
                diff
            }
        } else {
            speed
        };
        let next = (before + delta).clamp(0.0, self.data.extension_range as f32);
        let stalled = self.data.blocks.tick_drills(
            world,
            collider::Frame::translated(below(pos, 1), [0.0, -f64::from(before), 0.0]),
            collider::Frame::translated(below(pos, 1), [0.0, -f64::from(next), 0.0]),
        );
        if !stalled && next != before {
            let direction = if next < before { UP } else { DOWN };
            let shift = next.ceil() as i32 - i32::from(direction == UP);
            if !self.data.blocks.is_colliding_with_world(
                world,
                below(pos, shift + 1),
                direction,
                &HashSet::new(),
                None,
            ) {
                self.data.offset = next;
                collider::collide_entities(
                    world,
                    &mut self.data.blocks,
                    below(pos, 1),
                    [0.0, -f64::from(before), 0.0],
                    [0.0, -f64::from(next), 0.0],
                );
            }
        }
        if motion.arrived {
            motion.trigger_contact(world, states, true);
        }
        let dirty = stalled || before != self.data.offset || previous_target != motion.target;
        self.show(world, pos);
        dirty
    }

    fn tick(
        &mut self,
        world: World<'_>,
        states: &StateIds,
        pos: Pos,
        rpm: f32,
        kinetics: &mut Kinetics,
    ) -> bool {
        if matches!(kinetics.kind(pos), Some(Kind::Elevator(_))) {
            if let Some([x, y, z]) = self.data.mirror_parent {
                let parent = Pos { x, y, z };
                if let Some(pulley) = kinetics.get_mut(parent).and_then(|be| be.pulley.as_ref())
                    && pulley.data.elevator.is_some()
                {
                    self.show_cable(world, pos, pulley.data.offset);
                    return false;
                }
                self.clear_displays(world);
                if state_at(world, parent) != UNLOADED_STATE {
                    self.data.mirror_parent = None;
                    return true;
                }
                return false;
            }
            let clicked = std::mem::take(&mut self.assemble_next_tick);
            if !self.data.running {
                return clicked
                    && rpm != 0.0
                    && self.assemble(world, states, pos, actuator_speed(rpm), kinetics);
            }
            if clicked && self.disassemble(world, &states.contraption_blocks, pos, false) {
                self.previous_elevator_speed = 0.0;
                return true;
            }
            return self.tick_elevator(world, states, pos, rpm);
        }
        let previous = std::mem::replace(&mut self.previous_speed, rpm);
        let speed = actuator_speed(rpm);
        let queued = std::mem::take(&mut self.assemble_next_tick) || previous != rpm;
        if !self.data.running {
            return queued && rpm != 0.0 && self.assemble(world, states, pos, speed, kinetics);
        }
        let checks = &states.contraption_blocks;
        if speed == 0.0 {
            if self.disassemble(world, checks, pos, false) {
                return true;
            }
            self.show(world, pos);
            return false;
        }
        let next = self.data.offset + speed;
        let direction = if speed < 0.0 { UP } else { DOWN };
        if self.data.blocks.block_list.is_empty()
            && speed > 0.0
            && next as i32 != self.data.offset as i32
        {
            let under = below(pos, next as i32 + 1);
            if !checks.is_air(state_at(world, under)) && self.disassemble(world, checks, pos, false)
            {
                self.assemble_next_tick = true;
                return true;
            }
        }
        let before = self.data.offset;
        let extent = self.data.extension_range as f32;
        let next = next.clamp(0.0, extent);
        let stalled = self.data.blocks.tick_drills(
            world,
            collider::Frame::translated(below(pos, 1), [0.0, -f64::from(before), 0.0]),
            collider::Frame::translated(below(pos, 1), [0.0, -f64::from(next), 0.0]),
        );
        let shift = -(-next).floor() as i32 - i32::from(direction == UP);
        let collision = !stalled
            && self.data.blocks.is_colliding_with_world(
                world,
                below(pos, shift + 1),
                direction,
                &HashSet::new(),
                None,
            );
        if stalled {
            self.data.offset = before;
        } else if collision {
            self.data.offset =
                self.grid_offset(self.grid_offset(self.data.offset) as f32 - speed) as f32;
        } else {
            self.data.offset = next;
        }
        collider::collide_entities(
            world,
            &mut self.data.blocks,
            below(pos, 1),
            [0.0, -f64::from(before), 0.0],
            [0.0, -f64::from(self.data.offset), 0.0],
        );
        if !stalled
            && (collision || next == 0.0 || next == extent)
            && self.disassemble(world, checks, pos, false)
        {
            return true;
        }
        self.show(world, pos);
        true
    }
}

pub fn use_item_on(kinetics: &mut Kinetics, context: &UseBlock<'_>) -> BlockUseResult {
    let is_elevator = matches!(kinetics.kind(context.position), Some(Kind::Elevator(_)));
    if is_elevator && (context.sneaking || !context.player.is_some_and(|player| player.may_build()))
    {
        return BlockUseResult::Fail;
    }
    if context.sneaking || context.stack.read().is_some() {
        return BlockUseResult::TryEmptyHand;
    }
    let mut target = context.position;
    if is_elevator
        && let Some([x, y, z]) = kinetics
            .get_mut(target)
            .and_then(|be| be.pulley.as_ref())
            .and_then(|pulley| pulley.data.mirror_parent)
    {
        target = Pos { x, y, z };
    }
    if let Some(pulley) = kinetics.get_mut(target).and_then(|be| be.pulley.as_mut()) {
        pulley.assemble_next_tick = true;
    }
    BlockUseResult::Success
}

pub fn event(world: World<'_>, states: &StateIds, event: &content::ContentEvent) {
    if event.reason != content::STATE_CHANGED
        || !is_rope(states, event.previous)
        || is_rope(states, event.state)
    {
        return;
    }
    for direction in [-1, 1] {
        let pos = below(event.position, direction);
        if is_rope(states, state_at(world, pos)) {
            world.destroy(pos, true);
        }
    }
}

pub fn tick(world: World<'_>, states: &StateIds, kinetics: &mut Kinetics) {
    for (pos, mut pulley) in kinetics.dropped_pulleys.drain(..) {
        let rope = below(pos, 1);
        if pulley.data.elevator.is_none()
            && pulley.data.mirror_parent.is_none()
            && is_rope(states, state_at(world, rope))
        {
            world.destroy(rope, true);
        }
        if !pulley.disassemble(world, &states.contraption_blocks, pos, true) {
            pulley.clear_displays(world);
            pulley.data.blocks.drop_contents(world, pos, |item| item);
        }
    }
    for pos in kinetics.positions(|kind| matches!(kind, Kind::Pulley(_) | Kind::Elevator(_))) {
        let Some(be) = kinetics.get_mut(pos) else {
            continue;
        };
        let rpm = be.get_speed();
        let Some(mut pulley) = be.pulley.take() else {
            continue;
        };
        let dirty = pulley.tick(world, states, pos, rpm, kinetics);
        if let Some(be) = kinetics.get_mut(pos) {
            be.pulley = Some(pulley);
            be.dirty |= dirty;
        }
    }
}

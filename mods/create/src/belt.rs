//! Create 6.0.10's mechanical belt: `BeltBlock` states, `BeltConnectorItem` and the belt's
//! chain fields. The chain logic itself lives in `kinetics` next to the propagator it feeds.

use moddedmc_api::{
    Pos, RegistrationError, Registry,
    content::{self, Block, BlockUpdate, Content, ShapeBox, State, World},
    interaction::{UseOnBlock, UseResult},
};

use crate::{
    AXES, DIRECTIONS, SHAFT, StateIds,
    kinetics::{Belt, Kind, Part, Slope, face_axis, offset},
    states, with_world,
};

pub const BELT: &str = "create:belt";
pub const BELT_CONNECTOR: &str = "create:belt_connector";
/// `CKinetics.maxBeltLength` default.
const MAX_BELT_LENGTH: i32 = 20;
/// `BeltConnectorItem` adds this cooldown after each click.
const CONNECTOR_COOLDOWN: u32 = 5;
/// `Iterate.horizontalDirections` order for registration: south, west, north, east.
const HORIZONTAL: [u8; 4] = [3, 4, 2, 5];

/// Every `HORIZONTAL_FACING`, `SLOPE`, `PART` and `WATERLOGGED` combination. `CASING` is left
/// out until casing application is ported, which saves 160 carrier states.
fn all_states() -> impl Iterator<Item = (Belt, bool)> {
    HORIZONTAL.into_iter().flat_map(|facing| {
        Slope::ALL.into_iter().flat_map(move |slope| {
            Part::ALL.into_iter().flat_map(move |part| {
                [false, true].map(move |wet| {
                    (
                        Belt {
                            facing,
                            slope,
                            part,
                        },
                        wet,
                    )
                })
            })
        })
    })
}

pub fn properties(belt: Belt, wet: bool) -> String {
    format!(
        "facing={},part={},slope={},waterlogged={wet}",
        DIRECTIONS[usize::from(belt.facing)],
        belt.part.name(),
        belt.slope.name()
    )
}

/// `AngleHelper.horizontalAngle`.
const fn horizontal_angle(facing: u8) -> i16 {
    match facing {
        2 => 180,
        4 => 270,
        5 => 90,
        _ => 0,
    }
}

/// The top partial `BeltRenderer` draws for this state, with the blockstate rotation that
/// matches its pose. Sideways belts also need a Z rotation that blockstates can't express.
fn model(belt: Belt) -> (&'static str, i16, i16) {
    let diagonal = belt.slope.diagonal();
    let (mut start, mut end) = (belt.part == Part::Start, belt.part == Part::End);
    let positive = matches!(belt.facing, 3 | 5);
    if belt.slope == Slope::Downward || belt.slope == Slope::Vertical && positive {
        std::mem::swap(&mut start, &mut end);
    }
    let model = match (diagonal, start, end) {
        (true, true, _) => "create:block/belt/diagonal_start",
        (true, _, true) => "create:block/belt/diagonal_end",
        (true, ..) => "create:block/belt/diagonal_middle",
        (false, true, _) => "create:block/belt/start",
        (false, _, true) => "create:block/belt/end",
        (false, ..) => "create:block/belt/middle",
    };
    let pose_y = horizontal_angle(belt.facing)
        + if belt.slope == Slope::Upward { 180 } else { 0 }
        + if belt.slope == Slope::Sideways {
            270
        } else {
            0
        };
    let pose_x = if !diagonal && belt.slope != Slope::Horizontal {
        90
    } else {
        0
    };
    // Blockstate rotations turn the other way round from PoseStack's.
    (model, (360 - pose_x) % 360, (720 - pose_y) % 360)
}

/// `BeltRenderer`'s pulley pose as a quaternion, before it spins on the belt's axis.
fn pulley_orientation(axis: u8) -> [f32; 4] {
    const H: f32 = std::f32::consts::FRAC_1_SQRT_2;
    match axis {
        0 => [0.5, 0.5, -0.5, 0.5],
        1 => [1.0, 0.0, 0.0, 0.0],
        _ => [H, 0.0, 0.0, H],
    }
}

/// The flat belt halves from `BeltShapes`, as one box per state.
fn shape(belt: Belt) -> ShapeBox {
    let along_x = face_axis(belt.facing) == 0;
    let (min, max) = match (belt.slope, along_x) {
        (Slope::Vertical, false) => ([1, 0, 3], [15, 16, 13]),
        (Slope::Vertical, true) => ([3, 0, 1], [13, 16, 15]),
        (Slope::Sideways, false) => ([3, 1, 0], [13, 15, 16]),
        (Slope::Sideways, true) => ([0, 1, 3], [16, 15, 13]),
        (_, false) => ([1, 3, 0], [15, 13, 16]),
        (_, true) => ([0, 3, 1], [16, 13, 15]),
    };
    ShapeBox { min, max }
}

pub fn register(content: &Content, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
    let rows: Vec<_> = all_states()
        .map(|(belt, wet)| {
            let (model, x, y) = model(belt);
            let rotating = if belt.has_pulley() {
                "create:block/belt_pulley"
            } else {
                ""
            };
            (
                properties(belt, wet),
                model,
                x,
                y,
                rotating,
                belt,
                [shape(belt)],
            )
        })
        .collect();
    let states: Vec<_> = rows
        .iter()
        .map(|(properties, model, x, y, rotating, belt, shape)| State {
            properties,
            physical: "minecraft:gray_wool",
            model,
            rotating_model: rotating,
            x: *x,
            y: *y,
            orientation: pulley_orientation(Kind::Belt(*belt).axis()),
            shape,
        })
        .collect();
    content.add_block(
        registry,
        &Block {
            id: BELT,
            display_name: "Mechanical Belt",
            entity: "create:belt",
            item_model: "create:item/belt_connector",
            states: &states,
        },
        None,
    )
}

pub fn add_states(ids: &mut StateIds, content: Content) {
    for (belt, wet) in all_states() {
        ids.add(
            content,
            BELT,
            &properties(belt, wet),
            Some(Kind::Belt(belt)),
        );
    }
}

/// `BeltConnectorItem.useOn`.
pub fn use_on_block(context: &mut UseOnBlock<'_>) -> UseResult {
    if context.item != BELT_CONNECTOR {
        return UseResult::Pass;
    }
    if context.player.is_some() && context.sneaking {
        context.stack.set_data(&[]);
        return UseResult::Success;
    }
    let Some(states) = states() else {
        return UseResult::Pass;
    };
    let world = context.world;
    let pos = context.position;
    let valid_axis = shaft_axis(world, states, pos).is_some();
    let mut first = context.stack.data().as_deref().and_then(decode_pos);
    if let Some(first_pulley) = first
        && (shaft_axis(world, states, first_pulley).is_none()
            || !closer_than(first_pulley, pos, MAX_BELT_LENGTH * 2))
    {
        context.stack.set_data(&[]);
        first = None;
    }
    if !valid_axis || context.player.is_none() {
        return UseResult::Fail;
    }
    if let Some(first) = first {
        if !can_connect(world, states, first, pos) {
            return UseResult::Fail;
        }
        if first != pos {
            create_belts(world, states, first, pos);
            if !context.creative {
                context.stack.shrink(1);
            }
        }
        if context.stack.count() > 0 {
            context.stack.set_data(&[]);
            context.cooldown(CONNECTOR_COOLDOWN);
        }
        return UseResult::Success;
    }
    context.stack.set_data(&encode_pos(pos));
    context.cooldown(CONNECTOR_COOLDOWN);
    UseResult::Success
}

fn encode_pos(pos: Pos) -> [u8; 12] {
    let mut bytes = [0; 12];
    for (chunk, value) in bytes
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip([pos.x, pos.y, pos.z])
    {
        *chunk = value.to_le_bytes();
    }
    bytes
}

fn decode_pos(bytes: &[u8]) -> Option<Pos> {
    let ([x, y, z], []) = bytes.as_chunks::<4>() else {
        return None;
    };
    Some(Pos {
        x: i32::from_le_bytes(*x),
        y: i32::from_le_bytes(*y),
        z: i32::from_le_bytes(*z),
    })
}

/// `Vec3i.closerThan`.
fn closer_than(a: Pos, b: Pos, distance: i32) -> bool {
    let d = [a.x - b.x, a.y - b.y, a.z - b.z].map(i64::from);
    d.iter().map(|d| d * d).sum::<i64>() < i64::from(distance) * i64::from(distance)
}

fn state_at(world: World<'_>, pos: Pos) -> Option<u32> {
    let mut state = [content::UNLOADED_STATE];
    (world.read(&[pos], &mut state) && state[0] != content::UNLOADED_STATE).then_some(state[0])
}

/// `BeltConnectorItem.validateAxis`: a loaded shaft, returning its axis.
fn shaft_axis(world: World<'_>, states: &StateIds, pos: Pos) -> Option<u8> {
    match states.kind(state_at(world, pos)?) {
        Some(Kind::Shaft(axis)) => Some(axis),
        _ => None,
    }
}

/// `BlockState.canBeReplaced` for the blocks a belt can run through.
fn replaceable(states: &StateIds, state: u32) -> bool {
    states.replaceable.contains(&state)
}

/// `BeltConnectorItem.canConnect`.
fn can_connect(world: World<'_>, states: &StateIds, first: Pos, second: Pos) -> bool {
    let (Some(shaft_axis), Some(second_axis)) = (
        shaft_axis(world, states, first),
        shaft_axis(world, states, second),
    ) else {
        return false;
    };
    if !closer_than(second, first, MAX_BELT_LENGTH) {
        return false;
    }
    let diff = [second.x - first.x, second.y - first.y, second.z - first.z];
    let [x, y, z] = diff.map(i32::abs);
    let sames = i32::from(x == y) + i32::from(y == z) + i32::from(z == x);
    if diff[usize::from(shaft_axis)] != 0 || sames != 1 || shaft_axis != second_axis {
        return false;
    }
    if shaft_axis == 1 && x != 0 && z != 0 {
        return false;
    }
    let speeds = with_world(world, |kinetics| {
        Some((kinetics.get(first)?.speed, kinetics.get(second)?.speed))
    });
    let Some((speed1, speed2)) = speeds else {
        return false;
    };
    if speed1.signum() != speed2.signum() && speed1 != 0.0 && speed2 != 0.0 {
        return false;
    }
    let step = diff.map(i32::signum);
    let mut current = offset(first, step);
    for _ in 0..1000 {
        if current == second {
            break;
        }
        let Some(state) = state_at(world, current) else {
            return false;
        };
        if states.kind(state) != Some(Kind::Shaft(shaft_axis)) && !replaceable(states, state) {
            return false;
        }
        current = offset(current, step);
    }
    true
}

/// `BeltConnectorItem.getFacingFromTo`.
fn facing_from_to(start: Pos, end: Pos) -> u8 {
    let along_z = start.x == end.x;
    let diff = [end.x - start.x, end.y - start.y, end.z - start.z];
    let positive = if diff[0] == 0 && diff[2] == 0 {
        diff[1] > 0
    } else if along_z {
        diff[2] > 0
    } else {
        diff[0] > 0
    };
    match (along_z, positive) {
        (true, true) => 3,
        (true, false) => 2,
        (false, true) => 5,
        (false, false) => 4,
    }
}

/// `BeltConnectorItem.getSlopeBetween`.
const fn slope_between(start: Pos, end: Pos) -> Slope {
    let dy = end.y - start.y;
    if dy == 0 {
        Slope::Horizontal
    } else if end.x != start.x || end.z != start.z {
        if dy > 0 {
            Slope::Upward
        } else {
            Slope::Downward
        }
    } else {
        Slope::Vertical
    }
}

/// `BeltConnectorItem.getBeltChainBetween`.
fn chain_between(start: Pos, end: Pos, slope: Slope, facing: u8) -> Vec<Pos> {
    let probe = Belt {
        facing,
        slope,
        part: Part::Middle,
    };
    let mut positions = vec![start];
    let mut current = start;
    for _ in 0..1000 {
        let Some(next) = probe.next_segment(current, true) else {
            break;
        };
        if next == end {
            break;
        }
        positions.push(next);
        current = next;
    }
    positions.push(end);
    positions
}

/// `BeltConnectorItem.createBelts`. Each segment replaces what was there; the kinetic block
/// entities follow from the state changes on the next tick.
fn create_belts(world: World<'_>, states: &StateIds, start: Pos, end: Pos) {
    let mut slope = slope_between(start, end);
    let mut facing = facing_from_to(start, end);
    if end.x - start.x == end.z - start.z {
        // Diagonal ties between X and Z: use the axis across the start shaft.
        let across_x = shaft_axis(world, states, start) != Some(0);
        let positive = matches!(facing, 3 | 5);
        facing = match (across_x, positive) {
            (true, true) => 5,
            (true, false) => 4,
            (false, true) => 3,
            (false, false) => 2,
        };
    }
    let positions = chain_between(start, end, slope, facing);
    let mut updates = Vec::with_capacity(positions.len());
    for &pos in &positions {
        let Some(existing) = state_at(world, pos) else {
            return;
        };
        if states.unbreakable.contains(&existing) {
            return;
        }
        let pulley = matches!(states.kind(existing), Some(Kind::Shaft(_)));
        let mut part = if pos == start {
            Part::Start
        } else if pos == end {
            Part::End
        } else {
            Part::Middle
        };
        if part == Part::Middle && pulley {
            part = Part::Pulley;
        }
        if pulley && states.kind(existing) == Some(Kind::Shaft(1)) {
            slope = Slope::Sideways;
        }
        if !replaceable(states, existing) {
            world.destroy(pos, false);
        }
        let wet = states.wet(existing);
        let belt = Belt {
            facing,
            slope,
            part,
        };
        let Some(state) = states.lookup(&format!("{BELT}[{}]", properties(belt, wet))) else {
            return;
        };
        updates.push(BlockUpdate {
            position: pos,
            state,
            flags: content::UPDATE_ALL,
        });
    }
    world.write(&updates);
}

/// The shaft (or air) a removed belt segment leaves behind, keeping its water.
pub fn leftover(states: &StateIds, kind: Option<Kind>, wet: bool) -> Option<u32> {
    match kind {
        Some(Kind::Shaft(axis)) => states.lookup(&format!(
            "{SHAFT}[axis={},waterlogged={wet}]",
            AXES[usize::from(axis)]
        )),
        _ => states.lookup(if wet {
            "minecraft:water"
        } else {
            "minecraft:air"
        }),
    }
}

//! Steam Engine placement and linkage to its tank and powered shaft.

use std::cell::Cell;

use moddedmc_api::{
    Pos, RegistrationError, Registry,
    content::{self, Block, BlockUpdate, Content, ContentEvent, Placement, ShapeBox, State, World},
    interaction::{BlockUseResult, UseBlock},
};

use crate::{
    AXES, AXIS_ROTATIONS, CONTENT, DIRECTIONS, SHAFT, StateIds,
    belt_tunnel::read,
    fluids::{runtime::is_tank, tank_runtime},
    kinetics::{FACES, Kind, Kinetics, Level, face_axis, offset, positive},
    nearest_looking_direction, relay_orientation, relay_shape,
};

pub(crate) const ID: &str = "create:steam_engine";
pub(crate) const POWERED_SHAFT: &str = "create:powered_shaft";
const SHAFT_DISTANCE: i32 = 2;

thread_local! {
    static SHAFT_PLACEMENT: Cell<Option<(Pos, u8)>> = const { Cell::new(None) };
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct EngineBlock {
    pub(crate) connection: u8,
    facing: u8,
}

impl EngineBlock {
    fn variants() -> impl Iterator<Item = (Self, bool)> {
        [1u8, 0, 2, 3, 4, 5].into_iter().flat_map(|connection| {
            (2..6u8)
                .filter(move |&facing| connection < 2 || connection == facing)
                .flat_map(move |facing| {
                    [false, true].map(move |wet| (Self { connection, facing }, wet))
                })
        })
    }

    fn properties(self, wet: bool) -> String {
        let face = match self.connection {
            0 => "ceiling",
            1 => "floor",
            _ => "wall",
        };
        format!(
            "face={face},facing={},waterlogged={wet}",
            DIRECTIONS[usize::from(self.facing)]
        )
    }

    fn model_rotation(self) -> (i16, i16) {
        let yaw = [0, 180, 270, 90][usize::from(self.facing - 2)];
        match self.connection {
            0 => (180, (yaw + 180) % 360),
            1 => (0, yaw),
            _ => (90, yaw),
        }
    }

    fn shape(self) -> [ShapeBox; 3] {
        let (bounds, turns) = match self.connection {
            0 => (
                [
                    [1, 13, 1, 15, 16, 15],
                    [3, 1, 3, 13, 16, 13],
                    [1, 3, 4, 15, 11, 12],
                ],
                u8::from(face_axis(self.facing) == 0),
            ),
            1 => (
                [
                    [1, 0, 1, 15, 3, 15],
                    [3, 0, 3, 13, 15, 13],
                    [1, 5, 4, 15, 13, 12],
                ],
                u8::from(face_axis(self.facing) == 0),
            ),
            _ => (
                [
                    [1, 1, 0, 15, 15, 3],
                    [3, 3, 0, 13, 13, 15],
                    [1, 4, 5, 15, 12, 13],
                ],
                [2, 0, 1, 3][usize::from(self.facing - 2)],
            ),
        };
        bounds.map(|[mut x0, y0, mut z0, mut x1, y1, mut z1]| {
            for _ in 0..turns {
                (x0, z0, x1, z1) = (16 - z1, x0, 16 - z0, x1);
            }
            ShapeBox {
                min: [x0, y0, z0],
                max: [x1, y1, z1],
            }
        })
    }

    pub(crate) fn tank(self, pos: Pos) -> Pos {
        offset(pos, FACES[usize::from(self.connection ^ 1)])
    }

    pub(crate) fn shaft(self, pos: Pos) -> Pos {
        offset(
            pos,
            FACES[usize::from(self.connection)].map(|step| step * SHAFT_DISTANCE),
        )
    }

    pub(crate) fn direction(self, shaft_axis: u8, reversed: bool) -> i32 {
        let forward = shaft_axis == 1 || positive(self.facing);
        if forward ^ (shaft_axis == 2) ^ reversed {
            1
        } else {
            -1
        }
    }
}

pub(crate) fn register(
    content: Content,
    registry: &mut Registry<'_>,
) -> Result<(), RegistrationError> {
    let engine_rows: Vec<_> = EngineBlock::variants()
        .map(|(block, wet)| {
            let physical = if wet {
                "minecraft:copper_grate[waterlogged=true]"
            } else {
                "minecraft:copper_block"
            };
            (
                block.properties(wet),
                physical,
                block.model_rotation(),
                block.shape(),
            )
        })
        .collect();
    let engines: Vec<_> = engine_rows
        .iter()
        .map(|(properties, physical, (x, y), shape)| State {
            properties,
            physical,
            model: "create:block/steam_engine/block",
            rotating_model: "",
            x: *x,
            y: *y,
            orientation: [0.0, 0.0, 0.0, 1.0],
            shape,
        })
        .collect();
    content.add_block(
        registry,
        &Block {
            id: ID,
            display_name: "Steam Engine",
            entity: ID,
            item_model: "create:item/steam_engine",
            states: &engines,
        },
        Some(crate::placed_state),
    )?;

    let shaft_rows: Vec<_> = [1u8, 0, 2]
        .into_iter()
        .flat_map(|axis| {
            [false, true].map(move |wet| {
                let axis_name = AXES[usize::from(axis)];
                (
                    shaft_properties(axis, wet),
                    format!("minecraft:iron_chain[axis={axis_name},waterlogged={wet}]"),
                    axis,
                    relay_shape(Kind::Shaft(axis)),
                )
            })
        })
        .collect();
    let shafts: Vec<_> = shaft_rows
        .iter()
        .map(|(properties, physical, axis, shape)| {
            let (x, y) = AXIS_ROTATIONS[usize::from(*axis)];
            State {
                properties,
                physical,
                model: "",
                rotating_model: "create:block/powered_shaft",
                x,
                y,
                orientation: relay_orientation(*axis),
                shape,
            }
        })
        .collect();
    content.add_block(
        registry,
        &Block {
            id: POWERED_SHAFT,
            display_name: "Powered Shaft",
            entity: POWERED_SHAFT,
            item_model: "",
            states: &shafts,
        },
        None,
    )
}

pub(crate) fn place(args: Placement<'_>) -> Option<u32> {
    let connection = args.face ^ 1;
    let facing = if connection < 2 {
        nearest_looking_direction(args.yaw, 0.0)
    } else {
        connection
    };
    let block = EngineBlock { connection, facing };
    if !is_tank(read(args.world, block.tank(args.position))) {
        return None;
    }
    let wet = args.world.fluid_id(args.position)? == "minecraft:water";
    CONTENT
        .get()?
        .state(&format!("{ID}[{}]", block.properties(wet)))
}

pub(crate) fn add_states(content: Content, states: &mut StateIds) {
    for (block, wet) in EngineBlock::variants() {
        let properties = block.properties(wet);
        states.add(content, ID, &properties, None);
        if let Some(state) = states.lookup(&format!("{ID}[{properties}]")) {
            states.steam_engines.insert(state, block);
        }
    }
    for axis in 0..3 {
        for wet in [false, true] {
            states.add(
                content,
                POWERED_SHAFT,
                &shaft_properties(axis, wet),
                Some(Kind::PoweredShaft(axis)),
            );
        }
    }
}

fn shaft_properties(axis: u8, wet: bool) -> String {
    format!("axis={},waterlogged={wet}", AXES[usize::from(axis)])
}

pub(crate) fn convert_shaft(world: World<'_>, pos: Pos, axis: u8, powered: bool) -> bool {
    let block = if powered { POWERED_SHAFT } else { SHAFT };
    let Some(fluid) = world.fluid_id(pos) else {
        return false;
    };
    let properties = shaft_properties(axis, fluid == "minecraft:water");
    let Some(state) = CONTENT
        .get()
        .and_then(|content| content.state(&format!("{block}[{properties}]")))
    else {
        return false;
    };
    world.write(&[BlockUpdate {
        position: pos,
        state,
        flags: content::UPDATE_ALL,
    }]) == 1
}
const WARMUP: u8 = 3;

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(default, rename_all = "PascalCase")]
pub(crate) struct PoweredShaftData {
    direction: i32,
    warmup: u8,
    engine_pos: Option<fastnbt::IntArray>,
    engine_power: f32,
    engine_type: Option<String>,
}

impl Default for PoweredShaftData {
    fn default() -> Self {
        Self {
            direction: 1,
            warmup: WARMUP,
            engine_pos: None,
            engine_power: 0.0,
            engine_type: None,
        }
    }
}

impl PoweredShaftData {
    pub(crate) fn output(&self) -> (i32, f32) {
        crate::fluids::boiler::shaft_output(self.engine_power, self.direction)
    }

    fn owner(&self, shaft: Pos) -> Option<Pos> {
        let &[x, y, z] = self.engine_pos.as_ref()?.as_ref() else {
            return None;
        };
        // Relative offsets always span two blocks along a single axis.
        if !FACES
            .iter()
            .any(|step| step.map(|i| i * SHAFT_DISTANCE) == [x, y, z])
        {
            return None;
        }
        Some(offset(shaft, [-x, -y, -z]))
    }

    fn update(&mut self, shaft: Pos, engine: Pos, direction: i32, efficiency: f32) -> bool {
        if self.owner(shaft) == Some(engine)
            && self.direction == direction
            && self.engine_power == efficiency
        {
            return false;
        }
        self.engine_pos = Some(fastnbt::IntArray::new(vec![
            shaft.x - engine.x,
            shaft.y - engine.y,
            shaft.z - engine.z,
        ]));
        self.engine_type = Some(ID.to_owned());
        self.direction = direction;
        self.engine_power = efficiency;
        true
    }

    fn clear(&mut self) {
        *self = Self {
            direction: 0,
            warmup: 0,
            ..Self::default()
        };
    }
}

pub(crate) fn attached_engine(
    world: World<'_>,
    states: &StateIds,
    shaft: Pos,
    axis: u8,
) -> Option<Pos> {
    (0..6u8)
        .filter(|&face| face_axis(face) != axis)
        .find_map(|face| {
            let candidate = offset(
                shaft,
                FACES[usize::from(face)].map(|step| step * SHAFT_DISTANCE),
            );
            let engine = states.steam_engines.get(&read(world, candidate))?;
            (engine.shaft(candidate) == shaft).then_some(candidate)
        })
}

pub(crate) fn shaft_state(states: &StateIds, pos: Pos, axis: u8, wet: bool) -> Option<u32> {
    let axis = SHAFT_PLACEMENT.with(|placement| {
        placement
            .get()
            .filter(|&(target, _)| target == pos)
            .map_or(axis, |(_, axis)| axis)
    });
    // The host checks placement against the held block's shape. Its state callback must keep
    // that block ID. The engine converts the placed shaft once it observes the new block.
    states.lookup(&format!("{SHAFT}[{}]", shaft_properties(axis, wet)))
}
#[derive(Clone, Copy)]
pub(crate) struct Engine {
    block: EngineBlock,
    reversed: bool,
}

impl Engine {
    fn load(world: World<'_>, pos: Pos, block: EngineBlock) -> Self {
        let reversed = world.entity(pos)
            .and_then(|bytes| fastnbt::from_bytes::<fastnbt::Value>(&bytes).ok())
            .is_some_and(|data| {
                matches!(data, fastnbt::Value::Compound(fields) if fields.get("ScrollValue") == Some(&fastnbt::Value::Int(1)))
            });
        Self { block, reversed }
    }

    fn set_reversed(&mut self, world: World<'_>, pos: Pos, reversed: bool) -> bool {
        let tag = fastnbt::nbt!({ "ScrollValue": i32::from(reversed) });
        if !fastnbt::to_bytes(&tag).is_ok_and(|bytes| world.set_entity(pos, &bytes)) {
            return false;
        }
        self.reversed = reversed;
        true
    }
}

pub(crate) fn tick(
    world: World<'_>,
    states: &StateIds,
    kinetics: &mut Kinetics,
    level: &mut dyn Level,
    events: &[ContentEvent],
    time: u64,
) {
    for event in events {
        match event.reason {
            content::UNLOADED => {
                kinetics.steam_engines.remove(&event.position);
            }
            content::LOADED | content::STATE_CHANGED | content::MOVED_STATE_CHANGED => {
                if let Some(&block) = states.steam_engines.get(&event.state) {
                    kinetics
                        .steam_engines
                        .insert(event.position, Engine::load(world, event.position, block));
                } else if states.steam_engines.contains_key(&event.previous) {
                    kinetics.steam_engines.remove(&event.position);
                }
            }
            _ => {}
        }
    }

    let setters = kinetics.positions(|kind| matches!(kind, Kind::PoweredShaft(_)));
    for pos in setters {
        let Some(be) = kinetics.get_mut(pos) else {
            continue;
        };
        let axis = be.kind.axis();
        let Some(shaft) = &mut be.powered_shaft else {
            continue;
        };
        if shaft.warmup > 0 {
            shaft.warmup -= 1;
            be.dirty = true;
        }
        let current = shaft.owner(pos);
        let owner_lost = current.is_some_and(|engine| {
            let state = read(world, engine);
            state != content::UNLOADED_STATE
                && states
                    .steam_engines
                    .get(&state)
                    .is_none_or(|block| block.shaft(engine) != pos)
        });
        if owner_lost {
            shaft.clear();
            kinetics.update_generated_rotation(level, pos);
        }
        if (current.is_none() || owner_lost)
            && attached_engine(world, states, pos, axis).is_none()
            && (0..6u8)
                .filter(|&face| face_axis(face) != axis)
                .all(|face| {
                    let neighbor = offset(
                        pos,
                        FACES[usize::from(face)].map(|step| step * SHAFT_DISTANCE),
                    );
                    read(world, neighbor) != content::UNLOADED_STATE
                })
        {
            convert_shaft(world, pos, axis, false);
        }
    }

    let mut engines: Vec<_> = kinetics
        .steam_engines
        .iter()
        .map(|(&pos, &engine)| (pos, engine))
        .collect();
    engines.sort_unstable_by_key(|(pos, _)| (pos.x, pos.y, pos.z));
    let mut attached = Vec::with_capacity(engines.len());
    let mut groups = std::collections::HashMap::new();
    for (pos, engine) in engines {
        let shaft = engine.block.shaft(pos);
        if let Some(Kind::Shaft(axis)) = states.kind(read(world, shaft))
            && face_axis(engine.block.connection) != axis
        {
            convert_shaft(world, shaft, axis, true);
        }
        let tank = engine.block.tank(pos);
        let owner = tank_runtime::canonical(world, tank)
            .filter(|(owner, _)| !kinetics.fluid_tanks.pending(*owner))
            .map(|(owner, tank)| {
                groups
                    .entry(owner)
                    .or_insert((tank, Vec::new()))
                    .1
                    .push(pos);
                owner
            });
        attached.push((pos, engine, tank, owner));
    }
    kinetics.boilers.loaded_tanks(world, events);
    let efficiency = kinetics.boilers.tick(world, &mut groups, |owner, pos| {
        let state = read(world, pos);
        // Block data may remain readable after the chunk stops ticking its block entities.
        if state == content::UNLOADED_STATE {
            return true;
        }
        let Some(block) = states.steam_engines.get(&state) else {
            return false;
        };
        let support = block.tank(pos);
        tank_runtime::controller(world, support).map_or_else(
            || read(world, support) == content::UNLOADED_STATE,
            |current| current == owner,
        )
    });

    let mut visible = std::collections::HashSet::new();
    for (pos, mut engine, tank, owner) in attached {
        let target = engine.block.shaft(pos);
        let Some(be) = kinetics.get_mut(target) else {
            continue;
        };
        let axis = be.kind.axis();
        if axis == face_axis(engine.block.connection) {
            continue;
        }
        let Some(shaft) = &mut be.powered_shaft else {
            continue;
        };
        if shaft.warmup > 0 || shaft.owner(target).is_some_and(|owner| owner != pos) {
            continue;
        }
        let direction;
        let power;
        let mut update_direction = false;
        if let Some(owner) = owner {
            power = efficiency[&owner];
            let mut proposed = if power == 0.0 {
                1
            } else {
                engine.block.direction(axis, false)
            };
            if engine.reversed {
                proposed = -proposed;
            }
            if be.source.is_some()
                && be.speed != 0.0
                && be.speed.is_sign_positive() != (proposed > 0)
            {
                update_direction = engine.set_reversed(world, pos, !engine.reversed);
                proposed = -proposed;
            }
            direction = proposed;
        } else if shaft.owner(target) == Some(pos)
            && shaft.engine_power != 0.0
            && read(world, tank) != content::UNLOADED_STATE
        {
            direction = 0;
            power = 0.0;
        } else {
            continue;
        }
        if shaft.update(target, pos, direction, power) {
            kinetics.update_generated_rotation(level, target);
        }
        if update_direction {
            kinetics.steam_engines.insert(pos, engine);
        }
        if let Some(be) = kinetics.get(target) {
            let rpm = be.get_speed();
            kinetics.steam_displays.entry(pos).or_default().show(
                world,
                time,
                pos,
                engine.block,
                axis,
                rpm,
            );
            visible.insert(pos);
        }
    }
    kinetics.steam_displays.retain(|pos, display| {
        if visible.contains(pos) {
            return true;
        }
        display.clear(world);
        false
    });
}

pub(crate) fn use_block(
    context: &mut UseBlock<'_>,
    states: &StateIds,
    block: EngineBlock,
) -> BlockUseResult {
    if context.sneaking || !context.player.is_some_and(|player| player.may_build()) {
        return BlockUseResult::TryEmptyHand;
    }
    let Some(held) = context
        .stack
        .read()
        .and_then(|bytes| fastnbt::from_bytes::<fastnbt::Value>(&bytes).ok())
    else {
        return BlockUseResult::TryEmptyHand;
    };
    match crate::processing_recipes::item_id(&held) {
        Some(SHAFT) => return place_shaft(context, states, block),
        Some(crate::WRENCH) => {}
        _ => return BlockUseResult::TryEmptyHand,
    }
    let world = context.world;
    let pos = context.position;
    let shaft = block.shaft(pos);
    crate::with_world(world, |kinetics| {
        if kinetics.get(shaft).is_some_and(|be| be.source.is_some()) {
            return BlockUseResult::Fail;
        }
        let engine = kinetics
            .steam_engines
            .entry(pos)
            .or_insert_with(|| Engine::load(world, pos, block));
        if !engine.set_reversed(world, pos, !engine.reversed) {
            return BlockUseResult::Fail;
        }
        context.action_bar(if engine.reversed {
            "Steam Engine direction is counterclockwise"
        } else {
            "Steam Engine direction is clockwise"
        });
        BlockUseResult::Success
    })
}

fn place_shaft(
    context: &mut UseBlock<'_>,
    states: &StateIds,
    block: EngineBlock,
) -> BlockUseResult {
    let target = block.shaft(context.position);
    let Some(view) = context.player.and_then(|player| player.view()) else {
        return BlockUseResult::Consume;
    };
    if !states.replaceable.contains(&read(context.world, target)) {
        return BlockUseResult::Consume;
    }
    let (sy, cy) = view.yaw.to_radians().sin_cos();
    let (sp, cp) = view.pitch.to_radians().sin_cos();
    let look = [sy * cp, sp, cy * cp].map(f32::abs);
    let connection = face_axis(block.connection);
    let Some(axis) = [1u8, 2, 0]
        .into_iter()
        .filter(|axis| *axis != connection)
        .max_by(|&a, &b| look[usize::from(a)].total_cmp(&look[usize::from(b)]))
    else {
        return BlockUseResult::Consume;
    };
    // The normal placement pipeline still checks the destination, including player collision.
    let placed = SHAFT_PLACEMENT.with(|hint| {
        let previous = hint.replace(Some((target, axis)));
        let placed = context.place_held_block(target, context.face);
        hint.set(previous);
        placed
    });
    if !placed {
        return BlockUseResult::Consume;
    }
    if !context.creative {
        context.stack.shrink(1);
    }
    BlockUseResult::Success
}

#[derive(Default)]
pub(crate) struct SteamDisplays {
    parts: [crate::display_model::ModelDisplay; 3],
    last_pose: Option<(u8, u8, f32)>,
}

impl SteamDisplays {
    fn show(
        &mut self,
        world: World<'_>,
        time: u64,
        pos: Pos,
        block: EngineBlock,
        axis: u8,
        rpm: f32,
    ) {
        let origin = [pos.x, pos.y, pos.z].map(|coordinate| f64::from(coordinate) + 0.5);
        let pose = (block.connection, axis, rpm);
        if rpm == 0.0 && self.last_pose == Some(pose) && !time.is_multiple_of(20) {
            return;
        }
        self.last_pose = Some(pose);

        let vertical = block.connection < 2;
        let yaw = [0.0, 0.0, 180.0, 0.0, -90.0, 90.0][usize::from(block.connection)];
        let pitch = if vertical {
            if block.connection == 0 { 180.0 } else { 0.0 }
        } else {
            90.0
        };
        let roll = if (vertical && axis == 2) || (!vertical && axis == 1) {
            -90.0
        } else {
            0.0
        };
        let base = compose(
            compose(
                spin(1, f32::to_radians(yaw)),
                spin(0, f32::to_radians(pitch)),
            ),
            spin(1, f32::to_radians(roll)),
        );
        let shaft = block.shaft(pos);
        let parity = [shaft.x, shaft.y, shaft.z][usize::from(axis)].rem_euclid(2);
        let cam_offset = if parity == 0 { 180.0 } else { 0.0 };
        let facing_axis = face_axis(block.connection);
        let flipped = (axis != 1 && ((facing_axis == 0) ^ positive(block.connection)))
            ^ (axis == 0 && block.connection == 0);
        let phase = ((time as f64 * f64::from(rpm) * 0.3 + cam_offset) % 360.0) as f32;
        let angle = if flipped { -phase } else { phase }.to_radians();

        // SteamEngineRenderer's slider-crank geometry, in block units.
        let crank = 6.0 / 16.0;
        let rod = 14.0 / 16.0;
        let (sine, cosine) = angle.sin_cos();
        let distance = f32::sqrt(rod * rod - (crank * cosine).powi(2));
        let piston = crank * sine - distance + 20.0 / 16.0;
        let linkage = (distance / rod).acos().copysign(cosine);
        let (sin_rod, cos_rod) = linkage.sin_cos();
        for (display, (model, model_offset, rotation)) in self.parts.iter_mut().zip([
            (
                "create:moddedmc/parts/steam_engine/piston",
                [0.0, piston, 0.0],
                0.0,
            ),
            (
                "create:moddedmc/parts/steam_engine/linkage",
                [0.0, 1.0 + piston + (cos_rod - 1.0) / 4.0, sin_rod / 4.0],
                linkage,
            ),
            (
                "create:moddedmc/parts/steam_engine/shaft_connector",
                [0.0, 2.0, 0.0],
                -(angle + std::f32::consts::FRAC_PI_2),
            ),
        ]) {
            let [x, y, z, _] = compose(
                compose(
                    base,
                    [model_offset[0], model_offset[1], model_offset[2], 0.0],
                ),
                [-base[0], -base[1], -base[2], base[3]],
            );
            display.show(
                world,
                model,
                &moddedmc_api::stacks::ItemDisplay {
                    position: std::array::from_fn(|i| origin[i] + f64::from([x, y, z][i])),
                    rotation: compose(base, spin(0, rotation)),
                    scale: 1.0,
                    context: 0,
                    interpolation: 1,
                },
            );
        }
    }

    fn clear(&mut self, world: World<'_>) {
        for part in &mut self.parts {
            part.clear(world);
        }
    }
}

fn spin(axis: usize, radians: f32) -> [f32; 4] {
    let (sin, cos) = (radians / 2.0).sin_cos();
    let mut rotation = [0.0, 0.0, 0.0, cos];
    rotation[axis] = sin;
    rotation
}

fn compose([ax, ay, az, aw]: [f32; 4], [bx, by, bz, bw]: [f32; 4]) -> [f32; 4] {
    [
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    ]
}

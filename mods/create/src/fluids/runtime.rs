//! Create 6.0.10 pipe placement, pressure and connected fluid transport.

use std::{
    collections::{HashMap, HashSet},
    sync::{Mutex, OnceLock, PoisonError},
};

use moddedmc_api::{
    Pos,
    content::{self, BlockUpdate, Content, ContentEvent, Placement, World},
    interaction::{BlockUseResult, UseBlock},
};

use super::{
    blocks::PipeState,
    flow_runtime::{EndpointKind, FlowRuntime},
    item_drain,
    network_graph::BlockFace,
    open_end_runtime::open_cell,
    pipe_states::{OpenFaces, update_block_state},
    pump_graph::{PumpTopology, distribute_pressure},
    spout, tank_runtime,
    valve::{self, ValveState},
};
use crate::{
    CONTENT, DIRECTIONS, FACES, face_axis,
    kinetics::{Kind as KineticKind, Kinetics},
    nearest_looking_direction, offset,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Normal(PipeState),
    Encased(OpenFaces),
    Glass { axis: u8, alt: bool, wet: bool },
    Pump { facing: u8, wet: bool },
    Valve(ValveState),
}

struct StateIndex {
    kinds: HashMap<u32, Kind>,
    normal: [[Option<u32>; 2]; 64],
    glass: [[[Option<u32>; 2]; 2]; 3],
    pump: [[Option<u32>; 2]; 6],
    water: HashSet<u32>,
    endpoints: HashMap<u32, EndpointKind>,
}

static IDS: OnceLock<StateIndex> = OnceLock::new();

fn index() -> Option<&'static StateIndex> {
    let content = *CONTENT.get()?;
    Some(IDS.get_or_init(|| StateIndex::load(content)))
}

impl StateIndex {
    fn load(content: Content) -> Self {
        let mut result = Self {
            kinds: HashMap::with_capacity(216),
            normal: [[None; 2]; 64],
            glass: [[[None; 2]; 2]; 3],
            pump: [[None; 2]; 6],
            water: HashSet::new(),
            endpoints: tank_runtime::state_ids(content)
                .into_iter()
                .map(|id| (id, EndpointKind::Tank))
                .chain(
                    content
                        .state(item_drain::ID)
                        .map(|id| (id, EndpointKind::ItemDrain)),
                )
                .chain(content.state(spout::ID).map(|id| (id, EndpointKind::Spout)))
                .collect(),
        };
        for mask in 0..64u8 {
            for wet in [false, true] {
                let pipe = PipeState::from_mask(mask, wet);
                if let Some(id) =
                    content.state(&format!("create:fluid_pipe[{}]", pipe.properties(true)))
                {
                    result.kinds.insert(id, Kind::Normal(pipe));
                    result.normal[usize::from(mask)][usize::from(wet)] = Some(id);
                }
            }
            let pipe = PipeState::from_mask(mask, false);
            if let Some(id) = content.state(&format!(
                "create:encased_fluid_pipe[{}]",
                pipe.properties(false)
            )) {
                result.kinds.insert(id, Kind::Encased(pipe.openings));
            }
        }
        for (axis, name) in [(0usize, "x"), (1, "y"), (2, "z")] {
            for alt in [false, true] {
                for wet in [false, true] {
                    if let Some(id) = content.state(&format!(
                        "create:glass_fluid_pipe[axis={name},alt={alt},waterlogged={wet}]"
                    )) {
                        result.kinds.insert(
                            id,
                            Kind::Glass {
                                axis: axis as u8,
                                alt,
                                wet,
                            },
                        );
                        result.glass[axis][usize::from(alt)][usize::from(wet)] = Some(id);
                    }
                }
            }
        }
        for (facing, name) in DIRECTIONS.into_iter().enumerate() {
            for wet in [false, true] {
                if let Some(id) = content.state(&format!(
                    "create:mechanical_pump[facing={name},waterlogged={wet}]"
                )) {
                    result.kinds.insert(
                        id,
                        Kind::Pump {
                            facing: facing as u8,
                            wet,
                        },
                    );
                    result.pump[facing][usize::from(wet)] = Some(id);
                }
            }
        }
        for valve in valve::variants() {
            if let Some(id) = content.state(&format!("{}[{}]", valve::ID, valve.properties())) {
                result.kinds.insert(id, Kind::Valve(valve));
            }
        }
        // Existing BlockWorld V1 can read vanilla water blocks. Generic
        // waterlogged states and fluid-state APIs are requested in WS-2-02.
        for level in 0..=15 {
            if let Some(id) = content.state(&format!("minecraft:water[level={level}]")) {
                result.water.insert(id);
            }
        }
        if let Some(id) = content.state("minecraft:water") {
            result.water.insert(id);
        }
        result
    }

    fn normal_id(&self, open: OpenFaces, wet: bool) -> Option<u32> {
        let mask = open
            .into_iter()
            .enumerate()
            .fold(
                0usize,
                |mask, (face, connected)| {
                    if connected { mask | (1 << face) } else { mask }
                },
            );
        self.normal[mask][usize::from(wet)]
    }

    fn can_connect_to(&self, neighbor: u32, direction: u8) -> bool {
        self.endpoints
            .get(&neighbor)
            .is_some_and(|kind| kind.accessible(direction ^ 1))
            || self
                .kinds
                .get(&neighbor)
                .is_some_and(|kind| can_connect_to(*kind, direction))
    }
}

pub fn use_item_on(context: &mut UseBlock<'_>) -> Option<BlockUseResult> {
    Some(match index()?.endpoints.get(&context.state)? {
        EndpointKind::Tank => crate::with_world(context.world, |kinetics| {
            tank_runtime::use_item_on(context, &kinetics.fluid_tanks)
        }),
        EndpointKind::ItemDrain => crate::with_world(context.world, |kinetics| {
            item_drain::use_item_on(&mut kinetics.item_drains, context)
        }),
        EndpointKind::Spout => return None,
    })
}

pub fn is_item_drain(state: u32) -> bool {
    index().is_some_and(|ids| ids.endpoints.get(&state) == Some(&EndpointKind::ItemDrain))
}

pub(super) fn is_tank(state: u32) -> bool {
    index().is_some_and(|ids| ids.endpoints.get(&state) == Some(&EndpointKind::Tank))
}

pub fn is_spout(state: u32) -> bool {
    index().is_some_and(|ids| ids.endpoints.get(&state) == Some(&EndpointKind::Spout))
}

/// FluidPipeBlock.canConnectTo for the currently registered Create fluid blocks.
/// Other mod fluid handlers and vanilla cauldron targets need the generic ABI.
fn can_connect_to(kind: Kind, direction: u8) -> bool {
    match kind {
        Kind::Normal(_) => true, // unbracketed regular pipe accepts connection
        Kind::Encased(open) => open[usize::from(direction ^ 1)],
        Kind::Glass { axis, .. } => face_axis(direction) == axis,
        Kind::Pump { facing, .. } => face_axis(direction) == face_axis(facing),
        Kind::Valve(valve) => face_axis(direction) == valve.pipe_axis(),
    }
}

fn neighbor_connections(world: World<'_>, ids: &StateIndex, pos: Pos) -> Option<OpenFaces> {
    let positions = FACES.map(|vector| offset(pos, vector));
    let mut states = [content::UNLOADED_STATE; 6];
    if !world.read(&positions, &mut states) {
        // Unloaded neighbors do not force a chunk load or reject placement.
        // Normal case is one batched read; fallback runs only at borders.
        for (face, pos) in positions.into_iter().enumerate() {
            let mut neighbor = [content::UNLOADED_STATE];
            states[face] = if world.read(&[pos], &mut neighbor) {
                neighbor[0]
            } else {
                content::UNLOADED_STATE
            };
        }
    }
    Some(std::array::from_fn(|face| {
        ids.can_connect_to(states[face], face as u8)
    }))
}

fn current_water(world: World<'_>, ids: &StateIndex, pos: Pos) -> Option<bool> {
    let mut states = [content::UNLOADED_STATE];
    if !world.read(&[pos], &mut states) {
        return None;
    }
    Some(ids.water.contains(&states[0]))
}

/// FluidPipeBlock.getStateForPlacement, GlassFluidPipeBlock.getStateForPlacement,
/// and the basic PumpBlock.getStateForPlacement axis rule.
///
/// For pipes, the host-specific FluidState and fluid capability checks are
/// intentionally kept behind neighbor_connections / current_water.
pub fn placed_state(args: &Placement<'_>) -> Option<u32> {
    let ids = index()?;
    let kind = *ids.kinds.get(&args.state)?;
    let look = nearest_looking_direction(args.yaw, args.pitch);
    let wet = current_water(args.world, ids, args.position)?;
    match kind {
        Kind::Normal(_) => {
            let neighbors = neighbor_connections(args.world, ids, args.position)?;
            let connections = update_block_state([false; 6], look, None, neighbors, false);
            ids.normal_id(connections, wet)
        }
        Kind::Glass { .. } => {
            // AxisPipeBlock inherits RotatedPillarBlock.getStateForPlacement:
            // axis is clicked face, unlike FluidPipeBlock's preferred look.
            ids.glass[usize::from(face_axis(args.face))][0][usize::from(wet)]
        }
        Kind::Pump { .. } => {
            let neighbors = neighbor_connections(args.world, ids, args.position)?;
            let facing = pump_facing(look, args.sneak, neighbors);
            ids.pump[usize::from(facing)][usize::from(wet)]
        }
        Kind::Valve(_) => {
            let facing = if args.sneak { look } else { look ^ 1 };
            let pipe = neighbor_connections(args.world, ids, args.position)?;
            let states = crate::states()?;
            let positions = FACES.map(|direction| offset(args.position, direction));
            let mut neighbors = [content::UNLOADED_STATE; 6];
            args.world.read(&positions, &mut neighbors);
            let shaft = std::array::from_fn(|side| {
                states
                    .kind(neighbors[side])
                    .is_some_and(|kind| kind.has_shaft_towards(side as u8 ^ 1))
            });
            let along_first = valve_alignment(
                face_axis(facing),
                face_axis(nearest_looking_direction(args.yaw, 0.0)),
                shaft,
                pipe,
            );
            let valve = ValveState {
                facing,
                along_first,
                enabled: false,
                wet,
            };
            CONTENT
                .get()?
                .state(&format!("{}[{}]", valve::ID, valve.properties()))
        }
        Kind::Encased(_) => Some(args.state),
    }
}

fn valve_alignment(facing_axis: u8, look_axis: u8, shaft: OpenFaces, pipe: OpenFaces) -> bool {
    if facing_axis == 1 {
        let prefer_x = shaft[4] || shaft[5] || pipe[2] || pipe[3];
        let prefer_z = shaft[2] || shaft[3] || pipe[4] || pipe[5];
        return if prefer_x == prefer_z {
            look_axis == 0
        } else {
            prefer_x
        };
    }
    let perpendicular = if facing_axis == 0 { 3 } else { 5 };
    let prefer_vertical_shaft =
        shaft[0] || shaft[1] || pipe[perpendicular] || pipe[perpendicular ^ 1];
    (facing_axis == 0) == prefer_vertical_shaft
}

/// PumpBlock.getStateForPlacement: the default is nearest-looking opposite
/// (nearest-looking when sneaking); when unsneaked, prefer a connected fluid
/// direction on a different axis. Same-distance choices use the last face.
fn pump_facing(look: u8, sneak: bool, connected: OpenFaces) -> u8 {
    let target = if sneak { look } else { look ^ 1 };
    if sneak {
        return target;
    }
    let mut closest: Option<(u8, u8)> = None;
    for direction in 0..6u8 {
        if !connected[usize::from(direction)] {
            continue;
        }
        let distance = if direction == target {
            0
        } else if direction == (target ^ 1) {
            4
        } else {
            2
        };
        if closest.is_none_or(|(_, best_distance)| distance <= best_distance) {
            closest = Some((direction, distance));
        }
    }
    match closest {
        Some((direction, _)) if face_axis(direction) != face_axis(target) => direction,
        _ => target,
    }
}

/// FluidPipeBlock.updateShape-like recomputation for a loaded neighbor event.
/// Content V1 does not identify the triggering face; unlike Java updateShape,
/// we must check all six faces (ignore=None). Bracket registration and external
/// capabilities are pending and cannot be synthesized here.
pub fn event(world: World<'_>, event: &ContentEvent) {
    if event.reason != content::NEIGHBOR_CHANGED {
        return;
    }
    let Some(ids) = index() else { return };
    // Ignore unrelated Create/vanilla neighbor events without world lookups.
    let Some(Kind::Normal(previous)) = ids.kinds.get(&event.state).copied() else {
        return;
    };
    let mut current = [content::UNLOADED_STATE];
    if !world.read(&[event.position], &mut current) || current[0] != event.state {
        return;
    }
    let Some(neighbors) = neighbor_connections(world, ids, event.position) else {
        return;
    };
    let preferred = previous
        .openings
        .into_iter()
        .position(|open| open)
        .unwrap_or(1) as u8;
    let next = update_block_state(previous.openings, preferred, None, neighbors, false);
    if next == previous.openings {
        return;
    }
    let Some(next_id) = ids.normal_id(next, previous.wet) else {
        return;
    };
    world.write(&[BlockUpdate {
        position: event.position,
        state: next_id,
        flags: content::UPDATE_ALL,
    }]);
}

fn describe(kind: Kind) -> String {
    match kind {
        Kind::Normal(state) => format!("create:fluid_pipe[{}]", state.properties(true)),
        Kind::Encased(openings) => format!(
            "create:encased_fluid_pipe[{}]",
            PipeState {
                openings,
                wet: false
            }
            .properties(false)
        ),
        Kind::Glass { axis, alt, wet } => format!(
            "create:glass_fluid_pipe[axis={},alt={alt},waterlogged={wet}]",
            ["x", "y", "z"][usize::from(axis)]
        ),
        Kind::Pump { facing, wet } => format!(
            "create:mechanical_pump[facing={},waterlogged={wet}]",
            DIRECTIONS[usize::from(facing)]
        ),
        Kind::Valve(valve) => format!("{}[{}]", valve::ID, valve.properties()),
    }
}

/// Read the exact registered fluid state for the administrator diagnostic.
/// This does not mutate the world or force chunk loading.
pub fn inspect(world: World<'_>, pos: Pos) -> Option<String> {
    let ids = index()?;
    let mut current = [content::UNLOADED_STATE];
    if !world.read(&[pos], &mut current) {
        return None;
    }
    if let Some(kind) = ids.endpoints.get(&current[0]) {
        let tank = if *kind == EndpointKind::Tank {
            tank_runtime::canonical(world, pos)?.1
        } else {
            kind.load(world, pos)?
        };
        return Some(format!(
            "{}{}",
            CONTENT.get()?.describe_state(current[0])?,
            tank_runtime::describe(&tank)
        ));
    }
    Some(describe(*ids.kinds.get(&current[0])?))
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct PumpSource {
    pos: Pos,
    facing: u8,
    speed: f32,
}

#[derive(Default)]
struct WorldPressure {
    pumps: Vec<PumpSource>,
    /// PipeConnection pressure (inbound, outward) by position and face.
    faces: HashMap<(Pos, u8), [f32; 2]>,
    flow: FlowRuntime,
}

static PRESSURES: Mutex<Option<HashMap<usize, WorldPressure>>> = Mutex::new(None);

/// Read-only block topology from the currently loaded server world.
struct LoadedTopology<'a> {
    world: World<'a>,
    states: &'static StateIndex,
}

impl LoadedTopology<'_> {
    fn read(&self, pos: Pos) -> Option<(u32, Option<Kind>)> {
        let mut current = [content::UNLOADED_STATE];
        if !self.world.read(&[pos], &mut current) || current[0] == content::UNLOADED_STATE {
            return None;
        }
        Some((current[0], self.states.kinds.get(&current[0]).copied()))
    }

    fn closed_valve(&self, pos: Pos) -> bool {
        matches!(self.read(pos), Some((_, Some(Kind::Valve(valve)))) if !valve.enabled)
    }
}

fn pipe_faces(kind: Kind) -> Option<OpenFaces> {
    match kind {
        Kind::Normal(pipe) => Some(pipe.openings),
        Kind::Encased(open) => Some(open),
        Kind::Glass { axis, .. } => Some(std::array::from_fn(|face| face_axis(face as u8) == axis)),
        Kind::Pump { .. } => None,
        Kind::Valve(valve) => Some(std::array::from_fn(|face| {
            face_axis(face as u8) == valve.pipe_axis()
        })),
    }
}

impl PumpTopology for LoadedTopology<'_> {
    fn is_loaded(&self, pos: Pos) -> bool {
        self.read(pos).is_some()
    }

    fn pipe_faces(&self, pos: Pos) -> Option<OpenFaces> {
        pipe_faces(self.read(pos)?.1?)
    }

    fn has_valid_endpoint(&self, pipe: Pos, face: u8, pull: bool) -> bool {
        if usize::from(face) >= FACES.len() {
            return false;
        }
        let connected = offset(pipe, FACES[usize::from(face)]);
        let Some((state, kind)) = self.read(connected) else {
            return false;
        };
        match kind {
            // PumpBlockEntity.hasReachedValidEndpoint: two pumps can meet
            // only along their fluid axes and when facing transfer oppositely.
            Some(Kind::Pump { facing, .. }) if face_axis(facing) == face_axis(face) => {
                let neighbor_front = facing == (face ^ 1);
                return (!neighbor_front) != pull;
            }
            // A neighboring Create transport is not an open-world source.
            Some(_) => return false,
            None => {}
        }
        if let Some(kind) = self.states.endpoints.get(&state) {
            kind.accessible(face ^ 1)
        } else {
            open_cell(
                self.world,
                BlockFace {
                    pos: pipe,
                    side: face,
                },
            )
            .is_some()
        }
    }
}

fn add_pressure(map: &mut HashMap<(Pos, u8), [f32; 2]>, face: super::pump_graph::PressureFace) {
    let slot = map.entry((face.pos, face.face)).or_insert([0.0, 0.0]);
    slot[usize::from(!face.inbound)] += face.pressure;
}

/// Recompute pump-induced PipeConnection pressure after the kinetic tick,
/// only when absolute RPM, topology, or a pump's facing changes. Java
/// PumpBlockEntity.onSpeedChanged similarly ignores sign-only changes.
/// Advance fluid networks after the pump pressure update.
pub fn pressure_tick(
    world: World<'_>,
    kinetics: &Kinetics,
    events: &[ContentEvent],
    valve_changed: bool,
) {
    let Some(states) = index() else { return };
    let mut pumps: Vec<_> = kinetics
        .positions(|kind| matches!(kind, KineticKind::Pump(_)))
        .into_iter()
        .filter_map(|pos| {
            let be = kinetics.get(pos)?;
            let KineticKind::Pump(facing) = be.kind else {
                return None;
            };
            let speed = be.get_speed().abs();
            speed
                .is_finite()
                .then_some(PumpSource { pos, facing, speed })
        })
        .collect();
    pumps.sort_by_key(|pump| (pump.pos.x, pump.pos.y, pump.pos.z, pump.facing));

    let topology_changed = events.iter().any(|event| {
        [event.previous, event.state]
            .into_iter()
            .any(|state| states.kinds.contains_key(&state) || states.endpoints.contains_key(&state))
    });
    let refresh = {
        let cache = PRESSURES.lock().unwrap_or_else(PoisonError::into_inner);
        valve_changed
            || topology_changed
            || cache
                .as_ref()
                .and_then(|worlds| worlds.get(&world.id()))
                .is_none_or(|before| before.pumps != pumps)
    };
    let topology = LoadedTopology { world, states };
    let faces = refresh.then(|| {
        let mut faces = HashMap::new();
        for pump in &pumps {
            if pump.speed == 0.0 {
                continue;
            }
            for side in [pump.facing, pump.facing ^ 1] {
                let pull = side != pump.facing;
                for face in distribute_pressure(
                    &topology, pump.pos, side, pull, pump.speed,
                    16, // CFluids.mechanicalPumpRange default (configurable in Java)
                ) {
                    add_pressure(&mut faces, face);
                }
            }
        }
        faces
    });
    let mut cache = PRESSURES.lock().unwrap_or_else(PoisonError::into_inner);
    let pressure = cache
        .get_or_insert_with(HashMap::new)
        .entry(world.id())
        .or_default();
    if let Some(faces) = faces {
        let closed_valves: HashSet<_> = kinetics
            .positions(|kind| matches!(kind, KineticKind::Valve(_)))
            .into_iter()
            .filter(|&pos| topology.closed_valve(pos))
            .collect();
        pressure.flow.update_pressure(
            faces
                .iter()
                .filter(|((pos, _), _)| !closed_valves.contains(pos))
                .map(|(&(pos, face), &values)| (pos, face, values))
                .chain(
                    pumps
                        .iter()
                        .filter(|pump| pump.speed > 0.0)
                        .flat_map(|pump| {
                            [
                                (pump.pos, pump.facing, [0.0, pump.speed]),
                                (pump.pos, pump.facing ^ 1, [pump.speed, 0.0]),
                            ]
                        }),
                ),
        );
        pressure.pumps = pumps;
        pressure.faces = faces;
    }
    pressure
        .flow
        .tick(world, &states.endpoints, &kinetics.fluid_tanks, |state| {
            states.kinds.contains_key(&state)
        });
}

pub fn tick_valves(world: World<'_>, kinetics: &mut Kinetics) -> bool {
    let Some((states, content)) = index().zip(CONTENT.get()) else {
        return false;
    };
    let topology = LoadedTopology { world, states };
    let mut changed = false;
    for pos in kinetics.positions(|kind| matches!(kind, KineticKind::Valve(_))) {
        let Some(be) = kinetics.get_mut(pos) else {
            continue;
        };
        let rpm = be.get_speed();
        let pointer = be.valve.get_or_insert_with(Default::default);
        let previous = *pointer;
        let enabled = pointer.advance(rpm);
        be.dirty |= *pointer != previous;
        let Some((_, Some(Kind::Valve(mut valve)))) = topology.read(pos) else {
            continue;
        };
        let Some(enabled) = enabled.filter(|&enabled| enabled != valve.enabled) else {
            continue;
        };
        valve.enabled = enabled;
        if let Some(state) = content.state(&format!("{}[{}]", valve::ID, valve.properties())) {
            changed |= world.write(&[BlockUpdate {
                position: pos,
                state,
                flags: content::UPDATE_ALL,
            }]) == 1;
        }
    }
    changed
}

/// Pending engine ABI integration, show computed pressure as the two
/// Create/NeoForge directional values for each pipe face.
pub fn pressure_text(world: World<'_>, pos: Pos) -> String {
    let cache = PRESSURES.lock().unwrap_or_else(PoisonError::into_inner);
    let Some(pressure) = cache.as_ref().and_then(|worlds| worlds.get(&world.id())) else {
        return String::new();
    };
    let mut parts = Vec::new();
    for (face, name) in DIRECTIONS.into_iter().enumerate() {
        if let Some([inbound, outward]) = pressure.faces.get(&(pos, face as u8))
            && (*inbound != 0.0 || *outward != 0.0)
        {
            parts.push(format!("{name}={inbound:.2}/{outward:.2}"));
        }
    }
    if parts.is_empty() {
        " Pressure(none)".into()
    } else {
        format!(" Pressure({})", parts.join(","))
    }
}

#[cfg(test)]
mod tests {
    use super::{Kind, add_pressure, can_connect_to, describe, pipe_faces, pump_facing};
    use crate::fluids::pump_graph::PressureFace;
    use moddedmc_api::Pos;
    use std::collections::HashMap;

    #[test]
    fn pump_geometry_is_not_a_traversable_pipe_and_glass_has_two_axis_faces() {
        assert!(
            pipe_faces(Kind::Pump {
                facing: 1,
                wet: false
            })
            .is_none()
        );
        let horizontal = pipe_faces(Kind::Glass {
            axis: 0,
            alt: true,
            wet: false,
        })
        .unwrap();
        assert_eq!(horizontal, [false, false, false, false, true, true]);
    }

    #[test]
    fn pressure_values_accumulate_inbound_and_outward_independently() {
        let pos = Pos { x: 1, y: 2, z: 3 };
        let mut pressures = HashMap::new();
        add_pressure(
            &mut pressures,
            PressureFace {
                pos,
                face: 0,
                inbound: true,
                pressure: 32.0,
            },
        );
        add_pressure(
            &mut pressures,
            PressureFace {
                pos,
                face: 0,
                inbound: false,
                pressure: 16.0,
            },
        );
        add_pressure(
            &mut pressures,
            PressureFace {
                pos,
                face: 0,
                inbound: true,
                pressure: 8.0,
            },
        );
        assert_eq!(pressures.get(&(pos, 0)), Some(&[40.0, 16.0]));
    }

    #[test]
    fn state_diagnostics_preserve_all_create_properties() {
        let mut openings = [false; 6];
        openings[4] = true;
        openings[5] = true;
        let normal = Kind::Normal(super::PipeState {
            openings,
            wet: true,
        });
        assert_eq!(
            describe(normal),
            "create:fluid_pipe[down=false,up=false,north=false,south=false,west=true,east=true,waterlogged=true]"
        );
        assert_eq!(
            describe(Kind::Encased(openings)),
            "create:encased_fluid_pipe[down=false,up=false,north=false,south=false,west=true,east=true]"
        );
        assert_eq!(
            describe(Kind::Glass {
                axis: 1,
                alt: true,
                wet: false
            }),
            "create:glass_fluid_pipe[axis=y,alt=true,waterlogged=false]"
        );
        assert_eq!(
            describe(Kind::Pump {
                facing: 4,
                wet: true
            }),
            "create:mechanical_pump[facing=west,waterlogged=true]"
        );
    }

    #[test]
    fn source_pipe_neighbor_rules_match_axis_and_encased_openings() {
        assert!(can_connect_to(
            Kind::Normal(super::PipeState {
                openings: [false; 6],
                wet: false
            }),
            4
        ));
        assert!(!can_connect_to(Kind::Encased([false; 6]), 5));
        let mut encased = [false; 6];
        encased[4] = true;
        assert!(can_connect_to(Kind::Encased(encased), 5));
        assert!(!can_connect_to(Kind::Encased(encased), 4));
        assert!(can_connect_to(
            Kind::Glass {
                axis: 0,
                alt: false,
                wet: false
            },
            4
        ));
        assert!(!can_connect_to(
            Kind::Glass {
                axis: 0,
                alt: true,
                wet: false
            },
            2
        ));
        assert!(can_connect_to(
            Kind::Pump {
                facing: 3,
                wet: false
            },
            2
        ));
        assert!(!can_connect_to(
            Kind::Pump {
                facing: 3,
                wet: true
            },
            0
        ));
    }
    #[test]
    fn pump_uses_opposite_look_until_a_cross_axis_neighbor_is_present() {
        assert_eq!(pump_facing(4, false, [false; 6]), 5);
        assert_eq!(pump_facing(4, true, [false; 6]), 4);
        let mut connected = [false; 6];
        connected[2] = true;
        assert_eq!(pump_facing(4, false, connected), 2);
        connected[5] = true;
        assert_eq!(pump_facing(4, false, connected), 5);
        assert_eq!(pump_facing(4, true, connected), 4);
    }
    #[test]
    fn pump_tie_breaking_matches_java_direction_iteration() {
        let mut connected = [false; 6];
        connected[0] = true;
        connected[3] = true;
        assert_eq!(pump_facing(4, false, connected), 3);
    }
}

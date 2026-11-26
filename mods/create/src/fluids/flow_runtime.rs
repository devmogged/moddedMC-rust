//! Tick the Create transport and network models against loaded fluid endpoints.

use std::collections::{HashMap, HashSet};

use moddedmc_api::{
    Pos,
    content::{self, BlockUpdate, World},
};

use super::{
    basin_tanks::BasinTransfer,
    boiler::Boilers,
    hose_pulley::{self, HosePulleys, HoseTransfer},
    item_drain,
    network::{FluidHandler, transfer_tick},
    network_graph::{BlockFace, FluidNetworkGraph, NetworkTopology, Source},
    open_end_runtime::{OpenEnd, open_cell},
    pipe_connection::{FluidStack, PipeConnection},
    smart_tank, spout,
    tank::TankInventory,
    tank_connectivity::Connectivity,
    tank_runtime,
    transport::{TickOutcome, Transport},
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum EndpointKind {
    Tank,
    ItemDrain,
    Spout,
    Hose(u8),
    Basin,
}

impl EndpointKind {
    pub fn accessible(self, face: u8) -> bool {
        match self {
            Self::Tank | Self::Basin => true,
            Self::ItemDrain => face != 1,
            Self::Spout => face != 0,
            Self::Hose(facing) => face == hose_pulley::pipe_side(facing),
        }
    }

    pub fn load(self, world: World<'_>, pos: Pos) -> Option<TankInventory> {
        match self {
            Self::Tank => tank_runtime::load(world, pos),
            Self::ItemDrain => item_drain::load(world, pos),
            Self::Spout => spout::load_tank(world, pos),
            Self::Hose(_) => hose_pulley::load_tank(world, pos),
            Self::Basin => BasinTransfer::describe(world, pos),
        }
    }

    fn save(self, world: World<'_>, pos: Pos, tank: &TankInventory) -> bool {
        match self {
            Self::Tank => tank_runtime::save(world, pos, tank),
            Self::ItemDrain | Self::Spout => smart_tank::save(world, pos, tank),
            Self::Hose(_) | Self::Basin => false,
        }
    }
}

type Endpoints = HashMap<Pos, (EndpointKind, TankInventory)>;
type EndpointOwners = HashMap<Pos, Pos>;

#[derive(Default)]
pub struct FlowRuntime {
    pipes: HashMap<Pos, Transport>,
    networks: HashMap<BlockFace, FluidNetworkGraph>,
}

impl FlowRuntime {
    pub(super) fn completed_inbound(
        &self,
        pos: Pos,
        matches: impl Fn(&FluidStack) -> bool,
    ) -> bool {
        self.pipes.get(&pos).is_some_and(|pipe| {
            pipe.connections
                .iter()
                .flatten()
                .filter_map(|connection| connection.flow.as_ref())
                .any(|flow| flow.inbound && flow.complete && matches(&flow.fluid))
        })
    }

    pub fn update_pressure(&mut self, faces: impl Iterator<Item = (Pos, u8, [f32; 2])>) {
        let mut by_pipe = HashMap::new();
        for (pos, face, pressure) in faces {
            by_pipe.entry(pos).or_insert([None; 6])[usize::from(face)] = Some(pressure);
        }
        self.pipes.retain(|pos, _| by_pipe.contains_key(pos));
        for (pos, pressures) in by_pipe {
            let connected = || (0..6u8).filter(|&side| pressures[usize::from(side)].is_some());
            let pipe = self
                .pipes
                .entry(pos)
                .or_insert_with(|| Transport::new(connected()));
            if pipe
                .connections
                .each_ref()
                .map(|conn| conn.as_ref().map(|conn| conn.pressure))
                != pressures
            {
                pipe.wipe_pressure(connected());
                for (connection, pressure) in pipe.connections.iter_mut().zip(pressures) {
                    if let (Some(connection), Some(pressure)) = (connection, pressure) {
                        connection.pressure = pressure;
                    }
                }
            }
        }
        self.networks.clear();
    }

    pub fn tick(
        &mut self,
        world: World<'_>,
        endpoint_states: &HashMap<u32, EndpointKind>,
        connectivity: &Connectivity,
        pulleys: &mut HosePulleys,
        boilers: &mut Boilers,
        is_transport: impl Fn(u32) -> bool,
    ) {
        if self.pipes.is_empty() {
            return;
        }
        let mut tanks = HashMap::new();
        let mut owners = HashMap::new();
        let mut checked = HashMap::new();
        let mut open = HashSet::new();
        let mut provided = Vec::with_capacity(self.pipes.len());
        for (&pos, pipe) in &self.pipes {
            provided.push((
                pos,
                std::array::from_fn(|side| {
                    let connection = pipe.connections[side].as_ref()?;
                    let face = BlockFace {
                        pos,
                        side: side as u8,
                    };
                    let adjacent = face.opposite();
                    if let Some(other) = self.pipes.get(&adjacent.pos) {
                        return other.connections[usize::from(adjacent.side)]
                            .as_ref()?
                            .provide_outbound_flow()
                            .cloned();
                    }
                    if let std::collections::hash_map::Entry::Vacant(entry) =
                        checked.entry(adjacent.pos)
                    {
                        let mut state = [0];
                        if !world.read(&[adjacent.pos], &mut state) {
                            state[0] = moddedmc_api::content::UNLOADED_STATE;
                        }
                        entry.insert(state[0]);
                        if let Some(&kind) = endpoint_states.get(&state[0]) {
                            let (owner, tag) = if kind == EndpointKind::Tank {
                                let tag = tank_runtime::read_tag(world, adjacent.pos)?;
                                let owner = tank_runtime::controller_from_tag(&tag, adjacent.pos)?;
                                (owner, (owner == adjacent.pos).then_some(tag))
                            } else {
                                (adjacent.pos, None)
                            };
                            if let std::collections::hash_map::Entry::Vacant(entry) =
                                tanks.entry(owner)
                                && !connectivity.pending(owner)
                                && let Some(tank) = match tag {
                                    Some(tag) => tank_runtime::from_tag(&tag),
                                    None => kind.load(world, owner),
                                }
                            {
                                entry.insert((kind, tank));
                            }
                            if tanks.get(&owner).is_some_and(|(_, tank)| {
                                kind != EndpointKind::Tank
                                    || tank_runtime::contains(owner, tank, adjacent.pos)
                            }) {
                                owners.insert(adjacent.pos, owner);
                            }
                        }
                    }
                    if let Some(&owner) = owners.get(&adjacent.pos)
                        && let Some((kind, tank)) = tanks.get(&owner)
                    {
                        if !kind.accessible(adjacent.side) || boilers.active(owner) {
                            return None;
                        }
                        return tank.fluid().cloned().or_else(|| {
                            matches!(kind, EndpointKind::Hose(_))
                                .then(|| pulleys.get(&adjacent.pos))
                                .flatten()
                                .and_then(|pulley| HoseTransfer::source_fluid(world, pulley))
                        });
                    }
                    let state = checked[&adjacent.pos];
                    if endpoint_states.contains_key(&state) || is_transport(state) {
                        return None;
                    }
                    let cell = open_cell(world, face)?;
                    open.insert(face);
                    if connection.compare_pressure() >= 0.0 {
                        return None;
                    }
                    OpenEnd::source(world, face, &cell)?.1.fluid().cloned()
                }),
            ));
        }
        for (pos, provided) in provided {
            if let Some(TickOutcome::Collision { first, second }) =
                self.pipes.get_mut(&pos).map(|pipe| pipe.tick(&provided))
                && world.destroy(pos, true)
            {
                self.pipes.remove(&pos);
                self.networks.clear();
                if let Some(state) = collision_state(&first.id, &second.id) {
                    world.write(&[BlockUpdate {
                        position: pos,
                        state,
                        flags: content::UPDATE_ALL,
                    }]);
                }
            }
        }

        let mut starts = Vec::new();
        let topology = Topology {
            pipes: &self.pipes,
            tanks: &tanks,
            owners: &owners,
            open: &open,
        };
        for (&pos, pipe) in &self.pipes {
            for connection in pipe.connections.iter().flatten() {
                let face = BlockFace {
                    pos,
                    side: connection.side,
                };
                if connection.provided_fluid().is_some()
                    && topology.source(face) == Source::Endpoint
                {
                    starts.push(face);
                }
            }
        }
        starts.sort_unstable_by_key(face_order);
        let available: HashSet<_> = starts.iter().copied().collect();
        self.networks.retain(|face, _| available.contains(face));
        for &face in &starts {
            self.networks
                .entry(face)
                .or_insert_with(|| FluidNetworkGraph::new(face))
                .tick(&topology);
        }
        for face in starts {
            let graph = &self.networks[&face];
            move_fluid(world, &mut tanks, &owners, &open, pulleys, boilers, graph);
        }
    }
}

fn collision_state(first: &str, second: &str) -> Option<u32> {
    let other = match (first, second) {
        ("minecraft:lava", other) | (other, "minecraft:lava") => other,
        _ => return None,
    };
    let result = match other {
        "minecraft:water" => "minecraft:cobblestone",
        "create:honey" => "create:limestone",
        "create:chocolate" => "create:scoria",
        _ => return None,
    };
    crate::CONTENT.get()?.state(result)
}

fn face_order(face: &BlockFace) -> (i32, i32, i32, u8) {
    (face.pos.x, face.pos.y, face.pos.z, face.side)
}

struct Topology<'a> {
    pipes: &'a HashMap<Pos, Transport>,
    tanks: &'a Endpoints,
    owners: &'a EndpointOwners,
    open: &'a HashSet<BlockFace>,
}

impl NetworkTopology for Topology<'_> {
    fn is_loaded(&self, pos: Pos) -> bool {
        self.pipes.contains_key(&pos)
    }

    fn connection(&self, face: BlockFace) -> Option<PipeConnection> {
        self.pipes.get(&face.pos)?.connections[usize::from(face.side)].clone()
    }

    fn source(&self, face: BlockFace) -> Source {
        let adjacent = face.opposite();
        if self.open.contains(&face)
            || self
                .owners
                .get(&adjacent.pos)
                .and_then(|owner| self.tanks.get(owner))
                .is_some_and(|(kind, _)| kind.accessible(adjacent.side))
        {
            Source::Endpoint
        } else if self
            .pipes
            .get(&adjacent.pos)
            .is_some_and(|pipe| pipe.connections[usize::from(adjacent.side)].is_some())
        {
            Source::Other
        } else {
            Source::Undetermined
        }
    }
}

enum Storage {
    Tank(Pos, EndpointKind),
    Boiler(Pos),
    Open(OpenEnd),
    Hose(HoseTransfer),
    Basin(BasinTransfer),
}

impl Storage {
    fn save(
        &self,
        world: World<'_>,
        tank: &TankInventory,
        pulleys: &mut HosePulleys,
        boilers: &mut Boilers,
    ) -> bool {
        match self {
            Self::Tank(pos, kind) => kind.save(world, *pos, tank),
            Self::Boiler(pos) => boilers.receive(*pos, tank.amount()),
            Self::Open(end) => end.save(world, tank),
            Self::Hose(hose) => hose.save(world, tank, pulleys),
            Self::Basin(basin) => basin.save(world, tank),
        }
    }

    fn refund(&self, world: World<'_>, tank: &TankInventory) -> bool {
        match self {
            Self::Tank(pos, kind) => kind.save(world, *pos, tank),
            Self::Boiler(_) => false,
            Self::Open(end) => end.refund(world, tank),
            Self::Hose(hose) => hose.refund(world, tank),
            Self::Basin(basin) => basin.save(world, tank),
        }
    }

    fn update_cache(self, tanks: &mut Endpoints, tank: TankInventory) {
        if let Self::Tank(pos, kind) = self {
            tanks.insert(pos, (kind, tank));
        }
    }
}

fn move_fluid(
    world: World<'_>,
    tanks: &mut Endpoints,
    owners: &EndpointOwners,
    open: &HashSet<BlockFace>,
    pulleys: &mut HosePulleys,
    boilers: &mut Boilers,
    graph: &FluidNetworkGraph,
) {
    let Some(fluid) = &graph.fluid else { return };
    let mut seen_tanks = HashSet::new();
    let (source, mut updated_source) = if let Some(&owner) =
        owners.get(&graph.start.connected_pos())
        && let Some((kind, tank)) = tanks.get(&owner)
    {
        if boilers.active(owner) {
            return;
        }
        seen_tanks.insert(owner);
        match kind {
            EndpointKind::Hose(_) => {
                let Some((hose, tank)) = HoseTransfer::source(world, owner, pulleys.get(&owner))
                else {
                    return;
                };
                (Storage::Hose(hose), tank)
            }
            EndpointKind::Basin => {
                let Some((basin, tank)) = BasinTransfer::source(world, owner, fluid) else {
                    return;
                };
                (Storage::Basin(basin), tank)
            }
            _ => (Storage::Tank(owner, *kind), tank.clone()),
        }
    } else if open.contains(&graph.start)
        && let Some(cell) = open_cell(world, graph.start)
        && let Some((end, tank)) = OpenEnd::source(world, graph.start, &cell)
    {
        (Storage::Open(end), tank)
    } else {
        return;
    };
    if !updated_source
        .fluid()
        .is_some_and(|current| current.same_fluid_same_components(fluid))
    {
        return;
    }
    let mut targets = graph.targets.clone();
    targets.sort_unstable_by_key(|face| {
        let pos = face.connected_pos();
        let pos = owners.get(&pos).copied().unwrap_or(pos);
        (pos.x, pos.y, pos.z, face_order(face))
    });
    targets.dedup();
    let mut targets: Vec<_> = targets
        .into_iter()
        .filter(|&face| face != graph.start)
        .filter_map(|face| {
            let (storage, tank) = if let Some(&owner) = owners.get(&face.connected_pos()) {
                let (kind, tank) = tanks.get(&owner)?;
                // The item drain exposes extraction only.
                if *kind == EndpointKind::ItemDrain || !seen_tanks.insert(owner) {
                    return None;
                }
                match kind {
                    EndpointKind::Hose(_) => {
                        let (hose, tank) =
                            HoseTransfer::target(world, owner, pulleys.get_mut(&owner), fluid)?;
                        (Storage::Hose(hose), tank)
                    }
                    EndpointKind::Basin => {
                        let (basin, tank) = BasinTransfer::target(world, owner, fluid)?;
                        (Storage::Basin(basin), tank)
                    }
                    EndpointKind::Tank if boilers.active(owner) => {
                        if fluid.id != "minecraft:water" {
                            return None;
                        }
                        (
                            Storage::Boiler(owner),
                            TankInventory::new(i32::MAX as u32, 1).ok()?,
                        )
                    }
                    _ => (Storage::Tank(owner, *kind), tank.clone()),
                }
            } else if open.contains(&face) {
                let cell = open_cell(world, face)?;
                let (end, tank) = OpenEnd::target(world, face, &cell, fluid)?;
                (Storage::Open(end), tank)
            } else {
                return None;
            };
            Some((storage, tank.amount(), tank))
        })
        .collect();
    let mut handlers: Vec<&mut dyn FluidHandler> = targets
        .iter_mut()
        .map(|(_, _, tank)| tank as &mut dyn FluidHandler)
        .collect();
    let moved = transfer_tick(
        &mut updated_source,
        &mut handlers,
        fluid,
        graph.transfer_speed as f32 * 2.0,
    );
    if moved == 0 || !source.save(world, &updated_source, pulleys, boilers) {
        return;
    }

    // The tick is synchronous. If a target's entity write fails, return its share.
    let mut returned = 0;
    for (storage, before, tank) in targets {
        let received = tank.amount() - before;
        if received > 0 {
            if storage.save(world, &tank, pulleys, boilers) {
                storage.update_cache(tanks, tank);
            } else {
                returned += received;
            }
        }
    }
    if returned > 0 {
        let mut refund = fluid.clone();
        refund.amount = returned;
        updated_source.fill(&refund, false);
        if !source.refund(world, &updated_source) {
            return;
        }
    }
    source.update_cache(tanks, updated_source);
}

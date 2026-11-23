//! Tick the Create transport and network models against loaded fluid endpoints.

use std::collections::{HashMap, HashSet};

use moddedmc_api::{Pos, content::World};

use super::{
    network::{FluidHandler, transfer_tick},
    network_graph::{BlockFace, FluidNetworkGraph, NetworkTopology, Source},
    pipe_connection::{FluidStack, PipeConnection},
    tank::TankInventory,
    tank_runtime,
    transport::Transport,
};

#[derive(Default)]
pub struct FlowRuntime {
    pipes: HashMap<Pos, Transport>,
    networks: HashMap<BlockFace, FluidNetworkGraph>,
}

impl FlowRuntime {
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

    pub fn tick(&mut self, world: World<'_>, tank_states: &HashSet<u32>) {
        if self.pipes.is_empty() {
            return;
        }
        let mut tanks = HashMap::new();
        let mut checked = HashSet::new();
        let mut provided = Vec::with_capacity(self.pipes.len());
        for (&pos, pipe) in &self.pipes {
            provided.push((
                pos,
                std::array::from_fn(|side| {
                    pipe.connections[side].as_ref()?;
                    let adjacent = BlockFace {
                        pos,
                        side: side as u8,
                    }
                    .opposite();
                    if let Some(other) = self.pipes.get(&adjacent.pos) {
                        return other.connections[usize::from(adjacent.side)]
                            .as_ref()?
                            .provide_outbound_flow()
                            .cloned();
                    }
                    if checked.insert(adjacent.pos) {
                        let mut state = [0];
                        if world.read(&[adjacent.pos], &mut state)
                            && tank_states.contains(&state[0])
                            && let Some(tank) = tank_runtime::load(world, adjacent.pos)
                        {
                            tanks.insert(adjacent.pos, tank);
                        }
                    }
                    tanks.get(&adjacent.pos)?.fluid().cloned()
                }),
            ));
        }
        for (pos, provided) in provided {
            if let Some(pipe) = self.pipes.get_mut(&pos) {
                pipe.tick(&provided);
            }
        }

        let mut starts = Vec::new();
        let topology = Topology {
            pipes: &self.pipes,
            tanks: &tanks,
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
            if let Some(fluid) = &graph.fluid {
                move_tank_fluid(world, &mut tanks, graph, fluid);
            }
        }
    }
}

fn face_order(face: &BlockFace) -> (i32, i32, i32, u8) {
    (face.pos.x, face.pos.y, face.pos.z, face.side)
}

struct Topology<'a> {
    pipes: &'a HashMap<Pos, Transport>,
    tanks: &'a HashMap<Pos, TankInventory>,
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
        if self.tanks.contains_key(&adjacent.pos) {
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

fn move_tank_fluid(
    world: World<'_>,
    tanks: &mut HashMap<Pos, TankInventory>,
    graph: &FluidNetworkGraph,
    fluid: &FluidStack,
) {
    let source = graph.start.connected_pos();
    let Some(mut updated_source) = tanks.get(&source).cloned() else {
        return;
    };
    let mut targets: Vec<_> = graph
        .targets
        .iter()
        .map(|face| face.connected_pos())
        .collect();
    targets.sort_unstable_by_key(|pos| (pos.x, pos.y, pos.z));
    targets.dedup();
    let mut targets: Vec<_> = targets
        .into_iter()
        .filter(|&pos| pos != source)
        .filter_map(|pos| tanks.get(&pos).cloned().map(|tank| (pos, tank)))
        .collect();
    let mut handlers: Vec<&mut dyn FluidHandler> = targets
        .iter_mut()
        .map(|(_, tank)| tank as &mut dyn FluidHandler)
        .collect();
    let moved = transfer_tick(
        &mut updated_source,
        &mut handlers,
        fluid,
        graph.transfer_speed as f32 * 2.0,
    );
    if moved == 0 || !tank_runtime::save(world, source, &updated_source) {
        return;
    }

    // The tick is synchronous. If a target's entity write fails, return its share.
    let mut returned = 0;
    for (pos, tank) in targets {
        let received = tank.amount() - tanks[&pos].amount();
        if received > 0 {
            if tank_runtime::save(world, pos, &tank) {
                tanks.insert(pos, tank);
            } else {
                returned += received;
            }
        }
    }
    if returned > 0 {
        let mut refund = fluid.clone();
        refund.amount = returned;
        updated_source.fill(&refund, false);
        if !tank_runtime::save(world, source, &updated_source) {
            return;
        }
    }
    tanks.insert(source, updated_source);
}

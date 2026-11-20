//! Create 6.0.10 FluidNetwork.tick's breadth-first flow discovery.
//! Transfer of the discovered endpoints is handled separately in network.rs.

use std::collections::{HashSet, VecDeque};

use moddedmc_api::Pos;

use crate::kinetics::{FACES, offset};

use super::network::transfer_speed;
use super::pipe_connection::{FluidStack, PipeConnection};

const CYCLES_PER_TICK: usize = 16;
const PROPAGATION_PAUSE_TICKS: u8 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockFace {
    pub pos: Pos,
    pub side: u8,
}

impl BlockFace {
    pub fn connected_pos(self) -> Pos {
        offset(self.pos, FACES[usize::from(self.side)])
    }

    pub fn opposite(self) -> Self {
        Self {
            pos: self.connected_pos(),
            side: self.side ^ 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// A chunk boundary or a source still waiting to be discovered.
    Undetermined,
    Endpoint,
    Other,
}

/// FluidTransportBehaviour and FlowSource queries without chunk loading.
pub trait NetworkTopology {
    fn is_loaded(&self, pos: Pos) -> bool;
    fn connection(&self, face: BlockFace) -> Option<PipeConnection>;
    fn source(&self, face: BlockFace) -> Source;
}

/// Persistent FluidNetwork state. Calls to tick expand at most 16 cycles;
/// a source's flow and target list remain until reset() or network removal.
pub struct FluidNetworkGraph {
    pub start: BlockFace,
    pub queued: VecDeque<BlockFace>,
    pub frontier: HashSet<BlockFace>,
    pub visited: HashSet<Pos>,
    pub targets: Vec<BlockFace>,
    pub fluid: Option<FluidStack>,
    pub transfer_speed: u32,
    pause_before_propagation: u8,
}

impl FluidNetworkGraph {
    pub fn new(start: BlockFace) -> Self {
        let mut graph = Self {
            start,
            queued: VecDeque::new(),
            frontier: HashSet::new(),
            visited: HashSet::new(),
            targets: Vec::new(),
            fluid: None,
            transfer_speed: 0,
            pause_before_propagation: 0,
        };
        graph.reset();
        graph
    }

    pub fn reset(&mut self) {
        self.frontier.clear();
        self.visited.clear();
        self.targets.clear();
        self.queued.clear();
        self.fluid = None;
        self.queued.push_back(self.start);
        self.pause_before_propagation = PROPAGATION_PAUSE_TICKS;
    }

    /// FluidNetwork.tick up to the endpoint-transfer phase. A source and
    /// target FluidHandler must be resolved on the server tick before moving mB.
    pub fn tick(&mut self, world: &impl NetworkTopology) {
        if self.pause_before_propagation > 0 {
            self.pause_before_propagation -= 1;
            return;
        }
        for _ in 0..CYCLES_PER_TICK {
            let mut pending = VecDeque::new();
            while let Some(face) = self.queued.pop_front() {
                // Java retains unloaded queue entries; they can become reachable later.
                if !world.is_loaded(face.pos) {
                    pending.push_back(face);
                    continue;
                }
                let Some(connection) = world.connection(face) else {
                    continue;
                };
                if face == self.start {
                    self.transfer_speed = transfer_speed(connection.pressure[0]);
                }
                self.frontier.insert(face);
            }
            self.queued = pending;

            let mut should_continue = false;
            let frontier: Vec<_> = self.frontier.iter().copied().collect();
            for face in frontier {
                let Some(connection) = world.connection(face) else {
                    continue;
                };
                let Some(flow) = connection.flow.as_ref() else {
                    continue;
                };

                if self.fluid.as_ref().is_some_and(|fluid| {
                    !fluid.is_empty() && !fluid.same_fluid_same_components(&flow.fluid)
                }) {
                    self.frontier.remove(&face);
                    continue;
                }
                if !flow.inbound {
                    if connection.compare_pressure() >= 0.0 {
                        self.frontier.remove(&face);
                    }
                    continue;
                }
                if !flow.complete {
                    continue;
                }
                if self.fluid.is_none() {
                    self.fluid = Some(flow.fluid.clone());
                }

                let mut remove = true;
                for side in 0..6u8 {
                    if side == face.side {
                        continue;
                    }
                    let adjacent = BlockFace {
                        pos: face.pos,
                        side,
                    };
                    let Some(other) = world.connection(adjacent) else {
                        continue;
                    };
                    let Some(out_flow) = other.flow.as_ref() else {
                        if other.has_pressure() && other.pressure[1] > 0.0 {
                            remove = false;
                        }
                        continue;
                    };
                    if out_flow.inbound {
                        if other.compare_pressure() > 0.0 {
                            remove = false;
                        }
                        continue;
                    }
                    if !out_flow.complete {
                        remove = false;
                        continue;
                    }
                    match world.source(adjacent) {
                        Source::Undetermined => {
                            remove = false;
                        }
                        Source::Endpoint => {
                            self.targets.push(adjacent);
                        }
                        Source::Other => {
                            let connected = adjacent.connected_pos();
                            if self.visited.insert(connected) {
                                self.queued.push_back(adjacent.opposite());
                                should_continue = true;
                            }
                        }
                    }
                }
                if remove {
                    self.frontier.remove(&face);
                }
            }
            if !should_continue {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use moddedmc_api::Pos;

    use super::{BlockFace, FluidNetworkGraph, NetworkTopology, Source};
    use crate::fluids::pipe_connection::{Flow, FluidStack, PipeConnection};

    #[derive(Default)]
    struct World {
        connections: HashMap<BlockFace, PipeConnection>,
        endpoints: HashSet<BlockFace>,
        unavailable_sources: HashSet<BlockFace>,
        unloaded: HashSet<Pos>,
    }

    impl World {
        fn face(x: i32, side: u8) -> BlockFace {
            BlockFace {
                pos: Pos { x, y: 64, z: 0 },
                side,
            }
        }

        fn flow(&mut self, x: i32, side: u8, inbound: bool, fluid: &str, complete: bool) {
            let mut conn = PipeConnection::new(side);
            conn.pressure = if inbound { [64.0, 0.0] } else { [0.0, 64.0] };
            conn.flow = Some(Flow {
                inbound,
                fluid: FluidStack {
                    id: fluid.into(),
                    amount: 1000,
                    components: None,
                },
                progress: if complete { 1.0 } else { 0.0 },
                complete,
            });
            self.connections.insert(Self::face(x, side), conn);
        }
    }

    impl NetworkTopology for World {
        fn is_loaded(&self, pos: Pos) -> bool {
            !self.unloaded.contains(&pos)
        }

        fn connection(&self, face: BlockFace) -> Option<PipeConnection> {
            self.connections.get(&face).cloned()
        }

        fn source(&self, face: BlockFace) -> Source {
            if self.unavailable_sources.contains(&face) {
                Source::Undetermined
            } else if self.endpoints.contains(&face) {
                Source::Endpoint
            } else {
                Source::Other
            }
        }
    }

    #[test]
    fn two_wait_ticks_precede_single_pipe_source_to_endpoint() {
        let mut world = World::default();
        world.flow(0, 4, true, "minecraft:water", true);
        world.flow(0, 5, false, "minecraft:water", true);
        world.endpoints.insert(World::face(0, 5));
        let mut network = FluidNetworkGraph::new(World::face(0, 4));
        network.tick(&world);
        network.tick(&world);
        assert!(network.targets.is_empty());
        network.tick(&world);
        assert_eq!(network.targets, [World::face(0, 5)]);
        assert_eq!(network.transfer_speed, 32);
        assert_eq!(
            network.fluid.as_ref().map(|stack| stack.id.as_str()),
            Some("minecraft:water")
        );
    }

    #[test]
    fn a_seventeen_pipe_line_is_spread_across_multiple_ticks() {
        let mut world = World::default();
        for x in 0..18 {
            world.flow(x, 4, true, "minecraft:water", true);
            world.flow(x, 5, false, "minecraft:water", true);
        }
        world.endpoints.insert(World::face(17, 5));
        let mut network = FluidNetworkGraph::new(World::face(0, 4));
        for _ in 0..3 {
            network.tick(&world);
        }
        assert!(network.targets.is_empty());
        assert!(!network.queued.is_empty());
        network.tick(&world);
        assert_eq!(network.targets, [World::face(17, 5)]);
    }

    #[test]
    fn unloaded_next_pipe_remains_queued_until_the_chunk_returns() {
        let mut world = World::default();
        for x in 0..2 {
            world.flow(x, 4, true, "minecraft:water", true);
            world.flow(x, 5, false, "minecraft:water", true);
        }
        world.endpoints.insert(World::face(1, 5));
        world.unloaded.insert(World::face(1, 4).pos);
        let mut network = FluidNetworkGraph::new(World::face(0, 4));
        for _ in 0..3 {
            network.tick(&world);
        }
        assert!(network.targets.is_empty());
        network.tick(&world);
        assert_eq!(network.queued, [World::face(1, 4)]);
        world.unloaded.clear();
        network.tick(&world);
        assert_eq!(network.targets, [World::face(1, 5)]);
    }

    #[test]
    fn incompatible_branch_fluid_cannot_join_a_running_network() {
        let mut world = World::default();
        world.flow(0, 4, true, "minecraft:water", true);
        world.flow(0, 5, false, "minecraft:water", true);
        world.flow(1, 4, true, "minecraft:lava", true);
        world.flow(1, 5, false, "minecraft:lava", true);
        world.endpoints.insert(World::face(1, 5));
        let mut network = FluidNetworkGraph::new(World::face(0, 4));
        for _ in 0..5 {
            network.tick(&world);
        }
        assert!(network.targets.is_empty());
        assert_eq!(
            network.fluid.as_ref().map(|stack| stack.id.as_str()),
            Some("minecraft:water")
        );
    }

    #[test]
    fn unknown_endpoint_keeps_a_frontier_face_for_later_retry() {
        let mut world = World::default();
        world.flow(0, 4, true, "minecraft:water", true);
        world.flow(0, 5, false, "minecraft:water", true);
        world.unavailable_sources.insert(World::face(0, 5));
        let mut network = FluidNetworkGraph::new(World::face(0, 4));
        for _ in 0..3 {
            network.tick(&world);
        }
        assert!(network.frontier.contains(&World::face(0, 4)));
        world.unavailable_sources.clear();
        world.endpoints.insert(World::face(0, 5));
        network.tick(&world);
        assert_eq!(network.targets, [World::face(0, 5)]);
    }
}

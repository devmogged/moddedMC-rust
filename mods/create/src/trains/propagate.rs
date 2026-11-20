//! TrackPropagator.onRailAdded graph discovery, stages 2 and 3.
//! The read-only TrackWorld seam is backed by ITrackBlock.walkConnectedTracks
//! when content world events are wired in. No server globals in this module.
use super::graph::{DiscoveredLocation, TrackGraph, TrackNodeLocation, is_valid_graph_node};
use std::collections::{HashSet, VecDeque};

const FRONTIER_LIMIT: usize = 1_000;

/// Same location key semantics as TrackNodeLocation/DiscoveredLocation.equals.
pub(super) trait TrackWorld {
    fn walk_connected(&self, from: &TrackNodeLocation) -> Vec<DiscoveredLocation>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum PropagationError {
    NoStartNode,
    FrontierLimit,
    InvalidConnection,
}

#[derive(Clone)]
struct FrontierEntry {
    parent: TrackNodeLocation,
    prev: Option<TrackNodeLocation>,
    current: DiscoveredLocation,
}

fn filtered_ends(world: &impl TrackWorld, entry: &FrontierEntry) -> Vec<DiscoveredLocation> {
    let mut ends = world.walk_connected(&entry.current.location);
    if let Some(prev) = &entry.prev {
        // Java removes entry.prevNode via TrackNodeLocation.equals, not
        // a vector-angle or material comparison.
        ends.retain(|end| &end.location != prev);
    }
    ends
}

/// Phase 2 of TrackPropagator.onRailAdded. It starts from the track's two
/// initial ends (the first rail item/block that changed), rather than guessing
/// a graph root from a chunk origin. It searches for a forced/terminal/bend node.
fn find_start(
    world: &impl TrackWorld,
    initial: &[DiscoveredLocation],
) -> Result<DiscoveredLocation, PropagationError> {
    let mut frontier = VecDeque::new();
    let mut visited = HashSet::new();
    for node in initial {
        frontier.push_back(FrontierEntry {
            parent: node.location.clone(),
            prev: None,
            current: node.clone(),
        });
    }

    let mut explored = 0;
    while let Some(entry) = frontier.pop_front() {
        if explored == FRONTIER_LIMIT {
            return Err(PropagationError::FrontierLimit);
        }
        explored += 1;
        let ends = filtered_ends(world, &entry);
        let first = entry.prev.is_none();
        if is_valid_graph_node(&entry.current, &ends, first) {
            return Ok(entry.current);
        }
        for next in ends {
            if visited.insert(next.location.clone()) {
                frontier.push_back(FrontierEntry {
                    parent: next.location.clone(),
                    prev: Some(entry.current.location.clone()),
                    current: next,
                });
            }
        }
    }
    Err(PropagationError::NoStartNode)
}

/// Stage 3: walk until each valid node and connect it to the parent graph
/// node. A newly reached *existing* graph node terminates that branch;
/// revisiting a non-node is bounded by the Java 1000-entry emergency cap.
///
/// The Java implementation silently breaks at this limit; returning Err is a
/// safer fail-closed behavior, so callers never serialize a truncated graph.
pub(super) fn discover_graph(
    world: &impl TrackWorld,
    initial: &[DiscoveredLocation],
) -> Result<TrackGraph, PropagationError> {
    let start = find_start(world, initial)?;
    let mut graph = TrackGraph::default();
    graph.create_node_if_absent(start.location.clone(), start.normal);

    let mut frontier = VecDeque::from([FrontierEntry {
        parent: start.location.clone(),
        prev: None,
        current: start.clone(),
    }]);
    let mut explored = 0;

    while let Some(entry) = frontier.pop_front() {
        if explored == FRONTIER_LIMIT {
            return Err(PropagationError::FrontierLimit);
        }
        explored += 1;
        let ends = filtered_ends(world, &entry);
        let first = entry.prev.is_none();
        let mut parent = entry.parent.clone();

        if entry.current.location != start.location
            && is_valid_graph_node(&entry.current, &ends, first)
        {
            let is_new =
                graph.create_node_if_absent(entry.current.location.clone(), entry.current.normal);
            // Create's TrackGraph.connectNodes returns false for an already
            // connected edge; this is not an error for a loop traversal.
            let exists = graph
                .connections
                .get(&parent)
                .is_some_and(|edges| edges.contains_key(&entry.current.location));
            if !exists
                && !graph.connect_nodes_with_curve(
                    &parent,
                    &entry.current.location,
                    &entry.current.material_a,
                    entry.current.turn.clone(),
                )
            {
                return Err(PropagationError::InvalidConnection);
            }
            parent = entry.current.location.clone();
            if !is_new {
                continue;
            }
        }

        for next in ends {
            frontier.push_back(FrontierEntry {
                parent: parent.clone(),
                prev: Some(entry.current.location.clone()),
                current: next,
            });
        }
    }
    Ok(graph)
}

#[cfg(test)]
mod tests {
    use super::super::graph::Vec3;
    use super::*;
    use std::collections::{HashMap, HashSet};

    const UP: Vec3 = Vec3 {
        x: 0.0,
        y: 1.0,
        z: 0.0,
    };
    const EAST: Vec3 = Vec3 {
        x: 1.0,
        y: 0.0,
        z: 0.0,
    };
    const SOUTH: Vec3 = Vec3 {
        x: 0.0,
        y: 0.0,
        z: 1.0,
    };

    fn location(x: i32, z: i32) -> TrackNodeLocation {
        TrackNodeLocation::from_world(
            "minecraft:overworld",
            Vec3 {
                x: x as f64,
                y: 64.0,
                z: z as f64 + 0.5,
            },
            0,
        )
    }

    #[derive(Default)]
    struct RailFixtures {
        connected: HashMap<TrackNodeLocation, Vec<DiscoveredLocation>>,
        directions: HashMap<TrackNodeLocation, Vec3>,
        forced: HashSet<TrackNodeLocation>,
        material_at: HashMap<TrackNodeLocation, &'static str>,
    }

    impl RailFixtures {
        fn join(&mut self, from: (i32, i32), to: (i32, i32)) {
            let a = location(from.0, from.1);
            let b = location(to.0, to.1);
            self.connected
                .entry(a)
                .or_default()
                .push(DiscoveredLocation {
                    location: b.clone(),
                    direction: Some(EAST),
                    normal: UP,
                    material_a: "create:andesite".into(),
                    material_b: "create:andesite".into(),
                    forced: false,
                    turn: None,
                });
            self.connected
                .entry(b)
                .or_default()
                .push(DiscoveredLocation {
                    location: location(from.0, from.1),
                    direction: Some(EAST),
                    normal: UP,
                    material_a: "create:andesite".into(),
                    material_b: "create:andesite".into(),
                    forced: false,
                    turn: None,
                });
        }

        fn line(&mut self, from: i32, through: i32) {
            for x in from..through {
                self.join((x, 0), (x + 1, 0));
            }
        }

        fn node(&self, pos: (i32, i32)) -> DiscoveredLocation {
            let loc = location(pos.0, pos.1);
            let material = self
                .material_at
                .get(&loc)
                .copied()
                .unwrap_or("create:andesite");
            DiscoveredLocation {
                location: loc.clone(),
                direction: Some(self.directions.get(&loc).copied().unwrap_or(EAST)),
                normal: UP,
                material_a: material.to_owned(),
                material_b: material.to_owned(),
                forced: self.forced.contains(&loc),
                turn: None,
            }
        }

        fn starts(&self, pos: (i32, i32)) -> Vec<DiscoveredLocation> {
            vec![self.node(pos)]
        }
    }

    impl TrackWorld for RailFixtures {
        fn walk_connected(&self, from: &TrackNodeLocation) -> Vec<DiscoveredLocation> {
            self.connected
                .get(from)
                .into_iter()
                .flatten()
                .map(|item| self.node((item.location.x / 2, (item.location.z - 1) / 2)))
                .collect()
        }
    }

    #[test]
    fn thirty_two_blocks_keep_endpoint_and_periodic_16_block_nodes() {
        let mut world = RailFixtures::default();
        world.line(0, 32);
        let graph = discover_graph(&world, &world.starts((7, 0))).unwrap();
        assert_eq!(graph.nodes.len(), 3);
        for x in [0, 16, 32] {
            assert!(graph.nodes.contains_key(&location(x, 0)));
        }
        assert_eq!(
            graph.connections.values().map(HashMap::len).sum::<usize>(),
            4
        );
        assert_eq!(
            graph.connections[&location(0, 0)][&location(16, 0)].length(),
            16.0
        );
        assert_eq!(
            graph.connections[&location(16, 0)][&location(32, 0)].length(),
            16.0
        );
    }

    #[test]
    fn a_junction_and_direction_change_keep_branch_graph_nodes() {
        let mut world = RailFixtures::default();
        world.line(0, 8);
        world.join((4, 0), (4, 1));
        world.join((4, 1), (4, 2));
        world.join((4, 2), (4, 3));
        for z in 1..=3 {
            world.directions.insert(location(4, z), SOUTH);
        }
        let graph = discover_graph(&world, &world.starts((1, 0))).unwrap();
        assert_eq!(graph.nodes.len(), 4);
        for key in [(0, 0), (4, 0), (8, 0), (4, 3)] {
            assert!(graph.nodes.contains_key(&location(key.0, key.1)));
        }
        assert_eq!(graph.connections[&location(4, 0)].len(), 3);
        assert_eq!(graph.connected_components().len(), 1);
    }

    #[test]
    fn forced_marker_creates_extra_node_on_an_otherwise_straight_line() {
        let mut world = RailFixtures::default();
        world.line(1, 12);
        world.forced.insert(location(7, 0));
        let graph = discover_graph(&world, &world.starts((3, 0))).unwrap();
        // Java also forces the immediately previous node when a next end is forced.
        assert_eq!(graph.nodes.len(), 4);
        assert!(graph.nodes.contains_key(&location(6, 0)));
        assert!(graph.nodes.contains_key(&location(7, 0)));
        assert_eq!(graph.connections[&location(7, 0)].len(), 2);
    }

    #[test]
    fn disconnected_and_isolated_positions_fail_closed_without_phantom_edges() {
        let mut world = RailFixtures::default();
        world.line(0, 3);
        let graph = discover_graph(&world, &world.starts((1, 0))).unwrap();
        assert_eq!(graph.nodes.len(), 2);
        assert_eq!(graph.connected_components().len(), 1);
        assert_eq!(
            graph.connections.values().map(HashMap::len).sum::<usize>(),
            2
        );

        let island = discover_graph(&world, &world.starts((80, 80))).unwrap();
        assert_eq!(island.nodes.len(), 1);
        assert!(island.connections.is_empty());
        assert_eq!(
            discover_graph(&world, &[]).unwrap_err(),
            PropagationError::NoStartNode
        );
    }

    #[test]
    fn traversal_above_java_frontier_limit_is_error_not_silent_truncation() {
        let mut world = RailFixtures::default();
        world.line(1, 2500);
        let err = discover_graph(&world, &world.starts((500, 0))).unwrap_err();
        assert_eq!(err, PropagationError::FrontierLimit);
    }
}

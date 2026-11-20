//! Create 6.0.10 PumpBlockEntity.distributePressureTo, through its DFS.
use std::collections::{HashMap, HashSet, VecDeque};

use moddedmc_api::Pos;

use crate::kinetics::{FACES, offset};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PressureFace {
    pub pos: Pos,
    pub face: u8,
    pub inbound: bool,
    pub pressure: f32,
}

/// World questions from PumpBlockEntity.hasReachedValidEndpoint and FluidPropagator.
/// It is the caller's responsibility to distinguish handlers, open ends and pumps
/// from traversable pipes, and to skip unavailable chunks without loading them.
pub trait PumpTopology {
    fn is_loaded(&self, pos: Pos) -> bool;
    fn pipe_faces(&self, pos: Pos) -> Option<[bool; 6]>;
    fn has_valid_endpoint(&self, pipe: Pos, face: u8, pull: bool) -> bool;
}

#[derive(Default)]
struct Node {
    distance: u32,
    faces: HashMap<u8, bool>,
}

fn adjacent(pos: Pos, face: u8) -> Pos {
    offset(pos, FACES[usize::from(face)])
}

fn find_endpoints(
    graph: &HashMap<Pos, Node>,
    targets: &HashSet<(Pos, u8)>,
    valid_faces: &mut HashMap<u32, HashSet<(Pos, u8)>>,
    current: (Pos, u8),
    pull: bool,
) -> bool {
    let (pos, entering_face) = current;
    let Some(node) = graph.get(&pos) else {
        return false;
    };
    let mut succeeded = false;
    for next_face in 0..6u8 {
        if next_face == entering_face {
            continue;
        }
        let Some(&inbound) = node.faces.get(&next_face) else {
            continue;
        };
        let local = (pos, next_face);
        if targets.contains(&local) {
            valid_faces.entry(node.distance).or_default().insert(local);
            succeeded = true;
            continue;
        }
        if inbound != pull {
            continue;
        }
        if find_endpoints(
            graph,
            targets,
            valid_faces,
            (adjacent(pos, next_face), next_face ^ 1),
            pull,
        ) {
            valid_faces.entry(node.distance).or_default().insert(local);
            succeeded = true;
        }
    }
    if succeeded {
        valid_faces
            .entry(node.distance)
            .or_default()
            .insert(current);
    }
    succeeded
}

/// BFS collects candidate faces, then the original DFS discards paths that
/// cannot reach an endpoint. Each distance layer splits pump pressure across
/// its successful branches, excluding one face for its incoming connection.
pub fn distribute_pressure(
    topology: &impl PumpTopology,
    pump: Pos,
    side: u8,
    pull: bool,
    speed: f32,
    max_distance: u32,
) -> Vec<PressureFace> {
    if speed == 0.0 || usize::from(side) >= FACES.len() {
        return Vec::new();
    }
    let start_neighbor = adjacent(pump, side);
    let mut graph: HashMap<Pos, Node> = HashMap::new();
    let mut targets = HashSet::new();

    if !topology.has_valid_endpoint(pump, side, pull) {
        graph.entry(pump).or_default().faces.insert(side, pull);
        let first = graph.entry(start_neighbor).or_default();
        first.distance = 1;
        first.faces.insert(side ^ 1, !pull);

        let mut frontier = VecDeque::from([(1u32, start_neighbor)]);
        let mut visited = HashSet::new();
        while let Some((distance, pos)) = frontier.pop_front() {
            if !topology.is_loaded(pos) || !visited.insert(pos) {
                continue;
            }
            let Some(connections) = topology.pipe_faces(pos) else {
                continue;
            };
            for face in 0..6u8 {
                if !connections[usize::from(face)] {
                    continue;
                }
                let connected = adjacent(pos, face);
                if !topology.is_loaded(connected) || (pos == start_neighbor && face == (side ^ 1)) {
                    continue;
                }
                if topology.has_valid_endpoint(pos, face, pull) {
                    graph.entry(pos).or_default().faces.insert(face, pull);
                    targets.insert((pos, face));
                    continue;
                }
                if topology.pipe_faces(connected).is_none() || visited.contains(&connected) {
                    continue;
                }
                graph.entry(pos).or_default().faces.insert(face, pull);
                if distance + 1 >= max_distance {
                    targets.insert((pos, face));
                    continue;
                }
                let next = graph.entry(connected).or_default();
                next.distance = distance + 1;
                next.faces.insert(face ^ 1, !pull);
                frontier.push_back((distance + 1, connected));
            }
        }
    }

    let mut valid_faces = HashMap::new();
    find_endpoints(&graph, &targets, &mut valid_faces, (pump, side ^ 1), pull);
    let mut result = Vec::new();
    for faces in valid_faces.values() {
        let branches = faces.len().saturating_sub(1).max(1) as f32;
        for &(pos, face) in faces {
            if pos == pump {
                continue;
            }
            let Some(inbound) = graph.get(&pos).and_then(|node| node.faces.get(&face)) else {
                continue;
            };
            result.push(PressureFace {
                pos,
                face,
                inbound: *inbound,
                pressure: speed.abs() / branches,
            });
        }
    }
    result.sort_by_key(|p| (p.pos.x, p.pos.y, p.pos.z, p.face));
    result
}

#[cfg(test)]
mod tests {
    use super::{PressureFace, PumpTopology, distribute_pressure};
    use moddedmc_api::Pos;
    use std::collections::{HashMap, HashSet};

    struct Topology {
        pipes: HashMap<Pos, [bool; 6]>,
        ends: HashSet<(Pos, u8)>,
        unloaded: HashSet<Pos>,
    }

    impl Topology {
        fn new() -> Self {
            Self {
                pipes: HashMap::new(),
                ends: HashSet::new(),
                unloaded: HashSet::new(),
            }
        }

        fn pipe(&mut self, x: i32, z: i32, faces: &[u8]) {
            let mut present = [false; 6];
            for &face in faces {
                present[usize::from(face)] = true;
            }
            self.pipes.insert(Pos { x, y: 64, z }, present);
        }
    }

    impl PumpTopology for Topology {
        fn is_loaded(&self, pos: Pos) -> bool {
            !self.unloaded.contains(&pos)
        }

        fn pipe_faces(&self, pos: Pos) -> Option<[bool; 6]> {
            self.pipes.get(&pos).copied()
        }

        fn has_valid_endpoint(&self, pipe: Pos, face: u8, _pull: bool) -> bool {
            self.ends.contains(&(pipe, face))
        }
    }

    fn pump() -> Pos {
        Pos { x: 0, y: 64, z: 0 }
    }

    #[test]
    fn single_pipe_gets_full_pump_pressure_on_both_faces() {
        let mut world = Topology::new();
        world.pipe(1, 0, &[4, 5]);
        world.ends.insert((Pos { x: 1, y: 64, z: 0 }, 5));
        let faces = distribute_pressure(&world, pump(), 5, false, 64.0, 16);
        assert_eq!(
            faces,
            [
                PressureFace {
                    pos: Pos { x: 1, y: 64, z: 0 },
                    face: 4,
                    inbound: true,
                    pressure: 64.0
                },
                PressureFace {
                    pos: Pos { x: 1, y: 64, z: 0 },
                    face: 5,
                    inbound: false,
                    pressure: 64.0
                },
            ]
        );
    }

    #[test]
    fn two_ends_split_pressure_in_a_tee() {
        let mut world = Topology::new();
        world.pipe(1, 0, &[2, 4, 5]);
        let pos = Pos { x: 1, y: 64, z: 0 };
        world.ends.insert((pos, 2));
        world.ends.insert((pos, 5));
        let faces = distribute_pressure(&world, pump(), 5, false, 64.0, 16);
        assert_eq!(faces.len(), 3);
        assert!(faces.iter().all(|face| face.pressure == 32.0));
        assert_eq!(faces.iter().filter(|face| face.inbound).count(), 1);
    }

    #[test]
    fn dead_end_or_unloaded_pipe_has_no_pressure() {
        let mut world = Topology::new();
        world.pipe(1, 0, &[4, 5]);
        assert!(distribute_pressure(&world, pump(), 5, false, 64.0, 16).is_empty());

        let pos = Pos { x: 1, y: 64, z: 0 };
        world.ends.insert((pos, 5));
        world.unloaded.insert(pos);
        assert!(distribute_pressure(&world, pump(), 5, false, 64.0, 16).is_empty());
    }
}

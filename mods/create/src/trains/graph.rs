//! Graph-node geometry and connectivity from Create 6.0.10's TrackNodeLocation,
//! TrackPropagator and TrackGraph. World discovery and NBT integration follow.

use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::atomic::{AtomicI32, Ordering},
};

// TrackGraph.nodeNetIdGenerator is shared across every graph in the process.
static NODE_NET_IDS: AtomicI32 = AtomicI32::new(0);

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec3 {
    pub(super) fn minus(self, other: Self) -> Self {
        Self {
            x: self.x - other.x,
            y: self.y - other.y,
            z: self.z - other.z,
        }
    }

    pub(super) fn plus(self, other: Self) -> Self {
        Self {
            x: self.x + other.x,
            y: self.y + other.y,
            z: self.z + other.z,
        }
    }

    pub(super) fn scale(self, factor: f64) -> Self {
        Self {
            x: self.x * factor,
            y: self.y * factor,
            z: self.z * factor,
        }
    }

    pub(super) fn cross(self, other: Self) -> Self {
        Self {
            x: self.y * other.z - self.z * other.y,
            y: self.z * other.x - self.x * other.z,
            z: self.x * other.y - self.y * other.x,
        }
    }

    pub(super) fn dot(self, other: Self) -> f64 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    pub(super) fn normalized(self) -> Self {
        let length = self.dot(self).sqrt();
        if length < 1.0e-4 {
            return Self {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            };
        }
        Self {
            x: self.x / length,
            y: self.y / length,
            z: self.z / length,
        }
    }

    pub(super) fn distance(self, other: Self) -> f64 {
        self.minus(other).dot(self.minus(other)).sqrt()
    }
}

/// Create packs horizontal positions in half-blocks and vertical positions in
/// whole blocks; a second field carries the vertical smoothing in pixels.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(super) struct TrackNodeLocation {
    pub dimension: String,
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub y_offset_pixels: i32,
}

impl TrackNodeLocation {
    pub fn from_world(dimension: &str, pos: Vec3, y_offset_pixels: i32) -> Self {
        // Math.round in Java breaks negative ties towards positive infinity.
        Self {
            dimension: dimension.to_owned(),
            x: (pos.x * 2.0 + 0.5).floor() as i32,
            y: (pos.y.floor() as i32).saturating_mul(2),
            z: (pos.z * 2.0 + 0.5).floor() as i32,
            y_offset_pixels,
        }
    }

    pub fn world(&self) -> Vec3 {
        Vec3 {
            x: f64::from(self.x) / 2.0,
            y: f64::from(self.y) / 2.0 + f64::from(self.y_offset_pixels) / 16.0,
            z: f64::from(self.z) / 2.0,
        }
    }

    /// Block positions probed by TrackNodeLocation.allAdjacent.
    pub fn adjacent_blocks(&self) -> HashSet<[i32; 3]> {
        let mut result = HashSet::new();
        let center = self.world();
        let center_y = center.y - f64::from(self.y_offset_pixels) / 16.0;
        for x in [-0.125, 0.125] {
            for y in [-0.125, 0.125] {
                for z in [-0.125, 0.125] {
                    result.insert([
                        (center.x + x).floor() as i32,
                        (center_y + y).floor() as i32,
                        (center.z + z).floor() as i32,
                    ]);
                }
            }
        }
        result
    }
}

/// Metadata produced by ITrackBlock.getConnected, before the graph is reduced.
pub(super) struct DiscoveredLocation {
    pub location: TrackNodeLocation,
    pub direction: Option<Vec3>,
    pub normal: Vec3,
    pub material_a: String,
    pub material_b: String,
    pub forced: bool,
}

impl DiscoveredLocation {
    fn deviates_from(&self, direction: Vec3) -> bool {
        self.direction
            .is_some_and(|next| direction.normalized().dot(next.normalized()).abs() < 7.0 / 8.0)
    }
}

/// TrackPropagator.isValidGraphNodeLocation: keep endpoints, junctions, bends,
/// material boundaries and periodic nodes in otherwise long straight tracks.
pub(super) fn is_valid_graph_node(
    current: &DiscoveredLocation,
    next: &[DiscoveredLocation],
    first: bool,
) -> bool {
    if next.len() as isize - isize::from(first) != 1 {
        return true;
    }
    if current.forced
        || current.material_a != current.material_b
        || next.iter().any(|node| node.forced)
        || current
            .direction
            .is_some_and(|direction| next.iter().any(|node| node.deviates_from(direction)))
    {
        return true;
    }

    let pos = current.location.world();
    let centered_x = (pos.x - (pos.x + 0.5).floor()).abs() > 1.0e-5;
    let centered_z = (pos.z - (pos.z + 0.5).floor()).abs() > 1.0e-5;
    if centered_x && !centered_z {
        ((pos.z + 0.5).floor() as i32) % 16 == 0
    } else {
        ((pos.x + 0.5).floor() as i32) % 16 == 0
    }
}

#[derive(Clone, Debug)]
pub(super) struct TrackNode {
    pub net_id: i32,
    pub normal: Vec3,
}

/// A directed straight edge. A curve will carry its own Bezier geometry.
#[derive(Clone, Debug)]
pub(super) struct TrackEdge {
    pub from: TrackNodeLocation,
    pub to: TrackNodeLocation,
    pub material: String,
}

impl TrackEdge {
    pub fn length(&self) -> f64 {
        if self.from.dimension != self.to.dimension {
            0.0
        } else {
            self.from.world().distance(self.to.world())
        }
    }

    pub fn can_travel_to(&self, other: &Self) -> bool {
        if self.from.dimension != self.to.dimension || other.from.dimension != other.to.dimension {
            return true;
        }
        let incoming = self.to.world().minus(self.from.world()).normalized();
        let outgoing = other.to.world().minus(other.from.world()).normalized();
        incoming.dot(outgoing) > 7.0 / 8.0
    }
}

/// TrackGraph's location-indexed nodes and two directed edges per connection.
#[derive(Default)]
pub(super) struct TrackGraph {
    pub nodes: HashMap<TrackNodeLocation, TrackNode>,
    pub connections: HashMap<TrackNodeLocation, HashMap<TrackNodeLocation, TrackEdge>>,
}

impl TrackGraph {
    pub fn create_node_if_absent(&mut self, location: TrackNodeLocation, normal: Vec3) -> bool {
        if self.nodes.contains_key(&location) {
            return false;
        }
        self.nodes.insert(
            location,
            TrackNode {
                net_id: NODE_NET_IDS.fetch_add(1, Ordering::Relaxed) + 1,
                normal,
            },
        );
        true
    }

    pub fn connect_nodes(
        &mut self,
        a: &TrackNodeLocation,
        b: &TrackNodeLocation,
        material: &str,
    ) -> bool {
        if a == b || !self.nodes.contains_key(a) || !self.nodes.contains_key(b) {
            return false;
        }
        if self
            .connections
            .get(a)
            .is_some_and(|connections| connections.contains_key(b))
        {
            return false;
        }
        let forward = TrackEdge {
            from: a.clone(),
            to: b.clone(),
            material: material.to_owned(),
        };
        let reverse = TrackEdge {
            from: b.clone(),
            to: a.clone(),
            material: material.to_owned(),
        };
        self.connections
            .entry(a.clone())
            .or_default()
            .insert(b.clone(), forward);
        self.connections
            .entry(b.clone())
            .or_default()
            .insert(a.clone(), reverse);
        true
    }

    pub fn remove_node(&mut self, location: &TrackNodeLocation) -> bool {
        if self.nodes.remove(location).is_none() {
            return false;
        }
        if let Some(edges) = self.connections.remove(location) {
            for other in edges.keys() {
                if let Some(reverse) = self.connections.get_mut(other) {
                    reverse.remove(location);
                }
            }
        }
        true
    }

    /// Same connectivity partition that TrackGraph.findDisconnectedGraphs uses
    /// before moving each component to a new graph.
    pub fn connected_components(&self) -> Vec<HashSet<TrackNodeLocation>> {
        let mut unseen: HashSet<_> = self.nodes.keys().cloned().collect();
        let mut components = Vec::new();
        while let Some(start) = unseen.iter().next().cloned() {
            unseen.remove(&start);
            let mut component = HashSet::from([start.clone()]);
            let mut queue = VecDeque::from([start]);
            while let Some(node) = queue.pop_front() {
                if let Some(edges) = self.connections.get(&node) {
                    for neighbor in edges.keys() {
                        if unseen.remove(neighbor) {
                            component.insert(neighbor.clone());
                            queue.push_back(neighbor.clone());
                        }
                    }
                }
            }
            components.push(component);
        }
        components
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn node(x: f64, y: f64, z: f64) -> TrackNodeLocation {
        TrackNodeLocation::from_world("minecraft:overworld", Vec3 { x, y, z }, 0)
    }

    fn discovered(x: f64, z: f64, direction: Vec3) -> DiscoveredLocation {
        DiscoveredLocation {
            location: node(x, 64.0, z),
            direction: Some(direction),
            normal: UP,
            material_a: "create:andesite".to_owned(),
            material_b: "create:andesite".to_owned(),
            forced: false,
        }
    }

    #[test]
    fn node_coordinates_match_java_half_block_and_negative_rounding() {
        let location = TrackNodeLocation::from_world(
            "minecraft:overworld",
            Vec3 {
                x: -0.25,
                y: -0.1,
                z: -0.75,
            },
            3,
        );
        // Java Math.round(-0.5) = 0, Math.round(-1.5) = -1.
        assert_eq!((location.x, location.y, location.z), (0, -2, -1));
        assert_eq!(
            location.world(),
            Vec3 {
                x: 0.0,
                y: -0.8125,
                z: -0.5,
            }
        );
        assert_ne!(
            location,
            TrackNodeLocation::from_world(
                "minecraft:the_nether",
                Vec3 {
                    x: -0.25,
                    y: -0.1,
                    z: -0.75,
                },
                3,
            )
        );
        assert_ne!(
            location,
            TrackNodeLocation {
                y_offset_pixels: 0,
                ..location.clone()
            }
        );
    }

    #[test]
    fn adjacent_probes_drop_smoothing_offset() {
        let location = TrackNodeLocation::from_world(
            "minecraft:overworld",
            Vec3 {
                x: 0.0,
                y: 64.0,
                z: 0.0,
            },
            8,
        );
        let blocks = location.adjacent_blocks();
        assert_eq!(blocks.len(), 8);
        assert!(blocks.contains(&[0, 63, 0]));
        assert!(blocks.contains(&[-1, 64, -1]));
        assert!(!blocks.contains(&[0, 65, 0]));
    }

    #[test]
    fn straight_tracks_get_periodic_nodes_every_sixteen_blocks() {
        let along_x = discovered(0.5, 16.0, EAST);
        let farther_x = discovered(1.5, 16.0, EAST);
        assert!(is_valid_graph_node(&along_x, &[farther_x], false));
        assert!(!is_valid_graph_node(
            &discovered(0.5, 15.0, EAST),
            &[discovered(1.5, 15.0, EAST)],
            false
        ));
        assert!(is_valid_graph_node(
            &discovered(16.0, 0.5, EAST),
            &[discovered(17.0, 0.5, EAST)],
            false
        ));
        assert!(!is_valid_graph_node(
            &discovered(15.0, 0.5, EAST),
            &[discovered(16.0, 0.5, EAST)],
            false
        ));
    }

    #[test]
    fn endpoints_junctions_forced_and_material_boundaries_become_nodes() {
        let current = discovered(1.5, 2.0, EAST);
        assert!(is_valid_graph_node(&current, &[], false));
        assert!(is_valid_graph_node(
            &current,
            &[discovered(2.5, 2.0, EAST), discovered(3.5, 2.0, EAST)],
            false
        ));
        assert!(!is_valid_graph_node(
            &current,
            &[discovered(2.5, 2.0, EAST), discovered(3.5, 2.0, EAST)],
            true
        ));
        let mut force = discovered(1.5, 2.0, EAST);
        force.forced = true;
        assert!(is_valid_graph_node(
            &force,
            &[discovered(2.5, 2.0, EAST)],
            false
        ));
        let mut change = discovered(1.5, 2.0, EAST);
        change.material_b = "create:brass".to_owned();
        assert!(is_valid_graph_node(
            &change,
            &[discovered(2.5, 2.0, EAST)],
            false
        ));
    }

    #[test]
    fn node_selection_rejects_straight_and_reverse_but_keeps_a_bend() {
        let current = discovered(1.5, 2.0, EAST);
        assert_eq!(current.normal, UP);
        let west = Vec3 {
            x: -1.0,
            y: 0.0,
            z: 0.0,
        };
        assert!(!is_valid_graph_node(
            &current,
            &[discovered(2.5, 2.0, west)],
            false
        ));
        assert!(is_valid_graph_node(
            &current,
            &[discovered(
                2.5,
                2.0,
                Vec3 {
                    x: 0.0,
                    y: 0.0,
                    z: 1.0
                }
            )],
            false
        ));
        assert!(is_valid_graph_node(
            &current,
            &[DiscoveredLocation {
                forced: true,
                ..discovered(2.5, 2.0, EAST)
            }],
            false
        ));
    }

    #[test]
    fn directed_edges_match_create_turn_threshold_and_cross_dimension_rule() {
        let a = node(0.5, 64.0, 0.5);
        let b = node(4.5, 64.0, 0.5);
        let c = node(8.5, 64.0, 0.5);
        let side = node(8.5, 64.0, 4.5);
        let edge = TrackEdge {
            from: a,
            to: b.clone(),
            material: "create:andesite".into(),
        };
        let forward = TrackEdge {
            from: b.clone(),
            to: c,
            material: edge.material.clone(),
        };
        let turning = TrackEdge {
            from: b.clone(),
            to: side,
            material: edge.material.clone(),
        };
        assert_eq!(edge.length(), 4.0);
        assert!(edge.can_travel_to(&forward));
        assert!(!edge.can_travel_to(&turning));
        let portal = TrackEdge {
            from: b,
            to: TrackNodeLocation::from_world(
                "minecraft:the_nether",
                Vec3 {
                    x: 4.5,
                    y: 64.0,
                    z: 0.5,
                },
                0,
            ),
            material: "create:andesite".into(),
        };
        assert_eq!(portal.length(), 0.0);
        assert!(edge.can_travel_to(&portal));
        assert!(portal.can_travel_to(&forward));
    }

    #[test]
    fn graph_connections_are_bidirectional_and_split_after_removal() {
        let [a, b, c] = [
            node(0.5, 64.0, 0.5),
            node(1.5, 64.0, 0.5),
            node(2.5, 64.0, 0.5),
        ];
        let mut graph = TrackGraph::default();
        assert!(graph.create_node_if_absent(a.clone(), UP));
        assert!(!graph.create_node_if_absent(a.clone(), UP));
        assert!(graph.create_node_if_absent(b.clone(), UP));
        assert!(graph.create_node_if_absent(c.clone(), UP));
        assert_eq!(graph.nodes[&b].net_id, graph.nodes[&a].net_id + 1);
        assert_eq!(graph.nodes[&c].net_id, graph.nodes[&b].net_id + 1);
        assert_eq!(graph.nodes[&a].normal, UP);
        assert!(!graph.connect_nodes(&a, &a, "create:andesite"));
        assert!(graph.connect_nodes(&a, &b, "create:andesite"));
        assert!(graph.connect_nodes(&b, &c, "create:andesite"));
        assert!(!graph.connect_nodes(&a, &b, "create:andesite"));
        assert_eq!(graph.connections[&b][&a].from, b);
        assert_eq!(graph.connections[&b][&a].to, a);
        assert_eq!(graph.connected_components().len(), 1);
        assert!(graph.remove_node(&b));
        assert!(!graph.remove_node(&b));
        assert!(graph.connections[&a].is_empty());
        assert!(graph.connections[&c].is_empty());
        assert_eq!(graph.connected_components().len(), 2);
    }
}

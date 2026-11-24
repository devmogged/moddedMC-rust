//! ITrackBlock.walkConnectedTracks and TrackBlock.getConnected from Create 6.0.10.
//! RailCell distinguishes an ordinary block from unloaded world data.
use super::{
    curve::CurveRecord,
    graph::{DiscoveredLocation, TrackGraph, TrackNodeLocation},
    math::Vec3,
    propagate::{self, PropagationError, TrackWorld},
    track::TrackShape,
};
use moddedmc_api::Pos;
use std::cell::Cell;

#[derive(Clone, Debug, PartialEq)]
pub(super) enum RailCell {
    Rail(TrackShape, Vec<CurveRecord>),
    Other,
    Unloaded,
    Unsupported,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum GraphReadError {
    MissingRail,
    UnknownWorldData,
    UnsupportedTurnOrPortal,
    Propagation(PropagationError),
}

pub(super) struct RailWorld<'a, F> {
    pub dimension: &'a str,
    read: F,
    unknown: Cell<bool>,
    unsupported: Cell<bool>,
}

impl<'a, F> RailWorld<'a, F>
where
    F: Fn(Pos) -> RailCell,
{
    pub fn new(dimension: &'a str, read: F) -> Self {
        Self {
            dimension,
            read,
            unknown: Cell::new(false),
            unsupported: Cell::new(false),
        }
    }

    fn connected_from_block(
        &self,
        pos: Pos,
        shape: TrackShape,
        curves: Vec<CurveRecord>,
        from: Option<&TrackNodeLocation>,
    ) -> Vec<DiscoveredLocation> {
        // Portals and turn-bearing endpoints need BE and dimension resolution.
        if matches!(
            shape,
            TrackShape::None
                | TrackShape::TeleportNorth
                | TrackShape::TeleportSouth
                | TrackShape::TeleportEast
                | TrackShape::TeleportWest
        ) {
            self.unsupported.set(true);
            return Vec::new();
        }
        let axes = shape.axes();
        let mut result = Vec::with_capacity(axes.len() * 2);
        let center = Vec3 {
            x: f64::from(pos.x) + 0.5,
            y: f64::from(pos.y) + if axes[0].y != 0.0 { 0.5 } else { 0.0 },
            z: f64::from(pos.z) + 0.5,
        };
        for &axis in axes {
            let first =
                TrackNodeLocation::from_world(self.dimension, center.plus(axis.scale(0.5)), 0);
            let second =
                TrackNodeLocation::from_world(self.dimension, center.minus(axis.scale(0.5)), 0);
            let make = |location| DiscoveredLocation {
                location,
                direction: Some(axis.normalized()),
                normal: shape.normal(),
                material_a: "create:andesite".to_owned(),
                material_b: "create:andesite".to_owned(),
                forced: false,
                turn: None,
            };
            match from {
                None => {
                    result.push(make(first));
                    result.push(make(second));
                }
                Some(node) if node == &first && node == &second => {}
                Some(node) if node == &first => result.push(make(second)),
                Some(node) if node == &second => result.push(make(first)),
                Some(_) => {}
            }
        }
        for curve in curves {
            let [a, b] = curve
                .starts
                .map(|start| TrackNodeLocation::from_world(self.dimension, start, 0));
            if from.is_none() || from == Some(&a) {
                result.push(DiscoveredLocation {
                    location: b,
                    direction: None,
                    normal: curve.normals[1],
                    material_a: curve.material.clone(),
                    material_b: curve.material.clone(),
                    forced: true,
                    turn: Some(curve.clone()),
                });
            }
            if from.is_none() {
                result.push(DiscoveredLocation {
                    location: a,
                    direction: None,
                    normal: curve.normals[0],
                    material_a: curve.material.clone(),
                    material_b: curve.material.clone(),
                    forced: true,
                    turn: Some(curve.secondary()),
                });
            }
        }
        result
    }

    pub fn cell(&self, pos: Pos) -> RailCell {
        (self.read)(pos)
    }

    pub fn initial_ends(&self, pos: Pos) -> Result<Vec<DiscoveredLocation>, GraphReadError> {
        match (self.read)(pos) {
            RailCell::Rail(shape, curves) => {
                let ends = self.connected_from_block(pos, shape, curves, None);
                if self.unsupported.get() {
                    Err(GraphReadError::UnsupportedTurnOrPortal)
                } else {
                    Ok(ends)
                }
            }
            RailCell::Other => Err(GraphReadError::MissingRail),
            RailCell::Unloaded => Err(GraphReadError::UnknownWorldData),
            RailCell::Unsupported => Err(GraphReadError::UnsupportedTurnOrPortal),
        }
    }

    pub fn graph_at(&self, pos: Pos) -> Result<TrackGraph, GraphReadError> {
        self.unknown.set(false);
        self.unsupported.set(false);
        let ends = self.initial_ends(pos)?;
        if ends.is_empty() {
            return Err(GraphReadError::MissingRail);
        }
        let graph = propagate::discover_graph(self, &ends).map_err(GraphReadError::Propagation)?;
        if self.unknown.get() {
            return Err(GraphReadError::UnknownWorldData);
        }
        if self.unsupported.get() {
            return Err(GraphReadError::UnsupportedTurnOrPortal);
        }
        Ok(graph)
    }
}

impl<F> TrackWorld for RailWorld<'_, F>
where
    F: Fn(Pos) -> RailCell,
{
    fn walk_connected(&self, from: &TrackNodeLocation) -> Vec<DiscoveredLocation> {
        if from.dimension != self.dimension {
            return Vec::new();
        }
        let mut result = Vec::new();
        for [x, y, z] in from.adjacent_blocks() {
            let pos = Pos { x, y, z };
            match (self.read)(pos) {
                RailCell::Rail(shape, curves) => {
                    result.extend(self.connected_from_block(pos, shape, curves, Some(from)))
                }
                RailCell::Unloaded => self.unknown.set(true),
                RailCell::Unsupported => self.unsupported.set(true),
                RailCell::Other => {}
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};
    fn p(x: i32, y: i32, z: i32) -> Pos {
        Pos { x, y, z }
    }
    struct Fixture {
        tracks: HashMap<[i32; 3], TrackShape>,
        unloaded: HashSet<[i32; 3]>,
    }
    impl Fixture {
        fn empty() -> Self {
            Self {
                tracks: HashMap::new(),
                unloaded: HashSet::new(),
            }
        }
        fn insert(&mut self, pos: Pos, shape: TrackShape) {
            self.tracks.insert([pos.x, pos.y, pos.z], shape);
        }
        fn read(&self, pos: Pos) -> RailCell {
            let key = [pos.x, pos.y, pos.z];
            if self.unloaded.contains(&key) {
                RailCell::Unloaded
            } else if let Some(shape) = self.tracks.get(&key) {
                RailCell::Rail(*shape, Vec::new())
            } else {
                RailCell::Other
            }
        }
    }

    #[test]
    fn long_x_rail_produces_java_periodic_nodes_and_dimension() {
        let mut sample = Fixture::empty();
        for x in 0..=32 {
            sample.insert(p(x, 64, 2), TrackShape::XOrtho);
        }
        let world = RailWorld::new("minecraft:overworld", |pos| sample.read(pos));
        let graph = world.graph_at(p(8, 64, 2)).unwrap();
        // Thirty-three rail blocks occupy x=0..32; the far end is x=33.
        // Create inserts periodic nodes at x=16 and x=32 as well.
        assert_eq!(graph.nodes.len(), 4);
        assert_eq!(
            graph.connections.values().map(HashMap::len).sum::<usize>(),
            6
        );
        for x in [0, 16, 32, 33] {
            let loc = TrackNodeLocation::from_world(
                "minecraft:overworld",
                Vec3 {
                    x: f64::from(x),
                    y: 64.0,
                    z: 2.5,
                },
                0,
            );
            assert!(graph.nodes.contains_key(&loc));
        }
        let nether = RailWorld::new("minecraft:the_nether", |pos| sample.read(pos));
        assert!(
            nether
                .graph_at(p(8, 64, 2))
                .unwrap()
                .nodes
                .keys()
                .all(|loc| loc.dimension == "minecraft:the_nether")
        );
    }

    #[test]
    fn two_adjacent_rails_share_endpoints() {
        let mut sample = Fixture::empty();
        sample.insert(p(5, 64, 7), TrackShape::XOrtho);
        sample.insert(p(6, 64, 7), TrackShape::XOrtho);
        let world = RailWorld::new("minecraft:overworld", |pos| sample.read(pos));
        assert_eq!(world.initial_ends(p(5, 64, 7)).unwrap().len(), 2);
        let graph = world.graph_at(p(5, 64, 7)).unwrap();
        assert_eq!(graph.nodes.len(), 2);
        assert_eq!(
            graph.connections.values().map(HashMap::len).sum::<usize>(),
            2
        );
    }

    #[test]
    fn crossing_tracks_are_two_non_switching_directions() {
        let mut sample = Fixture::empty();
        for x in 0..=4 {
            sample.insert(p(x, 64, 0), TrackShape::XOrtho);
        }
        sample.insert(p(2, 64, 0), TrackShape::CrossOrtho);
        for z in 1..=2 {
            sample.insert(p(2, 64, z), TrackShape::ZOrtho);
        }
        let world = RailWorld::new("minecraft:overworld", |pos| sample.read(pos));
        // CrossOrtho has two independent axes; a crossing is not a
        // train switch and does not connect the east/west to north/south.
        let x_graph = world.graph_at(p(1, 64, 0)).unwrap();
        let z_graph = world.graph_at(p(2, 64, 1)).unwrap();
        assert_eq!(x_graph.connected_components().len(), 1);
        assert_eq!(z_graph.connected_components().len(), 1);
        assert!(x_graph.nodes.values().all(|node| node.normal.y > 0.0));
        assert!(x_graph.nodes.keys().all(|loc| loc.z == 1));
        assert!(z_graph.nodes.keys().all(|loc| loc.x == 5));
    }

    #[test]
    fn diagonal_rails_share_corner_points() {
        let mut sample = Fixture::empty();
        for n in 0..=4 {
            sample.insert(p(n, 64, n), TrackShape::PositiveDiagonal);
        }
        let world = RailWorld::new("minecraft:overworld", |pos| sample.read(pos));
        let graph = world.graph_at(p(2, 64, 2)).unwrap();
        assert_eq!(graph.nodes.len(), 2);
        assert_eq!(
            graph.connections.values().map(HashMap::len).sum::<usize>(),
            2
        );
    }

    #[test]
    fn unloaded_neighbor_cannot_be_written_as_truncated_graph() {
        let mut sample = Fixture::empty();
        sample.insert(p(0, 64, 0), TrackShape::XOrtho);
        sample.insert(p(1, 64, 0), TrackShape::XOrtho);
        sample.unloaded.insert([2, 64, 0]);
        let world = RailWorld::new("minecraft:overworld", |pos| sample.read(pos));
        assert!(matches!(
            world.graph_at(p(0, 64, 0)),
            Err(GraphReadError::UnknownWorldData)
        ));
    }

    #[test]
    fn missing_or_portal_track_refuses_to_fabricate_graph() {
        let mut sample = Fixture::empty();
        let world = RailWorld::new("minecraft:overworld", |pos| sample.read(pos));
        assert!(matches!(
            world.graph_at(p(0, 64, 0)),
            Err(GraphReadError::MissingRail)
        ));
        sample.insert(p(0, 64, 0), TrackShape::TeleportNorth);
        let world = RailWorld::new("minecraft:overworld", |pos| sample.read(pos));
        assert!(matches!(
            world.graph_at(p(0, 64, 0)),
            Err(GraphReadError::UnsupportedTurnOrPortal)
        ));
    }
}

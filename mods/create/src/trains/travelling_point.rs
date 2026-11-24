//! A wheel's directed position on a Create track graph.

use serde::{Deserialize, Serialize};

use super::{
    graph::{TrackEdge, TrackGraph, TrackNodeLocation},
    math::Vec3,
};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(super) struct TravellingPoint {
    from: TrackNodeLocation,
    to: TrackNodeLocation,
    position: f64,
}

impl TravellingPoint {
    pub(super) fn new(edge: &TrackEdge, position: f64) -> Self {
        Self {
            from: edge.from.clone(),
            to: edge.to.clone(),
            position,
        }
    }

    pub(super) fn location(&self, graph: &TrackGraph) -> Option<Vec3> {
        let edge = graph.edge(&self.from, &self.to)?;
        let length = edge.length();
        (length > 0.0 && (0.0..=length).contains(&self.position))
            .then(|| edge.position(self.position / length))
    }

    /// Follow connected edges until the distance is exhausted or no edge continues the route.
    /// During assembly, both wheel points must remain on the station's straight track run.
    pub(super) fn travel_straight(&mut self, graph: &TrackGraph, distance: f64) -> Option<f64> {
        let mut edge = graph.edge(&self.from, &self.to)?;
        let mut length = edge.length();
        if edge.turn.is_some() || length == 0.0 || !(0.0..=length).contains(&self.position) {
            return None;
        }
        let mut position = self.position + distance;
        let mut travelled = distance;
        while position > length || position < 0.0 {
            if position > length {
                let overflow = position - length;
                let next = graph.connections.get(&edge.to)?.values().find(|candidate| {
                    candidate.turn.is_none()
                        && candidate.to != edge.from
                        && candidate.length() > 0.0
                        && edge.can_travel_to(candidate)
                });
                let Some(next) = next else {
                    position = length;
                    travelled -= overflow;
                    break;
                };
                edge = next;
                length = edge.length();
                position = overflow;
            } else {
                let prior = graph.connections.get(&edge.from)?.keys().find_map(|node| {
                    let candidate = graph.edge(node, &edge.from)?;
                    (candidate.turn.is_none()
                        && candidate.from != edge.to
                        && candidate.length() > 0.0
                        && candidate.can_travel_to(edge))
                    .then_some(candidate)
                });
                let Some(prior) = prior else {
                    travelled -= position;
                    position = 0.0;
                    break;
                };
                edge = prior;
                length = edge.length();
                position += length;
            }
        }
        self.from.clone_from(&edge.from);
        self.to.clone_from(&edge.to);
        self.position = position;
        Some(travelled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(x: f64) -> TrackNodeLocation {
        TrackNodeLocation::from_world("minecraft:overworld", Vec3 { x, y: 8.0, z: 2.5 }, 0)
    }

    #[test]
    fn wheel_points_cross_a_graph_node_in_both_directions() {
        let [a, b, c] = [node(10.0), node(16.0), node(20.0)];
        let mut graph = TrackGraph::default();
        let up = Vec3 {
            x: 0.0,
            y: 1.0,
            z: 0.0,
        };
        for node in [&a, &b, &c] {
            graph.create_node_if_absent(node.clone(), up);
        }
        graph.connect_nodes(&a, &b, "create:standard");
        graph.connect_nodes(&b, &c, "create:standard");
        let mut wheel = TravellingPoint::new(graph.edge(&a, &b).unwrap(), 5.5);
        assert_eq!(wheel.travel_straight(&graph, 1.0), Some(1.0));
        assert_eq!(wheel.location(&graph).unwrap().x, 16.5);
        assert_eq!(wheel.travel_straight(&graph, -1.0), Some(-1.0));
        assert_eq!(wheel.location(&graph).unwrap().x, 15.5);
        assert_eq!(wheel.travel_straight(&graph, -6.0), Some(-5.5));
        assert_eq!(wheel.location(&graph).unwrap().x, 10.0);
    }
}

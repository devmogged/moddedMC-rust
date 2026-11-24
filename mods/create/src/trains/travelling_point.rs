//! A wheel's directed position on a Create track graph.

use serde::{Deserialize, Serialize};
use std::collections::{HashSet, VecDeque};

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

    pub(super) fn edge<'a>(&self, graph: &'a TrackGraph) -> Option<&'a TrackEdge> {
        graph.edge(&self.from, &self.to)
    }

    pub(super) fn offset(&self) -> f64 {
        self.position
    }

    pub(super) fn location(&self, graph: &TrackGraph) -> Option<Vec3> {
        let edge = self.edge(graph)?;
        let length = edge.length();
        (length > 0.0 && (0.0..=length).contains(&self.position))
            .then(|| edge.position(self.position / length))
    }

    /// During assembly, both wheel points must stay on the station's straight run.
    pub(super) fn travel_straight(&mut self, graph: &TrackGraph, distance: f64) -> Option<f64> {
        self.travel(graph, distance, |edge, forward| {
            next_edges(graph, edge, forward).find(|edge| edge.turn.is_none())
        })
    }

    pub(super) fn travel<'a>(
        &mut self,
        graph: &'a TrackGraph,
        distance: f64,
        mut select: impl FnMut(&'a TrackEdge, bool) -> Option<&'a TrackEdge>,
    ) -> Option<f64> {
        let mut edge = self.edge(graph)?;
        let mut position = self.position;
        if edge.length() == 0.0 || !(0.0..=edge.length()).contains(&position) {
            return None;
        }
        let forward = distance >= 0.0;
        let mut remaining = distance;
        loop {
            let length = edge.length();
            let t = position / length;
            let increment = if let Some(turn) = &edge.turn {
                if remaining.abs() <= 5.0 {
                    turn.geometry.increment_t(t, remaining / length) - t
                } else {
                    remaining / length
                }
            } else {
                remaining / length
            };
            let next = position + increment * length;
            if (0.0..=length).contains(&next) {
                position = next;
                remaining = 0.0;
                break;
            }
            let end = if forward { length } else { 0.0 };
            remaining *= ((end - next) / (position - next)).clamp(0.0, 1.0);
            let Some(next_edge) = select(edge, forward) else {
                position = end;
                break;
            };
            edge = next_edge;
            position = if forward { 0.0 } else { edge.length() };
        }
        self.from.clone_from(&edge.from);
        self.to.clone_from(&edge.to);
        self.position = position;
        Some(distance - remaining)
    }
}

pub(super) fn next_edges<'a>(
    graph: &'a TrackGraph,
    edge: &'a TrackEdge,
    forward: bool,
) -> impl Iterator<Item = &'a TrackEdge> {
    let (junction, behind) = if forward {
        (&edge.to, &edge.from)
    } else {
        (&edge.from, &edge.to)
    };
    graph
        .connections
        .get(junction)
        .into_iter()
        .flatten()
        .filter_map(move |(node, next)| {
            if node == behind {
                return None;
            }
            let (candidate, compatible) = if forward {
                (next, edge.can_travel_to(next))
            } else {
                let previous = graph.edge(node, junction)?;
                (previous, previous.can_travel_to(edge))
            };
            (compatible && candidate.length() > 0.0).then_some(candidate)
        })
}

/// TravellingPoint.follow uses the wheel in front of this one to resolve a junction.
fn distance_to_edge(
    graph: &TrackGraph,
    first: &TrackEdge,
    target: &TrackEdge,
    forward: bool,
) -> usize {
    let mut pending = VecDeque::from([(first, 0)]);
    let mut visited = HashSet::new();
    while let Some((edge, depth)) = pending.pop_front() {
        if std::ptr::eq(edge, target) {
            return depth;
        }
        if visited.len() >= 20 {
            break;
        }
        if visited.insert((&edge.from, &edge.to)) {
            pending.extend(next_edges(graph, edge, forward).map(|next| (next, depth + 1)));
        }
    }
    usize::MAX
}

pub(super) fn select_edge<'a>(
    graph: &'a TrackGraph,
    edge: &'a TrackEdge,
    forward: bool,
    steer: f64,
    follow: Option<&TrackEdge>,
) -> Option<&'a TrackEdge> {
    let mut choices = next_edges(graph, edge, forward).peekable();
    let first = choices.next()?;
    if choices.peek().is_none() {
        return Some(first);
    }
    let junction = if forward { &edge.to } else { &edge.from };
    let up = graph.nodes.get(junction)?.normal;
    let normal = edge.direction(!forward).cross(up);
    std::iter::once(first).chain(choices).min_by(|a, b| {
        let score = |candidate: &TrackEdge| {
            let path = follow.map_or(0, |target| {
                distance_to_edge(graph, candidate, target, forward)
            });
            let dot = normal.dot(candidate.direction(forward));
            (path, (steer - dot).abs())
        };
        let (a_path, a_angle) = score(a);
        let (b_path, b_angle) = score(b);
        a_path
            .cmp(&b_path)
            .then_with(|| a_angle.total_cmp(&b_angle))
            .then_with(|| {
                let node = |edge: &'a TrackEdge| if forward { &edge.to } else { &edge.from };
                let (a, b) = (node(a), node(b));
                (a.x, a.y, a.z, a.y_offset_pixels).cmp(&(b.x, b.y, b.z, b.y_offset_pixels))
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(x: f64) -> TrackNodeLocation {
        TrackNodeLocation::from_world("minecraft:overworld", Vec3 { x, y: 8.0, z: 2.5 }, 0)
    }

    #[test]
    fn following_wheels_keep_the_leading_wheels_junction_choice() {
        let [start, junction] = [node(0.0), node(8.0)];
        let mut left = node(16.0);
        let mut right = left.clone();
        left.z -= 4;
        right.z += 4;
        let up = Vec3 {
            x: 0.0,
            y: 1.0,
            z: 0.0,
        };
        let mut graph = TrackGraph::default();
        for point in [&start, &junction, &left, &right] {
            graph.create_node_if_absent(point.clone(), up);
        }
        for (a, b) in [(&start, &junction), (&junction, &left), (&junction, &right)] {
            graph.connect_nodes(a, b, "create:standard");
        }
        let entrance = graph.edge(&start, &junction).unwrap();
        let turn = select_edge(&graph, entrance, true, 1.0, None).unwrap();
        assert_eq!(turn.to, right);
        assert_eq!(
            select_edge(&graph, entrance, true, -1.0, None).unwrap().to,
            left
        );
        assert_eq!(
            select_edge(&graph, entrance, true, -1.0, Some(turn))
                .unwrap()
                .to,
            right
        );
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

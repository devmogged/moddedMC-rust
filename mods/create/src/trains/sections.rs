//! SignalPropagator's connected sections separated by signal boundaries.
use super::graph::{TrackEdge, TrackGraph, TrackNodeLocation};
use std::collections::{HashMap, HashSet, VecDeque};

pub(super) struct Cut<'a> {
    pub edge: &'a TrackEdge,
    pub distance: f64,
}

type DirectedEdge<'a> = (&'a TrackNodeLocation, &'a TrackNodeLocation);

struct Boundary {
    distance: f64,
    sides: [usize; 2],
}

pub(super) struct Sections<'a> {
    nodes: HashMap<&'a TrackNodeLocation, usize>,
    boundaries: HashMap<DirectedEdge<'a>, Vec<Boundary>>,
    pub adjacent: Vec<[usize; 2]>,
}

impl<'a> Sections<'a> {
    pub(super) fn new(graph: &'a TrackGraph, cuts: &[Cut<'a>]) -> Self {
        let nodes: HashMap<_, _> = graph
            .nodes
            .keys()
            .enumerate()
            .map(|(id, node)| (node, id))
            .collect();
        let mut boundaries: HashMap<DirectedEdge<'_>, Vec<Boundary>> = HashMap::new();
        for (i, cut) in cuts.iter().enumerate() {
            let edge = cut.edge;
            let ids = [nodes.len() + i * 2, nodes.len() + i * 2 + 1];
            let (ends, distance, sides) = if nodes[&edge.from] < nodes[&edge.to] {
                ((&edge.from, &edge.to), cut.distance, ids)
            } else {
                (
                    (&edge.to, &edge.from),
                    edge.length() - cut.distance,
                    [ids[1], ids[0]],
                )
            };
            boundaries
                .entry(ends)
                .or_default()
                .push(Boundary { distance, sides });
        }

        let mut group = Groups {
            parents: (0..nodes.len() + cuts.len() * 2).collect(),
        };
        for edge in graph.connections.values().flat_map(HashMap::values) {
            let (from, to) = (nodes[&edge.from], nodes[&edge.to]);
            if from > to {
                continue;
            }
            let mut next = from;
            if let Some(boundaries) = boundaries.get_mut(&(&edge.from, &edge.to)) {
                boundaries.sort_by(|a, b| a.distance.total_cmp(&b.distance));
                let mut prior: Option<&Boundary> = None;
                for boundary in boundaries {
                    let [before, after] = boundary.sides;
                    if let Some(prior) = prior
                        && (prior.distance - boundary.distance).abs() < 1.0e-6
                    {
                        // Several signal boxes can share both sides of one boundary.
                        group.join(before, prior.sides[0]);
                        group.join(after, prior.sides[1]);
                    } else {
                        group.join(next, before);
                        next = after;
                    }
                    prior = Some(boundary);
                }
            }
            group.join(next, to);
        }
        let adjacent = (0..cuts.len())
            .map(|i| {
                let id = nodes.len() + i * 2;
                [group.root(id), group.root(id + 1)]
            })
            .collect();
        let nodes = nodes
            .into_iter()
            .map(|(node, id)| (node, group.root(id)))
            .collect();
        for cut in boundaries.values_mut().flatten() {
            cut.sides = cut.sides.map(|id| group.root(id));
        }
        Self {
            nodes,
            boundaries,
            adjacent,
        }
    }

    pub(super) fn occupy(&self, edge: &TrackEdge, from: f64, to: f64, groups: &mut HashSet<usize>) {
        let (cuts, start, end) = if let Some(cuts) = self.boundaries.get(&(&edge.from, &edge.to)) {
            (cuts, from.min(to), from.max(to))
        } else if let Some(cuts) = self.boundaries.get(&(&edge.to, &edge.from)) {
            (
                cuts,
                edge.length() - from.max(to),
                edge.length() - from.min(to),
            )
        } else {
            if let Some(group) = self.nodes.get(&edge.from) {
                groups.insert(*group);
            }
            return;
        };
        for cut in cuts {
            if cut.distance < start {
                continue;
            }
            groups.insert(cut.sides[0]);
            if end < cut.distance {
                return;
            }
        }
        if let Some(last) = cuts.last() {
            groups.insert(last.sides[1]);
        }
    }
}

#[cfg(test)]
fn sections(graph: &TrackGraph, cuts: &[Cut<'_>]) -> Vec<[usize; 2]> {
    Sections::new(graph, cuts).adjacent
}

/// Walk from the outgoing side until the first signal on each navigable route.
pub(super) fn chained(graph: &TrackGraph, cuts: &[Cut<'_>], start: usize) -> Vec<usize> {
    let start = &cuts[start];
    let mut downstream = Vec::new();
    let mut visited = HashSet::new();
    let mut frontier = VecDeque::from([(start.edge, start.distance)]);
    while let Some((edge, minimum)) = frontier.pop_front() {
        if !visited.insert((&edge.from, &edge.to)) {
            continue;
        }
        visited.insert((&edge.to, &edge.from));
        let mut nearest = f64::INFINITY;
        let mut same_direction = Vec::new();
        for (index, cut) in cuts.iter().enumerate() {
            let forwards = edge.from == cut.edge.from && edge.to == cut.edge.to;
            let distance = if forwards {
                cut.distance
            } else if edge.from == cut.edge.to && edge.to == cut.edge.from {
                edge.length() - cut.distance
            } else {
                continue;
            };
            if distance <= minimum + 1.0e-6 || distance > nearest + 1.0e-6 {
                continue;
            }
            if distance < nearest - 1.0e-6 {
                nearest = distance;
                same_direction.clear();
            }
            if forwards {
                same_direction.push(index);
            }
        }
        if nearest.is_finite() {
            downstream.extend(same_direction);
        } else if let Some(next) = graph.connections.get(&edge.to) {
            frontier.extend(
                next.values()
                    .filter(|next| edge.can_travel_to(next))
                    .map(|next| (next, 0.0)),
            );
        }
    }
    downstream
}

struct Groups {
    parents: Vec<usize>,
}

impl Groups {
    fn root(&mut self, mut node: usize) -> usize {
        while node != self.parents[node] {
            self.parents[node] = self.parents[self.parents[node]];
            node = self.parents[node];
        }
        node
    }

    fn join(&mut self, a: usize, b: usize) {
        let (a, b) = (self.root(a), self.root(b));
        self.parents[b] = a;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trains::math::Vec3;

    #[test]
    fn train_span_occupies_intermediate_sections_in_either_edge_direction() {
        let node = |x| {
            TrackNodeLocation::from_world("minecraft:overworld", Vec3 { x, y: 64.0, z: 0.5 }, 0)
        };
        let (a, b) = (node(0.0), node(16.0));
        let mut graph = TrackGraph::default();
        for node in [&a, &b] {
            graph.create_node_if_absent(
                node.clone(),
                Vec3 {
                    x: 0.0,
                    y: 1.0,
                    z: 0.0,
                },
            );
        }
        graph.connect_nodes(&a, &b, "create:standard");
        let east = graph.edge(&a, &b).unwrap();
        let west = graph.edge(&b, &a).unwrap();
        let cuts = [4.0, 8.0, 12.0].map(|distance| Cut {
            edge: east,
            distance,
        });
        let sections = Sections::new(&graph, &cuts);
        let mut groups = HashSet::new();
        let expected = HashSet::from([sections.adjacent[0][1], sections.adjacent[1][1]]);
        sections.occupy(east, 5.0, 11.0, &mut groups);
        assert_eq!(groups, expected);
        groups.clear();
        sections.occupy(west, 5.0, 11.0, &mut groups);
        assert_eq!(groups, expected);
        groups.clear();
        sections.occupy(west, 4.0, 4.0, &mut groups);
        assert_eq!(groups, HashSet::from(sections.adjacent[2]));
    }

    #[test]
    fn loop_needs_two_distinct_boundaries_and_reversed_boxes_share_one_cut() {
        let node =
            |x, z| TrackNodeLocation::from_world("minecraft:overworld", Vec3 { x, y: 64.0, z }, 0);
        let [a, b, c, d] = [
            node(0.0, 0.0),
            node(8.0, 0.0),
            node(8.0, 8.0),
            node(0.0, 8.0),
        ];
        let mut graph = TrackGraph::default();
        for node in [&a, &b, &c, &d] {
            graph.create_node_if_absent(
                node.clone(),
                Vec3 {
                    x: 0.0,
                    y: 1.0,
                    z: 0.0,
                },
            );
        }
        for (from, to) in [(&a, &b), (&b, &c), (&c, &d), (&d, &a)] {
            assert!(graph.connect_nodes(from, to, "create:andesite"));
        }
        let cut = |from: &TrackNodeLocation, to: &TrackNodeLocation, distance| Cut {
            edge: &graph.connections[from][to],
            distance,
        };
        let isolated = sections(&graph, &[cut(&a, &b, 2.0)]);
        assert_eq!(isolated[0][0], isolated[0][1]);

        let cuts = [cut(&a, &b, 2.0), cut(&b, &a, 6.0), cut(&a, &b, 5.0)];
        let split = sections(&graph, &cuts);
        assert_eq!(chained(&graph, &cuts, 0), [2]);
        assert_ne!(split[0][0], split[0][1]);
        assert_eq!(split[1], [split[0][1], split[0][0]]);
        assert_eq!(split[2], [split[0][1], split[0][0]]);
    }
}

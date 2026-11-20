//! ElectricalNetwork, NetworkPathManager and NetworkPathConductivityContext.
//! Create: New Age 1.2.0 (NeoForge 1.21.1), source branch 1.21.1.
//!
//! This is the synchronous graph/throughput kernel, not a world connector host.
//! Callers supply connector identities and accepted energy amounts. No mod ABI
//! has been frozen; source ordering for equal-length paths needs a Java oracle.

use std::collections::{HashMap, HashSet, VecDeque};

type Connector = u32;
type Edge = (Connector, Connector);

fn pair(a: Connector, b: Connector) -> Edge {
    if a <= b { (a, b) } else { (b, a) }
}

#[derive(Clone, Copy)]
struct Conductivity {
    maximum: i64,
    available: i64,
    last_updated_tick: u64,
}

/// Source NetworkPathManager with lazy conductivity refill each game tick.
/// The first tick after registering an edge has an empty budget, as in Java.
pub struct ElectricalNetwork {
    links: HashMap<Edge, Conductivity>,
    neighbours: HashMap<Connector, Vec<Connector>>,
    shortest_paths: HashMap<Edge, Option<Vec<Connector>>>,
    tick: u64,
    max_path_depth: u32,
}

impl ElectricalNetwork {
    pub fn new(max_path_depth: u32) -> Self {
        Self {
            links: HashMap::new(),
            neighbours: HashMap::new(),
            shortest_paths: HashMap::new(),
            tick: 0,
            max_path_depth,
        }
    }

    pub fn add_connection(&mut self, a: Connector, b: Connector, conductivity: i64) {
        if a == b {
            return;
        }
        let key = pair(a, b);
        if let std::collections::hash_map::Entry::Vacant(entry) = self.links.entry(key) {
            entry.insert(Conductivity {
                maximum: conductivity,
                available: 0,
                last_updated_tick: self.tick,
            });
            self.neighbours.entry(a).or_default().push(b);
            self.neighbours.entry(b).or_default().push(a);
        }
        // Java clears all cached shortest paths even on an existing connection.
        self.shortest_paths.clear();
    }

    pub fn remove_connection(&mut self, a: Connector, b: Connector) {
        if self.links.remove(&pair(a, b)).is_some() {
            if let Some(neighbours) = self.neighbours.get_mut(&a) {
                neighbours.retain(|&other| other != b);
            }
            if let Some(neighbours) = self.neighbours.get_mut(&b) {
                neighbours.retain(|&other| other != a);
            }
        }
        self.shortest_paths.clear();
    }

    /// NetworkPathConductivityContext.updateConductivity increments the
    /// generation. Each edge gets its whole budget on its first read next tick.
    pub fn tick(&mut self) {
        self.tick = self.tick.wrapping_add(1);
    }

    fn find_path(&self, start: Connector, target: Connector) -> Option<Vec<Connector>> {
        let mut visited = HashSet::from([start]);
        let mut queue = VecDeque::from([(start, vec![start])]);
        while let Some((node, path)) = queue.pop_front() {
            if node == target && path.len() >= 2 {
                return Some(path);
            }
            if path.len() > self.max_path_depth as usize {
                continue;
            }
            if let Some(neighbours) = self.neighbours.get(&node) {
                for &next in neighbours {
                    if visited.insert(next) {
                        let mut next_path = path.clone();
                        next_path.push(next);
                        queue.push_back((next, next_path));
                    }
                }
            }
        }
        None
    }

    fn shortest(&mut self, from: Connector, to: Connector) -> Option<Vec<Connector>> {
        let key = pair(from, to);
        if !self.shortest_paths.contains_key(&key) {
            let path = self.find_path(from, to);
            self.shortest_paths.insert(key, path);
        }
        let mut path = self.shortest_paths.get(&key)?.clone()?;
        // The path cache is symmetrical, but the transfer direction is not.
        if path.first() == Some(&to) {
            path.reverse();
        }
        Some(path)
    }

    fn available(&mut self, from: Connector, to: Connector) -> Option<i64> {
        let edge = self.links.get_mut(&pair(from, to))?;
        if edge.last_updated_tick != self.tick {
            edge.available = edge.maximum;
            edge.last_updated_tick = self.tick;
        }
        Some(edge.available)
    }

    /// One energy handler insert along the shortest path. Java does not use
    /// a longer path when this shortest route is saturated.
    pub fn insert(
        &mut self,
        from: Connector,
        to: Connector,
        requested: i64,
        receiver_acceptance: i64,
    ) -> i64 {
        if requested <= 0 || receiver_acceptance <= 0 {
            return 0;
        }
        let Some(path) = self.shortest(from, to) else {
            return 0;
        };
        let mut budget = i64::MAX;
        for edge in path.windows(2) {
            let Some(available) = self.available(edge[0], edge[1]) else {
                return 0;
            };
            budget = budget.min(available);
        }
        if budget <= 0 {
            return 0;
        }
        let transferred = requested.min(receiver_acceptance).min(budget);
        for edge in path.windows(2) {
            if let Some(available) = self.available(edge[0], edge[1])
                && let Some(link) = self.links.get_mut(&pair(edge[0], edge[1]))
            {
                link.available = available - transferred;
            }
        }
        transferred
    }

    /// `ElectricalNetwork.insert` splits its requested amount by the total
    /// consumer count, including the sender (which is skipped), then revisits
    /// receivers until none can accept more. This operation uses each
    /// receiver's total available capacity; external per-call FE transfer
    /// ceilings remain a separate energy-handler responsibility.
    pub fn distribute(
        &mut self,
        source: Connector,
        requested: i64,
        consumers: &mut [(Connector, i64)],
    ) -> i64 {
        if requested <= 0 || consumers.is_empty() {
            return 0;
        }
        let share =
            requested / consumers.len() as i64 + i64::from(requested % consumers.len() as i64 != 0);
        let mut inserted = 0;
        let mut any = true;
        while inserted < requested && any {
            any = false;
            for (recipient, remaining) in consumers.iter_mut() {
                if *recipient == source {
                    continue;
                }
                let amount = share.min(requested - inserted);
                let taken = self.insert(source, *recipient, amount, *remaining);
                if taken > 0 {
                    *remaining -= taken;
                    inserted += taken;
                    any = true;
                }
            }
        }
        inserted
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortest_saturated_route_does_not_fall_back_to_longer_route() {
        // Short path 1->2->4 only carries 1 FE/t, longer 1->3->5->4
        // carries 8 FE/t. Java caches the shortest and refuses fallback.
        let mut network = ElectricalNetwork::new(24);
        for (a, b, rate) in [(1, 2, 1), (2, 4, 1), (1, 3, 8), (3, 5, 8), (5, 4, 8)] {
            network.add_connection(a, b, rate);
        }
        assert_eq!(network.insert(1, 4, 10, 10), 0); // new link, tick 0
        network.tick();
        assert_eq!(network.insert(1, 4, 10, 10), 1);
        assert_eq!(network.insert(1, 4, 10, 10), 0);
        network.tick();
        assert_eq!(network.insert(4, 1, 10, 10), 1);
    }

    #[test]
    fn paths_share_per_edge_budget_and_refresh_next_tick() {
        let mut network = ElectricalNetwork::new(24);
        for (a, b, rate) in [(1, 2, 10), (2, 3, 6), (2, 4, 9)] {
            network.add_connection(a, b, rate);
        }
        network.tick();
        assert_eq!(network.insert(1, 3, 9, 9), 6);
        assert_eq!(network.insert(1, 4, 9, 9), 4);
        assert_eq!(network.insert(1, 4, 9, 9), 0);
        network.tick();
        assert_eq!(network.insert(1, 4, 9, 9), 9);
    }

    #[test]
    fn removing_link_invalidates_cached_path_and_depth_bounds() {
        let mut network = ElectricalNetwork::new(2);
        network.add_connection(1, 2, 12);
        network.add_connection(2, 3, 12);
        network.add_connection(3, 4, 12);
        network.tick();
        assert_eq!(network.insert(1, 4, 12, 12), 0); // three hops
        assert_eq!(network.insert(1, 3, 12, 12), 12);
        network.remove_connection(2, 3);
        assert_eq!(network.insert(1, 3, 12, 12), 0);
        network.add_connection(1, 3, 12); // cached miss invalidated
        network.tick();
        assert_eq!(network.insert(1, 3, 12, 12), 12);
    }

    #[test]
    fn distributes_across_consumers_with_source_skipped() {
        let mut network = ElectricalNetwork::new(24);
        network.add_connection(1, 2, 10);
        network.add_connection(1, 3, 10);
        network.tick();
        let mut consumers = [(1, 10), (2, 5), (3, 5)];
        assert_eq!(network.distribute(1, 10, &mut consumers), 10);
        assert_eq!(consumers, [(1, 10), (2, 0), (3, 0)]);
        assert_eq!(network.distribute(1, 10, &mut consumers), 0);
    }
}

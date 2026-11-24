//! Bounded graph replacement and splitting for one rail placement or removal.

use super::{
    graph::{TrackEdge, TrackGraph},
    math::Vec3,
    railway::new_uuid,
    save::{RailwayData, RailwayGraph},
    world_graph::{GraphReadError, RailCell, RailWorld},
};
use moddedmc_api::Pos;
use std::collections::HashSet;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum ReconcileError {
    Unloaded,
    Unsupported,
    Traversal,
    IdExhausted,
}

fn block_touches_edge(pos: Pos, edge: &TrackEdge, dim: &str) -> bool {
    if edge.from.dimension != dim || edge.to.dimension != dim {
        return false;
    }
    // A curve needs the proper closest-point solver and loaded curve BE. Never
    // use a straight-line approximation to silently replace a saved curve.
    if edge.turn.is_some() {
        return false;
    }
    let a = edge.from.world();
    let b = edge.to.world();
    let delta = b.minus(a);
    let length_squared = delta.dot(delta);
    if length_squared <= 1.0e-8 {
        return false;
    }
    for level in [0.0, 0.5] {
        let center = Vec3 {
            x: f64::from(pos.x) + 0.5,
            y: f64::from(pos.y) + level,
            z: f64::from(pos.z) + 0.5,
        };
        let projection = center.minus(a).dot(delta) / length_squared;
        if !(-0.0625..=1.0625).contains(&projection) {
            continue;
        }
        let closest = a.plus(delta.scale(projection.clamp(0.0, 1.0)));
        if closest.distance(center) < 0.74 {
            return true;
        }
    }
    false
}

pub(super) fn graph_touches_change(graph: &TrackGraph, dimension: &str, pos: Pos) -> bool {
    graph
        .connections
        .values()
        .flat_map(|out| out.values())
        .any(|edge| block_touches_edge(pos, edge, dimension))
        || graph.nodes.keys().any(|loc| {
            if loc.dimension != dimension {
                return false;
            }
            let [x, y, z] = [loc.world().x, loc.world().y, loc.world().z];
            (x - (f64::from(pos.x) + 0.5)).abs() <= 0.74
                && (y - f64::from(pos.y)).abs() <= 0.74
                && (z - (f64::from(pos.z) + 0.5)).abs() <= 0.74
        })
}

fn intersects(a: &TrackGraph, b: &TrackGraph) -> bool {
    a.nodes.keys().any(|loc| b.nodes.contains_key(loc))
}

/// One geometry-only reconciliation. Build and validate all replacement
/// components *before* touching the saved snapshot: unknown chunks, portals,
/// malformed saved curves and traversal failures preserve the old graphs.
pub(super) fn reconcile<F>(
    data: &mut RailwayData,
    world: &RailWorld<'_, F>,
    changed: Pos,
) -> Result<(), ReconcileError>
where
    F: Fn(Pos) -> RailCell,
{
    // A state-change event may need both sides of a removed rail. Inspect
    // the changed block and all immediate cardinal/diagonal/sloped neighbors.
    let mut candidate_positions = Vec::new();
    for dy in -1i32..=1 {
        for dz in -1i32..=1 {
            for dx in -1i32..=1 {
                let (Some(x), Some(y), Some(z)) = (
                    changed.x.checked_add(dx),
                    changed.y.checked_add(dy),
                    changed.z.checked_add(dz),
                ) else {
                    return Err(ReconcileError::Unloaded);
                };
                let pos = Pos { x, y, z };
                match world.cell(pos) {
                    RailCell::Rail(_) => candidate_positions.push(pos),
                    RailCell::Other => {}
                    RailCell::Unloaded => return Err(ReconcileError::Unloaded),
                    RailCell::Unsupported => return Err(ReconcileError::Unsupported),
                }
            }
        }
    }

    let mut rebuilt = Vec::<TrackGraph>::new();
    for pos in candidate_positions {
        let graph = match world.graph_at(pos) {
            Ok(graph) => graph,
            Err(GraphReadError::UnknownWorldData) => return Err(ReconcileError::Unloaded),
            Err(GraphReadError::MissingRail) => continue,
            Err(GraphReadError::UnsupportedTurnOrPortal) => {
                return Err(ReconcileError::Unsupported);
            }
            Err(GraphReadError::Propagation(_)) => return Err(ReconcileError::Traversal),
        };
        if !rebuilt.iter().any(|other| intersects(other, &graph)) {
            rebuilt.push(graph);
        }
    }

    let touched_old: HashSet<usize> = data
        .graphs
        .iter()
        .enumerate()
        .filter(|(_, old)| {
            graph_touches_change(&old.graph, world.dimension, changed)
                || rebuilt.iter().any(|new| intersects(&old.graph, new))
        })
        .map(|(index, _)| index)
        .collect();

    let changed_is_rail = matches!(world.cell(changed), RailCell::Rail(_));
    rebuilt.retain(|graph| {
        (changed_is_rail && graph_touches_change(graph, world.dimension, changed))
            || touched_old
                .iter()
                .any(|&index| intersects(&data.graphs[index].graph, graph))
    });

    if rebuilt.is_empty() && touched_old.is_empty() {
        return Ok(());
    }

    let old_ids: HashSet<[i32; 4]> = data.graphs.iter().map(|g| g.uuid).collect();
    let mut used_ids = HashSet::new();
    let mut replacements = Vec::with_capacity(rebuilt.len());
    for graph in rebuilt {
        let reuse = touched_old.iter().copied().find(|index| {
            let old = &data.graphs[*index];
            !used_ids.contains(&old.uuid) && intersects(&old.graph, &graph)
        });
        let (uuid, color) = if let Some(index) = reuse {
            (data.graphs[index].uuid, data.graphs[index].color)
        } else {
            let mut generated = None;
            for _ in 0..16 {
                let id = new_uuid(&graph.nodes.keys().collect::<Vec<_>>(), data)
                    .map_err(|_| ReconcileError::IdExhausted)?;
                if !old_ids.contains(&id) && !used_ids.contains(&id) {
                    generated = Some(id);
                    break;
                }
            }
            (generated.ok_or(ReconcileError::IdExhausted)?, 0x9c9c9c)
        };
        used_ids.insert(uuid);
        replacements.push(RailwayGraph { uuid, color, graph });
    }

    // All failure checks passed: replace affected networks and retain every
    // independent graph (including those in other dimensions) unchanged.
    let mut replacement_index = 0;
    let mut original = std::mem::take(&mut data.graphs);
    original.retain(|_| {
        let should_keep = !touched_old.contains(&replacement_index);
        replacement_index += 1;
        should_keep
    });
    original.extend(replacements);
    data.graphs = original;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::track::TrackShape;
    use super::*;
    use std::collections::HashMap;

    fn p(x: i32, y: i32, z: i32) -> Pos {
        Pos { x, y, z }
    }
    fn reader(data: &HashMap<[i32; 3], TrackShape>, pos: Pos) -> RailCell {
        data.get(&[pos.x, pos.y, pos.z])
            .copied()
            .map_or(RailCell::Other, RailCell::Rail)
    }

    #[test]
    fn splitting_and_rejoining_track_preserves_unrelated_saved_network() {
        let mut cells = HashMap::new();
        for x in 0..=8 {
            cells.insert([x, 64, 0], TrackShape::XOrtho);
        }
        for x in 0..=4 {
            cells.insert([x, 64, 10], TrackShape::XOrtho);
        }
        let world = RailWorld::new("minecraft:overworld", |pos| reader(&cells, pos));
        let first = world.graph_at(p(3, 64, 0)).unwrap();
        let separate = world.graph_at(p(2, 64, 10)).unwrap();
        let mut saved = RailwayData {
            graphs: vec![
                RailwayGraph {
                    uuid: [7; 4],
                    color: 23,
                    graph: first,
                },
                RailwayGraph {
                    uuid: [8; 4],
                    color: 29,
                    graph: separate,
                },
            ],
        };
        assert_eq!(saved.total_nodes(), 4);
        assert_eq!(saved.total_directed_edges(), 4);
        cells.remove(&[4, 64, 0]);
        let world = RailWorld::new("minecraft:overworld", |pos| reader(&cells, pos));
        reconcile(&mut saved, &world, p(4, 64, 0)).unwrap();
        assert_eq!(saved.graphs.len(), 3);
        assert_eq!(saved.total_directed_edges(), 6);
        assert!(
            saved
                .graphs
                .iter()
                .any(|g| g.uuid == [8; 4] && g.color == 29)
        );
        assert!(
            saved
                .graphs
                .iter()
                .any(|g| g.uuid == [7; 4] && g.color == 23)
        );
        let roundtrip = RailwayData::from_nbt(&saved.to_nbt().unwrap()).unwrap();
        assert_eq!(roundtrip.graphs.len(), 3);
        cells.insert([4, 64, 0], TrackShape::XOrtho);
        let world = RailWorld::new("minecraft:overworld", |pos| reader(&cells, pos));
        reconcile(&mut saved, &world, p(4, 64, 0)).unwrap();
        assert_eq!(saved.graphs.len(), 2);
        assert_eq!(saved.total_directed_edges(), 4);
        assert!(
            saved
                .graphs
                .iter()
                .any(|g| g.uuid == [8; 4] && g.color == 29)
        );
    }

    #[test]
    fn unknown_chunk_never_modifies_prior_saved_railway() {
        let mut cells = HashMap::new();
        for x in 0..=6 {
            cells.insert([x, 64, 0], TrackShape::XOrtho);
        }
        let original = RailWorld::new("minecraft:overworld", |pos| reader(&cells, pos))
            .graph_at(p(3, 64, 0))
            .unwrap();
        let mut saved = RailwayData {
            graphs: vec![RailwayGraph {
                uuid: [1; 4],
                color: 45,
                graph: original,
            }],
        };
        let baseline = saved.to_nbt().unwrap();
        let world = RailWorld::new("minecraft:overworld", |pos| {
            if pos == p(5, 64, 0) {
                RailCell::Unloaded
            } else {
                reader(&cells, pos)
            }
        });
        assert_eq!(
            reconcile(&mut saved, &world, p(4, 64, 0)),
            Err(ReconcileError::Unloaded)
        );
        assert_eq!(saved.graphs.len(), 1);
        assert_eq!(saved.to_nbt().unwrap().len(), baseline.len());
        assert_eq!(saved.graphs[0].uuid, [1; 4]);
    }

    #[test]
    fn crossing_does_not_merge_orthogonal_railway_components() {
        let mut cells = HashMap::new();
        for x in 0..=4 {
            cells.insert([x, 64, 0], TrackShape::XOrtho);
        }
        cells.insert([2, 64, 0], TrackShape::CrossOrtho);
        for z in 1..=3 {
            cells.insert([2, 64, z], TrackShape::ZOrtho);
        }
        let world = RailWorld::new("minecraft:overworld", |pos| reader(&cells, pos));
        let mut saved = RailwayData::default();
        reconcile(&mut saved, &world, p(2, 64, 0)).unwrap();
        assert_eq!(saved.graphs.len(), 2);
        assert_eq!(saved.total_directed_edges(), 4);
        assert_eq!(
            saved
                .graphs
                .iter()
                .map(|g| g.graph.connected_components().len())
                .sum::<usize>(),
            2
        );
    }

    #[test]
    fn no_rail_change_keeps_original_snapshot_unchanged() {
        let world = RailWorld::new("minecraft:overworld", |_| RailCell::Other);
        let mut saved = RailwayData::default();
        reconcile(&mut saved, &world, p(50, 64, 50)).unwrap();
        assert!(saved.graphs.is_empty());
    }
}

//! Save-global Create RailwaySavedData and live rail updates.
use super::{
    graph::TrackNodeLocation,
    reconcile::{ReconcileError, graph_touches_change, reconcile},
    save::{RailwayData, RailwayGraph},
    world_graph::{RailCell, RailWorld},
};
use moddedmc_api::{
    Command, InterfaceUnavailable, Pos, RegistrationError, Registry,
    content::{self, Content, ContentEvent, World},
    saved_data::{self, Key, SavedData},
    world::{self, World as DimensionWorld},
};
use std::{
    collections::{HashMap, VecDeque, hash_map::RandomState},
    hash::{BuildHasher, Hash, Hasher},
    sync::OnceLock,
};

static TABLE: OnceLock<SavedData> = OnceLock::new();
static KEY: OnceLock<Key> = OnceLock::new();
static DIMENSIONS: OnceLock<DimensionWorld> = OnceLock::new();

pub(crate) fn require() -> Result<(), InterfaceUnavailable> {
    let saved = saved_data::require()?;
    let dimensions = world::require()?;
    let _ = TABLE.set(saved);
    let _ = DIMENSIONS.set(dimensions);
    Ok(())
}

pub(crate) fn register(registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
    let saved = *TABLE.get().ok_or(RegistrationError::InvalidDefinition)?;
    let key = saved.register(registry, "create_tracks")?;
    KEY.set(key).map_err(|_| RegistrationError::DuplicateId)
}

fn table() -> Result<(SavedData, Key), String> {
    Ok((
        *TABLE.get().ok_or("saved_data table was not registered")?,
        *KEY.get().ok_or("railway data key was not registered")?,
    ))
}

fn read(world: &World<'_>) -> Result<RailwayData, String> {
    let (save, key) = table()?;
    match save.read(world, key) {
        None => Ok(RailwayData::default()),
        Some(bytes) => RailwayData::from_nbt(&bytes)
            .map_err(|err| format!("create_tracks contains unsupported or invalid data: {err}")),
    }
}

pub(crate) fn summary(world: &World<'_>) -> Result<String, String> {
    let data = read(world)?;
    Ok(format!(
        "Create railway: Graphs={} Nodes={} DirectedEdges={}",
        data.graphs.len(),
        data.total_nodes(),
        data.total_directed_edges()
    ))
}

/// Force a rebuild of the component containing the command position.
pub(crate) fn rebuild_at(
    world: &World<'_>,
    command: &Command<'_>,
    content: Content,
    position: Pos,
) -> Result<String, String> {
    let dimension = DIMENSIONS
        .get()
        .and_then(|api| api.command_dimension(command))
        .ok_or("command source has no known dimension")?;
    let tracks = RailWorld::new(dimension, |pos| rail_cell(*world, content, pos));

    let graph = tracks
        .graph_at(position)
        .map_err(|err| format!("railway graph traversal incomplete: {err:?}"))?;
    let mut railway = read(world)?;
    // Preserve graph ID and color when updating a known network. Remove all
    // old networks intersecting the replacement, but never unrelated graphs.
    let mut preserved = None;
    railway.graphs.retain(|prior| {
        let overlap = prior
            .graph
            .nodes
            .keys()
            .any(|location| graph.nodes.contains_key(location));
        if overlap {
            if preserved.is_none() {
                preserved = Some((prior.uuid, prior.color));
            }
            false
        } else {
            true
        }
    });
    let (uuid, color) = match preserved {
        Some(previous) => previous,
        None => (
            new_uuid(&graph.nodes.keys().collect::<Vec<_>>(), &railway)?,
            0x9c9c9c,
        ),
    };
    railway.graphs.push(RailwayGraph { uuid, color, graph });
    let bytes = railway.to_nbt()?;
    let (saved, key) = table()?;
    if !saved.write(world, key, &bytes) {
        return Err("host refused create_tracks saved-data write".into());
    }
    summary(world)
}

fn rail_cell(world: World<'_>, content: Content, pos: Pos) -> RailCell {
    let mut state = [content::UNLOADED_STATE];
    if !world.read(&[pos], &mut state) || state[0] == content::UNLOADED_STATE {
        return RailCell::Unloaded;
    }
    match super::runtime::track_state(content, state[0]) {
        Some((shape, false)) => RailCell::Rail(shape),
        Some((_, true)) => RailCell::Unsupported,
        _ => RailCell::Other,
    }
}

#[derive(Default)]
pub(crate) struct RailUpdates {
    pending: HashMap<Pos, bool>,
    order: VecDeque<Pos>,
    retry_at: u64,
}

impl RailUpdates {
    fn enqueue(&mut self, pos: Pos, loaded: bool) {
        self.pending
            .entry(pos)
            .and_modify(|previous| *previous &= loaded)
            .or_insert_with(|| {
                self.order.push_back(pos);
                loaded
            });
    }

    pub(crate) fn tick(
        &mut self,
        world: World<'_>,
        content: Content,
        time: u64,
        events: &[ContentEvent],
    ) {
        const MAX_UPDATES_PER_TICK: usize = 8;
        const RETRY_TICKS: u64 = 20;
        let mut changed = false;
        for event in events {
            match event.reason {
                content::LOADED if super::runtime::track_state(content, event.state).is_some() => {
                    self.enqueue(event.position, true);
                    changed = true;
                }
                content::STATE_CHANGED | content::MOVED_STATE_CHANGED
                    if super::runtime::track_state(content, event.previous)
                        != super::runtime::track_state(content, event.state) =>
                {
                    self.enqueue(event.position, false);
                    changed = true;
                }
                _ => {}
            }
        }
        if self.order.is_empty() || (!changed && time < self.retry_at) {
            return;
        }
        let Some(dimension) = world.dimension_id() else {
            return;
        };
        let Ok(mut data) = read(&world) else {
            self.retry_at = time.saturating_add(RETRY_TICKS);
            return;
        };
        let rails = RailWorld::new(dimension, |pos| rail_cell(world, content, pos));
        let mut retry = Vec::new();
        let mut applied = Vec::new();
        for _ in 0..MAX_UPDATES_PER_TICK.min(self.order.len()) {
            let Some(pos) = self.order.pop_front() else {
                break;
            };
            let Some(loaded) = self.pending.remove(&pos) else {
                continue;
            };
            if loaded
                && data
                    .graphs
                    .iter()
                    .any(|graph| graph_touches_change(&graph.graph, dimension, pos))
            {
                continue;
            }
            match reconcile(&mut data, &rails, pos) {
                Ok(()) => applied.push((pos, loaded)),
                Err(ReconcileError::Unloaded) => retry.push((pos, loaded)),
                Err(_) => {}
            }
        }
        self.retry_at = if self.order.is_empty() || (applied.is_empty() && !retry.is_empty()) {
            time.saturating_add(RETRY_TICKS)
        } else {
            time
        };
        if !applied.is_empty() && !write(&world, &data) {
            retry.extend(applied);
        }
        for (pos, loaded) in retry {
            self.enqueue(pos, loaded);
        }
    }
}

fn write(world: &World<'_>, data: &RailwayData) -> bool {
    let (Ok((saved, key)), Ok(bytes)) = (table(), data.to_nbt()) else {
        return false;
    };
    saved.write(world, key, &bytes)
}

pub(super) fn new_uuid(
    nodes: &[&TrackNodeLocation],
    old: &RailwayData,
) -> Result<[i32; 4], String> {
    for _ in 0..16 {
        let mut first = RandomState::new().build_hasher();
        let mut second = RandomState::new().build_hasher();
        for location in nodes {
            location.hash(&mut first);
            location.hash(&mut second);
        }
        let mut octets =
            ((u128::from(first.finish()) << 64) | u128::from(second.finish())).to_be_bytes();
        octets[6] = (octets[6] & 0x0f) | 0x40; // UUID v4 form
        octets[8] = (octets[8] & 0x3f) | 0x80; // RFC variant
        let id = std::array::from_fn(|index| {
            let start = index * 4;
            i32::from_be_bytes(octets[start..start + 4].try_into().unwrap())
        });
        if old.graphs.iter().all(|network| network.uuid != id) {
            return Ok(id);
        }
    }
    // No safe unique identifier after repeated entropy collision.
    // The caller must reject this, never overwrite a different network.
    Err("unable to allocate a unique graph UUID".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn new_graph_uuid_is_version_four_form_and_unique_in_a_snapshot() {
        let a = TrackNodeLocation::from_world(
            "minecraft:overworld",
            super::super::math::Vec3 {
                x: 2.5,
                y: 64.0,
                z: -3.5,
            },
            0,
        );
        let id = new_uuid(&[&a], &RailwayData::default()).unwrap();
        let octets: Vec<u8> = id.into_iter().flat_map(i32::to_be_bytes).collect();
        assert_eq!(octets[6] >> 4, 4);
        assert_eq!(octets[8] >> 6, 2);
        let old = RailwayData {
            graphs: vec![RailwayGraph {
                uuid: id,
                color: 0,
                graph: Default::default(),
            }],
        };
        assert_ne!(new_uuid(&[&a], &old).unwrap(), id);
    }
}

//! An authoritative global registry snapshot for Compass selection screens.
//! A worldgen lookup from one dimension cannot stand in for this snapshot:
//! registered entries may never generate there, or may have no placements.
//! The host must supply the complete registry and generating-dimension data.

use std::collections::HashSet;

/// Whether a complete registry snapshot was available and internally coherent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapshotError {
    Unavailable,
    InvalidIdentifier,
    ConflictingMetadata,
    TooLarge,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionError {
    NotRegistered,
    Hidden,
    WrongDimension,
    PlacementUnavailable,
    NoPlacements,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlacementGate {
    /// Biome search uses noise biome sampling, not structure placement sets.
    Biome,
    /// Structure search must have a successful current-world placement lookup.
    /// `None` is an unavailable lookup, `Some(0)` is a known unplaced entry.
    Structure(Option<usize>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub id: String,
    /// Dimensions in which the registered biome or structure may generate.
    /// An empty set is authoritative and distinct from missing metadata.
    /// For structures, this does not promise a valid placement exists.
    pub generating_dimensions: Vec<String>,
}

impl Entry {
    pub fn may_generate_in(&self, dimension: &str) -> bool {
        self.generating_dimensions.iter().any(|id| id == dimension)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GlobalCatalog {
    entries: Vec<Entry>,
}

fn resource_id(id: &str) -> bool {
    let Some((namespace, path)) = id.split_once(':') else {
        return false;
    };
    let common =
        |c: u8| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'_' | b'-' | b'.');
    !namespace.is_empty()
        && !path.is_empty()
        && id.len() <= 256
        && namespace.bytes().all(common)
        && path.bytes().all(|c| common(c) || c == b'/')
}

impl GlobalCatalog {
    /// Only accept a successful, complete host snapshot. `Some(vec![])` is a
    /// valid empty registry; `None` means the interface or read failed. Reject
    /// contradictory entries rather than picking whichever arrived first.
    pub fn from_complete(snapshot: Option<Vec<Entry>>) -> Result<Self, SnapshotError> {
        let mut entries = snapshot.ok_or(SnapshotError::Unavailable)?;
        if entries.len() > 65_536 {
            return Err(SnapshotError::TooLarge);
        }
        for entry in &mut entries {
            if !resource_id(&entry.id)
                || entry.generating_dimensions.len() > 256
                || entry
                    .generating_dimensions
                    .iter()
                    .any(|id| !resource_id(id))
            {
                return Err(SnapshotError::InvalidIdentifier);
            }
            entry.generating_dimensions.sort_unstable();
            entry.generating_dimensions.dedup();
        }
        entries.sort_unstable_by(|a, b| a.id.cmp(&b.id));
        let mut canonical: Vec<Entry> = Vec::with_capacity(entries.len());
        for entry in entries {
            if let Some(last) = canonical.last()
                && last.id == entry.id
            {
                if last.generating_dimensions != entry.generating_dimensions {
                    return Err(SnapshotError::ConflictingMetadata);
                }
                continue;
            }
            canonical.push(entry);
        }
        Ok(Self { entries: canonical })
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn get(&self, id: &str) -> Option<&Entry> {
        self.entries
            .binary_search_by(|entry| entry.id.as_str().cmp(id))
            .ok()
            .map(|index| &self.entries[index])
    }

    /// UI filtering must not erase the metadata for an unlocatable entry or
    /// quietly convert this global registry into a dimension-local list.
    pub fn visible<'a>(
        &'a self,
        hidden: &HashSet<String>,
        mut blocked: impl FnMut(&str) -> bool,
    ) -> Vec<&'a Entry> {
        self.entries
            .iter()
            .filter(|entry| !hidden.contains(&entry.id) && !blocked(&entry.id))
            .collect()
    }

    /// Authorize a requested search against the current complete snapshot,
    /// not the list formerly shown by a client. This is an eligibility check,
    /// and placement readiness before XP debit or state changes. A registered
    /// structure that has eligible biomes but no structure set is not a valid
    /// executable search target in the current world.
    pub fn authorize_search(
        &self,
        id: &str,
        dimension: &str,
        hidden: &HashSet<String>,
        blocked: impl Fn(&str) -> bool,
        placement: PlacementGate,
    ) -> Result<(), SelectionError> {
        let entry = self.get(id).ok_or(SelectionError::NotRegistered)?;
        if hidden.contains(id) || blocked(id) {
            return Err(SelectionError::Hidden);
        }
        if !entry.may_generate_in(dimension) {
            return Err(SelectionError::WrongDimension);
        }
        match placement {
            PlacementGate::Biome | PlacementGate::Structure(Some(1..)) => {}
            PlacementGate::Structure(Some(0)) => return Err(SelectionError::NoPlacements),
            PlacementGate::Structure(None) => return Err(SelectionError::PlacementUnavailable),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, dimensions: &[&str]) -> Entry {
        Entry {
            id: id.into(),
            generating_dimensions: dimensions.iter().map(|s| (*s).into()).collect(),
        }
    }

    #[test]
    fn complete_empty_is_distinct_from_missing_or_partial_snapshot() {
        assert_eq!(
            GlobalCatalog::from_complete(None),
            Err(SnapshotError::Unavailable)
        );
        assert!(
            GlobalCatalog::from_complete(Some(vec![]))
                .unwrap()
                .entries()
                .is_empty()
        );
        assert_eq!(
            GlobalCatalog::from_complete(Some(vec![entry("minecraft:bad ID", &[])])),
            Err(SnapshotError::InvalidIdentifier)
        );
        assert_eq!(
            GlobalCatalog::from_complete(Some(vec![
                entry("test:a", &["minecraft:overworld"]),
                entry("test:a", &["minecraft:the_nether"]),
            ])),
            Err(SnapshotError::ConflictingMetadata)
        );
    }

    #[test]
    fn global_ids_remain_visible_but_dimension_generation_is_explicit() {
        let catalog = GlobalCatalog::from_complete(Some(vec![
            entry("minecraft:basalt_deltas", &["minecraft:the_nether"]),
            entry("test:never_eligible", &[]),
            entry("minecraft:birch_forest", &["minecraft:overworld"]),
            entry(
                "test:unplaced",
                &["minecraft:overworld", "minecraft:overworld"],
            ),
            entry("minecraft:birch_forest", &["minecraft:overworld"]),
        ]))
        .unwrap();
        let ids: Vec<_> = catalog.entries().iter().map(|e| e.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "minecraft:basalt_deltas",
                "minecraft:birch_forest",
                "test:never_eligible",
                "test:unplaced"
            ]
        );
        let overworld = "minecraft:overworld";
        let nether = "minecraft:the_nether";
        assert!(
            !catalog
                .get("minecraft:basalt_deltas")
                .unwrap()
                .may_generate_in(overworld)
        );
        assert!(
            catalog
                .get("minecraft:basalt_deltas")
                .unwrap()
                .may_generate_in(nether)
        );
        assert!(
            catalog
                .get("minecraft:birch_forest")
                .unwrap()
                .may_generate_in(overworld)
        );
        assert!(
            !catalog
                .get("test:never_eligible")
                .unwrap()
                .may_generate_in(nether)
        );
        // A structure with eligible biomes but no placement remains in the
        // registry; the later structure-placement lookup must reject it.
        assert!(
            catalog
                .get("test:unplaced")
                .unwrap()
                .may_generate_in(overworld)
        );
        assert!(
            !catalog
                .get("test:unplaced")
                .unwrap()
                .may_generate_in(nether)
        );
        let hidden = HashSet::from(["test:never_eligible".into()]);
        let visible = catalog.visible(&hidden, |id| id == "test:unplaced");
        let visible: Vec<_> = visible.iter().map(|entry| entry.id.as_str()).collect();
        assert_eq!(
            visible,
            ["minecraft:basalt_deltas", "minecraft:birch_forest"]
        );
        assert_eq!(
            catalog.authorize_search(
                "minecraft:basalt_deltas",
                overworld,
                &hidden,
                |_| false,
                PlacementGate::Biome,
            ),
            Err(SelectionError::WrongDimension)
        );
        assert_eq!(
            catalog.authorize_search(
                "test:never_eligible",
                overworld,
                &hidden,
                |_| false,
                PlacementGate::Structure(Some(0)),
            ),
            Err(SelectionError::Hidden)
        );
        assert_eq!(
            catalog.authorize_search(
                "test:never_eligible",
                overworld,
                &HashSet::new(),
                |_| false,
                PlacementGate::Structure(Some(0)),
            ),
            Err(SelectionError::WrongDimension)
        );
        assert_eq!(
            catalog.authorize_search(
                "test:unplaced",
                overworld,
                &hidden,
                |_| false,
                PlacementGate::Structure(Some(0)),
            ),
            Err(SelectionError::NoPlacements)
        );
        assert_eq!(
            catalog.authorize_search(
                "test:unplaced",
                overworld,
                &hidden,
                |_| true,
                PlacementGate::Structure(Some(1)),
            ),
            Err(SelectionError::Hidden)
        );
        assert_eq!(
            catalog.authorize_search(
                "test:unknown",
                overworld,
                &hidden,
                |_| false,
                PlacementGate::Biome,
            ),
            Err(SelectionError::NotRegistered)
        );
        assert_eq!(
            catalog.authorize_search(
                "test:unplaced",
                overworld,
                &hidden,
                |_| false,
                PlacementGate::Structure(None),
            ),
            Err(SelectionError::PlacementUnavailable)
        );
        assert_eq!(
            catalog.authorize_search(
                "test:unplaced",
                overworld,
                &hidden,
                |_| false,
                PlacementGate::Structure(Some(2)),
            ),
            Ok(())
        );
    }
}

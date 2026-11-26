//! Save-global, fixed-prefix shards. Each menu transaction writes one key.
use crate::state::{Archive, valid_id};
use serde::{Deserialize, Serialize};

pub const COUNT: usize = 64;
const SHIFT: u32 = 56;
pub const MAX_ID: i64 = (1_i64 << SHIFT) - 1;
const VERSION: i32 = 2;
const MIGRATED: &[u8] = b"lootr:inventories/v2/migrated";
const MANIFEST_PREFIX: &[u8] = b"lootr:inventories/v2/manifest\0";

/// A durable directory distinguishes untouched shards from lost initialized
/// shards. Writes are ordered shard -> directory -> legacy marker.
#[derive(Clone, Copy, Default)]
struct Manifest {
    required: u64,
    initialized: u64,
    legacy_high_water: i64,
    compacted: bool,
}

impl Manifest {
    fn decode(bytes: &[u8]) -> Option<Self> {
        let payload = bytes.strip_prefix(MANIFEST_PREFIX)?;
        let (required, payload) = payload.split_at_checked(8)?;
        let required = u64::from_be_bytes(required.try_into().ok()?);
        let (initialized, payload) = payload.split_at_checked(8)?;
        let initialized = u64::from_be_bytes(initialized.try_into().ok()?);
        let (high_water, compacted) = payload.split_at_checked(8)?;
        let legacy_high_water = i64::from_be_bytes(high_water.try_into().ok()?);
        if !(0..=MAX_ID).contains(&legacy_high_water) {
            return None;
        }
        let compacted = match compacted {
            [0] => false,
            [1] if initialized & required == required => true,
            _ => return None,
        };
        Some(Self {
            required,
            initialized,
            legacy_high_water,
            compacted,
        })
    }

    fn encode(self) -> Vec<u8> {
        let mut bytes = MANIFEST_PREFIX.to_vec();
        bytes.extend(self.required.to_be_bytes());
        bytes.extend(self.initialized.to_be_bytes());
        bytes.extend(self.legacy_high_water.to_be_bytes());
        bytes.push(u8::from(self.compacted));
        bytes
    }

    fn contains(self, index: usize) -> bool {
        self.initialized & (1_u64 << index) != 0
    }

    fn requires_legacy(self, index: usize) -> bool {
        self.required & (1_u64 << index) != 0
    }

    fn ready_to_compact(self) -> bool {
        self.initialized & self.required == self.required
    }

    fn from_legacy(legacy: &Archive) -> Option<Self> {
        let mut required = 0;
        for id in legacy.container_ids() {
            required |= 1_u64 << shard_for(id)?;
        }
        Some(Self {
            required,
            legacy_high_water: legacy.high_water(),
            ..Self::default()
        })
    }
}

pub fn shard_for(container: &str) -> Option<usize> {
    if !valid_id(container) {
        return None;
    }
    Some(usize::from(
        u8::from_str_radix(&container[..2], 16).ok()? >> 2,
    ))
}

pub fn menu_id(shard: usize, local: i64) -> Option<u64> {
    if shard >= COUNT || !(1..=MAX_ID).contains(&local) {
        return None;
    }
    Some(((shard as u64) << SHIFT) | local as u64)
}

pub fn split_menu(id: u64) -> Option<(usize, i64)> {
    let shard = (id >> SHIFT) as usize;
    let local = (id & MAX_ID as u64) as i64;
    menu_id(shard, local).map(|_| (shard, local))
}

#[derive(Deserialize, Serialize)]
pub struct Shard {
    version: i32,
    index: i32,
    pub archive: Archive,
}

impl Shard {
    fn new(index: usize, archive: Archive) -> Self {
        Self {
            version: VERSION,
            index: index as i32,
            archive,
        }
    }

    fn valid(&self, index: usize) -> bool {
        index < COUNT
            && self.version == VERSION
            && self.index == index as i32
            && self.archive.validate()
            && self.archive.fits_id_space(MAX_ID)
            && self
                .archive
                .all_containers_match(|id| shard_for(id) == Some(index))
    }

    pub fn decode(index: usize, bytes: &[u8]) -> Option<Self> {
        let shard: Self = fastnbt::from_bytes(bytes).ok()?;
        shard.valid(index).then_some(shard)
    }

    pub fn encode(&self) -> Option<Vec<u8>> {
        fastnbt::to_bytes(self).ok()
    }
}

/// The storage contract deliberately uses only the existing saved-data ABI.
/// Runtime callers serialize the migration and all transactions under LOCK.
pub trait Store {
    fn legacy(&self) -> Option<Vec<u8>>;
    fn manifest(&self) -> Option<Vec<u8>>;
    fn write_manifest(&mut self, bytes: &[u8]) -> bool;
    fn read(&self, index: usize) -> Option<Vec<u8>>;
    fn write(&mut self, index: usize, bytes: &[u8]) -> bool;
    fn mark_migrated(&mut self, bytes: &[u8]) -> bool;
}

/// Existing initialized shards are authoritative, even when empty. A missing
/// shard is initialized from a valid legacy archive only before compaction.
/// Persist initialization before returning anything that could roll loot.
pub fn load_or_init(store: &mut impl Store, index: usize) -> Option<Shard> {
    if index >= COUNT {
        return None;
    }
    // The first directory write records which shards actually contain legacy
    // claims. Empty/unvisited shards need not keep a duplicate legacy blob alive.
    let mut initial_legacy = None;
    let mut manifest = match store.manifest() {
        Some(bytes) => Manifest::decode(&bytes)?,
        None => {
            // A shard cannot be written before the initial manifest. A missing
            // manifest beside any shard means recovery evidence is incomplete;
            // do not adopt it or initialize a new empty archive.
            if (0..COUNT).any(|i| store.read(i).is_some()) {
                return None;
            }
            let legacy = match store.legacy() {
                Some(bytes) => decode_legacy(&bytes)?,
                None => Archive::default(),
            };
            let manifest = Manifest::from_legacy(&legacy)?;
            if !store.write_manifest(&manifest.encode()) {
                return None;
            }
            initial_legacy = Some(legacy);
            manifest
        }
    };
    let expected_initial = |store: &mut dyn Store| -> Option<Shard> {
        let legacy = if manifest.requires_legacy(index) {
            let legacy = match initial_legacy.as_ref() {
                Some(legacy) => legacy.clone(),
                None => decode_legacy(&store.legacy()?)?,
            };
            let expected = Manifest::from_legacy(&legacy)?;
            if expected.required != manifest.required
                || expected.legacy_high_water != manifest.legacy_high_water
            {
                return None;
            }
            legacy.partition(|id| shard_for(id) == Some(index))
        } else {
            Archive::empty_at(manifest.legacy_high_water)
        };
        Some(Shard::new(index, legacy))
    };
    let shard = match store.read(index) {
        Some(bytes) => {
            let shard = Shard::decode(index, &bytes)?;
            // A shard written before a failed manifest update was never
            // exposed to a menu. Its normalized payload must match the
            // exact initial partition before adoption.
            if !manifest.contains(index) && shard.encode()? != expected_initial(store)?.encode()? {
                return None;
            }
            shard
        }
        None if manifest.contains(index) => return None,
        None => {
            let shard = expected_initial(store)?;
            if !store.write(index, &shard.encode()?) {
                return None;
            }
            shard
        }
    };
    // A failed directory write may leave an initialized but unused shard.
    // Retry the directory write before exposing any claim or new loot.
    if !manifest.contains(index) {
        manifest.initialized |= 1_u64 << index;
        if !store.write_manifest(&manifest.encode()) {
            return None;
        }
    }
    // Compact as soon as all legacy-bearing shards are initialized. Empty
    // shards can still initialize after the legacy payload has been retired.
    if manifest.ready_to_compact()
        && !manifest.compacted
        && (0..COUNT).filter(|&i| manifest.contains(i)).all(|i| {
            store
                .read(i)
                .and_then(|bytes| Shard::decode(i, &bytes))
                .is_some()
        })
        && store.mark_migrated(MIGRATED)
    {
        // A missing unrelated shard prevents retirement of the legacy backup,
        // not use of this healthy shard. Never regenerate an initialized shard.
        manifest.compacted = true;
        let _ = store.write_manifest(&manifest.encode());
    }
    Some(shard)
}

fn decode_legacy(bytes: &[u8]) -> Option<Archive> {
    if bytes == MIGRATED {
        return None;
    }
    let archive: Archive = fastnbt::from_bytes(bytes).ok()?;
    (archive.validate() && archive.fits_id_space(MAX_ID)).then_some(archive)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[derive(Default)]
    struct Memory {
        legacy: Option<Vec<u8>>,
        manifest: Option<Vec<u8>>,
        shards: HashMap<usize, Vec<u8>>,
        reject_write: bool,
        reject_manifest: bool,
        reject_marker: bool,
    }

    impl Store for Memory {
        fn legacy(&self) -> Option<Vec<u8>> {
            self.legacy.clone()
        }
        fn manifest(&self) -> Option<Vec<u8>> {
            self.manifest.clone()
        }
        fn write_manifest(&mut self, bytes: &[u8]) -> bool {
            if self.reject_manifest {
                return false;
            }
            self.manifest = Some(bytes.to_vec());
            true
        }
        fn read(&self, index: usize) -> Option<Vec<u8>> {
            self.shards.get(&index).cloned()
        }
        fn write(&mut self, index: usize, bytes: &[u8]) -> bool {
            if self.reject_write {
                return false;
            }
            self.shards.insert(index, bytes.to_vec());
            true
        }
        fn mark_migrated(&mut self, bytes: &[u8]) -> bool {
            if self.reject_marker {
                return false;
            }
            self.legacy = Some(bytes.to_vec());
            true
        }
    }

    const A: &str = "00000000000000000000000000000001";
    const B: &str = "fc000000000000000000000000000001";

    fn fixture() -> (Memory, i64, i64, i64) {
        let mut legacy = Archive::default();
        let a = legacy
            .open(A, [1; 16], 27, || Some(vec![None; 27]))
            .unwrap();
        let b = legacy
            .open(A, [2; 16], 27, || {
                let mut slots = vec![None; 27];
                slots[0] = Some(vec![1, 2, 3]);
                Some(slots)
            })
            .unwrap();
        let c = legacy
            .open(B, [1; 16], 27, || Some(vec![None; 27]))
            .unwrap();
        legacy.get_mut(a, [1; 16]).unwrap().site = Some(crate::state::Site {
            dimension: "minecraft:overworld".into(),
            position: [1, 2, 3],
        });
        // Serialize the actual v1 schema, not the current struct with its
        // additional binding field, to test serde backward compatibility.
        #[derive(Serialize)]
        struct Legacy<'a> {
            next_id: i64,
            records: &'a [crate::state::Record],
        }
        (
            Memory {
                legacy: Some(
                    fastnbt::to_bytes(&Legacy {
                        next_id: c,
                        records: &legacy.records,
                    })
                    .unwrap(),
                ),
                ..Memory::default()
            },
            a,
            b,
            c,
        )
    }

    #[test]
    fn routing_and_id_boundaries() {
        for i in 0..COUNT {
            for suffix in 0..4 {
                let chest = format!("{:02x}{:030x}", i * 4 + suffix, 1);
                assert_eq!(shard_for(&chest), Some(i));
            }
            for local in [1, MAX_ID] {
                assert_eq!(split_menu(menu_id(i, local).unwrap()), Some((i, local)));
            }
        }
        assert_eq!(menu_id(COUNT, 1), None);
        assert_eq!(menu_id(0, 0), None);
        assert_eq!(menu_id(0, MAX_ID + 1), None);
        assert_eq!(split_menu(u64::MAX), None);
        assert_eq!(shard_for("invalid"), None);
    }

    #[test]
    fn lazy_migration_preserves_claims_and_high_water_mark() {
        let (mut store, a, b, c) = fixture();
        let mut first = load_or_init(&mut store, 0).unwrap();
        assert_eq!(
            first
                .archive
                .open(A, [1; 16], 27, || panic!("exhausted claim rerolled")),
            Some(a)
        );
        assert!(first.archive.get(a, [2; 16]).is_none());
        assert_eq!(
            first.archive.get(b, [2; 16]).unwrap().slots()[0],
            Some(vec![1, 2, 3])
        );
        assert!(first.archive.get(c, [1; 16]).is_none());
        let new = first
            .archive
            .open_bounded(A, [3; 16], 27, MAX_ID, || Some(vec![None; 27]))
            .unwrap();
        assert!(new > c);
        assert!(store.write(0, &first.encode().unwrap()));
        // Re-open from the persisted shard, not the old archive.
        let mut reopened = load_or_init(&mut store, 0).unwrap();
        assert_eq!(reopened.archive.refresh(A), 3);
        assert!(store.write(0, &reopened.encode().unwrap()));
        assert!(
            load_or_init(&mut store, 0)
                .unwrap()
                .archive
                .records
                .is_empty()
        );
        let mut empty = load_or_init(&mut store, 0).unwrap();
        assert!(!empty.archive.bind_site(
            A,
            &crate::state::Site {
                dimension: "minecraft:overworld".into(),
                position: [1, 2, 4],
            }
        ));
        assert!(
            load_or_init(&mut store, 63)
                .unwrap()
                .archive
                .get(c, [1; 16])
                .is_some()
        );
    }

    #[test]
    fn corrupt_or_failed_initialization_is_closed() {
        let (mut store, _, _, _) = fixture();
        store.reject_write = true;
        assert!(load_or_init(&mut store, 0).is_none());
        assert!(store.shards.is_empty());
        store.reject_write = false;
        store.legacy = Some(vec![1, 2, 3]);
        assert!(load_or_init(&mut store, 0).is_none());
        assert!(store.shards.is_empty());
        store.legacy = None;
        store.manifest = None;
        let first = load_or_init(&mut store, 0).unwrap().encode().unwrap();
        assert!(store.write(1, &first));
        assert!(load_or_init(&mut store, 1).is_none());
        assert!(store.write(0, &[1, 2, 3]));
        assert!(load_or_init(&mut store, 0).is_none());
    }

    #[test]
    fn completion_marker_prevents_legacy_fallback() {
        let (mut store, _, _, _) = fixture();
        store.reject_marker = true;
        assert!(load_or_init(&mut store, 0).is_some());
        assert!(load_or_init(&mut store, COUNT - 1).is_some());
        assert_ne!(store.legacy.as_deref(), Some(MIGRATED));
        // Retry compaction without re-initializing an existing shard.
        store.reject_marker = false;
        assert!(load_or_init(&mut store, COUNT - 1).is_some());
        assert_eq!(store.legacy.as_deref(), Some(MIGRATED));
        assert!(
            load_or_init(&mut store, 17)
                .unwrap()
                .archive
                .records
                .is_empty()
        );
        store.shards.remove(&0);
        assert!(load_or_init(&mut store, 0).is_none());
    }

    #[test]
    fn directory_failure_retries_and_lost_initialized_shard_fails_closed() {
        let (mut store, a, _, _) = fixture();
        assert!(load_or_init(&mut store, 1).is_some());
        store.reject_manifest = true;
        assert!(load_or_init(&mut store, 0).is_none());
        assert!(store.shards.contains_key(&0));
        store.reject_manifest = false;
        let mut first = load_or_init(&mut store, 0).unwrap();
        assert_eq!(
            first.archive.open(A, [1; 16], 27, || panic!("reroll")),
            Some(a)
        );
        assert_eq!(first.archive.refresh(A), 2);
        assert!(store.write(0, &first.encode().unwrap()));
        store.shards.remove(&0);
        assert!(load_or_init(&mut store, 0).is_none());
        assert!(load_or_init(&mut store, 63).is_some());
        assert_ne!(store.legacy.as_deref(), Some(MIGRATED));
        store.manifest = Some(vec![1, 2, 3]);
        assert!(load_or_init(&mut store, 63).is_none());
    }

    #[test]
    fn recovery_rejects_orphan_or_modified_unpublished_shard() {
        let (mut store, _, _, _) = fixture();
        assert!(load_or_init(&mut store, 1).is_some());
        store.reject_manifest = true;
        assert!(load_or_init(&mut store, 0).is_none());
        let mut orphan = Shard::decode(0, store.shards.get(&0).unwrap()).unwrap();
        orphan.archive.refresh(A);
        assert!(store.write(0, &orphan.encode().unwrap()));
        store.reject_manifest = false;
        assert!(load_or_init(&mut store, 0).is_none());
        store.manifest = None;
        assert!(load_or_init(&mut store, 2).is_none());
        assert!(!store.shards.contains_key(&2));
    }

    #[test]
    fn per_shard_payload_scales_with_local_records_not_world_records() {
        let mut archive = Archive::default();
        for prefix in 0..COUNT {
            for owner in 0..8u8 {
                let chest = format!("{:02x}{:030x}", prefix * 4, 1);
                archive
                    .open(&chest, [owner; 16], 27, || {
                        let mut slots = vec![None; 27];
                        slots[0] = Some(vec![42; 256]);
                        Some(slots)
                    })
                    .unwrap();
            }
        }
        let all_bytes = fastnbt::to_bytes(&archive).unwrap().len();
        let shard = Shard::new(17, archive.partition(|id| shard_for(id) == Some(17)));
        assert_eq!(shard.archive.records.len(), 8);
        let bytes = shard.encode().unwrap();
        assert!(bytes.len() * 32 < all_bytes);
        assert!(Shard::decode(17, &bytes).is_some());
        assert!(Shard::decode(18, &bytes).is_none());
    }
}

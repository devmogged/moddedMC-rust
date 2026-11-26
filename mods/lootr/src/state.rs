//! Per-container, per-player inventories; empty inventories remain claimed.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct Site {
    pub dimension: String,
    pub position: [i32; 3],
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Archive {
    next_id: i64,
    pub records: Vec<Record>,
    /// Identity survives refresh or clearing the last owner's inventory.
    /// Older archives derive bindings from their per-record site fields.
    #[serde(default)]
    bindings: BTreeMap<String, Site>,
    #[serde(default)]
    schedules: BTreeMap<String, crate::lifecycle::Schedule>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Record {
    pub id: i64,
    pub container: String,
    #[serde(default)]
    pub site: Option<Site>,
    owner: String,
    size: i32,
    items: Vec<Slot>,
    // Older records were claimed via an open. Keep their visual marker.
    #[serde(default = "opened_default")]
    visual_opened: bool,
}

fn opened_default() -> bool {
    true
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Slot {
    index: i32,
    stack: fastnbt::ByteArray,
}

fn owner_key(uuid: [u8; 16]) -> String {
    uuid.iter().map(|b| format!("{b:02x}")).collect()
}

impl Archive {
    /// Retain the allocation high-water mark when moving an existing claim.
    pub(crate) fn partition(&self, accepts: impl Fn(&str) -> bool) -> Self {
        let mut part = Self {
            next_id: self.next_id,
            records: self
                .records
                .iter()
                .filter(|record| accepts(&record.container))
                .cloned()
                .collect(),
            bindings: self
                .bindings
                .iter()
                .filter(|(id, _)| accepts(id))
                .map(|(id, site)| (id.clone(), site.clone()))
                .collect(),
            schedules: self
                .schedules
                .iter()
                .filter(|(id, _)| accepts(id))
                .map(|(id, schedule)| (id.clone(), schedule.clone()))
                .collect(),
        };
        // Preserve site ownership in legacy records independently of claims.
        for record in &part.records {
            if let Some(site) = &record.site {
                part.bindings
                    .entry(record.container.clone())
                    .or_insert_with(|| site.clone());
            }
        }
        part
    }

    pub(crate) fn fits_id_space(&self, max: i64) -> bool {
        self.next_id <= max
    }

    pub(crate) fn high_water(&self) -> i64 {
        self.next_id
    }

    pub(crate) fn empty_at(next_id: i64) -> Self {
        Self {
            next_id,
            ..Self::default()
        }
    }

    pub(crate) fn container_ids(&self) -> impl Iterator<Item = &str> {
        self.records
            .iter()
            .map(|r| r.container.as_str())
            .chain(self.bindings.keys().map(String::as_str))
            .chain(self.schedules.keys().map(String::as_str))
    }

    pub(crate) fn all_containers_match(&self, accepts: impl Fn(&str) -> bool) -> bool {
        self.container_ids().all(accepts)
    }

    /// Reject malformed or ambiguous saved records before exposing any slots.
    pub fn validate(&self) -> bool {
        let mut ids = HashSet::new();
        let mut owners = HashSet::new();
        let mut sites = HashMap::new();
        self.next_id >= 0
            && self
                .schedules
                .iter()
                .all(|(id, schedule)| self.bindings.contains_key(id) && schedule.valid())
            && self.bindings.iter().all(|(id, site)| {
                valid_id(id) && !site.dimension.is_empty() && sites.insert(id, site).is_none()
            })
            && self.records.iter().all(|r| {
                let mut slots = HashSet::new();
                r.id > 0
                    && r.id <= self.next_id
                    && ids.insert(r.id)
                    && valid_id(&r.container)
                    && valid_id(&r.owner)
                    && owners.insert((&r.container, &r.owner))
                    && r.site.as_ref().is_none_or(|site| {
                        !site.dimension.is_empty()
                            && *sites.entry(&r.container).or_insert(site) == site
                    })
                    && (9..=54).contains(&r.size)
                    && r.size % 9 == 0
                    && r.items.iter().all(|item| {
                        (0..r.size).contains(&item.index)
                            && slots.insert(item.index)
                            && !item.stack.is_empty()
                            && item.stack.len() <= moddedmc_api::abi::MAX_STACK_BYTES
                    })
            })
    }

    pub fn site_available(&self, container: &str, site: &Site) -> bool {
        self.bindings
            .get(container)
            .is_none_or(|existing| existing == site)
            && self
                .records
                .iter()
                .filter(|r| r.container == container)
                .all(|r| r.site.as_ref().is_none_or(|existing| existing == site))
    }

    pub(crate) fn bind_site(&mut self, container: &str, site: &Site) -> bool {
        if !valid_id(container)
            || site.dimension.is_empty()
            || !self.site_available(container, site)
        {
            return false;
        }
        self.bindings
            .entry(container.into())
            .or_insert_with(|| site.clone());
        true
    }

    pub fn open(
        &mut self,
        container: &str,
        uuid: [u8; 16],
        size: usize,
        generate: impl FnOnce() -> Option<Vec<Option<Vec<u8>>>>,
    ) -> Option<i64> {
        self.open_bounded(container, uuid, size, i64::MAX, generate)
    }

    pub(crate) fn open_bounded(
        &mut self,
        container: &str,
        uuid: [u8; 16],
        size: usize,
        max_id: i64,
        generate: impl FnOnce() -> Option<Vec<Option<Vec<u8>>>>,
    ) -> Option<i64> {
        if !valid_id(container) {
            return None;
        }
        let owner = owner_key(uuid);
        if let Some(record) = self
            .records
            .iter()
            .find(|r| r.container == container && r.owner == owner)
        {
            return (record.size as usize == size).then_some(record.id);
        }
        if !(9..=54).contains(&size) || !size.is_multiple_of(9) {
            return None;
        }
        let id = self.next_id.checked_add(1)?;
        if id > max_id {
            return None;
        }
        let slots = generate()?;
        if slots.len() != size {
            return None;
        }
        let mut record = Record {
            id,
            container: container.into(),
            site: None,
            owner,
            size: size as i32,
            items: Vec::new(),
            visual_opened: true,
        };
        record.replace(slots.iter().map(|s| s.as_deref()))?;
        self.next_id = id;
        self.records.push(record);
        Some(id)
    }

    pub fn get(&self, id: i64, uuid: [u8; 16]) -> Option<&Record> {
        let owner = owner_key(uuid);
        self.records.iter().find(|r| r.id == id && r.owner == owner)
    }

    pub fn get_mut(&mut self, id: i64, uuid: [u8; 16]) -> Option<&mut Record> {
        let owner = owner_key(uuid);
        self.records
            .iter_mut()
            .find(|r| r.id == id && r.owner == owner)
    }

    /// Upstream refresh clears all private inventories for this container.
    /// Never recycle menu IDs, even if a stale screen submits another click.
    pub fn refresh(&mut self, container: &str) -> usize {
        if let Some(schedule) = self.schedules.get_mut(container) {
            schedule.refresh_at = None;
        }
        if self
            .schedules
            .get(container)
            .is_some_and(|s| *s == crate::lifecycle::Schedule::default())
        {
            self.schedules.remove(container);
        }
        let before = self.records.len();
        self.records.retain(|record| {
            if record.container != container {
                return true;
            }
            if let Some(site) = &record.site {
                self.bindings
                    .entry(record.container.clone())
                    .or_insert_with(|| site.clone());
            }
            false
        });
        before - self.records.len()
    }

    pub(crate) fn schedule(&self, container: &str) -> Option<&crate::lifecycle::Schedule> {
        self.schedules.get(container)
    }

    pub(crate) fn set_schedule(
        &mut self,
        container: &str,
        schedule: crate::lifecycle::Schedule,
    ) -> bool {
        if !self.bindings.contains_key(container) || !schedule.valid() {
            return false;
        }
        if schedule == crate::lifecycle::Schedule::default() {
            self.schedules.remove(container);
        } else {
            self.schedules.insert(container.into(), schedule);
        }
        true
    }

    /// Evaluate a bound container against a supplied persisted world age.
    /// Refresh and its deadline reset are one shard mutation. Decay is a
    /// proposal: callers must guard the block/entity replacement separately.
    pub(crate) fn advance_lifecycle(
        &mut self,
        container: &str,
        config: &crate::lifecycle::Config,
        now: i64,
        table: Option<&str>,
        ticking: bool,
    ) -> Option<crate::lifecycle::Action> {
        use crate::lifecycle::Action;
        let site = self.bindings.get(container)?.clone();
        let mut schedule = self.schedule(container).cloned().unwrap_or_default();
        let action = schedule.due(config, now, ticking);
        if action == Action::Decay {
            return Some(action);
        }
        if action == Action::Refresh {
            self.refresh(container);
            schedule.refresh_at = None;
            // The Java tick clears hasBeenOpened; only a subsequent open
            // starts the next deadline. The open path starts it immediately.
            if ticking {
                self.set_schedule(container, schedule);
                return Some(action);
            }
        }
        if !ticking || self.records.iter().any(|r| r.container == container) {
            schedule.begin(config, now, table, &site.dimension, ticking);
        }
        self.set_schedule(container, schedule);
        Some(action)
    }

    pub(crate) fn mark_visual(&mut self, container: &str, uuid: [u8; 16], opened: bool) -> bool {
        let owner = owner_key(uuid);
        let Some(record) = self
            .records
            .iter_mut()
            .find(|r| r.container == container && r.owner == owner)
        else {
            return false;
        };
        let changed = record.visual_opened != opened;
        record.visual_opened = opened;
        changed
    }

    /// The upstream administrative clear removes a player's stored inventories.
    pub fn clear_player(&mut self, uuid: [u8; 16]) -> usize {
        let owner = owner_key(uuid);
        let before = self.records.len();
        self.records.retain(|record| {
            if record.owner != owner {
                return true;
            }
            if let Some(site) = &record.site {
                self.bindings
                    .entry(record.container.clone())
                    .or_insert_with(|| site.clone());
            }
            false
        });
        before - self.records.len()
    }
}

pub fn valid_id(id: &str) -> bool {
    id.len() == 32
        && id
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

impl Record {
    pub fn slots(&self) -> Vec<Option<Vec<u8>>> {
        let mut slots = vec![None; self.size as usize];
        for item in &self.items {
            if let Some(slot) = slots.get_mut(item.index as usize) {
                *slot = Some(item.stack.iter().map(|b| *b as u8).collect());
            }
        }
        slots
    }

    pub fn replace<'a>(
        &mut self,
        slots: impl ExactSizeIterator<Item = Option<&'a [u8]>>,
    ) -> Option<()> {
        if slots.len() != self.size as usize {
            return None;
        }
        let mut items = Vec::new();
        for (index, stack) in slots.enumerate() {
            if let Some(stack) = stack {
                if stack.is_empty() || stack.len() > moddedmc_api::abi::MAX_STACK_BYTES {
                    return None;
                }
                items.push(Slot {
                    index: index as i32,
                    stack: fastnbt::ByteArray::new(stack.iter().map(|b| *b as i8).collect()),
                });
            }
        }
        self.items = items;
        Some(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const CHEST: &str = "00000000000000000000000000000001";

    #[test]
    fn different_players_get_independent_claims_and_empty_stays_empty_after_reload() {
        let mut a = Archive::default();
        let first = a
            .open(CHEST, [1; 16], 27, || {
                let mut slots = vec![None; 27];
                slots[0] = Some(vec![1, 2]);
                Some(slots)
            })
            .unwrap();
        let second = a.open(CHEST, [2; 16], 27, || Some(vec![None; 27])).unwrap();
        assert_ne!(first, second);
        assert!(a.get(first, [2; 16]).is_none());
        a.get_mut(first, [1; 16])
            .unwrap()
            .replace(std::iter::repeat_n(None, 27))
            .unwrap();
        let bytes = fastnbt::to_bytes(&a).unwrap();
        let mut loaded: Archive = fastnbt::from_bytes(&bytes).unwrap();
        assert!(loaded.validate());
        assert_eq!(
            loaded.open(CHEST, [1; 16], 27, || panic!("must not regenerate")),
            Some(first)
        );
        assert!(
            loaded
                .get(first, [1; 16])
                .unwrap()
                .slots()
                .iter()
                .all(Option::is_none)
        );
    }

    #[test]
    fn malformed_archive_and_failed_replacement_are_rejected() {
        let mut a = Archive::default();
        let id = a.open(CHEST, [1; 16], 27, || Some(vec![None; 27])).unwrap();
        assert!(a.validate());
        let record = a.get_mut(id, [1; 16]).unwrap();
        assert!(
            record
                .replace(std::iter::repeat_n(Some(&[][..]), 27))
                .is_none()
        );
        assert!(record.slots().iter().all(Option::is_none));
        a.records.push(a.records[0].clone());
        assert!(!a.validate());
        a.records.pop();
        a.records[0].size = -1;
        assert!(!a.validate());
    }

    #[test]
    fn replacement_container_never_inherits_old_claim() {
        let mut a = Archive::default();
        let old = a.open(CHEST, [1; 16], 27, || Some(vec![None; 27])).unwrap();
        let new = a
            .open("00000000000000000000000000000002", [1; 16], 27, || {
                Some(vec![None; 27])
            })
            .unwrap();
        assert_ne!(old, new);
        assert!(a.validate());
    }

    #[test]
    fn failed_generation_and_reset_do_not_reuse_menu_ids() {
        let mut a = Archive::default();
        assert!(a.open(CHEST, [1; 16], 27, || None).is_none());
        assert!(a.records.is_empty());
        let first = a.open(CHEST, [1; 16], 27, || Some(vec![None; 27])).unwrap();
        let second = a.open(CHEST, [2; 16], 27, || Some(vec![None; 27])).unwrap();
        assert_eq!(a.clear_player([1; 16]), 1);
        assert!(a.get(first, [1; 16]).is_none());
        assert!(a.get(second, [2; 16]).is_some());
        assert_eq!(a.refresh(CHEST), 1);
        assert!(a.get(second, [2; 16]).is_none());
        let third = a.open(CHEST, [1; 16], 27, || Some(vec![None; 27])).unwrap();
        assert!(third > second);
        assert!(a.validate());
    }

    #[test]
    fn cloned_container_id_cannot_bind_to_another_site() {
        let mut a = Archive::default();
        let first = a.open(CHEST, [1; 16], 27, || Some(vec![None; 27])).unwrap();
        let site = Site {
            dimension: "minecraft:overworld".into(),
            position: [1, 2, 3],
        };
        a.get_mut(first, [1; 16]).unwrap().site = Some(site.clone());
        assert!(a.site_available(CHEST, &site));
        let other = Site {
            position: [1, 2, 4],
            ..site
        };
        assert!(!a.site_available(CHEST, &other));
        let second = a.open(CHEST, [2; 16], 27, || Some(vec![None; 27])).unwrap();
        a.get_mut(second, [2; 16]).unwrap().site = Some(other);
        assert!(!a.validate());
    }

    #[test]
    fn refresh_and_last_player_clear_keep_identity_bound() {
        let mut a = Archive::default();
        let site = Site {
            dimension: "minecraft:overworld".into(),
            position: [1, 2, 3],
        };
        let moved = Site {
            position: [1, 2, 4],
            ..site.clone()
        };
        let id = a.open(CHEST, [1; 16], 27, || Some(vec![None; 27])).unwrap();
        a.get_mut(id, [1; 16]).unwrap().site = Some(site.clone());
        assert_eq!(a.refresh(CHEST), 1);
        assert!(a.records.is_empty());
        assert!(!a.bind_site(CHEST, &moved));
        assert!(a.bind_site(CHEST, &site));
        let id2 = a.open(CHEST, [2; 16], 27, || Some(vec![None; 27])).unwrap();
        assert!(id2 > id);
        assert_eq!(a.clear_player([2; 16]), 1);
        let bytes = fastnbt::to_bytes(&a).unwrap();
        let loaded: Archive = fastnbt::from_bytes(&bytes).unwrap();
        assert!(loaded.validate());
        assert!(!loaded.site_available(CHEST, &moved));
        let partition = loaded.partition(|id| id == CHEST);
        assert!(partition.records.is_empty());
        assert!(!partition.site_available(CHEST, &moved));
    }

    #[test]
    fn persisted_refresh_is_atomic_with_claim_invalidation() {
        use crate::lifecycle::{Action, Config};
        let config =
            Config::parse("refresh_all=true\nrefresh_value=20\ndecay_all=true\ndecay_value=60")
                .unwrap();
        let mut archive = Archive::default();
        let site = Site {
            dimension: "minecraft:overworld".into(),
            position: [1, 2, 3],
        };
        assert!(archive.bind_site(CHEST, &site));
        assert_eq!(
            archive.advance_lifecycle(CHEST, &config, 100, None, false),
            Some(Action::None)
        );
        let id = archive
            .open(CHEST, [1; 16], 27, || Some(vec![None; 27]))
            .unwrap();
        assert!(archive.mark_visual(CHEST, [1; 16], false));
        assert!(archive.get(id, [1; 16]).is_some());
        let mut restored: Archive =
            fastnbt::from_bytes(&fastnbt::to_bytes(&archive).unwrap()).unwrap();
        assert!(restored.validate());
        assert_eq!(
            restored.advance_lifecycle(CHEST, &config, 120, None, true),
            Some(Action::Refresh)
        );
        assert!(restored.get(id, [1; 16]).is_none());
        assert_eq!(restored.schedule(CHEST).unwrap().decay_at, Some(160));
        assert_eq!(restored.schedule(CHEST).unwrap().refresh_at, None);
        assert_eq!(
            restored.advance_lifecycle(CHEST, &config, 121, None, true),
            Some(Action::None)
        );
        assert_eq!(restored.schedule(CHEST).unwrap().refresh_at, None);
        assert_eq!(
            restored.advance_lifecycle(CHEST, &config, 130, None, false),
            Some(Action::None)
        );
        assert_eq!(restored.schedule(CHEST).unwrap().refresh_at, Some(150));
        let next = restored
            .open(CHEST, [1; 16], 27, || Some(vec![None; 27]))
            .unwrap();
        assert!(next > id);
        assert_eq!(
            restored.advance_lifecycle(CHEST, &config, 160, None, false),
            Some(Action::Decay)
        );
        assert!(restored.get(next, [1; 16]).is_some());
    }
}

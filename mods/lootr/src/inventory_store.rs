//! Per-container player inventory ownership and refresh/decay semantics.
//!
//! Oracle: LootrMinecraft/Lootr 2a9c830,
//! `common/data/LootrInventoryStore.java` and `api/data/ILootrInventoryStore.java`.
//! This is the mod-side data model. Actual item generation, UI and persistence require
//! host interfaces and must not be substituted with shared vanilla inventories.

use std::collections::{HashMap, HashSet};

pub type PlayerId = [u8; 16];

/// Each entry is one saved ItemStack NBT payload, or an empty slot.
pub type Inventory = Vec<Option<Vec<u8>>>;

#[derive(Debug)]
pub struct InventoryStore {
    inventories: HashMap<PlayerId, Inventory>,
    visual_openers: HashSet<PlayerId>,
    actual_openers: HashSet<PlayerId>,
    has_inventories: bool,
    decay_time: i64,
    refresh_time: i64,
    dirty: bool,
}

impl Default for InventoryStore {
    fn default() -> Self {
        Self {
            inventories: HashMap::new(),
            visual_openers: HashSet::new(),
            actual_openers: HashSet::new(),
            has_inventories: false,
            decay_time: -1,
            refresh_time: -1,
            dirty: false,
        }
    }
}

impl InventoryStore {
    /// Team resolution belongs to the caller: pass the same team UUID that
    /// LootrAPI.resolveServerPlayerTeam returns, or the player's own UUID by default.
    pub fn get_or_create(
        &mut self,
        identity: PlayerId,
        initial_contents: impl FnOnce() -> Inventory,
    ) -> &mut Inventory {
        use std::collections::hash_map::Entry;

        match self.inventories.entry(identity) {
            Entry::Occupied(occupied) => {
                // The returned reference permits slot changes; a saved inventory must become dirty.
                self.dirty = true;
                occupied.into_mut()
            }
            Entry::Vacant(vacant) => {
                let contents = initial_contents();
                self.has_inventories = true;
                self.dirty = true;
                vacant.insert(contents)
            }
        }
    }

    pub fn get(&self, identity: PlayerId) -> Option<&Inventory> {
        self.inventories.get(&identity)
    }

    pub fn get_mut(&mut self, identity: PlayerId) -> Option<&mut Inventory> {
        self.dirty |= self.inventories.contains_key(&identity);
        self.inventories.get_mut(&identity)
    }

    pub fn add_visual_opener(&mut self, identity: PlayerId) -> bool {
        let newly_added = self.visual_openers.insert(identity);
        self.dirty |= newly_added;
        newly_added
    }

    pub fn remove_visual_opener(&mut self, identity: PlayerId) -> bool {
        let removed = self.visual_openers.remove(&identity);
        self.dirty |= removed;
        removed
    }

    pub fn add_actual_opener(&mut self, identity: PlayerId) -> bool {
        let newly_added = self.actual_openers.insert(identity);
        self.dirty |= newly_added;
        newly_added
    }

    pub fn visual_openers(&self) -> &HashSet<PlayerId> {
        &self.visual_openers
    }

    pub fn actual_openers(&self) -> &HashSet<PlayerId> {
        &self.actual_openers
    }

    pub fn has_inventories(&self) -> bool {
        self.has_inventories
    }

    pub fn can_be_culled(&self) -> bool {
        self.inventories.is_empty() && !self.has_inventories
    }

    /// Java's `decayTime = LootrAPI.getGameTime() + getDecayValue()`.
    pub fn begin_decay(&mut self, game_time: i64, delay: i64) {
        self.decay_time = game_time.wrapping_add(delay);
        self.dirty = true;
    }

    /// Java's `refreshTime = LootrAPI.getGameTime() + getRefreshValue()`.
    pub fn begin_refresh(&mut self, game_time: i64, delay: i64) {
        self.refresh_time = game_time.wrapping_add(delay);
        self.dirty = true;
    }

    pub fn is_decaying(&self) -> bool {
        self.decay_time != -1
    }

    pub fn is_refreshing(&self) -> bool {
        self.refresh_time != -1
    }

    pub fn is_decayed(&self, game_time: i64) -> bool {
        self.is_decaying() && game_time >= self.decay_time
    }

    pub fn is_refreshed(&self, game_time: i64) -> bool {
        self.is_refreshing() && game_time >= self.refresh_time
    }

    pub fn remaining_decay_time(&self, game_time: i64) -> i32 {
        if self.decay_time == -1 {
            -1
        } else {
            self.decay_time.wrapping_sub(game_time) as i32
        }
    }

    pub fn remaining_refresh_time(&self, game_time: i64) -> i32 {
        if self.refresh_time == -1 {
            -1
        } else {
            self.refresh_time.wrapping_sub(game_time) as i32
        }
    }

    /// Matches LootrInventoryStore.performRefresh: actualOpeners are intentionally
    /// NOT cleared, unlike visualOpeners, and decayTime is retained.
    pub fn perform_refresh(&mut self) {
        self.visual_openers.clear();
        self.inventories.clear();
        self.has_inventories = false;
        self.refresh_time = -1;
        self.dirty = true;
    }

    /// Matches LootrInventoryStore.clearInventories: `hasInventories` is not reset.
    pub fn clear_inventories(&mut self, identity: PlayerId) -> bool {
        if self.inventories.remove(&identity).is_some() {
            self.visual_openers.remove(&identity);
            self.dirty = true;
            true
        } else {
            false
        }
    }

    pub fn dirty(&self) -> bool {
        self.dirty
    }

    pub fn mark_saved(&mut self) {
        self.dirty = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALICE: PlayerId = [1; 16];
    const BOB: PlayerId = [2; 16];

    #[test]
    fn independently_generated_inventories_are_owned_per_resolved_identity() {
        let mut data = InventoryStore::default();
        data.get_or_create(ALICE, || vec![Some(vec![1]), None]);
        data.get_or_create(BOB, || vec![Some(vec![2]), None]);
        data.get_mut(ALICE).unwrap()[0] = None;
        assert_eq!(data.get(ALICE).unwrap()[0], None);
        assert_eq!(data.get(BOB).unwrap()[0], Some(vec![2]));
        assert!(data.has_inventories());
        assert!(!data.can_be_culled());
    }

    #[test]
    fn repeat_open_does_not_generate_second_loot() {
        let mut data = InventoryStore::default();
        data.get_or_create(ALICE, || vec![Some(vec![8])]);
        let stored = data.get_or_create(ALICE, || panic!("must not roll loot again"));
        assert_eq!(stored[0], Some(vec![8]));
    }

    #[test]
    fn editing_a_saved_inventory_on_reopen_marks_the_store_dirty() {
        let mut data = InventoryStore::default();
        data.get_or_create(ALICE, || vec![Some(vec![1])]);
        data.mark_saved();
        assert!(!data.dirty());

        data.get_or_create(ALICE, || panic!("must not regenerate loot"))[0] = None;
        assert!(data.dirty());
        assert_eq!(data.get(ALICE).unwrap()[0], None);
    }

    #[test]
    fn refresh_clears_loot_and_visual_openers_but_retains_actual_openers_and_decay() {
        let mut data = InventoryStore::default();
        data.get_or_create(ALICE, || vec![Some(vec![8])]);
        data.add_visual_opener(ALICE);
        data.add_actual_opener(ALICE);
        data.begin_decay(100, 10);
        data.begin_refresh(100, 5);
        data.perform_refresh();
        assert!(!data.has_inventories());
        assert!(data.can_be_culled());
        assert!(data.get(ALICE).is_none());
        assert!(!data.visual_openers().contains(&ALICE));
        assert!(data.actual_openers().contains(&ALICE));
        assert!(!data.is_refreshing());
        assert!(data.is_decaying());
    }

    #[test]
    fn individual_clear_preserves_has_inventories_flag_and_other_players() {
        let mut data = InventoryStore::default();
        data.get_or_create(ALICE, || vec![Some(vec![1])]);
        data.get_or_create(BOB, || vec![Some(vec![2])]);
        data.add_visual_opener(ALICE);
        assert!(data.clear_inventories(ALICE));
        assert!(!data.clear_inventories(ALICE));
        assert!(data.get(ALICE).is_none());
        assert!(data.get(BOB).is_some());
        assert!(!data.visual_openers().contains(&ALICE));
        assert!(data.has_inventories());
        assert!(!data.can_be_culled());
    }

    #[test]
    fn refresh_and_decay_use_game_time_and_negative_one_is_the_unscheduled_sentinel() {
        let mut data = InventoryStore::default();
        assert!(!data.is_decaying());
        assert!(!data.is_refreshing());
        assert_eq!(data.remaining_decay_time(10), -1);
        assert_eq!(data.remaining_refresh_time(10), -1);
        data.begin_decay(40, 15);
        data.begin_refresh(40, 5);
        assert!(!data.is_decayed(54));
        assert!(data.is_decayed(55));
        assert!(!data.is_refreshed(44));
        assert!(data.is_refreshed(45));
        assert_eq!(data.remaining_decay_time(56), -1);
        assert_eq!(data.remaining_refresh_time(42), 3);
        assert!(data.dirty());
        data.mark_saved();
        assert!(!data.dirty());
    }
}

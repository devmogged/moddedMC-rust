//! Create 6.0.10 BasinInventory insertion semantics (SmartInventory wrapping
//! a nine-slot item handler). Pure model until live basin registration is wired.
//! Original: content/processing/basin/BasinInventory.java.

use fastnbt::Value;

pub const BASIN_SLOTS: usize = 9;
pub const SLOT_LIMIT: u32 = 64;

#[derive(Clone, Debug, PartialEq)]
pub struct BasinStack {
    pub id: String,
    pub amount: u32,
    pub components: Option<Value>,
}

impl BasinStack {
    fn same_item(&self, other: &Self) -> bool {
        self.id == other.id
            && match (&self.components, &other.components) {
                (None, Some(Value::Compound(empty))) | (Some(Value::Compound(empty)), None) => {
                    empty.is_empty()
                }
                (left, right) => left == right,
            }
    }

    fn is_empty(&self) -> bool {
        self.amount == 0 || self.id.is_empty() || self.id == "minecraft:air"
    }
}

#[derive(Clone, Debug)]
pub struct BasinInventory {
    slots: [Option<BasinStack>; BASIN_SLOTS],
    pub packager_mode: bool,
    /// The output inventory used by BasinBlockEntity forbids insertion.
    pub allow_insertion: bool,
}

impl Default for BasinInventory {
    fn default() -> Self {
        Self {
            slots: std::array::from_fn(|_| None),
            packager_mode: false,
            allow_insertion: true,
        }
    }
}

impl BasinInventory {
    pub fn output() -> Self {
        Self {
            allow_insertion: false,
            ..Self::default()
        }
    }

    pub fn slot(&self, index: usize) -> Option<&BasinStack> {
        self.slots.get(index).and_then(Option::as_ref)
    }

    /// Returns the uninserted quantity, like NeoForge IItemHandler.insertItem.
    /// A simulation never mutates slots; duplicate stacks in different slots
    /// are forbidden except in packager mode. The first empty slot is mandatory.
    pub fn insert(&mut self, slot: usize, incoming: &BasinStack, simulate: bool) -> u32 {
        if slot >= BASIN_SLOTS || !self.allow_insertion || incoming.is_empty() {
            return incoming.amount;
        }
        if !self.packager_mode {
            let mut first_free = None;
            for (index, occupied) in self.slots.iter().enumerate() {
                match occupied {
                    Some(existing) if index != slot && existing.same_item(incoming) => {
                        return incoming.amount;
                    }
                    None if first_free.is_none() => first_free = Some(index),
                    _ => {}
                }
            }
            if self.slots[slot].is_none() && first_free != Some(slot) {
                return incoming.amount;
            }
        }

        let accepted = match self.slots[slot].as_ref() {
            Some(existing) if existing.same_item(incoming) => incoming
                .amount
                .min(SLOT_LIMIT.saturating_sub(existing.amount)),
            Some(_) => 0,
            None => incoming.amount.min(SLOT_LIMIT),
        };
        if !simulate && accepted > 0 {
            match &mut self.slots[slot] {
                Some(existing) => existing.amount += accepted,
                empty @ None => {
                    let mut stored = incoming.clone();
                    stored.amount = accepted;
                    *empty = Some(stored);
                }
            }
        }
        incoming.amount - accepted
    }

    /// Returns a matching extracted stack, or None on an empty/invalid slot.
    /// The caller performs BasinBlockEntity.notifyChangeOfContents on execute.
    pub fn extract(&mut self, slot: usize, count: u32, simulate: bool) -> Option<BasinStack> {
        let existing = self.slots.get(slot)?.as_ref()?;
        let taken = count.min(existing.amount);
        if taken == 0 {
            return None;
        }
        let mut extracted = existing.clone();
        extracted.amount = taken;
        if !simulate {
            let remaining = existing.amount - taken;
            if remaining == 0 {
                self.slots[slot] = None;
            } else if let Some(existing) = self.slots[slot].as_mut() {
                existing.amount = remaining;
            }
        }
        Some(extracted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stack(id: &str, amount: u32) -> BasinStack {
        BasinStack {
            id: id.into(),
            amount,
            components: None,
        }
    }

    #[test]
    fn non_packager_disallows_duplicates_in_other_slots() {
        let mut basin = BasinInventory::default();
        assert_eq!(
            basin.insert(0, &stack("minecraft:iron_ingot", 20), false),
            0
        );
        assert_eq!(
            basin.insert(1, &stack("minecraft:iron_ingot", 15), false),
            15
        );
        assert_eq!(
            basin.insert(0, &stack("minecraft:iron_ingot", 50), false),
            6
        );
        assert_eq!(basin.slot(0).unwrap().amount, 64);
        assert!(basin.slot(1).is_none());
    }

    #[test]
    fn requires_first_free_slot_in_normal_mode() {
        let mut basin = BasinInventory::default();
        assert_eq!(
            basin.insert(3, &stack("minecraft:copper_ingot", 1), false),
            1
        );
        assert_eq!(
            basin.insert(0, &stack("minecraft:copper_ingot", 1), false),
            0
        );
        assert_eq!(basin.insert(2, &stack("minecraft:iron_ingot", 1), false), 1);
        assert_eq!(basin.insert(1, &stack("minecraft:iron_ingot", 1), false), 0);
    }

    #[test]
    fn packager_mode_allows_duplicate_slots_but_respects_capacity() {
        let mut basin = BasinInventory {
            packager_mode: true,
            ..BasinInventory::default()
        };
        assert_eq!(
            basin.insert(4, &stack("minecraft:iron_ingot", 70), false),
            6
        );
        assert_eq!(
            basin.insert(5, &stack("minecraft:iron_ingot", 12), false),
            0
        );
        assert_eq!(basin.slot(4).unwrap().amount, 64);
        assert_eq!(basin.slot(5).unwrap().amount, 12);
    }

    #[test]
    fn simulate_does_not_change_contents_and_extract_releases_slot() {
        let mut basin = BasinInventory::default();
        let copper = stack("minecraft:copper_ingot", 10);
        assert_eq!(basin.insert(0, &copper, true), 0);
        assert!(basin.slot(0).is_none());
        basin.insert(0, &copper, false);
        assert_eq!(basin.extract(0, 20, true), Some(copper.clone()));
        assert_eq!(basin.slot(0).unwrap().amount, 10);
        assert_eq!(basin.extract(0, 20, false), Some(copper));
        assert!(basin.slot(0).is_none());
    }

    #[test]
    fn output_rejects_insertion_and_different_components_are_distinct() {
        let mut output = BasinInventory::output();
        assert_eq!(
            output.insert(0, &stack("minecraft:iron_ingot", 1), false),
            1
        );
        let mut basin = BasinInventory::default();
        let mut enchanted = stack("minecraft:iron_ingot", 2);
        enchanted.components = Some(Value::String("special".into()));
        basin.insert(0, &enchanted, false);
        assert_eq!(basin.insert(1, &stack("minecraft:iron_ingot", 3), false), 0);
        assert_eq!(basin.insert(1, &enchanted, false), 2);
    }
}

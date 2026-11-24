//! Create 6.0.10 BasinInventory, a SmartInventory with nine slots.
//! Original: content/processing/basin/BasinInventory.java.

use std::collections::HashMap;

use fastnbt::Value;

use crate::{belt_inventory::count, processing_recipes::item_id};

pub const BASIN_SLOTS: usize = 9;
pub const SLOT_LIMIT: u32 = 64;

#[derive(Clone, Debug, PartialEq)]
pub struct BasinStack {
    pub id: String,
    pub amount: u32,
    pub components: Option<Value>,
}

impl BasinStack {
    pub fn from_nbt(item: &Value) -> Option<Self> {
        let Value::Compound(fields) = item else {
            return None;
        };
        let stack = Self {
            id: item_id(item)?.to_owned(),
            amount: u32::try_from(count(item)).ok()?,
            components: fields.get("components").cloned(),
        };
        (!stack.is_empty()).then_some(stack)
    }

    pub fn nbt(&self) -> Value {
        let mut fields = HashMap::from([
            ("id".to_owned(), Value::String(self.id.clone())),
            ("count".to_owned(), Value::Int(self.amount as i32)),
        ]);
        if let Some(components) = &self.components {
            fields.insert("components".to_owned(), components.clone());
        }
        Value::Compound(fields)
    }

    pub(crate) fn same_item(&self, other: &Self) -> bool {
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

    pub fn restore_slot(&mut self, index: usize, stack: BasinStack) {
        if let Some(slot) = self.slots.get_mut(index) {
            *slot = Some(stack);
        }
    }

    pub fn load(&mut self, data: Option<&Value>) {
        let Some(Value::Compound(inventory)) = data else {
            return;
        };
        let Some(Value::List(stacks)) = inventory.get("Items") else {
            return;
        };
        for item in stacks.iter().take(BASIN_SLOTS) {
            let Value::Compound(fields) = item else {
                continue;
            };
            if let Some(Value::Int(slot)) = fields.get("Slot")
                && let Some(slot) = usize::try_from(*slot)
                    .ok()
                    .and_then(|slot| self.slots.get_mut(slot))
            {
                *slot = BasinStack::from_nbt(item).filter(|stored| stored.amount <= SLOT_LIMIT);
            }
        }
    }

    pub fn save(&self) -> Value {
        let items = self.slots.iter().enumerate().filter_map(|(index, stack)| {
            let Value::Compound(mut stack) = stack.as_ref()?.nbt() else {
                return None;
            };
            stack.insert("Slot".to_owned(), Value::Int(index as i32));
            Some(Value::Compound(stack))
        });
        Value::Compound(HashMap::from([
            ("Size".to_owned(), Value::Int(BASIN_SLOTS as i32)),
            ("Items".to_owned(), Value::List(items.collect())),
        ]))
    }

    pub fn insert_stack(&mut self, stack: &BasinStack, max: u32) -> u32 {
        let matching = self.slots.iter().position(|stored| {
            stored
                .as_ref()
                .is_some_and(|stored| stored.same_item(stack))
        });
        let target = matching.or_else(|| self.slots.iter().position(Option::is_none));
        let Some(slot) = target else { return 0 };
        let existing = self.slot(slot).map_or(0, |stored| stored.amount);
        let mut offered = stack.clone();
        offered.amount = offered.amount.min(max.saturating_sub(existing));
        offered.amount - self.insert(slot, &offered, false)
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

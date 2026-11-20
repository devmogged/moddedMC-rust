//! Single-slot cutting-board item inventory, based on Farmer's Delight 1.3.4.
//!
//! Source: CuttingBoardBlockEntity::createHandler/addItem/removeItem and
//! NeoForge 1.21.1 ItemStackHandler::insertItem/extractItem/serializeNBT.
//! This is the mod-owned inventory algorithm; the host still needs to expose
//! a generic container provider for hopper interactions and an accurate block.

use fastnbt::Value;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StackError {
    InvalidStack,
    InvalidLimit,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CuttingBoardInventory {
    held: Option<Value>,
    is_item_carved: bool,
}

#[derive(Serialize, Deserialize)]
struct SavedInventory {
    #[serde(rename = "Size", default = "default_size")]
    size: i32,
    #[serde(rename = "Items", default)]
    items: Vec<Value>,
}

const fn default_size() -> i32 {
    1
}

impl Default for SavedInventory {
    fn default() -> Self {
        Self {
            size: 1,
            items: Vec::new(),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct SavedBoard {
    #[serde(rename = "Inventory", default)]
    inventory: SavedInventory,
    #[serde(rename = "IsItemCarved", default)]
    is_item_carved: bool,
}

fn parts(stack: &Value) -> Option<(&str, u32, Option<&Value>)> {
    let Value::Compound(fields) = stack else {
        return None;
    };
    let Some(Value::String(id)) = fields.get("id") else {
        return None;
    };
    let Some(Value::Int(count)) = fields.get("count") else {
        return None;
    };
    let count = u32::try_from(*count).ok().filter(|count| *count > 0)?;
    (!id.is_empty()).then_some((id.as_str(), count, fields.get("components")))
}

fn set_count(stack: &Value, count: u32) -> Result<Value, StackError> {
    let count = i32::try_from(count).map_err(|_| StackError::InvalidStack)?;
    let Value::Compound(mut fields) = stack.clone() else {
        return Err(StackError::InvalidStack);
    };
    fields.insert("count".into(), Value::Int(count));
    Ok(Value::Compound(fields))
}

fn components_match(left: Option<&Value>, right: Option<&Value>) -> bool {
    match (left, right) {
        (None, None) => true,
        (None, Some(Value::Compound(fields))) | (Some(Value::Compound(fields)), None) => {
            fields.is_empty()
        }
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

impl CuttingBoardInventory {
    pub fn held(&self) -> Option<&Value> {
        self.held.as_ref()
    }

    pub fn carved(&self) -> bool {
        self.is_item_carved
    }

    /// Mirrors vanilla ItemStackHandler inserting into slot 0. Empty remainder
    /// is represented by None. Item-specific max_count comes from the host's
    /// authoritative item component data, not a hardcoded vanilla carrier.
    pub fn insert(
        &mut self,
        incoming: &Value,
        item_max_count: u32,
        slot_limit: u32,
        simulate: bool,
    ) -> Result<Option<Value>, StackError> {
        let Some((incoming_id, count, incoming_components)) = parts(incoming) else {
            return Err(StackError::InvalidStack);
        };
        let limit = item_max_count.min(slot_limit);
        if limit == 0 {
            return Err(StackError::InvalidLimit);
        }
        let accepted = match self.held.as_ref() {
            None => limit.min(count),
            Some(held) => {
                let (stored_id, stored_count, stored_components) =
                    parts(held).ok_or(StackError::InvalidStack)?;
                if incoming_id != stored_id
                    || !components_match(incoming_components, stored_components)
                {
                    0
                } else {
                    limit.saturating_sub(stored_count).min(count)
                }
            }
        };
        if accepted == 0 {
            return Ok(Some(incoming.clone()));
        }
        if !simulate {
            let stack = match self.held.as_ref() {
                Some(old) => {
                    let old_count = parts(old).ok_or(StackError::InvalidStack)?.1;
                    set_count(old, old_count + accepted)?
                }
                None => set_count(incoming, accepted)?,
            };
            self.held = Some(stack);
        }
        let remaining = count - accepted;
        if remaining == 0 {
            Ok(None)
        } else {
            set_count(incoming, remaining).map(Some)
        }
    }

    /// Player-facing CuttingBoardBlockEntity.canAddItem: unlike the raw
    /// ItemStackHandler capability, a carved-tool marker prevents placement.
    pub fn can_add_item(&self) -> bool {
        self.held.is_none() && !self.is_item_carved
    }

    /// Player-facing CuttingBoardBlockEntity.addItem, separate from the
    /// capability's raw ItemStackHandler.insertItem semantics.
    pub fn add_item(
        &mut self,
        incoming: &Value,
        item_max_count: u32,
        slot_limit: u32,
    ) -> Result<Option<Value>, StackError> {
        if !self.can_add_item() {
            return Ok(Some(incoming.clone()));
        }
        self.insert(incoming, item_max_count, slot_limit, false)
    }

    /// Vanilla ItemStackHandler.extractItem; amount 0 leaves the inventory
    /// unchanged; otherwise extraction takes at most item_max_count.
    pub fn extract(
        &mut self,
        amount: u32,
        item_max_count: u32,
        simulate: bool,
    ) -> Result<Option<Value>, StackError> {
        let Some(held) = self.held.as_ref() else {
            return Ok(None);
        };
        let count = parts(held).ok_or(StackError::InvalidStack)?.1;
        if amount == 0 || item_max_count == 0 {
            return Ok(None);
        }
        let taken = count.min(amount.min(item_max_count));
        let result = set_count(held, taken)?;
        if !simulate {
            self.held = if count == taken {
                None
            } else {
                Some(set_count(held, count - taken)?)
            };
        }
        Ok(Some(result))
    }

    /// Source removeItem resets the tool-carving marker and extracts all
    /// available items, limited to one item's configured maximum count.
    pub fn remove(&mut self, item_max_count: u32) -> Result<Option<Value>, StackError> {
        self.is_item_carved = false;
        self.extract(u32::MAX, item_max_count, false)
    }

    /// A successful cutting action consumes exactly one input item. The
    /// caller must commit this only after required effects/output operations.
    pub fn consume_one(&mut self) -> Result<Option<Value>, StackError> {
        self.extract(1, 1, false)
    }

    /// Capture the exact NeoForge one-slot Inventory compound structure and
    /// the CuttingBoardBlockEntity IsItemCarved flag as named-root Java NBT.
    pub fn save_nbt(&self) -> Result<Vec<u8>, fastnbt::error::Error> {
        let mut items = Vec::new();
        if let Some(Value::Compound(mut fields)) = self.held.clone() {
            fields.insert("Slot".into(), Value::Int(0));
            items.push(Value::Compound(fields));
        }
        fastnbt::to_bytes(&SavedBoard {
            inventory: SavedInventory { size: 1, items },
            is_item_carved: self.is_item_carved,
        })
    }

    pub fn load_nbt(bytes: &[u8]) -> Option<Self> {
        let saved: SavedBoard = fastnbt::from_bytes(bytes).ok()?;
        if saved.inventory.size != 1 || saved.inventory.items.len() > 1 {
            return None;
        }
        let mut held = None;
        for item in saved.inventory.items {
            let Value::Compound(mut fields) = item else {
                return None;
            };
            if fields.remove("Slot") != Some(Value::Int(0)) {
                return None;
            }
            let stack = Value::Compound(fields);
            parts(&stack)?;
            held = Some(stack);
        }
        Some(Self {
            held,
            is_item_carved: saved.is_item_carved,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn stack(id: &str, count: i32, variant: i32) -> Value {
        let mut fields = HashMap::new();
        fields.insert("id".into(), Value::String(id.into()));
        fields.insert("count".into(), Value::Int(count));
        if variant != 0 {
            fields.insert(
                "components".into(),
                Value::Compound(HashMap::from([(
                    "minecraft:custom_model_data".into(),
                    Value::Int(variant),
                )])),
            );
        }
        Value::Compound(fields)
    }

    fn count(stack: &Value) -> u32 {
        parts(stack).unwrap().1
    }

    #[test]
    fn empty_slot_and_stack_caps_are_exact_and_simulation_never_mutates() {
        let mut board = CuttingBoardInventory::default();
        let offered = stack("minecraft:carrot", 70, 0);
        let remainder = board.insert(&offered, 64, 99, true).unwrap().unwrap();
        assert_eq!(count(&remainder), 6);
        assert!(board.held().is_none());
        let remainder = board.insert(&offered, 64, 99, false).unwrap().unwrap();
        assert_eq!(count(board.held().unwrap()), 64);
        assert_eq!(count(&remainder), 6);
        assert_eq!(count(&offered), 70);
        assert_eq!(
            board.insert(&offered, 64, 99, false).unwrap(),
            Some(offered)
        );
    }

    #[test]
    fn matching_components_merge_but_different_components_do_not() {
        let mut board = CuttingBoardInventory::default();
        assert!(
            board
                .insert(&stack("minecraft:carrot", 8, 13), 64, 99, false)
                .unwrap()
                .is_none()
        );
        assert_eq!(
            board
                .insert(&stack("minecraft:carrot", 4, 12), 64, 99, false)
                .unwrap(),
            Some(stack("minecraft:carrot", 4, 12))
        );
        assert!(
            board
                .insert(&stack("minecraft:carrot", 4, 13), 64, 99, false)
                .unwrap()
                .is_none()
        );
        assert_eq!(count(board.held().unwrap()), 12);
        assert!(
            board
                .insert(&stack("minecraft:potato", 3, 13), 64, 99, false)
                .unwrap()
                .is_some()
        );
        assert_eq!(count(board.held().unwrap()), 12);
    }

    #[test]
    fn extract_is_bounded_and_one_item_cutting_preserves_remainder() {
        let mut board = CuttingBoardInventory::default();
        board
            .insert(&stack("minecraft:oak_log", 5, 0), 64, 99, false)
            .unwrap();
        assert_eq!(count(&board.extract(4, 64, true).unwrap().unwrap()), 4);
        assert_eq!(count(board.held().unwrap()), 5);
        assert_eq!(count(&board.consume_one().unwrap().unwrap()), 1);
        assert_eq!(count(board.held().unwrap()), 4);
        assert_eq!(count(&board.extract(10, 64, false).unwrap().unwrap()), 4);
        assert!(board.held().is_none());
        assert!(board.extract(1, 64, false).unwrap().is_none());
    }

    #[test]
    fn java_compatible_nbt_round_trip_preserves_single_slot_and_components() {
        let mut board = CuttingBoardInventory::default();
        board
            .insert(&stack("farmersdelight:rice_panicle", 16, 42), 64, 99, false)
            .unwrap();
        board.is_item_carved = true;
        let bytes = board.save_nbt().unwrap();
        let decoded: SavedBoard = fastnbt::from_bytes(&bytes).unwrap();
        assert_eq!(decoded.inventory.size, 1);
        assert_eq!(decoded.inventory.items.len(), 1);
        let Value::Compound(item) = &decoded.inventory.items[0] else {
            panic!("not compound")
        };
        assert_eq!(item.get("Slot"), Some(&Value::Int(0)));
        assert_eq!(item.get("count"), Some(&Value::Int(16)));
        assert!(decoded.is_item_carved);
        let loaded = CuttingBoardInventory::load_nbt(&bytes).unwrap();
        assert_eq!(loaded, board);
        assert_eq!(count(&loaded.held.unwrap()), 16);
    }

    #[test]
    fn malformed_inventory_size_and_duplicate_items_fail_closed() {
        let mut board = CuttingBoardInventory::default();
        board
            .insert(&stack("minecraft:carrot", 1, 0), 64, 99, false)
            .unwrap();
        let bytes = board.save_nbt().unwrap();
        let mut parsed: SavedBoard = fastnbt::from_bytes(&bytes).unwrap();
        parsed.inventory.size = 2;
        assert!(CuttingBoardInventory::load_nbt(&fastnbt::to_bytes(&parsed).unwrap()).is_none());
        parsed.inventory.size = 1;
        parsed
            .inventory
            .items
            .push(parsed.inventory.items[0].clone());
        assert!(CuttingBoardInventory::load_nbt(&fastnbt::to_bytes(&parsed).unwrap()).is_none());
        assert_eq!(
            board.insert(&stack("minecraft:carrot", 0, 0), 64, 99, false),
            Err(StackError::InvalidStack)
        );
    }

    #[test]
    fn missing_old_save_fields_load_as_empty_like_neo_forge() {
        let old_empty = std::collections::HashMap::<String, Value>::new();
        let bytes = fastnbt::to_bytes(&old_empty).unwrap();
        let loaded = CuttingBoardInventory::load_nbt(&bytes).unwrap();
        assert_eq!(loaded, CuttingBoardInventory::default());
        let saved: SavedBoard = fastnbt::from_bytes(&loaded.save_nbt().unwrap()).unwrap();
        assert_eq!(saved.inventory.size, 1);
        assert!(saved.inventory.items.is_empty());
        assert!(!saved.is_item_carved);
    }

    #[test]
    fn raw_handler_insert_after_carved_tool_extraction_preserves_marker() {
        let mut board = CuttingBoardInventory::default();
        board
            .insert(&stack("minecraft:iron_axe", 1, 0), 1, 99, false)
            .unwrap();
        board.is_item_carved = true;
        assert!(board.extract(1, 1, false).unwrap().is_some());
        assert!(board.carved()); // raw extraction must not reset this marker
        let offered = stack("minecraft:carrot", 3, 0);
        assert_eq!(
            board.add_item(&offered, 64, 99).unwrap(),
            Some(offered.clone())
        );
        assert!(board.held().is_none());
        assert!(board.insert(&offered, 64, 99, false).unwrap().is_none());
        assert_eq!(board.held(), Some(&offered));
        assert!(board.carved());
    }

    #[test]
    fn carving_blocks_insert_until_original_remove_resets_flag() {
        let mut board = CuttingBoardInventory::default();
        board
            .insert(&stack("minecraft:iron_axe", 1, 0), 1, 99, false)
            .unwrap();
        board.is_item_carved = true;
        let offered = stack("minecraft:carrot", 12, 0);
        assert_eq!(board.add_item(&offered, 64, 99).unwrap(), Some(offered));
        assert_eq!(count(board.held().unwrap()), 1);
        let removed = board.remove(1).unwrap().unwrap();
        assert_eq!(count(&removed), 1);
        assert!(!board.carved());
        assert!(board.held().is_none());
    }
}

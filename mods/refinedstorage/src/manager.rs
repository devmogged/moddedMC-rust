//! Refined Storage 1.12.4 Crafter Manager's persistent grid presentation settings.
//! Crafting requests themselves remain unavailable until processing transactions
//! can atomically debit storage and route inputs to the target machine.

use fastnbt::Value;
use moddedmc_api::{Pos, content::World};
use std::collections::HashMap;

type Compound = HashMap<String, Value>;
const KEY: &str = "refinedstorage_crafter_manager";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Manager {
    /// IGrid.SIZE_STRETCH is the source default (0); 1..3 are small/medium/large.
    pub size: u8,
    /// IGrid.SEARCH_BOX_MODE_NORMAL is the source default (0). JEI modes are
    /// retained as settings, but external JEI synchronization is not emulated.
    pub search_box_mode: u8,
}

impl Manager {
    pub fn set_size(&mut self, value: u8) -> bool {
        if value > 3 || value == self.size {
            return false;
        }
        self.size = value;
        true
    }
    pub fn cycle_size(&mut self) {
        self.size = (self.size + 1) % 4;
    }
    pub fn set_search_box_mode(&mut self, value: u8) -> bool {
        if value > 5 || value == self.search_box_mode {
            return false;
        }
        self.search_box_mode = value;
        true
    }
    pub fn cycle_search_box_mode(&mut self) {
        self.search_box_mode = (self.search_box_mode + 1) % 6;
    }
    pub fn visible_pattern_limit(&self) -> usize {
        match self.size {
            0 => 45,
            1 => 9,
            2 => 18,
            3 => 27,
            _ => 0,
        }
    }
    pub fn encode(&self, root: &mut Compound) -> Option<()> {
        if self.size > 3 || self.search_box_mode > 5 {
            return None;
        }
        root.insert(
            KEY.into(),
            Value::Compound(Compound::from([
                ("version".into(), Value::Int(1)),
                ("size".into(), Value::Int(self.size as i32)),
                (
                    "searchBoxMode".into(),
                    Value::Int(self.search_box_mode as i32),
                ),
            ])),
        );
        Some(())
    }
    pub fn decode(root: &Compound) -> Option<Self> {
        let Some(value) = root.get(KEY) else {
            return Some(Self::default());
        };
        let Value::Compound(saved) = value else {
            return None;
        };
        if saved.get("version") != Some(&Value::Int(1)) {
            return None;
        }
        let (Some(Value::Int(size)), Some(Value::Int(search))) =
            (saved.get("size"), saved.get("searchBoxMode"))
        else {
            return None;
        };
        let size = u8::try_from(*size).ok()?;
        let search_box_mode = u8::try_from(*search).ok()?;
        (size <= 3 && search_box_mode <= 5).then_some(Self {
            size,
            search_box_mode,
        })
    }
}

pub fn load(world: World<'_>, site: Pos) -> Option<Manager> {
    let snapshot = world.entity_snapshot(site)?;
    let root: Compound = fastnbt::from_bytes(&snapshot.nbt).ok()?;
    Manager::decode(&root)
}

pub fn update(world: World<'_>, site: Pos, change: impl FnOnce(&mut Manager) -> bool) -> bool {
    let Some(snapshot) = world.entity_snapshot(site) else {
        return false;
    };
    let Ok(mut root) = fastnbt::from_bytes::<Compound>(&snapshot.nbt) else {
        return false;
    };
    let Some(mut manager) = Manager::decode(&root) else {
        return false;
    };
    if !change(&mut manager) || manager.encode(&mut root).is_none() {
        return false;
    }
    let Ok(bytes) = fastnbt::to_bytes(&root) else {
        return false;
    };
    world.compare_entity(site, &snapshot, &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_size_and_search_settings_roundtrip_and_reject_invalid_values() {
        let mut manager = Manager::default();
        assert_eq!((manager.size, manager.search_box_mode), (0, 0));
        manager.cycle_size();
        manager.cycle_search_box_mode();
        assert_eq!((manager.size, manager.search_box_mode), (1, 1));
        assert_eq!(manager.visible_pattern_limit(), 9);
        assert!(!manager.set_size(4));
        assert!(!manager.set_search_box_mode(6));
        let mut root = Compound::from([("foreign".into(), Value::Int(31))]);
        manager.encode(&mut root).unwrap();
        let copy: Compound = fastnbt::from_bytes(&fastnbt::to_bytes(&root).unwrap()).unwrap();
        assert_eq!(copy.get("foreign"), Some(&Value::Int(31)));
        assert_eq!(Manager::decode(&copy), Some(manager));
        let mut corrupt = copy;
        let Value::Compound(saved) = corrupt.get_mut(KEY).unwrap() else {
            unreachable!()
        };
        saved.insert("size".into(), Value::Int(9));
        assert!(Manager::decode(&corrupt).is_none());
    }
}

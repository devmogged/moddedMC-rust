//! Items with no block: ingots, alloys, tools and the like.

use crate::{InterfaceUnavailable, RegistrationError, Registry, abi, require_interface_prefix, s};

#[derive(Clone, Copy)]
pub struct Items(&'static abi::ItemsV1);

pub fn require() -> Result<Items, InterfaceUnavailable> {
    // SAFETY: the base fields are unchanged and later fields are checked before use.
    unsafe { require_interface_prefix(abi::ITEMS, 1, abi::ITEMS_V1_BASE_SIZE) }.map(Items)
}

pub struct Item<'a> {
    pub id: &'a str,
    pub display_name: &'a str,
    /// The vanilla item it behaves like (stack size, food, tool), such as `minecraft:iron_ingot`.
    pub carrier: &'a str,
    pub model: &'a str,
}

impl Items {
    /// Give a registered item a durability bar and a single-item stack limit.
    pub fn item_durability(
        self,
        reg: &mut Registry<'_>,
        item: &str,
        max_damage: u16,
    ) -> Result<(), RegistrationError> {
        if self.0.size < size_of::<abi::ItemsV1>() {
            return Err(RegistrationError::InvalidDefinition);
        }
        // SAFETY: the table size was checked and the registrar and id are borrowed.
        RegistrationError::from_code(unsafe {
            (self.0.item_durability)(reg.reg, s(item), max_damage)
        })
    }

    pub fn add_item(
        self,
        reg: &mut Registry<'_>,
        item: &Item<'_>,
    ) -> Result<(), RegistrationError> {
        let def = abi::ContentItemDefinition {
            id: s(item.id),
            display_name: s(item.display_name),
            carrier: s(item.carrier),
            model: s(item.model),
        };
        // SAFETY: registration and the borrowed definition are live for the call.
        RegistrationError::from_code(unsafe { (self.0.add_item)(reg.reg, &def) })
    }
}

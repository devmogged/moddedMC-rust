//! Items with no block: ingots, alloys, tools and the like.

use crate::{InterfaceUnavailable, RegistrationError, Registry, abi, require_interface, s};

#[derive(Clone, Copy)]
pub struct Items(&'static abi::ItemsV1);

pub fn require() -> Result<Items, InterfaceUnavailable> {
    // SAFETY: this is the published table type for moddedmc:items v1.
    unsafe { require_interface(abi::ITEMS, 1) }.map(Items)
}

pub struct Item<'a> {
    pub id: &'a str,
    pub display_name: &'a str,
    /// The vanilla item it behaves like (stack size, food, tool), such as `minecraft:iron_ingot`.
    pub carrier: &'a str,
    pub model: &'a str,
}

impl Items {
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

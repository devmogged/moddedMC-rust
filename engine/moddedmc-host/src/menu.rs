//! `moddedmc:menus/1`: chest-style inventories a mod fills; the contents go back to the mods when
//! the player closes it.

use std::{
    any::Any,
    cell::Cell,
    io::Cursor,
    sync::{Arc, Mutex},
};

use pumpkin_core::entity::player::Player;
use pumpkin_data::{item::Item, item_stack::ItemStack, screen::WindowType};
use pumpkin_inventory::{
    Clearable, Inventory, SimpleInventory,
    generic_container_screen_handler::GenericContainerScreenHandler,
    player::player_inventory::PlayerInventory,
    screen_handler::{InventoryPlayer, ScreenHandlerFactory, SharedScreenHandler},
};
use pumpkin_nbt::{Nbt, NbtCompound, deserializer::NbtReadHelperJava};
use pumpkin_util::text::TextComponent;

use crate::{HOST, lock, native::Native};

thread_local! {
    /// Set while mods hear about a close, so a mod cannot reopen a screen from inside one.
    static CLOSING: Cell<bool> = const { Cell::new(false) };
}

/// An item's bytes: vanilla's item NBT; empty for an empty slot.
pub fn encode(stack: &ItemStack) -> Vec<u8> {
    if stack.is_empty() {
        return Vec::new();
    }
    let mut compound = NbtCompound::new();
    stack.write_item_stack(&mut compound);
    Nbt::new(String::new(), compound).write().to_vec()
}

pub fn decode(bytes: &[u8]) -> ItemStack {
    if bytes.is_empty() {
        return ItemStack::EMPTY.clone();
    }
    let mut reader = NbtReadHelperJava::new(Cursor::new(bytes));
    Nbt::read(&mut reader)
        .ok()
        .and_then(|nbt| ItemStack::read_item_stack(&nbt.root_tag))
        .unwrap_or_else(|| ItemStack::EMPTY.clone())
}

pub fn item(id: &str, count: u32) -> Option<Vec<u8>> {
    let item = Item::from_registry_key(id.strip_prefix("minecraft:").unwrap_or(id))?;
    let count = u8::try_from(count).ok().filter(|c| (1..=99).contains(c))?;
    Some(encode(&ItemStack::new(count, item)))
}

/// Id and count; count 0 for an empty or unreadable item.
pub fn describe(bytes: &[u8]) -> (String, u32) {
    let stack = decode(bytes);
    if stack.is_empty() {
        return (String::new(), 0);
    }
    (
        format!("minecraft:{}", stack.item.registry_key),
        u32::from(stack.item_count),
    )
}

/// Opens the menu; false if `rows` is out of range, the player is gone, or a close is running.
pub fn open(player: &Player, title: &str, rows: u32, key: &str, items: &[&[u8]]) -> bool {
    let window = match rows {
        1 => WindowType::Generic9x1,
        2 => WindowType::Generic9x2,
        3 => WindowType::Generic9x3,
        4 => WindowType::Generic9x4,
        5 => WindowType::Generic9x5,
        6 => WindowType::Generic9x6,
        _ => return false,
    };
    if CLOSING.get() {
        return false;
    }
    let Some(player) = player.world().get_player_by_uuid(player.gameprofile.id) else {
        return false;
    };
    let size = rows as usize * 9;
    let inventory = MenuInventory {
        slots: SimpleInventory::new(size),
        key: key.to_owned(),
        player: Mutex::new(Some(Arc::clone(&player))),
    };
    for (slot, bytes) in items.iter().take(size).enumerate() {
        inventory.set_stack(slot, decode(bytes));
    }
    let factory = MenuFactory {
        title: TextComponent::text(title.to_owned()),
        window,
        rows: rows as u8,
        inventory: Arc::new(inventory),
    };
    player.open_handled_screen(&factory, None).is_some()
}

struct MenuFactory {
    title: TextComponent,
    window: WindowType,
    rows: u8,
    inventory: Arc<MenuInventory>,
}

impl ScreenHandlerFactory for MenuFactory {
    fn create_screen_handler(
        &self,
        sync_id: u8,
        player_inventory: &Arc<PlayerInventory>,
        player: &dyn InventoryPlayer,
    ) -> Option<SharedScreenHandler> {
        let handler = GenericContainerScreenHandler::new(
            self.window,
            sync_id,
            player_inventory,
            self.inventory.clone(),
            self.rows,
            9,
            player.is_spectator(),
        );
        Some(Arc::new(Mutex::new(handler)))
    }

    fn get_display_name(&self) -> TextComponent {
        self.title.clone()
    }
}

struct MenuInventory {
    slots: SimpleInventory,
    key: String,
    /// Taken on close: the contents go back once, and the player is not kept alive by its menu.
    player: Mutex<Option<Arc<Player>>>,
}

impl Clearable for MenuInventory {
    fn clear(&self) {
        self.slots.clear();
    }
}

impl Inventory for MenuInventory {
    fn size(&self) -> usize {
        self.slots.size()
    }

    fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    fn get_stack(&self, slot: usize) -> ItemStack {
        self.slots.get_stack(slot)
    }

    fn remove_stack(&self, slot: usize) -> ItemStack {
        self.slots.remove_stack(slot)
    }

    fn remove_stack_specific(&self, slot: usize, amount: u8) -> ItemStack {
        self.slots.remove_stack_specific(slot, amount)
    }

    fn set_stack(&self, slot: usize, stack: ItemStack) {
        self.slots.set_stack(slot, stack);
    }

    fn on_close(&self) {
        let Some(player) = lock(&self.player).take() else {
            return;
        };
        let items: Vec<Vec<u8>> = (0..self.size())
            .map(|slot| encode(&self.get_stack(slot)))
            .collect();
        let mods: Vec<Native> = lock(&HOST.mods).iter().map(|m| m.module).collect();
        CLOSING.set(true);
        for module in mods {
            module.menu_closed(&player, &self.key, &items);
        }
        CLOSING.set(false);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn items_round_trip() {
        let described = item("minecraft:diamond", 5).map(|bytes| describe(&bytes));
        assert_eq!(described, Some(("minecraft:diamond".to_owned(), 5)));
        assert_eq!(describe(&[]), (String::new(), 0));
        assert_eq!(describe(b"junk"), (String::new(), 0));
        assert!(item("minecraft:no_such_item", 1).is_none());
        assert!(item("minecraft:diamond", 0).is_none());
    }
}

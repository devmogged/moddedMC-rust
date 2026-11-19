//! `moddedmc:menus/1`: chest-style inventories (see [`crate::abi::MenusV1`]).

use crate::{
    Player,
    abi::{self, MenusV1, Str},
    interface, s, with_sink,
};

/// An item as the server stores it (vanilla item NBT, every component kept); empty for an empty
/// slot. Save the bytes as they are.
pub type Item = Vec<u8>;

/// The menus interface, if the server offers it.
#[must_use]
pub fn get() -> Option<Menus> {
    interface::<MenusV1>(abi::MENUS_V1).map(Menus)
}

#[derive(Clone, Copy)]
pub struct Menus(&'static MenusV1);

impl Menus {
    /// Opens `rows` (1 to 6) rows of 9 slots filled from `items`. When the player closes it,
    /// [`crate::Mod::menu_closed`] gets `key` and the contents: start `key` with your mod id.
    pub fn open(
        &self,
        player: &Player<'_>,
        title: &str,
        rows: u32,
        key: &str,
        items: &[Item],
    ) -> bool {
        let items: Vec<Str> = items.iter().map(|i| Str::new(i)).collect();
        // SAFETY: everything is borrowed for the call.
        unsafe {
            (self.0.open)(
                player.raw,
                s(title),
                rows,
                s(key),
                items.as_ptr(),
                items.len(),
            )
        }
    }

    /// Opens a furnace-like machine screen (`minecraft:furnace`, `minecraft:blast_furnace`,
    /// `minecraft:smoker`): 3 slots (input, fuel, output) filled from `items`, the mod deciding
    /// what they do. Closing it calls [`crate::Mod::menu_closed`] with `key`.
    pub fn open_machine(
        &self,
        player: &Player<'_>,
        kind: &str,
        title: &str,
        key: &str,
        items: &[Item],
    ) -> bool {
        let items: Vec<Str> = items.iter().map(|i| Str::new(i)).collect();
        // SAFETY: everything is borrowed for the call.
        unsafe {
            (self.0.open_machine)(
                player.raw,
                s(kind),
                s(title),
                s(key),
                items.as_ptr(),
                items.len(),
            )
        }
    }

    /// Sets a property of the player's open screen `key`; for a furnace 0 = fuel left, 1 = fuel
    /// total, 2 = progress, 3 = progress total. False if that screen is not open.
    pub fn set_property(&self, player: &Player<'_>, key: &str, property: u32, value: i32) -> bool {
        // SAFETY: borrowed for the call.
        unsafe { (self.0.set_property)(player.raw, s(key), property, value) }
    }

    /// Lets players put only `allowed` items (`minecraft:coal`) into `slot` of their open screen
    /// `key`; an empty list keeps everything out (an output slot). False if it is not open.
    pub fn restrict_slot(
        &self,
        player: &Player<'_>,
        key: &str,
        slot: u32,
        allowed: &[&str],
    ) -> bool {
        let allowed: Vec<Str> = allowed.iter().map(|a| s(a)).collect();
        // SAFETY: everything is borrowed for the call.
        unsafe { (self.0.restrict_slot)(player.raw, s(key), slot, allowed.as_ptr(), allowed.len()) }
    }

    /// `count` of item `id` (`minecraft:diamond`); `None` if the item is unknown.
    #[must_use]
    pub fn item(&self, id: &str, count: u32) -> Option<Item> {
        let mut out = Vec::new();
        // SAFETY: borrowed for the call; the sink is live for it.
        with_sink(
            |sink| unsafe { (self.0.item)(s(id), count, sink) },
            |_, b| out = b.to_vec(),
        )
        .then_some(out)
    }

    /// An item's id and count; `None` for an empty slot.
    #[must_use]
    pub fn describe(&self, item: &[u8]) -> Option<(String, u32)> {
        let mut id = String::new();
        // SAFETY: borrowed for the call; the sink is live for it.
        let count = with_sink(
            |sink| unsafe { (self.0.describe)(Str::new(item), sink) },
            |_, b| id = String::from_utf8_lossy(b).into_owned(),
        );
        (count > 0).then_some((id, count))
    }
}

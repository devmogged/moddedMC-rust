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

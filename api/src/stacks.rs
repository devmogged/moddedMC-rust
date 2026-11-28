//! Item stacks outside inventories: item entities, and stacks a mod keeps itself.
//!
//! A stack is vanilla's saved `ItemStack` NBT (`id`, `count`, `components`) as Java named-root
//! binary, so a mod can store it in its block entity data as is.

use crate::{InterfaceUnavailable, abi, content::World, entities::Entity, require_interface};

pub use abi::{ItemDisplay, MAX_STACK_BYTES, StackInfo};

#[derive(Clone, Copy)]
pub struct ItemStacks(
    &'static abi::ItemStacksV1,
    Option<&'static abi::ItemStacksV1Fuel>,
    Option<&'static abi::ItemStacksV1MaxEnchantment>,
    Option<&'static abi::ItemStacksV1Damage>,
);

pub fn require() -> Result<ItemStacks, InterfaceUnavailable> {
    // SAFETY: this is the full append-only v1 table.
    if let Ok(table) = unsafe { require_interface::<abi::ItemStacksV1Damage>(abi::ITEM_STACKS, 1) }
    {
        return Ok(ItemStacks(
            &table.base.base.base,
            Some(&table.base.base),
            Some(&table.base),
            Some(table),
        ));
    }
    // SAFETY: this is the extended table type for moddedmc:item_stacks v1.
    if let Ok(table) =
        unsafe { require_interface::<abi::ItemStacksV1MaxEnchantment>(abi::ITEM_STACKS, 1) }
    {
        return Ok(ItemStacks(
            &table.base.base,
            Some(&table.base),
            Some(table),
            None,
        ));
    }
    // SAFETY: this is the extended table type for moddedmc:item_stacks v1.
    if let Ok(table) = unsafe { require_interface::<abi::ItemStacksV1Fuel>(abi::ITEM_STACKS, 1) } {
        return Ok(ItemStacks(&table.base, Some(table), None, None));
    }
    // SAFETY: this is the published table type for moddedmc:item_stacks v1.
    unsafe { require_interface(abi::ITEM_STACKS, 1) }
        .map(|table| ItemStacks(table, None, None, None))
}

impl ItemStacks {
    /// Apply vanilla durability wear, including Unbreaking. Empty bytes indicate a broken stack.
    pub fn damage(self, stack: &[u8], amount: i32) -> Option<Vec<u8>> {
        let table = self.3?;
        let mut data = Vec::new();
        let sink = abi::Sink {
            ctx: (&raw mut data).cast(),
            put: crate::content::bytes,
        };
        // SAFETY: stack and sink are borrowed for the call.
        unsafe { (table.damage)(abi::Str::new(stack), amount, sink) }.then_some(data)
    }

    pub fn require_damage(self) -> Result<(), InterfaceUnavailable> {
        self.3.map(|_| ()).ok_or(InterfaceUnavailable {
            name: "moddedmc:item_stacks.damage",
            version: 1,
        })
    }
    /// The stack an item entity carries; `None` for other entities.
    pub fn entity_stack(self, entity: Entity<'_>) -> Option<Vec<u8>> {
        let mut data = Vec::new();
        let sink = abi::Sink {
            ctx: (&raw mut data).cast(),
            put: crate::content::bytes,
        };
        // SAFETY: the entity is live and the sink collects into the local Vec synchronously.
        unsafe { (self.0.entity_stack)(entity.raw(), sink) }.then_some(data)
    }

    /// Replace an item entity's stack; an empty stack discards the entity.
    pub fn set_entity_stack(self, entity: Entity<'_>, stack: &[u8]) -> bool {
        // SAFETY: the entity and bytes are borrowed for the call.
        unsafe { (self.0.set_entity_stack)(entity.raw(), abi::Str::new(stack)) }
    }

    /// Drop `stack` as an item entity moving at `motion`, with the default pickup delay.
    pub fn spawn(
        self,
        world: World<'_>,
        position: [f64; 3],
        motion: [f64; 3],
        stack: &[u8],
    ) -> Option<i32> {
        // SAFETY: the world and bytes are borrowed for the call.
        let id = unsafe {
            (self.0.spawn)(
                world.raw(),
                abi::Vec3(position),
                abi::Vec3(motion),
                abi::Str::new(stack),
            )
        };
        (id != 0).then_some(id)
    }

    /// `None` when the host can't read the stack.
    pub fn info(self, stack: &[u8]) -> Option<StackInfo> {
        let mut info = StackInfo::default();
        // SAFETY: the bytes are borrowed for the call and the output is a local.
        unsafe { (self.0.info)(abi::Str::new(stack), &raw mut info) }.then_some(info)
    }

    /// Requires the functions appended after the first release, in `Mod::start`.
    pub fn require_extension(self, extension: StacksExtension) -> Result<(), InterfaceUnavailable> {
        let available = match extension {
            StacksExtension::Component => extension.available(self.0),
            StacksExtension::Fuel => self.1.is_some(),
        };
        if available {
            Ok(())
        } else {
            Err(InterfaceUnavailable {
                name: match extension {
                    StacksExtension::Component => "moddedmc:item_stacks.component",
                    StacksExtension::Fuel => "moddedmc:item_stacks.fuel",
                },
                version: 1,
            })
        }
    }

    /// The stack's effective component (patch over item defaults) as `{value: <component>}`
    /// named-root NBT, empty for a unit component. `None` when absent or on a server without
    /// `StacksExtension::Component`.
    pub fn component(self, stack: &[u8], component: &str) -> Option<Vec<u8>> {
        if !StacksExtension::Component.available(self.0) {
            return None;
        }
        let mut data = Vec::new();
        let sink = abi::Sink {
            ctx: (&raw mut data).cast(),
            put: crate::content::bytes,
        };
        // SAFETY: the size check covers the appended field; the inputs are borrowed and the
        // sink collects into the local Vec for the call.
        unsafe {
            (self.0.component)(
                abi::Str::new(stack),
                abi::Str::new(component.as_bytes()),
                sink,
            )
        }
        .then_some(data)
    }

    /// Effective regular furnace burn time and the item's crafting remainder.
    pub fn fuel(self, stack: &[u8]) -> Option<(u32, Vec<u8>)> {
        let table = self.1?;
        let mut data = Vec::new();
        let sink = abi::Sink {
            ctx: (&raw mut data).cast(),
            put: crate::content::bytes,
        };
        // SAFETY: the saved stack and sink are valid for this synchronous callback.
        let ticks = unsafe { (table.fuel)(abi::Str::new(stack), sink) };
        (ticks > 0).then_some((ticks, data))
    }

    /// Whether a stack has an applied enchantment at or above its defined maximum.
    /// Returns false when the host lacks the appended operation.
    pub fn has_maxed_enchantment(self, stack: &[u8]) -> bool {
        // SAFETY: the saved stack is valid for the duration of the synchronous callback.
        self.2
            .is_some_and(|table| unsafe { (table.has_maxed_enchantment)(abi::Str::new(stack)) })
    }
}

/// Item stack functions added after the first ABI 1 release.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StacksExtension {
    Component,
    Fuel,
}

impl StacksExtension {
    const fn available(self, api: &abi::ItemStacksV1) -> bool {
        match self {
            Self::Component => {
                api.size >= std::mem::offset_of!(abi::ItemStacksV1, component) + size_of::<usize>()
            }
            Self::Fuel => api.size >= size_of::<abi::ItemStacksV1Fuel>(),
        }
    }
}

#[derive(Clone, Copy)]
pub struct Displays(&'static abi::DisplaysV1);

pub fn require_displays() -> Result<Displays, InterfaceUnavailable> {
    // SAFETY: this is the published table type for moddedmc:displays v1.
    unsafe { require_interface(abi::DISPLAYS, 1) }.map(Displays)
}

impl Displays {
    /// A client-side item display that is never saved. Returns its id.
    pub fn spawn_item(self, world: World<'_>, stack: &[u8], display: &ItemDisplay) -> Option<i32> {
        // SAFETY: the world, bytes and display are borrowed for the call.
        let id = unsafe { (self.0.spawn_item)(world.raw(), abi::Str::new(stack), display) };
        (id != 0).then_some(id)
    }

    /// False when `id` is gone, for example because its chunk unloaded.
    pub fn update(self, world: World<'_>, id: i32, display: &ItemDisplay) -> bool {
        // SAFETY: the world and display are borrowed for the call.
        unsafe { (self.0.update)(world.raw(), id, display) }
    }

    pub fn set_item(self, world: World<'_>, id: i32, stack: &[u8]) -> bool {
        // SAFETY: the world and bytes are borrowed for the call.
        unsafe { (self.0.set_item)(world.raw(), id, abi::Str::new(stack)) }
    }

    pub fn remove(self, world: World<'_>, id: i32) {
        // SAFETY: the world is borrowed for the call.
        unsafe { (self.0.remove)(world.raw(), id) }
    }
}

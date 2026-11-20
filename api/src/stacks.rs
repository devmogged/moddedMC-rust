//! Item stacks outside inventories: item entities, and stacks a mod keeps itself.
//!
//! A stack is vanilla's saved `ItemStack` NBT (`id`, `count`, `components`) as Java named-root
//! binary, so a mod can store it in its block entity data as is.

use crate::{InterfaceUnavailable, abi, content::World, entities::Entity, require_interface};

pub use abi::{ItemDisplay, MAX_STACK_BYTES, StackInfo};

#[derive(Clone, Copy)]
pub struct ItemStacks(&'static abi::ItemStacksV1);

pub fn require() -> Result<ItemStacks, InterfaceUnavailable> {
    // SAFETY: this is the published table type for moddedmc:item_stacks v1.
    unsafe { require_interface(abi::ITEM_STACKS, 1) }.map(ItemStacks)
}

impl ItemStacks {
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
        if extension.available(self.0) {
            Ok(())
        } else {
            Err(InterfaceUnavailable {
                name: "moddedmc:item_stacks.component",
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
}

/// Item stack functions added after the first ABI 1 release.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StacksExtension {
    Component,
}

impl StacksExtension {
    const fn available(self, api: &abi::ItemStacksV1) -> bool {
        match self {
            Self::Component => {
                api.size >= std::mem::offset_of!(abi::ItemStacksV1, component) + size_of::<usize>()
            }
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

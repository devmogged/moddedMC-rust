//! Vanilla block containers as `NeoForge`'s `IItemHandler`, for mods that move items into and out
//! of chests, hoppers, furnaces and the rest. Stacks are saved `ItemStack` NBT.

use crate::{InterfacePrefix, InterfaceUnavailable, Pos, abi, content::World};

pub use abi::{FurnaceHeat, NO_FACE};

#[derive(Clone, Copy)]
pub struct Containers(InterfacePrefix<abi::ContainersV1>);

pub fn require() -> Result<Containers, InterfaceUnavailable> {
    // SAFETY: this is the published table type for moddedmc:containers v1.
    unsafe {
        InterfacePrefix::require(
            abi::CONTAINERS,
            1,
            std::mem::offset_of!(abi::ContainersV1, furnace_heat),
        )
    }
    .map(Containers)
}

/// `Capabilities.ItemHandler.BLOCK` at a position, seen from one side. Each call looks the
/// container up again, so a handle stays correct when the block changes.
#[derive(Clone, Copy)]
pub struct Container<'a> {
    api: *const abi::ContainersV1,
    world: World<'a>,
    position: Pos,
    face: u8,
}

/// Runs a host call that answers through a byte sink.
fn collect(call: impl FnOnce(abi::Sink) -> bool) -> Option<Vec<u8>> {
    let mut data = Vec::new();
    let sink = abi::Sink {
        ctx: (&raw mut data).cast(),
        put: crate::content::bytes,
    };
    call(sink).then_some(data)
}

impl Containers {
    pub fn require_furnace_heat(self) -> Result<(), InterfaceUnavailable> {
        self.0
            .upto(std::mem::offset_of!(abi::ContainersV1, furnace_heat))
            .map(|_| ())
            .ok_or(InterfaceUnavailable {
                name: abi::CONTAINERS,
                version: 1,
            })
    }

    /// Read (both increments zero) or advance the loaded furnace's progress and minimum lit time.
    /// Mutations are accepted only inside this world's serial content tick callback.
    pub fn furnace_heat(
        self,
        world: World<'_>,
        position: Pos,
        boost: u16,
        min_lit: u16,
    ) -> Option<FurnaceHeat> {
        let api = self
            .0
            .upto(std::mem::offset_of!(abi::ContainersV1, furnace_heat))?;
        let mut out = FurnaceHeat::default();
        // SAFETY: the checked extension is present and both borrows live through this call.
        unsafe { ((*api).furnace_heat)(world.raw(), position, boost, min_lit, &mut out) }
            .then_some(out)
    }
    /// The container at `position`, accessed from `face` (vanilla direction order, or `NO_FACE`).
    /// `None` when there isn't one.
    pub fn at(self, world: World<'_>, position: Pos, face: u8) -> Option<Container<'_>> {
        let container = Container {
            api: self
                .0
                .upto(std::mem::offset_of!(abi::ContainersV1, extract))?,
            world,
            position,
            face,
        };
        (container.slots().is_some()).then_some(container)
    }
}

impl Container<'_> {
    /// `getSlots`; `None` once the container is gone.
    pub fn slots(&self) -> Option<u32> {
        // SAFETY: the world is live for the call.
        let slots = unsafe { ((*self.api).slots)(self.world.raw(), self.position, self.face) };
        u32::try_from(slots).ok()
    }

    /// `getStackInSlot`, `None` for an empty slot.
    pub fn stack(&self, slot: u32) -> Option<Vec<u8>> {
        // SAFETY: the world is live and the sink fills a local Vec during the call.
        collect(|out| unsafe {
            ((*self.api).stack)(self.world.raw(), self.position, self.face, slot, out)
        })
    }

    /// `getSlotLimit`.
    pub fn slot_limit(&self, slot: u32) -> u32 {
        // SAFETY: the world is live for the call.
        unsafe { ((*self.api).slot_limit)(self.world.raw(), self.position, self.face, slot) }
    }

    /// `insertItem`: the remainder, empty when everything went in. `None` when the container is
    /// gone or the stack is unreadable.
    pub fn insert(&self, slot: u32, stack: &[u8], simulate: bool) -> Option<Vec<u8>> {
        // SAFETY: the world and stack are borrowed for the call; the sink fills a local Vec.
        collect(|out| unsafe {
            ((*self.api).insert)(
                self.world.raw(),
                self.position,
                self.face,
                slot,
                abi::Str::new(stack),
                simulate,
                out,
            )
        })
    }

    /// `ItemHandlerHelper.insertItemStacked`, with the remainder as for `insert`.
    pub fn insert_stacked(&self, stack: &[u8], simulate: bool) -> Option<Vec<u8>> {
        // SAFETY: the world and stack are borrowed for the call; the sink fills a local Vec.
        collect(|out| unsafe {
            ((*self.api).insert_stacked)(
                self.world.raw(),
                self.position,
                self.face,
                abi::Str::new(stack),
                simulate,
                out,
            )
        })
    }

    /// `extractItem`: the stack taken, `None` when nothing was.
    pub fn extract(&self, slot: u32, amount: u32, simulate: bool) -> Option<Vec<u8>> {
        // SAFETY: the world is live and the sink fills a local Vec during the call.
        collect(|out| unsafe {
            ((*self.api).extract)(
                self.world.raw(),
                self.position,
                self.face,
                slot,
                amount,
                simulate,
                out,
            )
        })
    }
}

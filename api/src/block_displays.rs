//! Block displays for blocks that aren't in the world, such as a moving contraption.

use crate::{InterfacePrefix, InterfaceUnavailable, abi, content::World};

pub use abi::{BlockDisplay, BlockDisplayClick};

#[derive(Clone, Copy)]
pub struct BlockDisplays(InterfacePrefix<abi::BlockDisplaysV1Use>);

pub fn require() -> Result<BlockDisplays, InterfaceUnavailable> {
    // SAFETY: the base table is the ABI 1 prefix. Extensions check their fields separately.
    unsafe { InterfacePrefix::require(abi::BLOCK_DISPLAYS, 1, size_of::<abi::BlockDisplaysV1>()) }
        .map(BlockDisplays)
}

impl BlockDisplays {
    /// A display of a block state id; `None` when nothing spawned.
    pub fn spawn(self, world: World<'_>, state: u32, display: &BlockDisplay) -> Option<i32> {
        // SAFETY: the world and display are borrowed for the call.
        let id =
            unsafe { ((&raw const (*self.0.ptr).base.spawn).read())(world.raw(), state, display) };
        (id != 0).then_some(id)
    }

    /// False once the display is gone; spawn a new one then.
    pub fn update(self, world: World<'_>, id: i32, display: &BlockDisplay) -> bool {
        // SAFETY: the world and display are borrowed for the call.
        unsafe { ((&raw const (*self.0.ptr).base.update).read())(world.raw(), id, display) }
    }

    pub fn set_state(self, world: World<'_>, id: i32, state: u32) -> bool {
        // SAFETY: the world is borrowed for the call.
        unsafe { ((&raw const (*self.0.ptr).base.set_state).read())(world.raw(), id, state) }
    }

    pub fn enable_use(self, world: World<'_>, id: i32) -> bool {
        let Some(table) = self
            .0
            .upto(std::mem::offset_of!(abi::BlockDisplaysV1Use, enable_use))
        else {
            return false;
        };
        // SAFETY: the checked table includes the field and the world is borrowed.
        unsafe { ((&raw const (*table).enable_use).read())(world.raw(), id) }
    }

    pub fn next_click(self, world: World<'_>, id: i32) -> Option<BlockDisplayClick> {
        let table = self
            .0
            .upto(std::mem::offset_of!(abi::BlockDisplaysV1Use, next_click))?;
        let mut click = BlockDisplayClick::default();
        // SAFETY: the checked table includes the field. The world and output are borrowed.
        unsafe { ((&raw const (*table).next_click).read())(world.raw(), id, &raw mut click) }
            .then_some(click)
    }

    pub fn remove(self, world: World<'_>, id: i32) {
        // SAFETY: the world is borrowed for the call.
        unsafe { ((&raw const (*self.0.ptr).base.remove).read())(world.raw(), id) }
    }
}

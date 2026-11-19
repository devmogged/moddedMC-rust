//! Block displays for blocks that aren't in the world, such as a moving contraption.

use crate::{InterfaceUnavailable, abi, content::World, require_interface};

pub use abi::BlockDisplay;

#[derive(Clone, Copy)]
pub struct BlockDisplays(&'static abi::BlockDisplaysV1);

pub fn require() -> Result<BlockDisplays, InterfaceUnavailable> {
    // SAFETY: this is the published table type for moddedmc:block_displays v1.
    unsafe { require_interface(abi::BLOCK_DISPLAYS, 1) }.map(BlockDisplays)
}

impl BlockDisplays {
    /// A display of a block state id; `None` when nothing spawned.
    pub fn spawn(self, world: World<'_>, state: u32, display: &BlockDisplay) -> Option<i32> {
        // SAFETY: the world and display are borrowed for the call.
        let id = unsafe { (self.0.spawn)(world.raw(), state, display) };
        (id != 0).then_some(id)
    }

    /// False once the display is gone; spawn a new one then.
    pub fn update(self, world: World<'_>, id: i32, display: &BlockDisplay) -> bool {
        // SAFETY: the world and display are borrowed for the call.
        unsafe { (self.0.update)(world.raw(), id, display) }
    }

    pub fn set_state(self, world: World<'_>, id: i32, state: u32) -> bool {
        // SAFETY: the world is borrowed for the call.
        unsafe { (self.0.set_state)(world.raw(), id, state) }
    }

    pub fn remove(self, world: World<'_>, id: i32) {
        // SAFETY: the world is borrowed for the call.
        unsafe { (self.0.remove)(world.raw(), id) }
    }
}
